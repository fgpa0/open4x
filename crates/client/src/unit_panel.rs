//! Selected unit card (bottom left: portrait, stats, order buttons), the turn
//! button (bottom right: what still needs attention, or End Turn).

use crate::icons::{Icon, Icons};
use crate::loading::InGameSet;
use crate::play::{BuildRequest, EndTurnRequest, FoundCityRequest, GameEvent, NextAttention, OrderRequest, Selected, SelectedCity, Session, UpgradeRequest};
use crate::ui::{self, palette, ButtonState, Tooltip};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use sim::city::CityId;
use sim::game::{Command, Order};
use sim::rules::{Ability, UnitClass};
use std::collections::HashSet;

#[derive(Component)]
struct UnitPanel;

#[derive(Component)]
struct TurnWidget;

/// Cities whose panel the player has opened this turn: the turn button stops
/// asking about them (if nothing is chosen, the governor decides at turn end).
#[derive(Resource, Default)]
struct Reviewed {
    turn: u32,
    cities: HashSet<CityId>,
    /// The tech tree has been opened this turn.
    research: bool,
}

/// The city [`next_attention`] opened because it had nothing to build: once
/// the player picks something there, the panel closes and play moves on.
#[derive(Resource, Default)]
struct Prompted(Option<CityId>);

pub struct UnitPanelPlugin;

impl Plugin for UnitPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Reviewed>()
            .init_resource::<Prompted>()
            .add_systems(
                Update,
                (track_reviewed, next_attention, rebuild_unit_panel, rebuild_turn_widget)
                    .chain()
                    .after(crate::play::PlaySet)
                    .in_set(InGameSet),
            );
    }
}

fn track_reviewed(
    session: Res<Session>,
    city: Res<SelectedCity>,
    tree: Res<crate::research_ui::TechTreeOpen>,
    mut reviewed: ResMut<Reviewed>,
) {
    if reviewed.turn != session.game.turn {
        reviewed.turn = session.game.turn;
        reviewed.cities.clear();
        reviewed.research = false;
    }
    if tree.0 && !reviewed.research {
        reviewed.research = true;
    }
    if let Some(id) = city.0.filter(|id| !reviewed.cities.contains(id)) {
        reviewed.cities.insert(id);
    }
}

/// Goes to whatever needs the player next (the turn button's order: units,
/// research, cities with nothing to build). With nothing left it clears the
/// selection; ending the turn stays the player's call.
fn next_attention(
    mut requests: MessageReader<NextAttention>,
    session: Res<Session>,
    reviewed: Res<Reviewed>,
    mut prompted: ResMut<Prompted>,
    mut unit: ResMut<Selected>,
    mut city: ResMut<SelectedCity>,
    mut tree: ResMut<crate::research_ui::TechTreeOpen>,
    mut rig: ResMut<crate::camera::CameraRig>,
) {
    // A prompted city that now has a build is done with.
    if let Some(id) = prompted.0 {
        if city.0 != Some(id) {
            prompted.0 = None;
        } else if session.game.cities.get(&id).is_none_or(|c| !c.queue.is_empty()) {
            prompted.0 = None;
            city.0 = None;
            return go_to_next(&session, &reviewed, &mut prompted, &mut unit, &mut city, &mut tree, &mut rig);
        }
    }
    if requests.read().count() > 0 {
        go_to_next(&session, &reviewed, &mut prompted, &mut unit, &mut city, &mut tree, &mut rig);
    }
}

fn go_to_next(
    session: &Session,
    reviewed: &Reviewed,
    prompted: &mut Prompted,
    unit: &mut Selected,
    city: &mut SelectedCity,
    tree: &mut crate::research_ui::TechTreeOpen,
    rig: &mut crate::camera::CameraRig,
) {
    // Don't pull the player away from a panel they're using.
    if tree.0 || city.0.is_some() {
        return;
    }
    match turn_state(session, reviewed) {
        TurnState::Units(..) => {
            crate::play::select_next_unit(session, unit, rig);
        }
        TurnState::Research => {
            unit.0 = None;
            tree.0 = true;
        }
        TurnState::Production(id, _) => {
            unit.0 = None;
            city.0 = Some(id);
            prompted.0 = Some(id);
            rig.focus = crate::layout::tile_center(&session.game.map.grid, session.game.cities[&id].tile);
        }
        TurnState::Ready => unit.0 = None,
    }
}

