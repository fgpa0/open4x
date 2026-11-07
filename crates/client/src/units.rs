//! Unit rendering: procedural low-poly models in the owner's colours, smooth
//! movement along paths, the selection marker and the move-path preview.

use crate::bridges::Bridges;
use crate::layout::{hex_at, tile_center};
use crate::loading::{AppState, InGameSet};
use crate::overlay::{corner, HoveredTile, XRay, XRayMaterial};
use crate::play::{GameEvent, Selected, Session};
use crate::procmesh::{at, blob, cone, cylinder, lin, Model};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::icons::{Icon, Icons};
use crate::models;
use crate::rigs;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use std::sync::Arc;
use crate::ui::{palette, Tooltip};
use crate::wrap::{nearest_shift, WrapPeriod, Wrapped};
use bevy::prelude::*;
use sim::game::{Event, Order, PlayerId, UnitId, Slot};
use sim::hex::TileId;
use sim::path::Path;
use sim::rules::Domain;
use std::collections::{HashMap, VecDeque};

/// Walking speed of unit models, in world units per second.
const SPEED: f32 = 4.5;
/// Model scale (a hex is ~1.7 units across). Deliberately oversized, as in most
/// strategy games, so units stand out above the terrain.
const SCALE: f32 = 3.4;

#[derive(Component)]
pub struct UnitView {
    pub id: UnitId,
    /// Current (unwrapped) world position.
    pos: Vec3,
    /// Points still to walk through.
    queue: VecDeque<Vec3>,
    yaw: f32,
    /// A land unit shown in its boat (it's on water).
    afloat: bool,
    /// Walking, travel and attacks, for its animation.
    motion: rigs::Motion,
}

/// The entities making up a unit's model (its mesh and any joints), so
/// they can be replaced when it boards a boat.
#[derive(Component)]
struct UnitBody(Vec<Entity>);

/// How a unit type looks for one owner.
#[derive(Clone)]
struct UnitLook {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    rig: Option<Arc<rigs::RigInfo>>,
}

/// The mesh key for embarked land units' boat.
const BOAT: &str = "~boat";

#[derive(Resource)]
struct UnitAssets {
    material: Handle<StandardMaterial>,
    looks: HashMap<(String, PlayerId), UnitLook>,
    /// Models made in Blender (see `models`) with their textured material
    /// and rig, `None` for art that is still built here.
    baked: HashMap<String, Option<BakedUnit>>,
}

/// A Blender model's mesh, textured material and rig.
type BakedUnit = (models::Geometry, Handle<StandardMaterial>, Option<Arc<rigs::RigInfo>>);

/// The asset stores a unit's look is made into.
#[derive(bevy::ecs::system::SystemParam)]
struct LookAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
}

/// Gives a unit entity its model: the mesh, skinned to freshly spawned
/// joints when the look has a rig.
fn dress(commands: &mut Commands, unit: Entity, look: &UnitLook, phase: f32) {
    let mut parts = Vec::new();
    let mesh = commands.spawn((Mesh3d(look.mesh.clone()), MeshMaterial3d(look.material.clone()), Transform::IDENTITY, ChildOf(unit)));
    if let Some(rig) = &look.rig {
        let mesh_entity = mesh.id();
        let joints = rigs::spawn_joints(commands, unit, rig);
        commands.entity(mesh_entity).insert(SkinnedMesh { inverse_bindposes: rig.bindposes.clone(), joints: joints.clone() });
        parts.push(mesh_entity);
        parts.extend(joints.iter().copied());
        commands.entity(unit).insert(rigs::Animated { joints, rig: rig.clone(), phase });
    } else {
        parts.push(mesh.id());
        commands.entity(unit).remove::<rigs::Animated>();
    }
    commands.entity(unit).insert(UnitBody(parts));
}

impl UnitAssets {
    /// The mesh and material for a unit's art in its owner's colours.
    fn look(&mut self, art: &str, owner: PlayerId, color: Vec4, assets: &mut LookAssets) -> UnitLook {
        if let Some(look) = self.looks.get(&(art.to_string(), owner)) {
            return look.clone();
        }
        let baked = self.baked.entry(art.to_string()).or_insert_with(|| {
            models::baked(if art == BOAT { "boat" } else { art }).map(|mut b| {
                let material = assets.materials.add(b.material(&mut assets.images));
                let rig = b.rig.as_ref().map(|r| Arc::new(rigs::RigInfo::new(r, SCALE, &mut assets.bindposes)));
                (b.nodes.swap_remove(0).1, material, rig)
            })
        });
        let look = match baked {
            Some((g, material, rig)) => UnitLook { mesh: assets.meshes.add(g.team_mesh(color)), material: material.clone(), rig: rig.clone() },
            None => UnitLook { mesh: assets.meshes.add(unit_model(art, color).into_mesh()), material: self.material.clone(), rig: None },
        };
        self.looks.insert((art.to_string(), owner), look.clone());
        look
    }
}

/// A screen-space flag above a unit (owner's colour, unit code), so units can
/// be found at any zoom and even inside forests. Clicking it selects the unit.
#[derive(Component)]
struct UnitFlag {
    unit: UnitId,
    /// The unit type its icon shows (an upgrade makes a new flag).
    kind: sim::rules::UnitTypeId,
    /// Offset sideways so flags of stacked units don't overlap.
    nudge: f32,
    /// What the flag shows besides the unit's icon (`None` until built).
    extras: Option<FlagExtras>,
}

/// A wounded unit's health and our unit's standing order, drawn on its flag.
/// Their nodes only exist while shown: hundreds of flags each carrying hidden
/// ones made the UI layout noticeably slower.
#[derive(Clone, PartialEq)]
struct FlagExtras {
    hp: Option<i32>,
    order: Option<Icon>,
    /// An army's members' hit points (its banner's bar is split by member).
    members: Vec<i32>,
    /// Whether the members' bar shows (several members, or anyone wounded).
    bar: bool,
    /// The army's rank by its members' average experience (0: none yet).
    rank: usize,
}

/// Turn-number labels along the path preview.
#[derive(Component)]
struct PathLabel;

#[derive(Resource, Default)]
struct PathPreview {
    for_unit: Option<UnitId>,
    to: Option<TileId>,
    path: Option<Path>,
}

pub struct UnitsPlugin;

impl Plugin for UnitsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PathPreview>()
            .add_systems(OnEnter(AppState::InGame), (setup, spawn_selection_marker))
            .add_systems(
                Update,
                (sync_units, animate_units, float_units, pose_units, update_flags, update_path_preview, draw_markers, update_selection_marker)
                    .chain()
                    .after(crate::play::PlaySet)
                    .in_set(InGameSet),
            );
    }
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(UnitAssets {
        material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.7,
            ..default()
        }),
        looks: HashMap::new(),
        baked: HashMap::new(),
    });
}

fn team(color: [u8; 3]) -> Vec4 {
    lin(color[0], color[1], color[2])
}

/// A person: legs, a tunic in the team colour, a head.
fn person(m: &mut Model, cloth: Vec4, x: f32, z: f32) {
    let skin = lin(222, 182, 148);
    m.add(cylinder(0.03, 0.11, 6), lin(70, 56, 44), at(x, 0.09, z));
    m.add(cone(0.065, 0.16, 7), cloth, at(x, 0.21, z));
    m.add(cylinder(0.04, 0.08, 7), cloth, at(x, 0.2, z));
    m.add(blob(0.042, 1), skin, at(x, 0.31, z));
}

/// A horse side-on along z (its head towards +z), standing on the ground.
fn horse(m: &mut Model, coat: Vec4, transform: Transform) {
    let dark = lin(70, 46, 30);
    let mut h = Model::default();
    h.add(Cuboid::new(0.08, 0.08, 0.2).mesh().build(), coat, at(0.0, 0.17, 0.0));
    h.add(blob(0.045, 1), coat, at(0.0, 0.17, -0.09));
    h.add(Cuboid::new(0.05, 0.12, 0.05).mesh().build(), coat, at(0.0, 0.25, 0.11).with_rotation(Quat::from_rotation_x(0.5)));
    h.add(Cuboid::new(0.045, 0.045, 0.1).mesh().build(), coat, at(0.0, 0.3, 0.17).with_rotation(Quat::from_rotation_x(0.3)));
    h.add(Cuboid::new(0.012, 0.08, 0.06).mesh().build(), dark, at(0.0, 0.29, 0.1).with_rotation(Quat::from_rotation_x(0.5)));
    h.add(Cuboid::new(0.02, 0.1, 0.02).mesh().build(), dark, at(0.0, 0.14, -0.13).with_rotation(Quat::from_rotation_x(-0.5)));
    for (x, z) in [(-0.03f32, 0.08f32), (0.03, 0.08), (-0.03, -0.08), (0.03, -0.08)] {
        h.add(cylinder(0.012, 0.13, 5), coat, at(x, 0.065, z));
    }
    m.append(&h, &transform, Vec4::ONE);
}

