//! Map overlays draped over the terrain: the hex grid (toggle with G) and the
//! highlight under the mouse cursor.

use crate::layout::{hex_at, tile_center};
use crate::loading::{AppState, InGameSet};
use crate::menu::Menu;
use crate::terrain::{height::{HeightField, WATER_LEVEL}, Terrain};
use crate::wrap::Wrapped;
use crate::GameMap;
use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use sim::hex::TileId;
use sim::map::Map;

/// Tile currently under the mouse cursor.
#[derive(Resource, Default)]
pub struct HoveredTile(pub Option<TileId>);

#[derive(Component)]
struct HexGrid;

#[derive(Component)]
struct Highlight;

/// A material drawn on top of everything (no depth test), so map outlines
/// show through trees and units as a faint "x-ray" line.
pub type XRayMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, XRay>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct XRay {}

impl bevy::pbr::MaterialExtension for XRay {
    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(bevy::render::render_resource::CompareFunction::Always);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HoveredTile>()
            .add_plugins(MaterialPlugin::<XRayMaterial>::default())
            .add_systems(Startup, gizmos_on_top)
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(
                Update,
                (toggle_grid, pick_tile, move_highlight).chain().run_if(in_state(Menu::Closed)).in_set(InGameSet),
            );
    }
}

/// Pointy-top hex corner `i` (0..6) relative to the centre, slightly inset.
pub fn corner(i: usize, inset: f32) -> Vec2 {
    let a = (60.0 * i as f32 - 30.0).to_radians();
    Vec2::new(a.cos(), a.sin()) * (1.0 - inset)
}

/// Builds flat ribbons along hex edges, following the terrain (or water surface).
pub struct Ribbons {
    positions: Vec<[f32; 3]>,
    /// Per-vertex opacity (vertex colour alpha).
    alphas: Vec<f32>,
    indices: Vec<u32>,
}

impl Ribbons {
    pub fn new() -> Self {
        Self { positions: Vec::new(), alphas: Vec::new(), indices: Vec::new() }
    }

    pub fn edge(&mut self, heights: &HeightField, a: Vec2, b: Vec2, width: f32, lift: f32) {
        const SEGMENTS: usize = 6;
        let side = (b - a).perp().normalize() * width * 0.5;
        let base = self.positions.len() as u32;
        for s in 0..=SEGMENTS {
            let p = a.lerp(b, s as f32 / SEGMENTS as f32);
            let y = heights.sample(p).max(WATER_LEVEL) + lift;
            for q in [p - side, p + side] {
                self.positions.push([q.x, y, q.y]);
                self.alphas.push(1.0);
            }
        }
        for s in 0..SEGMENTS as u32 {
            let i = base + s * 2;
            self.indices.extend_from_slice(&[i, i + 1, i + 2, i + 1, i + 3, i + 2]);
        }
    }

