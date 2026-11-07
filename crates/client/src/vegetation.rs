//! Procedural low-poly vegetation chosen by climate, the way it grows on Earth:
//! spruce taiga in the north (snow-laden where it's cold), mixed and broadleaf
//! forest in temperate zones, rainforest at the equator, acacia savanna, cacti in
//! hot deserts, dwarf shrubs on the tundra, palms only at oases and on tropical
//! beaches.
//!
//! For speed, all plants in an area are merged into one mesh, with a simplified
//! version of the same area shown at a distance.

use crate::layout::tile_center;
use crate::lod::Lod;
use crate::loading::AppState;
use crate::terrain::{height::{HeightField, WATER_LEVEL}, texture::treeline_height};
use crate::wrap::Wrapped;
use crate::procmesh::Model;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sim::climate;
use sim::map::{Elevation, Map, Tile};
use sim::rules::Rules;
use std::collections::HashMap;

/// World units per side of one merged vegetation area.
const AREA: f32 = 9.0;
/// Camera distance at which plants switch to their simplified meshes.
const LOD_DISTANCE: f32 = 48.0;

pub struct VegetationPlugin;

pub type VegetationMaterial = ExtendedMaterial<StandardMaterial, VegetationExtension>;

/// The plants' material, shared by towns and improvements so snow settles on
/// them alike (see `snow_tag`).
#[derive(Resource)]
pub struct Scenery(pub Handle<VegetationMaterial>);

/// How a surface takes snow and the seasons, for the plants' shader.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// Trunks, walls: no snow.
    Bare = 0,
    /// Conifers and other evergreen foliage: snow all over.
    Evergreen = 1,
    /// Broadleaf crowns: autumn colours, bare in winter.
    Deciduous = 2,
    /// Roofs, fields, paving: snow on faces turned to the sky.
    Roof = 3,
}

/// The vertex alpha for a surface standing on ground at height `ground`
/// (-1..4): the shader reads the kind from the quarter and the ground height
/// (for the temperature, and so the snow) from within it.
pub fn snow_tag(surface: Surface, ground: f32) -> f32 {
    (surface as u8 as f32 + ((ground + 1.0) / 5.0).clamp(0.0, 0.999)) / 4.0
}

#[derive(Clone, Copy, ShaderType, Debug)]
struct VegetationParams {
    fog_area: Vec4,
    /// x: the season for snow (see `season::Season::for_snow`), y: whether
    /// it is warming (positive) or cooling (`Season::trend`).
    season: Vec4,
}

/// Adds fog of war, weather and snow to the plants' standard material.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct VegetationExtension {
    #[uniform(100)]
    params: VegetationParams,
    #[texture(101)]
    #[sampler(102)]
    fog: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    weather: Handle<Image>,
    /// The terrain's climate texture (snow line and frost), so snow on the
    /// plants matches the ground's.
    #[texture(105)]
    #[sampler(106)]
    climate: Handle<Image>,
}

impl MaterialExtension for VegetationExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://client/shaders/vegetation.wesl".into()
    }
}

fn follow_season(season: Res<crate::season::Season>, mut materials: ResMut<Assets<VegetationMaterial>>) {
    for (_, m) in materials.iter_mut() {
        m.extension.params.season.x = season.for_snow();
        m.extension.params.season.y = season.trend();
    }
}

/// Number of plants placed, for the debug screen.
#[derive(Resource)]
pub struct TreeCount(pub usize);

impl Plugin for VegetationPlugin {
    fn build(&self, app: &mut App) {
        crate::embed_vegetation_shader(app);
        app.add_plugins(MaterialPlugin::<VegetationMaterial>::default())
            .add_systems(OnEnter(AppState::InGame), spawn.after(crate::terrain::spawn))
            .add_systems(Update, clear_for_cities.after(crate::play::PlaySet).after(crate::terraform::ReshapeSet).in_set(crate::loading::InGameSet))
            .add_systems(Update, follow_season.run_if(resource_changed::<crate::season::Season>));
    }
}

