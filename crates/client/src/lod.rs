//! Level of detail: each area of the world has a detailed and a simplified mesh;
//! exactly one is shown, chosen by distance from the camera. (Bevy's built-in
//! visibility ranges crossfade with dithering, which looked noisy on dense
//! foliage.)

use crate::loading::InGameSet;
use crate::wrap::{nearest_shift, WrapPeriod};
use bevy::prelude::*;
use bevy::transform::TransformSystems;

/// Not drawn at all, regardless of distance (e.g. plants in unexplored land).
#[derive(Component)]
pub struct Dormant;

#[derive(Component, Clone, Copy)]
pub struct Lod {
    /// World-space centre of the area (unwrapped).
    pub center: Vec3,
    /// Whether this is the detailed level.
    pub near: bool,
    /// Camera distance at which the detailed level gives way to the simple one.
    pub switch: f32,
}

pub struct LodPlugin;

impl Plugin for LodPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, update_lod.before(TransformSystems::Propagate).in_set(InGameSet));
    }
}

fn update_lod(
    period: Res<WrapPeriod>,
    camera: Single<&Transform, With<crate::camera::MainCamera>>,
    mut q: Query<(&Lod, &mut Visibility, Has<Dormant>)>,
) {
    let cam = camera.translation;
    q.par_iter_mut().for_each(|(lod, mut vis, dormant)| {
        let mut c = lod.center;
        c.x += nearest_shift(c.x, cam.x, period.0);
        let near_enough = cam.distance(c) < lod.switch;
        let shown = near_enough == lod.near && !dormant;
        let target = if shown { Visibility::Inherited } else { Visibility::Hidden };
        vis.set_if_neq(target);
    });
}
