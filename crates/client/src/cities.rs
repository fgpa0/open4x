//! City rendering: procedural towns that grow with population, floating city
//! banners, and border lines in each player's colour.

use crate::camera::CameraRig;
use crate::daylight::WindowGlow;
use crate::vegetation::{snow_tag, Scenery, Surface};
use crate::layout::tile_center;
use crate::loading::InGameSet;
use crate::overlay::{corner, Ribbons, XRay, XRayMaterial};
use crate::play::{GameEvent, SelectedCity, Session};
use crate::procmesh::{lin, Model};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::icons::{Icon, Icons};
use crate::ui::{self, palette, Tooltip};
use crate::wrap::{nearest_shift, WrapPeriod, Wrapped};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate, Button};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sim::city::{growth_threshold, Build, CityId};
use sim::combat::CITY_MAX_HP;
use sim::game::{Event, PlayerId};
use sim::hex::TileId;
use std::collections::HashMap;

pub struct CitiesPlugin;

impl Plugin for CitiesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_cities, update_banners, rebuild_borders, draw_worked_tiles)
                .after(crate::play::PlaySet)
                .in_set(InGameSet),
        );
    }
}

/// A rendered city. `look` is what the model was built for; when the city
/// changes (grows, builds walls, changes hands) the model is rebuilt.
#[derive(Component)]
struct CityView {
    id: CityId,
    look: (i32, bool, PlayerId, sim::rules::EraId),
    /// The lit windows (a child with the glowing material).
    windows: Handle<Mesh>,
}

#[derive(Component)]
struct CityBanner {
    city: CityId,
    /// The owner it was made for (it is replaced when the city is captured).
    owner: PlayerId,
}

/// Border line meshes (rebuilt whenever borders or exploration change).
#[derive(Component, Clone)]
struct BorderMesh;

/// How a town is built in each era (by the owner's era): its centre and
/// the centre's size, houses, wall stretch and towers.
struct TownStyle {
    centre: (&'static str, f32),
    houses: &'static [&'static str],
    wall: &'static str,
    tower: &'static str,
}

fn town_style(era: sim::rules::EraId) -> TownStyle {
    match era {
        0 => TownStyle { centre: ("ancient_hall", 1.0), houses: &["ancient_roundhouse", "ancient_hut", "ancient_longhouse"], wall: "palisade", tower: "palisade_tower" },
        1 => TownStyle { centre: ("temple", 0.85), houses: &["classical_house", "classical_villa", "classical_insula"], wall: "classical_wall", tower: "classical_tower" },
        2 => TownStyle { centre: ("keep", 1.0), houses: &["house_cottage", "house_town", "house_long", "house_stone", "house_workshop"], wall: "wall", tower: "tower" },
        3 => TownStyle { centre: ("palace", 0.62), houses: &["ren_townhouse", "ren_brick", "ren_tall", "house_stone", "house_town"], wall: "rampart", tower: "bastion" },
        4 => TownStyle { centre: ("town_hall", 0.7), houses: &["ind_terrace", "ind_terrace_long", "ind_tenement", "ind_mill", "ren_brick"], wall: "rampart", tower: "bastion" },
        _ => TownStyle {
            centre: ("civic_tower", 0.62),
            houses: &["mod_apartment", "mod_block", "mod_office", "mod_shops", "mod_house", "mod_house", "ind_tenement"],
            wall: "rampart",
            tower: "bastion",
        },
    }
}