/// A spoked wheel of radius `r` turning about the x axis.
fn wheel(m: &mut Model, r: f32, x: f32, y: f32, z: f32, color: Vec4) {
    use std::f32::consts::FRAC_PI_2;
    let side = Quat::from_rotation_z(FRAC_PI_2);
    m.add(cylinder(r, 0.014, 12), color, at(x, y, z).with_rotation(side));
    m.add(cylinder(r * 0.3, 0.03, 6), lin(150, 120, 60), at(x, y, z).with_rotation(side));
}

/// A gun barrel along +z, `len` long and of radius `r`, placed by `transform`.
fn barrel(m: &mut Model, len: f32, r: f32, color: Vec4, transform: Transform) {
    use std::f32::consts::FRAC_PI_2;
    let mut b = Model::default();
    let along = Quat::from_rotation_x(FRAC_PI_2);
    b.add(cylinder(r, len, 8), color, at(0.0, 0.0, 0.0).with_rotation(along));
    b.add(cylinder(r * 1.25, 0.02, 8), color, at(0.0, 0.0, len / 2.0 - 0.01).with_rotation(along));
    b.add(blob(r * 1.1, 1), color, at(0.0, 0.0, -len / 2.0));
    m.append(&b, &transform, Vec4::ONE);
}

/// A lateen (triangular) sail on a mast at `z`.
fn lateen(m: &mut Model, z: f32, height: f32, width: f32, color: Vec4) {
    let wood = lin(110, 78, 48);
    m.add(cylinder(0.008, height, 5), wood, at(0.0, height / 2.0 + 0.04, z));
    // A flattened three-sided cone is a triangle: tall along the yard, leaning aft.
    let lean = Quat::from_rotation_x(-0.35) * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    m.add(cone(width / 2.0, height * 0.85, 3), color, at(0.0, height * 0.55, z - 0.02).with_rotation(lean).with_scale(Vec3::new(0.08, 1.0, 1.0)));
    m.add(cylinder(0.005, height * 0.95, 4), wood, at(0.0, height * 0.58, z - 0.03).with_rotation(Quat::from_rotation_x(-0.35)));
}

/// A ship's hull along +z (the bow), `len` long, `w` wide and `h` high: a
/// pointed bow, a raised stern, a deck and a stripe in the owner's colour.
fn hull(m: &mut Model, len: f32, w: f32, h: f32, color: Vec4) {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
    let wood = lin(110, 78, 48);
    let y = h / 2.0 - 0.01;
    m.add(Cuboid::new(w, h, len).mesh().build(), wood, at(0.0, y, 0.0));
    let bow = Quat::from_rotation_x(FRAC_PI_2) * Quat::from_rotation_y(FRAC_PI_4);
    m.add(cone(w * 0.71, 0.13, 4), wood, at(0.0, y, len / 2.0 + 0.065).with_rotation(bow));
    m.add(Cuboid::new(w * 0.9, h * 1.6, 0.07).mesh().build(), wood, at(0.0, y + h * 0.3, -len / 2.0 + 0.02));
    m.add(cylinder(0.014, 0.12, 5), wood, at(0.0, h + 0.06, -len / 2.0 - 0.02).with_rotation(Quat::from_rotation_x(-0.5)));
    m.add(Cuboid::new(w * 0.86, 0.012, len * 0.92).mesh().build(), lin(150, 112, 72), at(0.0, h - 0.006, 0.0));
    m.add(Cuboid::new(w + 0.008, 0.016, len * 0.96).mesh().build(), color, at(0.0, h - 0.02, 0.0));
}

/// A row of oars along both sides of a hull of width `w`.
fn oars(m: &mut Model, w: f32, y: f32, zs: &[f32]) {
    let wood = lin(110, 78, 48);
    for &z in zs {
        for side in [-1.0f32, 1.0] {
            m.add(
                Cuboid::new(0.13, 0.008, 0.012).mesh().build(),
                wood,
                at(side * (w / 2.0 + 0.05), y, z).with_rotation(Quat::from_rotation_z(-side * 0.45)),
            );
        }
    }
}

/// A mast at `z` with a square sail in the owner's colour.
fn sail(m: &mut Model, z: f32, height: f32, width: f32, color: Vec4) {
    let wood = lin(110, 78, 48);
    m.add(cylinder(0.008, height, 5), wood, at(0.0, height / 2.0 + 0.04, z));
    m.add(Cuboid::new(width, height * 0.42, 0.006).mesh().build(), color, at(0.0, height * 0.7, z + 0.012));
    m.add(
        cylinder(0.006, width + 0.03, 4),
        wood,
        at(0.0, height * 0.92, z + 0.012).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
    );
}

/// Ships (sea units), or `None` for other art.
fn ship_model(art: &str, color: Vec4) -> Option<Model> {
    let mut m = Model::default();
    match art {
        "galley" => {
            hull(&mut m, 0.36, 0.1, 0.06, color);
            oars(&mut m, 0.1, 0.02, &[-0.12, -0.06, 0.0, 0.06, 0.12]);
            sail(&mut m, 0.02, 0.34, 0.2, color);
            m.add(cone(0.018, 0.07, 6), lin(186, 142, 72), at(0.0, 0.0, 0.29).with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)));
        }
        "quadrireme" => {
            hull(&mut m, 0.44, 0.13, 0.075, color);
            oars(&mut m, 0.13, 0.03, &[-0.15, -0.09, -0.03, 0.03, 0.09]);
            oars(&mut m, 0.13, 0.0, &[-0.12, -0.06, 0.0, 0.06, 0.12]);
            sail(&mut m, -0.1, 0.3, 0.18, color);
            // A catapult on the foredeck.
            let wood = lin(110, 78, 48);
            for x in [-0.03f32, 0.03] {
                m.add(Cuboid::new(0.015, 0.08, 0.015).mesh().build(), wood, at(x, 0.11, 0.1));
            }
            m.add(Cuboid::new(0.018, 0.018, 0.2).mesh().build(), wood, at(0.0, 0.14, 0.08).with_rotation(Quat::from_rotation_x(-0.8)));
            m.add(blob(0.022, 1), lin(150, 144, 136), at(0.0, 0.215, 0.145));
        }
        "caravel" => {
            hull(&mut m, 0.42, 0.13, 0.075, color);
            // A raised sterncastle.
            m.add(Cuboid::new(0.12, 0.06, 0.1).mesh().build(), lin(110, 78, 48), at(0.0, 0.1, -0.15));
            m.add(Cuboid::new(0.125, 0.012, 0.105).mesh().build(), color, at(0.0, 0.13, -0.15));
            lateen(&mut m, 0.07, 0.38, 0.3, lin(236, 228, 206));
            lateen(&mut m, -0.08, 0.3, 0.24, lin(236, 228, 206));
            m.add(Cuboid::new(0.01, 0.07, 0.07).mesh().build(), color, at(0.0, 0.32, 0.075));
        }
        "frigate" => {
            let dark = lin(66, 50, 38);
            hull(&mut m, 0.5, 0.15, 0.1, color);
            m.add(Cuboid::new(0.155, 0.05, 0.42).mesh().build(), dark, at(0.0, 0.055, 0.0));
            // Gunports down both sides.
            for z in [-0.15f32, -0.09, -0.03, 0.03, 0.09, 0.15] {
                for side in [-1.0f32, 1.0] {
                    m.add(Cuboid::new(0.01, 0.02, 0.025).mesh().build(), lin(24, 18, 14), at(side * 0.079, 0.06, z));
                }
            }
            m.add(Cuboid::new(0.14, 0.05, 0.1).mesh().build(), dark, at(0.0, 0.12, -0.19));
            for (z, h) in [(0.13f32, 0.36f32), (0.0, 0.44), (-0.12, 0.34)] {
                sail(&mut m, z, h, 0.2, lin(236, 228, 206));
                m.add(Cuboid::new(0.18, h * 0.2, 0.006).mesh().build(), lin(236, 228, 206), at(0.0, h * 0.42, z + 0.012));
            }
            m.add(Cuboid::new(0.01, 0.05, 0.08).mesh().build(), color, at(0.0, 0.5, -0.0));
        }
        _ => return None,
    }
    Some(m)
}

/// The boat a land unit travels in while embarked.
fn boat_model(color: Vec4) -> Model {
    let mut m = Model::default();
    hull(&mut m, 0.28, 0.11, 0.045, color);
    oars(&mut m, 0.11, 0.02, &[-0.08, 0.0]);
    sail(&mut m, 0.07, 0.3, 0.16, color);
    let mut crew = Model::default();
    person(&mut crew, color, 0.0, 0.0);
    m.append(&crew, &Transform::from_xyz(0.0, 0.03, -0.08).with_scale(Vec3::splat(0.75)), Vec4::ONE);
    m
}

