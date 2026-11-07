//! The land's small motions, each where and when it belongs: mist over
//! water, rivers and marshes at dawn, leaves falling from broadleaf woods in
//! autumn, snow drifting over frozen ground, butterflies over summer
//! meadows by day and fireflies at summer nights, dust devils over hot
//! deserts at midday. Emitters try random points in view and keep the ones
//! that fit, so each follows the land on screen.

use super::puffs::{Puff, Puffs, Shape};
use super::{smooth, spot, AmbienceSet, Dice, Spot, Stage};
use crate::loading::InGameSet;
use crate::play::Session;
use crate::season::Season;
use crate::terrain::Terrain;
use crate::weather::WIND;
use bevy::prelude::*;
use rand::Rng;

pub fn plugin(app: &mut App) {
    app.init_resource::<Devils>().add_systems(Update, (emit, dust_devils).in_set(InGameSet).in_set(AmbienceSet::Emit));
}

/// How misty it is at time of day `t`: from the end of the night until
/// mid-morning, thickest around sunrise.
pub fn morning_mist(t: f32) -> f32 {
    let u = (t + 0.08).fract();
    smooth(u / 0.06) * (1.0 - smooth((u - 0.16) / 0.1))
}

fn wind3(k: f32) -> Vec3 {
    Vec3::new(WIND.x, 0.0, WIND.y) * k
}

