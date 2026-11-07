//! Shared UI style: dark parchment-and-bronze panels, fonts, icon buttons,
//! stat chips, progress bars and hover tooltips used by every screen.

use crate::icons::{Icon, Icons};
use bevy::asset::uuid_handle;
use bevy::picking::hover::{HoverMap, Hovered};
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

pub mod palette {
    use bevy::prelude::Color;

    pub const PANEL: Color = Color::srgba(0.085, 0.075, 0.065, 0.95);
    pub const PANEL_LIGHT: Color = Color::srgba(0.15, 0.13, 0.105, 0.95);
    pub const PANEL_BORDER: Color = Color::srgb(0.55, 0.43, 0.24);
    pub const BUTTON: Color = Color::srgb(0.17, 0.15, 0.12);
    pub const BUTTON_HOVER: Color = Color::srgb(0.27, 0.22, 0.15);
    pub const BUTTON_PRESSED: Color = Color::srgb(0.38, 0.29, 0.16);
    pub const BUTTON_ACTIVE: Color = Color::srgb(0.31, 0.24, 0.13);
    pub const BUTTON_DISABLED: Color = Color::srgb(0.11, 0.1, 0.09);
    pub const BUTTON_BORDER: Color = Color::srgb(0.42, 0.34, 0.2);
    pub const ACCENT: Color = Color::srgb(0.95, 0.8, 0.47);
    pub const TEXT: Color = Color::srgb(0.95, 0.92, 0.85);
    pub const TEXT_DIM: Color = Color::srgb(0.68, 0.64, 0.56);
    pub const FOOD: Color = Color::srgb(0.55, 0.85, 0.35);
    pub const PRODUCTION: Color = Color::srgb(0.95, 0.6, 0.28);
    pub const GOLD: Color = Color::srgb(1.0, 0.84, 0.3);
    pub const CULTURE: Color = Color::srgb(0.8, 0.6, 0.98);
    pub const SCIENCE: Color = Color::srgb(0.45, 0.75, 1.0);
    pub const DANGER: Color = Color::srgb(0.95, 0.45, 0.36);
}

/// Inter: the body font (variable weight).
const BODY_FONT_DATA: &[u8] = include_bytes!("../assets/fonts/Inter.ttf");
/// Cinzel: titles and names, for a classical strategy-game feel.
const HEADING_FONT_DATA: &[u8] = include_bytes!("../assets/fonts/Cinzel.ttf");
pub const HEADING_FONT: Handle<Font> = uuid_handle!("6b0f6b1e-3f2c-4bd2-9a59-3c1c64c2f7a1");
/// Bevy's built-in monospace font, kept for the debug overlay.
pub const MONO_FONT: Handle<Font> = uuid_handle!("d1f5bfa6-8d4c-4a51-a0c3-5a9a3e2a9b10");

pub struct UiStylePlugin;

impl Plugin for UiStylePlugin {
    fn build(&self, app: &mut App) {
        let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
        // Inter replaces the default font, so all text uses it unless told otherwise.
        let _ = fonts.insert(AssetId::default(), Font::from_bytes(BODY_FONT_DATA.to_vec()));
        let _ = fonts.insert(&HEADING_FONT, Font::from_bytes(HEADING_FONT_DATA.to_vec()));
        let _ = fonts.insert(&MONO_FONT, Font::from_bytes(bevy::text::DEFAULT_FONT_DATA.to_vec()));
        app.init_resource::<TooltipState>()
            .add_systems(Startup, spawn_tooltip)
            .add_systems(OnEnter(crate::loading::AppState::InGame), spawn_world_labels)
            .add_systems(Update, (button_feedback, show_tooltip, wheel_scroll));
    }
}

/// Body text at the given size (medium weight reads better on dark panels).
pub fn font(size: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(size), weight: FontWeight(500), ..default() }
}

pub fn font_bold(size: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(size), weight: FontWeight::BOLD, ..default() }
}

pub fn heading_font(size: f32) -> TextFont {
    TextFont { font: HEADING_FONT.into(), font_size: FontSize::Px(size), weight: FontWeight::BOLD, ..default() }
}

pub fn mono_font(size: f32) -> TextFont {
    TextFont { font: MONO_FONT.into(), font_size: FontSize::Px(size), ..default() }
}

pub fn text(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), font(size), TextColor(color))
}

pub fn bold(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), font_bold(size), TextColor(color))
}

pub fn heading(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), heading_font(size), TextColor(color))
}

/// A square icon image of the given size.
pub fn icon(image: Handle<Image>, size: f32) -> impl Bundle {
    (ImageNode::new(image), Node { width: px(size), height: px(size), flex_shrink: 0.0, ..default() }, Pickable::IGNORE)
}