/// A town around the city centre in the style of its owner's era (huts and
/// a palisade, a temple and tiled houses, a keep and timbered houses, a
/// palace and tall townhouses, a town hall with brick terraces and mills,
/// a civic tower among apartment blocks, offices and shops): roofs or awnings in the owner's colour,
/// more houses as the city grows, walls if built. Made in Blender
/// (`art/models/town.py`). Built in world space relative
/// to the centre so houses sit on the terrain. The second model is the
/// windows that light up at night (others stay dark).
fn town_model(terrain: &Terrain, centre: Vec2, seed: u64, population: i32, color: Vec4, walls: bool, era: sim::rules::EraId) -> (Model, Model, TownSpots) {
    let style = town_style(era);
    use crate::models::{prop, Flag};
    let mut m = Model::default();
    let mut lit = Model::default();
    let mut spots = TownSpots::default();
    let ground = |p: Vec2| terrain.heights.sample(centre + p).max(WATER_LEVEL);
    let tag = snow_tag(Surface::Roof, ground(Vec2::ZERO));
    let base = ground(Vec2::ZERO);
    // Buildings are drawn oversized (like units) but placed at true positions
    // on the terrain: heights and sizes are scaled, positions are not.
    const K: f32 = 3.0;
    let at_ground = |p: Vec2, yaw: f32, size: Vec3| Transform::from_xyz(p.x, ground(p) - base, p.y).with_rotation(Quat::from_rotation_y(yaw)).with_scale(size * K);
    let glass = lin(40, 34, 30);
    // Roofs take the owner's colour; windows go to the lit model or stay dark.
    let mut place = |name: &str, t: Transform, roof: Vec4, light: bool| {
        let model = prop(name);
        m.append_with(model, &t, |c| match Flag::of(c.w) {
            Flag::Team => Some((c * roof).with_w(1.0)),
            Flag::Window if light => None,
            Flag::Window => Some(glass.with_w(1.0)),
            _ => Some(c.with_w(1.0)),
        });
        if light {
            lit.append_with(model, &t, |c| (Flag::of(c.w) == Flag::Window).then_some(glass));
        }
    };
    let roof = color * Vec4::new(0.85, 0.85, 0.85, 1.0);
    place(style.centre.0, at_ground(Vec2::ZERO, 0.0, Vec3::splat(style.centre.1)), roof, true);

    // Houses on rings around the keep; more and further out as the city grows.
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let kinds = style.houses;
    let houses = (4 + population * 2).min(22);
    let mut placed: Vec<Vec2> = vec![Vec2::ZERO];
    let mut tries = 0;
    while (placed.len() as i32) <= houses && tries < 400 {
        tries += 1;
        let spread = 0.45 + 0.42 * (placed.len() as f32 / 22.0).sqrt();
        let a = rng.gen_range(0.0..std::f32::consts::TAU);
        let r = rng.gen_range(0.33..spread);
        let p = Vec2::new(a.cos(), a.sin()) * r;
        if placed.iter().any(|q| q.distance(p) < 0.27) || terrain.heights.sample(centre + p) < WATER_LEVEL + 0.02 {
            continue;
        }
        placed.push(p);
        let kind = kinds[rng.gen_range(0..kinds.len())];
        let yaw = rng.gen_range(0.0..std::f32::consts::PI);
        let size = Vec3::splat(rng.gen_range(0.9..1.1));
        let light = rng.gen_bool(0.65);
        let t = at_ground(p, yaw, size);
        place(kind, t, roof * rng.gen_range(0.85..1.05), light);
        // Smoke rises from about every other house.
        if placed.len().is_multiple_of(2) {
            spots.chimneys.push(Vec3::new(p.x, t.translation.y + crate::models::prop(kind).top() * t.scale.y * 0.95, p.y));
        }
    }

    if walls {
        let radius = 0.95;
        for i in 0..18 {
            let a0 = i as f32 / 18.0 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / 18.0 * std::f32::consts::TAU;
            let (p0, p1) = (Vec2::new(a0.cos(), a0.sin()) * radius, Vec2::new(a1.cos(), a1.sin()) * radius);
            let mid = (p0 + p1) * 0.5;
            let yaw = -(p1 - p0).y.atan2((p1 - p0).x);
            // The wall model is 10 m (0.1 units) long; stretch it to the gap.
            let stretch = p0.distance(p1) / (0.1 * K);
            place(style.wall, at_ground(mid, yaw, Vec3::new(stretch * 1.05, 1.0, 1.0)), roof, false);
            if i % 3 == 0 {
                let t = at_ground(p0, -(p1 - p0).y.atan2((p1 - p0).x), Vec3::ONE);
                place(style.tower, t, roof, true);
                spots.torches.push(Vec3::new(p0.x * 1.04, t.translation.y + crate::models::prop(style.tower).top() * K * 1.05, p0.y * 1.04));
            }
        }
    }
    // Snow settles on roofs and in the streets (see the plants' shader).
    m.map_alpha_from(0, |_| tag);
    let centre_top = crate::models::prop(style.centre.0).top() * K * style.centre.1;
    spots.chimneys.push(Vec3::new(0.05, centre_top * 0.9, 0.03));
    (m, lit, spots)
}

/// Where a town's smoke rises and its torches burn, relative to its centre
/// on the ground (see `ambience::towns`).
#[derive(Component, Default, Clone)]
pub struct TownSpots {
    pub chimneys: Vec<Vec3>,
    pub torches: Vec<Vec3>,
}

