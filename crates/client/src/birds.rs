//! Flocks of birds that now and then fly across the view, for atmosphere:
//! geese in a V, loose flocks of small dark birds weaving about, gulls gliding
//! over the sea. Visual only. They fly high above the ground (their shadows
//! cross the land below), keep to explored land, and stay away at night and
//! in storms.

use crate::camera::{CameraRig, MainCamera};
use crate::layout::{hex_at, map_bounds};
use crate::loading::{AppState, InGameSet};
use crate::play::Session;
use crate::procmesh::{blob, cylinder, lin, Model};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::wrap::nearest_shift;
use crate::GameMap;
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Seconds between flocks (a random time in this range).
const INTERVAL: (f32, f32) = (35.0, 80.0);
/// The first flock of a game comes sooner.
const FIRST: (f32, f32) = (12.0, 25.0);
/// No flocks with the camera pulled back further than this (they'd be specks).
const MAX_DISTANCE: f32 = 60.0;
/// Height above the ground the flocks cruise at.
const CRUISE: f32 = 2.6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Goose,
    Starling,
    Gull,
}

/// How each kind looks and flies.
struct Style {
    count: (usize, usize),
    speed: f32,
    /// Wingbeats per second and their amplitude (radians).
    beat: f32,
    sweep: f32,
    /// How much of the time the birds glide (0..1).
    glide: f32,
    /// How far each bird wanders from its place in the flock.
    wander: f32,
    /// Length of the inner and outer wing.
    wing: (f32, f32),
}

impl Kind {
    fn style(self) -> Style {
        match self {
            Kind::Goose => Style { count: (7, 15), speed: 2.6, beat: 2.6, sweep: 0.55, glide: 0.1, wander: 0.04, wing: (0.09, 0.1) },
            Kind::Starling => Style { count: (14, 28), speed: 3.4, beat: 6.0, sweep: 0.75, glide: 0.35, wander: 0.35, wing: (0.058, 0.068) },
            Kind::Gull => Style { count: (3, 7), speed: 2.0, beat: 2.0, sweep: 0.45, glide: 0.65, wander: 0.5, wing: (0.08, 0.1) },
        }
    }
}

/// When the next flock comes.
#[derive(Resource)]
struct Skyward {
    next: f32,
    rng: ChaCha8Rng,
}

/// Meshes per kind (body, inner wing, outer wing), and their one material.
#[derive(Resource)]
struct BirdAssets {
    meshes: Vec<(Kind, [Handle<Mesh>; 3])>,
    material: Handle<StandardMaterial>,
}

impl BirdAssets {
    fn meshes(&self, kind: Kind) -> &[Handle<Mesh>; 3] {
        &self.meshes.iter().find(|m| m.0 == kind).unwrap().1
    }
}

/// A flock flying straight across the view, curving gently.
#[derive(Component)]
struct Flock {
    kind: Kind,
    start: Vec2,
    dir: Vec2,
    /// How far it flies before it's gone, and how far it has come.
    length: f32,
    travelled: f32,
    /// How much it swings sideways, and how fast.
    swing: f32,
    swing_rate: f32,
    altitude: f32,
}

#[derive(Component)]
struct Bird {
    /// Place in the flock (x right, z behind the leader).
    slot: Vec3,
    phase: f32,
    /// Each bird beats a little faster or slower than the others.
    rate: f32,
    /// Shown (1) or faded out over unexplored land (0).
    shown: f32,
}

/// A wing's hinge: the inner wing on the body, the outer on the inner wing.
#[derive(Component)]
struct Wing {
    left: bool,
}

pub struct BirdsPlugin;

impl Plugin for BirdsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create)
            .add_systems(OnEnter(AppState::InGame), restart)
            .add_systems(Update, (spawn_flocks, fly, flap).chain().in_set(InGameSet));
    }
}

fn create(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let meshes = [Kind::Goose, Kind::Starling, Kind::Gull]
        .into_iter()
        .map(|kind| {
            let [body, inner, outer] = bird_models(kind);
            (kind, [meshes.add(body.into_mesh()), meshes.add(inner.into_mesh()), meshes.add(outer.into_mesh())])
        })
        .collect();
    commands.insert_resource(BirdAssets { meshes, material });
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64);
    commands.insert_resource(Skyward { next: 0.0, rng: ChaCha8Rng::seed_from_u64(seed) });
}

fn restart(mut sky: ResMut<Skyward>) {
    sky.next = if dev().is_some() { 0.0 } else { sky.rng.gen_range(FIRST.0..FIRST.1) };
}

