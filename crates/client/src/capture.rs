//! "Keep or raze?": after we take a city, a prompt asks whether to keep it
//! or burn it to the ground. Keeping needs no command; razing is
//! `Command::RazeCity`, which works only on the turn the city fell, so the
//! prompt closes by itself once that turn is over.

use crate::icons::Icons;
use crate::loading::InGameSet;
use crate::play::{GameEvent, Notice, SelectedCity, Session};
use crate::ui::{self, palette, ButtonState};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use sim::city::CityId;
use sim::game::{Command, Event};

/// The city we just took, waiting for the player's choice.
#[derive(Resource, Default)]
pub struct RazePrompt(pub Option<CityId>);

#[derive(Component)]
struct PromptRoot;

pub struct CapturePlugin;

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RazePrompt>()
            .add_systems(Update, (watch, refresh_prompt).chain().after(crate::play::PlaySet).in_set(InGameSet));
    }
}

/// Opens the prompt when one of our attacks takes a city, and closes it when
/// the choice is no longer ours (the turn ended, the city changed hands).
fn watch(session: Res<Session>, mut events: MessageReader<GameEvent>, mut prompt: ResMut<RazePrompt>) {
    if session.is_added() {
        prompt.0 = None;
    }
    let game = &session.game;
    for GameEvent(e) in events.read() {
        if let Event::CityCaptured { city, .. } = e {
            if game.cities.get(city).is_some_and(|c| c.owner == session.me) {
                prompt.0 = Some(*city);
            }
        }
    }
    let open = prompt.0.and_then(|c| game.cities.get(&c)).filter(|c| c.owner == session.me && c.captured.is_some_and(|(turn, _)| turn == game.turn));
    if prompt.0.is_some() && open.is_none() {
        prompt.0 = None;
    }
}

fn refresh_prompt(mut commands: Commands, session: Res<Session>, icons: Res<Icons>, prompt: Res<RazePrompt>, roots: Query<Entity, With<PromptRoot>>) {
    if !prompt.is_changed() {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    let game = &session.game;
    let Some(c) = prompt.0.and_then(|c| game.cities.get(&c)) else { return };
    let from = c.captured.map_or(c.owner, |(_, from)| from);
    let civ = game.civ(from);
    let problem = game.raze_problem(c.id);
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
            root.spawn(ui::panel(px(420))).with_children(|panel| {
                panel.spawn(crate::frontend::civ_badge(&icons, Some(civ), 56.0));
                panel.spawn(ui::title(&format!("{} Has Fallen", c.name)));
                panel.spawn(ui::text(
                    format!(
                        "You took {} (size {}) from {}. Keep it as one of your cities, or raze it to the ground: the city is gone for good and its land belongs to nobody.",
                        c.name, c.population, civ.name
                    ),
                    15.0,
                    palette::TEXT,
                ));
                if let Some(why) = problem {
                    panel.spawn(ui::text(format!("It can't be razed: {why}."), 14.0, palette::TEXT_DIM));
                }
                panel.spawn(Node { column_gap: px(10), width: percent(100), margin: UiRect::top(px(6)), ..default() }).with_children(|row| {
                    row.spawn((
                        ui::base_button(percent(50), px(40)),
                        children![(Text::new("Keep City"), ui::font_bold(16.0), TextColor(palette::ACCENT), Pickable::IGNORE)],
                        observe(|_: On<Activate>, mut prompt: ResMut<RazePrompt>| prompt.0 = None),
                    ));
                    let label = (Text::new("Raze City"), ui::font_bold(16.0), TextColor(palette::DANGER), Pickable::IGNORE);
                    if problem.is_some() {
                        row.spawn((ui::base_button(percent(50), px(40)), ButtonState { active: false, disabled: true }, children![label]));
                    } else {
                        row.spawn((ui::base_button(percent(50), px(40)), children![label], observe(raze)));
                    }
                });
            });
        });
}

/// Razes the city the prompt is about.
fn raze(
    _: On<Activate>,
    mut prompt: ResMut<RazePrompt>,
    mut session: ResMut<Session>,
    mut selected: ResMut<SelectedCity>,
    mut notice: ResMut<Notice>,
    mut out: MessageWriter<GameEvent>,
) {
    let Some(city) = prompt.0.take() else { return };
    if let Err(e) = session.command(Command::RazeCity { city }, &mut out) {
        notice.show(format!("Can't raze it: {e}"));
    } else if selected.0 == Some(city) {
        selected.0 = None;
    }
}