/// Ships are built to the people's scale but shown smaller, so they fit on
/// a tile; an embarked unit's boat less so, to fit its crew.
const SHIP_SCALE: f32 = 0.62;
const BOAT_SCALE: f32 = 0.72;

/// The detailed model for a unit type in a player's colours.
fn unit_model(art: &str, color: Vec4) -> Model {
    let vessel = if art == BOAT { Some((boat_model(color), BOAT_SCALE)) } else { ship_model(art, color).map(|m| (m, SHIP_SCALE)) };
    if let Some((v, scale)) = vessel {
        let mut m = Model::default();
        m.append(&v, &Transform::from_scale(Vec3::splat(scale)), Vec4::ONE);
        return m;
    }
    let mut m = Model::default();
    let wood = lin(110, 78, 48);
    let metal = lin(150, 150, 156);
    // A banner pole so units read from afar.
    m.add(cylinder(0.008, 0.52, 4), wood, at(-0.14, 0.27, -0.1));
    m.add(Cuboid::new(0.15, 0.08, 0.008).mesh().build(), color, at(-0.065, 0.48, -0.1));
    match art {
        "warrior" => {
            person(&mut m, color, 0.02, 0.0);
            m.add(cylinder(0.075, 0.014, 10), color, at(0.09, 0.2, 0.04).with_rotation(Quat::from_rotation_z(1.5)));
            m.add(blob(0.018, 0), metal, at(0.1, 0.2, 0.04));
            m.add(cylinder(0.013, 0.2, 5), wood, at(-0.07, 0.22, 0.03).with_rotation(Quat::from_rotation_z(0.5)));
        }
        "archer" => {
            person(&mut m, color, 0.02, 0.0);
            // Bow: two angled limbs and a string.
            for s in [-1.0f32, 1.0] {
                m.add(
                    Cuboid::new(0.01, 0.13, 0.012).mesh().build(),
                    wood,
                    at(0.1, 0.22 + s * 0.06, 0.04).with_rotation(Quat::from_rotation_x(s * 0.35)),
                );
            }
            m.add(cylinder(0.025, 0.12, 6), lin(120, 84, 52), at(-0.02, 0.24, -0.06).with_rotation(Quat::from_rotation_x(0.3)));
        }
        "spearman" => {
            person(&mut m, color, 0.02, 0.0);
            // A long spear held upright and a round shield in the owner's colour.
            m.add(cylinder(0.009, 0.46, 5), wood, at(0.08, 0.25, 0.03));
            m.add(cone(0.02, 0.06, 6), metal, at(0.08, 0.5, 0.03));
            m.add(
                cylinder(0.07, 0.014, 12),
                color,
                at(-0.06, 0.2, 0.06).with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 - 0.2)),
            );
            m.add(blob(0.016, 0), metal, at(-0.06, 0.2, 0.075));
        }
        "horseman" => {
            // A horse side-on along z, its rider in the owner's colour with a spear.
            horse(&mut m, lin(118, 78, 48), Transform::IDENTITY);
            m.add(Cuboid::new(0.09, 0.015, 0.08).mesh().build(), color, at(0.0, 0.22, 0.0));
            let mut rider = Model::default();
            person(&mut rider, color, 0.0, 0.0);
            m.append(&rider, &Transform::from_xyz(0.0, 0.13, 0.0).with_scale(Vec3::splat(0.85)), Vec4::ONE);
            m.add(cylinder(0.008, 0.42, 5), wood, at(0.07, 0.38, 0.06).with_rotation(Quat::from_rotation_x(0.35)));
            m.add(cone(0.016, 0.05, 6), metal, at(0.07, 0.58, 0.13).with_rotation(Quat::from_rotation_x(0.35)));
        }
        "swordsman" => {
            person(&mut m, color, 0.02, 0.0);
            // An iron helmet, a short sword and a tall rectangular shield.
            m.add(cone(0.048, 0.05, 8), metal, at(0.02, 0.36, 0.0));
            m.add(Cuboid::new(0.012, 0.15, 0.02).mesh().build(), lin(196, 200, 208), at(0.1, 0.25, 0.05).with_rotation(Quat::from_rotation_x(0.3)));
            m.add(Cuboid::new(0.05, 0.012, 0.02).mesh().build(), lin(150, 120, 60), at(0.1, 0.18, 0.03));
            m.add(Cuboid::new(0.11, 0.17, 0.014).mesh().build(), color, at(-0.05, 0.2, 0.07).with_rotation(Quat::from_rotation_y(-0.3)));
            m.add(Cuboid::new(0.02, 0.15, 0.018).mesh().build(), lin(214, 172, 78), at(-0.05, 0.2, 0.075).with_rotation(Quat::from_rotation_y(-0.3)));
        }
        "scout" => {
            person(&mut m, color, 0.0, 0.0);
            m.add(cone(0.06, 0.05, 8), lin(120, 92, 56), at(0.0, 0.36, 0.0));
            m.add(Cuboid::new(0.07, 0.08, 0.05).mesh().build(), lin(130, 100, 64), at(0.0, 0.22, -0.06));
            m.add(cylinder(0.012, 0.09, 6), metal, at(0.07, 0.29, 0.05).with_rotation(Quat::from_rotation_x(1.3)));
        }
        "catapult" => {
            let dark = lin(70, 50, 34);
            // A wheeled frame in the owner's colour, uprights and a raised
            // throwing arm with a stone in its cup; a crewman alongside.
            for x in [-0.06f32, 0.06] {
                m.add(Cuboid::new(0.03, 0.03, 0.26).mesh().build(), wood, at(x, 0.08, 0.0));
                m.add(Cuboid::new(0.02, 0.15, 0.02).mesh().build(), wood, at(x, 0.16, -0.01));
                for z in [-0.08f32, 0.08] {
                    m.add(
                        cylinder(0.042, 0.016, 10),
                        dark,
                        at(x * 1.45, 0.045, z).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                    );
                }
            }
            for z in [-0.1f32, 0.1] {
                m.add(Cuboid::new(0.15, 0.025, 0.03).mesh().build(), color, at(0.0, 0.08, z));
            }
            m.add(Cuboid::new(0.14, 0.02, 0.02).mesh().build(), wood, at(0.0, 0.23, -0.01));
            m.add(Cuboid::new(0.03, 0.03, 0.34).mesh().build(), wood, at(0.0, 0.2, -0.04).with_rotation(Quat::from_rotation_x(-0.9)));
            m.add(blob(0.036, 1), dark, at(0.0, 0.33, 0.065));
            m.add(blob(0.028, 1), lin(150, 144, 136), at(0.0, 0.36, 0.065));
            person(&mut m, color, 0.15, 0.06);
        }
        "composite_bowman" => {
            person(&mut m, color, 0.02, 0.0);
            // A recurve bow (limbs bent back at the tips), a quiver and a leather cap.
            let horn = lin(120, 74, 44);
            for s in [-1.0f32, 1.0] {
                m.add(Cuboid::new(0.012, 0.11, 0.013).mesh().build(), horn, at(0.1, 0.22 + s * 0.055, 0.035).with_rotation(Quat::from_rotation_x(s * 0.4)));
                m.add(Cuboid::new(0.01, 0.05, 0.012).mesh().build(), horn, at(0.1, 0.22 + s * 0.12, 0.035).with_rotation(Quat::from_rotation_x(-s * 0.5)));
            }
            m.add(cylinder(0.025, 0.13, 6), lin(120, 84, 52), at(-0.02, 0.24, -0.06).with_rotation(Quat::from_rotation_x(0.3)));
            m.add(cone(0.05, 0.06, 8), lin(140, 104, 64), at(0.02, 0.37, 0.0));
        }
        "chariot_archer" => {
            // A horse drawing a two-wheeled chariot in the owner's colour, an archer aboard.
            horse(&mut m, lin(150, 110, 70), Transform::from_xyz(0.0, 0.0, 0.14).with_scale(Vec3::splat(0.85)));
            m.add(Cuboid::new(0.13, 0.06, 0.1).mesh().build(), color, at(0.0, 0.12, -0.08));
            m.add(Cuboid::new(0.135, 0.014, 0.105).mesh().build(), lin(214, 172, 78), at(0.0, 0.155, -0.08));
            for x in [-0.08f32, 0.08] {
                wheel(&mut m, 0.065, x, 0.065, -0.08, lin(80, 58, 38));
            }
            m.add(Cuboid::new(0.015, 0.015, 0.2).mesh().build(), wood, at(0.0, 0.11, 0.06));
            let mut rider = Model::default();
            person(&mut rider, color, 0.0, 0.0);
            for s in [-1.0f32, 1.0] {
                rider.add(Cuboid::new(0.01, 0.12, 0.012).mesh().build(), wood, at(0.08, 0.22 + s * 0.055, 0.04).with_rotation(Quat::from_rotation_x(s * 0.35)));
            }
            m.append(&rider, &Transform::from_xyz(0.0, 0.1, -0.09).with_scale(Vec3::splat(0.85)), Vec4::ONE);
        }
        "pikeman" => {
            person(&mut m, color, 0.02, 0.0);
            // A very long pike and a brimmed steel helmet.
            m.add(cylinder(0.008, 0.66, 5), wood, at(0.08, 0.33, 0.03).with_rotation(Quat::from_rotation_x(0.12)));
            m.add(cone(0.016, 0.06, 6), metal, at(0.08, 0.67, 0.07).with_rotation(Quat::from_rotation_x(0.12)));
            m.add(cylinder(0.065, 0.008, 10), metal, at(0.02, 0.345, 0.0));
            m.add(cone(0.045, 0.05, 8), metal, at(0.02, 0.37, 0.0));
            m.add(Cuboid::new(0.08, 0.08, 0.05).mesh().build(), metal, at(0.02, 0.22, 0.02));
        }
        "crossbowman" => {
            person(&mut m, color, 0.02, 0.0);
            // A crossbow levelled forward: stock, steel prod, a kettle hat.
            m.add(Cuboid::new(0.02, 0.022, 0.16).mesh().build(), wood, at(0.06, 0.23, 0.08));
            m.add(Cuboid::new(0.15, 0.012, 0.014).mesh().build(), metal, at(0.06, 0.235, 0.15));
            m.add(Cuboid::new(0.01, 0.008, 0.1).mesh().build(), lin(230, 220, 200), at(0.06, 0.245, 0.11));
            m.add(cylinder(0.06, 0.008, 10), metal, at(0.02, 0.345, 0.0));
            m.add(blob(0.044, 1), metal, at(0.02, 0.35, 0.0));
            m.add(Cuboid::new(0.07, 0.06, 0.04).mesh().build(), lin(130, 100, 64), at(-0.05, 0.2, -0.05));
        }
        "knight" => {
            // A barded horse in the owner's colours, an armoured rider with a lance and shield.
            horse(&mut m, lin(196, 196, 202), Transform::IDENTITY);
            m.add(Cuboid::new(0.1, 0.07, 0.22).mesh().build(), color, at(0.0, 0.16, -0.005));
            m.add(Cuboid::new(0.104, 0.012, 0.224).mesh().build(), lin(214, 172, 78), at(0.0, 0.13, -0.005));
            let mut rider = Model::default();
            person(&mut rider, metal, 0.0, 0.0);
            rider.add(cylinder(0.044, 0.07, 8), metal, at(0.0, 0.33, 0.0));
            rider.add(Cuboid::new(0.07, 0.02, 0.012).mesh().build(), lin(30, 26, 24), at(0.0, 0.335, 0.04));
            m.append(&rider, &Transform::from_xyz(0.0, 0.13, 0.0).with_scale(Vec3::splat(0.85)), Vec4::ONE);
            m.add(cone(0.012, 0.34, 6), lin(150, 112, 70), at(0.07, 0.36, 0.13).with_rotation(Quat::from_rotation_x(1.35)));
            m.add(cone(0.03, 0.04, 8), metal, at(0.07, 0.35, 0.0).with_rotation(Quat::from_rotation_x(1.35)));
            m.add(Cuboid::new(0.012, 0.11, 0.08).mesh().build(), color, at(-0.065, 0.32, 0.0));
            m.add(Cuboid::new(0.014, 0.025, 0.06).mesh().build(), lin(214, 172, 78), at(-0.067, 0.33, 0.0));
        }
        "trebuchet" => {
            let dark = lin(70, 50, 34);
            // A base frame, an A-frame, a long arm with a weight box and a sling.
            for x in [-0.07f32, 0.07] {
                m.add(Cuboid::new(0.025, 0.025, 0.3).mesh().build(), wood, at(x, 0.03, 0.0));
                for s in [-1.0f32, 1.0] {
                    m.add(Cuboid::new(0.02, 0.3, 0.02).mesh().build(), wood, at(x, 0.17, s * 0.05).with_rotation(Quat::from_rotation_x(-s * 0.35)));
                }
            }
            for z in [-0.12f32, 0.12] {
                m.add(Cuboid::new(0.16, 0.025, 0.025).mesh().build(), color, at(0.0, 0.04, z));
            }
            m.add(cylinder(0.012, 0.16, 6), dark, at(0.0, 0.31, 0.0).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)));
            m.add(Cuboid::new(0.025, 0.025, 0.46).mesh().build(), wood, at(0.0, 0.36, 0.04).with_rotation(Quat::from_rotation_x(-0.45)));
            m.add(Cuboid::new(0.07, 0.07, 0.07).mesh().build(), dark, at(0.0, 0.22, -0.13));
            m.add(Cuboid::new(0.005, 0.12, 0.005).mesh().build(), lin(220, 204, 170), at(0.0, 0.42, 0.24));
            m.add(blob(0.026, 1), lin(150, 144, 136), at(0.0, 0.36, 0.24));
            person(&mut m, color, 0.16, 0.08);
        }
        "longswordsman" => {
            person(&mut m, color, 0.02, 0.0);
            // A closed great helm, plate shoulders and a long sword held upright.
            m.add(cylinder(0.046, 0.075, 8), metal, at(0.02, 0.33, 0.0));
            m.add(Cuboid::new(0.07, 0.012, 0.012).mesh().build(), lin(30, 26, 24), at(0.02, 0.335, 0.045));
            for x in [-0.04f32, 0.08] {
                m.add(blob(0.03, 1), metal, at(x, 0.26, 0.0));
            }
            m.add(Cuboid::new(0.014, 0.24, 0.02).mesh().build(), lin(196, 200, 208), at(0.1, 0.32, 0.06));
            m.add(Cuboid::new(0.07, 0.012, 0.02).mesh().build(), lin(150, 120, 60), at(0.1, 0.2, 0.06));
            m.add(Cuboid::new(0.014, 0.06, 0.016).mesh().build(), lin(90, 60, 40), at(0.1, 0.16, 0.06));
        }
        "musketman" => {
            person(&mut m, color, 0.02, 0.0);
            // A brimmed hat, white cross-belts and a musket on the shoulder.
            m.add(cylinder(0.065, 0.01, 10), lin(40, 34, 30), at(0.02, 0.345, 0.0));
            m.add(cylinder(0.04, 0.05, 8), lin(40, 34, 30), at(0.02, 0.37, 0.0));
            for s in [-1.0f32, 1.0] {
                m.add(Cuboid::new(0.012, 0.15, 0.012).mesh().build(), lin(236, 230, 214), at(0.02, 0.22, 0.045).with_rotation(Quat::from_rotation_z(s * 0.6)));
            }
            m.add(cylinder(0.008, 0.36, 5), lin(60, 62, 70), at(0.08, 0.3, 0.02).with_rotation(Quat::from_rotation_x(0.25)));
            m.add(Cuboid::new(0.022, 0.12, 0.03).mesh().build(), wood, at(0.08, 0.16, -0.015).with_rotation(Quat::from_rotation_x(0.25)));
            m.add(cylinder(0.003, 0.07, 4), lin(196, 200, 208), at(0.08, 0.5, 0.075).with_rotation(Quat::from_rotation_x(0.25)));
        }
        "cannon" => {
            // A bronze gun on a carriage in the owner's colour, two big wheels, a gunner.
            m.add(Cuboid::new(0.08, 0.035, 0.26).mesh().build(), color, at(0.0, 0.06, -0.06).with_rotation(Quat::from_rotation_x(-0.25)));
            for x in [-0.075f32, 0.075] {
                wheel(&mut m, 0.075, x, 0.075, 0.02, lin(80, 58, 38));
            }
            barrel(&mut m, 0.2, 0.026, lin(150, 106, 44), Transform::from_xyz(0.0, 0.12, 0.04).with_rotation(Quat::from_rotation_x(-0.15)));
            for (x, z) in [(-0.1f32, -0.16f32), (-0.12, -0.12), (-0.08, -0.12)] {
                m.add(blob(0.022, 1), lin(46, 46, 52), at(x, 0.02, z));
            }
            person(&mut m, color, 0.14, -0.08);
        }
        "worker" => {
            person(&mut m, color, 0.0, 0.0);
            m.add(cylinder(0.01, 0.22, 4), wood, at(0.08, 0.22, 0.02).with_rotation(Quat::from_rotation_z(-0.4)));
            m.add(Cuboid::new(0.1, 0.02, 0.02).mesh().build(), metal, at(0.12, 0.32, 0.02).with_rotation(Quat::from_rotation_z(-0.4)));
            m.add(blob(0.05, 1), lin(196, 176, 130), at(-0.08, 0.08, 0.04));
        }
        "settler" => {
            // A covered wagon with a walking settler in front.
            m.add(Cuboid::new(0.14, 0.06, 0.22).mesh().build(), wood, at(0.02, 0.1, -0.02));
            m.add(
                cylinder(0.075, 0.2, 8),
                lin(232, 224, 200),
                at(0.02, 0.14, -0.02).with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            );
            for (x, z) in [(-0.075, 0.06), (0.115, 0.06), (-0.075, -0.1), (0.115, -0.1)] {
                m.add(
                    cylinder(0.035, 0.015, 8),
                    lin(60, 46, 34),
                    at(x, 0.065, z).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                );
            }
            person(&mut m, color, 0.02, 0.16);
        }
        "merchant" => {
            // A pack mule laden with bales and a striped bundle, led by a
            // merchant in a wide hat with a staff.
            horse(&mut m, lin(126, 104, 86), Transform::from_xyz(0.03, 0.0, -0.04).with_scale(Vec3::splat(0.8)));
            let cloth = [lin(186, 74, 52), lin(214, 170, 82), lin(92, 120, 160)];
            for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
                m.add(Cuboid::new(0.045, 0.07, 0.1).mesh().build(), cloth[i], at(0.03 + side * 0.06, 0.13, -0.05));
            }
            m.add(Cuboid::new(0.1, 0.05, 0.12).mesh().build(), cloth[2], at(0.03, 0.2, -0.05));
            m.add(cylinder(0.025, 0.12, 8), lin(232, 224, 200), at(0.03, 0.24, -0.05).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)));
            m.add(Cuboid::new(0.102, 0.012, 0.03).mesh().build(), color, at(0.03, 0.226, -0.05));
            person(&mut m, color, -0.12, 0.1);
            m.add(cylinder(0.056, 0.008, 10), lin(150, 112, 64), at(-0.12, 0.345, 0.1));
            m.add(cylinder(0.035, 0.04, 8), lin(150, 112, 64), at(-0.12, 0.365, 0.1));
            m.add(cylinder(0.008, 0.36, 5), wood, at(-0.19, 0.19, 0.13).with_rotation(Quat::from_rotation_x(0.15)));
            // The lead rope.
            m.add(cylinder(0.003, 0.17, 3), lin(200, 180, 140), at(-0.05, 0.24, 0.15).with_rotation(Quat::from_rotation_z(1.2)));
        }
        _ => person(&mut m, color, 0.0, 0.0),
    }
    m
}

