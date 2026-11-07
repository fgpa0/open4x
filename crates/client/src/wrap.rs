//! East-west map wrapping. Every world object exists once; each frame it is moved
//! to whichever copy (shifted by whole map widths) is nearest the camera. The view
//! is always narrower than half the map, so this shows a seamless endless world.

use crate::loading::InGameSet;
use bevy::prelude::*;
use bevy::transform::TransformSystems;

/// Marks an entity as part of the wrapping world.
#[derive(Component, Clone, Copy)]
pub struct Wrapped {
    /// X position used to decide which copy is nearest (e.g. the chunk centre).
    pub anchor: f32,
    /// Transform X of the original copy.
    pub base: f32,
}

impl Wrapped {
    /// For meshes built in world coordinates with an identity transform.
    pub fn mesh(anchor: f32) -> Self {
        Self { anchor, base: 0.0 }
    }
}

#[derive(Resource)]
pub struct WrapPeriod(pub f32);

pub struct WrapPlugin;

impl Plugin for WrapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, wrap_entities.before(TransformSystems::Propagate).in_set(InGameSet));
    }
}

/// Shift (a whole number of periods) that brings `anchor` nearest to `camera_x`.
pub fn nearest_shift(anchor: f32, camera_x: f32, period: f32) -> f32 {
    period * ((camera_x - anchor) / period).round()
}

fn wrap_entities(
    period: Res<WrapPeriod>,
    camera: Single<&Transform, (With<crate::camera::MainCamera>, With<Camera3d>)>,
    mut q: Query<(&Wrapped, &mut Transform), Without<Camera3d>>,
) {
    let cam_x = camera.translation.x;
    q.par_iter_mut().for_each(|(w, mut tf)| {
        let x = w.base + nearest_shift(w.anchor, cam_x, period.0);
        // Only write on change so unchanged entities skip transform propagation.
        if tf.translation.x != x {
            tf.translation.x = x;
        }
    });
}
