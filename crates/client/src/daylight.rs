//! Day and night in real time, independent of turns. One directional light is
//! the sun by day and the moon by night (so there is always exactly one
//! shadow-casting light); it swaps over during dawn and dusk, while it is dim.
//! The ambient light, the sky colour and the distance haze follow the same
//! clock, and town windows glow at night.
//!
//! Night stays readable: it is a cool, moonlit blue, never dark, and the moon
//! hangs in the south so the faces the camera sees are lit. The terrain, water
//! and vegetation shaders read the light from Bevy's view uniforms, so the fog
//! of war clouds follow it too (see `fog_sky` in the shaders).
//!
//! `FOURX_TIME=0.0..1.0` pins the time of day (0 is the start of dawn, 0.33
//! noon, 0.8 the middle of the night).

use crate::loading::AppState;
use crate::settings::Settings;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use std::f32::consts::PI;

/// The illuminance of the noon sun; the shaders' `FOG_NOON_SUN` is this times
/// the noon colour (and `FOG_NOON_SKY` the noon ambient light).
const NOON_LUX: f32 = 11_000.0;
/// When a game starts: late morning, already in full daylight.
const START: f32 = 0.2;
/// Where the clock stays when the cycle is turned off.
const ALWAYS_DAY: f32 = 0.33;

/// Parts of the day (fractions of the cycle). The sun is up from 0 to
/// `DUSK_END`; dawn and dusk are when the light turns from moon to sun and back.
const DAWN_END: f32 = 0.07;
const DUSK_START: f32 = 0.6;
const DUSK_END: f32 = 0.67;

pub struct DaylightPlugin;

impl Plugin for DaylightPlugin {
    fn build(&self, app: &mut App) {
        let pinned = std::env::var("FOURX_TIME").ok().and_then(|t| t.parse::<f32>().ok()).map(|t| t.rem_euclid(1.0));
        app.insert_resource(DayClock { t: pinned.unwrap_or(START), pinned })
            .insert_resource(ClearColor(Color::srgb(0.62, 0.74, 0.86)))
            .insert_resource(GlobalAmbientLight::default())
            .add_systems(Startup, (spawn_sun, create_glow))
            .add_systems(OnEnter(AppState::InGame), restart)
            .add_systems(Update, tick.run_if(in_state(AppState::InGame)))
            // After the settings (which re-insert the distance fog) and before
            // transforms propagate, so the light moves this frame.
            .add_systems(PostUpdate, apply.before(TransformSystems::Propagate));
    }
}

/// The time of day, 0..1 (0 is the start of dawn).
#[derive(Resource)]
pub struct DayClock {
    pub t: f32,
    /// Set by `FOURX_TIME`: the clock doesn't move.
    pinned: Option<f32>,
}

/// The material of lit windows in towns (its glow follows the night).
#[derive(Resource)]
pub struct WindowGlow(pub Handle<StandardMaterial>);

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        DirectionalLight { illuminance: NOON_LUX, shadow_maps_enabled: true, ..default() },
        light_transform(ALWAYS_DAY),
        // Cascades are fitted to the view each frame (see settings::fit_shadow_cascades).
        CascadeShadowConfigBuilder::default().build(),
    ));
}

fn create_glow(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(WindowGlow(materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.4,
        ..default()
    })));
}

fn restart(mut clock: ResMut<DayClock>) {
    clock.t = clock.pinned.unwrap_or(START);
}

fn tick(time: Res<Time>, settings: Res<Settings>, mut clock: ResMut<DayClock>) {
    if clock.pinned.is_some() {
        return;
    }
    if settings.day_length == 0 {
        clock.t = ALWAYS_DAY;
        return;
    }
    clock.t = (clock.t + time.delta_secs() / (settings.day_length as f32 * 60.0)).rem_euclid(1.0);
}

/// The light at one time of day. Colours are sRGB.
#[derive(Clone, Copy)]
struct Sky {
    light: Vec3,
    lux: f32,
    ambient: Vec3,
    ambient_brightness: f32,
    /// The clear colour and the distance haze.
    horizon: Vec3,
}

const fn sky(light: [f32; 3], lux: f32, ambient: [f32; 3], ambient_brightness: f32, horizon: [f32; 3]) -> Sky {
    Sky {
        light: Vec3::from_array(light),
        lux,
        ambient: Vec3::from_array(ambient),
        ambient_brightness,
        horizon: Vec3::from_array(horizon),
    }
}

