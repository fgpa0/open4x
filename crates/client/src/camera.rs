//! Strategy camera: fixed angle looking north, pans across the map and zooms.

use crate::layout::map_bounds;
use crate::loading::{AppState, InGameSet};
use crate::menu::Menu;
use crate::GameMap;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, Hdr};
use bevy::render::render_resource::BlendState;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;

/// Camera pitch below the horizon, in radians.
const PITCH: f32 = 0.95;
const MIN_DISTANCE: f32 = 9.0;
const MAX_DISTANCE: f32 = 90.0;

#[derive(Resource)]
pub struct CameraRig {
    /// Point on the ground the camera looks at.
    pub focus: Vec2,
    pub distance: f32,
    /// Smoothed values actually used for the transform.
    current_focus: Vec2,
    pub current_distance: f32,
}

/// The 3D camera that views the map. Post-processing on it is managed by settings.
#[derive(Component)]
pub struct MainCamera;

/// A second camera, riding along with the main one, that draws map icons
/// (the yields overlay) over the finished image without tonemapping, haze or
/// bloom, so they keep the exact colours of the interface's icons. It also
/// draws the interface, which must come after the icons.
#[derive(Component)]
pub struct IconCamera;

/// The render layer only the icon camera sees.
pub const ICON_LAYER: usize = 2;

/// Click-and-drag panning: the ground point grabbed under the cursor stays
/// under it while dragging.
#[derive(Resource, Default)]
struct Drag {
    /// Cursor position where the button went down.
    press: Option<Vec2>,
    /// Grabbed ground point (x, height, z), once the drag has started.
    grab: Option<Vec3>,
}

/// Pixels the cursor must move before a press becomes a drag, so plain clicks
/// stay free for selecting things.
const DRAG_THRESHOLD: f32 = 5.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drag>()
            .add_systems(Startup, (spawn_camera, cpu_light_clustering))
            .add_systems(OnEnter(AppState::InGame), (center_on_map, start_intro.after(crate::play::start_game)))
            .add_systems(
                Update,
                ((drag_pan, camera_input).chain().run_if(in_state(Menu::Closed)), run_intro, apply_rig)
                    .chain()
                    .in_set(InGameSet),
            );
    }
}

/// The camera exists from startup (the loading screen renders through it); it is
/// pointed at the world once that has been built.
fn spawn_camera(mut commands: Commands) {
    let focus = Vec2::ZERO;
    let distance = 22.0;
    commands.insert_resource(CameraRig {
        focus,
        distance,
        current_focus: focus,
        current_distance: distance,
    });
    commands.spawn((
        MainCamera,
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            fov: 30f32.to_radians(),
            ..default()
        }),
        Hdr,
        Tonemapping::AgX,
        // SSAO and TAA need MSAA off; anti-aliasing comes from TAA/FXAA instead.
        Msaa::Off,
        // No point lights to sort into clusters (see `cpu_light_clustering`).
        bevy::light::cluster::ClusterConfig::Single,
        rig_transform(focus, distance),
        children![(
            IconCamera,
            // The interface is drawn by this camera, so it stays above the icons.
            IsDefaultUiCamera,
            Camera3d::default(),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                // Blended over the finished scene rather than replacing it.
                output_mode: CameraOutputMode::Write { blend_state: Some(BlendState::ALPHA_BLENDING), clear_color: ClearColorConfig::None },
                ..default()
            },
            Projection::from(PerspectiveProjection { fov: 30f32.to_radians(), ..default() }),
            Tonemapping::None,
            Msaa::Off,
            bevy::light::cluster::ClusterConfig::Single,
            RenderLayers::layer(ICON_LAYER),
            Transform::default(),
        )],
    ));
}

/// There are no point or spot lights (the sun and moon are directional), so
/// light clustering has nothing to do. Bevy's GPU clustering still cost about
/// 0.7 ms of CPU a frame; the CPU path with no lights costs next to nothing.
fn cpu_light_clustering(mut settings: ResMut<bevy::light::cluster::GlobalClusterSettings>) {
    settings.gpu_clustering = None;
}