fn team(color: [u8; 3]) -> Vec4 {
    lin(color[0], color[1], color[2])
}

fn sync_cities(
    mut commands: Commands,
    session: Res<Session>,
    terrain: Res<Terrain>,
    scenery: Res<Scenery>,
    glow: Res<WindowGlow>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut views: Query<(Entity, &mut CityView, &Mesh3d, &mut TownSpots)>,
    banners: Query<(Entity, &CityBanner)>,
    labels: Single<Entity, With<crate::ui::WorldLabels>>,
) {
    if !session.is_changed() {
        return;
    }
    let game = &session.game;
    let me = game.players.get(session.me as usize).expect("player");
    let walls = game.rules.building_id("walls");
    let mut shown: HashMap<CityId, Entity> = HashMap::new();
    for (entity, mut view, mesh, mut town_spots) in &mut views {
        let Some(c) = game.cities.get(&view.id) else {
            commands.entity(entity).despawn();
            continue;
        };
        shown.insert(c.id, entity);
        let look = (c.population, walls.is_some_and(|w| c.has(w)), c.owner, game.era(c.owner));
        if view.look != look {
            view.look = look;
            let centre = tile_center(&game.map.grid, c.tile);
            let (model, lit, spots) = town_model(&terrain, centre, c.id as u64, look.0, team(game.players[c.owner as usize].color), look.1, look.3);
            *town_spots = spots;
            if let Some(mut m) = meshes.get_mut(&mesh.0) {
                *m = model.into_mesh();
            }
            if let Some(mut m) = meshes.get_mut(&view.windows) {
                *m = lit.into_mesh();
            }
        }
    }
    for (entity, banner) in &banners {
        match game.cities.get(&banner.city) {
            None => {
                commands.entity(entity).despawn();
            }
            Some(c) if c.owner != banner.owner => {
                commands.entity(entity).despawn();
                spawn_banner(&mut commands, &session, c.id, *labels);
            }
            Some(_) => {}
        }
    }
    // Cities appear once their tile has been explored (they don't move).
    for c in game.cities.values() {
        if shown.contains_key(&c.id) || !me.explored[c.tile as usize] {
            continue;
        }
        let centre = tile_center(&game.map.grid, c.tile);
        let look = (c.population, walls.is_some_and(|w| c.has(w)), c.owner, game.era(c.owner));
        let (model, lit, spots) = town_model(&terrain, centre, c.id as u64, look.0, team(game.players[c.owner as usize].color), look.1, look.3);
        let y = terrain.heights.sample(centre).max(WATER_LEVEL);
        let windows = meshes.add(lit.into_mesh());
        commands.spawn((
            CityView { id: c.id, look, windows: windows.clone() },
            spots,
            Mesh3d(meshes.add(model.into_mesh())),
            MeshMaterial3d(scenery.0.clone()),
            Transform::from_xyz(centre.x, y, centre.y),
            Wrapped { anchor: centre.x, base: centre.x },
            children![(Mesh3d(windows), MeshMaterial3d(glow.0.clone()), Transform::IDENTITY)],
        ));
        spawn_banner(&mut commands, &session, c.id, *labels);
    }
}

/// Banners hang from `labels` (`ui::WorldLabels`).
fn spawn_banner(commands: &mut Commands, session: &Session, city: CityId, labels: Entity) {
    let c = &session.game.cities[&city];
    let owner = &session.game.players[c.owner as usize];
    let tip = if c.owner == session.me {
        Tooltip::new(c.name.clone(), "Click to manage the city.")
    } else {
        Tooltip::new(c.name.clone(), owner.name.clone())
    };
    commands.spawn((
        CityBanner { city, owner: c.owner },
        ChildOf(labels),
        Button,
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(3),
            padding: UiRect::new(px(4), px(8), px(4), px(5)),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        // Anchored at its top centre, at the city tile's near (lower) edge, so
        // it stays clear of the flags of units standing in the city.
        UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
        BackgroundColor(Color::BLACK),
        BorderColor::all(Color::BLACK),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), px(0), px(3), px(0), px(8)),
        Visibility::Hidden,
        GlobalZIndex(-1),
        tip,
        observe(
            move |_: On<Activate>,
                  session: Res<Session>,
                  mut selected: ResMut<SelectedCity>,
                  mut unit: ResMut<crate::play::Selected>,
                  mut rig: ResMut<CameraRig>| {
                let Some(c) = session.game.cities.get(&city) else { return };
                if c.owner == session.me {
                    selected.0 = Some(city);
                    unit.0 = None;
                    rig.focus = tile_center(&session.game.map.grid, c.tile);
                }
            },
        ),
    ));
}

