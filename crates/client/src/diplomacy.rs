//! Diplomacy: the other civilizations in a row under the top bar on the
//! right, with a shield for each civ you've met (framed in red when at war)
//! and a question mark for those you haven't. Clicking a civ you've met opens
//! its panel, where you can trade (peace is part of a deal, see `trade.rs`)
//! or declare war.
//! Attacking a civ you're at peace with asks first, since it means war.

use crate::icons::{Icon, Icons};
use crate::loading::{AppState, InGameSet};
use crate::play::{GameEvent, NewsKind, Notice, Session};
use crate::topbar::HEIGHT;
use crate::ui::{self, palette, ButtonState, OwnStyle, Tooltip};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate, Button};
use sim::game::{Command, Event, PlayerId, UnitId};
use sim::hex::TileId;

const BADGE: f32 = 38.0;
/// Where side panels (a civ's, a city's) start, below the row of civs.
pub const PANEL_TOP: f32 = HEIGHT + 8.0 + BADGE + 16.0;

/// Which civ's panel is open, and whether its Declare War button is waiting
/// for the confirming second click.
#[derive(Resource, Default)]
pub struct CivMenu {
    pub open: Option<PlayerId>,
    confirm: bool,
}

/// An attack on a civ we're at peace with, waiting for the player to agree
/// to declare war.
#[derive(Resource, Default)]
pub struct WarPrompt(pub Option<PendingAttack>);

#[derive(Clone)]
pub struct PendingAttack {
    /// The attacking unit, or the whole group.
    pub units: Vec<UnitId>,
    pub tile: TileId,
    pub on: PlayerId,
}

/// The row of civs, remembering what it shows (met, at war, eliminated) for each player.
#[derive(Component)]
struct CivList(Vec<(bool, bool, bool)>);

#[derive(Component)]
struct CivPanel;

#[derive(Component)]
struct PromptRoot;

pub struct DiplomacyPlugin;

impl Plugin for DiplomacyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CivMenu>()
            .init_resource::<WarPrompt>()
            .add_systems(OnEnter(AppState::InGame), spawn_civ_list)
            .add_systems(
                Update,
                (reset, announce, refresh_civ_list, refresh_panel, refresh_prompt)
                    .chain()
                    .after(crate::play::PlaySet)
                    .in_set(InGameSet),
            );
    }
}

/// A new game starts with nothing open.
fn reset(session: Res<Session>, mut menu: ResMut<CivMenu>, mut prompt: ResMut<WarPrompt>) {
    if session.is_added() {
        *menu = CivMenu::default();
        prompt.0 = None;
    }
}

/// News about meetings and wars.
fn announce(session: Res<Session>, mut events: MessageReader<GameEvent>, mut notice: ResMut<Notice>) {
    let game = &session.game;
    let me = session.me;
    let name = |p: PlayerId| game.civ(p).name.clone();
    let (turn, kind) = (game.turn, NewsKind::Diplomacy);
    for GameEvent(e) in events.read() {
        match *e {
            Event::Met { player, other } if player == me || other == me => {
                let them = if player == me { other } else { player };
                notice.news(turn, kind, Icon::Banner, format!("You met {} ({})", name(them), game.civ(them).leader), None);
            }
            Event::WarDeclared { by, on } if on == me => notice.alert(turn, kind, format!("{} declared war on you!", name(by)), None),
            Event::WarDeclared { by, on } if by == me => notice.news(turn, kind, Icon::Strength, format!("You declared war on {}", name(on)), None),
            Event::WarDeclared { by, on } if game.has_met(me, by) && game.has_met(me, on) => {
                notice.news(turn, kind, Icon::Strength, format!("{} declared war on {}", name(by), name(on)), None);
            }
            Event::PlayerEliminated { player } if player != me => {
                let who = if game.has_met(me, player) { name(player) } else { "A civilization you never met".into() };
                notice.news(turn, kind, Icon::Warning, format!("{who} has been eliminated"), None);
            }
            _ => {}
        }
    }
}

fn spawn_civ_list(mut commands: Commands) {
    commands.spawn((
        CivList(Vec::new()),
        Node { position_type: PositionType::Absolute, top: px(HEIGHT + 8.0), right: px(14), column_gap: px(4), ..default() },
        GlobalZIndex(1),
    ));
}