pub fn player_color(session: &Session, player: sim::game::PlayerId) -> Color {
    let [r, g, b] = session.game.players[player as usize].color;
    Color::srgb_u8(r, g, b)
}

fn order_status(game: &sim::game::Game, u: &sim::game::Unit) -> (Icon, String) {
    let building = |w| {
        let left = game.work_left(u.owner, u.tile, w);
        let turns = if left == 1 { "1 turn".to_string() } else { format!("{left} turns") };
        format!("Building a {}: {turns} left", crate::improvements::work_name(game, w))
    };
    if game.is_alerted(u) {
        let was = if u.order == Order::Fortify { "Fortified" } else { "Asleep" };
        return (Icon::Warning, format!("{was}, but enemies are near"));
    }
    match u.order {
        Order::None => (Icon::Warning, "Awaiting orders".into()),
        Order::Goto(_) => (Icon::Moves, "Moving to destination".into()),
        Order::Hunt { target, .. } => (
            Icon::Strength,
            game.units.get(&target).map_or("Hunting".into(), |t| format!("Hunting the enemy {}", game.unit_def(t).name)),
        ),
        Order::Explore => (Icon::Explore, "Exploring".into()),
        Order::Take { city } => (Icon::Strength, game.cities.get(&city).map_or("Taking a city".into(), |c| format!("Taking {}", c.name))),
        Order::Defend { city } => (Icon::Defense, game.cities.get(&city).map_or("Defending a city".into(), |c| format!("Defending {}", c.name))),
        Order::Fortify => (Icon::Defense, "Fortified".into()),
        Order::Sleep => (Icon::Sleep, "Sleeping".into()),
        Order::Skip => (Icon::Skip, "Waiting until next turn".into()),
        Order::Automate => (Icon::Auto, "Improving the land on its own".into()),
        Order::Build(w) => (crate::improvements::work_icon(game, w), building(w)),
        Order::AutoBuild(w) => (crate::improvements::work_icon(game, w), format!("{} (automated)", building(w))),
        Order::Trade(r) => {
            let name = |c| game.cities.get(&c).map_or("?".to_string(), |c| c.name.clone());
            let mut text = if r.back {
                format!("Trading with {}: heading home to {}", name(r.to), name(r.home))
            } else {
                format!("Trading from {}: on the way to {}", name(r.home), name(r.to))
            };
            if !r.repeat {
                text.push_str(" (then the best route)");
            }
            (Icon::Trade, text)
        }
    }
}

/// An action button (its hotkey badge is added as a child after spawning).
fn action(icons: &Icons, which: Icon, tip: Tooltip, active: bool, enabled: bool) -> impl Bundle {
    ui::icon_button_with(icons.get(which), 50.0, tip, ButtonState { active, disabled: !enabled })
}

