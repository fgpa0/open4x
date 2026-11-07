//! Fog of war: a soft visibility texture for this screen's player, sampled by
//! the terrain, water and vegetation shaders. 0 = never explored, 0.5 = explored
//! but not currently seen, 1 = visible.
//!
//! The texture doesn't jump to a new state: each texel eases towards it
//! (`FogFade`), so fog dissolves slowly where units move into it. A new game
//! starts fully fogged and clears around the player as the intro camera lands.

use crate::layout::{hex_at, hex_center, map_bounds};
use crate::loading::{AppState, InGameSet};
use crate::play::{GameEvent, Session};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rayon::prelude::*;
use sim::game::Event;
use sim::hex::MapGrid;

/// Fog texture samples per world unit.
const RESOLUTION: f32 = 4.0;

/// How fast fog clears, in texture units (0..1) per second: unexplored to
/// explored takes about 1.4 s, and reaching full visibility as long again.
const CLEAR_RATE: f32 = 0.36;
/// How fast seen land falls back to remembered when units leave.
const COVER_RATE: f32 = 0.8;
/// Seconds before the first reveal of a game, while the intro camera descends.
const FIRST_REVEAL_DELAY: f32 = 1.0;

/// The world rectangle an entity covers. When nothing in it has been explored,
/// the entity skips work: `hide` entities aren't drawn at all (plants), others
/// just stop casting shadows (terrain, which still draws the mist).
#[derive(Component)]
pub struct Footprint {
    pub min: Vec2,
    pub max: Vec2,
    pub hide: bool,
}

/// The fog texture (shared by materials; its contents are updated in place).
#[derive(Resource)]
pub struct FogOfWar {
    pub image: Handle<Image>,
}

/// The fog shown (`current`) easing towards the real visibility (`target`).
#[derive(Resource, Default)]
struct FogFade {
    width: u32,
    height: u32,
    target: Vec<f32>,
    current: Vec<f32>,
    /// Seconds before fading starts.
    hold: f32,
    settled: bool,
}

pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FogFade>()
            .add_systems(Startup, create)
            .add_systems(OnEnter(AppState::InGame), refresh_now)
            .add_systems(Update, (refresh_on_change, fade).chain().after(crate::play::PlaySet).in_set(InGameSet));
    }
}

/// World XZ origin and size covered by the fog texture (one wrap period wide),
/// for shaders: uv = (xz - origin) / size.
pub fn params(grid: &MapGrid) -> Vec4 {
    let (min, max) = map_bounds(grid);
    Vec4::new(min.x, min.y, max.x - min.x, max.y - min.y)
}

fn create(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Placeholder until the world exists: everything visible.
    let mut image = Image::new(
        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![255],
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = sampler();
    commands.insert_resource(FogOfWar { image: images.add(image) });
}

fn sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..Default::default()
    })
}

/// Visibility per texel: tile states blended between neighbours so fog edges
/// are soft and don't follow the hex grid exactly.
fn compute(session: &Session) -> (u32, u32, Vec<f32>) {
    let game = &session.game;
    let grid = &game.map.grid;
    let player = &game.players[session.me as usize];
    let (min, max) = map_bounds(grid);
    let width = ((max.x - min.x) * RESOLUTION).round() as u32;
    let height = ((max.y - min.y) * RESOLUTION).round() as u32;
    let texel = (max - min) / Vec2::new(width as f32, height as f32);
    let state = |h| match grid.tile(h) {
        Some(t) if player.sees(t) => 1.0,
        Some(t) if player.explored[t as usize] => 0.5,
        _ => 0.0,
    };
    let data = (0..height)
        .into_par_iter()
        .flat_map_iter(|y| {
            (0..width).map(move |x| {
                let p = min + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * texel;
                let home = hex_at(p);
                let (mut sum, mut weight) = (0.0, 0.0);
                for h in std::iter::once(home).chain((0..6).map(|d| home.neighbor(d))) {
                    let g = (-p.distance_squared(hex_center(h)) / (2.0 * 0.42 * 0.42)).exp();
                    sum += g * state(h);
                    weight += g;
                }
                sum / weight
            })
        })
        .collect();
    (width, height, data)
}

/// Sets where the fog is heading. A new texture size (another map) starts
/// from full fog.
fn retarget(fade: &mut FogFade, session: &Session) {
    let (width, height, target) = compute(session);
    if (width, height) != (fade.width, fade.height) {
        fade.current = vec![0.0; target.len()];
        fade.hold = FIRST_REVEAL_DELAY;
    }
    *fade = FogFade { width, height, target, current: std::mem::take(&mut fade.current), hold: fade.hold, settled: false };
}