    /// A band along a polyline, between `near` and `far` in from it on its
    /// left side (negative is outside), with mitred corners so the band is one
    /// unbroken piece; its opacity goes from `alpha.0` at `near` to `alpha.1`
    /// at `far`. `closed` joins the last point back to the first.
    pub fn outline(&mut self, heights: &HeightField, points: &[Vec2], closed: bool, near: f32, far: f32, alpha: (f32, f32), lift: f32) {
        const SEGMENTS: usize = 6;
        let n = points.len();
        if n < 2 {
            return;
        }
        let left = |a: Vec2, b: Vec2| (b - a).perp().normalize();
        // Per point, the offset that puts a line at distance 1 from both edges.
        let miter = |i: usize| {
            let prev = if i > 0 { Some(left(points[i - 1], points[i])) } else if closed { Some(left(points[n - 1], points[0])) } else { None };
            let next = if i + 1 < n { Some(left(points[i], points[i + 1])) } else if closed { Some(left(points[n - 1], points[0])) } else { None };
            match (prev, next) {
                (Some(a), Some(b)) => (a + b) / (1.0 + a.dot(b)).max(0.2),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => Vec2::ZERO,
            }
        };
        let offsets: Vec<Vec2> = (0..n).map(miter).collect();
        let edges = if closed { n } else { n - 1 };
        for e in 0..edges {
            let (i, j) = (e, (e + 1) % n);
            let base = self.positions.len() as u32;
            for s in 0..=SEGMENTS {
                let t = s as f32 / SEGMENTS as f32;
                let p = points[i].lerp(points[j], t);
                let m = offsets[i].lerp(offsets[j], t);
                // Height from the polyline itself, so bands side by side stay
                // together on slopes.
                let y = heights.sample(p).max(WATER_LEVEL) + lift;
                for (q, a) in [(p + m * near, alpha.0), (p + m * far, alpha.1)] {
                    self.positions.push([q.x, y, q.y]);
                    self.alphas.push(a);
                }
            }
            for s in 0..SEGMENTS as u32 {
                let k = base + s * 2;
                self.indices.extend_from_slice(&[k, k + 1, k + 2, k + 1, k + 3, k + 2]);
            }
        }
    }

    pub fn into_mesh(self) -> Mesh {
        let n = self.positions.len();
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
            .with_inserted_indices(Indices::U32(self.indices));
        // Vertex colours only where opacity varies (they multiply the material's).
        if self.alphas.iter().all(|&a| a == 1.0) {
            return mesh;
        }
        let colors: Vec<[f32; 4]> = self.alphas.iter().map(|&a| [1.0, 1.0, 1.0, a]).collect();
        mesh.with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    }
}

pub fn overlay_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        depth_bias: 10.0,
        ..default()
    }
}

/// Grid strips built off the main thread, with the X of each strip's centre.
#[derive(Resource)]
pub struct OverlayBuild {
    strips: Vec<(Mesh, f32)>,
}

/// Builds the hex grid meshes. Pure CPU work, safe to run on any thread.
pub fn build(map: &Map, heights: &HeightField) -> OverlayBuild {
    let grid = &map.grid;
    // Split into column strips so each strip can wrap independently.
    const STRIP: i32 = 16;
    let strips = (0..grid.width)
        .step_by(STRIP as usize)
        .map(|col0| {
            let mut ribbons = Ribbons::new();
            for row in 0..grid.height {
                for col in col0..(col0 + STRIP).min(grid.width) {
                    let c = tile_center(grid, (row * grid.width + col) as TileId);
                    // Each edge is shared by two hexes, so draw only half of them per hex.
                    for i in 0..3 {
                        ribbons.edge(heights, c + corner(i, 0.0), c + corner(i + 1, 0.0), 0.03, 0.02);
                    }
                }
            }
            let mid = tile_center(grid, (col0 + STRIP / 2).min(grid.width - 1) as TileId).x;
            (ribbons.into_mesh(), mid)
        })
        .collect();
    OverlayBuild { strips }
}

/// Path previews and move-target rings (gizmos) draw on top of the terrain and
/// trees, so they stay visible in forests and behind hills.
fn gizmos_on_top(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.depth_bias = -1.0;
}

