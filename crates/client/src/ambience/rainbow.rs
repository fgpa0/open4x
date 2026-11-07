//! A rainbow when rain clears where the camera looks by day: a faint arc of
//! colour standing over the land ahead, facing the camera, that fades in,
//! lingers and fades away (or goes when the rain comes back).
//! `FOURX_RAINBOW=1` shows one at once.

use super::{smooth, AmbienceSet, Stage};
use crate::loading::{AppState, InGameSet};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// Seconds it fades in, stays and fades out.
const LIFE: (f32, f32, f32) = (6.0, 45.0, 12.0);

pub fn plugin(app: &mut App) {
    app.add_systems(OnEnter(AppState::InGame), spawn).add_systems(Update, shine.in_set(InGameSet).in_set(AmbienceSet::Emit));
}

#[derive(Component)]
struct Rainbow {
    /// Seconds since it appeared, if it's up.
    age: Option<f32>,
    /// Seconds since it last rained here, and since the last rainbow.
    dry: f32,
    since: f32,
    rained: bool,
    material: Handle<StandardMaterial>,
}

fn spawn(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE.with_alpha(0.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let forced = std::env::var("FOURX_RAINBOW").is_ok();
    commands.spawn((
        Rainbow { age: forced.then_some(LIFE.0), dry: 0.0, since: 1000.0, rained: forced, material: material.clone() },
        Mesh3d(meshes.add(arc())),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Visibility::Hidden,
        NotShadowCaster,
    ));
}

/// A half ring of radius 1 in the XY plane, red outside to violet inside,
/// fading towards its feet.
fn arc() -> Mesh {
    const BANDS: [[f32; 3]; 7] = [[0.55, 0.2, 0.9], [0.3, 0.3, 1.0], [0.2, 0.7, 1.0], [0.3, 0.95, 0.35], [1.0, 0.95, 0.25], [1.0, 0.6, 0.15], [1.0, 0.2, 0.15]];
    let (inner, outer) = (0.9, 1.0);
    let steps = 64;
    let rings = BANDS.len() + 1;
    let (mut pos, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..=steps {
        let a = 0.08 * std::f32::consts::PI + (i as f32 / steps as f32) * 0.84 * std::f32::consts::PI;
        // Thinner towards the feet, where it meets the haze.
        let feet = smooth(a.sin() / 0.6);
        for r in 0..rings {
            let f = r as f32 / (rings - 1) as f32;
            let rad = inner + (outer - inner) * f;
            pos.push([a.cos() * rad, a.sin() * rad, 0.0]);
            let band = BANDS[((f * BANDS.len() as f32) as usize).min(BANDS.len() - 1)];
            // Soft edges inside and out.
            let edge = smooth(f / 0.25) * smooth((1.0 - f) / 0.25);
            col.push([band[0], band[1], band[2], edge * feet]);
        }
        if i > 0 {
            for r in 0..rings as u32 - 1 {
                let (a0, b0) = ((i as u32 - 1) * rings as u32 + r, i as u32 * rings as u32 + r);
                idx.extend([a0, b0, b0 + 1, a0, b0 + 1, a0 + 1]);
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; col.len()])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

fn shine(
    stage: Res<Stage>,
    weather: Res<crate::weather::LocalWeather>,
    terrain: Res<Terrain>,
    camera: Single<&GlobalTransform, With<crate::camera::MainCamera>>,
    mut bow: Query<(&mut Rainbow, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((mut r, mut tf, mut vis)) = bow.single_mut() else { return };
    let dt = stage.dt;
    r.since += dt;
    // Remember rain until it has been dry for a minute.
    if weather.rain > 0.25 {
        r.rained = true;
        r.dry = 0.0;
    } else {
        r.dry += dt;
        if r.dry > 60.0 && r.age.is_none() {
            r.rained = false;
        }
    }
    let clearing = r.rained && weather.rain < 0.06 && weather.cover < 0.8 && r.dry > 3.0;
    let daylight = stage.night < 0.05 && stage.day > 0.05 && stage.day < 0.55;
    if r.age.is_none() && clearing && daylight && stage.on && r.since > 240.0 {
        r.age = Some(0.0);
        r.since = 0.0;
    }
    let Some(age) = r.age.as_mut() else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    let (fade_in, stay, fade_out) = LIFE;
    *age += dt;
    // Rain coming back or nightfall ends it early: it starts fading out.
    if (weather.rain > 0.3 || !daylight || !stage.on) && *age > fade_in && *age < fade_in + stay {
        *age = fade_in + stay;
    }
    let age = *age;
    let level = smooth(age / fade_in) * (1.0 - smooth((age - fade_in - stay) / fade_out));
    if age > fade_in + stay + fade_out {
        r.age = None;
        r.rained = false;
        vis.set_if_neq(Visibility::Hidden);
        return;
    }
    vis.set_if_neq(Visibility::Inherited);
    // Over the land ahead of the focus, facing the camera.
    let d = stage.distance;
    let anchor = stage.focus + Vec2::new(0.0, -0.18 * d);
    let ground = terrain.heights.sample(anchor).max(WATER_LEVEL);
    tf.translation = Vec3::new(anchor.x, ground - 0.05 * d, anchor.y);
    tf.rotation = camera.rotation();
    tf.scale = Vec3::splat(0.5 * d);
    if let Some(mut m) = materials.get_mut(&r.material) {
        m.base_color = Color::WHITE.with_alpha(0.32 * level);
    }
}