/// Where on a bridge's deck land units stand (as a `Span` position).
fn deck_spot(slot: Slot) -> f32 {
    match slot {
        Slot::Land | Slot::Sea => 0.72,
        Slot::Civilian => 0.28,
        Slot::Leader => 0.5,
    }
}

/// Where a unit stands on its tile (military and civilian units side by
/// side, a ship behind them; on a bridge, land units stand on its deck and
/// ships moor beside it).
fn spot(session: &Session, terrain: &Terrain, bridges: &Bridges, unit: UnitId, tile: TileId) -> Vec3 {
    let u = &session.game.units[&unit];
    if let Some(span) = bridges.0.get(&tile) {
        let centre = tile_center(&session.game.map.grid, tile);
        let slot = session.game.slot(u);
        return match slot {
            _ if session.game.unit_def(u).domain == Domain::Sea => {
                let p = centre + span.mooring();
                Vec3::new(p.x, terrain.heights.sample(p).max(WATER_LEVEL), p.y)
            }
            _ => {
                let t = deck_spot(slot);
                let p = centre + span.point(t);
                Vec3::new(p.x, span.deck(t), p.y)
            }
        };
    }
    // A General (or an Admiral) stands behind the soldier (or ship) he leads.
    let sea = session.game.unit_def(u).domain == Domain::Sea;
    let offset = match session.game.slot(u) {
        Slot::Sea => Vec2::new(-0.12, -0.3),
        Slot::Land => Vec2::new(0.3, 0.12),
        Slot::Civilian => Vec2::new(-0.36, -0.1),
        Slot::Leader if sea => Vec2::new(0.3, -0.36),
        Slot::Leader => Vec2::new(-0.02, 0.4),
    };
    // Merchants share their tile with other civilians (see `Game::passes`):
    // those after the first stand a step aside.
    const ASIDE: [Vec2; 4] = [Vec2::ZERO, Vec2::new(0.06, 0.34), Vec2::new(0.1, -0.32), Vec2::new(0.34, 0.02)];
    let aside = if session.game.slot(u) == Slot::Civilian {
        let before = session.game.units_at(tile).filter(|o| o.id < unit && session.game.slot(o) == Slot::Civilian).count();
        ASIDE[before % ASIDE.len()]
    } else {
        Vec2::ZERO
    };
    let p = tile_center(&session.game.map.grid, tile) + offset + aside;
    Vec3::new(p.x, terrain.heights.sample(p).max(WATER_LEVEL), p.y)
}

