//! The end of the game: when someone wins, or when we're eliminated, a screen
//! with the outcome and every civ's score. After a victory you can keep
//! playing; once eliminated, only the main menu is left.

use crate::icons::{Icon, Icons};
use crate::loading::{AppState, InGameSet};
use crate::play::{GameEvent, Session};
use crate::ui::{self, palette};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use sim::game::{Event, Game, PlayerId};
use sim::victory::{Victory, VictoryKind};

/// Whether the game-over screen is showing.
#[derive(Resource, Default)]
pub struct GameOverScreen {
    pub open: bool,
}

#[derive(Component)]
struct GameOverRoot;

pub struct GameOverPlugin;

impl Plugin for GameOverPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameOverScreen>().add_systems(
            Update,
            (reset, open_on_events, refresh).chain().after(crate::play::PlaySet).in_set(InGameSet),
        );
    }
}

/// A new game starts with the screen closed. Dev aid: FOURX_GAMEOVER=win|lose
/// pretends we won by domination, or that someone else won on score.
fn reset(mut session: ResMut<Session>, mut screen: ResMut<GameOverScreen>) {
    if !session.is_added() {
        return;
    }
    screen.open = false;
    let me = session.me;
    let turn = session.game.turn;
    let fake = match std::env::var("FOURX_GAMEOVER").as_deref() {
        Ok("win") => Some(Victory { winner: me, kind: VictoryKind::Domination, turn }),
        Ok("lose") => Some(Victory { winner: (me + 1) % session.game.players.len() as PlayerId, kind: VictoryKind::Score, turn }),
        _ => None,
    };
    if fake.is_some() {
        session.game.victory = fake;
        screen.open = true;
    }
}

fn open_on_events(session: Res<Session>, mut events: MessageReader<GameEvent>, mut screen: ResMut<GameOverScreen>) {
    let me = session.me;
    for GameEvent(e) in events.read() {
        if matches!(*e, Event::GameOver(_)) || matches!(*e, Event::PlayerEliminated { player } if player == me) {
            screen.open = true;
        }
    }
}

/// The title and the sentence under it.
fn headline(game: &Game, me: PlayerId) -> (&'static str, String) {
    let name = |p: PlayerId| game.civ(p).name.clone();
    let out = game.players[me as usize].eliminated;
    match game.victory {
        Some(v) if v.winner == me => match v.kind {
            VictoryKind::Domination if game.alive().count() == 1 => ("Victory", "Every other civilization has fallen. The world is yours.".into()),
            VictoryKind::Domination => ("Victory", "You hold every capital in the world. No one can stand against you.".into()),
            VictoryKind::Score => ("Victory", format!("Time is up, and {} has the highest score in the world.", name(me))),
        },
        Some(v) if !out || v.turn == game.turn => match v.kind {
            VictoryKind::Domination => ("Defeat", format!("{} ({}) has conquered the world.", name(v.winner), game.civ(v.winner).leader)),
            VictoryKind::Score => ("Defeat", format!("Time is up, and {} has the highest score in the world.", name(v.winner))),
        },
        _ => ("Defeat", format!("{} has lost its last city. Your civilization is no more.", name(me))),
    }
}