/// `FOURX_BIRDS=goose|starling|gull|1` keeps a flock (of that kind) passing
/// over the camera's focus all the time, for screenshots.
fn dev() -> Option<Option<Kind>> {
    let v = std::env::var("FOURX_BIRDS").ok()?;
    Some(match v.as_str() {
        "goose" => Some(Kind::Goose),
        "starling" => Some(Kind::Starling),
        "gull" => Some(Kind::Gull),
        _ => None,
    })
}

/// A body (facing -Z, like Bevy's forward), and a right wing in two parts
/// hinged on the Z axis and reaching along +X (the left one is mirrored).
fn bird_models(kind: Kind) -> [Model; 3] {
    let (body_c, wing_c, tip_c, head_c) = match kind {
        Kind::Goose => (lin(122, 110, 96), lin(96, 86, 76), lin(58, 52, 48), lin(30, 28, 28)),
        Kind::Starling => (lin(62, 60, 70), lin(54, 52, 62), lin(38, 37, 44), lin(56, 54, 64)),
        Kind::Gull => (lin(236, 236, 232), lin(178, 186, 194), lin(36, 36, 38), lin(240, 240, 236)),
    };
    let Style { wing: (inner_len, outer_len), .. } = kind.style();
    let s = inner_len / 0.09;
    let mut body = Model::default();
    let squash = |x, y, z| Transform::from_scale(Vec3::new(x, y, z));
    body.add(blob(1.0, 1), body_c, squash(0.022 * s, 0.02 * s, 0.07 * s));
    match kind {
        Kind::Goose => {
            // A long dark neck and head, a white chin strap left out (too small to see).
            let neck = Transform::from_xyz(0.0, 0.006 * s, -0.08 * s).with_rotation(Quat::from_rotation_x(-1.45));
            body.add(cylinder(0.008 * s, 0.07 * s, 5), head_c, neck);
            body.add(blob(1.0, 1), head_c, Transform::from_xyz(0.0, 0.009 * s, -0.12 * s).with_scale(Vec3::new(0.011, 0.01, 0.018) * s));
        }
        _ => body.add(blob(1.0, 1), head_c, Transform::from_xyz(0.0, 0.006 * s, -0.065 * s).with_scale(Vec3::splat(0.016 * s))),
    }
    // A flat, fanned tail.
    body.add(Cuboid::new(0.045 * s, 0.004 * s, 0.04 * s).mesh().build(), wing_c, Transform::from_xyz(0.0, 0.0, 0.075 * s));
    // Wings: a broad inner part and a pointed, swept-back outer part.
    let inner = flat(&[(0.0, -0.035), (inner_len, -0.03), (inner_len, 0.03), (0.0, 0.04)].map(|(x, z)| (x, z * s)), wing_c);
    let outer = flat(&[(0.0, -0.03 * s), (outer_len * 0.75, -0.012 * s), (outer_len, 0.03 * s), (0.0, 0.03 * s)], tip_c);
    [body, inner, outer]
}

/// A flat quad in the XZ plane (drawn from both sides).
fn flat(points: &[(f32, f32); 4], color: Vec4) -> Model {
    let positions = points.iter().map(|&(x, z)| Vec3::new(x, 0.0, z)).collect();
    Model::from_parts(positions, vec![Vec3::Y; 4], vec![color; 4], vec![0, 2, 1, 0, 3, 2])
}