fn sync_units(
    mut commands: Commands,
    session: Res<Session>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    bridges: Res<Bridges>,
    mut events: MessageReader<GameEvent>,
    mut assets: ResMut<UnitAssets>,
    mut stores: LookAssets,
    mut views: Query<(Entity, &mut UnitView)>,
) {
    let me = session.me;
    let viewer = &session.game.players[me as usize];
    let mut by_id: HashMap<UnitId, Entity> = views.iter().map(|(e, v)| (v.id, e)).collect();

    // Queue walking animations for moves we can see, and lunges for melee attacks.
    for GameEvent(event) in events.read() {
        // Attackers play their attack, turned towards the target.
        if let Event::Combat(r) = event {
            if let sim::combat::Attacker::Unit { id, .. } = r.attacker {
                if let Some(Ok((_, mut view))) = by_id.get(&id).map(|e| views.get_mut(*e)) {
                    let grid = &session.game.map.grid;
                    let toward = tile_center(grid, r.tile) - tile_center(grid, r.from);
                    if toward.length() > 1e-3 {
                        view.yaw = toward.x.atan2(toward.y);
                    }
                    view.motion.attack = Some(0.0);
                }
            }
        }
        let melee = match event {
            Event::Combat(r) if !r.ranged => Some(r),
            _ => None,
        };
        if let Some((attacker, r)) = melee.and_then(|r| match r.attacker {
            sim::combat::Attacker::Unit { id, .. } => Some((id, r)),
            sim::combat::Attacker::City(_) | sim::combat::Attacker::Fort(_) => None,
        }) {
            let (attacker, from, tile) = (&attacker, &r.from, &r.tile);
            let Some(&entity) = by_id.get(attacker) else { continue };
            let Ok((_, mut view)) = views.get_mut(entity) else { continue };
            let start = view.queue.back().copied().unwrap_or(view.pos);
            let grid = &session.game.map.grid;
            let toward = tile_center(grid, *tile) - tile_center(grid, *from);
            let mut mid = start + Vec3::new(toward.x, 0.0, toward.y) * 0.45;
            mid.y = bridges.ground(&session.game, &terrain, period.0, mid.xz());
            view.queue.push_back(mid);
            view.queue.push_back(start);
        }
        // An upgraded unit gets its new model (spawned again below).
        if let Event::UnitUpgraded { unit, .. } = event {
            if let Some(entity) = by_id.remove(unit) {
                commands.entity(entity).despawn();
            }
            continue;
        }
        if let Event::UnitMoved { unit, path } = event {
            // Gone already (it moved, then died): its model goes below.
            if !session.game.units.contains_key(unit) {
                continue;
            }
            let Some(&entity) = by_id.get(unit) else { continue };
            let Ok((_, mut view)) = views.get_mut(entity) else { continue };
            let mut prev = view.queue.back().copied().unwrap_or(view.pos);
            let game = &session.game;
            let grid = &game.map.grid;
            let u = &game.units[unit];
            let land = game.unit_def(u).domain == Domain::Land;
            let class = game.slot(u);
            let mut from = grid.tile(hex_at(prev.xz()));
            // A tile's centre, near the last point (across the wrap).
            let near = |tile: TileId, prev: Vec3| {
                let mut c = tile_center(grid, tile);
                c.x += nearest_shift(c.x, prev.x, period.0);
                c
            };
            for &tile in path {
                // Land units go on and off bridges at their ends, along the deck.
                if land {
                    let mut deck = |at: TileId, ts: Vec<f32>, prev: &mut Vec3| {
                        let span = bridges.0[&at];
                        let c = near(at, *prev);
                        for t in ts {
                            let q = c + span.point(t);
                            *prev = Vec3::new(q.x, span.deck(t), q.y);
                            view.queue.push_back(*prev);
                        }
                    };
                    if let Some(f) = from.filter(|f| bridges.0.contains_key(f)) {
                        let span = bridges.0[&f];
                        let c = near(f, prev);
                        let (now, _) = span.project(prev.xz() - c);
                        let end = span.end_towards(near(tile, prev) - c);
                        deck(f, span.walk(now, end), &mut prev);
                    }
                    if let Some(span) = bridges.0.get(&tile).copied() {
                        let c = near(tile, prev);
                        let end = span.end_towards(prev.xz() - c);
                        let mut ts = vec![end];
                        ts.extend(span.walk(end, deck_spot(class)));
                        ts.pop();
                        deck(tile, ts, &mut prev);
                    }
                }
                from = Some(tile);
                let mut p = spot(&session, &terrain, &bridges, *unit, tile);
                // Keep the walk continuous across the east-west wrap.
                p.x += nearest_shift(p.x, prev.x, period.0);
                view.queue.push_back(p);
                prev = p;
            }
        }
    }

    // Spawn units that came into view, remove those that left it.
    for u in session.game.units.values() {
        let shown = u.owner == me || viewer.sees(u.tile);
        match (shown, by_id.get(&u.id)) {
            (true, None) => {
                let art = session.game.unit_def(u).art.clone();
                let color = team(session.game.players[u.owner as usize].color);
                let look = assets.look(&art, u.owner, color, &mut stores);
                let pos = spot(&session, &terrain, &bridges, u.id, u.tile);
                let entity = commands
                    .spawn((
                        UnitView {
                            id: u.id,
                            pos,
                            queue: VecDeque::new(),
                            yaw: initial_yaw(session.game.unit_def(u).domain),
                            afloat: false,
                            motion: rigs::Motion::default(),
                        },
                        Transform::from_translation(pos).with_scale(Vec3::splat(SCALE)),
                        Visibility::default(),
                        Wrapped { anchor: pos.x, base: pos.x },
                    ))
                    .id();
                dress(&mut commands, entity, &look, u.id as f32 * 1.7);
                by_id.insert(u.id, entity);
            }
            (false, Some(&entity)) => {
                commands.entity(entity).despawn();
            }
            _ => {}
        }
    }
    for (entity, view) in &views {
        if !session.game.units.contains_key(&view.id) {
            commands.entity(entity).despawn();
        }
    }
}

