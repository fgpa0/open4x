//! Minimap (bottom right, above the turn button): the explored world in atlas
//! colours, territory tinted with each civ's colour and edged at borders,
//! cities and our units as dots, and a frame around what the camera sees.
//! Clicking or dragging on it moves the camera. M hides it.

use crate::camera::{CameraRig, MainCamera};
use crate::layout::{hex_at, hex_center, map_bounds, tile_center};
use crate::loading::{AppState, InGameSet};
use crate::menu::Menu;
use crate::play::{GameEvent, Session};
use crate::ui::{palette, Tooltip};
use bevy::asset::RenderAssetUsages;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rayon::prelude::*;

/// On-screen width; the height follows the map's shape.
const WIDTH: f32 = 300.0;
const MAX_HEIGHT: f32 = 190.0;
/// Texture pixels per screen pixel, for crisp borders.
const SUPERSAMPLE: f32 = 2.0;
/// Space kept free below for the turn button.
const BOTTOM: f32 = 118.0;
/// Most often the picture is repainted, in seconds (it's repainted when
/// anything happens, and much happens at once at turn end).
const REPAINT_EVERY: f32 = 0.25;

#[derive(Resource)]
struct Minimap {
    image: Handle<Image>,
    /// Screen size of the map picture.
    size: Vec2,
    dirty: bool,
    since: f32,
    /// A drag that started on the minimap.
    dragging: bool,
}

#[derive(Component)]
struct MinimapRoot;

#[derive(Component)]
struct MinimapPicture;

/// The camera's view frame; the second copy shows the part that wraps around
/// the east–west edge.
#[derive(Component)]
struct ViewFrame(usize);

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InGame), spawn).add_systems(
            Update,
            (toggle.run_if(in_state(Menu::Closed)), mark_dirty, repaint, click, frame_view)
                .chain()
                .after(crate::play::PlaySet)
                .in_set(InGameSet),
        );
    }
}

fn spawn(mut commands: Commands, session: Res<Session>, mut images: ResMut<Assets<Image>>) {
    let (min, max) = map_bounds(&session.game.map.grid);
    let span = max - min;
    let size = Vec2::new(WIDTH, (WIDTH * span.y / span.x).min(MAX_HEIGHT));
    let image = images.add(paint(&session, size));
    commands.insert_resource(Minimap { image: image.clone(), size, dirty: false, since: 0.0, dragging: false });
    commands
        .spawn((
            MinimapRoot,
            Node {
                position_type: PositionType::Absolute,
                right: px(16),
                bottom: px(BOTTOM),
                width: px(size.x + 4.0),
                height: px(size.y + 4.0),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(palette::PANEL),
            BorderColor::all(palette::PANEL_BORDER),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.55), px(0), px(4), px(2), px(16)),
            Tooltip::new("Map", "Click or drag to look somewhere else. M hides it."),
        ))
        .with_children(|root| {
            root.spawn((MinimapPicture, ImageNode::new(image), Node { width: px(size.x), height: px(size.y), ..default() }, Pickable::IGNORE));
            for i in 0..2 {
                root.spawn((
                    ViewFrame(i),
                    Node { position_type: PositionType::Absolute, border: UiRect::all(px(1.5)), border_radius: BorderRadius::all(px(2)), ..default() },
                    BorderColor::all(Color::srgba(1.0, 0.95, 0.8, 0.9)),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                ));
            }
        });
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut root: Single<&mut Visibility, With<MinimapRoot>>) {
    if keys.just_pressed(KeyCode::KeyM) {
        let next = if **root == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
        **root = next;
    }
}

fn mark_dirty(mut events: MessageReader<GameEvent>, mut map: ResMut<Minimap>) {
    if events.read().next().is_some() {
        map.dirty = true;
    }
}

fn repaint(time: Res<Time>, session: Res<Session>, mut map: ResMut<Minimap>, mut images: ResMut<Assets<Image>>) {
    map.since += time.delta_secs();
    if !map.dirty || map.since < REPAINT_EVERY {
        return;
    }
    map.dirty = false;
    map.since = 0.0;
    let fresh = paint(&session, map.size);
    if let Some(mut image) = images.get_mut(&map.image) {
        *image = fresh;
    }
}

