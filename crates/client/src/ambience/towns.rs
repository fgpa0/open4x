//! Towns and roads that look lived in: steady plumes of smoke from chimneys
//! (more in the cold, leaning with the wind), torches on town walls, and
//! lamp posts beside every road in a civ's land (from the Classical era on)
//! that light up at night.

use super::puffs::{Puff, Puffs, Shape};
use super::{AmbienceSet, Dice, Stage};
use crate::cities::TownSpots;
use crate::layout::tile_center;
use crate::loading::InGameSet;
use crate::play::Session;
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::weather::WIND;
use crate::wrap::Wrapped;
use bevy::prelude::*;
use rand::Rng;
use sim::hex::TileId;

/// Puffs of smoke a second from each smoking chimney: enough that they
/// overlap into one plume.
const SMOKE_RATE: f32 = 9.0;
/// Chimneys smoking at once in one town at most.
const MAX_CHIMNEYS: usize = 8;
/// The lantern on the `lamp_post` prop, relative to its foot (works.py:
/// 0.45 m out, 2.2 m up, at GEAR * 0.9 and the props' 3x).
const LANTERN: Vec3 = Vec3::new(0.45 * 0.0594, 2.2 * 0.0594, 0.0);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (smoke, lamp_posts, lights).chain().in_set(InGameSet).in_set(AmbienceSet::Emit));
}

fn smoke(stage: Res<Stage>, mut dice: ResMut<Dice>, mut puffs: ResMut<Puffs>, session: Res<Session>, season: Res<crate::season::Season>, towns: Query<(&TownSpots, &Transform)>) {
    if !stage.on || stage.distance > 70.0 {
        return;
    }
    let rng = &mut dice.0;
    for (spots, tf) in &towns {
        let centre = tf.translation;
        if !stage.sees(centre.xz(), 2.0) {
            continue;
        }
        // Cold towns heat every house; warm ones only cook, and fires are
        // banked at night: fewer chimneys smoke.
        let tile = session.game.map.grid.tile(crate::layout::hex_at(centre.xz()));
        let temp = tile.map_or(10.0, |t| crate::season::temperature_now(&session.game, t, &season).0);
        let cold = 1.0 - super::smooth((temp - 4.0) / 18.0) * 0.6;
        let share = cold * (1.0 - 0.35 * stage.night);
        let lit = ((spots.chimneys.len() as f32 * share).ceil() as usize).min(MAX_CHIMNEYS);
        for (i, c) in spots.chimneys.iter().take(lit).enumerate() {
            // A steady stream of small puffs that widen, thin and drift
            // off with the wind as they rise; each chimney draws a little
            // differently.
            let draw = 0.8 + 0.4 * ((i as f32 * 1.7 + centre.x).sin() * 0.5 + 0.5);
            for _ in 0..stage.spawns(SMOKE_RATE * draw, rng) {
                if puffs.room() == 0 {
                    return;
                }
                let at = centre + *c + Vec3::new(rng.gen_range(-0.004..0.004), 0.0, rng.gen_range(-0.004..0.004));
                // Seen from above, a plume reads by trailing off downwind.
                let rise = Vec3::Y * rng.gen_range(0.1..0.13) * draw;
                let grey = rng.gen_range(0.66..0.74);
                puffs.add(
                    Puff::new(Shape::Smoke, at, rng.gen_range(6.0..8.0), 0.01, Vec4::new(grey, grey, grey * 1.02, 0.16))
                        .grow(rng.gen_range(0.07..0.1))
                        .vel(rise + WIND.extend(0.0).xzy() * 0.08)
                        .accel(Vec3::Y * -0.01 + WIND.extend(0.0).xzy() * 0.07)
                        .drag(0.92)
                        .spin(rng.gen_range(0.0..6.3), rng.gen_range(-0.25..0.25))
                        .flutter(0.02)
                        .fade(0.04, 0.7),
                );
            }
        }
    }
}