const NOON: Sky = sky([1.0, 0.95, 0.86], NOON_LUX, [0.75, 0.85, 1.0], 900.0, [0.62, 0.74, 0.86]);
const GOLDEN: Sky = sky([1.0, 0.87, 0.7], 9_000.0, [0.76, 0.82, 0.98], 870.0, [0.68, 0.73, 0.82]);
const SUNRISE: Sky = sky([1.0, 0.74, 0.56], 5_000.0, [0.7, 0.68, 0.9], 780.0, [0.8, 0.66, 0.64]);
const SUNSET: Sky = sky([1.0, 0.7, 0.5], 5_000.0, [0.68, 0.64, 0.9], 780.0, [0.78, 0.6, 0.6]);
const BLUE_HOUR: Sky = sky([0.7, 0.74, 0.95], 4_200.0, [0.66, 0.7, 0.94], 760.0, [0.36, 0.4, 0.54]);
// Bright and only a little blue, so the land and its colours still read.
const NIGHT: Sky = sky([0.68, 0.76, 1.0], 4_800.0, [0.64, 0.7, 0.92], 800.0, [0.2, 0.25, 0.38]);

/// The sky through the day, blended between neighbours (wrapping at 1).
const KEYS: [(f32, Sky); 10] = [
    (0.0, BLUE_HOUR),
    (0.035, SUNRISE),
    (0.1, GOLDEN),
    (0.2, NOON),
    (0.47, NOON),
    (0.56, GOLDEN),
    (0.635, SUNSET),
    (DUSK_END, BLUE_HOUR),
    (0.74, NIGHT),
    (0.96, NIGHT),
];

fn srgb(c: Vec3) -> Vec3 {
    let l = Color::srgb(c.x, c.y, c.z).to_linear();
    Vec3::new(l.red, l.green, l.blue)
}

fn linear(c: Vec3) -> Color {
    Color::LinearRgba(LinearRgba::rgb(c.x, c.y, c.z))
}

