//! Life on the water: fish leaping (more over fishing grounds), whales that
//! surface in the open ocean, blow and dive with a flick of their flukes,
//! and fishing boats that row out from coastal towns in the morning, fish
//! the day and come home in the evening.

use super::herds::Actor;
use super::puffs::{Puff, Puffs, Shape};
use super::{scenery_prop, smooth, spot, AmbienceSet, Dice, Stage};
use crate::layout::tile_center;
use crate::loading::InGameSet;
use crate::play::Session;
use crate::season::Season;
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::wrap::{nearest_shift, WrapPeriod};
use bevy::prelude::*;
use rand::Rng;
use sim::city::CityId;
use std::collections::HashMap;

pub fn plugin(app: &mut App) {
    app.init_resource::<SeaProps>()
        .add_systems(OnEnter(crate::loading::AppState::InGame), make_props)
        .add_systems(Update, (spawn_leaps, leap, spawn_whales, swim, fishing_fleet, row).in_set(InGameSet).in_set(AmbienceSet::Emit));
}

#[derive(Resource, Default)]
struct SeaProps {
    fish: Handle<Mesh>,
    whale: Handle<Mesh>,
    fluke: Handle<Mesh>,
}

fn make_props(mut props: ResMut<SeaProps>, mut meshes: ResMut<Assets<Mesh>>) {
    if props.fish != Handle::default() {
        return;
    }
    *props = SeaProps { fish: meshes.add(scenery_prop("leaping_fish")), whale: meshes.add(scenery_prop("whale_body")), fluke: meshes.add(scenery_prop("whale_fluke")) };
}

/// Spray thrown up where something breaks the water.
fn splash(puffs: &mut Puffs, rng: &mut impl Rng, at: Vec3, n: usize, power: f32) {
    for _ in 0..n {
        let a = Vec2::from_angle(rng.gen_range(0.0..std::f32::consts::TAU)) * rng.gen_range(0.05..0.25) * power;
        puffs.add(
            Puff::new(Shape::Drop, at, rng.gen_range(0.4..0.7), 0.006 * power.sqrt(), Vec4::new(0.92, 0.96, 1.0, 0.9))
                .vel(Vec3::new(a.x, rng.gen_range(0.35..0.7) * power, a.y))
                .accel(Vec3::Y * -2.4)
                .fade(0.0, 0.3),
        );
    }
    puffs.add(Puff::new(Shape::Smoke, at + Vec3::Y * 0.005, 0.9, 0.012 * power, Vec4::new(0.95, 0.97, 1.0, 0.45)).grow(0.05 * power).fade(0.0, 0.7));
}

/// A fish in the air: out of the water and back in a short arc.
#[derive(Component)]
struct Leap {
    from: Vec3,
    dir: Vec2,
    age: f32,
}

const LEAP_TIME: f32 = 0.65;

fn spawn_leaps(
    mut commands: Commands,
    stage: Res<Stage>,
    mut dice: ResMut<Dice>,
    mut puffs: ResMut<Puffs>,
    props: Res<SeaProps>,
    session: Res<Session>,
    season: Res<Season>,
    terrain: Res<Terrain>,
    scenery: Res<crate::vegetation::Scenery>,
) {
    let close = stage.close(20.0, 34.0);
    if !stage.on || close <= 0.0 {
        return;
    }
    let rng = &mut dice.0;
    let game = &session.game;
    for _ in 0..stage.spawns(stage.area() * 0.03 * close, rng) {
        let Some(s) = spot(game, &season, &terrain, session.me, stage.point(rng)) else { continue };
        if !s.water || s.frozen || !s.visible {
            continue;
        }
        // Mostly over fishing grounds and in shallow water.
        let fish = game.visible_resource(session.me, s.tile).is_some_and(|r| game.rules.resource(r).id == "fish");
        if !rng.gen_bool(if fish { 1.0 } else if s.deep { 0.08 } else { 0.25 }) {
            continue;
        }
        let from = Vec3::new(s.p.x, WATER_LEVEL, s.p.y);
        let dir = Vec2::from_angle(rng.gen_range(0.0..std::f32::consts::TAU));
        splash(&mut puffs, rng, from, 8, 0.6);
        commands.spawn((
            Leap { from, dir, age: 0.0 },
            Mesh3d(props.fish.clone()),
            MeshMaterial3d(scenery.0.clone()),
            Transform::from_translation(from).with_scale(Vec3::splat(3.0)),
        ));
    }
}

