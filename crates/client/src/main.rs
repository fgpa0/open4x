//! Game client: renders the simulation and turns player input into commands.
// Release builds on Windows open no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ambience;
mod birds;
mod bridges;
mod camera;
mod capture;
mod cities;
mod city_panel;
mod land;
mod combat_fx;
mod daylight;
mod debug;
mod diplomacy;
mod fog;
mod frontend;
mod gameover;
mod hud;
mod icons;
mod improvements;
mod image_util;
mod layout;
mod loading;
mod lod;
mod map_preview;
mod menu;
mod merchants;
mod minimap;
mod models;
mod news;
mod overlay;
mod placement;
mod play;
mod procmesh;
mod research_ui;
mod rigs;
mod saves;
mod season;
mod settings;
mod site_hints;
mod strategic;
mod terrain;
mod terraform;
mod topbar;
mod trade;
mod ui;
mod unit_panel;
mod forces;
mod units;
mod vegetation;
mod water;
mod weather;
mod wrap;
mod wrapnoise;
mod yields;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use loading::{AppState, InGameSet};
use sim::{map::Map, rules::Rules};

/// `embedded_asset!` paths are relative to the calling file, so shaders under
/// `src/shaders` are registered from here.
pub fn embed_terrain_shader(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/terrain.wesl");
}

pub fn embed_puff_shader(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/puffs.wesl");
}

pub fn embed_vegetation_shader(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/vegetation.wesl");
}

/// The generated world and its rules, shared by all rendering systems.
#[derive(Resource)]
pub struct GameMap {
    pub map: Map,
    pub rules: Rules,
}

/// The game's name, shown in the window title and on the main menu.
pub const GAME_NAME: &str = "Open4X";

