//! Procedural animation of rigged unit models (see `models::Rig`).
//!
//! Joints are recognised by name: a group with `<p>hips` is a person
//! (spine, chest, head, arm_l_up/lo, leg_r_up/lo, foot_l...), one with
//! `<p>body` a four-legged animal (neck, head, fl_up/lo, hr_up/lo..., tail);
//! `wheel*` turn with the distance travelled, `oar*` row, `throw` is a siege
//! engine's arm, `barrel` a gun's barrel, `hull` a ship's, `flag*` and
//! `sail*` flutter. People walk in step with the distance they cover, so
//! feet don't slide, breathe and look around when idle, and swing, thrust,
//! draw or fire (the rig's style) when they attack. Riders sit still while
//! their horse walks.

use crate::models;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

/// World distance of one walking cycle for people and for animals.
const STRIDE: f32 = 1.5;
const BEAST_STRIDE: f32 = 2.6;
/// How long an attack takes, in seconds.
pub const ATTACK_TIME: f32 = 0.8;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Hips { mounted: bool },
    Spine,
    Chest,
    Head,
    ArmUp(f32),
    ArmLo(f32),
    LegUp(f32, bool),
    LegLo(f32, bool),
    Foot(f32, bool),
    Body,
    Neck,
    BeastHead,
    /// front, side (+1 left)
    BeastUp(bool, f32),
    BeastLo(bool, f32),
    Tail,
    Wheel(f32),
    Oar(f32, f32),
    Throw,
    Barrel,
    Hull,
    Flutter,
    Fixed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Melee,
    Thrust,
    Bow,
    Gun,
    Siege,
    Cannon,
    None,
}

/// A rig ready to spawn: joint kinds, rest offsets from their parents and
/// the inverse bind poses.
pub struct RigInfo {
    pub joints: Vec<(Option<usize>, Vec3)>,
    kinds: Vec<Kind>,
    pub bindposes: Handle<SkinnedMeshInverseBindposes>,
    pub style: Style,
}

impl RigInfo {
    /// `scale` is the size the model is shown at (wheels need their true radius).
    pub fn new(rig: &models::Rig, scale: f32, bindposes: &mut Assets<SkinnedMeshInverseBindposes>) -> Self {
        let names: Vec<&str> = rig.joints.iter().map(|j| j.name.as_str()).collect();
        let has = |n: &str| names.contains(&n);
        let kinds: Vec<Kind> = rig
            .joints
            .iter()
            .map(|j| {
                let name = j.name.as_str();
                let side = |s: &str| if s.contains("_l_") || s.ends_with("_l") { 1.0 } else { -1.0 };
                // People.
                for suffix in ["hips", "spine", "chest", "head", "arm_l_up", "arm_r_up", "arm_l_lo", "arm_r_lo", "leg_l_up", "leg_r_up", "leg_l_lo", "leg_r_lo", "foot_l", "foot_r"] {
                    let Some(prefix) = name.strip_suffix(suffix) else { continue };
                    if !has(&format!("{prefix}hips")) {
                        continue;
                    }
                    let hips = names.iter().position(|n| *n == format!("{prefix}hips")).unwrap_or(0);
                    let mounted = rig.joints[hips].parent.is_some();
                    return match suffix {
                        "hips" => Kind::Hips { mounted },
                        "spine" => Kind::Spine,
                        "chest" => Kind::Chest,
                        "head" => Kind::Head,
                        s if s.starts_with("arm") && s.ends_with("up") => Kind::ArmUp(side(s)),
                        s if s.starts_with("arm") => Kind::ArmLo(side(s)),
                        s if s.starts_with("leg") && s.ends_with("up") => Kind::LegUp(side(s), mounted),
                        s if s.starts_with("leg") => Kind::LegLo(side(s), mounted),
                        s => Kind::Foot(side(s), mounted),
                    };
                }
                // Animals.
                for suffix in ["body", "neck", "head", "tail", "fl_up", "fr_up", "hl_up", "hr_up", "fl_lo", "fr_lo", "hl_lo", "hr_lo"] {
                    let Some(prefix) = name.strip_suffix(suffix) else { continue };
                    if !has(&format!("{prefix}body")) {
                        continue;
                    }
                    let (front, side) = (suffix.starts_with('f'), if suffix.as_bytes()[1] == b'l' { 1.0 } else { -1.0 });
                    return match suffix {
                        "body" => Kind::Body,
                        "neck" => Kind::Neck,
                        "head" => Kind::BeastHead,
                        "tail" => Kind::Tail,
                        s if s.ends_with("up") => Kind::BeastUp(front, side),
                        _ => Kind::BeastLo(front, side),
                    };
                }
                match name {
                    n if n.starts_with("wheel") => Kind::Wheel((j.pivot.y * scale).max(0.01)),
                    n if n.starts_with("oar") => Kind::Oar(j.pivot.x.signum(), j.pivot.z),
                    "throw" => Kind::Throw,
                    "barrel" => Kind::Barrel,
                    "hull" => Kind::Hull,
                    n if n.starts_with("flag") || n.starts_with("sail") => Kind::Flutter,
                    _ => Kind::Fixed,
                }
            })
            .collect();
        let joints = rig.joints.iter().map(|j| (j.parent, j.pivot - j.parent.map_or(Vec3::ZERO, |p| rig.joints[p].pivot))).collect();
        let inverse: Vec<Mat4> = rig.joints.iter().map(|j| Mat4::from_translation(-j.pivot)).collect();
        let style = match rig.style.as_str() {
            "melee" => Style::Melee,
            "thrust" => Style::Thrust,
            "bow" => Style::Bow,
            "gun" => Style::Gun,
            "siege" => Style::Siege,
            "cannon" => Style::Cannon,
            _ => Style::None,
        };
        RigInfo { joints, kinds, bindposes: bindposes.add(SkinnedMeshInverseBindposes::from(inverse)), style }
    }
}