/// Moves the shown fog towards its target and uploads it while it changes.
fn fade(time: Res<Time>, fog: Res<FogOfWar>, mut fade: ResMut<FogFade>, mut images: ResMut<Assets<Image>>) {
    if fade.settled || fade.target.is_empty() {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    if fade.hold > 0.0 {
        fade.hold -= dt;
    } else {
        let (clear, cover) = (CLEAR_RATE * dt, COVER_RATE * dt);
        let FogFade { target, current, .. } = &mut *fade;
        let moving = current
            .par_iter_mut()
            .zip(target.par_iter())
            .map(|(c, &t)| {
                if *c < t {
                    *c = (*c + clear).min(t);
                } else if *c > t {
                    *c = (*c - cover).max(t);
                }
                (*c != t) as u32
            })
            .sum::<u32>();
        fade.settled = moving == 0;
    }
    let data: Vec<u8> = fade.current.par_iter().map(|v| (v * 255.0).round() as u8).collect();
    let (width, height) = (fade.width, fade.height);
    let Some(mut image) = images.get_mut(&fog.image) else { return };
    if image.width() == width && image.height() == height {
        image.data = Some(data);
    } else {
        let mut fresh = Image::new(
            Extent3d { width, height, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            TextureFormat::R8Unorm,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        fresh.sampler = sampler();
        *image = fresh;
    }
}

/// Whether any tile overlapping the rectangle (plus a margin) has been explored.
fn explored_in(session: &Session, min: Vec2, max: Vec2) -> bool {
    let game = &session.game;
    let explored = &game.players[session.me as usize].explored;
    let (lo, hi) = (min - Vec2::splat(1.5), max + Vec2::splat(1.5));
    let mut y = lo.y;
    while y <= hi.y {
        let mut x = lo.x;
        while x <= hi.x {
            if game.map.grid.tile(hex_at(Vec2::new(x, y))).is_some_and(|t| explored[t as usize]) {
                return true;
            }
            x += 0.8;
        }
        y += 0.8;
    }
    false
}

/// Wakes up entities whose area has been explored (and puts others to sleep).
fn update_footprints(
    commands: &mut Commands,
    session: &Session,
    entities: &Query<(Entity, &Footprint, Has<crate::lod::Dormant>, Has<bevy::light::NotShadowCaster>)>,
) {
    for (entity, fp, dormant, no_shadow) in entities {
        let known = explored_in(session, fp.min, fp.max);
        let mut e = commands.entity(entity);
        if fp.hide {
            match (known, dormant) {
                (true, true) => {
                    e.remove::<crate::lod::Dormant>();
                }
                (false, false) => {
                    e.insert(crate::lod::Dormant);
                }
                _ => {}
            }
        } else {
            match (known, no_shadow) {
                (true, true) => {
                    e.remove::<bevy::light::NotShadowCaster>();
                }
                (false, false) => {
                    e.insert(bevy::light::NotShadowCaster);
                }
                _ => {}
            }
        }
    }
}

fn refresh_now(
    mut commands: Commands,
    session: Res<Session>,
    entities: Query<(Entity, &Footprint, Has<crate::lod::Dormant>, Has<bevy::light::NotShadowCaster>)>,
) {
    // Every game starts from full fog.
    let mut fade = FogFade::default();
    retarget(&mut fade, &session);
    commands.insert_resource(fade);
    update_footprints(&mut commands, &session, &entities);
}

fn refresh_on_change(
    mut commands: Commands,
    mut fade: ResMut<FogFade>,
    session: Res<Session>,
    mut events: MessageReader<GameEvent>,
    entities: Query<(Entity, &Footprint, Has<crate::lod::Dormant>, Has<bevy::light::NotShadowCaster>)>,
    added: Query<(), Added<Footprint>>,
) {
    let me = session.me;
    let changed = events.read().any(|GameEvent(e)| match e {
        Event::Explored { player, .. } | Event::VisibilityChanged { player } => *player == me,
        _ => false,
    });
    // Entities spawned this frame need their first check too.
    let changed = changed || !added.is_empty();
    if changed {
        retarget(&mut fade, &session);
        update_footprints(&mut commands, &session, &entities);
    }
}

#[cfg(test)]
mod tests {
    /// The fog code between its header and the end of `apply_fog`.
    fn fog_block(shader: &str) -> &str {
        let start = shader.find("// --- Fog of war").expect("fog block");
        let apply = start + shader[start..].find("fn apply_fog").expect("apply_fog");
        let end = apply + shader[apply..].find("\n}\n").expect("end of apply_fog") + 3;
        &shader[start..end]
    }

    /// The weather code between its header and the end of `weather_at`.
    fn weather_block(shader: &str) -> &str {
        let start = shader.find("// --- Weather").expect("weather block");
        let at = start + shader[start..].find("fn weather_at").expect("weather_at");
        let end = at + shader[at..].find("\n}\n").expect("end of weather_at") + 3;
        &shader[start..end]
    }

    #[test]
    fn shaders_share_weather_code() {
        let terrain = weather_block(include_str!("shaders/terrain.wesl"));
        assert_eq!(terrain, weather_block(include_str!("shaders/water.wesl")));
        assert_eq!(terrain, weather_block(include_str!("shaders/vegetation.wesl")));
    }

    #[test]
    fn shaders_share_fog_code() {
        let terrain = fog_block(include_str!("shaders/terrain.wesl"));
        assert_eq!(terrain, fog_block(include_str!("shaders/water.wesl")));
        assert_eq!(terrain, fog_block(include_str!("shaders/vegetation.wesl")));
        assert!(terrain.contains("fn apply_fog"));
    }
}
