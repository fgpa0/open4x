//! F3 debug overlay: performance, hardware usage and game state at a glance.

use crate::camera::CameraRig;
use crate::loading::InGameSet;
use crate::overlay::HoveredTile;
use crate::settings::Settings;
use crate::terrain::Terrain;
use crate::vegetation::TreeCount;
use crate::GameMap;
use bevy::diagnostic::{
    DiagnosticPath, DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin, SystemInfo,
    SystemInformationDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::PrimaryWindow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Component)]
struct DebugOverlay;

#[derive(Component)]
struct DebugText;

/// Latest GPU statistics, filled in by a background thread.
#[derive(Clone, Default)]
struct GpuStats {
    available: Option<String>,
    utilization: u32,
    vram_used: u64,
    vram_total: u64,
    temperature: u32,
    power_watts: f32,
}

#[derive(Resource, Clone)]
struct GpuMonitor(Arc<Mutex<GpuStats>>);

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            SystemInformationDiagnosticsPlugin,
            EntityCountDiagnosticsPlugin::default(),
            RenderDiagnosticsPlugin,
        ))
        .insert_resource(start_gpu_monitor())
        .add_systems(Startup, spawn_overlay)
        .add_systems(Update, (toggle_overlay, update_overlay.in_set(InGameSet)));
    }
}

/// GPU time of each render pass in ms, most expensive first.
pub fn gpu_pass_times(diagnostics: &DiagnosticsStore) -> Vec<(String, f64)> {
    let mut passes: Vec<(String, f64)> = diagnostics
        .iter()
        .filter_map(|d| {
            let path = d.path().as_str();
            let name = path.strip_prefix("render/")?.strip_suffix("/elapsed_gpu")?;
            Some((name.to_string(), d.smoothed()?))
        })
        .collect();
    passes.sort_by(|a, b| b.1.total_cmp(&a.1));
    passes
}