fn emit(stage: Res<Stage>, mut dice: ResMut<Dice>, mut puffs: ResMut<Puffs>, session: Res<Session>, season: Res<Season>, terrain: Res<Terrain>) {
    if !stage.on {
        return;
    }
    let game = &session.game;
    let rng = &mut dice.0;
    let area = stage.area();
    let at = |rng: &mut rand_chacha::ChaCha8Rng| {
        let p = stage.point(rng);
        spot(game, &season, &terrain, session.me, p)
    };
    let grow = stage.grow();

    // Mist at dawn, low over water and wet land.
    let mist = morning_mist(stage.day) * (1.0 - stage.storm);
    if mist > 0.02 && stage.distance < 75.0 {
        for _ in 0..stage.spawns(area * 0.05 * mist, rng) {
            let Some(s) = at(rng) else { continue };
            let wet = (s.water && !s.deep && !s.frozen) || s.river || s.feature == Some("marsh") || (!s.water && s.moisture > 0.6 && rng.gen_bool(0.3));
            if !wet || s.temp < -6.0 {
                continue;
            }
            let size = rng.gen_range(0.45..0.95);
            let white = rng.gen_range(0.9..0.97);
            puffs.add(
                Puff::new(Shape::Mist, Vec3::new(s.p.x, s.ground + rng.gen_range(0.04..0.12), s.p.y), rng.gen_range(16.0..26.0), size, Vec4::new(white, white, white * 1.02, 0.32 * mist))
                    .grow(size * 1.3)
                    .aspect(2.4)
                    .vel(wind3(0.12))
                    .fade(0.3, 0.45),
            );
        }
    }

    // Leaves from broadleaf woods in autumn, fluttering down.
    let close = stage.close(18.0, 30.0);
    if close > 0.0 {
        const LEAVES: [Vec3; 5] = [Vec3::new(0.85, 0.42, 0.08), Vec3::new(0.72, 0.16, 0.06), Vec3::new(0.92, 0.72, 0.16), Vec3::new(0.55, 0.32, 0.12), Vec3::new(0.8, 0.55, 0.1)];
        for _ in 0..stage.spawns(area * 0.6 * close, rng) {
            let Some(s) = at(rng) else { continue };
            if !(s.forest() && s.mean > 2.0 && s.mean < 21.0 && s.autumn(&season)) {
                continue;
            }
            let c = LEAVES[rng.gen_range(0..LEAVES.len())] * rng.gen_range(0.8..1.1);
            puffs.add(
                Puff::new(Shape::Leaf, Vec3::new(s.p.x, s.ground + rng.gen_range(0.18..0.32), s.p.y), rng.gen_range(5.0..8.0), 0.024 * grow, c.extend(0.95))
                    .vel(Vec3::new(0.0, -rng.gen_range(0.04..0.07), 0.0) + wind3(0.4))
                    .flutter(0.35)
                    .drag(0.5)
                    .spin(rng.gen_range(0.0..6.3), rng.gen_range(-4.0..4.0))
                    .fade(0.1, 0.2),
            );
        }
    }

    // Snow blown low across frozen ground in gusts.
    let close = stage.close(24.0, 40.0);
    if close > 0.0 {
        for _ in 0..stage.spawns(area * 1.2 * close * (0.6 + 0.8 * stage.storm), rng) {
            let Some(s) = at(rng) else { continue };
            if s.water || s.temp > -4.0 || s.forest() {
                continue;
            }
            let gust = 1.8 + 2.5 * stage.storm + rng.gen_range(0.0..1.2);
            puffs.add(
                Puff::new(Shape::Smoke, Vec3::new(s.p.x, s.ground + rng.gen_range(0.01..0.05), s.p.y), rng.gen_range(1.8..3.2), 0.035 * grow, Vec4::new(0.95, 0.97, 1.0, 0.22))
                    .grow(0.07 * grow)
                    .aspect(3.0)
                    .vel(wind3(gust) + Vec3::Y * 0.02)
                    .flutter(0.2)
                    .fade(0.3, 0.5),
            );
        }
    }

    // Butterflies over summer meadows by day, in fair weather.
    let close = stage.close(16.0, 26.0);
    let fair = stage.night < 0.05 && stage.cover < 0.6;
    if close > 0.0 && fair && count(&puffs, Shape::Butterfly) < 40 {
        const WINGS: [Vec3; 5] = [Vec3::new(0.98, 0.97, 0.9), Vec3::new(0.98, 0.86, 0.25), Vec3::new(0.95, 0.5, 0.12), Vec3::new(0.35, 0.6, 0.95), Vec3::new(0.9, 0.9, 0.55)];
        for _ in 0..stage.spawns(area * 0.08 * close, rng) {
            let Some(s) = at(rng) else { continue };
            if !s.meadow() || s.temp < 14.0 || !s.visible {
                continue;
            }
            let c = WINGS[rng.gen_range(0..WINGS.len())];
            let v = Vec2::from_angle(rng.gen_range(0.0..6.3)) * 0.08;
            puffs.add(
                Puff::new(Shape::Butterfly, Vec3::new(s.p.x, s.ground + rng.gen_range(0.04..0.14), s.p.y), rng.gen_range(9.0..14.0), 0.032 * grow, c.extend(1.0))
                    .vel(Vec3::new(v.x, 0.0, v.y))
                    .flutter(0.5)
                    .drag(0.6)
                    .fade(0.1, 0.1),
            );
        }
    }

    // Fireflies on warm nights over woods, meadows and marsh.
    let close = stage.close(20.0, 32.0);
    if close > 0.0 && stage.night > 0.4 && count(&puffs, Shape::Glow) < 160 {
        for _ in 0..stage.spawns(area * 0.12 * close * stage.night, rng) {
            let Some(s) = at(rng) else { continue };
            let haunt = s.forest() || s.meadow() || s.feature == Some("marsh") || s.river;
            if !haunt || s.temp < 16.0 || s.moisture < 0.35 || !s.visible {
                continue;
            }
            puffs.add(
                Puff::new(Shape::Glow, Vec3::new(s.p.x, s.ground + rng.gen_range(0.02..0.18), s.p.y), rng.gen_range(6.0..11.0), 0.035 * grow, Vec4::new(1.4, 2.2, 0.45, 1.0))
                    .blink(rng.gen_range(0.9..1.6))
                    .flutter(0.12)
                    .drag(0.4)
                    .fade(0.1, 0.2),
            );
        }
    }
}

fn count(puffs: &Puffs, shape: Shape) -> usize {
    puffs.list.iter().filter(|p| p.shape == shape).count()
}