const ALL_SPECIES: [Species; 12] = [
    Species::Spruce,
    Species::Pine,
    Species::Birch,
    Species::Oak,
    Species::TropicalTree,
    Species::RainforestGiant,
    Species::Acacia,
    Species::Palm,
    Species::Cactus,
    Species::Shrub,
    Species::DwarfShrub,
    Species::Boulder,
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Species {
    Spruce,
    Pine,
    Birch,
    Oak,
    TropicalTree,
    RainforestGiant,
    Acacia,
    Palm,
    Cactus,
    Shrub,
    DwarfShrub,
    Boulder,
}

/// Detailed (near) and simplified (far) models of a species, made in
/// Blender (`art/models/trees.py`). Foliage has alpha 0, wood and rock 1.
fn models(species: Species) -> (Model, Model) {
    let name = match species {
        Species::Spruce => "spruce",
        Species::Pine => "pine",
        Species::Birch => "birch",
        Species::Oak => "oak",
        Species::TropicalTree => "tropical",
        Species::RainforestGiant => "giant",
        Species::Acacia => "acacia",
        Species::Palm => "palm",
        Species::Cactus => "cactus",
        Species::Shrub => "shrub",
        Species::DwarfShrub => "dwarf_shrub",
        Species::Boulder => "boulder",
    };
    (crate::models::prop(name).clone(), crate::models::prop(&format!("{name}_far")).clone())
}

/// What grows on a tile: (number of plants, species weights).
fn planting(tile: &Tile, rules: &Rules, coastal: bool, rng: &mut impl Rng) -> (usize, Vec<(Species, f32)>) {
    use Species::*;
    let t = tile.temperature;
    let m = tile.moisture;
    let terrain = rules.terrain(tile.terrain).id.as_str();
    let feature = tile.feature.map(|f| rules.feature(f).id.as_str());
    let few = |rng: &mut dyn rand::RngCore, max: usize| rng.gen_range(0..=max);
    match (feature, terrain) {
        // Taiga: dense spruce in the north, with pine and birch towards its south.
        (Some("forest"), _) if t < 4.0 => (34, vec![(Spruce, 0.85), (Birch, 0.15)]),
        (Some("forest"), _) if t < climate::BOREAL => (32, vec![(Spruce, 0.55), (Pine, 0.2), (Birch, 0.25)]),
        (Some("forest"), _) if t < 15.0 => (30, vec![(Oak, 0.6), (Pine, 0.15), (Spruce, 0.15), (Birch, 0.1)]),
        (Some("forest"), _) if t < climate::RAINFOREST => (30, vec![(Oak, 0.85), (Pine, 0.15)]),
        (Some("forest"), _) => (30, vec![(TropicalTree, 1.0)]),
        (Some("jungle"), _) => (42, vec![(RainforestGiant, 0.55), (TropicalTree, 0.45)]),
        (Some("oasis"), _) => (7, vec![(Palm, 1.0)]),
        (Some("marsh"), _) => (6, vec![(Shrub, 0.6), (if t < climate::BOREAL { Birch } else { Oak }, 0.4)]),
        // Tropical beaches get a few palms.
        (None, "grassland" | "plains" | "desert") if coastal && t > 22.0 && m > 0.3 && rng.gen_bool(0.6) => {
            (few(rng, 3), vec![(Palm, 1.0)])
        }
        (None, "grassland") if t < climate::BOREAL => (few(rng, 4), vec![(Spruce, 0.6), (Birch, 0.4)]),
        (None, "grassland") if t < climate::SUBTROPICAL + 1.0 => (few(rng, 3), vec![(Oak, 0.7), (Shrub, 0.3)]),
        (None, "grassland") => (few(rng, 3), vec![(TropicalTree, 0.6), (Shrub, 0.4)]),
        (None, "plains") if t < climate::BOREAL => (few(rng, 3), vec![(Shrub, 0.6), (Spruce, 0.4)]),
        (None, "plains") if t < climate::SUBTROPICAL + 1.0 => (few(rng, 4), vec![(Shrub, 0.8), (Oak, 0.2)]),
        (None, "plains") => (few(rng, 4), vec![(Acacia, 0.6), (Shrub, 0.4)]),
        // Forest-tundra: stunted spruce scattered among shrubs near the tree line.
        (None, "tundra") if t > climate::TUNDRA - 3.0 => {
            (few(rng, 6), vec![(DwarfShrub, 0.55), (Boulder, 0.2), (Spruce, 0.25)])
        }
        (None, "tundra") => (few(rng, 5), vec![(DwarfShrub, 0.5), (Boulder, 0.5)]),
        (None, "desert") if t >= 18.0 => (few(rng, 3), vec![(Cactus, 0.4), (Shrub, 0.3), (Boulder, 0.3)]),
        (None, "desert") => (few(rng, 3), vec![(Shrub, 0.5), (Boulder, 0.5)]),
        _ => (0, vec![]),
    }
}

fn pick(weights: &[(Species, f32)], rng: &mut impl Rng) -> Species {
    let total: f32 = weights.iter().map(|w| w.1).sum();
    let mut r = rng.gen_range(0.0..total);
    for &(s, w) in weights {
        if r < w {
            return s;
        }
        r -= w;
    }
    weights[weights.len() - 1].0
}

type Plant = (Species, Transform, Vec4);
type AreaKey = (i32, i32);

/// Vegetation built off the main thread: one near and one far mesh per area.
#[derive(Resource)]
pub struct VegetationBuild {
    areas: Vec<(AreaKey, Mesh, Mesh, Vec3)>,
    plants: HashMap<AreaKey, Vec<Plant>>,
    count: usize,
}

/// Plants kept after spawning, so areas can be rebuilt (e.g. cleared for a city).
#[derive(Resource)]
struct VegetationState {
    library: HashMap<Species, (Model, Model)>,
    plants: HashMap<AreaKey, Vec<Plant>>,
}

/// One area's merged plant mesh (near or far level of detail).
#[derive(Component)]
struct VegetationArea {
    key: AreaKey,
    near: bool,
}

fn area_key(p: Vec3) -> AreaKey {
    ((p.x / AREA).floor() as i32, (p.z / AREA).floor() as i32)
}

fn merge_area(library: &HashMap<Species, (Model, Model)>, plants: &[Plant]) -> (Mesh, Mesh) {
    let (mut near, mut far) = (Model::default(), Model::default());
    for (species, transform, tint) in plants {
        let (near_model, far_model) = &library[species];
        // Foliage was marked with alpha 0 (see `models`); the rest is wood.
        let ground = transform.translation.y;
        let leaves = if matches!(species, Species::Birch | Species::Oak) { Surface::Deciduous } else { Surface::Evergreen };
        let encode = |a: f32| snow_tag(if a < 0.5 { leaves } else { Surface::Bare }, ground);
        let (n, f) = (near.vertex_count(), far.vertex_count());
        near.append(near_model, transform, *tint);
        far.append(far_model, transform, *tint);
        near.map_alpha_from(n, encode);
        far.map_alpha_from(f, encode);
    }
    (near.into_mesh(), far.into_mesh())
}

/// Places all plants and merges them into area meshes. Pure CPU work, safe to
/// run on any thread.
pub fn build(map: &Map, rules: &Rules, hf: &HeightField) -> VegetationBuild {
    use rayon::prelude::*;
    let library: HashMap<Species, (Model, Model)> = ALL_SPECIES.iter().map(|&sp| (sp, models(sp))).collect();
    let coast = rules.terrain_id("coast");

    // Plant every tile in parallel. Each tile has its own random generator, so the
    // result doesn't depend on thread scheduling.
    let plants: Vec<Plant> = (0..map.tiles.len() as u32)
        .into_par_iter()
        .flat_map_iter(|id| {
            let mut rng = ChaCha8Rng::seed_from_u64(map.seed ^ 0x7EE5 ^ (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            plant_tile(map, rules, hf, id, coast, &mut rng)
        })
        .collect();
    let count = plants.len();

    // Merge plants area by area, also in parallel.
    let mut by_area: HashMap<AreaKey, Vec<Plant>> = HashMap::new();
    for plant in plants {
        by_area.entry(area_key(plant.1.translation)).or_default().push(plant);
    }
    let areas: Vec<(AreaKey, Mesh, Mesh, Vec3)> = by_area
        .par_iter()
        .map(|(&(ax, az), members)| {
            let (near, far) = merge_area(&library, members);
            let mid = Vec2::new(ax as f32 + 0.5, az as f32 + 0.5) * AREA;
            ((ax, az), near, far, Vec3::new(mid.x, hf.sample(mid).max(0.0), mid.y))
        })
        .collect();
    info!("vegetation: {count} plants in {} areas", areas.len());
    VegetationBuild { areas, plants: by_area, count }
}

/// The plants on one tile: species, placement and colour tint.
fn plant_tile(
    map: &Map,
    rules: &Rules,
    hf: &HeightField,
    id: u32,
    coast: Option<sim::rules::TerrainId>,
    rng: &mut ChaCha8Rng,
) -> Vec<(Species, Transform, Vec4)> {
    let tile = map.tile(id);
    if rules.terrain(tile.terrain).water {
        return Vec::new();
    }
    let coastal = map.grid.neighbors(id).any(|nb| Some(map.tile(nb).terrain) == coast);
    let (mut n, mut weights) = planting(tile, rules, coastal, rng);
    // Mountains are 13 °C colder than the lowlands around them (see mapgen).
    let sea_temp = tile.temperature + if tile.elevation == Elevation::Mountains { 13.0 } else { 0.0 };
    if tile.elevation == Elevation::Mountains {
        // Forests climb the slopes up to the tree line: conifers in cold and
        // temperate ranges, broadleaf cloud forest in the tropics, scrub where dry.
        n = (40.0 * (0.3 + tile.moisture)).min(42.0) as usize;
        weights = if tile.moisture < 0.25 {
            vec![(Species::Shrub, 0.7), (Species::Boulder, 0.3)]
        } else if sea_temp < 4.0 {
            vec![(Species::Spruce, 1.0)]
        } else if sea_temp < 20.0 {
            vec![(Species::Spruce, 0.65), (Species::Pine, 0.35)]
        } else {
            vec![(Species::TropicalTree, 0.7), (Species::RainforestGiant, 0.3)]
        };
    }
    let tree_line = treeline_height(sea_temp);
    let center = tile_center(&map.grid, id);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let offset = loop {
            let v = Vec2::new(rng.gen_range(-0.9..0.9), rng.gen_range(-0.9..0.9));
            if v.length() < 0.9 {
                break v;
            }
        };
        let p = center + offset;
        let h = hf.sample(p);
        let slope = hf.gradient(p).length();
        // Not in water, on cliffs, or above the tree line on mountains (boulders
        // and shrubs may go a little higher).
        let limit = tree_line + if weights.iter().any(|w| w.0 == Species::Boulder) { 0.4 } else { 0.0 };
        let max_slope = if tile.elevation == Elevation::Mountains { 1.8 } else { 1.1 };
        if h < WATER_LEVEL + 0.04 || slope > max_slope || (tile.elevation == Elevation::Mountains && h > limit) {
            continue;
        }
        let species = pick(&weights, rng);
        let scale = rng.gen_range(0.85..1.3) * if tile.elevation == Elevation::Mountains { 0.8 } else { 1.0 };
        let transform = Transform::from_xyz(p.x, h - 0.01, p.y)
            .with_rotation(Quat::from_rotation_y(rng.gen_range(0.0..std::f32::consts::TAU)))
            .with_scale(Vec3::splat(scale));
        let shade = rng.gen_range(0.85..1.12);
        let tint = Vec4::new(shade * rng.gen_range(0.95..1.05), shade, shade * rng.gen_range(0.95..1.05), 1.0);
        out.push((species, transform, tint));
    }
    out
}

/// Uploads the plant meshes and spawns them (main thread, fast).
fn spawn(
    mut commands: Commands,
    mut build: ResMut<VegetationBuild>,
    game: Res<crate::GameMap>,
    fog: Res<crate::fog::FogOfWar>,
    weather: Res<crate::weather::WeatherMap>,
    climate: Res<crate::terrain::ClimateMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<VegetationMaterial>>,
) {
    let material = materials.add(VegetationMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.85,
            ..default()
        },
        extension: VegetationExtension {
            params: VegetationParams { fog_area: crate::fog::params(&game.map.grid), season: Vec4::ZERO },
            fog: fog.image.clone(),
            weather: weather.image.clone(),
            climate: climate.0.clone(),
        },
    });
    commands.insert_resource(Scenery(material.clone()));
    for (key, near, far, center) in std::mem::take(&mut build.areas) {
        for (mesh, is_near) in [(near, true), (far, false)] {
            let entity = commands.spawn((
                VegetationArea { key, near: is_near },
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                Wrapped::mesh(center.x),
                Lod { center, near: is_near, switch: LOD_DISTANCE },
                // Plants in unexplored land aren't drawn at all.
                crate::fog::Footprint {
                    min: Vec2::new(key.0 as f32, key.1 as f32) * AREA,
                    max: Vec2::new(key.0 as f32 + 1.0, key.1 as f32 + 1.0) * AREA,
                    hide: true,
                },
                crate::lod::Dormant,
            )).id();
            // Distant trees are too small to cast visible shadows; skipping them
            // saves a large part of the shadow pass when zoomed out.
            if !is_near {
                commands.entity(entity).insert(bevy::light::NotShadowCaster);
            }
        }
    }
    commands.insert_resource(TreeCount(build.count));
    commands.insert_resource(VegetationState {
        library: ALL_SPECIES.iter().map(|&sp| (sp, models(sp))).collect(),
        plants: std::mem::take(&mut build.plants),
    });
    commands.remove_resource::<VegetationBuild>();
}