/// An icon followed by a value, e.g. a yield.
pub fn stat(icons: &Icons, which: Icon, value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Node { align_items: AlignItems::Center, column_gap: px(size * 0.22), ..default() },
        children![icon(icons.get(which), size * 1.25), bold(value, size, color)],
    )
}

pub fn progress_bar(fraction: f32, color: Color, height: f32) -> impl Bundle {
    (
        Node {
            width: percent(100),
            height: px(height),
            border_radius: BorderRadius::all(px(height / 2.0)),
            border: UiRect::all(px(1)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        children![(
            Node {
                width: percent(fraction.clamp(0.0, 1.0) * 100.0),
                height: percent(100),
                border_radius: BorderRadius::all(px(height / 2.0)),
                ..default()
            },
            BackgroundColor(color),
        )],
    )
}

/// A labelled divider between panel sections.
pub fn section(title: &str) -> impl Bundle {
    (
        Node {
            align_items: AlignItems::Center,
            column_gap: px(8),
            margin: UiRect::new(px(0), px(0), px(10), px(4)),
            ..default()
        },
        children![
            (heading(title.to_uppercase(), 13.0, palette::ACCENT)),
            (Node { flex_grow: 1.0, height: px(1), ..default() }, BackgroundColor(palette::PANEL_BORDER.with_alpha(0.5))),
        ],
    )
}

pub fn panel(width: Val) -> impl Bundle {
    panel_with(Node { width, ..default() })
}

/// A panel whose node has more set than its width (e.g. a maximum height):
/// a bundle can't hold a second `Node`.
pub fn panel_with(node: Node) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(10),
            padding: UiRect::axes(px(28), px(22)),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(8)),
            ..node
        },
        BackgroundColor(palette::PANEL),
        BorderColor::all(palette::PANEL_BORDER),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), px(0), px(6), px(4), px(24)),
    )
}

/// A HUD panel's frame: dark fill, bronze edge, soft shadow.
pub fn frame() -> impl Bundle {
    (
        BackgroundColor(palette::PANEL),
        BorderColor::all(palette::PANEL_BORDER),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.55), px(0), px(4), px(2), px(16)),
    )
}

pub fn title(text: &str) -> impl Bundle {
    (heading(text.to_string(), 30.0, palette::ACCENT), TextShadow::default(), Node { margin: UiRect::bottom(px(8)), ..default() })
}

pub fn label(text: &str) -> impl Bundle {
    (Text::new(text), font(17.0), TextColor(palette::TEXT))
}

pub fn value_text(text: &str) -> impl Bundle {
    (Text::new(text), font_bold(17.0), TextColor(palette::ACCENT))
}

pub fn hint(text: &str) -> impl Bundle {
    (Text::new(text), font(13.0), TextColor(palette::TEXT_DIM), Node { margin: UiRect::top(px(6)), ..default() })
}

pub fn base_button(width: Val, height: Val) -> impl Bundle {
    (
        Button,
        Hovered::default(),
        Node {
            width,
            height,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(16)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(palette::BUTTON),
        BorderColor::all(palette::BUTTON_BORDER),
    )
}

pub fn button(text: &str) -> impl Bundle {
    (base_button(percent(100), px(42)), children![(Text::new(text), font(18.0), TextColor(palette::TEXT))])
}

/// A button as wide as its label.
pub fn text_button(text: &str) -> impl Bundle {
    (base_button(Val::Auto, px(30)), children![(Text::new(text), font(13.5), TextColor(palette::TEXT))])
}

pub fn small_button(text: &str) -> impl Bundle {
    (base_button(px(36), px(30)), children![(Text::new(text), font(17.0), TextColor(palette::TEXT))])
}

/// How a button looks beyond hover: highlighted (the current choice of
/// several) or disabled (dimmed; it still shows its tooltip).
#[derive(Component, Clone, Copy, Default, PartialEq)]
pub struct ButtonState {
    pub active: bool,
    pub disabled: bool,
}

/// A square button showing an icon, with a tooltip.
pub fn icon_button(image: Handle<Image>, size: f32, tip: Tooltip) -> impl Bundle {
    icon_button_with(image, size, tip, ButtonState::default())
}

pub fn icon_button_with(image: Handle<Image>, size: f32, tip: Tooltip, state: ButtonState) -> impl Bundle {
    let tint = if state.disabled { Color::srgba(1.0, 1.0, 1.0, 0.35) } else { Color::WHITE };
    (
        Button,
        Hovered::default(),
        Node {
            width: px(size),
            height: px(size),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::all(px(size * 0.18)),
            ..default()
        },
        BackgroundColor(palette::BUTTON),
        BorderColor::all(palette::BUTTON_BORDER),
        state,
        tip,
        children![(
            ImageNode { color: tint, ..ImageNode::new(image) },
            Node { width: px(size * 0.66), height: px(size * 0.66), ..default() },
            Pickable::IGNORE,
        )],
    )
}

/// A small key cap ("E") pinned to a button's corner.
pub fn hotkey_badge(key: &str) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            right: px(-5),
            bottom: px(-5),
            min_width: px(16),
            height: px(16),
            padding: UiRect::horizontal(px(3)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.07, 0.06, 0.05)),
        BorderColor::all(palette::BUTTON_BORDER),
        Pickable::IGNORE,
        children![(bold(key.to_string(), 10.0, palette::TEXT_DIM), Pickable::IGNORE)],
    )
}