/// A lamp post by a road near a town.
#[derive(Component)]
struct LampPost {
    lantern: Vec3,
}

/// Where lamp posts stand, rebuilt when roads, towns or eras change.
#[derive(Default)]
struct Posts(Vec<TileId>);

fn lamp_posts(
    mut commands: Commands,
    session: Res<Session>,
    terrain: Res<Terrain>,
    scenery: Res<crate::vegetation::Scenery>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut placed: Local<Posts>,
    mut mesh: Local<Option<Handle<Mesh>>>,
    posts: Query<Entity, With<LampPost>>,
) {
    if !session.is_changed() {
        return;
    }
    let game = &session.game;
    let me = &game.players[session.me as usize];
    // Every road in a civ's land once it has reached the Classical era;
    // roads through the wilds stay dark.
    let tiles: Vec<TileId> = (0..game.map.tiles.len() as TileId)
        .filter(|&t| {
            game.has_road(t)
                && me.explored[t as usize]
                && game.territory_owner(t).is_some_and(|p| game.era(p) >= 1)
                && game.city_at(t).is_none()
                && !game.has_bridge(t)
                && !game.has_tunnel(t)
        })
        .collect();
    if tiles == placed.0 && !session.is_added() {
        return;
    }
    for e in &posts {
        commands.entity(e).despawn();
    }
    let mesh = mesh.get_or_insert_with(|| meshes.add(super::scenery_prop("lamp_post"))).clone();
    for &t in &tiles {
        // Beside the road, off the tile's centre where the roads meet.
        let h = (t as f32 * 0.618).fract() * std::f32::consts::TAU;
        let p = tile_center(&game.map.grid, t) + Vec2::from_angle(h) * 0.16;
        let y = terrain.heights.sample(p).max(WATER_LEVEL);
        let yaw = -h + std::f32::consts::PI;
        let rot = Quat::from_rotation_y(yaw);
        commands.spawn((
            LampPost { lantern: Vec3::new(p.x, y, p.y) + rot * LANTERN },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(scenery.0.clone()),
            Transform::from_xyz(p.x, y, p.y).with_rotation(rot).with_scale(Vec3::splat(3.0)),
            Wrapped { anchor: p.x, base: p.x },
        ));
    }
    placed.0 = tiles;
}

/// Lanterns and torches as glowing puffs, flickering, lit at night.
fn lights(time: Res<Time>, stage: Res<Stage>, mut puffs: ResMut<Puffs>, towns: Query<(&TownSpots, &Transform)>, posts: Query<&LampPost>) {
    let t = time.elapsed_secs();
    puffs.lamps.clear();
    puffs.lamp_level = if stage.on { stage.night } else { 0.0 };
    if puffs.lamp_level < 0.01 {
        return;
    }
    let flicker = |seed: f32, fast: f32| 0.82 + 0.18 * ((t * fast + seed * 40.0).sin() * (t * 7.3 + seed * 13.0).sin());
    let grow = stage.grow().min(2.0);
    let mut lamps = Vec::new();
    for (spots, tf) in &towns {
        if !stage.sees(tf.translation.xz(), 2.0) {
            continue;
        }
        for (i, c) in spots.torches.iter().enumerate() {
            let seed = i as f32 * 0.37 + tf.translation.x;
            let p = tf.translation + *c;
            lamps.push(Puff::new(Shape::Glow, p, 1.0, 0.08 * grow, Vec4::new(1.0, 0.5, 0.16, flicker(seed, 17.0))).fade(0.0, 0.0).seed(seed.fract()));
        }
    }
    for post in &posts {
        if !stage.sees(post.lantern.xz(), 2.0) {
            continue;
        }
        let seed = post.lantern.x * 0.13;
        lamps.push(Puff::new(Shape::Glow, post.lantern, 1.0, 0.07 * grow, Vec4::new(1.0, 0.66, 0.3, flicker(seed, 9.0))).fade(0.0, 0.0).seed(seed.fract()));
    }
    puffs.lamps = lamps;
}