pub fn center_on_map(game: Res<GameMap>, mut rig: ResMut<CameraRig>) {
    let (min, max) = map_bounds(&game.map.grid);
    let distance = rig.distance;
    snap(&mut rig, (min + max) * 0.5, distance);
}

fn rig_transform(focus: Vec2, distance: f32) -> Transform {
    let target = Vec3::new(focus.x, 0.0, focus.y);
    let offset = Vec3::new(0.0, PITCH.sin(), PITCH.cos()) * distance;
    Transform::from_translation(target + offset).looking_at(target, Vec3::Y)
}

/// Where a ray from the cursor hits the horizontal plane at height `y`.
fn cursor_on_plane(window: &Window, camera: &Camera, cam_tf: &GlobalTransform, y: f32) -> Option<Vec3> {
    let ray = camera.viewport_to_world(cam_tf, window.cursor_position()?).ok()?;
    let t = (y - ray.origin.y) / ray.direction.y;
    (t > 0.0).then(|| ray.get_point(t))
}

fn drag_pan(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    hover: Res<bevy::picking::hover::HoverMap>,
    nodes: Query<(), With<Node>>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    terrain: Option<Res<crate::terrain::Terrain>>,
    mut drag: ResMut<Drag>,
    mut rig: ResMut<CameraRig>,
) {
    let (camera, cam_tf) = *camera;
    let buttons = [MouseButton::Left, MouseButton::Middle, MouseButton::Right];
    if !mouse.any_pressed(buttons) {
        *drag = Drag::default();
        return;
    }
    let Some(cursor) = window.cursor_position() else { return };
    if mouse.any_just_pressed(buttons) {
        // Presses on the interface don't drag the map, nor do Shift-drags
        // with the left button (they select units, see `play::box_select`).
        let selecting = mouse.just_pressed(MouseButton::Left) && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        drag.press = (!selecting && !crate::ui::pointer_over_ui(&hover, &nodes)).then_some(cursor);
    }
    let Some(press) = drag.press else { return };
    if drag.grab.is_none() {
        if cursor.distance(press) < DRAG_THRESHOLD {
            return;
        }
        // Grab the ground under the cursor (at its terrain height, so the
        // grabbed spot really sticks to the pointer).
        let Some(flat) = cursor_on_plane(&window, camera, cam_tf, 0.0) else { return };
        let ground = terrain.as_ref().map_or(0.0, |t| t.heights.sample(flat.xz()).max(0.0));
        drag.grab = cursor_on_plane(&window, camera, cam_tf, ground);
        return;
    }
    let grab = drag.grab.unwrap();
    if let Some(now) = cursor_on_plane(&window, camera, cam_tf, grab.y) {
        // Move the camera so the grabbed point is back under the cursor. Apply it
        // to the smoothed position too, so the map follows the pointer 1:1.
        let delta = (grab - now).xz();
        rig.focus += delta;
        rig.current_focus += delta;
    }
}

fn camera_input(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    hover: Res<bevy::picking::hover::HoverMap>,
    nodes: Query<(), With<Node>>,
    time: Res<Time>,
    game: Res<GameMap>,
    mut rig: ResMut<CameraRig>,
) {
    let dt = time.delta_secs();
    let mut pan = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        pan.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        pan.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        pan.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        pan.x += 1.0;
    }
    let speed = rig.distance * 1.2;
    rig.focus += pan.normalize_or_zero() * speed * dt;

    // Over the interface the wheel scrolls panels instead of zooming.
    let lines = match scroll.unit {
        _ if crate::ui::pointer_over_ui(&hover, &nodes) => 0.0,
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    // (= is the strategic view.)
    if keys.pressed(KeyCode::NumpadAdd) {
        rig.distance *= 1.0 - dt;
    }
    if keys.any_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
        rig.distance *= 1.0 + dt;
    }
    rig.distance = (rig.distance * 0.88f32.powf(lines)).clamp(MIN_DISTANCE, MAX_DISTANCE);

    // The map wraps east-west: keep the focus within one period, shifting the
    // smoothed position by the same amount so the jump is invisible.
    let (min, max) = map_bounds(&game.map.grid);
    let period = max.x - min.x;
    // Fly the short way round to a new focus (across the seam if that's nearer).
    rig.current_focus.x += crate::wrap::nearest_shift(rig.current_focus.x, rig.focus.x, period);
    if rig.focus.x < min.x || rig.focus.x >= max.x {
        let shift = ((rig.focus.x - min.x) / period).floor() * period;
        rig.focus.x -= shift;
        rig.current_focus.x -= shift;
    }
    rig.focus.y = rig.focus.y.clamp(min.y, max.y);
}