/// A flag's health bar or order badge (see `FlagExtras`).
#[derive(Component)]
struct FlagExtra;

fn order_icon(order: Order) -> Option<Icon> {
    match order {
        Order::None | Order::Skip => None,
        Order::Goto(_) => Some(Icon::Moves),
        Order::Take { .. } => Some(Icon::Catapult),
        Order::Defend { .. } => Some(Icon::Walls),
        Order::Hunt { .. } => Some(Icon::Strength),
        Order::Explore => Some(Icon::Explore),
        Order::Fortify => Some(Icon::Defense),
        Order::Sleep => Some(Icon::Sleep),
        Order::Automate => Some(Icon::Auto),
        Order::Build(_) | Order::AutoBuild(_) => Some(Icon::Worker),
        Order::Trade(_) => Some(Icon::Trade),
    }
}

fn update_flags(
    mut commands: Commands,
    session: Res<Session>,
    selected: Res<Selected>,
    period: Res<WrapPeriod>,
    icons: Res<Icons>,
    ui_scale: Res<UiScale>,
    (strategic, terrain, group): (Res<crate::strategic::StrategicView>, Res<Terrain>, Res<crate::play::Group>),
    views: Query<&UnitView>,
    mut flags: Query<(Entity, &mut UnitFlag, &mut Node, &mut Visibility, &mut BorderColor, &mut BackgroundColor, &Children)>,
    extra_nodes: Query<(), With<FlagExtra>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    labels: Single<Entity, With<crate::ui::WorldLabels>>,
) {
    let (cam, cam_tf) = *camera;
    let cam_x = cam_tf.translation().x;
    let positions: HashMap<UnitId, (Vec3, bool)> = views.iter().map(|v| (v.id, (v.pos, v.afloat))).collect();
    let scale = ui_scale.0;
    let screen = crate::ui::ui_screen(cam, scale);

    let mut has_flag = std::collections::HashSet::new();
    for (entity, mut flag, mut node, mut vis, mut border, mut bg, children) in &mut flags {
        let current = |u: &&sim::game::Unit| u.kind == flag.kind;
        let (Some(&(pos, afloat)), Some(u)) = (positions.get(&flag.unit), session.game.units.get(&flag.unit).filter(current)) else {
            commands.entity(entity).despawn();
            continue;
        };
        has_flag.insert(flag.unit);
        // Vessels are lower than people with their banners.
        let vessel = afloat || session.game.unit_def(u).domain == Domain::Sea;
        let mut world = if strategic.0 {
            // No models in the strategic view: the flag stands on the tile.
            Vec3::new(pos.x, crate::strategic::ground(&terrain, pos.xz()) + 0.5, pos.z)
        } else {
            pos + Vec3::Y * if vessel { 1.1 } else { 2.05 }
        };
        world.x += nearest_shift(world.x, cam_x, period.0);
        let at = cam.world_to_viewport(cam_tf, world).ok().map(|s| s / scale + Vec2::new(flag.nudge - 15.0, -15.0));
        crate::ui::place_label(&mut node, &mut vis, at, screen, 40.0);
        let picked = selected.0 == Some(flag.unit) || group.0.contains(&flag.unit);
        let edge = if picked { palette::ACCENT } else { Color::srgba(0.0, 0.0, 0.0, 0.7) };
        border.set_if_neq(BorderColor::all(edge));
        // Our units that have used up their moves fade a little.
        let tired = u.owner == session.me && u.moves == 0;
        let base = crate::unit_panel::player_color(&session, u.owner);
        bg.set_if_neq(BackgroundColor(if tired { base.darker(0.2).with_alpha(0.8) } else { base }));
        if session.is_changed() || flag.extras.is_none() {
            let game = &session.game;
            let order = if game.is_alerted(u) { Some(Icon::Warning) } else { order_icon(u.order) };
            let extras = FlagExtras {
                hp: Some(u.hp).filter(|&hp| hp < sim::combat::MAX_HP),
                order: order.filter(|_| u.owner == session.me),
                members: Vec::new(),
                bar: false,
                rank: game.rank(u),
            };
            if flag.extras.as_ref() != Some(&extras) {
                for child in children.iter().filter(|&c| extra_nodes.contains(c)) {
                    commands.entity(child).despawn();
                }
                commands.entity(entity).with_children(|f| spawn_flag_extras(f, &extras, &icons));
                flag.extras = Some(extras);
            }
        }
    }

    for v in &views {
        if has_flag.contains(&v.id) {
            continue;
        }
        let Some(u) = session.game.units.get(&v.id) else { continue };
        let def = session.game.unit_def(u);
        let [r, g, b] = session.game.players[u.owner as usize].color;
        // Dark glyphs on light colours, light glyphs on dark ones.
        let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
        let fg = if luma > 150.0 { Color::srgb(0.1, 0.09, 0.08) } else { Color::srgb(1.0, 0.98, 0.94) };
        let unit = u.id;
        let nudge = match session.game.slot(u) {
            Slot::Civilian => -17.0,
            Slot::Leader => 0.0,
            _ => 17.0,
        };
        commands
            .spawn((
                UnitFlag { unit, kind: u.kind, nudge, extras: None },
                ChildOf(*labels),
                bevy::ui_widgets::Button,
                Node {
                    position_type: PositionType::Absolute,
                    width: px(30),
                    height: px(30),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(r, g, b)),
                BorderColor::all(Color::BLACK),
                BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), px(0), px(2), px(0), px(4)),
                Visibility::Hidden,
                // Below menus and panels.
                GlobalZIndex(-1),
                match &u.name {
                    Some(name) => Tooltip::new(format!("{} {name}", def.name), session.game.players[u.owner as usize].name.clone()),
                    None => Tooltip::new(def.name.clone(), session.game.players[u.owner as usize].name.clone()),
                },
                bevy::ui_widgets::observe(
                    move |_: On<bevy::ui_widgets::Activate>,
                          session: Res<Session>,
                          mut selected: ResMut<Selected>,
                          mut group: ResMut<crate::play::Group>| {
                        if session.game.units.get(&unit).is_some_and(|u| u.owner == session.me) {
                            selected.0 = Some(unit);
                            group.0.clear();
                        }
                    },
                ),
            ))
            .with_children(|f| {
                f.spawn((
                    ImageNode { color: fg, ..ImageNode::new(icons.white(Icon::unit(&def.art))) },
                    Node { width: px(21), height: px(21), ..default() },
                    Pickable::IGNORE,
                ));
            });
    }
}