fn refresh(
    mut commands: Commands,
    session: Res<Session>,
    screen: Res<GameOverScreen>,
    icons: Res<Icons>,
    roots: Query<Entity, With<GameOverRoot>>,
) {
    if !screen.is_changed() {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    if !screen.open {
        return;
    }
    let game = &session.game;
    let me = session.me;
    let (title, line) = headline(game, me);
    let won = title == "Victory";
    let mut ranking: Vec<PlayerId> = (0..game.players.len() as PlayerId).collect();
    ranking.sort_by_key(|&p| (game.players[p as usize].eliminated, std::cmp::Reverse(game.score(p).total()), p));
    commands
        .spawn((
            GameOverRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            Pickable::default(),
            GlobalZIndex(15),
        ))
        .with_children(|root| {
            root.spawn(ui::panel(px(640))).with_children(|panel| {
                let crest = game.victory.map_or(me, |v| v.winner);
                panel.spawn(Node { align_items: AlignItems::Center, column_gap: px(18), ..default() }).with_children(|head| {
                    let tint = if won { palette::GOLD } else { palette::TEXT_DIM };
                    let laurel = || (Node { width: px(44), height: px(44), ..default() }, ImageNode { color: tint, ..ImageNode::new(icons.white(Icon::Laurel)) });
                    head.spawn(laurel());
                    head.spawn(crate::frontend::civ_badge(&icons, Some(game.civ(crest)), 76.0));
                    head.spawn((laurel(), UiTransform::from_scale(Vec2::new(-1.0, 1.0))));
                });
                panel.spawn((
                    ui::heading(title.to_uppercase(), 46.0, if won { palette::ACCENT } else { palette::DANGER }),
                    TextShadow::default(),
                ));
                panel.spawn((ui::text(line, 17.0, palette::TEXT), TextLayout::justify(Justify::Center)));
                let detail = match game.victory {
                    Some(v) => format!("{} victory, turn {}", v.kind.name(), v.turn),
                    None => format!("Turn {}", game.turn),
                };
                panel.spawn(ui::text(detail, 14.0, palette::TEXT_DIM));
                panel.spawn((Node { width: percent(100), flex_direction: FlexDirection::Column, ..default() }, children![ui::section("Scores")]));
                score_row(panel, None, &icons, game, me);
                for &p in &ranking {
                    score_row(panel, Some(p), &icons, game, me);
                }
                panel.spawn(Node { column_gap: px(10), width: percent(100), margin: UiRect::top(px(12)), ..default() }).with_children(|row| {
                    let alive = !game.players[me as usize].eliminated;
                    if alive {
                        row.spawn((
                            ui::base_button(percent(50), px(42)),
                            children![(Text::new("Keep Playing"), ui::font(18.0), TextColor(palette::TEXT), Pickable::IGNORE)],
                            observe(|_: On<Activate>, mut screen: ResMut<GameOverScreen>| screen.open = false),
                        ));
                    }
                    row.spawn((
                        ui::base_button(percent(if alive { 50 } else { 100 }), px(42)),
                        children![(Text::new("Main Menu"), ui::font(18.0), TextColor(palette::TEXT), Pickable::IGNORE)],
                        observe(|_: On<Activate>, mut next: ResMut<NextState<AppState>>| next.set(AppState::MainMenu)),
                    ));
                });
            });
        });
}

/// Column widths: civ, cities, citizens, techs, land, score.
const COLUMNS: [f32; 6] = [230.0, 64.0, 72.0, 60.0, 56.0, 72.0];

/// One civ's line in the score table, or the header for `None`.
fn score_row(panel: &mut ChildSpawnerCommands, player: Option<PlayerId>, icons: &Icons, game: &Game, me: PlayerId) {
    let you = player == Some(me);
    let out = player.is_some_and(|p| game.players[p as usize].eliminated);
    let won = player.is_some_and(|p| game.victory.is_some_and(|v| v.winner == p));
    let fill = if won { palette::BUTTON_ACTIVE } else if you { palette::PANEL_LIGHT } else { Color::NONE };
    let cell = |i: usize, width: f32| Node {
        width: px(width),
        align_items: AlignItems::Center,
        justify_content: if i == 0 { JustifyContent::Start } else { JustifyContent::End },
        column_gap: px(10),
        ..default()
    };
    let row = (
        Node {
            width: percent(100),
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(8), px(4)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(fill),
        Pickable::IGNORE,
    );
    panel.spawn(row).with_children(|row| {
        let Some(p) = player else {
            for (i, h) in ["Civilization", "Cities", "Citizens", "Techs", "Land", "Score"].into_iter().enumerate() {
                row.spawn(cell(i, COLUMNS[i])).with_children(|c| {
                    c.spawn(ui::bold(h, 13.0, palette::TEXT_DIM));
                });
            }
            return;
        };
        let civ = game.civ(p);
        let color = if out { palette::TEXT_DIM } else if you { palette::ACCENT } else { palette::TEXT };
        row.spawn(cell(0, COLUMNS[0])).with_children(|c| {
            c.spawn(crate::frontend::civ_badge(icons, Some(civ), 28.0));
            c.spawn(ui::bold(if you { format!("{} (you)", civ.name) } else { civ.name.clone() }, 15.0, color));
        });
        if out {
            // The note spans the stat columns.
            row.spawn(cell(1, COLUMNS[1..].iter().sum())).with_children(|c| {
                c.spawn(ui::text("Eliminated", 14.0, palette::DANGER));
            });
            return;
        }
        let s = game.score(p);
        for (i, v) in [s.cities, s.population, s.techs, s.land].into_iter().enumerate() {
            row.spawn(cell(i + 1, COLUMNS[i + 1])).with_children(|c| {
                c.spawn(ui::text(v.to_string(), 15.0, color));
            });
        }
        row.spawn(cell(5, COLUMNS[5])).with_children(|c| {
            c.spawn(ui::bold(s.total().to_string(), 16.0, palette::ACCENT));
        });
    });
}