/// The banner's contents: population, name, and for our cities what they
/// build (with turns) plus thin growth and production bars.
fn fill_banner(b: &mut ChildSpawnerCommands, session: &Session, icons: &Icons, city: CityId) {
    let game = &session.game;
    let c = &game.cities[&city];
    let ours = c.owner == session.me;
    b.spawn((Node { align_items: AlignItems::Center, column_gap: px(7), ..default() }, Pickable::IGNORE)).with_children(|row| {
        row.spawn((
            Node {
                min_width: px(24),
                height: px(24),
                padding: UiRect::horizontal(px(4)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.07, 0.06)),
            BorderColor::all(palette::ACCENT),
            Pickable::IGNORE,
            children![(ui::bold(c.population.to_string(), 13.0, palette::ACCENT), Pickable::IGNORE)],
        ));
        row.spawn((ui::heading(c.name.clone(), 15.0, Color::WHITE), TextShadow::default(), Pickable::IGNORE));
        if ours {
            match (c.queue.first(), game.turns_to_build(c.id)) {
                // A project goes on and on: just its icon.
                (Some(&build @ Build::Project(_)), _) => {
                    row.spawn((
                        Node { margin: UiRect::left(px(2)), ..default() },
                        Pickable::IGNORE,
                        children![ui::icon(icons.get(crate::city_panel::build_icon(game, build)), 20.0)],
                    ));
                }
                (Some(&build), Some(turns)) => {
                    let icon = crate::city_panel::build_icon(game, build);
                    row.spawn((
                        Node { align_items: AlignItems::Center, column_gap: px(3), margin: UiRect::left(px(2)), ..default() },
                        Pickable::IGNORE,
                        children![ui::icon(icons.get(icon), 20.0), (ui::bold(turns.to_string(), 13.0, Color::WHITE), Pickable::IGNORE)],
                    ));
                }
                _ => {
                    row.spawn(ui::icon(icons.get(Icon::Warning), 18.0));
                }
            }
        }
    });
    if c.hp < CITY_MAX_HP {
        b.spawn((ui::progress_bar(c.hp as f32 / CITY_MAX_HP as f32, palette::DANGER, 5.0), Pickable::IGNORE));
    }
    if ours {
        let grow = c.food as f32 / growth_threshold(c.population) as f32;
        let build = c.queue.first().map_or(0.0, |&b| c.production as f32 / game.build_cost(b) as f32);
        b.spawn((ui::progress_bar(grow, palette::FOOD, 4.0), Pickable::IGNORE));
        b.spawn((ui::progress_bar(build, palette::PRODUCTION, 4.0), Pickable::IGNORE));
    }
}

