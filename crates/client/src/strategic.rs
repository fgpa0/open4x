//! The strategic view (toggle with V): the map as clean, flat-coloured hex
//! tiles like a board game, each at its own height, with simple glyphs for
//! mountains, forests, cities, improvements and roads. Units show only as
//! their flags. It's drawn on its own render layer: the camera switches to
//! it, and overlays that belong to both views (borders, selection, path
//! previews, gizmos) are on both layers.

use crate::camera::MainCamera;
use crate::icons::{Glyph, GlyphAtlas, Icon};
use crate::layout::{hex_at, hex_center, tile_center};
use crate::loading::{AppState, InGameSet};
use crate::menu::Menu;
use crate::play::{GameEvent, Session};
use crate::procmesh::{cone, cylinder};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::wrap::Wrapped;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use sim::game::{Event, Game};
use sim::hex::TileId;
use sim::map::Elevation;

/// Whether the strategic view is on.
#[derive(Resource, Default)]
pub struct StrategicView(pub bool);

/// The render layer only the strategic view shows.
const LAYER: usize = 1;

/// For overlays shown in both views.
pub fn both_layers() -> RenderLayers {
    RenderLayers::from_layers(&[0, LAYER])
}

/// The height a point's tile is drawn at in the strategic view: its
/// centre's ground height, the same for the whole tile.
pub fn ground(terrain: &Terrain, p: Vec2) -> f32 {
    terrain.heights.sample(hex_center(hex_at(p))).max(WATER_LEVEL) * HEIGHT
}

/// Relief is kept, but lowered, so the board reads as one surface.
const HEIGHT: f32 = 0.5;
/// Tiles per mesh, by column (so each mesh wraps on its own).
const STRIP: i32 = 16;

#[derive(Component, Clone)]
struct StrategicMesh;

pub struct StrategicPlugin;

impl Plugin for StrategicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StrategicView>()
            .add_systems(Startup, gizmos_in_both)
            .add_systems(OnEnter(AppState::InGame), reset)
            .add_systems(
                Update,
                (toggle.run_if(in_state(Menu::Closed)), switch_camera, rebuild).chain().after(crate::play::PlaySet).after(crate::terraform::ReshapeSet).in_set(InGameSet),
            )
            .add_systems(OnExit(AppState::InGame), leave);
    }
}

fn gizmos_in_both(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.render_layers = both_layers();
}

fn reset(mut view: ResMut<StrategicView>) {
    // Dev aid: FOURX_STRATEGIC=1 starts in the strategic view.
    view.0 = std::env::var("FOURX_STRATEGIC").is_ok();
}

fn leave(mut view: ResMut<StrategicView>, mut camera: Query<&mut RenderLayers, With<MainCamera>>) {
    view.0 = false;
    for mut layers in &mut camera {
        *layers = RenderLayers::layer(0);
    }
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut view: ResMut<StrategicView>) {
    if keys.just_pressed(KeyCode::Equal) {
        view.0 = !view.0;
    }
}

fn switch_camera(mut commands: Commands, view: Res<StrategicView>, camera: Single<Entity, With<MainCamera>>) {
    if view.is_changed() {
        let layers = if view.0 { RenderLayers::layer(LAYER) } else { RenderLayers::layer(0) };
        commands.entity(*camera).insert(layers);
    }
}

// --- Geometry ---------------------------------------------------------------

/// Unlit geometry with per-vertex colour; shading is baked in from a fixed
/// light so glyphs read as solid shapes.
#[derive(Default)]
struct Board {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

const LIGHT: Vec3 = Vec3::new(-0.45, 0.8, 0.4);

fn shaded(c: Vec4, n: Vec3) -> [f32; 4] {
    let l = 0.62 + 0.42 * n.normalize_or_zero().dot(LIGHT.normalize()).max(0.0);
    [c.x * l, c.y * l, c.z * l, 1.0]
}

impl Board {
    /// A quad with a colour already shaded.
    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: [f32; 4]) {
        let base = self.positions.len() as u32;
        for p in [a, b, c, d] {
            self.positions.push(p.to_array());
            self.colors.push(color);
        }
        self.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// A flat band `w` wide along `points`, at height `y`.
    fn band(&mut self, points: &[Vec2], y: f32, w: f32, color: Vec4) {
        for pair in points.windows(2) {
            let (a, z) = (pair[0], pair[1]);
            let side = (z - a).perp().normalize_or_zero() * w * 0.5;
            let v = |p: Vec2| Vec3::new(p.x, y, p.y);
            // Round joints: a little disc at each point.
            self.quad(v(a - side), v(z - side), v(z + side), v(a + side), color.to_array());
            for i in 0..6 {
                let (d0, d1) = (Vec2::from_angle(i as f32 * std::f32::consts::FRAC_PI_3) * w * 0.5, Vec2::from_angle((i + 1) as f32 * std::f32::consts::FRAC_PI_3) * w * 0.5);
                self.tri(v(z), v(z + d0), v(z + d1), color, true);
            }
        }
    }

    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec4, flat: bool) {
        let n = if flat { Vec3::Y } else { (b - a).cross(c - a) };
        let col = if flat { color.to_array() } else { shaded(color, n) };
        let base = self.positions.len() as u32;
        for p in [a, b, c] {
            self.positions.push(p.to_array());
            self.colors.push(col);
        }
        self.indices.extend([base, base + 1, base + 2]);
    }