type Rgb = [f32; 3];

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn civ_rgb(c: (u8, u8, u8)) -> Rgb {
    [c.0 as f32, c.1 as f32, c.2 as f32]
}

/// Smooth noise (0..1) for the unexplored cloud.
fn value_noise(p: Vec2) -> f32 {
    let hash = |x: i32, y: i32| {
        let h = (x as u32).wrapping_mul(1_597_334_677) ^ (y as u32).wrapping_mul(3_812_015_801);
        let h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
        (h ^ (h >> 15)) as f32 / u32::MAX as f32
    };
    let (i, f) = (p.floor(), p - p.floor());
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let (x, y) = (i.x as i32, i.y as i32);
    let top = hash(x, y) + (hash(x + 1, y) - hash(x, y)) * u.x;
    let bottom = hash(x, y + 1) + (hash(x + 1, y + 1) - hash(x, y + 1)) * u.x;
    top + (bottom - top) * u.y
}

/// Paints what this screen's player knows of the world.
fn paint(session: &Session, size: Vec2) -> Image {
    let game = &session.game;
    let grid = &game.map.grid;
    let player = &game.players[session.me as usize];
    let (min, max) = map_bounds(grid);
    let (width, height) = ((size.x * SUPERSAMPLE) as u32, (size.y * SUPERSAMPLE) as u32);
    let texel = (max - min) / Vec2::new(width as f32, height as f32);
    let owner = |t: u32| game.territory_owner(t);
    let mut data: Vec<u8> = (0..height)
        .into_par_iter()
        .flat_map_iter(|y| {
            (0..width).flat_map(move |x| {
                let p = min + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * texel;
                let h = hex_at(p);
                let tile = grid.tile(h).filter(|&t| player.explored[t as usize]);
                let c: Rgb = match tile {
                    // Unexplored: the fog's slate grey, mottled so it reads as cloud, not a hole.
                    None => {
                        let mottle = value_noise(p * 0.3) * 0.65 + value_noise(p * 0.9 + 17.0) * 0.35;
                        mix([44.0, 48.0, 56.0], [74.0, 78.0, 86.0], mottle)
                    }
                    Some(t) => {
                        let mut c = crate::map_preview::tile_color(&game.map, &game.rules, t);
                        if let Some(o) = owner(t) {
                            let civ = civ_rgb(game.civ(o).colors.primary);
                            c = mix(c, civ, 0.35);
                            // Border: the hex across the nearest edge belongs to someone else.
                            let centre = hex_center(h);
                            let out = (p - centre).normalize_or_zero();
                            let across = grid.tile(hex_at(centre + out * 1.2));
                            let edge = (p - centre).length() > 0.62;
                            if edge && across.is_some_and(|n| owner(n) != Some(o)) {
                                c = mix(civ, [255.0; 3], 0.15);
                            }
                        }
                        if !player.sees(t) {
                            // Remembered, not seen: dimmer and greyer.
                            let lum = (c[0] + c[1] + c[2]) / 3.0;
                            c = mix(c, [lum; 3], 0.4).map(|v| v * 0.68);
                        }
                        c
                    }
                };
                [c[0] as u8, c[1] as u8, c[2] as u8, 255]
            })
        })
        .collect();

    let mut dot = |at: Vec2, radius: f32, fill: Rgb, ring: Rgb| {
        let centre = (at - min) / texel;
        let r = radius + 1.2;
        for y in (centre.y - r).floor() as i32..=(centre.y + r).ceil() as i32 {
            for x in (centre.x - r).floor() as i32..=(centre.x + r).ceil() as i32 {
                let (wx, wy) = (x.rem_euclid(width as i32) as u32, y);
                if wy < 0 || wy >= height as i32 {
                    continue;
                }
                let d = Vec2::new(x as f32 + 0.5, wy as f32 + 0.5).distance(centre);
                let c = if d <= radius {
                    fill
                } else if d <= r {
                    ring
                } else {
                    continue;
                };
                let i = ((wy as u32 * width + wx) * 4) as usize;
                data[i..i + 3].copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8]);
            }
        }
    };
    for unit in game.units.values().filter(|u| u.owner == session.me) {
        dot(tile_center(grid, unit.tile), 1.4 * SUPERSAMPLE, [250.0, 246.0, 236.0], [40.0, 34.0, 28.0]);
    }
    for city in game.cities.values().filter(|c| player.explored[c.tile as usize]) {
        let colors = &game.civ(city.owner).colors;
        dot(tile_center(grid, city.tile), 2.6 * SUPERSAMPLE, civ_rgb(colors.primary), civ_rgb(colors.secondary));
    }

    let mut image = Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = bevy::image::ImageSampler::linear();
    image
}

