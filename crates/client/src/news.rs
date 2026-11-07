//! News: toasts under the top bar, and the notification log (L, or the scroll
//! button in the top bar) listing everything newsworthy by turn. Clicking an
//! entry looks at where it happened. Messages are posted with `Notice` (see
//! `play.rs`); the pop-up setting decides which news also shows as a toast.

use crate::camera::CameraRig;
use crate::icons::{Icon, Icons};
use crate::layout::tile_center;
use crate::loading::{AppState, InGameSet};
use crate::menu::Menu;
use crate::play::{GameEvent, NewsKind, Notice, Session, ToastLevel};
use crate::settings::{PopUps, Settings};
use crate::ui::{self, palette, Tooltip};
use bevy::prelude::*;
use bevy::picking::hover::Hovered;
use bevy::ui_widgets::{observe, Activate, Button};
use sim::game::Event;

/// Whether the log is open, and which kind of news it shows.
#[derive(Resource, Default)]
pub struct NewsLog {
    pub open: bool,
    filter: Option<NewsKind>,
}

#[derive(Component)]
struct ToastStack;

#[derive(Component)]
struct ToastNode(u64);

#[derive(Component)]
struct LogPanel;

#[derive(Component)]
struct LogScroll;

/// A log entry that looks at where it happened when clicked.
#[derive(Component)]
struct LogRow;

/// The count of unread news on the log button.
#[derive(Component)]
struct UnreadBadge;

/// Seconds a toast takes to fade out at the end.
const FADE: f32 = 0.5;

const FILTERS: [(Option<NewsKind>, Icon, &str); 5] = [
    (None, Icon::Log, "All news"),
    (Some(NewsKind::Combat), Icon::Strength, "Combat"),
    (Some(NewsKind::Cities), Icon::FoundCity, "Cities"),
    (Some(NewsKind::Research), Icon::Science, "Research"),
    (Some(NewsKind::Diplomacy), Icon::Banner, "Diplomacy"),
];

pub struct NewsPlugin;

impl Plugin for NewsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NewsLog>().add_systems(OnEnter(AppState::InGame), (spawn_toast_stack, dev_open_log)).add_systems(
            Update,
            (announce_builds, toggle_log.run_if(in_state(Menu::Closed)), show_toasts, rebuild_log, refresh_badge, hover_rows)
                .chain()
                .after(crate::play::PlaySet)
                .in_set(InGameSet),
        );
    }
}

/// The log button for the top bar, with its unread count.
pub fn log_button(parent: &mut ChildSpawnerCommands, icons: &Icons) {
    parent
        .spawn((
            ui::icon_button(icons.get(Icon::Log), 30.0, Tooltip::new("Notifications", "Everything that has happened, by turn (L)")),
            observe(|_: On<Activate>, mut log: ResMut<NewsLog>| log.open = !log.open),
        ))
        .with_child((
            UnreadBadge,
            Node {
                position_type: PositionType::Absolute,
                right: px(-6),
                top: px(-5),
                min_width: px(17),
                height: px(17),
                padding: UiRect::horizontal(px(4)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(9)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.72, 0.2, 0.14)),
            Visibility::Hidden,
            Pickable::IGNORE,
            children![(ui::bold("", 11.0, palette::TEXT), Pickable::IGNORE)],
        ));
}

/// News of our cities' finished builds.
fn announce_builds(session: Res<Session>, mut events: MessageReader<GameEvent>, mut notice: ResMut<Notice>) {
    let game = &session.game;
    for GameEvent(e) in events.read() {
        if let Event::CityBuilt { city, build } = e {
            let Some(c) = game.cities.get(city).filter(|c| c.owner == session.me) else { continue };
            let icon = crate::city_panel::build_icon(game, *build);
            notice.news(game.turn, NewsKind::Cities, icon, format!("{} built {}", c.name, game.build_name(*build)), Some(c.tile));
        }
    }
}

/// Dev aid: FOURX_LOG=1 opens the log.
fn dev_open_log(mut log: ResMut<NewsLog>) {
    if std::env::var("FOURX_LOG").is_ok() {
        log.open = true;
    }
}

fn toggle_log(keys: Res<ButtonInput<KeyCode>>, mut log: ResMut<NewsLog>) {
    if keys.just_pressed(KeyCode::KeyL) {
        log.open = !log.open;
    }
}