    /// A primitive mesh, coloured and transformed, shaded by its faces.
    fn add(&mut self, mesh: &Mesh, color: Vec4, t: Transform) {
        let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
        let m = t.to_matrix();
        let p = |i: u32| m.transform_point3(Vec3::from(pos[i as usize]));
        let idx: Vec<u32> = match mesh.indices() {
            Some(i) => i.iter().map(|v| v as u32).collect(),
            None => (0..pos.len() as u32).collect(),
        };
        for tri in idx.chunks(3) {
            // Primitives wind counter-clockwise seen from outside.
            self.tri(p(tri[0]), p(tri[1]), p(tri[2]), color, false);
        }
    }

    fn into_mesh(self) -> Mesh {
        let n = self.positions.len();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Vec4 {
    let c = Color::srgb_u8(r, g, b).to_linear();
    Vec4::new(c.red, c.green, c.blue, 1.0)
}

/// A tile's flat colour on the board.
fn tile_color(game: &Game, t: TileId) -> Vec4 {
    let tile = game.map.tile(t);
    let terrain = game.rules.terrain(tile.terrain);
    let base = match terrain.id.as_str() {
        "ocean" => rgb(46, 84, 128),
        "coast" => rgb(78, 128, 170),
        "lake" => rgb(86, 140, 178),
        "grassland" => rgb(118, 158, 80),
        "plains" => rgb(170, 164, 94),
        "desert" => rgb(214, 192, 136),
        "tundra" => rgb(146, 150, 126),
        "snow" => rgb(226, 230, 234),
        _ => rgb(150, 150, 150),
    };
    let mut c = match tile.feature.map(|f| game.rules.feature(f).id.as_str()) {
        Some("forest") => base.lerp(rgb(62, 104, 58), 0.45),
        Some("jungle") => base.lerp(rgb(48, 112, 72), 0.55),
        Some("marsh") => base.lerp(rgb(96, 128, 116), 0.5),
        _ => base,
    };
    // Ice on water this turn: pack ice white, thin ice paler water.
    match game.sea_ice(t) {
        sim::climate::SeaIce::Pack => c = rgb(220, 232, 240),
        sim::climate::SeaIce::Thin => c = c.lerp(rgb(220, 232, 240), 0.55),
        sim::climate::SeaIce::Open => {}
    }
    match tile.elevation {
        Elevation::Hills => c *= 0.88,
        Elevation::Mountains => c = rgb(132, 124, 118),
        Elevation::Flat => {}
    }
    c.w = 1.0;
    c
}

/// Builds the board for the tiles in columns `col0..col0 + STRIP`.
fn build_strip(game: &Game, me: sim::game::PlayerId, terrain: &Terrain, col0: i32) -> Mesh {
    let grid = &game.map.grid;
    let player = &game.players[me as usize];
    let mut b = Board::default();
    let peak = cone(0.32, 0.5, 5);
    let snowcap = cone(0.14, 0.2, 5);
    let tree = cone(0.12, 0.3, 5);
    let keep = Cuboid::new(0.22, 0.26, 0.22).mesh().build();
    let roof = cone(0.2, 0.16, 4);
    let wall = cylinder(0.5, 0.08, 6);
    let corner = |i: usize, inset: f32| crate::overlay::corner(i, inset);
    let height = |t: TileId| terrain.heights.sample(tile_center(grid, t)).max(WATER_LEVEL) * HEIGHT;
    for row in 0..grid.height {
        for col in col0..(col0 + STRIP).min(grid.width) {
            let t = (row * grid.width + col) as TileId;
            let c = tile_center(grid, t);
            if !player.explored[t as usize] {
                // Unknown land: plain dark tiles.
                let y = WATER_LEVEL - 0.02;
                for i in 0..6 {
                    let (p0, p1) = (c + corner(i, 0.0), c + corner((i + 1) % 6, 0.0));
                    b.tri(Vec3::new(c.x, y, c.y), Vec3::new(p1.x, y, p1.y), Vec3::new(p0.x, y, p0.y), rgb(34, 32, 30), true);
                }
                continue;
            }
            let h = height(t);
            let mut color = tile_color(game, t);
            if let Some(owner) = game.territory_owner(t) {
                let [r, g, bl] = game.players[owner as usize].color;
                color = color.lerp(rgb(r, g, bl), 0.22);
            }
            let seen = player.sees(t);
            let dim = |c: Vec4| if seen { c } else { (c * 0.6).lerp(Vec4::splat(c.x * 0.2 + c.y * 0.2 + c.z * 0.2), 0.35).with_w(1.0) };
            let top = dim(color);
            let rim = dim(color * 0.78);
            let v = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
            // The top, with a darker rim so tiles read separately.
            for i in 0..6 {
                let (a0, a1) = (c + corner(i, 0.08), c + corner((i + 1) % 6, 0.08));
                let (o0, o1) = (c + corner(i, 0.0), c + corner((i + 1) % 6, 0.0));
                b.tri(v(c, h), v(a1, h), v(a0, h), top, true);
                b.tri(v(a0, h), v(a1, h), v(o1, h), rim, true);
                b.tri(v(a0, h), v(o1, h), v(o0, h), rim, true);
            }
            // Sides down to lower neighbours.
            let hex = grid.hex(t);
            for d in 0..6 {
                let Some(n) = grid.tile(hex.neighbor(d)) else { continue };
                let hn = if player.explored[n as usize] { height(n) } else { WATER_LEVEL - 0.02 };
                if hn >= h - 0.005 {
                    continue;
                }
                // The edge facing neighbour `d` runs between the corners 30°
                // either side of the direction to it.
                let out = (hex_center(hex.neighbor(d)) - hex_center(hex)).normalize();
                let (q0, q1) = (c + Vec2::from_angle(-std::f32::consts::FRAC_PI_6).rotate(out), c + Vec2::from_angle(std::f32::consts::FRAC_PI_6).rotate(out));
                let side = shaded(dim(color * 0.75), Vec3::new(out.x, 0.0, out.y));
                b.quad(v(q0, h), v(q1, h), v(q1, hn), v(q0, hn), side);
            }
            // Rivers, as on the map: one bend through the middle, or arms
            // that meet there.
            let tile = game.map.tile(t);
            if tile.rivers != 0 && !game.map.is_water(&game.rules, t) {
                let edges: Vec<Vec2> = (0..6)
                    .filter(|d| tile.rivers & (1 << d) != 0)
                    .map(|d| (c + hex_center(hex.neighbor(d)) - hex_center(hex) + c) * 0.5)
                    .collect();
                let water = dim(rgb(64, 132, 186));
                if let [a, z] = edges[..] {
                    let curve: Vec<Vec2> = (0..=8)
                        .map(|i| {
                            let k = i as f32 / 8.0;
                            a * (1.0 - k) * (1.0 - k) + c * 2.0 * k * (1.0 - k) + z * k * k
                        })
                        .collect();
                    b.band(&curve, h + 0.008, 0.13, water);
                } else {
                    for e in edges {
                        b.band(&[e, c], h + 0.008, 0.13, water);
                    }
                }
            }
            // Glyphs: peaks, trees, towns.
            let at = |p: Vec2, y: f32| Transform::from_xyz(p.x, h + y, p.y);
            if tile.elevation == Elevation::Mountains {
                b.add(&peak, dim(rgb(150, 140, 132)), at(c + Vec2::new(-0.18, 0.1), 0.25));
                b.add(&snowcap, dim(rgb(240, 242, 246)), at(c + Vec2::new(-0.18, 0.1), 0.42));
                b.add(&peak, dim(rgb(128, 120, 112)), at(c + Vec2::new(0.22, -0.05), 0.2).with_scale(Vec3::splat(0.8)));
            }
            let feature = tile.feature.map(|f| game.rules.feature(f).id.as_str());
            if matches!(feature, Some("forest" | "jungle")) && game.city_at(t).is_none() {
                let leaf = if feature == Some("jungle") { rgb(36, 104, 66) } else { rgb(40, 84, 48) };
                for o in [Vec2::new(-0.3, -0.12), Vec2::new(0.05, -0.3), Vec2::new(0.28, 0.08), Vec2::new(-0.06, 0.22)] {
                    b.add(&tree, dim(leaf), at(c + o, 0.15));
                }
            }
            if let Some(city) = game.city_at(t) {
                let [r, g, bl] = game.players[city.owner as usize].color;
                let team = dim(rgb(r, g, bl));
                b.add(&wall, team, at(c, 0.04));
                b.add(&keep, dim(rgb(236, 230, 216)), at(c, 0.21));
                b.add(&roof, team, at(c, 0.42).with_rotation(Quat::from_rotation_y(0.785)));
            }
            // Roads: brown lines to each neighbouring road.
            if game.has_road(t) {
                for d in 0..6 {
                    if !grid.tile(hex.neighbor(d)).is_some_and(|n| game.has_road(n) && player.explored[n as usize]) {
                        continue;
                    }
                    let e = (hex_center(hex.neighbor(d)) - hex_center(hex)) * 0.5;
                    let side = Vec2::new(-e.y, e.x).normalize() * 0.05;
                    let (a, z) = (c, c + e);
                    let road = dim(rgb(122, 92, 60));
                    b.tri(v(a - side, h + 0.01), v(z + side, h + 0.01), v(z - side, h + 0.01), road, true);
                    b.tri(v(a - side, h + 0.01), v(a + side, h + 0.01), v(z + side, h + 0.01), road, true);
                }
            }
        }
    }
    b.into_mesh()
}

/// Improvement icons on their tiles' lower left, facing the camera, for
/// the tiles in columns `col0..col0 + STRIP`.
fn build_icons(game: &Game, me: sim::game::PlayerId, terrain: &Terrain, atlas: &GlyphAtlas, col0: i32, right: Vec3, up: Vec3) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let explored = &game.players[me as usize].explored;
    let grid = &game.map.grid;
    for (t, w) in game.works.iter().enumerate() {
        let col = t as i32 % grid.width;
        if col < col0 || col >= col0 + STRIP {
            continue;
        }
        let Some(imp) = w.improvement.filter(|_| explored[t]) else { continue };
        let c = tile_center(&game.map.grid, t as TileId) + Vec2::new(-0.42, 0.4);
        let p = Vec3::new(c.x, ground(terrain, c) + 0.2, c.y);
        let (uv0, uv1) = atlas.uv(Glyph::Icon(Icon::improvement(&game.rules.improvement(imp).art)));
        let (r, u) = (right * 0.17, up * 0.17);
        let base = positions.len() as u32;
        for (q, uv) in [(p - r - u, Vec2::new(uv0.x, uv1.y)), (p + r - u, uv1), (p + r + u, Vec2::new(uv1.x, uv0.y)), (p - r + u, uv0)] {
            positions.push(q.to_array());
            uvs.push(uv.to_array());
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let n = positions.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

#[derive(Default)]
struct Materials {
    board: Option<Handle<StandardMaterial>>,
    icons: Option<Handle<StandardMaterial>>,
}

fn rebuild(
    mut commands: Commands,
    view: Res<StrategicView>,
    session: Res<Session>,
    terrain: Res<Terrain>,
    atlas: Res<GlyphAtlas>,
    camera: Single<&GlobalTransform, With<MainCamera>>,
    mut events: MessageReader<GameEvent>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Materials>,
    old: Query<Entity, With<StrategicMesh>>,
) {
    let changed = events.read().any(|GameEvent(e)| {
        matches!(
            e,
            Event::TileImproved { .. } | Event::TerrainChanged { .. }
                | Event::TurnStarted { .. }
                | Event::VisibilityChanged { .. }
                | Event::Explored { .. }
                | Event::BordersChanged { .. }
                | Event::CityFounded { .. }
                | Event::CityCaptured { .. }
                | Event::CityRazed { .. }
                | Event::CityTraded { .. }
        )
    });
    if !view.is_changed() && !(changed && view.0) {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    if !view.0 {
        return;
    }
    let board = cache
        .board
        .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, cull_mode: None, ..default() }))
        .clone();
    let icons = cache
        .icons
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color_texture: Some(atlas.image.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            })
        })
        .clone();
    let game = &session.game;
    let grid = &game.map.grid;
    let layer = RenderLayers::layer(LAYER);
    let common = (StrategicMesh, Transform::default(), NotShadowCaster, NotShadowReceiver, layer.clone());
    let (right, up) = (camera.right().as_vec3(), camera.up().as_vec3());
    for col0 in (0..grid.width).step_by(STRIP as usize) {
        let mid = tile_center(grid, (col0 + STRIP / 2).min(grid.width - 1) as TileId).x;
        let mesh = build_strip(game, session.me, &terrain, col0);
        commands.spawn((common.clone(), Mesh3d(meshes.add(mesh)), MeshMaterial3d(board.clone()), Wrapped::mesh(mid)));
        let mesh = build_icons(game, session.me, &terrain, &atlas, col0, right, up);
        commands.spawn((common.clone(), Mesh3d(meshes.add(mesh)), MeshMaterial3d(icons.clone()), Wrapped::mesh(mid)));
    }
}