/// The sky at time `t`: colours blend in linear space, brightness by ratio.
fn sky_at(t: f32) -> (Color, f32, Color, f32, Color) {
    let i = KEYS.iter().rposition(|(k, _)| *k <= t).unwrap_or(KEYS.len() - 1);
    let (t0, a) = KEYS[i];
    let (t1, b) = if i + 1 < KEYS.len() { KEYS[i + 1] } else { (KEYS[0].0 + 1.0, KEYS[0].1) };
    let f = smooth((t - t0) / (t1 - t0));
    let mix3 = |x: Vec3, y: Vec3| linear(srgb(x).lerp(srgb(y), f));
    let mix1 = |x: f32, y: f32| x * (y / x).powf(f);
    (
        mix3(a.light, b.light),
        mix1(a.lux, b.lux),
        mix3(a.ambient, b.ambient),
        mix1(a.ambient_brightness, b.ambient_brightness),
        mix3(a.horizon, b.horizon),
    )
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Direction towards a body at `yaw` around the vertical (0 is south, positive
/// east) and `elevation` above the horizon.
fn towards(yaw: f32, elevation: f32) -> Vec3 {
    Vec3::new(yaw.sin() * elevation.cos(), elevation.sin(), yaw.cos() * elevation.cos())
}

/// Where the light comes from at time `t`. The sun rises in the south-east,
/// stands in the south-west at noon (43°, the angle the relief was made to
/// read in) and sets in the north-west, never below 15° so shadows stay short
/// enough; the moon crosses the southern sky. They swap during dawn and dusk.
fn light_direction(t: f32) -> Vec3 {
    let s = t / DUSK_END;
    let sun = towards(0.75 - 2.9 * s, (15.0 + 28.0 * (PI * s.clamp(0.0, 1.0)).sin()).to_radians());
    // The moon is up from the start of dusk to the end of dawn.
    let m = (t - DUSK_START).rem_euclid(1.0) / (1.0 - DUSK_START + DAWN_END);
    let moon = towards(1.1 - 1.6 * m, (22.0 + 26.0 * (PI * m.clamp(0.0, 1.0)).sin()).to_radians());
    sun.lerp(moon, moonlight(t)).normalize()
}

/// How much of the light is the moon's (0 by day, 1 at night).
fn moonlight(t: f32) -> f32 {
    if t < DUSK_START {
        1.0 - smooth(t / DAWN_END)
    } else {
        smooth((t - DUSK_START) / (DUSK_END - DUSK_START))
    }
}

/// The clock time (hours, minutes) at time of day `t`: noon when the sun is
/// highest, the sun up from 04:00 to 20:00.
pub fn clock(t: f32) -> (u32, u32) {
    let hours = if t < DUSK_END { 4.0 + t / DUSK_END * 16.0 } else { 20.0 + (t - DUSK_END) / (1.0 - DUSK_END) * 8.0 };
    let minutes = (hours * 60.0) as u32 % (24 * 60);
    (minutes / 60, minutes % 60)
}

/// How dark it is, for lit windows: 0 by day, 1 from late dusk to early dawn.
pub fn night(t: f32) -> f32 {
    if t < 0.3 {
        1.0 - smooth((t - 0.01) / 0.05)
    } else {
        smooth((t - 0.6) / 0.05)
    }
}

fn light_transform(t: f32) -> Transform {
    Transform::IDENTITY.looking_to(-light_direction(t), Vec3::Y)
}

fn apply(
    clock: Res<DayClock>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform)>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut haze: Query<&mut DistanceFog>,
    glow: Res<WindowGlow>,
    weather: Res<crate::weather::LocalWeather>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut glowing: Local<Option<f32>>,
) {
    let (light, mut lux, ambient_color, mut brightness, mut horizon) = sky_at(clock.t);
    // An overcast sky where the camera looks: a weaker sun (so shadows soften
    // against the sky light) and a greyer horizon. Cloud shadows themselves
    // are in the shaders.
    let overcast = weather.cover;
    lux *= 1.0 - 0.6 * overcast;
    brightness *= 1.0 + 0.3 * overcast;
    let grey = horizon.luminance();
    horizon = horizon.mix(&Color::srgb(grey, grey, grey), 0.5 * overcast);
    // A storm darkens it all to slate, except in a blizzard, where the
    // driven snow turns the haze white.
    let (storm, blizzard) = (weather.storm, weather.blizzard);
    lux *= 1.0 - 0.65 * storm;
    brightness *= 1.0 - 0.35 * (storm - blizzard);
    horizon = horizon.mix(&Color::srgb(grey * 0.55, grey * 0.6, grey * 0.68), 0.7 * (storm - blizzard));
    let white = (grey * 1.25 + 0.08).min(1.0);
    horizon = horizon.mix(&Color::srgb(white * 0.94, white * 0.97, white), 0.85 * blizzard);
    // Lightning: a cold flash of light from the whole sky.
    let mut ambient_color = ambient_color;
    if weather.flash > 0.0 {
        let sky = ambient_color.to_linear().to_vec3() * brightness + Vec3::new(2200.0, 2400.0, 3000.0) * weather.flash;
        brightness = sky.max_element();
        ambient_color = Color::linear_rgb(sky.x / brightness, sky.y / brightness, sky.z / brightness);
        horizon = horizon.mix(&Color::srgb(0.8, 0.84, 0.95), 0.4 * weather.flash.min(1.0));
    }
    for (mut l, mut transform) in &mut sun {
        l.color = light;
        l.illuminance = lux;
        *transform = light_transform(clock.t);
    }
    ambient.color = ambient_color;
    ambient.brightness = brightness;
    clear.0 = horizon;
    for mut fog in &mut haze {
        fog.color = horizon;
        // The haze glows towards the sun, less so towards the moon.
        fog.directional_light_color = light.with_alpha(0.4 * (1.0 - moonlight(clock.t) * 0.6));
    }
    // Changing the material re-uploads it, so only when the glow changes.
    let n = night(clock.t);
    if *glowing != Some(n) {
        *glowing = Some(n);
        if let Some(mut m) = materials.get_mut(&glow.0) {
            // Dark glass by day; warm lamplight, bright enough to bloom, at night.
            m.emissive = LinearRgba::rgb(30.0, 11.0, 2.5) * n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_stays_above_the_horizon() {
        for i in 0..1000 {
            let t = i as f32 / 1000.0;
            let d = light_direction(t);
            assert!(d.y > (14.0f32).to_radians().sin(), "t = {t}: {d}");
        }
    }

    #[test]
    fn noon_matches_the_shaders() {
        // The fog clouds' colours were made for this light (FOG_NOON_SUN and
        // FOG_NOON_SKY in the shaders).
        let (light, lux, ambient, brightness, _) = sky_at(ALWAYS_DAY);
        let sun = light.to_linear().to_vec3() * lux;
        let sky = ambient.to_linear().to_vec3() * brightness;
        assert!((sun - Vec3::new(11000.0, 9790.0, 7817.0)).abs().max_element() < 10.0, "{sun}");
        assert!((sky - Vec3::new(470.3, 622.9, 900.0)).abs().max_element() < 2.0, "{sky}");
        let d = light_direction(ALWAYS_DAY);
        let along = Vec2::new(d.x, d.z).normalize();
        assert!(along.distance(Vec2::new(-0.645, 0.765)) < 0.05, "{along}");
    }

    #[test]
    fn clock_has_noon_at_the_sun_s_peak() {
        assert_eq!(clock(DUSK_END / 2.0), (12, 0));
        assert_eq!(clock(0.0), (4, 0));
        assert_eq!(clock(DUSK_END), (20, 0));
        assert_eq!(clock(0.9999).0, 3);
    }

    #[test]
    fn sky_wraps_smoothly() {
        let (a, ..) = sky_at(0.9999);
        let (b, ..) = sky_at(0.0);
        assert!((a.to_linear().to_vec3() - b.to_linear().to_vec3()).abs().max_element() < 0.01);
        assert!((light_direction(0.9999) - light_direction(0.0)).length() < 0.01);
    }
}