/// A flag's health bar (once wounded) and order badge; an army's banner
/// has its member count and a bar split by member.
fn spawn_flag_extras(f: &mut ChildSpawnerCommands, extras: &FlagExtras, icons: &Icons) {
    if !extras.members.is_empty() {
        let n = extras.members.len();
        // An army of one shows its bar only once wounded.
        if extras.bar {
        f.spawn((
            FlagExtra,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(-9),
                left: px(-4),
                width: px(34),
                height: px(6),
                column_gap: px(1),
                padding: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            Pickable::IGNORE,
        ))
        .with_children(|bar| {
            for &hp in &extras.members {
                bar.spawn((
                    Node { flex_grow: 1.0, height: percent(100), ..default() },
                    BackgroundColor(Color::srgba(0.25, 0.22, 0.2, 1.0)),
                    Pickable::IGNORE,
                    children![(
                        Node { width: percent(hp.clamp(0, 100) as f32), height: percent(100), ..default() },
                        BackgroundColor(if hp < 50 { palette::DANGER } else { palette::FOOD }),
                        Pickable::IGNORE,
                    )],
                ));
            }
        });
        }
        f.spawn((
            FlagExtra,
            Node {
                position_type: PositionType::Absolute,
                top: px(-8),
                right: px(-10),
                min_width: px(17),
                height: px(17),
                padding: UiRect::horizontal(px(3)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgb(0.12, 0.1, 0.09)),
            BorderColor::all(palette::ACCENT),
            Pickable::IGNORE,
            children![(crate::ui::bold(n.to_string(), 11.0, palette::ACCENT), Pickable::IGNORE)],
        ));
    }
    if extras.rank > 0 {
        // A chevron per rank, down the flag's left side.
        f.spawn((
            FlagExtra,
            Node {
                position_type: PositionType::Absolute,
                top: px(-8),
                left: px(-10),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(-5),
                padding: UiRect::new(px(1), px(1), px(2), px(2)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.12, 0.1, 0.09)),
            BorderColor::all(palette::ACCENT),
            Pickable::IGNORE,
        ))
        .with_children(|col| {
            for _ in 0..extras.rank {
                col.spawn((ImageNode::new(icons.get(Icon::Rank)), Node { width: px(13), height: px(11), ..default() }, Pickable::IGNORE));
            }
        });
    }
    let extras = extras.clone();
    if let Some(hp) = extras.hp {
        f.spawn((
            FlagExtra,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(-9),
                left: px(1),
                width: px(24),
                height: px(5),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            BorderColor::all(Color::BLACK),
            Pickable::IGNORE,
            children![(
                Node { width: percent(hp as f32), height: percent(100), ..default() },
                BackgroundColor(if hp < 50 { palette::DANGER } else { palette::FOOD }),
                Pickable::IGNORE,
            )],
        ));
    }
    if let Some(icon) = extras.order {
        f.spawn((
            FlagExtra,
            ImageNode::new(icons.get(icon)),
            Node { position_type: PositionType::Absolute, right: px(-9), bottom: px(-7), width: px(16), height: px(16), ..default() },
            Pickable::IGNORE,
        ));
    }
}

fn animate_units(
    time: Res<Time>,
    session: Res<Session>,
    terrain: Res<Terrain>,
    bridges: Res<Bridges>,
    period: Res<WrapPeriod>,
    mut views: Query<(&mut UnitView, &mut Transform, &mut Wrapped)>,
) {
    let dt = time.delta_secs();
    for (mut view, mut tf, mut wrapped) in &mut views {
        let walking = !view.queue.is_empty();
        let before = view.pos;
        let mut step = SPEED * dt;
        while step > 0.0 {
            let Some(&target) = view.queue.front() else {
                // Idle: stand on the unit's tile (also covers moves we didn't watch).
                if let Some(u) = session.game.units.get(&view.id) {
                    view.pos = spot(&session, &terrain, &bridges, view.id, u.tile);
                    // Ships moored at a bridge lie alongside it.
                    if let Some(span) = bridges.0.get(&u.tile).filter(|_| session.game.unit_def(u).domain == Domain::Sea) {
                        let along = span.ends[1] - span.ends[0];
                        let yaw = along.x.atan2(along.y);
                        let turn = |y: f32| (y - view.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                        let (a, b) = (turn(yaw), turn(yaw + std::f32::consts::PI));
                        view.yaw += if a.abs() <= b.abs() { a } else { b };
                    }
                }
                break;
            };
            let delta = target - view.pos;
            let dist = delta.xz().length();
            if dist > 1e-4 {
                view.yaw = delta.x.atan2(delta.z);
            }
            if dist <= step {
                view.pos = target;
                view.queue.pop_front();
                step -= dist;
            } else {
                let p = view.pos + delta * (step / dist);
                view.pos = p;
                step = 0.0;
            }
        }
        let moved = (view.pos - before).xz().length();
        let m = &mut view.motion;
        m.travel += moved;
        m.walk += ((if walking { 1.0 } else { 0.0 }) - m.walk) * (dt * 8.0).min(1.0);
        m.time = time.elapsed_secs();
        m.attack = m.attack.map(|a| a + dt).filter(|a| *a < rigs::ATTACK_TIME);
        // Follow the ground while walking (ships stay on the water under bridges).
        let land = session.game.units.get(&view.id).is_some_and(|u| session.game.unit_def(u).domain == Domain::Land);
        let ground = if land {
            bridges.ground(&session.game, &terrain, period.0, view.pos.xz())
        } else {
            terrain.heights.sample(view.pos.xz()).max(WATER_LEVEL)
        };
        tf.translation.y = ground;
        tf.translation.z = view.pos.z;
        tf.rotation = Quat::from_rotation_y(view.yaw);
        let scale = Vec3::splat(SCALE);
        if tf.scale != scale {
            tf.scale = scale;
        }
        *wrapped = Wrapped { anchor: view.pos.x, base: view.pos.x };
    }
}

/// Which way a unit faces before it first moves: people face the camera,
/// ships turn side-on so their profile shows.
fn initial_yaw(domain: Domain) -> f32 {
    match domain {
        Domain::Land => 0.0,
        Domain::Sea => -1.15,
    }
}

/// Land units on water ride in a boat (not on bridges): swaps the model when one embarks or lands.
fn float_units(
    mut commands: Commands,
    session: Res<Session>,
    terrain: Res<Terrain>,
    bridges: Res<Bridges>,
    period: Res<WrapPeriod>,
    mut assets: ResMut<UnitAssets>,
    mut stores: LookAssets,
    mut views: Query<(Entity, &mut UnitView, &UnitBody)>,
) {
    for (entity, mut view, body) in &mut views {
        let Some(u) = session.game.units.get(&view.id) else { continue };
        let def = session.game.unit_def(u);
        let afloat = def.domain == Domain::Land
            && terrain.heights.sample(view.pos.xz()) < WATER_LEVEL - 0.02
            && bridges.deck_at(&session.game, period.0, view.pos.xz()).is_none();
        if afloat == view.afloat {
            continue;
        }
        view.afloat = afloat;
        // A boat that hasn't sailed anywhere yet lies side-on, like ships.
        if afloat && view.queue.is_empty() && view.yaw == 0.0 {
            view.yaw = initial_yaw(Domain::Sea);
        }
        let art = if afloat { BOAT.to_string() } else { def.art.clone() };
        let color = team(session.game.players[u.owner as usize].color);
        let look = assets.look(&art, u.owner, color, &mut stores);
        for part in &body.0 {
            commands.entity(*part).despawn();
        }
        dress(&mut commands, entity, &look, u.id as f32 * 1.7);
    }
}

/// Recomputes the path preview when the selection or hovered tile changes.
fn update_path_preview(
    session: Res<Session>,
    selected: Res<Selected>,
    hovered: Res<HoveredTile>,
    mut preview: ResMut<PathPreview>,
) {
    let me = session.me;
    // Merchants only follow their routes: no path to preview.
    let unit = selected.0.filter(|u| session.game.units.get(u).is_some_and(|u| u.owner == me && !session.game.is_merchant(u)));
    if preview.for_unit == unit && preview.to == hovered.0 && !session.is_changed() {
        return;
    }
    preview.for_unit = unit;
    preview.to = hovered.0;
    preview.path = match (unit, hovered.0) {
        // On an enemy out of reach: the way to where it can attack from.
        (Some(u), Some(to)) => session.game.path_for(u, to).or_else(|| {
            let game = &session.game;
            // A city it can't enter: the way to the tile next to it.
            if let Some((_, p)) = game.approach_city(u, to) {
                return Some(p);
            }
            game.hunt_target(me, to)?;
            let def = game.unit_def(&game.units[&u]);
            if def.class != sim::rules::UnitClass::Military || (def.strength <= 0 && def.ranged <= 0) {
                return None;
            }
            game.approach(u, to).map(|(_, p)| p).filter(|p| !p.tiles.is_empty())
        }),
        _ => None,
    };
}

/// Hex ring around the selected unit, and the path preview with turn numbers.
fn draw_markers(
    mut commands: Commands,
    mut gizmos: Gizmos,
    session: Res<Session>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    selected: Res<Selected>,
    preview: Res<PathPreview>,
    ui_scale: Res<UiScale>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    labels: Query<Entity, With<PathLabel>>,
) {
    let (cam, cam_tf) = *camera;
    let cam_x = cam_tf.translation().x;
    let grid = &session.game.map.grid;
    // World point for drawing near the camera (wrapped) and slightly above ground.
    let place = |p: Vec2, lift: f32| {
        let y = terrain.heights.sample(p).max(WATER_LEVEL) + lift;
        Vec3::new(p.x + nearest_shift(p.x, cam_x, period.0), y, p.y)
    };
    let hex_ring = |gizmos: &mut Gizmos, tile: TileId, inset: f32, color: Color| {
        let c = tile_center(grid, tile);
        let pts: Vec<Vec3> = (0..=6).map(|i| place(c + corner(i % 6, inset), 0.05)).collect();
        gizmos.linestrip(pts, color);
    };

    for e in &labels {
        commands.entity(e).despawn();
    }
    let Some(unit) = selected.0 else { return };
    let Some(u) = session.game.units.get(&unit) else { return };

    // A merchant's route: its two cities ringed, and the way it's taking.
    if let Order::Trade(route) = u.order {
        let trade = Color::srgba(0.45, 0.85, 0.95, 0.9);
        for c in [route.home, route.to].iter().filter_map(|c| session.game.cities.get(c)) {
            hex_ring(&mut gizmos, c.tile, 0.4, trade);
        }
        if let Some(way) = session.game.trade_way(unit) {
            let at = way.iter().position(|&t| t == u.tile).unwrap_or(0);
            let pts: Vec<Vec3> = way[at..].iter().map(|&t| place(tile_center(grid, t), 0.1)).collect();
            gizmos.linestrip(pts, trade);
        }
    }

    let Some(path) = &preview.path else {
        if let Some(to) = preview.to.filter(|&t| t != u.tile && u.owner == session.me) {
            hex_ring(&mut gizmos, to, 0.2, Color::srgba(0.9, 0.25, 0.2, 0.8));
        }
        return;
    };
    // Path line: bright for this turn, gold for later turns.
    let mut prev = place(tile_center(grid, u.tile), 0.08);
    for (i, &tile) in path.tiles.iter().enumerate() {
        let p = place(tile_center(grid, tile), 0.08);
        let color = if path.turns[i] == 0 { Color::srgb(1.0, 1.0, 0.95) } else { palette::ACCENT };
        gizmos.line(prev, p, color);
        prev = p;
        let turn_ends = i + 1 == path.tiles.len() || path.turns[i + 1] != path.turns[i];
        if turn_ends {
            hex_ring(&mut gizmos, tile, 0.55, color);
            if let Ok(screen) = cam.world_to_viewport(cam_tf, p) {
                commands.spawn((
                    PathLabel,
                    Text::new(format!("{}", path.turns[i] + 1)),
                    crate::ui::font_bold(14.0),
                    TextColor(Color::BLACK),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(screen.x / ui_scale.0 - 9.0),
                        top: px(screen.y / ui_scale.0 - 30.0),
                        padding: UiRect::axes(px(5), px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(if path.turns[i] == 0 { Color::srgb(1.0, 1.0, 0.95) } else { palette::ACCENT }),
                    // Labels must not catch clicks meant for the map, and stay below panels.
                    Pickable::IGNORE,
                    GlobalZIndex(-1),
                ));
            }
        }
    }
}

/// The marker on the selected unit's tile: a gold ring and a light gold
/// tint over the whole tile, draped over the terrain. The ring also has an x-ray copy so it shows through trees and hills.
#[derive(Component)]
struct SelectionMarker {
    tile: Option<TileId>,
    /// Ring (glowing outline) or fill (tint over the tile).
    ring: bool,
}

#[derive(Resource)]
struct SelectionMaterials {
    ring: Handle<StandardMaterial>,
}

fn spawn_selection_marker(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut xray: ResMut<Assets<XRayMaterial>>,
) {
    let ring = materials.add(StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::new(1.6, 1.05, 0.3, 0.95)),
        // HDR emissive: picked up by bloom as a soft glow.
        emissive: LinearRgba::rgb(1.6, 1.05, 0.3),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        depth_bias: 20.0,
        ..default()
    });
    let fill = materials.add(StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::new(1.0, 0.7, 0.25, 0.22)),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        depth_bias: 15.0,
        ..default()
    });
    let marker = |ring: bool| {
        (
            SelectionMarker { tile: None, ring },
            Transform::default(),
            Wrapped::mesh(0.0),
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
            Visibility::Hidden,
        )
    };
    let placeholder = || Mesh::from(Triangle3d::default());
    commands.spawn((marker(true), Mesh3d(meshes.add(placeholder())), MeshMaterial3d(ring.clone()), crate::strategic::both_layers()));
    commands.spawn((marker(false), Mesh3d(meshes.add(placeholder())), MeshMaterial3d(fill), crate::strategic::both_layers()));
    let see_through = xray.add(XRayMaterial {
        base: StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::new(1.3, 0.85, 0.25, 0.6)),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        },
        extension: XRay {},
    });
    commands.spawn((marker(true), Mesh3d(meshes.add(placeholder())), MeshMaterial3d(see_through), crate::strategic::both_layers()));
    commands.insert_resource(SelectionMaterials { ring });
}