fn refresh_civ_list(
    mut commands: Commands,
    session: Res<Session>,
    menu: Res<CivMenu>,
    icons: Res<Icons>,
    mut list: Query<(Entity, &mut CivList)>,
) {
    let Ok((root, mut shown)) = list.single_mut() else { return };
    let game = &session.game;
    let me = session.me;
    let state: Vec<(bool, bool, bool)> =
        (0..game.players.len() as PlayerId).map(|p| (game.has_met(me, p), game.at_war(me, p), game.players[p as usize].eliminated)).collect();
    if shown.0 == state && !menu.is_changed() {
        return;
    }
    shown.0 = state;
    commands.entity(root).despawn_children();
    for p in (0..game.players.len() as PlayerId).filter(|&p| p != me) {
        let war = game.at_war(me, p);
        let open = menu.open == Some(p);
        // A frame around each shield: red when at war, gold when its panel is open.
        let frame = (
            Node {
                padding: UiRect::all(px(2)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(7)),
                ..default()
            },
            BorderColor::all(match (war, open) {
                (true, _) => palette::DANGER,
                (false, true) => palette::ACCENT,
                _ => Color::NONE,
            }),
            BackgroundColor(if war { Color::srgba(0.55, 0.08, 0.05, 0.55) } else { Color::NONE }),
        );
        let entity = if game.has_met(me, p) {
            let civ = game.civ(p);
            let out = game.players[p as usize].eliminated;
            let status = if out { "Eliminated" } else if war { "At war" } else { "At peace" };
            let tip = Tooltip::new(civ.name.clone(), format!("{}, {}\n{status}\n\nClick for diplomacy.", civ.leader, civ.leader_title));
            let badge = commands
                .spawn((
                    frame,
                    Button,
                    Hovered::default(),
                    OwnStyle,
                    tip,
                    observe(move |_: On<Activate>, mut menu: ResMut<CivMenu>| {
                        menu.open = if menu.open == Some(p) { None } else { Some(p) };
                        menu.confirm = false;
                    }),
                ))
                .with_children(|b| {
                    b.spawn((crate::frontend::civ_badge(&icons, Some(civ), BADGE), Pickable::IGNORE));
                })
                .id();
            if out {
                // Faded out behind a dark veil.
                commands.entity(badge).with_children(|b| {
                    b.spawn((
                        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), border_radius: BorderRadius::all(px(5)), ..default() },
                        BackgroundColor(Color::srgba(0.05, 0.04, 0.03, 0.7)),
                        Pickable::IGNORE,
                    ));
                });
            }
            if war {
                commands.entity(badge).with_children(|b| {
                    b.spawn(war_mark(&icons));
                });
            }
            badge
        } else {
            commands
                .spawn((frame, Pickable::default(), Tooltip::new("Unknown civilization", "You haven't met this civilization yet.")))
                .with_children(|b| {
                    b.spawn((
                        Node {
                            width: px(BADGE),
                            height: px(BADGE),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        ImageNode { color: Color::srgb(0.3, 0.28, 0.26), ..ImageNode::new(icons.white(Icon::Banner)) },
                        Pickable::IGNORE,
                        children![(ui::heading("?", 19.0, palette::TEXT_DIM), Node { margin: UiRect::bottom(px(3)), ..default() }, Pickable::IGNORE)],
                    ));
                })
                .id()
        };
        commands.entity(root).add_child(entity);
    }
}

/// Crossed swords in a red disc, on the corner of a civ we're at war with.
fn war_mark(icons: &Icons) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            right: px(-7),
            bottom: px(-7),
            width: px(21),
            height: px(21),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::srgb(0.62, 0.1, 0.07)),
        BorderColor::all(palette::DANGER),
        Pickable::IGNORE,
        children![ui::icon(icons.white(Icon::Strength), 14.0)],
    )
}