fn update_banners(
    mut commands: Commands,
    session: Res<Session>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    selected: Res<SelectedCity>,
    icons: Res<Icons>,
    ui_scale: Res<UiScale>,
    rig: Res<CameraRig>,
    strategic: Res<crate::strategic::StrategicView>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    mut banners: Query<(
        Entity,
        &CityBanner,
        &mut Node,
        &mut UiTransform,
        &mut Visibility,
        &mut BorderColor,
        &mut BackgroundColor,
        Option<&Children>,
    )>,
) {
    let (cam, cam_tf) = *camera;
    let cam_x = cam_tf.translation().x;
    let game = &session.game;
    let scale = ui_scale.0;
    let screen = crate::ui::ui_screen(cam, scale);
    // Banners shrink as the camera pulls back so they don't cover more and
    // more tiles (at the usual distance, 22, they're drawn as made). Scaling
    // is about the centre, so shift up to keep the top edge at the anchor.
    let zoom = (22.0 / rig.distance).powf(0.6).clamp(0.55, 1.0);
    let anchor = UiTransform { scale: Vec2::splat(zoom), ..UiTransform::from_translation(Val2::percent(-50.0, -(1.0 - zoom) * 50.0)) };
    for (entity, banner, mut node, mut transform, mut vis, mut border, mut bg, children) in &mut banners {
        transform.set_if_neq(anchor);
        let Some(c) = game.cities.get(&banner.city) else { continue };
        let centre = tile_center(&game.map.grid, c.tile) + Vec2::new(0.0, 0.62);
        let ground = if strategic.0 {
            crate::strategic::ground(&terrain, tile_center(&game.map.grid, c.tile))
        } else {
            terrain.heights.sample(centre).max(WATER_LEVEL)
        };
        let mut world = Vec3::new(centre.x, ground + 0.1, centre.y);
        world.x += nearest_shift(world.x, cam_x, period.0);
        // Banners hang centred below their anchor, so allow for their width.
        let at = cam.world_to_viewport(cam_tf, world).ok().map(|s| s / scale);
        crate::ui::place_label(&mut node, &mut vis, at, screen, 200.0);
        let color = crate::unit_panel::player_color(&session, c.owner);
        let edge = if selected.0 == Some(c.id) { palette::ACCENT } else { color };
        border.set_if_neq(BorderColor::all(edge));
        let lin = color.to_linear();
        bg.set_if_neq(BackgroundColor(Color::LinearRgba(LinearRgba::new(lin.red * 0.3, lin.green * 0.3, lin.blue * 0.3, 0.92))));
        if session.is_changed() || children.is_none() {
            commands.entity(entity).despawn_related::<Children>();
            commands.entity(entity).with_children(|b| fill_banner(b, &session, &icons, c.id));
        }
    }
}

/// How much brighter than their colour borders are drawn (for bloom).
const BORDER_GLOW: f32 = 2.5;