fn leap(mut commands: Commands, stage: Res<Stage>, mut dice: ResMut<Dice>, mut puffs: ResMut<Puffs>, mut fish: Query<(Entity, &mut Leap, &mut Transform)>) {
    for (e, mut l, mut tf) in &mut fish {
        l.age += stage.dt;
        let u = l.age / LEAP_TIME;
        if u >= 1.0 {
            let at = l.from + Vec3::new(l.dir.x, 0.0, l.dir.y) * 0.14;
            splash(&mut puffs, &mut dice.0, at, 10, 0.7);
            commands.entity(e).despawn();
            continue;
        }
        let h = 4.0 * u * (1.0 - u) * 0.09;
        let along = l.dir * 0.14 * u;
        tf.translation = l.from + Vec3::new(along.x, h - 0.01, along.y);
        // Nose up on the way out, down on the way in.
        let pitch = (1.0 - 2.0 * u) * 0.9;
        // Props face +Z; rotating about X by a negative angle lifts the nose.
        tf.rotation = Quat::from_rotation_y(l.dir.x.atan2(l.dir.y)) * Quat::from_rotation_x(-pitch);
    }
}

/// A whale passing: it rises, swims along blowing now and then, and dives.
#[derive(Component)]
struct Whale {
    from: Vec2,
    dir: Vec2,
    age: f32,
    life: f32,
    blows: Vec<f32>,
    fluke: Entity,
}

const WHALE_SPEED: f32 = 0.12;

fn spawn_whales(
    mut commands: Commands,
    stage: Res<Stage>,
    mut dice: ResMut<Dice>,
    props: Res<SeaProps>,
    session: Res<Session>,
    season: Res<Season>,
    terrain: Res<Terrain>,
    scenery: Res<crate::vegetation::Scenery>,
    whales: Query<(), With<Whale>>,
) {
    let rng = &mut dice.0;
    if !stage.on || stage.distance > 50.0 || whales.iter().count() >= 2 || !rng.gen_bool((stage.dt / 25.0).min(1.0) as f64) {
        return;
    }
    let Some(s) = spot(&session.game, &season, &terrain, session.me, stage.point(rng)) else { return };
    // Deep, open water, and room to swim.
    let dir = Vec2::from_angle(rng.gen_range(0.0..std::f32::consts::TAU));
    let life = rng.gen_range(14.0..20.0);
    let deep = |p: Vec2| terrain.heights.sample(p) < WATER_LEVEL - 0.15;
    if !s.deep || s.frozen || !s.visible || !(0..=4).all(|i| deep(s.p + dir * WHALE_SPEED * life * i as f32 / 4.0)) {
        return;
    }
    let fluke = commands.spawn((Mesh3d(props.fluke.clone()), MeshMaterial3d(scenery.0.clone()), Transform::IDENTITY)).id();
    let blows = vec![rng.gen_range(2.5..4.0), rng.gen_range(7.0..9.0)];
    commands
        .spawn((
            Whale { from: s.p, dir, age: 0.0, life, blows, fluke },
            Mesh3d(props.whale.clone()),
            MeshMaterial3d(scenery.0.clone()),
            Transform::from_xyz(s.p.x, -1.0, s.p.y).with_scale(Vec3::splat(3.0)),
        ))
        .add_child(fluke);
}

