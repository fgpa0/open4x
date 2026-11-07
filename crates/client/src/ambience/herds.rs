//! Rigged models that move about on their own (animals, carts, boats), and
//! the animals on resource tiles and pastures: they graze (the rig lowers
//! their heads now and then), swish their tails, and every so often walk a
//! few steps to fresh grass, staying near where they were put.

use super::{AmbienceSet, Dice};
use crate::loading::InGameSet;
use crate::models;
use crate::rigs::{self, Motion, RigInfo};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;

/// The size per metre units are drawn at (units.rs: SCALE times a typical
/// export scale): walking cycles are timed for it.
const UNIT_METRE: f32 = 3.4 * 0.17;

pub fn plugin(app: &mut App) {
    app.init_resource::<Looks>().add_systems(
        Update,
        (dress, graze, pose).chain().in_set(InGameSet).in_set(AmbienceSet::Emit).after(crate::improvements::WorksSet),
    );
}

/// The rigged model shown for a static animal prop, if there is one.
pub fn herd_model(prop: &str) -> Option<&'static str> {
    Some(match prop {
        "cow" => "beast_cow",
        "cow_pied" => "beast_cow_pied",
        "sheep" => "beast_sheep",
        "horse" => "beast_horse",
        "deer" => "beast_deer",
        "fox" => "beast_fox",
        "elephant" => "beast_elephant",
        _ => return None,
    })
}

/// Export scale (metres to model units) of each ambient model, from its
/// Blender script, so walking can be timed to its size.
fn export_scale(model: &str) -> f32 {
    match model {
        "beast_sheep" => 0.021 * 1.7,
        "beast_deer" => 0.021 * 1.25,
        "beast_fox" => 0.021 * 0.9,
        "beast_elephant" => 0.021 * 0.75,
        "oxcart" => 0.16,
        "fishing_boat" => 0.13,
        _ => 0.021,
    }
}

/// A rigged model that moves by itself. `motion` drives its animation;
/// `size` is its transform's scale.
#[derive(Component)]
pub struct Actor {
    pub model: &'static str,
    pub size: f32,
    pub color: Vec4,
    pub motion: Motion,
    pub phase: f32,
}

impl Actor {
    pub fn new(model: &'static str, size: f32, color: Vec4, phase: f32) -> Self {
        Actor { model, size, color, motion: Motion::default(), phase }
    }

    /// Moves the animation along by a walk of `dist` world units at `walk`
    /// (0 standing .. 1 walking, eased).
    pub fn walk(&mut self, dist: f32, walking: bool, dt: f32, time: f32) {
        let metre = export_scale(self.model) * self.size;
        self.motion.travel += dist / metre * UNIT_METRE;
        let target = if walking { 1.0 } else { 0.0 };
        self.motion.walk += (target - self.motion.walk) * (1.0 - (-4.0 * dt).exp());
        self.motion.time = time;
    }
}

/// A model's geometry, material and rig, once loaded.
type Baked = (models::Geometry, Handle<StandardMaterial>, Option<Arc<RigInfo>>);

/// Meshes and materials of the models, by name and colour.
#[derive(Resource, Default)]
struct Looks {
    baked: HashMap<&'static str, Option<Baked>>,
    meshes: HashMap<(&'static str, [u8; 3]), Handle<Mesh>>,
}

/// Gives new actors their model, skinned to fresh joints.
fn dress(
    mut commands: Commands,
    mut looks: ResMut<Looks>,
    actors: Query<(Entity, &Actor), Added<Actor>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    for (entity, actor) in &actors {
        let looks = &mut *looks;
        let Some((geometry, material, rig)) = looks
            .baked
            .entry(actor.model)
            .or_insert_with(|| {
                let mut b = models::baked(actor.model)?;
                let material = materials.add(b.material(&mut images));
                let rig = b.rig.as_ref().map(|r| Arc::new(RigInfo::new(r, actor.size, &mut bindposes)));
                Some((b.nodes.swap_remove(0).1, material, rig))
            })
            .clone()
        else {
            continue;
        };
        let key = (actor.model, (actor.color.truncate() * 255.0).to_array().map(|c| c as u8));
        let mesh = looks.meshes.entry(key).or_insert_with(|| meshes.add(geometry.team_mesh(actor.color))).clone();
        let body = commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::IDENTITY, ChildOf(entity))).id();
        if let Some(rig) = rig {
            let joints = rigs::spawn_joints(&mut commands, entity, &rig);
            commands.entity(body).insert(SkinnedMesh { inverse_bindposes: rig.bindposes.clone(), joints: joints.clone() });
            commands.entity(entity).insert(rigs::Animated { joints, rig, phase: actor.phase });
        }
    }
}