/// Border lines: along every edge between a player's territory and the outside,
/// inset into the owned tiles: one solid line. Edges are joined into outlines
/// so each is drawn as one unbroken piece.
fn rebuild_borders(
    mut commands: Commands,
    session: Res<Session>,
    terrain: Res<Terrain>,
    mut events: MessageReader<GameEvent>,
    old: Query<Entity, With<BorderMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut xray: ResMut<Assets<XRayMaterial>>,
    mut cache: Local<HashMap<u8, Handle<XRayMaterial>>>,
) {
    let changed = events
        .read()
        .any(|GameEvent(e)| matches!(e, Event::BordersChanged { .. } | Event::Explored { .. } | Event::CityFounded { .. }));
    // A new game (or a loaded one) draws all borders once.
    if session.is_added() {
        cache.clear();
    } else if !changed {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    let game = &session.game;
    let grid = &game.map.grid;
    let me = &game.players[session.me as usize];
    let owner_of = |t: TileId| game.territory_owner(t);

    // Each player's border edges, running with their land on the left.
    let mut edges: HashMap<u8, Vec<(Vec2, Vec2)>> = HashMap::new();
    for t in 0..grid.len() as TileId {
        let Some(owner) = owner_of(t) else { continue };
        let hex = grid.hex(t);
        let c = tile_center(grid, t);
        for d in 0..6 {
            let nb = grid.tile(hex.neighbor(d));
            if nb.is_some_and(|n| owner_of(n) == Some(owner)) {
                continue;
            }
            // Only edges we know about.
            if !me.explored[t as usize] && !nb.is_some_and(|n| me.explored[n as usize]) {
                continue;
            }
            // The edge facing hex direction d (east, then counter-clockwise on
            // screen) runs between corners (6-d)%6 and (7-d)%6.
            let (i0, i1) = ((6 - d) % 6, (7 - d) % 6);
            let (a, b) = (c + corner(i0, 0.0), c + corner(i1, 0.0));
            let edge = if (c - a).dot((b - a).perp()) > 0.0 { (a, b) } else { (b, a) };
            edges.entry(owner).or_default().push(edge);
        }
    }
    // One mesh per outline, so each wraps on its own.
    for (owner, edges) in edges {
        let color = game.players[owner as usize].color;
        let material = cache
            .entry(owner)
            .or_insert_with(|| {
                let [r, g, b] = color;
                // Drawn on top so borders stay visible through forests, after
                // the water (depth bias), and brighter than white so bloom
                // gives it a glow, by day and by night (it's unlit).
                let glow = LinearRgba::from(Color::srgb_u8(r, g, b)) * BORDER_GLOW;
                xray.add(XRayMaterial {
                    base: StandardMaterial {
                        base_color: Color::LinearRgba(glow.with_alpha(1.0)),
                        alpha_mode: AlphaMode::Blend,
                        unlit: true,
                        cull_mode: None,
                        depth_bias: 1000.0,
                        ..default()
                    },
                    extension: XRay {},
                })
            })
            .clone();
        for (points, closed) in chain_edges(edges) {
            let mut line = Ribbons::new();
            // Inset so neighbouring players' borders sit side by side, with a
            // narrow glow fading away on both sides.
            const GLOW: f32 = 0.07;
            line.outline(&terrain.heights, &points, closed, 0.03 - GLOW, 0.03, (0.0, 0.45), 0.05);
            line.outline(&terrain.heights, &points, closed, 0.03, 0.1, (1.0, 1.0), 0.05);
            line.outline(&terrain.heights, &points, closed, 0.1, 0.1 + GLOW, (0.45, 0.0), 0.05);
            let mid = points.iter().map(|p| p.x).sum::<f32>() / points.len() as f32;
            commands.spawn((
                BorderMesh,
                Transform::default(),
                Wrapped::mesh(mid),
                NotShadowCaster,
                NotShadowReceiver,
                crate::strategic::both_layers(),
                Mesh3d(meshes.add(line.into_mesh())),
                MeshMaterial3d(material.clone()),
            ));
        }
    }
}

/// Joins edges (each running from its first point to its second) into
/// polylines, closed where they come back round. Edges meet where one ends
/// and the next starts; at most one edge starts at any point.
fn chain_edges(edges: Vec<(Vec2, Vec2)>) -> Vec<(Vec<Vec2>, bool)> {
    let key = |p: Vec2| ((p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64);
    let mut from: HashMap<(i64, i64), usize> = HashMap::new();
    for (i, &(a, _)) in edges.iter().enumerate() {
        from.insert(key(a), i);
    }
    let ends: std::collections::HashSet<(i64, i64)> = edges.iter().map(|&(_, b)| key(b)).collect();
    let mut used = vec![false; edges.len()];
    let mut out = Vec::new();
    // Open chains first (from edges nothing leads into), then loops.
    let starts: Vec<usize> = (0..edges.len()).filter(|&i| !ends.contains(&key(edges[i].0))).chain(0..edges.len()).collect();
    for start in starts {
        if used[start] {
            continue;
        }
        let mut points = vec![edges[start].0];
        let mut i = start;
        let closed = loop {
            used[i] = true;
            let end = edges[i].1;
            match from.get(&key(end)) {
                Some(&next) if next == start => break true,
                Some(&next) if !used[next] => {
                    points.push(end);
                    i = next;
                }
                _ => {
                    points.push(end);
                    break false;
                }
            }
        };
        out.push((points, closed));
    }
    out
}

/// While a city panel is open, mark the tiles its citizens work and the tile
/// its borders grow into next.
fn draw_worked_tiles(
    mut gizmos: Gizmos,
    session: Res<Session>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    selected: Res<SelectedCity>,
    time: Res<Time>,
    camera: Single<&GlobalTransform, With<crate::camera::MainCamera>>,
) {
    let Some(c) = selected.0.and_then(|id| session.game.cities.get(&id)) else { return };
    let grid = &session.game.map.grid;
    let cam_x = camera.translation().x;
    // The tile the borders take next, pulsing gently.
    if let Some(t) = session.game.next_border_tile(c.id).filter(|_| c.owner == session.me) {
        let p = tile_center(grid, t);
        let shift = nearest_shift(p.x, cam_x, period.0);
        let glow = 0.65 + 0.35 * (time.elapsed_secs() * 3.0).sin();
        let color = palette::CULTURE.with_alpha(glow);
        for inset in [0.08, 0.1, 0.12] {
            let pts: Vec<Vec3> = (0..=6)
                .map(|i| {
                    let q = p + corner(i % 6, inset);
                    Vec3::new(q.x + shift, terrain.heights.sample(q).max(WATER_LEVEL) + 0.12, q.y)
                })
                .collect();
            gizmos.linestrip(pts, color);
        }
    }
    for &t in &c.worked {
        let p = tile_center(grid, t);
        let y = terrain.heights.sample(p).max(WATER_LEVEL) + 0.1;
        let pos = Vec3::new(p.x + nearest_shift(p.x, cam_x, period.0), y, p.y);
        gizmos.circle(Isometry3d::new(pos, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), 0.32, palette::ACCENT);
        gizmos.circle(Isometry3d::new(pos, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), 0.28, palette::ACCENT);
    }
}
