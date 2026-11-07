//! Top bar: empire totals on the left (gold, culture, cities, citizens), the
//! turn, a controls reference and the game menu on the right. (The other
//! civilizations below it are in `diplomacy.rs`.)

use crate::icons::{Icon, Icons};
use crate::loading::{AppState, InGameSet};
use crate::menu::Menu;
use crate::play::Session;
use crate::ui::{self, palette, Tooltip};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};

pub const HEIGHT: f32 = 40.0;

/// Which value a top-bar text shows.
#[derive(Component, Clone, Copy)]
enum Field {
    Science,
    Gold,
    Culture,
    /// Merchants out of the trade capacity.
    Merchants,
    /// Soldiers and ships.
    Armies,
    Cities,
    Citizens,
    Turn,
    /// The time of day and the season.
    Clock,
}

/// The sun or moon beside the clock.
#[derive(Component)]
struct ClockIcon;

pub struct TopBarPlugin;

impl Plugin for TopBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(Update, (refresh, tick_clock).after(crate::play::PlaySet).in_set(InGameSet));
    }
}

const CONTROLS: &str = "Left-click: select a unit or city\n\
Ctrl-click, Shift-drag, double-click: select several units\n\
Right-click: move or attack (a group: take, defend, hunt)\n\
O: Forces screen (every group and its orders)\n\
Drag / WASD: pan    Wheel / numpad +-: zoom\n\
Tab or . : next unit waiting for orders\n\
Enter: end the turn\n\
E explore · F fortify · Z sleep · Space skip\n\
B found city · 1–7 build · R road · U upgrade\n\
T: technology tree\n\
L: notifications    M: minimap\n\
=: strategic view    Y: yields    Shift+G: grid\n\
F3: debug info    Esc: menu";

fn stat(icons: &Icons, which: Icon, field: Field, color: Color, tip: Tooltip) -> impl Bundle {
    (
        Node { align_items: AlignItems::Center, column_gap: px(6), padding: UiRect::horizontal(px(4)), ..default() },
        // Hover area for the tooltip (the children ignore picking).
        Pickable::default(),
        tip,
        children![ui::icon(icons.get(which), 24.0), (field, ui::bold("", 16.0, color), Pickable::IGNORE)],
    )
}