/// A button that draws itself (no hover or pressed colours from `button_feedback`).
#[derive(Component)]
pub struct OwnStyle;

fn button_feedback(
    mut buttons: Query<
        (&Hovered, Has<Pressed>, Option<&ButtonState>, &mut BackgroundColor, &mut BorderColor),
        (With<Button>, Without<OwnStyle>),
    >,
) {
    for (hovered, pressed, state, mut bg, mut border) in &mut buttons {
        let state = state.copied().unwrap_or_default();
        let hover = hovered.get() && !state.disabled;
        let fill = match (hover, pressed && !state.disabled) {
            (_, true) => palette::BUTTON_PRESSED,
            (true, false) => palette::BUTTON_HOVER,
            _ if state.active => palette::BUTTON_ACTIVE,
            _ if state.disabled => palette::BUTTON_DISABLED,
            _ => palette::BUTTON,
        };
        let edge = if hover || state.active { palette::ACCENT } else { palette::BUTTON_BORDER };
        bg.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(edge));
    }
}

/// The one UI root that screen-anchored world labels (unit flags, city
/// banners, resource badges, price tags, site markers) hang from. Hundreds of
/// separate roots would each be laid out every frame. Spawned for each game;
/// leaving the game despawns it with its labels.
#[derive(Component)]
pub struct WorldLabels;

fn spawn_world_labels(mut commands: Commands) {
    commands.spawn((
        WorldLabels,
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        Pickable::IGNORE,
    ));
}

/// Places a screen-anchored world label at `at` (its top-left, in UI pixels),
/// or hides it when that's missing or further than `margin` outside the
/// screen (`screen`, in UI pixels). Writes only what changed: a moved `Node`
/// makes the UI lay itself out again, and hidden labels aren't drawn.
pub fn place_label(node: &mut Mut<Node>, vis: &mut Mut<Visibility>, at: Option<Vec2>, screen: Vec2, margin: f32) {
    let on_screen = at.filter(|p| p.cmpge(Vec2::splat(-margin)).all() && p.cmple(screen + margin).all());
    let Some(p) = on_screen else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    if node.left != Val::Px(p.x) || node.top != Val::Px(p.y) {
        node.left = Val::Px(p.x);
        node.top = Val::Px(p.y);
    }
    vis.set_if_neq(Visibility::Inherited);
}

/// The screen's size in UI pixels.
pub fn ui_screen(camera: &Camera, scale: f32) -> Vec2 {
    camera.logical_viewport_size().unwrap_or(Vec2::splat(1e5)) / scale
}

/// Whether the mouse is over a UI element (so clicks, drags and the wheel
/// belong to the UI rather than the map).
pub fn pointer_over_ui(hover: &HoverMap, nodes: &Query<(), With<Node>>) -> bool {
    hover.get(&PointerId::Mouse).is_some_and(|hits| hits.keys().any(|&e| nodes.contains(e)))
}

/// The mouse wheel scrolls the scrollable panel under the cursor: down if it
/// can, sideways if it only scrolls sideways or with Shift held.
fn wheel_scroll(
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    keys: Res<ButtonInput<KeyCode>>,
    hover: Res<HoverMap>,
    parents: Query<&ChildOf>,
    mut panels: Query<(&Node, &ComputedNode, &mut ScrollPosition)>,
) {
    if scroll.delta == Vec2::ZERO {
        return;
    }
    let mut pixels = match scroll.unit {
        bevy::input::mouse::MouseScrollUnit::Line => scroll.delta * 40.0,
        bevy::input::mouse::MouseScrollUnit::Pixel => scroll.delta,
    };
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        pixels = Vec2::new(pixels.x + pixels.y, 0.0);
    }
    let Some(hits) = hover.get(&PointerId::Mouse) else { return };
    for &hit in hits.keys() {
        let mut cur = hit;
        for _ in 0..12 {
            if let Ok((node, computed, mut pos)) = panels.get_mut(cur) {
                let room = ((computed.content_size - computed.size) * computed.inverse_scale_factor).max(Vec2::ZERO);
                let x = node.overflow.x == OverflowAxis::Scroll && room.x > 0.5;
                let y = node.overflow.y == OverflowAxis::Scroll && room.y > 0.5;
                if x || y {
                    let mut d = pixels;
                    if !y {
                        d = Vec2::new(d.x + d.y, 0.0);
                    }
                    if x {
                        pos.0.x = (pos.0.x - d.x).clamp(0.0, room.x);
                    }
                    if y {
                        pos.0.y = (pos.0.y - d.y).clamp(0.0, room.y);
                    }
                    return;
                }
            }
            let Ok(parent) = parents.get(cur) else { break };
            cur = parent.parent();
        }
    }
}