/// The open civ's panel: who they are, war or peace, and what you can do.
fn refresh_panel(
    mut commands: Commands,
    session: Res<Session>,
    menu: Res<CivMenu>,
    icons: Res<Icons>,
    panels: Query<Entity, With<CivPanel>>,
) {
    if !menu.is_changed() && !session.is_changed() {
        return;
    }
    for e in &panels {
        commands.entity(e).despawn();
    }
    let game = &session.game;
    let me = session.me;
    let Some(p) = menu.open.filter(|&p| game.has_met(me, p)) else { return };
    let civ = game.civ(p);
    let war = game.at_war(me, p);
    commands
        .spawn((
            CivPanel,
            Node {
                position_type: PositionType::Absolute,
                top: px(PANEL_TOP),
                right: px(14),
                width: px(310),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(14)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            ui::frame(),
            GlobalZIndex(3),
        ))
        .with_children(|panel| {
            panel.spawn(Node { align_items: AlignItems::Center, column_gap: px(10), ..default() }).with_children(|head| {
                head.spawn(crate::frontend::civ_badge(&icons, Some(civ), 46.0));
                head.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: px(2), ..default() }).with_children(|names| {
                    names.spawn(ui::heading(civ.name.clone(), 19.0, palette::ACCENT));
                    names.spawn(ui::text(format!("{}, {}", civ.leader, civ.leader_title), 13.0, palette::TEXT_DIM));
                });
                head.spawn((
                    ui::icon_button(icons.get(Icon::Close), 26.0, Tooltip::title("Close")),
                    observe(|_: On<Activate>, mut menu: ResMut<CivMenu>| *menu = CivMenu::default()),
                ));
            });
            let out = game.players[p as usize].eliminated;
            panel.spawn(Node { align_items: AlignItems::Center, column_gap: px(6), ..default() }).with_children(|row| {
                if out {
                    row.spawn(ui::bold("Eliminated", 15.0, palette::TEXT_DIM));
                } else if war {
                    row.spawn(ui::icon(icons.get(Icon::Strength), 18.0));
                    row.spawn(ui::bold("At war", 15.0, palette::DANGER));
                } else {
                    row.spawn(ui::bold("At peace", 15.0, palette::FOOD));
                }
            });
            let (score, mine) = (game.score(p), game.score(me));
            panel.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|row| {
                row.spawn(ui::text("Score", 14.0, palette::TEXT_DIM));
                row.spawn(ui::bold(format!("{}  (yours: {})", score.total(), mine.total()), 14.0, palette::ACCENT));
            });
            let truce = game.truce_left(me, p);
            if war {
                panel.spawn(ui::text(format!("At war for {} turns", game.war_length(me, p)), 13.5, palette::TEXT_DIM));
            } else if truce > 0 {
                panel.spawn(ui::text(format!("Peace treaty: no war for {truce} more turns"), 13.5, palette::TEXT_DIM));
            }
            if game.allied(me, p) {
                panel.spawn(ui::text("Allies: your borders are open to each other", 13.5, palette::FOOD));
            } else {
                for (owner, to, text) in [(p, me, "Their borders are open to you"), (me, p, "Your borders are open to them")] {
                    let left = game.borders_open_left(owner, to);
                    if left > 0 && !war {
                        panel.spawn(ui::text(format!("{text} ({left} turns left)"), 13.5, palette::TEXT));
                    }
                }
            }
            for pay in game.payments.iter().filter(|x| (x.from == me && x.to == p) || (x.from == p && x.to == me)) {
                let who = if pay.from == me { "You pay" } else { "They pay you" };
                panel.spawn(ui::text(format!("{who} {} gold per turn ({} turns left)", pay.amount, pay.turns_left), 13.5, palette::GOLD));
            }
            if !out {
                let label = if war { "Trade · Make Peace" } else { "Trade" };
                panel.spawn((
                    ui::base_button(percent(100), px(38)),
                    Tooltip::new(label, "Exchange gold, payments and cities. At war, a deal brings peace."),
                    children![(Text::new(label), ui::font_bold(16.0), TextColor(palette::ACCENT), Pickable::IGNORE)],
                    observe(move |_: On<Activate>, mut trade: ResMut<crate::trade::TradeScreen>, session: Res<Session>| {
                        trade.open(&session.game, session.me, p);
                    }),
                ));
            }
            panel.spawn(ui::section(&civ.agenda.name));
            panel.spawn(ui::text(civ.agenda.description.clone(), 13.0, palette::TEXT));
            if out {
                panel.spawn(ui::hint("This civilization has lost every city."));
            } else if !war {
                let label = if menu.confirm { "Click again to declare war" } else { "Declare War" };
                let tip = if truce > 0 {
                    format!("A peace treaty forbids war for {truce} more turns.")
                } else {
                    let mut tip = format!("Go to war with {}. Your units can then attack theirs, and theirs yours.", civ.name);
                    tip.push_str(&guests_note(game, me, p));
                    tip
                };
                panel.spawn((
                    ui::base_button(percent(100), px(38)),
                    ButtonState { active: menu.confirm, disabled: truce > 0 },
                    Tooltip::new("Declare War", tip),
                    children![(Text::new(label), ui::font_bold(16.0), TextColor(palette::DANGER), Pickable::IGNORE)],
                    observe(
                        move |_: On<Activate>,
                              mut menu: ResMut<CivMenu>,
                              mut session: ResMut<Session>,
                              mut notice: ResMut<Notice>,
                              mut out: MessageWriter<GameEvent>| {
                            if session.game.truce_left(session.me, p) > 0 {
                                return;
                            }
                            if !menu.confirm {
                                menu.confirm = true;
                                return;
                            }
                            menu.confirm = false;
                            if let Err(e) = session.command(Command::DeclareWar { on: p }, &mut out) {
                                notice.show(format!("Can't declare war: {e}"));
                            }
                        },
                    ),
                ));
            }
        });
}