fn rebuild_unit_panel(
    mut commands: Commands,
    session: Res<Session>,
    selected: Res<Selected>,
    group: Res<crate::play::Group>,
    icons: Res<Icons>,
    old: Query<Entity, With<UnitPanel>>,
) {
    if !session.is_changed() && !selected.is_changed() && !group.is_changed() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    // A group has its own panel (see `forces.rs`).
    if group.0.len() > 1 {
        return;
    }
    let game = &session.game;
    let Some(u) = selected.0.and_then(|id| game.units.get(&id)).filter(|u| u.owner == session.me) else { return };
    let def = game.unit_def(u);
    let color = player_color(&session, u.owner);
    let (status_icon, status) = order_status(game, u);
    let military = def.class == UnitClass::Military;

    commands
        .spawn((
            UnitPanel,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(16),
                left: px(16),
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                padding: UiRect::new(px(14), px(18), px(14), px(16)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            ui::frame(),
        ))
        .with_children(|panel| {
            // Portrait and details.
            panel.spawn(Node { column_gap: px(14), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                row.spawn((
                    Node {
                        width: px(76),
                        height: px(76),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.16, 0.14, 0.12)),
                    BorderColor::all(color),
                    BoxShadow::new(color.with_alpha(0.35), px(0), px(0), px(0), px(10)),
                    children![ui::icon(icons.get(Icon::unit(&def.art)), 52.0)],
                ));
                row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }).with_children(|col| {
                    col.spawn(ui::heading(def.name.clone(), 22.0, palette::ACCENT));
                    col.spawn(Node { column_gap: px(16), ..default() }).with_children(|stats| {
                        let moves_color = if u.moves > 0 { palette::TEXT } else { palette::TEXT_DIM };
                        stats.spawn((
                            ui::stat(&icons, Icon::Moves, format!("{}/{}", sim::path::show_moves(u.moves), sim::path::show_moves(game.turn_moves(u))), 15.0, moves_color),
                            Tooltip::new(
                                "Movement",
                                "Movement points left this turn. Open land costs 1; forests, hills and rivers cost more. Military units that move next to an enemy military unit or city stop there for the turn (zone of control).",
                            ),
                        ));
                        if military {
                            let hp_color = if u.hp < 50 { palette::DANGER } else if u.hp < 100 { palette::ACCENT } else { palette::TEXT };
                            stats.spawn((
                                ui::stat(&icons, Icon::Health, u.hp.to_string(), 15.0, hp_color),
                                Tooltip::new(
                                    "Health",
                                    "Hit points out of 100. Wounded units fight weaker. Units heal when they neither move nor fight for a turn: 10, 15 in your territory, 20 in a city.",
                                ),
                            ));
                            stats.spawn((
                                ui::stat(&icons, Icon::Strength, def.strength.to_string(), 15.0, palette::TEXT),
                                Tooltip::new("Combat strength", "Strength in melee attacks and when defending. Right-click an enemy next to the unit to attack it."),
                            ));
                            let (rank, tip) = crate::forces::xp_stat(game, u);
                            stats.spawn((
                                ui::stat(&icons, Icon::Rank, rank, 15.0, if u.xp > 0 { palette::ACCENT } else { palette::TEXT }),
                                Tooltip::new("Experience", tip),
                            ));
                            if def.ranged > 0 {
                                stats.spawn((
                                    ui::stat(&icons, Icon::Archer, format!("{} ({})", def.ranged, def.range), 15.0, palette::TEXT),
                                    Tooltip::new(
                                        "Ranged strength",
                                        format!("Shoots enemies up to {} tiles away without taking damage. Right-click a target to fire.", def.range),
                                    ),
                                ));
                            }
                        }
                        stats.spawn((
                            ui::stat(&icons, Icon::Sight, game.unit_sight(u).to_string(), 15.0, palette::TEXT),
                            Tooltip::new("Sight", "How many tiles away the unit can see."),
                        ));
                    });
                    let status_color = if u.order == Order::None { palette::ACCENT } else { palette::TEXT_DIM };
                    col.spawn(Node { column_gap: px(6), align_items: AlignItems::Center, ..default() }).with_children(|s| {
                        s.spawn(ui::icon(icons.get(status_icon), 16.0));
                        s.spawn(ui::text(status, 13.5, status_color));
                        if game.is_embarked(u) {
                            s.spawn((
                                ui::bold("· Embarked", 13.5, palette::SCIENCE),
                                Pickable::default(),
                                Tooltip::new("Embarked", "At sea in boats: can't attack and defends at half strength. Landing takes the rest of the turn."),
                            ));
                        }
                    });
                });
            });

            // Orders.
            panel.spawn(Node { column_gap: px(10), ..default() }).with_children(|row| {
                if def.has(Ability::FoundCity) {
                    let problem = game.found_city_problem(u.owner, u.tile);
                    let body = match problem {
                        Some(p) => format!("Not possible here: {p}."),
                        None => "Settle a new city on this tile. The settler is used up.".into(),
                    };
                    row.spawn((
                        action(&icons, Icon::FoundCity, Tooltip::new("Found City", body), false, problem.is_none()),
                        observe(|_: On<Activate>, mut out: MessageWriter<FoundCityRequest>| {
                            out.write(FoundCityRequest);
                        }),
                    ))
                    .with_child(ui::hotkey_badge("B"));
                }
                if def.has(Ability::Trade) {
                    let body = match u.order {
                        Order::Trade(_) => "This merchant has a route. Pick another city for it to trade with: see what every route would earn, or move it to another of your cities to trade from there.",
                        _ => "Pick a city for this merchant to trade with, yours or a rival's at peace, and see what every route would earn. It then goes back and forth on its own. You can also move it to another of your cities at once, to trade from there.",
                    };
                    row.spawn((
                        action(&icons, Icon::Trade, Tooltip::new("Trade Route", body), matches!(u.order, Order::Trade(_)), true),
                        observe(move |_: On<Activate>, mut screen: ResMut<crate::merchants::MerchantScreen>, selected: Res<Selected>| {
                            if let Some(unit) = selected.0 {
                                screen.choose(unit);
                            }
                        }),
                    ))
                    .with_child(ui::hotkey_badge("E"));
                    {
                        let unit = u.id;
                        let others: Vec<_> = game.cities.values().filter(|c| c.owner == u.owner && c.tile != u.tile).collect();
                        let problems: Vec<_> = others.iter().map(|c| game.transfer_problem(unit, c.id)).collect();
                        let free = problems.iter().any(|p| p.is_none());
                        let what = "Move this merchant to another of your cities at once, to trade from there";
                        let body = match problems.iter().flatten().next() {
                            _ if free => format!("{what}: see the best route from each. Its route ends and so does its turn."),
                            None => format!("{what}. You have no other city yet."),
                            Some(why) => format!("{what}. Not now: {why}."),
                        };
                        row.spawn((
                            action(&icons, Icon::MoveToCity, Tooltip::new("Move to City", body), false, free),
                            observe(move |_: On<Activate>, mut screen: ResMut<crate::merchants::MerchantScreen>| {
                                screen.transfer(unit);
                            }),
                        ));
                    }
                    if let Order::Trade(route) = u.order {
                        let unit = u.id;
                        let body = if route.repeat {
                            "On: this merchant keeps trading with the same city. Turn it off to have it take whichever route pays best each time it's home."
                        } else {
                            "Off: each time this merchant is home from a trip it takes whichever route pays best then. Turn it on to keep trading with the same city."
                        };
                        row.spawn((
                            action(&icons, Icon::Repeat, Tooltip::new("Repeat Route", body), route.repeat, true),
                            observe(move |_: On<Activate>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>| {
                                let _ = session.command(Command::RepeatRoute { unit, repeat: !route.repeat }, &mut out);
                            }),
                        ));
                    }
                }
                let mut order = |icon, key, title: &str, body: &str, o: Order| {
                    row.spawn((
                        action(&icons, icon, Tooltip::new(title, body), u.order == o, true),
                        observe(move |_: On<Activate>, mut out: MessageWriter<OrderRequest>| {
                            out.write(OrderRequest(o));
                        }),
                    ))
                    .with_child(ui::hotkey_badge(key));
                };
                let worker = def.has(Ability::BuildImprovements);
                if worker {
                    order(
                        Icon::Auto,
                        "E",
                        "Automate",
                        "Improves your land on its own, every turn: farms, mines, the resources you need, then roads between your cities. Give it any other order to take over again.",
                        Order::Automate,
                    );
                } else if !def.has(Ability::Trade) {
                    order(Icon::Explore, "E", "Explore", "Move automatically to uncover the map, every turn, until there is nothing left nearby.", Order::Explore);
                }
                if military {
                    order(Icon::Defense, "F", "Fortify", "Dig in for a defence bonus. Stays put until you give new orders; asks again when an enemy comes within striking distance.", Order::Fortify);
                }
                order(Icon::Sleep, "Z", "Sleep", "Stays put and stops asking for orders until you select it again, or an enemy comes within striking distance.", Order::Sleep);
                order(Icon::Skip, "Space", "Skip Turn", "Do nothing this turn.", Order::Skip);
                // Upgrading, once there's something better to build.
                if let (Some(to), Some(price)) = (game.upgrade_target(u.owner, u.kind), game.upgrade_price(u.id)) {
                    let to = game.rules.unit(to);
                    let problem = game.upgrade_problem(u.id);
                    let mut body = format!(
                        "Refit as a {} for {price} gold: strength {} instead of {}{}. Keeps its wounds and takes the rest of the turn; only inside your borders.",
                        to.name,
                        to.strength.max(to.ranged),
                        def.strength.max(def.ranged),
                        if to.ranged > 0 && def.ranged == 0 { " (ranged)" } else { "" },
                    );
                    if let Some(p) = problem {
                        body += &format!("\n\nNot now: {p}.");
                    }
                    row.spawn((
                        action(&icons, Icon::Upgrade, Tooltip::new(format!("Upgrade to {}", to.name), body), false, problem.is_none()),
                        observe(|_: On<Activate>, mut out: MessageWriter<UpgradeRequest>| {
                            out.write(UpgradeRequest);
                        }),
                    ))
                    .with_child(ui::hotkey_badge("U"));
                }
            });
            // A member picked on its own (Alt-click) can leave its army, as
            // an army of its own.
            // A General's level and traits.
            if def.has(Ability::General) {
                crate::forces::spawn_general(panel, game, &icons, u.id, true);
            }
            // What a Worker (or a road-building unit) can build here.
            let slots = crate::play::build_slots(game, u.id);
            let road = crate::play::road_work(game, u.id);
            if !slots.is_empty() || road.is_some() {
                panel.spawn(Node { column_gap: px(8), ..default() }).with_children(|row| {
                    let current = match u.order {
                        Order::Build(w) | Order::AutoBuild(w) => Some(w),
                        _ => None,
                    };
                    let works = slots.iter().copied().enumerate().map(|(i, w)| (w, (i + 1).to_string()));
                    let works = works.chain(road.map(|w| (w, "R".to_string())));
                    for (w, key) in works {
                        row.spawn((
                            ui::icon_button_with(
                                icons.get(crate::improvements::work_icon(game, w)),
                                42.0,
                                crate::improvements::work_tooltip(game, u.owner, u.tile, w),
                                ButtonState { active: current == Some(w), disabled: u.moves <= 0 },
                            ),
                            observe(move |_: On<Activate>, mut out: MessageWriter<BuildRequest>| {
                                out.write(BuildRequest::Work(w));
                            }),
                        ))
                        .with_child(ui::hotkey_badge(&key));
                    }
                });
            } else if def.has(Ability::BuildImprovements) && game.city_at(u.tile).is_none() {
                let why = if game.territory_owner(u.tile) != Some(u.owner) {
                    "Workers improve land inside your borders (roads go anywhere unclaimed)."
                } else {
                    "Nothing more to build on this tile yet."
                };
                panel.spawn(ui::text(why, 12.5, palette::TEXT_DIM));
            }
            // Merchants go only where their route takes them.
            if !def.has(Ability::Trade) {
                let hint = if military { "Right-click a tile to move, or an enemy to attack" } else { "Right-click a tile to move" };
                panel.spawn(ui::text(hint, 12.5, palette::TEXT_DIM));
            }
        });
}

