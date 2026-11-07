//! The yields overlay (toggle with Y): every explored tile shows what it
//! yields, as food, production and gold icons with numbers, facing the camera
//! and drawn over everything else. Shown in the strategic view too.

use crate::camera::MainCamera;
use crate::icons::{Glyph, GlyphAtlas, Icon};
use crate::layout::tile_center;
use crate::loading::{AppState, InGameSet};
use crate::menu::Menu;
use crate::overlay::{XRay, XRayMaterial};
use crate::play::{GameEvent, Session};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::wrap::Wrapped;
use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use crate::settings::Settings;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use sim::game::Event;
use sim::hex::TileId;

/// Whether the yields overlay is on.
#[derive(Resource, Default, PartialEq)]
pub struct YieldsView(pub bool);

#[derive(Component)]
struct YieldMesh;

pub struct YieldsPlugin;

impl Plugin for YieldsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<YieldsView>()
            .add_systems(OnEnter(AppState::InGame), reset)
            .add_systems(
                Update,
                (toggle.run_if(in_state(Menu::Closed)), follow_setting, rebuild).chain().after(crate::play::PlaySet).after(crate::terraform::ReshapeSet).in_set(InGameSet),
            );
    }
}

fn reset(mut view: ResMut<YieldsView>, settings: Res<Settings>) {
    // Dev aid: FOURX_YIELDS=1 or 0 starts with the overlay on or off.
    view.0 = match std::env::var("FOURX_YIELDS") {
        Ok(v) => v != "0",
        Err(_) => settings.show_yields,
    };
}

/// Y turns the overlay on or off, and that's remembered (it's a setting).
fn toggle(keys: Res<ButtonInput<KeyCode>>, view: Res<YieldsView>, mut settings: ResMut<Settings>) {
    if keys.just_pressed(KeyCode::KeyY) {
        settings.show_yields = !view.0;
    }
}

fn follow_setting(settings: Res<Settings>, mut view: ResMut<YieldsView>) {
    if settings.is_changed() && !settings.is_added() {
        view.set_if_neq(YieldsView(settings.show_yields));
    }
}

/// World size of a yield icon, and of its number.
const ICON: f32 = 0.3;
const DIGIT: f32 = 0.21;
/// Tiles per mesh, by column (so each mesh wraps on its own).
const STRIP: i32 = 16;

/// Camera-facing quads for one batch of tiles.
#[derive(Default)]
struct Quads {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Quads {
    fn add(&mut self, centre: Vec3, size: f32, right: Vec3, up: Vec3, uv: (Vec2, Vec2)) {
        let base = self.positions.len() as u32;
        let (r, u) = (right * size * 0.5, up * size * 0.5);
        for (p, t) in [(centre - r - u, Vec2::new(uv.0.x, uv.1.y)), (centre + r - u, uv.1), (centre + r + u, Vec2::new(uv.1.x, uv.0.y)), (centre - r + u, uv.0)] {
            self.positions.push(p.to_array());
            self.uvs.push(t.to_array());
        }
        self.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn into_mesh(self) -> Mesh {
        let n = self.positions.len();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn rebuild(
    mut commands: Commands,
    view: Res<YieldsView>,
    session: Res<Session>,
    terrain: Res<Terrain>,
    atlas: Res<GlyphAtlas>,
    strategic: Res<crate::strategic::StrategicView>,
    camera: Single<&GlobalTransform, With<MainCamera>>,
    mut events: MessageReader<GameEvent>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<XRayMaterial>>,
    mut material: Local<Option<Handle<XRayMaterial>>>,
    old: Query<Entity, With<YieldMesh>>,
) {
    let changed = events.read().any(|GameEvent(e)| {
        matches!(
            e,
            Event::TileImproved { .. } | Event::TerrainChanged { .. }
                | Event::TechResearched { .. }
                | Event::BordersChanged { .. }
                | Event::Explored { .. }
                | Event::CityFounded { .. }
                | Event::CityCaptured { .. }
                | Event::CityRazed { .. }
                | Event::TileBought { .. }
        )
    });
    if !view.is_changed() && !strategic.is_changed() && !(changed && view.0) && !session.is_added() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    if !view.0 {
        return;
    }
    let material = material
        .get_or_insert_with(|| {
            materials.add(XRayMaterial {
                base: StandardMaterial {
                    base_color_texture: Some(atlas.image.clone()),
                    unlit: true,
                    fog_enabled: false,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                },
                extension: XRay {},
            })
        })
        .clone();
    let game = &session.game;
    let me = session.me;
    let grid = &game.map.grid;
    let (right, up) = (camera.right().as_vec3(), camera.up().as_vec3());
    let explored = &game.players[me as usize].explored;
    let mut strips: Vec<Quads> = (0..(grid.width + STRIP - 1) / STRIP).map(|_| Quads::default()).collect();
    for t in 0..game.map.tiles.len() as TileId {
        if !explored[t as usize] {
            continue;
        }
        let y = game.tile_yields(me, t);
        let parts: Vec<(Icon, i32)> =
            [(Icon::Food, y.food), (Icon::Production, y.production), (Icon::Gold, y.gold)].into_iter().filter(|p| p.1 > 0).collect();
        if parts.is_empty() {
            continue;
        }
        let c = tile_center(grid, t);
        let ground = if strategic.0 { crate::strategic::ground(&terrain, c) } else { terrain.heights.sample(c).max(WATER_LEVEL) };
        let centre = Vec3::new(c.x, ground + 0.35, c.y);
        // A disc per yield, with a little gap between them.
        let step = ICON * 1.32;
        let width = step * parts.len() as f32;
        let q = &mut strips[(grid.hex(t).to_offset().0 / STRIP) as usize];
        let back = camera.forward().as_vec3() * 0.01;
        // Soft shadows first, so the icons read on bright ground.
        for i in 0..parts.len() {
            let p = centre + right * (-width * 0.5 + step * (i as f32 + 0.5));
            q.add(p + back, ICON * 1.25, right, up, atlas.uv(Glyph::Disc));
        }
        for (i, (icon, n)) in parts.iter().enumerate() {
            let p = centre + right * (-width * 0.5 + step * (i as f32 + 0.5));
            // With a number, the icon moves up-left to make room for it in
            // the disc's lower right, so nothing spills onto the neighbour.
            if *n > 1 {
                q.add(p + (up * 0.1 - right * 0.12) * ICON, ICON * 0.74, right, up, atlas.uv(Glyph::Icon(*icon)));
                let corner = p + (right * 0.26 - up * 0.24) * ICON - camera.forward().as_vec3() * 0.02;
                q.add(corner, DIGIT * 0.85, right, up, atlas.uv(Glyph::Digit((*n).min(9) as u8)));
            } else {
                q.add(p, ICON * 0.9, right, up, atlas.uv(Glyph::Icon(*icon)));
            }
        }
    }
    for (i, q) in strips.into_iter().enumerate() {
        if q.indices.is_empty() {
            continue;
        }
        // The strip's middle, for wrapping.
        let mid = tile_center(grid, (i as i32 * STRIP + STRIP / 2).min(grid.width - 1) as TileId).x;
        commands.spawn((
            YieldMesh,
            Mesh3d(meshes.add(q.into_mesh())),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            Wrapped::mesh(mid),
            NotShadowCaster,
            NotShadowReceiver,
            // Drawn by the icon camera, untouched by tonemapping.
            RenderLayers::layer(crate::camera::ICON_LAYER),
            Pickable::IGNORE,
        ));
    }
}