fn swim(
    mut commands: Commands,
    stage: Res<Stage>,
    mut dice: ResMut<Dice>,
    mut puffs: ResMut<Puffs>,
    period: Res<WrapPeriod>,
    camera: Single<&GlobalTransform, With<crate::camera::MainCamera>>,
    mut whales: Query<(Entity, &mut Whale, &mut Transform)>,
    mut flukes: Query<&mut Transform, Without<Whale>>,
) {
    for (e, mut w, mut tf) in &mut whales {
        w.age += stage.dt;
        if w.age >= w.life {
            commands.entity(e).despawn();
            continue;
        }
        w.from.x += nearest_shift(w.from.x, camera.translation().x, period.0);
        let (age, life) = (w.age, w.life);
        let p = w.from + w.dir * WHALE_SPEED * age;
        // Its back rises over 2 s; it dives over the last 4 s, nose first,
        // its flukes lifting clear before they slide under.
        let rise = smooth(age / 2.0);
        let dive = smooth((age - (life - 4.0)) / 3.0);
        let depth = -0.07 + 0.065 * rise - 0.12 * smooth((age - (life - 2.0)) / 2.0);
        let pitch = dive * 0.45;
        tf.translation = Vec3::new(p.x, depth, p.y);
        tf.rotation = Quat::from_rotation_y(w.dir.x.atan2(w.dir.y)) * Quat::from_rotation_x(pitch) * Quat::from_rotation_z((age * 0.6).sin() * 0.03);
        if let Ok(mut f) = flukes.get_mut(w.fluke) {
            // At the tail stock (6 m behind the middle at the body's scale),
            // beating slowly and lifting as it dives.
            let beat = (age * 1.4).sin() * 0.15 * (1.0 - dive);
            *f = Transform::from_xyz(0.0, 0.0, -6.0 * 0.021 * 0.8).with_rotation(Quat::from_rotation_x(beat + dive * 1.1));
        }
        let rng = &mut dice.0;
        let blows: Vec<f32> = w.blows.clone();
        for b in blows {
            if age >= b && age - stage.dt < b {
                w.blows.retain(|x| *x != b);
            }
            // A blow: a column of spray from the blowhole drifting off as mist.
            if age >= b && age < b + 0.8 {
                let head = tf.translation + tf.rotation * Vec3::new(0.0, 0.05, 0.17);
                for _ in 0..stage.spawns(70.0, rng) {
                    let spread = Vec2::from_angle(rng.gen_range(0.0..6.3)) * rng.gen_range(0.0..0.06);
                    puffs.add(
                        Puff::new(Shape::Drop, head, rng.gen_range(0.7..1.1), 0.007, Vec4::new(0.95, 0.97, 1.0, 0.85))
                            .vel(Vec3::new(spread.x, rng.gen_range(0.5..0.75), spread.y))
                            .accel(Vec3::Y * -0.9)
                            .drag(0.6)
                            .fade(0.0, 0.4),
                    );
                }
                for _ in 0..stage.spawns(12.0, rng) {
                    puffs.add(
                        Puff::new(Shape::Smoke, head + Vec3::Y * rng.gen_range(0.05..0.25), rng.gen_range(1.5..2.5), 0.03, Vec4::new(0.95, 0.97, 1.0, 0.35))
                            .grow(0.1)
                            .vel(Vec3::new(crate::weather::WIND.x, 0.05, crate::weather::WIND.y) * 0.3)
                            .fade(0.1, 0.6),
                    );
                }
            }
        }
        // White water where the back breaks the surface.
        if rise > 0.3 && dive < 0.9 && dice.0.gen_bool((stage.dt * 3.0).min(1.0) as f64) {
            let at = Vec3::new(p.x, WATER_LEVEL, p.y) + Vec3::new(w.dir.x, 0.0, w.dir.y) * dice.0.gen_range(-0.1..0.1);
            splash(&mut puffs, &mut dice.0, at, 2, 0.5);
        }
    }
}

/// A boat fishing for a coastal town: out in the morning, home at dusk.
#[derive(Component)]
struct FishingBoat {
    city: CityId,
    /// The water by the town, and the fishing ground.
    harbour: Vec2,
    ground: Vec2,
    pos: Vec2,
    yaw: f32,
}

impl FishingBoat {
    /// Where it should be at time of day `day`: rowing out, drifting about
    /// the fishing ground, or rowing home.
    fn target(&self, day: f32, t: f32) -> Vec2 {
        if day < SAIL[1] {
            self.harbour.lerp(self.ground, smooth((day - SAIL[0]) / (SAIL[1] - SAIL[0])))
        } else if day < SAIL[2] {
            self.ground + Vec2::new((t * 0.05).sin(), (t * 0.037).cos()) * 0.12
        } else {
            self.ground.lerp(self.harbour, smooth((day - SAIL[2]) / (SAIL[3] - SAIL[2])))
        }
    }
}

/// A fishing boat's scale: about a third of a tile long.
const BOAT_SIZE: f32 = 0.55;

/// Times of day: setting out, arriving, leaving for home, home.
const SAIL: [f32; 4] = [0.08, 0.15, 0.48, 0.56];