fn main() {
    let rules = Rules::load_default().unwrap_or_else(|e| {
        eprintln!("failed to load rules: {e}");
        std::process::exit(1);
    });
    // FOURX_MENU=main|civs|setup|maptypes shows a front-end screen. Otherwise dev runs
    // (screenshots, FOURX_PLAY) skip the front end and start a game right away.
    let menu = std::env::var("FOURX_MENU").unwrap_or_default();
    let initial = match menu.as_str() {
        "main" => AppState::MainMenu,
        "civs" => AppState::CivSelect,
        "setup" | "maptypes" => AppState::Setup,
        _ if std::env::var("FOURX_SCREENSHOT").is_ok() || std::env::var("FOURX_PLAY").is_ok() => AppState::Loading,
        _ => AppState::MainMenu,
    };

    let mut app = App::new();
    // A command on an entity that's gone (a stale visual) logs a warning
    // rather than ending the game.
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: GAME_NAME.into(),
            // FOURX_RES=WxH overrides the window size (handy for performance tests).
            resolution: std::env::var("FOURX_RES")
                .ok()
                .and_then(|r| {
                    let (w, h) = r.split_once('x')?;
                    Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
                })
                .unwrap_or((1600, 900))
                .into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(frontend::RulesRes(rules))
    .insert_state(initial)
    .add_plugins(loading::LoadingPlugin)
    .add_plugins((
        terrain::TerrainPlugin,
        water::WaterPlugin,
        vegetation::VegetationPlugin,
        camera::CameraPlugin,
        overlay::OverlayPlugin,
        wrap::WrapPlugin,
        hud::HudPlugin,
        icons::IconsPlugin,
    ))
    .add_plugins((
        settings::SettingsPlugin,
        lod::LodPlugin,
        play::PlayPlugin,
        fog::FogPlugin,
        cities::CitiesPlugin,
        city_panel::CityPanelPlugin,
        units::UnitsPlugin,
        unit_panel::UnitPanelPlugin,
        topbar::TopBarPlugin,
        research_ui::ResearchUiPlugin,
        combat_fx::CombatFxPlugin,
        ui::UiStylePlugin,
        menu::MenuPlugin,
        debug::DebugPlugin,
        frontend::FrontendPlugin,
    ))
    .add_plugins((daylight::DaylightPlugin, season::SeasonPlugin, weather::WeatherPlugin, saves::SavesPlugin, diplomacy::DiplomacyPlugin, capture::CapturePlugin, gameover::GameOverPlugin, trade::TradePlugin, land::LandPlugin))
    .add_plugins((improvements::ImprovementsPlugin, strategic::StrategicPlugin, yields::YieldsPlugin, news::NewsPlugin, minimap::MinimapPlugin, site_hints::SiteHintsPlugin))
    .add_plugins((merchants::MerchantsPlugin, terraform::TerraformPlugin, placement::PlacementPlugin, bridges::BridgesPlugin, birds::BirdsPlugin, ambience::AmbiencePlugin, forces::ForcesPlugin))
;
    if initial == AppState::Loading {
        // Straight into a game: FOURX_SEED (default 7) and FOURX_CIV pick it.
        let seed = std::env::var("FOURX_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(7);
        let rules = &app.world().resource::<frontend::RulesRes>().0;
        // FOURX_LOAD=path opens a save instead (e.g. a late game for profiling).
        let spec = match std::env::var("FOURX_LOAD") {
            Ok(path) => saves::load(std::path::Path::new(&path), rules).unwrap_or_else(|e| {
                eprintln!("failed to load {path}: {e}");
                std::process::exit(1);
            }),
            Err(_) => frontend::NewGame::direct(rules, seed).spec(rules),
        };
        app.insert_resource(spec);
    }

    if let Ok(spec) = std::env::var("FOURX_SCREENSHOT") {
        let mode = ScreenshotMode::parse(&spec);
        let clicks_into_game = std::env::var("FOURX_CLICKS").is_ok_and(|c| c.contains("game"));
        if mode.loading {
            app.add_systems(Update, screenshot_early.run_if(in_state(AppState::Loading)));
        } else if initial != AppState::Loading && !clicks_into_game {
            app.add_systems(Update, screenshot_early.run_if(not(in_state(AppState::InGame))));
        } else {
            app.add_systems(Update, screenshot_mode.in_set(InGameSet));
        }
        app.insert_resource(mode);
        // Run at full speed even without focus, so the logged FPS is real.
        app.insert_resource(bevy::winit::WinitSettings::continuous());
    }
    if let Ok(spec) = std::env::var("FOURX_CLICKS") {
        app.insert_resource(DevClicks::parse(&spec)).add_systems(Update, dev_clicks);
    }
    app.run();
}

/// Debug aid: `FOURX_CLICKS=x,y,t;x,y,t...` left-clicks at window position
/// (x, y) `t` seconds after startup, to test the front end in screenshot
/// runs. Ending the list with `;game` takes the screenshot in game instead.
/// `x,y,t,down`, `x,y,t,move` and `x,y,t,up` press, move and release
/// separately, for drags.
#[derive(Resource)]
struct DevClicks {
    clicks: Vec<(Vec2, f32, ClickStep)>,
    /// The click whose button is down (released next frame).
    pressed: bool,
    /// When the last click happens.
    end: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum ClickStep {
    Click,
    Down,
    Move,
    Up,
}

impl DevClicks {
    fn parse(spec: &str) -> Self {
        let clicks: Vec<(Vec2, f32, ClickStep)> = spec
            .split(';')
            .filter_map(|c| {
                let v: Vec<f32> = c.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                let step = match c.rsplit(',').next().map(str::trim) {
                    Some("down") => ClickStep::Down,
                    Some("move") => ClickStep::Move,
                    Some("up") => ClickStep::Up,
                    _ => ClickStep::Click,
                };
                (v.len() == 3).then(|| (Vec2::new(v[0], v[1]), v[2], step))
            })
            .collect();
        let end = clicks.iter().map(|c| c.1).fold(0.0, f32::max);
        Self { clicks, pressed: false, end }
    }
}

fn dev_clicks(
    time: Res<Time>,
    mut clicks: ResMut<DevClicks>,
    mut window: Single<(Entity, &mut Window)>,
    mut out: MessageWriter<bevy::window::WindowEvent>,
) {
    use bevy::input::{mouse::MouseButtonInput, ButtonState};
    use bevy::window::{CursorMoved, WindowEvent};
    let entity = window.0;
    let button = |state| WindowEvent::MouseButtonInput(MouseButtonInput { button: MouseButton::Left, state, window: entity });
    if clicks.pressed {
        clicks.pressed = false;
        out.write(button(ButtonState::Released));
        clicks.clicks.remove(0);
        return;
    }
    let Some(&(at, when, step)) = clicks.clicks.first() else { return };
    if time.elapsed_secs() < when {
        return;
    }
    window.1.set_cursor_position(Some(at));
    out.write(WindowEvent::CursorMoved(CursorMoved { window: entity, position: at, delta: None }));
    match step {
        ClickStep::Click => {
            out.write(button(ButtonState::Pressed));
            clicks.pressed = true;
        }
        ClickStep::Down => {
            out.write(button(ButtonState::Pressed));
            clicks.clicks.remove(0);
        }
        ClickStep::Up => {
            out.write(button(ButtonState::Released));
            clicks.clicks.remove(0);
        }
        ClickStep::Move => {
            clicks.clicks.remove(0);
        }
    }
}

/// Debug aid: `FOURX_SCREENSHOT=out.png[,x,z,distance]` renders a few frames,
/// saves a screenshot and exits. `FOURX_SCREENSHOT=out.png,loading` captures the
/// loading screen instead.
#[derive(Resource)]
struct ScreenshotMode {
    path: String,
    view: Option<(Vec2, f32)>,
    loading: bool,
    /// Zoom out but keep the camera where the game put it (`far`, or
    /// `far,<distance>`).
    far: Option<f32>,
}

impl ScreenshotMode {
    fn parse(spec: &str) -> Self {
        let parts: Vec<&str> = spec.split(',').collect();
        let nums: Vec<f32> = parts[1..].iter().filter_map(|s| s.parse().ok()).collect();
        let view = (nums.len() == 3).then(|| (Vec2::new(nums[0], nums[1]), nums[2]));
        let far = parts.contains(&"far").then(|| nums.first().copied().unwrap_or(55.0));
        Self { path: parts[0].to_string(), view, loading: parts.contains(&"loading"), far }
    }
}

/// Screenshots of the loading screen and the front end (after the backdrop
/// has been painted).
fn screenshot_early(
    mut commands: Commands,
    mode: Res<ScreenshotMode>,
    time: Res<Time>,
    mut requested: Local<bool>,
    mut exit: MessageWriter<AppExit>,
    clicks: Option<Res<DevClicks>>,
) {
    let after_clicks = clicks.map_or(0.0, |c| c.end + 2.0);
    let wait = if mode.loading { 1.5 } else { 4.0f32.max(after_clicks) };
    if time.elapsed_secs() > wait && !*requested {
        *requested = true;
        let _ = std::fs::remove_file(&mode.path);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(mode.path.clone()));
    }
    if *requested && std::path::Path::new(&mode.path).exists() {
        exit.write(AppExit::Success);
    }
}

fn screenshot_mode(
    mut commands: Commands,
    mode: Res<ScreenshotMode>,
    mut rig: ResMut<camera::CameraRig>,
    time: Res<Time>,
    mut requested: Local<bool>,
    mut started: Local<Option<f32>>,
    entered: Res<State<AppState>>,
    clicks: Option<Res<DevClicks>>,
    mut exit: MessageWriter<AppExit>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mut window: Single<(Entity, &mut Window)>,
    mut moved: MessageWriter<bevy::window::WindowEvent>,
    duel: Option<Res<play::DevCursorTile>>,
    game: Res<GameMap>,
    terrain: Res<terrain::Terrain>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
) {
    // FOURX_CURSOR=x,y (or `duel`) parks a virtual mouse cursor there (to capture hovers).
    let duel_spot = duel.and_then(|d| {
        let p = layout::tile_center(&game.map.grid, d.0);
        let world = Vec3::new(p.x, terrain.heights.sample(p).max(0.0), p.y);
        camera.0.world_to_viewport(camera.1, world).ok().map(|s| (s.x, s.y))
    });
    let cursor = std::env::var("FOURX_CURSOR").ok().and_then(|v| {
        if v == "duel" {
            return duel_spot;
        }
        let (x, y) = v.split_once(',')?;
        Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?))
    });
    if let Some((x, y)) = cursor {
        let (entity, window) = &mut *window;
        window.set_cursor_position(Some(Vec2::new(x, y)));
        moved.write(bevy::window::WindowEvent::CursorMoved(bevy::window::CursorMoved {
            window: *entity,
            position: Vec2::new(x, y),
            delta: None,
        }));
    }
    if let Some((focus, distance)) = mode.view {
        camera::snap(&mut rig, focus, distance);
    } else if let Some(distance) = mode.far {
        let focus = rig.focus;
        camera::snap(&mut rig, focus, distance);
    }
    // Shader pipelines compile in the background; give them and temporal effects
    // time after the world has loaded.
    // Timed from the latest entry into a game (FOURX_CLICKS may load another).
    if entered.is_changed() || clicks.as_ref().is_some_and(|c| !c.clicks.is_empty()) {
        *started = None;
    }
    let t = time.elapsed_secs() - *started.get_or_insert(time.elapsed_secs());
    // FOURX_SHOT_AT=seconds takes it earlier or later (e.g. during the intro).
    let at = std::env::var("FOURX_SHOT_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(8.0);
    if t > at && !*requested {
        *requested = true;
        let _ = std::fs::remove_file(&mode.path);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(mode.path.clone()));
    }
    if *requested && (std::path::Path::new(&mode.path).exists() || t > 30.0) {
        // Report GPU cost per pass alongside the screenshot.
        let fps = diagnostics
            .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
            .and_then(|d| d.smoothed())
            .unwrap_or(0.0);
        info!("perf: {fps:.0} fps");
        for (name, ms) in debug::gpu_pass_times(&diagnostics).iter().take(14) {
            info!("perf: {ms:7.3} ms  {name}");
        }
        exit.write(AppExit::Success);
    }
}
