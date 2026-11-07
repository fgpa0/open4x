//! Player settings: graphics, display and frame rate. Stored as RON in the user's
//! config directory and applied live whenever they change.

use crate::camera::{CameraRig, MainCamera};
use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::pbr::{DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, VideoModeSelection, WindowMode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    Windowed,
    Borderless,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowQuality {
    Off,
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AntiAliasing {
    Off,
    Fxaa,
    Taa,
}

/// Which news pops up on screen (everything goes into the notification log).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopUps {
    All,
    /// Only bad news: attacks, losses, wars declared on us.
    Alerts,
    Off,
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Frames per second limit; 0 means unlimited.
    pub fps_cap: u32,
    pub vsync: bool,
    pub display_mode: DisplayMode,
    pub shadows: ShadowQuality,
    pub ambient_occlusion: bool,
    pub anti_aliasing: AntiAliasing,
    pub bloom: bool,
    pub fog: bool,
    pub ui_scale: f32,
    /// Autosave every this many turns (0: never).
    pub autosave_every: u32,
    pub popups: PopUps,
    /// The yields overlay (Y) on the map.
    pub show_yields: bool,
    /// Minutes per day and night (0: always day).
    pub day_length: u32,
    /// Clouds, rain and snow.
    pub weather: bool,
    /// Birds, animals, smoke, mist and the like (see `ambience`).
    pub ambience: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fps_cap: 0,
            vsync: true,
            display_mode: DisplayMode::Windowed,
            shadows: ShadowQuality::Medium,
            ambient_occlusion: true,
            anti_aliasing: AntiAliasing::Taa,
            bloom: true,
            fog: true,
            ui_scale: 1.0,
            autosave_every: 1,
            popups: PopUps::All,
            show_yields: true,
            day_length: 16,
            weather: true,
            ambience: true,
        }
    }
}

pub const FPS_CAPS: [u32; 9] = [0, 30, 60, 75, 90, 120, 144, 165, 240];
pub const UI_SCALES: [f32; 6] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5];
pub const AUTOSAVE_EVERY: [u32; 6] = [0, 1, 2, 5, 10, 20];
pub const DAY_LENGTHS: [u32; 5] = [0, 8, 16, 24, 40];

/// The game's folder for one kind of file: `$<xdg_var>/open4x` if that is set
/// (so tests can point it elsewhere), else `%APPDATA%\open4x` on Windows,
/// `~/Library/Application Support/open4x` on macOS and `~/<xdg_default>/open4x`
/// on other systems. A folder left by the game's old name, `fourx`, is moved
/// there the first time.
pub fn app_dir(xdg_var: &str, xdg_default: &str) -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if let Some(dir) = var(xdg_var) {
        dir
    } else if cfg!(windows) {
        var("APPDATA")?
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Application Support")
    } else {
        var("HOME")?.join(xdg_default)
    };
    let dir = base.join("open4x");
    let old = base.join("fourx");
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &dir);
    }
    Some(dir)
}

fn config_path() -> Option<PathBuf> {
    Some(app_dir("XDG_CONFIG_HOME", ".config")?.join("settings.ron"))
}

impl Settings {
    pub fn load() -> Self {
        let Some(path) = config_path() else { return Self::default() };
        match std::fs::read_to_string(&path) {
            Ok(text) => ron::from_str(&text).unwrap_or_else(|e| {
                warn!("ignoring invalid settings file {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    fn save(&self) {
        let Some(path) = config_path() else { return };
        let text = match ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) {
            Ok(t) => t,
            Err(e) => return warn!("could not serialise settings: {e}"),
        };
        if let Err(e) = path.parent().map(std::fs::create_dir_all).unwrap_or(Ok(())).and_then(|_| std::fs::write(&path, text)) {
            warn!("could not save settings to {}: {e}", path.display());
        }
    }
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Settings::load())
            .init_resource::<FrameLimiter>()
            .add_systems(PostStartup, apply_settings)
            .add_systems(Update, (apply_settings.run_if(resource_changed::<Settings>), fit_shadow_cascades, follow_zoom_with_fog))
            .add_systems(Last, limit_frame_rate);
    }
}

fn apply_settings(
    mut commands: Commands,
    settings: Res<Settings>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    camera: Single<Entity, With<MainCamera>>,
    mut sun: Query<&mut DirectionalLight>,
    mut ui_scale: ResMut<UiScale>,
    mut saved: Local<Option<Settings>>,
) {
    let s = &*settings;
    window.present_mode = if s.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
    window.mode = match s.display_mode {
        DisplayMode::Windowed => WindowMode::Windowed,
        DisplayMode::Borderless => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
        DisplayMode::Fullscreen => WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current),
    };

    for mut light in &mut sun {
        light.shadow_maps_enabled = s.shadows != ShadowQuality::Off;
    }
    let size = match s.shadows {
        ShadowQuality::Off | ShadowQuality::Low => 1024,
        ShadowQuality::Medium => 2048,
        ShadowQuality::High => 4096,
    };
    commands.insert_resource(DirectionalLightShadowMap { size });