/// A filled hex draped over the terrain (each sector subdivided into a grid).
fn hex_fill(heights: &crate::terrain::height::HeightField, center: Vec2, inset: f32, lift: f32) -> Mesh {
    const N: usize = 6;
    let mut positions = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for sector in 0..6 {
        let (a, b) = (corner(sector, inset), corner((sector + 1) % 6, inset));
        let base = positions.len() as u32;
        // Points c + (i/N) * lerp(a, b, j/i) for rows i = 0..=N.
        let mut row_start = Vec::new();
        for i in 0..=N {
            row_start.push(positions.len() as u32 - base);
            for j in 0..=i {
                let t = if i == 0 { 0.0 } else { j as f32 / i as f32 };
                let p = center + a.lerp(b, t) * (i as f32 / N as f32);
                positions.push([p.x, heights.sample(p).max(WATER_LEVEL) + lift, p.y]);
            }
        }
        for i in 0..N {
            let (r0, r1) = (base + row_start[i], base + row_start[i + 1]);
            for j in 0..=i as u32 {
                indices.extend_from_slice(&[r0 + j, r1 + j, r1 + j + 1]);
                if j < i as u32 {
                    indices.extend_from_slice(&[r0 + j, r1 + j + 1, r0 + j + 1]);
                }
            }
        }
    }
    let n = positions.len();
    Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
        .with_inserted_indices(bevy::mesh::Indices::U32(indices))
}

fn update_selection_marker(
    time: Res<Time>,
    session: Res<Session>,
    terrain: Res<Terrain>,
    selected: Res<Selected>,
    views: Query<&UnitView>,
    selection: Res<SelectionMaterials>,
    mut markers: Query<(&mut SelectionMarker, &Mesh3d, &mut Visibility, &mut Wrapped)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let grid = &session.game.map.grid;
    // The tile the selected unit stands on (or is walking through).
    let tile = selected.0.and_then(|id| {
        let u = session.game.units.get(&id)?;
        let walking = views.iter().find(|v| v.id == id).and_then(|v| grid.tile(crate::layout::hex_at(v.pos.xz())));
        Some(walking.unwrap_or(u.tile))
    });

    // Gentle pulse on the glow.
    let pulse = 0.8 + 0.2 * (time.elapsed_secs() * 3.0).sin();
    if let Some(mut ring) = materials.get_mut(&selection.ring) {
        ring.emissive = LinearRgba::rgb(1.6 * pulse, 1.05 * pulse, 0.3 * pulse);
    }

    for (mut marker, mesh, mut vis, mut wrapped) in &mut markers {
        if marker.tile == tile {
            continue;
        }
        marker.tile = tile;
        let Some(tile) = tile else {
            *vis = Visibility::Hidden;
            continue;
        };
        let c = tile_center(grid, tile);
        let new_mesh = if marker.ring {
            let mut ribbons = crate::overlay::Ribbons::new();
            for i in 0..6 {
                ribbons.edge(&terrain.heights, c + corner(i, 0.08), c + corner((i + 1) % 6, 0.08), 0.05, 0.06);
            }
            ribbons.into_mesh()
        } else {
            hex_fill(&terrain.heights, c, 0.05, 0.04)
        };
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            *m = new_mesh;
        }
        *wrapped = Wrapped::mesh(c.x);
        *vis = Visibility::Inherited;
    }
}

/// Poses each rigged unit's joints for what it is doing.
fn pose_units(views: Query<(&UnitView, &rigs::Animated)>, mut joints: Query<&mut Transform, (With<rigs::RigJoint>, Without<UnitView>)>) {
    for (view, animated) in &views {
        let pose = rigs::pose(&animated.rig, view.motion, animated.phase);
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