/// What the turn button does now.
enum TurnState {
    Units(usize, Icon),
    Research,
    Production(CityId, String),
    Ready,
}

fn turn_state(session: &Session, reviewed: &Reviewed) -> TurnState {
    let game = &session.game;
    let mut waiting = game.units_needing_orders(session.me);
    if let Some(first) = waiting.next() {
        let icon = Icon::unit(&game.unit_def(first).art);
        return TurnState::Units(1 + waiting.count(), icon);
    }
    let me = &game.players[session.me as usize];
    let researching = !me.research_path.is_empty() || me.techs.iter().all(|&k| k);
    if !researching && !reviewed.research && game.science_per_turn(session.me) > 0 {
        return TurnState::Research;
    }
    let mut idle: Vec<_> = game
        .cities
        .values()
        .filter(|c| c.owner == session.me && c.queue.is_empty() && !reviewed.cities.contains(&c.id))
        .collect();
    idle.sort_by_key(|c| c.id);
    if let Some(c) = idle.first() {
        return TurnState::Production(c.id, c.name.clone());
    }
    TurnState::Ready
}

fn rebuild_turn_widget(
    mut commands: Commands,
    session: Res<Session>,
    icons: Res<Icons>,
    reviewed: Res<Reviewed>,
    old: Query<Entity, With<TurnWidget>>,
    mut built: Local<bool>,
) {
    if !session.is_changed() && !reviewed.is_changed() && *built {
        return;
    }
    *built = true;
    for e in &old {
        commands.entity(e).despawn();
    }
    let (title, sub, icon, ready) = match turn_state(&session, &reviewed) {
        TurnState::Units(n, icon) => (
            "Unit Needs Orders".to_string(),
            if n == 1 { "1 unit is waiting".to_string() } else { format!("{n} units are waiting") },
            icon,
            false,
        ),
        TurnState::Research => ("Choose Research".into(), "Pick a technology to work towards".into(), Icon::Science, false),
        TurnState::Production(_, name) => ("Choose Production".into(), format!("{name} has nothing to build"), Icon::Production, false),
        TurnState::Ready => ("End Turn".into(), "Enter".into(), Icon::EndTurn, true),
    };
    let ring = if ready { palette::ACCENT } else { palette::PANEL_BORDER };
    commands
        .spawn((
            TurnWidget,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(16),
                right: px(16),
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|w| {
            // The caption, tucked behind the round button.
            w.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    padding: UiRect::new(px(16), px(54), px(8), px(8)),
                    margin: UiRect::right(px(-44)),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                ui::frame(),
                Pickable::IGNORE,
                children![
                    (ui::heading(title, 17.0, palette::ACCENT), Pickable::IGNORE),
                    (ui::text(format!("Turn {}  ·  {sub}", session.game.turn), 12.5, palette::TEXT_DIM), Pickable::IGNORE),
                ],
            ));
            w.spawn((
                bevy::ui_widgets::Button,
                bevy::picking::hover::Hovered::default(),
                Node {
                    width: px(92),
                    height: px(92),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(palette::BUTTON),
                BorderColor::all(ring),
                BoxShadow::new(
                    if ready { palette::ACCENT.with_alpha(0.55) } else { Color::srgba(0.0, 0.0, 0.0, 0.6) },
                    px(0),
                    px(0),
                    px(2),
                    px(14),
                ),
                Tooltip::new(
                    "Turn",
                    "Takes you to whatever still needs a decision: units waiting for orders, then research, then new cities with nothing to build. When nothing is left it ends the turn.\nCities left without a choice get one from their governor when the turn ends.\nEnter ends the turn at any time.",
                ),
                children![ui::icon(icons.get(icon), 54.0)],
                observe(
                    |_: On<Activate>,
                     session: Res<Session>,
                     reviewed: Res<Reviewed>,
                     mut next: MessageWriter<NextAttention>,
                     mut end: MessageWriter<EndTurnRequest>,
                     mut city: ResMut<SelectedCity>,
                     mut tree: ResMut<crate::research_ui::TechTreeOpen>| match turn_state(&session, &reviewed) {
                        TurnState::Ready => {
                            end.write(EndTurnRequest);
                        }
                        _ => {
                            // The button takes over from any open panel.
                            city.0 = None;
                            tree.0 = false;
                            next.write(NextAttention);
                        }
                    },
                ),
            ));
        });
}