    let mut cam = commands.entity(*camera);
    if s.ambient_occlusion {
        // Medium quality: nearly identical from a strategy camera, much cheaper
        // at high resolutions.
        cam.insert(ScreenSpaceAmbientOcclusion {
            quality_level: bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::Medium,
            ..default()
        });
    } else {
        cam.remove::<ScreenSpaceAmbientOcclusion>();
    }
    match s.anti_aliasing {
        AntiAliasing::Off => {
            cam.remove::<(TemporalAntiAliasing, Fxaa)>();
        }
        AntiAliasing::Fxaa => {
            cam.remove::<TemporalAntiAliasing>().insert(Fxaa::default());
        }
        AntiAliasing::Taa => {
            cam.remove::<Fxaa>().insert(TemporalAntiAliasing::default());
        }
    }
    if s.bloom {
        cam.insert(Bloom::NATURAL);
    } else {
        cam.remove::<Bloom>();
    }
    if s.fog {
        cam.insert(fog());
    } else {
        cam.remove::<DistanceFog>();
    }
    ui_scale.0 = s.ui_scale;

    // Persist only real changes, not the initial application at startup.
    if saved.as_ref().is_some_and(|prev| prev != s) {
        s.save();
    }
    *saved = Some(s.clone());
}

/// Fits the sun's shadow cascades to what the camera can see. Shadow rendering is
/// the most expensive pass, so covering only the visible area (instead of a fixed
/// long range) saves most of its cost and sharpens shadows when zoomed in.
fn fit_shadow_cascades(
    rig: Res<CameraRig>,
    settings: Res<Settings>,
    mut sun: Query<&mut CascadeShadowConfig>,
    mut applied: Local<Option<(i32, ShadowQuality)>>,
) {
    // Quantise the zoom so cascades only change in steps (no shimmering).
    let step = (rig.distance.ln() * 8.0).round() as i32;
    if *applied == Some((step, settings.shadows)) {
        return;
    }
    *applied = Some((step, settings.shadows));
    let d = (step as f32 / 8.0).exp();
    // The steep camera sees little depth: one cascade looks the same as two
    // from the usual distance, at half the cost of drawing the shadow casters.
    let num_cascades = match settings.shadows {
        ShadowQuality::Off | ShadowQuality::Low | ShadowQuality::Medium => 1,
        ShadowQuality::High => 2,
    };
    let config: CascadeShadowConfig = CascadeShadowConfigBuilder {
        num_cascades,
        minimum_distance: 0.1,
        first_cascade_far_bound: d * 1.15 + 4.0,
        maximum_distance: d * 1.9 + 8.0,
        overlap_proportion: 0.2,
    }
    .build();
    for mut c in &mut sun {
        *c = config.clone();
    }
}

pub fn fog() -> DistanceFog {
    DistanceFog {
        color: Color::srgba(0.62, 0.74, 0.86, 1.0),
        directional_light_color: Color::srgba(1.0, 0.92, 0.75, 0.4),
        directional_light_exponent: 20.0,
        falloff: fog_falloff(22.0, 0.0),
    }
}

/// Distance haze for a camera `distance` from the ground: it only softens the
/// far edge of the view (towards the horizon), so zooming out doesn't wash out
/// the land under the camera. In `murk` (a storm, 0..1) it closes in on the
/// land itself, though the focus always stays visible.
fn fog_falloff(distance: f32, murk: f32) -> FogFalloff {
    let lerp = |a: f32, b: f32| a + (b - a) * murk;
    FogFalloff::Linear {
        start: lerp(distance * 1.3 + 30.0, distance * 0.75),
        end: lerp(distance * 2.5 + 140.0, distance * 3.2 + 10.0),
    }
}

fn follow_zoom_with_fog(
    rig: Res<CameraRig>,
    weather: Res<crate::weather::LocalWeather>,
    mut fog: Query<&mut DistanceFog, With<MainCamera>>,
) {
    // Blizzards white out the view; rainstorms only dim the distance.
    let murk = weather.blizzard + 0.3 * (weather.storm - weather.blizzard);
    for mut f in &mut fog {
        f.falloff = fog_falloff(rig.current_distance, murk);
    }
}

#[derive(Resource)]
struct FrameLimiter {
    last: Instant,
}

impl Default for FrameLimiter {
    fn default() -> Self {
        Self { last: Instant::now() }
    }
}

/// Sleeps at the end of each frame to hold the configured frame rate.
fn limit_frame_rate(settings: Res<Settings>, mut limiter: ResMut<FrameLimiter>) {
    if settings.fps_cap > 0 {
        let target = Duration::from_secs_f64(1.0 / settings.fps_cap as f64);
        let deadline = limiter.last + target;
        // Sleep most of the way, then spin for precision.
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            if remaining > Duration::from_millis(2) {
                std::thread::sleep(remaining - Duration::from_millis(1));
            }
            while Instant::now() < deadline {
                std::hint::spin_loop();
            }
        }
    }
    limiter.last = Instant::now();
}