fn spawn(mut commands: Commands, icons: Res<Icons>, session: Res<Session>) {
    let civ = session.game.civ(session.me);
    let badge_tip = Tooltip::new(
        civ.name.clone(),
        format!("{}, {}\n\n{}: {}\n{}: {}", civ.leader, civ.leader_title, civ.ability.name, civ.ability.description, civ.leader_ability.name, civ.leader_ability.description),
    );
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(0),
                left: px(0),
                width: percent(100),
                height: px(HEIGHT),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(px(14)),
                border: UiRect::bottom(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.06, 0.05, 0.92)),
            BorderColor::all(palette::PANEL_BORDER),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.45), px(0), px(3), px(0), px(10)),
            GlobalZIndex(2),
        ))
        .with_children(|bar| {
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: px(22), ..default() }).with_children(|left| {
                // Who you are: the civ's shield, its perks on hover.
                left.spawn((crate::frontend::civ_badge(&icons, Some(civ), 32.0), badge_tip));
                left.spawn(stat(
                    &icons,
                    Icon::Science,
                    Field::Science,
                    palette::SCIENCE,
                    Tooltip::new("Science", "Science per turn from all cities. Science researches technologies (T)."),
                ));
                left.spawn(stat(
                    &icons,
                    Icon::Gold,
                    Field::Gold,
                    palette::GOLD,
                    Tooltip::new(
                        "Gold",
                        "Treasury and income per turn. Cities with nothing to build turn their production into gold.\nSpend it in a city: buy units and buildings outright, or tiles to grow its borders.",
                    ),
                ));
                left.spawn(stat(
                    &icons,
                    Icon::Culture,
                    Field::Culture,
                    palette::CULTURE,
                    Tooltip::new("Culture", "Culture per turn from all cities. Culture makes city borders grow."),
                ));
                left.spawn((
                    stat(
                        &icons,
                        Icon::Trade,
                        Field::Merchants,
                        palette::TEXT,
                        Tooltip::new(
                            "Trade Routes",
                            "Merchants you have, out of how many your cities support (one each, two with a Market). Click to see every route and what it earns.",
                        ),
                    ),
                    bevy::ui_widgets::Button,
                    observe(|_: On<Activate>, mut screen: ResMut<crate::merchants::MerchantScreen>| {
                        if screen.open {
                            *screen = crate::merchants::MerchantScreen::default();
                        } else {
                            screen.overview();
                        }
                    }),
                ));
                left.spawn((
                    stat(
                        &icons,
                        Icon::Army,
                        Field::Armies,
                        palette::TEXT,
                        Tooltip::new(
                            "Forces",
                            "Your soldiers and ships. Click (or press O) for the Forces screen: what each group is doing (taking a city, defending one, on the move) and who's waiting for orders.\nSelect several units with Ctrl-click, Shift-drag or a double-click, then right-click: an enemy city to take it, one of yours to defend it.",
                        ),
                    ),
                    bevy::ui_widgets::Button,
                    observe(|_: On<Activate>, mut open: ResMut<crate::forces::ForcesOpen>| open.0 = !open.0),
                ));
                left.spawn(stat(&icons, Icon::FoundCity, Field::Cities, palette::TEXT, Tooltip::title("Cities")));
                left.spawn(stat(
                    &icons,
                    Icon::Population,
                    Field::Citizens,
                    palette::TEXT,
                    Tooltip::new("Citizens", "Total population of your cities. Each citizen works one tile."),
                ));
            });
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: px(10), ..default() }).with_children(|right| {
                right.spawn((
                    Node { align_items: AlignItems::Center, column_gap: px(6), margin: UiRect::right(px(10)), ..default() },
                    Pickable::default(),
                    Tooltip::new(
                        "Time and season",
                        "Day and night pass in real time (Settings: Day and night).\nThe seasons advance with the turns, 10 turns each; in the south they are the other way round.",
                    ),
                    children![
                        (ClockIcon, ui::icon(icons.get(Icon::Daytime), 22.0)),
                        (Field::Clock, ui::bold("", 15.0, palette::TEXT), Pickable::IGNORE),
                    ],
                ));
                right.spawn((
                    Node { align_items: AlignItems::Center, column_gap: px(6), margin: UiRect::right(px(8)), ..default() },
                    children![
                        ui::icon(icons.get(Icon::Turns), 22.0),
                        (Field::Turn, ui::heading("", 17.0, palette::ACCENT)),
                    ],
                ));
                crate::news::log_button(right, &icons);
                right.spawn((ui::small_button("?"), Tooltip::new("Controls", CONTROLS)));
                right.spawn((
                    ui::icon_button(icons.get(Icon::Menu), 30.0, Tooltip::new("Menu", "Settings and quit (Esc)")),
                    observe(|_: On<Activate>, mut next: ResMut<NextState<Menu>>| next.set(Menu::Pause)),
                ));
            });
        });
}

fn refresh(session: Res<Session>, mut fields: Query<(&Field, &mut Text)>) {
    if !session.is_changed() {
        return;
    }
    let game = &session.game;
    let me = session.me;
    let mine: Vec<_> = game.cities.values().filter(|c| c.owner == me).collect();
    let (mut gold, mut culture) = (0, 0);
    for c in &mine {
        let out = game.city_output(c.id);
        gold += out.yields.gold;
        culture += out.culture;
    }
    for (field, mut text) in &mut fields {
        let value = match field {
            Field::Science => format!("{:+}", game.science_per_turn(me)),
            Field::Gold => format!("{} ({:+})", game.players[me as usize].gold, gold),
            Field::Culture => format!("{culture:+}"),
            Field::Cities => mine.len().to_string(),
            Field::Merchants => format!("{}/{}", game.merchants(me).count(), game.trade_capacity(me)),
            Field::Armies => game.military_units(me).to_string(),
            Field::Citizens => mine.iter().map(|c| c.population).sum::<i32>().to_string(),
            Field::Turn => match game.options.turn_limit {
                Some(limit) => format!("Turn {} / {limit}", game.turn),
                None => format!("Turn {}", game.turn),
            },
            Field::Clock => continue,
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}

/// The clock: time of day (real time) and season (by turn).
fn tick_clock(
    clock: Res<crate::daylight::DayClock>,
    season: Res<crate::season::Season>,
    icons: Res<Icons>,
    mut fields: Query<(&Field, &mut Text)>,
    mut icon: Query<&mut ImageNode, With<ClockIcon>>,
) {
    let (h, m) = crate::daylight::clock(clock.t);
    let value = format!("{h:02}:{m:02} · {}, year {}", season.name(), season.year());
    for (field, mut text) in &mut fields {
        if matches!(field, Field::Clock) && text.0 != value {
            text.0 = value.clone();
        }
    }
    let which = if crate::daylight::night(clock.t) > 0.5 { Icon::Night } else { Icon::Daytime };
    let image = icons.get(which);
    for mut node in &mut icon {
        if node.image != image {
            node.image = image.clone();
        }
    }
}