fn spawn(
    mut commands: Commands,
    mut build: ResMut<OverlayBuild>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut xray: ResMut<Assets<XRayMaterial>>,
) {
    let grid_material = materials.add(overlay_material(Color::srgba(1.0, 1.0, 0.95, 0.22)));
    // FOURX_GRID=1 starts with the grid shown (handy for screenshots).
    let visibility = if std::env::var("FOURX_GRID").is_ok_and(|v| !v.is_empty()) { Visibility::Visible } else { Visibility::Hidden };
    for (mesh, mid) in std::mem::take(&mut build.strips) {
        commands.spawn((
            HexGrid,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(grid_material.clone()),
            Transform::default(),
            Wrapped::mesh(mid),
            NotShadowCaster,
            NotShadowReceiver,
            visibility,
            crate::strategic::both_layers(),
        ));
    }
    // Hover outline, plus a faint copy drawn on top that shows through trees.
    let mesh = meshes.add(Ribbons::new().into_mesh());
    let highlight = || {
        (
            Highlight,
            Mesh3d(mesh.clone()),
            Transform::default(),
            Wrapped::mesh(0.0),
            NotShadowCaster,
            NotShadowReceiver,
            Visibility::Hidden,
            crate::strategic::both_layers(),
        )
    };
    commands.spawn((highlight(), MeshMaterial3d(materials.add(overlay_material(Color::srgba(1.0, 0.85, 0.3, 0.9))))));
    let see_through = xray.add(XRayMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(1.0, 0.85, 0.3, 0.55),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        },
        extension: XRay {},
    });
    commands.spawn((highlight(), MeshMaterial3d(see_through)));
    commands.remove_resource::<OverlayBuild>();
}

fn toggle_grid(keys: Res<ButtonInput<KeyCode>>, mut grid: Query<&mut Visibility, With<HexGrid>>) {
    // G alone forms an army (see `play.rs`).
    if keys.just_pressed(KeyCode::KeyG) && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        for mut v in &mut grid {
            v.toggle_visible_hidden();
        }
    }
}

/// Casts a ray from the cursor and marches it against the terrain heights.
fn pick_tile(
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    terrain: Res<Terrain>,
    game: Res<GameMap>,
    hover: Res<bevy::picking::hover::HoverMap>,
    nodes: Query<(), With<Node>>,
    mut hovered: ResMut<HoveredTile>,
) {
    let (camera, cam_tf) = *camera;
    // The interface covers the map: nothing is hovered under a panel.
    let over_ui = crate::ui::pointer_over_ui(&hover, &nodes);
    let hit = window
        .cursor_position()
        .filter(|_| !over_ui)
        .and_then(|cursor| camera.viewport_to_world(cam_tf, cursor).ok())
        .and_then(|ray| {
            let surface = |p: Vec3| terrain.heights.sample(p.xz()).max(WATER_LEVEL);
            let mut prev = 0.0;
            let mut t = 0.0;
            while t < 600.0 {
                let p = ray.get_point(t);
                if p.y <= surface(p) {
                    // Refine between the last point above and this one below.
                    let (mut lo, mut hi) = (prev, t);
                    for _ in 0..12 {
                        let mid = (lo + hi) * 0.5;
                        let q = ray.get_point(mid);
                        if q.y <= surface(q) { hi = mid } else { lo = mid }
                    }
                    return Some(ray.get_point(hi).xz());
                }
                prev = t;
                t += 0.15;
            }
            None
        });
    // Hex lookup wraps columns, so hits on any copy of the map resolve correctly.
    let tile = hit.and_then(|p| game.map.grid.tile(hex_at(p)));
    if hovered.0 != tile {
        hovered.0 = tile;
    }
}

fn move_highlight(
    hovered: Res<HoveredTile>,
    game: Res<GameMap>,
    terrain: Res<Terrain>,
    mut highlights: Query<(&Mesh3d, &mut Visibility, &mut Wrapped), With<Highlight>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if !hovered.is_changed() {
        return;
    }
    let Some(id) = hovered.0 else {
        for (_, mut visibility, _) in &mut highlights {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let c = tile_center(&game.map.grid, id);
    let mut ribbons = Ribbons::new();
    for i in 0..6 {
        ribbons.edge(&terrain.heights, c + corner(i, 0.04), c + corner(i + 1, 0.04), 0.06, 0.03);
    }
    // Both outlines share one mesh.
    let mut mesh = Some(ribbons.into_mesh());
    for (handle, mut visibility, mut wrapped) in &mut highlights {
        if let (Some(new), Some(mut m)) = (mesh.take(), meshes.get_mut(&handle.0)) {
            *m = new;
        }
        *wrapped = Wrapped::mesh(c.x);
        *visibility = Visibility::Visible;
    }
}