/// A spawned rig: its joint entities, in the rig's order.
#[derive(Component)]
pub struct Animated {
    pub joints: Vec<Entity>,
    pub rig: Arc<RigInfo>,
    /// Offsets idle motion so units don't move in lockstep.
    pub phase: f32,
}

/// Marks a joint entity.
#[derive(Component)]
pub struct RigJoint;

/// What a unit is doing, for its animation.
#[derive(Clone, Copy, Default)]
pub struct Motion {
    /// 0 standing to 1 walking, eased.
    pub walk: f32,
    /// World distance walked so far.
    pub travel: f32,
    /// Seconds since its last attack began, if one is playing.
    pub attack: Option<f32>,
    /// Seconds of real time (for idle motion).
    pub time: f32,
}

/// The pose of each joint for a motion: rotations and offsets from rest.
pub fn pose(rig: &RigInfo, m: Motion, phase: f32) -> Vec<(Quat, Vec3)> {
    let t = m.time + phase;
    let w = m.walk;
    let step = m.travel / STRIDE * TAU;
    let gait = m.travel / BEAST_STRIDE * TAU;
    let a = m.attack.map(|s| (s / ATTACK_TIME).clamp(0.0, 1.0));
    let rx = Quat::from_rotation_x;
    let ry = Quat::from_rotation_y;
    let rz = Quat::from_rotation_z;
    // The attack curves: a swing (raise, strike, recover) and a jab.
    let swing = |a: f32| -> f32 {
        if a < 0.4 {
            -2.3 * smooth(a / 0.4)
        } else if a < 0.55 {
            -2.3 + 2.8 * smooth((a - 0.4) / 0.15)
        } else {
            0.5 * (1.0 - smooth((a - 0.55) / 0.45))
        }
    };
    let jab = |a: f32| -> f32 {
        if a < 0.35 {
            0.5 * smooth(a / 0.35)
        } else if a < 0.5 {
            0.5 - 1.5 * smooth((a - 0.35) / 0.15)
        } else {
            -(1.0 - smooth((a - 0.5) / 0.5))
        }
    };
    let kick = |a: f32| -> f32 { if a < 0.08 { a / 0.08 } else { (1.0 - (a - 0.08) / 0.5).max(0.0) } };
    let style = rig.style;
    rig.kinds
        .iter()
        .map(|kind| match *kind {
            Kind::Hips { mounted } => {
                let bob = if mounted { 0.0 } else { (step * 2.0).cos().abs() * 0.008 * w };
                (ry(step.sin() * 0.06 * w), Vec3::Y * bob)
            }
            Kind::Spine => {
                let lean = match (style, a) {
                    (Style::Melee, Some(a)) => -0.25 * (swing(a) / -2.3).clamp(-0.3, 1.0),
                    (Style::Thrust, Some(a)) => 0.18 * jab(a),
                    _ => 0.0,
                };
                (rx(-0.05 * w + lean) * ry(-step.sin() * 0.05 * w), Vec3::ZERO)
            }
            Kind::Chest => {
                let breathe = (t * 1.4).sin() * 0.025;
                let twist = match (style, a) {
                    (Style::Melee, Some(a)) => 0.35 * (swing(a) / -2.3),
                    _ => 0.0,
                };
                let recoil = match (style, a) {
                    (Style::Gun, Some(a)) => 0.25 * kick(a),
                    _ => 0.0,
                };
                (rx(breathe + recoil) * ry(twist), Vec3::ZERO)
            }
            Kind::Head => {
                let look = (t * 0.37).sin() * 0.3 * (1.0 - w) + (t * 0.11).sin() * 0.15;
                (ry(look) * rx((t * 0.23).sin() * 0.06), Vec3::ZERO)
            }
            Kind::ArmUp(side) => {
                let walk = step.sin() * 0.35 * w * side;
                let idle = (t * 1.1 + side).sin() * 0.03;
                let attack = match (style, a, side < 0.0) {
                    (Style::Melee, Some(a), true) => swing(a),
                    (Style::Thrust, Some(a), true) => jab(a) * 0.8,
                    (Style::Bow, Some(a), true) => 0.35 * draw(a),
                    (Style::Gun, Some(a), _) => -0.25 * kick(a),
                    _ => 0.0,
                };
                (rx(walk + idle + attack), Vec3::ZERO)
            }
            Kind::ArmLo(side) => {
                let attack = match (style, a, side < 0.0) {
                    (Style::Melee, Some(a), true) => (swing(a) / -2.3).max(0.0) * -0.7,
                    (Style::Thrust, Some(a), true) => jab(a).min(0.0) * -0.4,
                    (Style::Bow, Some(a), true) => -0.4 * draw(a),
                    _ => 0.0,
                };
                (rx(-0.15 * w + attack), Vec3::ZERO)
            }
            Kind::LegUp(side, mounted) => {
                if mounted {
                    (Quat::IDENTITY, Vec3::ZERO)
                } else {
                    (rx(-(step.sin()) * 0.55 * w * side), Vec3::ZERO)
                }
            }
            Kind::LegLo(side, mounted) => {
                if mounted {
                    (Quat::IDENTITY, Vec3::ZERO)
                } else {
                    // The knee bends while the leg swings forward.
                    (rx((step.cos() * side).max(0.0) * 0.9 * w), Vec3::ZERO)
                }
            }
            Kind::Foot(side, mounted) => {
                if mounted {
                    (Quat::IDENTITY, Vec3::ZERO)
                } else {
                    (rx(-(step.sin()) * 0.25 * w * side), Vec3::ZERO)
                }
            }
            Kind::Body => {
                let bob = (gait * 2.0).sin() * 0.012 * w;
                (rx((gait * 2.0).cos() * 0.015 * w), Vec3::Y * bob)
            }
            Kind::Neck => (rx((gait * 2.0).sin() * 0.08 * w + ((t * 0.4).sin() * 0.5 + 0.5).powi(3) * 0.35 * (1.0 - w)), Vec3::ZERO),
            Kind::BeastHead => (rx((t * 0.7).sin() * 0.05) * ry((t * 0.3).sin() * 0.15 * (1.0 - w)), Vec3::ZERO),
            Kind::BeastUp(front, side) => {
                // A four-beat walk: near fore, off hind, off fore, near hind.
                let off = match (front, side > 0.0) {
                    (true, true) => 0.0,
                    (false, false) => 0.25,
                    (true, false) => 0.5,
                    (false, true) => 0.75,
                };
                let p = gait + off * TAU;
                (rx(-p.sin() * 0.45 * w), Vec3::ZERO)
            }
            Kind::BeastLo(front, side) => {
                let off = match (front, side > 0.0) {
                    (true, true) => 0.0,
                    (false, false) => 0.25,
                    (true, false) => 0.5,
                    (false, true) => 0.75,
                };
                let p = gait + off * TAU;
                // Lifted while swinging forward: fore knees bend back, hocks forward.
                let lift = p.cos().max(0.0) * 0.9 * w;
                (rx(if front { lift } else { -lift * 0.7 }), Vec3::ZERO)
            }
            Kind::Tail => (rz((t * 1.3).sin() * 0.25) * rx(0.1 * w), Vec3::ZERO),
            Kind::Wheel(r) => (rx(m.travel / r), Vec3::ZERO),
            Kind::Oar(side, z) => {
                let speed = 0.6 + 1.4 * w;
                let p = t * speed * PI + z * 0.0;
                (ry(p.sin() * 0.35 * side) * rz(p.cos() * 0.12 * side), Vec3::ZERO)
            }
            Kind::Throw => {
                let angle = match a {
                    Some(a) if a < 0.15 => 0.15 * smooth(a / 0.15),
                    Some(a) if a < 0.35 => 0.15 - 2.2 * smooth((a - 0.15) / 0.2),
                    Some(a) => -2.05 * (1.0 - smooth((a - 0.35) / 0.65)),
                    None => 0.0,
                };
                (rx(angle), Vec3::ZERO)
            }
            Kind::Barrel => (Quat::IDENTITY, Vec3::Z * -0.05 * a.map_or(0.0, kick)),
            Kind::Hull => (rz((t * 0.9).sin() * 0.035) * rx((t * 0.7 + 1.0).sin() * 0.02), Vec3::ZERO),
            Kind::Flutter => (ry((t * 3.1).sin() * 0.12) * rx((t * 2.3).sin() * 0.04), Vec3::ZERO),
            Kind::Fixed => (Quat::IDENTITY, Vec3::ZERO),
        })
        .collect()
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A bow's draw: pull back, hold, release.
fn draw(a: f32) -> f32 {
    if a < 0.5 {
        smooth(a / 0.5)
    } else if a < 0.6 {
        1.0
    } else {
        1.0 - smooth((a - 0.6) / 0.1)
    }
}

/// Spawns a rig's joints under `parent`, returning them in the rig's order.
pub fn spawn_joints(commands: &mut Commands, parent: Entity, rig: &RigInfo) -> Vec<Entity> {
    let mut out: Vec<Entity> = Vec::with_capacity(rig.joints.len());
    for (p, offset) in &rig.joints {
        let under = p.map_or(parent, |p| out[p]);
        out.push(commands.spawn((RigJoint, Transform::from_translation(*offset), ChildOf(under))).id());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rigs_classify() {
        let mut assets = Assets::<SkinnedMeshInverseBindposes>::default();
        for name in ["horseman"] {
            let Some(b) = models::baked(name) else { continue };
            let Some(rig) = &b.rig else { continue };
            let info = RigInfo::new(rig, 1.0, &mut assets);
            assert!(info.kinds.iter().any(|k| matches!(k, Kind::Hips { mounted: true })), "{name}: rider not on the horse");
            assert!(info.kinds.iter().any(|k| matches!(k, Kind::BeastUp(true, _))), "{name}: no horse legs");
            assert_eq!(pose(&info, Motion::default(), 0.0).len(), rig.joints.len());
        }
    }
}