fn fishing_fleet(
    mut commands: Commands,
    stage: Res<Stage>,
    mut dice: ResMut<Dice>,
    session: Res<Session>,
    terrain: Res<Terrain>,
    boats: Query<(Entity, &FishingBoat)>,
) {
    let game = &session.game;
    let me = &game.players[session.me as usize];
    let out = stage.on && stage.day > SAIL[0] && stage.day < SAIL[3];
    // Boats leave when their town is out of sight or the day is over.
    let mut afloat: HashMap<CityId, usize> = HashMap::new();
    for (e, b) in &boats {
        let keep = out && game.cities.get(&b.city).is_some_and(|c| stage.sees(tile_center(&game.map.grid, c.tile), 6.0));
        if keep {
            *afloat.entry(b.city).or_default() += 1;
        } else {
            commands.entity(e).despawn();
        }
    }
    if !out || stage.distance > 55.0 {
        return;
    }
    let rng = &mut dice.0;
    for c in game.cities.values() {
        let centre = tile_center(&game.map.grid, c.tile);
        if afloat.contains_key(&c.id) || !me.explored[c.tile as usize] || !stage.sees(centre, 4.0) {
            continue;
        }
        // Coastal towns: water beside the town and a fishing ground nearby.
        let water = |t: sim::hex::TileId| game.map.is_water(&game.rules, t) && game.sea_ice(t) == sim::climate::SeaIce::Open;
        let harbours: Vec<_> = game.map.grid.neighbors(c.tile).filter(|t| water(*t)).collect();
        if harbours.is_empty() {
            continue;
        }
        let grounds: Vec<_> = game.map.grid.within(c.tile, 3).into_iter().filter(|t| water(*t) && me.explored[*t as usize]).collect();
        let n = if c.population >= 6 { 2 } else { 1 };
        for i in 0..n {
            let harbour = tile_center(&game.map.grid, harbours[rng.gen_range(0..harbours.len())]);
            let harbour = harbour + (centre - harbour) * 0.15;
            // A ground the boat can reach in a straight line over water.
            let fish = grounds.iter().copied().filter(|t| game.map.tile(*t).resource.is_some_and(|r| game.rules.resource(r).id == "fish"));
            let pick = fish.chain(grounds.iter().copied()).skip(i).find(|t| {
                let to = tile_center(&game.map.grid, *t);
                (1..10).all(|k| terrain.heights.sample(harbour + (to - harbour) * k as f32 / 10.0) < WATER_LEVEL - 0.02)
            });
            let Some(t) = pick else { continue };
            let ground = tile_center(&game.map.grid, t) + Vec2::from_angle(rng.gen_range(0.0..6.3)) * 0.25;
            let mut boat = FishingBoat { city: c.id, harbour, ground, pos: harbour, yaw: 0.0 };
            // Already where it would be by now.
            boat.pos = boat.target(stage.day, 0.0);
            let towards = boat.ground - boat.harbour;
            boat.yaw = towards.x.atan2(towards.y);
            commands.spawn((
                boat,
                Actor::new("fishing_boat", BOAT_SIZE, Vec4::ONE, rng.gen_range(0.0..30.0)),
                Transform::from_xyz(harbour.x, WATER_LEVEL, harbour.y).with_scale(Vec3::splat(BOAT_SIZE)),
                Visibility::default(),
            ));
        }
    }
}

fn row(time: Res<Time>, stage: Res<Stage>, period: Res<WrapPeriod>, camera: Single<&GlobalTransform, With<crate::camera::MainCamera>>, mut boats: Query<(&mut FishingBoat, &mut Actor, &mut Transform)>) {
    let t = time.elapsed_secs();
    let day = stage.day;
    for (mut b, mut actor, mut tf) in &mut boats {
        let shift = nearest_shift(b.pos.x, camera.translation().x, period.0);
        b.pos.x += shift;
        b.harbour.x += shift;
        b.ground.x += shift;
        let target = b.target(day, t);
        let delta = target - b.pos;
        let step = delta.length().min(0.2 * stage.dt);
        if delta.length() > 0.002 {
            let want = delta.x.atan2(delta.y);
            let turn = (want - b.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            b.yaw += turn.clamp(-1.5 * stage.dt, 1.5 * stage.dt);
            let d = delta.normalize() * step;
            b.pos += d;
        }
        actor.walk(step, step > 0.0005, stage.dt, t);
        tf.translation = Vec3::new(b.pos.x, WATER_LEVEL - 0.005, b.pos.y);
        tf.rotation = Quat::from_rotation_y(b.yaw);
    }
}