fn spawn_flocks(
    mut commands: Commands,
    time: Res<Time>,
    mut sky: ResMut<Skyward>,
    assets: Res<BirdAssets>,
    rig: Res<CameraRig>,
    session: Res<Session>,
    game: Res<GameMap>,
    terrain: Res<Terrain>,
    clock: Res<crate::daylight::DayClock>,
    weather: Res<crate::weather::LocalWeather>,
    view: Res<crate::strategic::StrategicView>,
    flocks: Query<(), With<Flock>>,
) {
    sky.next -= time.delta_secs();
    if sky.next > 0.0 {
        return;
    }
    sky.next = if dev().is_some() { 0.0 } else { sky.rng.gen_range(INTERVAL.0..INTERVAL.1) };
    let focus = rig.focus;
    let calm = crate::daylight::night(clock.t) < 0.5 && weather.storm < 0.15;
    let explored = |p: Vec2| explored(&session, &game, p);
    if !flocks.is_empty() || !calm || view.0 || rig.distance > MAX_DISTANCE || !explored(focus) {
        return;
    }
    let rng = &mut sky.rng;
    // Gulls over the sea and along coasts, geese and starlings inland.
    let sea = (0..6).filter(|i| {
        let a = *i as f32 * std::f32::consts::TAU / 6.0;
        terrain.heights.sample(focus + Vec2::from_angle(a) * 3.0) <= WATER_LEVEL
    });
    let kind = match (sea.count(), rng.gen_range(0..10)) {
        _ if let Some(Some(kind)) = dev() => kind,
        (2.., 0..7) => Kind::Gull,
        (_, 0..4) => Kind::Goose,
        _ => Kind::Starling,
    };
    let style = kind.style();
    // In one side of the view and out of the other, passing near the middle.
    let dir = Vec2::from_angle(rng.gen_range(0.0..std::f32::consts::TAU));
    let reach = rig.distance * 0.75 + 6.0;
    let side = dir.perp() * rng.gen_range(-0.3..0.3) * rig.distance;
    let start = focus - dir * reach + side;
    let count = rng.gen_range(style.count.0..=style.count.1);
    let mut flock = Flock {
        kind,
        start,
        dir,
        length: reach * 2.0,
        travelled: 0.0,
        swing: rng.gen_range(0.5..2.0),
        swing_rate: rng.gen_range(0.08..0.18),
        altitude: terrain.heights.sample(start).max(WATER_LEVEL) + CRUISE,
    };
    if dev().is_some() {
        // Only the few seconds around the focus.
        flock.travelled = reach - style.speed * 2.0;
        flock.length = reach + style.speed * 2.0;
        flock.swing = 0.0;
        flock.start -= side;
    }
    let [body, inner, outer] = assets.meshes(kind).clone();
    let mat = assets.material.clone();
    let slots = formation(kind, count, rng);
    commands.spawn((flock, Transform::from_xyz(start.x, 0.0, start.y), Visibility::default())).with_children(|f| {
        for slot in slots {
            let bird = Bird { slot, phase: rng.gen_range(0.0..std::f32::consts::TAU), rate: rng.gen_range(0.9..1.1), shown: 0.0 };
            f.spawn((bird, Transform::from_translation(slot).with_scale(Vec3::ZERO), Visibility::default())).with_children(|b| {
                b.spawn((Mesh3d(body.clone()), MeshMaterial3d(mat.clone()), Transform::default()));
                for left in [false, true] {
                    let mirror = Vec3::new(if left { -1.0 } else { 1.0 }, 1.0, 1.0);
                    let hinge = Vec3::new(if left { -0.012 } else { 0.012 }, 0.008, -0.005) * (style.wing.0 / 0.09);
                    b.spawn((Wing { left }, Mesh3d(inner.clone()), MeshMaterial3d(mat.clone()), Transform::from_translation(hinge).with_scale(mirror)))
                        .with_child((
                            Wing { left },
                            Mesh3d(outer.clone()),
                            MeshMaterial3d(mat.clone()),
                            // In the inner wing's (mirrored) frame.
                            Transform::from_xyz(style.wing.0, 0.0, 0.0),
                        ));
                }
            });
        }
    });
}

/// Where each bird flies in the flock (x to the right, z behind the leader).
fn formation(kind: Kind, count: usize, rng: &mut ChaCha8Rng) -> Vec<Vec3> {
    (0..count)
        .map(|i| match kind {
            // A V, one arm usually longer than the other.
            Kind::Goose => {
                let rank = i.div_ceil(2) as f32;
                let side = if i % 2 == 0 { 1.0 } else { -1.0 };
                Vec3::new(side * rank * 0.22, rank * 0.015, rank * 0.24) + jitter(rng, 0.03)
            }
            // A loose cloud, denser in the middle.
            Kind::Starling => {
                let r = rng.gen_range(0.0f32..1.0).sqrt() * (0.5 + count as f32 * 0.04);
                let a = rng.gen_range(0.0..std::f32::consts::TAU);
                Vec3::new(a.cos() * r, rng.gen_range(-0.25..0.25), a.sin() * r * 1.4)
            }
            Kind::Gull => Vec3::new(rng.gen_range(-1.2..1.2), rng.gen_range(-0.3..0.3), rng.gen_range(-1.0..1.6)),
        })
        .collect()
}

fn jitter(rng: &mut ChaCha8Rng, r: f32) -> Vec3 {
    Vec3::new(rng.gen_range(-r..r), rng.gen_range(-r..r), rng.gen_range(-r..r))
}

fn explored(session: &Session, game: &GameMap, p: Vec2) -> bool {
    let me = &session.game.players[session.me as usize];
    game.map.grid.tile(hex_at(p)).is_some_and(|t| me.explored[t as usize])
}