fn spawn_toast_stack(mut commands: Commands) {
    commands.spawn((
        ToastStack,
        Node {
            position_type: PositionType::Absolute,
            top: px(crate::topbar::HEIGHT + 14.0),
            width: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            ..default()
        },
        Pickable::IGNORE,
    ));
}

fn shown(level: ToastLevel, popups: PopUps) -> bool {
    matches!((level, popups), (ToastLevel::Feedback, _) | (_, PopUps::All) | (ToastLevel::Alert, PopUps::Alerts))
}

/// Keeps the stack of toasts in line with `Notice`: new ones appear on top,
/// old ones fade out.
fn show_toasts(
    mut commands: Commands,
    time: Res<Time>,
    icons: Res<Icons>,
    settings: Res<Settings>,
    mut notice: ResMut<Notice>,
    stack: Single<Entity, With<ToastStack>>,
    nodes: Query<(Entity, &ToastNode, &Children)>,
    mut fills: Query<(&mut BackgroundColor, &mut BorderColor)>,
    mut texts: Query<&mut TextColor>,
    mut images: Query<&mut ImageNode>,
) {
    // Ticking the timers isn't a change anyone needs to see.
    let notice = notice.bypass_change_detection();
    let dt = time.delta_secs();
    for t in &mut notice.toasts {
        t.timer -= dt;
    }
    notice.toasts.retain(|t| t.timer > 0.0 && shown(t.level, settings.popups));

    for (e, node, _) in &nodes {
        if !notice.toasts.iter().any(|t| t.id == node.0) {
            commands.entity(e).despawn();
        }
    }
    for t in &notice.toasts {
        let Some((node, _, children)) = nodes.iter().find(|(_, n, _)| n.0 == t.id) else {
            let edge = if t.level == ToastLevel::Alert { palette::DANGER } else { palette::PANEL_BORDER };
            let toast = commands
                .spawn((
                    ToastNode(t.id),
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(10),
                        max_width: px(720),
                        padding: UiRect::axes(px(16), px(9)),
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(palette::PANEL),
                    BorderColor::all(edge),
                    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.55), px(0), px(4), px(2), px(16)),
                    Pickable::IGNORE,
                    children![
                        ui::icon(icons.get(t.icon), 24.0),
                        (ui::bold(t.text.clone(), 16.0, palette::TEXT), Pickable::IGNORE),
                    ],
                ))
                .id();
            // Newest on top.
            commands.entity(*stack).insert_children(0, &[toast]);
            continue;
        };
        let alpha = (t.timer / FADE).min(1.0);
        if alpha < 1.0 {
            for &child in children {
                if let Ok(mut c) = texts.get_mut(child) {
                    c.0.set_alpha(alpha);
                }
                if let Ok(mut i) = images.get_mut(child) {
                    i.color.set_alpha(alpha);
                }
            }
            if let Ok((mut bg, mut border)) = fills.get_mut(node) {
                bg.0.set_alpha(palette::PANEL.alpha() * alpha);
                let edge = if t.level == ToastLevel::Alert { palette::DANGER } else { palette::PANEL_BORDER };
                *border = BorderColor::all(edge.with_alpha(alpha));
            }
        }
    }
}

fn refresh_badge(
    notice: Res<Notice>,
    badge: Single<(&mut Visibility, &Children), With<UnreadBadge>>,
    mut texts: Query<&mut Text>,
) {
    if !notice.is_changed() {
        return;
    }
    let (mut visibility, children) = badge.into_inner();
    visibility.set_if_neq(if notice.unread > 0 { Visibility::Inherited } else { Visibility::Hidden });
    if let Some(mut text) = children.first().and_then(|&c| texts.get_mut(c).ok()) {
        let count = if notice.unread > 99 { "99+".to_string() } else { notice.unread.to_string() };
        if text.0 != count {
            text.0 = count;
        }
    }
}