fn apply_rig(time: Res<Time>, mut rig: ResMut<CameraRig>, mut cam: Single<&mut Transform, With<crate::camera::MainCamera>>) {
    let k = 1.0 - (-12.0 * time.delta_secs()).exp();
    let (focus, distance) = (rig.focus, rig.distance);
    rig.current_focus = rig.current_focus.lerp(focus, k);
    rig.current_distance += (distance - rig.current_distance) * k;
    **cam = rig_transform(rig.current_focus, rig.current_distance);
}

/// The opening shot of a game: the screen fades in from the loading screen
/// while the camera glides down from high above to where play starts (set by
/// `play::start_game`). Any key or click skips it.
#[derive(Resource)]
struct Intro {
    age: f32,
    /// Starting distance; the focus is the target's from the start.
    from: f32,
    to: (Vec2, f32),
}

/// The fade from the loading screen's colour.
#[derive(Component)]
struct IntroFade;

const INTRO_SECONDS: f32 = 4.8;
const INTRO_FADE_SECONDS: f32 = 1.6;
/// A moment on the high view before the descent starts.
const INTRO_HOLD: f32 = 0.4;
const INTRO_DISTANCE: f32 = 78.0;

fn start_intro(mut commands: Commands, mut rig: ResMut<CameraRig>) {
    let to = (rig.focus, rig.distance);
    // Straight down onto the target, which stays centred throughout.
    let from = INTRO_DISTANCE;
    snap(&mut rig, to.0, from);
    commands.insert_resource(Intro { age: 0.0, from, to });
    commands.spawn((
        IntroFade,
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        BackgroundColor(Color::srgb(0.06, 0.055, 0.05)),
        GlobalZIndex(90),
        Pickable::IGNORE,
    ));
}

fn run_intro(
    mut commands: Commands,
    time: Res<Time>,
    intro: Option<ResMut<Intro>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut rig: ResMut<CameraRig>,
    mut fade: Query<(Entity, &mut BackgroundColor), With<IntroFade>>,
) {
    let Some(mut intro) = intro else { return };
    intro.age += time.delta_secs().min(0.05);
    let skip = keys.get_just_pressed().next().is_some() || mouse.get_just_pressed().next().is_some() || scroll.delta != Vec2::ZERO;
    let u = ((intro.age - INTRO_HOLD) / INTRO_SECONDS).clamp(0.0, 1.0);
    if skip || u >= 1.0 {
        snap(&mut rig, intro.to.0, intro.to.1);
        commands.remove_resource::<Intro>();
        for (e, _) in &fade {
            commands.entity(e).despawn();
        }
        return;
    }
    // Eased in and out, with a long, gentle landing.
    let fall = 1.0 - (1.0 - u * u * (3.0 - 2.0 * u)).powi(2);
    let (from, to) = (intro.from, intro.to);
    snap(&mut rig, to.0, from * (to.1 / from).powf(fall));
    let alpha = 1.0 - (intro.age / INTRO_FADE_SECONDS).clamp(0.0, 1.0);
    for (_, mut bg) in &mut fade {
        bg.0.set_alpha(alpha * alpha * (3.0 - 2.0 * alpha));
    }
}

/// Jumps the camera without smoothing (used by screenshot mode).
pub fn snap(rig: &mut CameraRig, focus: Vec2, distance: f32) {
    rig.focus = focus;
    rig.distance = distance;
    rig.current_focus = focus;
    rig.current_distance = distance;
}