/// Poses each actor's joints for its motion.
fn pose(actors: Query<(&Actor, &rigs::Animated)>, mut joints: Query<&mut Transform, (With<rigs::RigJoint>, Without<Actor>)>) {
    for (actor, animated) in &actors {
        let pose = rigs::pose(&animated.rig, actor.motion, animated.phase);
        for ((entity, (rotation, offset)), (_, rest)) in animated.joints.iter().zip(pose).zip(&animated.rig.joints) {
            if let Ok(mut tf) = joints.get_mut(*entity) {
                let next = Transform::from_translation(*rest + offset).with_rotation(rotation);
                if *tf != next {
                    *tf = next;
                }
            }
        }
    }
}

/// An animal on a tile: spawned by `improvements.rs` as a child of the
/// tile's works (at `home`, relative to the tile's centre `centre` whose
/// ground is at `base`).
#[derive(Component)]
pub struct HerdAnimal {
    /// Its rigged model (see `herd_model`).
    pub model: &'static str,
    pub home: Vec2,
    pub yaw: f32,
    pub size: f32,
    pub centre: Vec2,
    pub base: f32,
}

/// Where an animal is going and how long it stands first.
#[derive(Component)]
struct Grazing {
    pos: Vec2,
    yaw: f32,
    target: Option<Vec2>,
    wait: f32,
}

/// How far an animal strays from home, and how fast it walks (world units).
const STRAY: f32 = 0.07;
const AMBLE: f32 = 0.025;

fn graze(
    mut commands: Commands,
    time: Res<Time>,
    mut dice: ResMut<Dice>,
    terrain: Res<Terrain>,
    settings: Res<crate::settings::Settings>,
    new: Query<(Entity, &HerdAnimal), Without<Actor>>,
    mut herd: Query<(&HerdAnimal, &mut Actor, &mut Grazing, &mut Transform)>,
) {
    let rng = &mut dice.0;
    for (entity, animal) in &new {
        commands.entity(entity).insert((
            Actor::new(animal.model, animal.size, Vec4::ONE, rng.gen_range(0.0..40.0)),
            Grazing { pos: animal.home, yaw: animal.yaw, target: None, wait: rng.gen_range(1.0..10.0) },
        ));
    }
    let dt = time.delta_secs().min(0.1);
    let t = time.elapsed_secs();
    let moving = settings.ambience;
    for (animal, mut actor, mut g, mut tf) in &mut herd {
        let mut dist = 0.0;
        match g.target {
            Some(target) if moving => {
                let delta = target - g.pos;
                let len = delta.length();
                // Turn towards it first, then amble.
                // Models face +Z (as units do).
                let want = delta.x.atan2(delta.y);
                let turn = (want - g.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                g.yaw += turn.clamp(-1.2 * dt, 1.2 * dt);
                if turn.abs() < 0.5 {
                    let step = (AMBLE * animal.size / 0.06 * dt).min(len);
                    g.pos += delta / len.max(1e-5) * step;
                    dist = step;
                }
                if len < 0.003 {
                    g.target = None;
                    g.wait = rng.gen_range(6.0..20.0);
                }
            }
            _ => {
                g.wait -= dt;
                if g.wait <= 0.0 {
                    let off = Vec2::from_angle(rng.gen_range(0.0..std::f32::consts::TAU)) * rng.gen_range(0.02..STRAY);
                    g.target = Some(animal.home + off);
                }
            }
        }
        actor.walk(dist, dist > 0.0, dt, t);
        let ground = terrain.heights.sample(animal.centre + g.pos).max(WATER_LEVEL) - animal.base;
        tf.translation = Vec3::new(g.pos.x, ground, g.pos.y);
        tf.rotation = Quat::from_rotation_y(g.yaw);
    }
}