/// Radius around a city centre that is cleared of plants.
const CITY_CLEARING: f32 = 1.05;
/// Radius cleared on an improved tile: all of it when its forest (or other
/// feature) was cleared or for a tunnel, else just room for the
/// improvement's buildings.
const CLEARED_TILE: f32 = 0.88;
const IMPROVEMENT_CLEARING: f32 = 0.4;

/// Removes plants where cities are founded or land is improved, and
/// rebuilds the affected areas.
fn clear_for_cities(
    mut events: MessageReader<crate::play::GameEvent>,
    session: Res<crate::play::Session>,
    state: Option<ResMut<VegetationState>>,
    areas: Query<(&VegetationArea, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
    terrain: Option<Res<crate::terrain::Terrain>>,
) {
    let Some(mut state) = state else { return };
    let mut dirty = Vec::new();
    let mut dug = Vec::new();
    // For a new or loaded game, clear around every existing city; afterwards only new ones.
    let (mut founded, mut improved): (Vec<sim::city::CityId>, Vec<sim::hex::TileId>) = (Vec::new(), Vec::new());
    for crate::play::GameEvent(e) in events.read() {
        match e {
            sim::game::Event::CityFounded { city } => founded.push(*city),
            sim::game::Event::TileImproved { tile } => improved.push(*tile),
            sim::game::Event::TerrainChanged { tile } => dug.push(*tile),
            _ => {}
        }
    }
    if session.is_added() {
        founded = session.game.cities.keys().copied().collect();
        let g = &session.game;
        improved = (0..g.map.tiles.len() as sim::hex::TileId).filter(|&t| g.improvement_at(t).is_some() || g.has_tunnel(t)).collect();
    }
    let game = &session.game;
    let mut clearings: Vec<(Vec2, f32)> =
        founded.iter().filter_map(|c| game.cities.get(c)).map(|c| (crate::layout::tile_center(&game.map.grid, c.tile), CITY_CLEARING)).collect();
    for &t in &improved {
        // Tunnel portals take the whole tile.
        let passage = game.has_tunnel(t);
        if game.improvement_at(t).is_none() && !passage {
            continue;
        }
        let radius = if game.map.tile(t).feature.is_none() || passage { CLEARED_TILE } else { IMPROVEMENT_CLEARING };
        clearings.push((crate::layout::tile_center(&game.map.grid, t), radius));
    }
    // Land dug out into water: plants go where it's under water now, and
    // the rest stand on the new ground (see `terraform.rs`).
    if let Some(terrain) = terrain.as_ref().filter(|_| !dug.is_empty()) {
        let hf = &terrain.heights;
        for &t in &dug {
            let centre = crate::layout::tile_center(&game.map.grid, t);
            let reach = (crate::terraform::REACH / AREA).ceil() as i32 + 1;
            let home = area_key(Vec3::new(centre.x, 0.0, centre.y));
            for dx in -reach..=reach {
                for dz in -reach..=reach {
                    let key = (home.0 + dx, home.1 + dz);
                    let Some(plants) = state.plants.get_mut(&key) else { continue };
                    let before = plants.clone();
                    plants.retain_mut(|(_, tr, _)| {
                        let p = tr.translation.xz();
                        if (p - centre).abs().max_element() > crate::terraform::REACH {
                            return true;
                        }
                        let h = hf.sample(p);
                        tr.translation.y = h - 0.01;
                        h > crate::terrain::height::WATER_LEVEL + 0.03
                    });
                    if *plants != before && !dirty.contains(&key) {
                        dirty.push(key);
                    }
                }
            }
        }
    }
    for (centre, radius) in clearings {
        let reach = (radius / AREA).ceil() as i32 + 1;
        let home = area_key(Vec3::new(centre.x, 0.0, centre.y));
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                let key = (home.0 + dx, home.1 + dz);
                if let Some(plants) = state.plants.get_mut(&key) {
                    let before = plants.len();
                    plants.retain(|(_, t, _)| t.translation.xz().distance(centre) > radius);
                    if plants.len() != before && !dirty.contains(&key) {
                        dirty.push(key);
                    }
                }
            }
        }
    }
    for key in dirty {
        let (near, far) = merge_area(&state.library, &state.plants[&key]);
        let mut fresh = [Some(near), Some(far)];
        for (area, mesh) in &areas {
            if area.key == key {
                if let (Some(m), Some(mut asset)) = (fresh[if area.near { 0 } else { 1 }].take(), meshes.get_mut(&mesh.0)) {
                    *asset = m;
                }
            }
        }
    }
}