fn rebuild_log(
    mut commands: Commands,
    mut notice: ResMut<Notice>,
    log: Res<NewsLog>,
    icons: Res<Icons>,
    old: Query<Entity, With<LogPanel>>,
    old_scroll: Query<&ScrollPosition, With<LogScroll>>,
) {
    if !notice.is_changed() && !log.is_changed() {
        return;
    }
    if log.open && notice.unread > 0 {
        notice.unread = 0;
    }
    let scroll = old_scroll.iter().next().map_or(Vec2::ZERO, |s| s.0);
    for e in &old {
        commands.entity(e).despawn();
    }
    if !log.open {
        return;
    }
    commands
        .spawn((
            LogPanel,
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::topbar::HEIGHT + 62.0),
                right: px(14),
                width: px(390),
                // Clear of the minimap below.
                max_height: percent(50),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            ui::frame(),
            GlobalZIndex(3),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    align_items: AlignItems::Center,
                    padding: UiRect::new(px(16), px(10), px(10), px(6)),
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|head| {
                    head.spawn((ui::heading("Notifications", 21.0, palette::ACCENT), Node { flex_grow: 1.0, ..default() }));
                    head.spawn((
                        ui::icon_button(icons.get(Icon::Close), 26.0, Tooltip::title("Close (L)")),
                        observe(|_: On<Activate>, mut log: ResMut<NewsLog>| log.open = false),
                    ));
                });
            panel.spawn(Node { column_gap: px(6), padding: UiRect::new(px(16), px(16), px(0), px(8)), ..default() }).with_children(|row| {
                for (kind, icon, name) in FILTERS {
                    let state = ui::ButtonState { active: log.filter == kind, disabled: false };
                    row.spawn((
                        ui::icon_button_with(icons.get(icon), 32.0, Tooltip::title(name), state),
                        observe(move |_: On<Activate>, mut log: ResMut<NewsLog>| log.filter = kind),
                    ));
                }
            });
            panel
                .spawn((
                    LogScroll,
                    Node {
                        flex_direction: FlexDirection::Column,
                        flex_shrink: 1.0,
                        min_height: px(0),
                        row_gap: px(3),
                        padding: UiRect::new(px(10), px(10), px(2), px(12)),
                        border: UiRect::top(px(1)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BorderColor::all(palette::BUTTON_BORDER),
                    ScrollPosition(scroll),
                ))
                .with_children(|list| {
                    let mut turn = None;
                    let entries = notice.log.iter().rev().filter(|e| log.filter.is_none_or(|k| e.kind == k));
                    for entry in entries {
                        if turn != Some(entry.turn) {
                            turn = Some(entry.turn);
                            list.spawn((
                                ui::heading(format!("Turn {}", entry.turn), 14.0, palette::TEXT_DIM),
                                Node { margin: UiRect::new(px(6), px(0), px(8), px(2)), ..default() },
                            ));
                        }
                        let at = entry.at;
                        let edge = if entry.alert { palette::DANGER } else { Color::NONE };
                        let mut row = list.spawn((
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: px(10),
                                padding: UiRect::axes(px(8), px(6)),
                                border: UiRect::left(px(3)),
                                border_radius: BorderRadius::all(px(4)),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BackgroundColor(palette::PANEL_LIGHT),
                            BorderColor::all(edge),
                            children![
                                ui::icon(icons.get(entry.icon), 22.0),
                                (ui::text(entry.text.clone(), 14.0, palette::TEXT), Node { flex_shrink: 1.0, ..default() }, Pickable::IGNORE),
                            ],
                        ));
                        if let Some(tile) = at {
                            row.insert((
                                LogRow,
                                Button,
                                Hovered::default(),
                                ui::OwnStyle,
                                Tooltip::title("Click to look there"),
                                observe(move |_: On<Activate>, session: Res<Session>, mut rig: ResMut<CameraRig>| {
                                    rig.focus = tile_center(&session.game.map.grid, tile);
                                }),
                            ));
                        }
                    }
                    if notice.log.is_empty() {
                        list.spawn(Node { margin: UiRect::all(px(8)), ..default() }).with_child(ui::hint("Nothing has happened yet."));
                    }
                });
        });
}

type HoveredRows<'w, 's> = Query<'w, 's, (&'static Hovered, &'static mut BackgroundColor), (With<LogRow>, Changed<Hovered>)>;

fn hover_rows(mut rows: HoveredRows) {
    for (hovered, mut bg) in &mut rows {
        bg.0 = if hovered.get() { palette::BUTTON_HOVER } else { palette::PANEL_LIGHT };
    }
}