/// Polls NVML (NVIDIA's management library) once a second on its own thread so
/// the game never waits on it. Other vendors show "n/a".
fn start_gpu_monitor() -> GpuMonitor {
    let stats = Arc::new(Mutex::new(GpuStats::default()));
    let shared = stats.clone();
    std::thread::Builder::new()
        .name("gpu-monitor".into())
        .spawn(move || {
            let nvml = match nvml_wrapper::Nvml::init() {
                Ok(n) => n,
                Err(e) => {
                    shared.lock().unwrap().available = Some(format!("n/a ({e})"));
                    return;
                }
            };
            let Ok(device) = nvml.device_by_index(0) else {
                shared.lock().unwrap().available = Some("n/a (no device)".into());
                return;
            };
            loop {
                let mut s = GpuStats::default();
                if let Ok(u) = device.utilization_rates() {
                    s.utilization = u.gpu;
                }
                if let Ok(m) = device.memory_info() {
                    s.vram_used = m.used;
                    s.vram_total = m.total;
                }
                s.temperature = device
                    .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                    .unwrap_or(0);
                s.power_watts = device.power_usage().map(|mw| mw as f32 / 1000.0).unwrap_or(0.0);
                *shared.lock().unwrap() = s;
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("spawn gpu monitor thread");
    GpuMonitor(stats)
}

fn spawn_overlay(mut commands: Commands) {
    commands
        .spawn((
            DebugOverlay,
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::topbar::HEIGHT + 120.0),
                left: px(12),
                padding: UiRect::axes(px(12), px(8)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.62)),
            GlobalZIndex(5),
            // FOURX_DEBUG=1 opens the overlay at startup.
            if std::env::var("FOURX_DEBUG").is_ok_and(|v| !v.is_empty()) {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        ))
        .with_child((
            DebugText,
            Text::new(""),
            crate::ui::mono_font(14.0),
            TextColor(Color::srgb(0.85, 0.95, 0.85)),
        ));
}

fn toggle_overlay(keys: Res<ButtonInput<KeyCode>>, mut overlay: Single<&mut Visibility, With<DebugOverlay>>) {
    if keys.just_pressed(KeyCode::F3) {
        overlay.toggle_visible_hidden();
    }
}

fn update_overlay(
    time: Res<Time>,
    mut timer: Local<f32>,
    overlay: Single<&Visibility, With<DebugOverlay>>,
    mut text: Single<&mut Text, With<DebugText>>,
    diagnostics: Res<DiagnosticsStore>,
    gpu: Res<GpuMonitor>,
    adapter: Option<Res<RenderAdapterInfo>>,
    system: Option<Res<SystemInfo>>,
    game: Res<GameMap>,
    terrain: Res<Terrain>,
    trees: Option<Res<TreeCount>>,
    rig: Res<CameraRig>,
    hovered: Res<HoveredTile>,
    settings: Res<Settings>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    *timer -= time.delta_secs();
    if **overlay == Visibility::Hidden || *timer > 0.0 {
        return;
    }
    *timer = 0.25;

    let diag = |path: &DiagnosticPath| -> Option<f64> { diagnostics.get(path).and_then(|d| d.smoothed()) };
    let fps = diag(&FrameTimeDiagnosticsPlugin::FPS).unwrap_or(0.0);
    let frame_ms = diag(&FrameTimeDiagnosticsPlugin::FRAME_TIME).unwrap_or(0.0);
    let proc_cpu = diag(&SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE);
    let sys_cpu = diag(&SystemInformationDiagnosticsPlugin::SYSTEM_CPU_USAGE);
    let proc_mem = diag(&SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE);
    let sys_mem = diag(&SystemInformationDiagnosticsPlugin::SYSTEM_MEM_USAGE);
    let entities = diag(&EntityCountDiagnosticsPlugin::ENTITY_COUNT).unwrap_or(0.0);
    let pct = |v: Option<f64>| v.map_or("-".to_string(), |v| format!("{v:.1}%"));

    let mut lines = vec![
        format!("FPS {fps:.0}  ({frame_ms:.2} ms)"),
        format!(
            "CPU  process {}  system {}{}",
            pct(proc_cpu),
            pct(sys_cpu),
            system.as_ref().map_or(String::new(), |s| format!("  ({}, {} cores)", s.cpu.trim(), s.core_count))
        ),
        format!(
            "RAM  process {}  system {}{}",
            proc_mem.map_or("-".into(), |g| format!("{:.0} MB", g * 1024.0)),
            pct(sys_mem),
            system.as_ref().map_or(String::new(), |s| format!(" of {}", s.memory))
        ),
    ];
    if let Some(a) = &adapter {
        lines.push(format!("GPU  {}  ({:?}, driver {})", a.name, a.backend, a.driver_info));
    }
    let g = gpu.0.lock().unwrap().clone();
    lines.push(match g.available {
        Some(reason) => format!("GPU  usage {reason}"),
        None => format!(
            "GPU  usage {}%  VRAM {:.1}/{:.1} GB  {} C  {:.0} W",
            g.utilization,
            g.vram_used as f64 / 1e9,
            g.vram_total as f64 / 1e9,
            g.temperature,
            g.power_watts
        ),
    });
    let res = window.resolution.physical_size();
    lines.push(format!(
        "Display {}x{}  {:?}  cap {}",
        res.x,
        res.y,
        window.present_mode,
        if settings.fps_cap == 0 { "none".to_string() } else { settings.fps_cap.to_string() }
    ));
    let passes = gpu_pass_times(&diagnostics);
    if !passes.is_empty() {
        // Nested spans repeat their parents' time, so only list leaf-ish passes.
        let total: f64 = passes.iter().filter(|(n, _)| !n.contains('/')).map(|(_, t)| t).sum();
        lines.push(format!("GPU frame {total:.2} ms; top passes:"));
        for (name, ms) in passes.iter().take(8) {
            lines.push(format!("  {ms:6.2} ms  {name}"));
        }
    }
    lines.push(String::new());
    let grid = &game.map.grid;
    lines.push(format!("Seed {}  Map {}x{} ({} tiles)", game.map.seed, grid.width, grid.height, grid.len()));
    lines.push(format!(
        "Entities {:.0}  Trees {}  Terrain gen {:.0} ms",
        entities,
        trees.map_or(0, |t| t.0),
        terrain.build_ms
    ));
    lines.push(format!("Camera ({:.1}, {:.1})  distance {:.1}", rig.focus.x, rig.focus.y, rig.distance));
    lines.push(match hovered.0 {
        Some(id) => {
            let (c, r) = grid.offset(id);
            let t = game.map.tile(id);
            format!("Tile #{id} ({c}, {r})  height {:.2}  rivers {:06b}", t.height, t.rivers)
        }
        None => "Tile -".into(),
    });
    lines.push("F3 to hide".into());
    text.0 = lines.join("\n");
}