/// Whirls of dust wandering over hot, dry land.
#[derive(Resource, Default)]
struct Devils(Vec<Devil>);

struct Devil {
    pos: Vec2,
    vel: Vec2,
    age: f32,
    life: f32,
}

fn hot_and_dry(s: &Spot) -> bool {
    !s.water && s.temp > 24.0 && (s.terrain == "desert" || (s.terrain == "plains" && s.moisture < 0.3)) && s.feature.is_none()
}

fn dust_devils(
    stage: Res<Stage>,
    mut dice: ResMut<Dice>,
    mut puffs: ResMut<Puffs>,
    mut devils: ResMut<Devils>,
    session: Res<Session>,
    season: Res<Season>,
    terrain: Res<Terrain>,
) {
    let rng = &mut dice.0;
    // In the hot middle of the day.
    let midday = smooth((stage.day - 0.14) / 0.08) * (1.0 - smooth((stage.day - 0.48) / 0.08));
    if stage.on && midday > 0.0 && devils.0.len() < 3 && stage.distance < 45.0 && rng.gen_bool((stage.dt * 0.12 * midday).min(1.0) as f64) {
        let p = stage.point(rng);
        if let Some(s) = spot(&session.game, &season, &terrain, session.me, p).filter(|s| hot_and_dry(s) && s.visible) {
            let v = Vec2::from_angle(rng.gen_range(0.0..6.3)) * rng.gen_range(0.15..0.3) + WIND * 0.4;
            devils.0.push(Devil { pos: s.p, vel: v, age: 0.0, life: rng.gen_range(12.0..22.0) });
        }
    }
    let dt = stage.dt;
    let t = stage.day * 1000.0;
    devils.0.retain_mut(|d| {
        d.age += dt;
        d.pos += d.vel * dt;
        d.vel = Vec2::from_angle((t * 0.7 + d.life).sin() * 0.6 * dt).rotate(d.vel);
        d.age < d.life && stage.sees(d.pos, 3.0)
    });
    if !stage.on {
        return;
    }
    for d in &devils.0 {
        // It spins up, wanders, and dies down.
        let strength = smooth(d.age / 2.0) * smooth((d.life - d.age) / 3.0);
        let ground = terrain.heights.sample(d.pos).max(0.0);
        for _ in 0..stage.spawns(40.0 * strength, rng) {
            let h: f32 = rng.gen_range(0.0f32..1.0).powf(1.6) * 0.55;
            let r = 0.015 + h * 0.2;
            let a = rng.gen_range(0.0..std::f32::consts::TAU);
            let radial = Vec2::from_angle(a);
            let tangent = radial.perp();
            let at = Vec3::new(d.pos.x + radial.x * r, ground + h, d.pos.y + radial.y * r);
            let v = Vec3::new(tangent.x, 0.0, tangent.y) * (0.9 + h * 1.5) + Vec3::Y * 0.3 + d.vel.extend(0.0).xzy();
            let tan = rng.gen_range(0.85..1.0);
            puffs.add(
                Puff::new(Shape::Smoke, at, rng.gen_range(0.8..1.5), 0.025 + h * 0.07, Vec4::new(0.8 * tan, 0.68 * tan, 0.5 * tan, 0.28))
                    .grow(0.05 + h * 0.12)
                    .vel(v)
                    .spin(a, 2.0)
                    .fade(0.2, 0.5),
            );
        }
        // Dust kicked up at its foot.
        for _ in 0..stage.spawns(6.0 * strength, rng) {
            let off = Vec2::from_angle(rng.gen_range(0.0..6.3)) * rng.gen_range(0.0..0.08);
            puffs.add(
                Puff::new(Shape::Smoke, Vec3::new(d.pos.x + off.x, ground + 0.02, d.pos.y + off.y), rng.gen_range(1.5..2.5), 0.05, Vec4::new(0.78, 0.66, 0.5, 0.2))
                    .grow(0.14)
                    .vel(wind3(0.3) + Vec3::Y * 0.03)
                    .fade(0.2, 0.6),
            );
        }
    }
}