// --- Tooltips ----------------------------------------------------------------

/// Hover text for a UI element: a title line and optional details.
#[derive(Component, Clone, Default)]
pub struct Tooltip {
    pub title: String,
    pub body: String,
}

impl Tooltip {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self { title: title.into(), body: body.into() }
    }

    pub fn title(title: impl Into<String>) -> Self {
        Self { title: title.into(), body: String::new() }
    }
}

#[derive(Resource, Default)]
struct TooltipState {
    target: Option<Entity>,
    since: f32,
}

#[derive(Component)]
struct TooltipBox;

#[derive(Component)]
struct TooltipTitle;

#[derive(Component)]
struct TooltipBody;

fn spawn_tooltip(mut commands: Commands) {
    commands.spawn((
        TooltipBox,
        Node {
            position_type: PositionType::Absolute,
            max_width: px(340),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            padding: UiRect::axes(px(12), px(9)),
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        frame(),
        Visibility::Hidden,
        GlobalZIndex(30),
        Pickable::IGNORE,
        children![
            (TooltipTitle, bold("", 15.0, palette::ACCENT), Pickable::IGNORE),
            // A width limit on the text itself, so it is measured wrapped
            // (otherwise long bodies overflow the box).
            (TooltipBody, text("", 13.5, palette::TEXT), Node { max_width: px(314), ..default() }, Pickable::IGNORE),
        ],
    ));
}

/// Shows the tooltip of the hovered element after a short delay, next to the
/// cursor and kept inside the window.
fn show_tooltip(
    time: Res<Time>,
    hover: Res<HoverMap>,
    window: Single<&Window>,
    ui_scale: Res<UiScale>,
    tips: Query<&Tooltip>,
    parents: Query<&ChildOf>,
    mut state: ResMut<TooltipState>,
    mut tip_box: Single<(&mut Node, &mut Visibility, &ComputedNode), With<TooltipBox>>,
    mut title: Single<&mut Text, (With<TooltipTitle>, Without<TooltipBody>)>,
    mut body: Single<(&mut Text, &mut Node), (With<TooltipBody>, Without<TooltipBox>)>,
) {
    // The hovered entity (or an ancestor) that has a tooltip.
    let target = hover.get(&PointerId::Mouse).and_then(|hits| {
        hits.keys().find_map(|&e| {
            let mut cur = e;
            for _ in 0..4 {
                if tips.contains(cur) {
                    return Some(cur);
                }
                cur = parents.get(cur).ok()?.parent();
            }
            None
        })
    });
    if target != state.target {
        state.target = target;
        state.since = 0.0;
    }
    state.since += time.delta_secs();
    let (node, vis, computed) = &mut *tip_box;
    let tip = target.and_then(|e| tips.get(e).ok().cloned());
    let (Some(tip), Some(cursor)) = (tip, window.cursor_position()) else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    if state.since < 0.3 {
        vis.set_if_neq(Visibility::Hidden);
        return;
    }
    if title.0 != tip.title {
        title.0 = tip.title.clone();
    }
    let (body_text, body_node) = &mut *body;
    if body_text.0 != tip.body {
        body_text.0 = tip.body.clone();
    }
    body_node.display = if tip.body.is_empty() { Display::None } else { Display::Flex };
    vis.set_if_neq(Visibility::Inherited);
    // Below-right of the cursor, flipped to stay on screen.
    let scale = ui_scale.0;
    let size = computed.size() * computed.inverse_scale_factor() * scale;
    let (w, h) = (window.width(), window.height());
    let mut x = cursor.x + 18.0;
    let mut y = cursor.y + 22.0;
    if x + size.x > w - 8.0 {
        x = cursor.x - size.x - 12.0;
    }
    if y + size.y > h - 8.0 {
        y = cursor.y - size.y - 12.0;
    }
    node.left = px(x.max(4.0) / scale);
    node.top = px(y.max(4.0) / scale);
}