fn fly(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<Session>,
    game: Res<GameMap>,
    terrain: Res<Terrain>,
    camera: Single<&GlobalTransform, With<MainCamera>>,
    mut flocks: Query<(Entity, &mut Flock, &mut Transform, &Children), Without<Bird>>,
    mut birds: Query<(&mut Bird, &mut Transform), Without<Flock>>,
) {
    let dt = time.delta_secs().min(0.1);
    let t = time.elapsed_secs();
    let (min, max) = map_bounds(&game.map.grid);
    let period = max.x - min.x;
    for (entity, mut flock, mut tf, children) in &mut flocks {
        let style = flock.kind.style();
        flock.travelled += style.speed * dt;
        if flock.travelled > flock.length {
            commands.entity(entity).despawn();
            continue;
        }
        // The camera's focus wraps round the map: follow it.
        flock.start.x += nearest_shift(flock.start.x, camera.translation().x, period);
        let at = |d: f32| {
            let sway = (d * flock.swing_rate).sin() * flock.swing;
            flock.start + flock.dir * d + flock.dir.perp() * sway
        };
        let (d, here) = (flock.travelled, at(flock.travelled));
        let heading = (at(d + 0.5) - here).normalize_or(flock.dir);
        // Climb over hills ahead, sink slowly back after them.
        let ground = [0.0, 2.0, 4.0].map(|a| terrain.heights.sample(at(d + a)).max(WATER_LEVEL)).into_iter().fold(f32::MIN, f32::max);
        let target = ground + CRUISE;
        let ease = if target > flock.altitude { 1.2 } else { 0.25 };
        flock.altitude += (target - flock.altitude) * (1.0 - (-ease * dt).exp());
        tf.translation = Vec3::new(here.x, flock.altitude, here.y);
        tf.rotation = Quat::from_rotation_y((-heading.x).atan2(-heading.y));
        // Fade in at the ends of the trip, and out over unexplored land.
        let ends = (d / 2.0).min((flock.length - d) / 2.0).clamp(0.0, 1.0);
        for &child in children {
            let Ok((mut bird, mut btf)) = birds.get_mut(child) else { continue };
            let p = bird.phase;
            let drift = Vec3::new((t * 0.45 + p).sin(), (t * 0.7 + p * 1.3).sin() * 0.4, (t * 0.33 + p * 0.7).cos()) * style.wander;
            btf.translation = bird.slot + drift + Vec3::Y * (t * style.beat * bird.rate * std::f32::consts::TAU + p).sin() * -0.006;
            let world = here + Vec2::new(btf.translation.x, btf.translation.z);
            let goal = if explored(&session, &game, world) { ends } else { 0.0 };
            bird.shown += (goal - bird.shown) * (1.0 - (-3.0 * dt).exp());
            btf.scale = Vec3::splat(bird.shown);
        }
    }
}

/// Wingbeats, with spells of gliding on held-out wings.
fn flap(
    time: Res<Time>,
    flocks: Query<(&Flock, &Children)>,
    birds: Query<(&Bird, &Children)>,
    outer_wings: Query<&Children, With<Wing>>,
    mut wings: Query<(&Wing, &mut Transform)>,
) {
    let t = time.elapsed_secs();
    for (flock, children) in &flocks {
        let style = flock.kind.style();
        for (bird, parts) in birds.iter_many(children).flatten() {
            let cycle = t * style.beat * bird.rate * std::f32::consts::TAU + bird.phase;
            // Gliding comes and goes slowly, for about `style.glide` of the time.
            let wave = (t * 0.35 + bird.phase * 3.1).sin();
            let glide = smooth((wave - (std::f32::consts::PI * style.glide).cos()) * 3.0);
            let beat = 1.0 - glide;
            let inner = cycle.sin() * style.sweep * beat + 0.12 * glide + 0.08;
            let outer = (cycle - 0.7).sin() * style.sweep * 0.6 * beat - 0.05 * glide;
            for &part in parts {
                let Ok((wing, mut tf)) = wings.get_mut(part) else { continue };
                // The left wing is mirrored on X, so its hinge turns the other way;
                // the outer wing lives in that mirrored frame and needs no sign.
                tf.rotation = Quat::from_rotation_z(if wing.left { -inner } else { inner });
                for &tip in outer_wings.get(part).into_iter().flatten() {
                    if let Ok((_, mut otf)) = wings.get_mut(tip) {
                        otf.rotation = Quat::from_rotation_z(outer);
                    }
                }
            }
        }
    }
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