/// Where the cursor is on the map picture (0..1 each way), if it's over it.
fn cursor_on_map(window: &Window, picture: (&ComputedNode, &UiGlobalTransform)) -> Option<Vec2> {
    let cursor = window.physical_cursor_position()?;
    let (node, transform) = picture;
    let size = node.size();
    let uv = (cursor - transform.translation) / size + Vec2::splat(0.5);
    (uv.cmpge(Vec2::ZERO).all() && uv.cmple(Vec2::ONE).all()).then_some(uv)
}

fn click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    picture: Single<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), With<MinimapPicture>>,
    root: Single<Entity, With<MinimapRoot>>,
    hover: Res<HoverMap>,
    session: Res<Session>,
    mut map: ResMut<Minimap>,
    mut rig: ResMut<CameraRig>,
) {
    let (node, transform, visible) = *picture;
    let uv = cursor_on_map(&window, (node, transform)).filter(|_| visible.get());
    if mouse.just_pressed(MouseButton::Left) {
        // Only when the minimap itself is under the cursor, not a panel over it.
        let on_top = hover.get(&PointerId::Mouse).is_some_and(|hits| hits.contains_key(&*root));
        map.dragging = uv.is_some() && on_top;
    }
    if !mouse.pressed(MouseButton::Left) {
        map.dragging = false;
    }
    if !map.dragging {
        return;
    }
    let Some(uv) = uv else { return };
    let (min, max) = map_bounds(&session.game.map.grid);
    rig.focus = min + (max - min) * uv;
}

/// Frames the ground the camera sees (the bounding box of the screen's corners).
fn frame_view(
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    session: Res<Session>,
    map: Res<Minimap>,
    mut frames: Query<(&ViewFrame, &mut Node, &mut Visibility)>,
) {
    let (camera, cam_tf) = *camera;
    let size = window.size();
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for corner in [Vec2::ZERO, Vec2::new(size.x, 0.0), Vec2::new(0.0, size.y), size] {
        let Ok(ray) = camera.viewport_to_world(cam_tf, corner) else { return };
        if ray.direction.y >= -1e-4 {
            return;
        }
        let ground = ray.get_point(-ray.origin.y / ray.direction.y).xz();
        lo = lo.min(ground);
        hi = hi.max(ground);
    }
    let (min, max) = map_bounds(&session.game.map.grid);
    let period = max.x - min.x;
    let scale = map.size / (max - min);
    // Shift the frame into the map's period; a part beyond the east edge shows on the west.
    let shift = ((lo.x - min.x) / period).floor() * period;
    let (lo, hi) = (lo - Vec2::new(shift, 0.0), hi - Vec2::new(shift, 0.0));
    for (frame, mut node, mut visibility) in &mut frames {
        let offset = frame.0 as f32 * period;
        let (a, b) = ((lo - min - Vec2::new(offset, 0.0)) * scale, (hi - min - Vec2::new(offset, 0.0)) * scale);
        let shown = frame.0 == 0 || hi.x - offset > max.x - period;
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
        node.left = px(a.x);
        node.top = px(a.y);
        node.width = px(b.x - a.x);
        node.height = px(b.y - a.y);
    }
}