/// "Your N units inside their borders will be moved out of them." on a new
/// line (empty if there are none).
fn guests_note(game: &sim::game::Game, me: PlayerId, them: PlayerId) -> String {
    match game.guests(me, them) {
        0 => String::new(),
        1 => "\nYour unit inside their borders will be moved out of them.".into(),
        n => format!("\nYour {n} units inside their borders will be moved out of them."),
    }
}

/// "Attacking this unit declares war": the confirmation for an attack on a
/// civ we're at peace with.
fn refresh_prompt(
    mut commands: Commands,
    session: Res<Session>,
    icons: Res<Icons>,
    prompt: Res<WarPrompt>,
    roots: Query<Entity, With<PromptRoot>>,
) {
    if !prompt.is_changed() {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(pending) = &prompt.0 else { return };
    let game = &session.game;
    let civ = game.civ(pending.on);
    let what = match game.city_at(pending.tile) {
        Some(c) => c.name.clone(),
        None => game.units_at(pending.tile).find(|u| u.owner == pending.on).map_or("them".into(), |u| format!("this {}", game.unit_def(u).name)),
    };
    commands
        .spawn((
            PromptRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            Pickable::default(),
            GlobalZIndex(20),
        ))
        .with_children(|root| {
            root.spawn(ui::panel(px(400))).with_children(|panel| {
                panel.spawn(crate::frontend::civ_badge(&icons, Some(civ), 56.0));
                panel.spawn(ui::title("Declare War?"));
                panel.spawn(ui::text(
                    format!("Attacking {what} declares war on {} ({}). You are at peace with them now.", civ.name, civ.leader),
                    15.0,
                    palette::TEXT,
                ));
                // Units in their land are sent home when war breaks out, so
                // they can't strike from inside.
                let inside = pending.units.iter().any(|u| game.units.get(u).is_some_and(|u| game.territory_owner(u.tile) == Some(pending.on)));
                if inside {
                    panel.spawn(ui::text(
                        "Your units inside their borders will be moved out of them first, so this attack can't be made from where they stand.",
                        14.0,
                        palette::DANGER,
                    ));
                } else if game.guests(session.me, pending.on) > 0 {
                    panel.spawn(ui::text(guests_note(game, session.me, pending.on).trim().to_string(), 14.0, palette::TEXT_DIM));
                }
                panel.spawn(Node { column_gap: px(10), width: percent(100), margin: UiRect::top(px(6)), ..default() }).with_children(|row| {
                    row.spawn((
                        ui::base_button(percent(50), px(40)),
                        children![(Text::new("Declare War"), ui::font_bold(16.0), TextColor(palette::DANGER), Pickable::IGNORE)],
                        observe(confirm_attack),
                    ));
                    row.spawn((
                        ui::base_button(percent(50), px(40)),
                        children![(Text::new("Cancel"), ui::font(16.0), TextColor(palette::TEXT), Pickable::IGNORE)],
                        observe(|_: On<Activate>, mut prompt: ResMut<WarPrompt>| prompt.0 = None),
                    ));
                });
            });
        });
}

/// Declares war and makes the attack that was waiting for it.
fn confirm_attack(
    _: On<Activate>,
    mut prompt: ResMut<WarPrompt>,
    mut session: ResMut<Session>,
    mut notice: ResMut<Notice>,
    mut out: MessageWriter<GameEvent>,
) {
    let Some(pending) = prompt.0.take() else { return };
    let game = &session.game;
    let inside = pending.units.iter().any(|u| game.units.get(u).is_some_and(|u| game.territory_owner(u.tile) == Some(pending.on)));
    if let Err(e) = session.command(Command::DeclareWar { on: pending.on }, &mut out) {
        notice.show(format!("Can't declare war: {e}"));
        return;
    }
    // Sent home by the war: the attack waits for another turn.
    if inside {
        return;
    }
    if let Err(e) = crate::play::order_units(&mut session, &pending.units, pending.tile, &mut out) {
        notice.show(e);
    }
}
