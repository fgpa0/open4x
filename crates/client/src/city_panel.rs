//! City panel: shown on the right while a city is selected. Yields, growth and
//! borders (and buying tiles, see `land.rs`), the build queue and what can be
//! added to it or bought outright, the governor's focus and whether it
//! refills the queue, and the buildings already built.

use crate::icons::{Icon, Icons};
use crate::loading::InGameSet;
use crate::land::BuyLand;
use crate::play::{GameEvent, Notice, SelectedCity, Session};
use crate::ui::{self, palette, ButtonState, Tooltip};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use sim::city::{expansion_threshold, growth_threshold, Build, CityId, Focus, Project};
use sim::game::{Command, Game};
use sim::improvement::Passage;

#[derive(Component)]
struct CityPanel;

/// The panel's scrolling part (its position survives rebuilds).
#[derive(Component)]
struct CityScroll;

pub struct CityPanelPlugin;

impl Plugin for CityPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (rebuild, fit_height).chain().after(crate::play::PlaySet).in_set(InGameSet));
    }
}

const WIDTH: f32 = 370.0;
/// Room left free below the panel, for the End Turn button.
const BOTTOM_CLEARANCE: f32 = 116.0;

/// Keeps the panel between the civs' row and the End Turn button.
fn fit_height(window: Single<&Window>, ui_scale: Res<UiScale>, mut panels: Query<&mut Node, With<CityPanel>>) {
    let room = (window.height() / ui_scale.0 - crate::diplomacy::PANEL_TOP - BOTTOM_CLEARANCE).max(200.0);
    for mut node in &mut panels {
        if node.max_height != px(room) {
            node.max_height = px(room);
        }
    }
}

pub fn build_icon(game: &Game, b: Build) -> Icon {
    match b {
        Build::Unit(u) => Icon::unit(&game.rules.unit(u).art),
        Build::Building(id) => Icon::building(&game.rules.building(id).id),
        Build::Project(p) => project_icon(p),
        Build::Passage(p, _) => passage_icon(p),
    }
}

pub fn passage_icon(p: Passage) -> Icon {
    match p {
        Passage::Canal => Icon::Canal,
        Passage::Tunnel => Icon::Tunnel,
        Passage::Bridge => Icon::Bridge,
        Passage::Fort => Icon::Walls,
        Passage::Watchtower => Icon::Sight,
    }
}

/// What a canal, tunnel or bridge does (and, given its tile, what it would do there).
fn passage_text(game: &Game, p: Passage, tile: Option<sim::hex::TileId>) -> String {
    let mut body = match p {
        Passage::Canal => "Digs out a tile of flat land next to water and turns it into water for good. Dig a chain of them to join seas or bring ships to an inland city; whatever stood on the land (forest, farm, resource) is lost.".to_string(),
        Passage::Tunnel => "A road through a mountain: land units cross it like any road.".to_string(),
        Passage::Bridge => "A road over shallow water between two separate stretches of land: land units walk across without embarking, and ships still sail under it.".to_string(),
        Passage::Fort => {
            let f = &game.rules.improvements.fort;
            format!(
                "A fort on your land, up to {} tiles from the city: units in it defend {}% better, and it fires at an enemy next to it each turn. An enemy unit that stands on it pulls it down.",
                f.reach, f.defense
            )
        }
        Passage::Watchtower => format!(
            "A lookout on your land, up to {} tiles from the city: it sees {} tiles all round (one more from hills). An enemy unit that stands on it pulls it down.",
            game.rules.improvements.watchtower.reach, game.rules.improvements.watchtower.sight
        ),
    };
    if !p.is_post() {
        body.push_str(" Everyone may use it, wherever their borders let them go.");
    }
    if let Some(t) = tile {
        match p {
            Passage::Canal if !game.canal_joins(t) => {}
            Passage::Canal => match game.canal_shortcut(t) {
                None => body.push_str("\nHere it joins two separate waters."),
                Some(d) => body.push_str(&format!("\nHere it saves ships {} tiles going round.", d.saturating_sub(2))),
            },
            Passage::Bridge => match game.bridge_shortcut(t) {
                None => body.push_str("\nHere it joins two separate lands."),
                Some(d) => body.push_str(&format!("\nHere it saves armies {} tiles going round.", d.saturating_sub(2))),
            },
            Passage::Tunnel | Passage::Fort | Passage::Watchtower => {}
        }
    }
    format!("{body}\nCost: {} production", game.rules.passage(p).cost)
}

fn project_icon(p: Project) -> Icon {
    match p {
        Project::Food => Icon::Food,
        Project::Gold => Icon::Gold,
        Project::Science => Icon::Science,
        Project::Culture => Icon::Culture,
    }
}

fn project_yield(p: Project) -> &'static str {
    match p {
        Project::Food => "food",
        Project::Gold => "gold",
        Project::Science => "science",
        Project::Culture => "culture",
    }
}

/// "+3 gold per turn" for a project in a city making `production`.
fn project_gain(p: Project, production: i32) -> String {
    format!("+{} {} per turn", p.output(production), project_yield(p))
}

/// What a build does, for its tooltip.
fn build_tooltip(game: &Game, b: Build) -> Tooltip {
    let body = match b {
        Build::Unit(u) => {
            let d = game.rules.unit(u);
            let mut parts = vec![format!("{} moves", d.moves), format!("{} sight", d.sight)];
            if d.strength > 0 {
                parts.insert(0, format!("{} strength", d.strength));
            }
            let role = if d.has(sim::rules::Ability::FoundCity) {
                "Founds new cities.\n"
            } else if d.has(sim::rules::Ability::BuildImprovements) {
                "Improves tiles (coming soon).\n"
            } else {
                ""
            };
            let unique = if d.description.is_empty() { String::new() } else { format!("{}\n", d.description) };
            format!("{unique}{role}{}\nCost: {} production", parts.join(" · "), d.cost)
        }
        Build::Building(id) => {
            let d = game.rules.building(id);
            format!("{}\nCost: {} production", d.description, d.cost)
        }
        Build::Project(p) => format!(
            "Turns {}% of the city's production into {} every turn. It never finishes: the city keeps at it until you queue something else.",
            p.percent(),
            project_yield(p)
        ),
        Build::Passage(p, tile) => passage_text(game, p, Some(tile)),
    };
    Tooltip::new(game.build_name(b), body)
}

/// A full-width row button: icon, label, and something on the right.
fn row_button(image: Handle<Image>, label: String, right: impl Bundle, state: ButtonState, tip: Tooltip) -> impl Bundle {
    (
        bevy::ui_widgets::Button,
        bevy::picking::hover::Hovered::default(),
        Node {
            width: percent(100),
            height: px(36),
            align_items: AlignItems::Center,
            column_gap: px(10),
            padding: UiRect::new(px(6), px(10), px(0), px(0)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(palette::BUTTON),
        BorderColor::all(palette::BUTTON_BORDER),
        state,
        tip,
        children![
            ui::icon(image, 26.0),
            (ui::text(label, 14.5, palette::TEXT), Node { flex_grow: 1.0, ..default() }, Pickable::IGNORE),
            right,
        ],
    )
}

/// "⌛ 8" (turns), right-aligned in a row.
fn turns_tag(icons: &Icons, turns: i32) -> impl Bundle {
    (
        Node { align_items: AlignItems::Center, column_gap: px(4), ..default() },
        Pickable::IGNORE,
        children![ui::icon(icons.get(Icon::Turns), 16.0), (ui::bold(turns.to_string(), 14.0, palette::TEXT), Pickable::IGNORE)],
    )
}

fn set_queue(id: sim::city::CityId, queue: Vec<Build>) -> impl Fn(On<Activate>, ResMut<Session>, MessageWriter<GameEvent>) {
    move |_: On<Activate>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>| {
        let _ = session.command(Command::SetQueue { city: id, queue: queue.clone() }, &mut out);
    }
}

/// A gold button that buys `b` in `city` now, showing its price (with
/// "Buy" when `label`), dimmed when it can't be bought; the tooltip says why.
fn buy_button(icons: &Icons, game: &Game, city: CityId, b: Build, height: f32, label: bool) -> impl Bundle {
    let price = game.buy_price(city, b);
    let problem = game.buy_problem(city, b);
    let name = game.build_name(b).to_string();
    let c = &game.cities[&city];
    let mut body = if c.queue.first() == Some(&b) && c.production > 0 {
        format!("Finish it now for {price} gold. The production already made counts towards it.")
    } else {
        format!("Buy it now for {price} gold.")
    };
    if matches!(b, Build::Unit(_)) {
        body.push_str("\nBought units can move from next turn.");
    }
    if let Some(why) = problem {
        body.push_str(&format!("\n\nCan't buy now: {why}."));
    }
    let color = if problem.is_none() { palette::GOLD } else { palette::TEXT_DIM };
    let caption = if label { format!("Buy {price}") } else { price.to_string() };
    (
        bevy::ui_widgets::Button,
        bevy::picking::hover::Hovered::default(),
        Node {
            min_width: px(if label { 0.0 } else { 66.0 }),
            height: px(height),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: px(5),
            padding: UiRect::horizontal(px(8)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(5)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(palette::BUTTON),
        BorderColor::all(palette::BUTTON_BORDER),
        ButtonState { active: false, disabled: problem.is_some() },
        Tooltip::new(format!("Buy {name}"), body),
        children![
            ui::icon(icons.get(Icon::Gold), 16.0),
            (ui::bold(caption, 14.0, color), Pickable::IGNORE),
        ],
        observe(move |_: On<Activate>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>, mut notice: ResMut<Notice>| {
            match session.command(Command::Buy { city, build: b }, &mut out) {
                Ok(()) => notice.inform(Icon::Gold, format!("Bought {name} for {price} gold.")),
                Err(e) => notice.show(format!("Can't buy {name}: {e}.")),
            }
        }),
    )
}

/// A gold price beside a passage: click, then pick its site on the map to
/// buy it there (any site costs the same; the price counts production
/// already stored only for what's being built now).
fn passage_buy_button(icons: &Icons, game: &Game, city: CityId, passage: Passage, active: bool) -> impl Bundle {
    let sites = game.passage_sites(city, passage);
    let site = sites.first().copied();
    let price = site.map_or(0, |t| game.buy_price(city, Build::Passage(passage, t)));
    let problem = match site {
        None => Some("no site on this city's land"),
        Some(t) => game.buy_problem(city, Build::Passage(passage, t)),
    };
    let mut body = format!("Buy it for {price} gold: click, then pick a highlighted tile on the map.");
    if let Some(why) = problem {
        body.push_str(&format!("\n\nCan't buy now: {why}."));
    }
    let color = if problem.is_none() { palette::GOLD } else { palette::TEXT_DIM };
    (
        bevy::ui_widgets::Button,
        bevy::picking::hover::Hovered::default(),
        Node {
            min_width: px(66.0),
            height: px(36.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: px(5),
            padding: UiRect::horizontal(px(8)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(5)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(palette::BUTTON),
        BorderColor::all(palette::BUTTON_BORDER),
        ButtonState { active, disabled: problem.is_some() },
        Tooltip::new(format!("Buy a {}", passage.name()), body),
        children![
            ui::icon(icons.get(Icon::Gold), 16.0),
            (ui::bold(price.to_string(), 14.0, color), Pickable::IGNORE),
        ],
        observe(move |_: On<Activate>, session: Res<Session>, mut placing: ResMut<crate::placement::Placing>| {
            let game = &session.game;
            let Some(&t) = game.passage_sites(city, passage).first() else { return };
            if game.buy_problem(city, Build::Passage(passage, t)).is_none() {
                placing.0 = Some(crate::placement::Placement { city, passage, front: false, buy: true });
            }
        }),
    )
}

/// Rebuilds the panel whenever the selection or the game changes (cheap: a
/// few dozen nodes, and the game only changes on commands and turns).
fn rebuild(
    mut commands: Commands,
    session: Res<Session>,
    selected: Res<SelectedCity>,
    icons: Res<Icons>,
    old: Query<Entity, With<CityPanel>>,
    old_scroll: Query<&ScrollPosition, With<CityScroll>>,
    land: Res<BuyLand>,
    season: Res<crate::season::Season>,
    placement: Res<crate::placement::Placing>,
    mut shown: Local<Option<sim::city::CityId>>,
) {
    if !session.is_changed() && !selected.is_changed() && !land.is_changed() && !placement.is_changed() {
        return;
    }
    // Keep the scroll position while the same city stays open.
    let scroll = match old_scroll.iter().next() {
        Some(pos) if *shown == selected.0 => pos.0,
        _ => Vec2::ZERO,
    };
    *shown = selected.0;
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some(id) = selected.0 else { return };
    let game = &session.game;
    let Some(c) = game.cities.get(&id) else { return };
    if c.owner != session.me {
        return;
    }
    let out = game.city_output(id);
    let y = out.yields;
    let color = crate::unit_panel::player_color(&session, c.owner);
    let threshold = growth_threshold(c.population);
    let border_cost = expansion_threshold(c.expansions);

    commands
        .spawn((
            CityPanel,
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::diplomacy::PANEL_TOP),
                right: px(16),
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(10)),
                overflow: Overflow::clip(),
                ..default()
            },
            ui::frame(),
        ))
        .with_children(|p| {
            // Header in the owner's colour: population, name, close.
            p.spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(12),
                    padding: UiRect::axes(px(14), px(10)),
                    border: UiRect::bottom(px(2)),
                    ..default()
                },
                BackgroundColor(color.darker(0.25).with_alpha(0.9)),
                BorderColor::all(palette::PANEL_BORDER),
            ))
            .with_children(|row| {
                row.spawn((
                    Node {
                        width: px(40),
                        height: px(40),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.1, 0.09, 0.08)),
                    BorderColor::all(palette::ACCENT),
                    Tooltip::new("Population", "Each citizen works one tile around the city."),
                    children![(ui::heading(c.population.to_string(), 20.0, palette::ACCENT), Pickable::IGNORE)],
                ));
                row.spawn((ui::heading(c.name.clone(), 24.0, Color::WHITE), TextShadow::default(), Node { flex_grow: 1.0, ..default() }));
                let (now, mean, lat) = crate::season::temperature_now(game, c.tile, &season);
                row.spawn((
                    ui::bold(format!("{now:.0} °C"), 16.0, palette::TEXT),
                    Tooltip::new(
                        format!("{}: {now:.0} °C", season.name_at(lat)),
                        format!("Averages {mean:.0} °C over the year. Below freezing, snow settles."),
                    ),
                ));
                row.spawn((
                    ui::icon_button(icons.get(Icon::Close), 30.0, Tooltip::title("Close (Esc)")),
                    observe(|_: On<Activate>, mut selected: ResMut<SelectedCity>| selected.0 = None),
                ));
            });

            // Everything else scrolls.
            p.spawn((
                CityScroll,
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 1.0,
                    min_height: px(0),
                    row_gap: px(5),
                    padding: UiRect::new(px(14), px(14), px(10), px(14)),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ScrollPosition(scroll),
            ))
            .with_children(|p| {
                // Yields per turn.
                p.spawn(Node { justify_content: JustifyContent::SpaceBetween, padding: UiRect::horizontal(px(4)), ..default() })
                    .with_children(|row| {
                        let food = format!("{:+}", out.food_surplus);
                        let food_color = if out.food_surplus < 0 { palette::DANGER } else { palette::FOOD };
                        row.spawn((
                            ui::stat(&icons, Icon::Food, food, 17.0, food_color),
                            Tooltip::new(
                                "Food",
                                format!("{} food made, {} eaten by citizens. The surplus grows the city.", y.food, y.food - out.food_surplus),
                            ),
                        ));
                        row.spawn((
                            ui::stat(&icons, Icon::Production, y.production.to_string(), 17.0, palette::PRODUCTION),
                            Tooltip::new("Production", "Builds what's at the front of the queue: units, buildings, settlers, workers and merchants."),
                        ));
                        row.spawn((
                            ui::stat(&icons, Icon::Gold, format!("{:+}", y.gold), 17.0, palette::GOLD),
                            Tooltip::new("Gold", "Added to your treasury every turn."),
                        ));
                        row.spawn((
                            ui::stat(&icons, Icon::Science, format!("{:+}", out.science), 17.0, palette::SCIENCE),
                            Tooltip::new("Science", "One per citizen plus one, and more from libraries and temples."),
                        ));
                        row.spawn((
                            ui::stat(&icons, Icon::Culture, format!("{:+}", out.culture), 17.0, palette::CULTURE),
                            Tooltip::new("Culture", "Makes the city's borders grow."),
                        ));
                    });

                // Growth and borders.
                p.spawn(ui::section("Growth"));
                let grow = match game.turns_to_grow(id) {
                    Some(t) => format!("Grows in {t} turns"),
                    None if out.food_surplus < 0 => "Starving!".into(),
                    None => "Not growing".into(),
                };
                p.spawn(meter(&icons, Icon::Population, grow, format!("{}/{}", c.food, threshold), c.food as f32 / threshold as f32, palette::FOOD));
                let borders = match game.turns_to_expand(id) {
                    Some(t) => format!("Borders grow in {t} turns"),
                    None => "Borders not growing".into(),
                };
                p.spawn((
                    meter(&icons, Icon::Culture, borders, format!("{}/{}", c.culture, border_cost), c.culture as f32 / border_cost as f32, palette::CULTURE),
                    Tooltip::new("Borders", "Culture claims one more tile when the bar fills. The tile it takes next glows on the map."),
                ));
                // Buying tiles: a toggle that puts price tags on the map.
                let cheapest = game.buyable_tiles(id).into_iter().map(|t| game.tile_price(id, t)).min();
                let on = land.0 && cheapest.is_some();
                let (right, tip) = match cheapest {
                        Some(_) if on => ("Done".to_string(), Tooltip::new("Stop buying tiles", "Hide the prices on the map.")),
                    Some(price) => (
                        format!("from {price}"),
                        Tooltip::new(
                            "Buy tiles",
                            "Shows a price on every tile next to the city's borders, within its reach. Click a price to buy that tile: it joins the city's territory straight away. Tiles cost more further out, the better they are, and the more land the city has.",
                        ),
                    ),
                    None => ("none for sale".to_string(), Tooltip::new("Buy tiles", "No unclaimed tiles next to the borders are within the city's reach.")),
                };
                p.spawn((
                    row_button(
                        icons.get(Icon::Gold),
                        if on { "Click a price on the map".into() } else { "Buy tiles".into() },
                        (ui::text(right, 13.0, if cheapest.is_some() { palette::GOLD } else { palette::TEXT_DIM }), Pickable::IGNORE),
                        ButtonState { active: on, disabled: cheapest.is_none() },
                        tip,
                    ),
                    observe(move |_: On<Activate>, session: Res<Session>, mut land: ResMut<BuyLand>| {
                        if !session.game.buyable_tiles(id).is_empty() {
                            land.0 = !land.0;
                        }
                    }),
                ));

                defence(p, &icons, game, id);

                // Current production.
                p.spawn(ui::section("Production"));
                match c.queue.first() {
                    Some(&Build::Project(project)) => {
                        let b = Build::Project(project);
                        p.spawn((
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: px(12),
                                padding: UiRect::all(px(8)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(6)),
                                ..default()
                            },
                            BackgroundColor(palette::PANEL_LIGHT),
                            BorderColor::all(palette::PRODUCTION.with_alpha(0.6)),
                            build_tooltip(game, b),
                        ))
                        .with_children(|card| {
                            card.spawn(ui::icon(icons.get(build_icon(game, b)), 44.0));
                            card.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(4), flex_grow: 1.0, ..default() }, Pickable::IGNORE))
                                .with_children(|col| {
                                    col.spawn((ui::bold(game.build_name(b), 16.0, palette::TEXT), Pickable::IGNORE));
                                    col.spawn((ui::text(project_gain(project, y.production), 13.0, palette::TEXT), Pickable::IGNORE));
                                    col.spawn((ui::text("Repeats until you queue something else.", 12.0, palette::TEXT_DIM), Pickable::IGNORE));
                                });
                            card.spawn((
                                ui::icon_button(icons.get(Icon::Close), 26.0, Tooltip::new("Stop", "Stop the project.")),
                                observe(set_queue(id, c.queue[1..].to_vec())),
                            ));
                        });
                    }
                    Some(&b) => {
                        let cost = game.build_cost(b);
                        let turns = game.turns_to_build(id).unwrap_or(0);
                        p.spawn((
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: px(12),
                                padding: UiRect::all(px(8)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(6)),
                                ..default()
                            },
                            BackgroundColor(palette::PANEL_LIGHT),
                            BorderColor::all(palette::PRODUCTION.with_alpha(0.6)),
                            build_tooltip(game, b),
                        ))
                        .with_children(|card| {
                            card.spawn(ui::icon(icons.get(build_icon(game, b)), 44.0));
                            card.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(5), flex_grow: 1.0, ..default() }, Pickable::IGNORE))
                                .with_children(|col| {
                                    col.spawn((
                                        Node { justify_content: JustifyContent::SpaceBetween, ..default() },
                                        Pickable::IGNORE,
                                        children![
                                            (ui::bold(game.build_name(b), 16.0, palette::TEXT), Pickable::IGNORE),
                                            turns_tag(&icons, turns),
                                        ],
                                    ));
                                    col.spawn((ui::progress_bar(c.production as f32 / cost as f32, palette::PRODUCTION, 8.0), Pickable::IGNORE));
                                    col.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() })
                                        .with_children(|row| {
                                            row.spawn((ui::text(format!("{}/{} production", c.production, cost), 12.0, palette::TEXT_DIM), Pickable::IGNORE));
                                            row.spawn(buy_button(&icons, game, id, b, 26.0, true));
                                        });
                                });
                            // Cancel: the stored production carries over to the next choice.
                            card.spawn((
                                ui::icon_button(
                                    icons.get(Icon::Close),
                                    26.0,
                                    Tooltip::new("Cancel", "Remove from the queue. Production already made carries over to whatever you build next."),
                                ),
                                observe(set_queue(id, c.queue[1..].to_vec())),
                            ));
                        });
                    }
                    None => {
                        let mut note = if c.auto_queue {
                            "Choose something below. If you don't, the governor picks when the turn ends.".to_string()
                        } else {
                            "Nothing queued: this turn's production becomes gold.".to_string()
                        };
                        if c.production > 0 {
                            note.push_str(&format!("\n{} production is stored for the next build.", c.production));
                        }
                        p.spawn((
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: px(10),
                                padding: UiRect::all(px(8)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(6)),
                                ..default()
                            },
                            BorderColor::all(palette::ACCENT.with_alpha(0.6)),
                            children![
                                ui::icon(icons.get(Icon::Warning), 24.0),
                                (ui::text(note, 13.5, palette::ACCENT), Node { flex_shrink: 1.0, ..default() }),
                            ],
                        ));
                    }
                }

                // The rest of the queue: click to remove.
                for i in 1..c.queue.len() {
                    let mut rest = c.queue.clone();
                    let b = rest.remove(i);
                    let mut tip = build_tooltip(game, b);
                    tip.body.push_str("\nClick to remove from the queue.");
                    p.spawn(Node { width: percent(100), column_gap: px(6), align_items: AlignItems::Center, ..default() }).with_children(|line| {
                        line.spawn((
                            row_button(
                                icons.get(build_icon(game, b)),
                                format!("{i}. {}", game.build_name(b)),
                                ui::icon(icons.get(Icon::Close), 14.0),
                                ButtonState::default(),
                                tip,
                            ),
                            observe(set_queue(id, rest)),
                        ));
                        line.spawn(buy_button(&icons, game, id, b, 36.0, false));
                    });
                }

                // What can be built.
                p.spawn(ui::section("Build"));
                p.spawn(ui::text(
                    "Click to queue · Shift-click to build next · Click a price to buy it now. Production carries over when you switch.",
                    12.0,
                    palette::TEXT_DIM,
                ));
                let prod = (y.production).max(1);
                let builds = game.available_builds(id);
                let (units, buildings): (Vec<Build>, Vec<Build>) = builds.into_iter().partition(|b| matches!(b, Build::Unit(_)));
                for (label, list) in [("Units", units), ("Buildings", buildings)] {
                    if list.is_empty() {
                        continue;
                    }
                    p.spawn((ui::text(label, 12.5, palette::ACCENT), Node { margin: UiRect::top(px(4)), ..default() }));
                    for b in list {
                        // Stored production carries over, so it counts towards anything.
                        let turns = ((game.build_cost(b) - c.production).max(0) + prod - 1) / prod;
                        let queued = c.queue.contains(&b);
                        let mut line = p.spawn(Node { width: percent(100), column_gap: px(6), align_items: AlignItems::Center, ..default() });
                        line.with_children(|line| {
                        line.spawn((
                            row_button(
                                icons.get(build_icon(game, b)),
                                game.build_name(b).to_string(),
                                turns_tag(&icons, turns),
                                ButtonState { active: queued, disabled: false },
                                build_tooltip(game, b),
                            ),
                            // Click appends; Shift-click builds it next (front of the queue).
                            observe(
                                move |_: On<Activate>,
                                      keys: Res<ButtonInput<KeyCode>>,
                                      mut session: ResMut<Session>,
                                      mut out: MessageWriter<GameEvent>| {
                                    let Some(c) = session.game.cities.get(&id) else { return };
                                    // Choosing something to build ends a project.
                                    let mut queue: Vec<Build> = c.queue.iter().copied().filter(|b| !matches!(b, Build::Project(_))).collect();
                                    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
                                        queue.insert(0, b);
                                    } else {
                                        queue.push(b);
                                    }
                                    let _ = session.command(Command::SetQueue { city: id, queue }, &mut out);
                                },
                            ),
                        ));
                        line.spawn(buy_button(&icons, game, id, b, 36.0, false));
                        });
                    }
                }

                // Canals, tunnels, bridges, forts and watchtowers: choose a site on the map, then build.
                let passages: Vec<Passage> =
                    Passage::ALL.into_iter().filter(|&pa| game.knows(c.owner, game.rules.passage_tech(pa))).collect();
                if !passages.is_empty() {
                    p.spawn((ui::text("Passages", 12.5, palette::ACCENT), Node { margin: UiRect::top(px(4)), ..default() }));
                }
                for passage in passages {
                    let sites = game.passage_sites(id, passage).len();
                    let cost = game.rules.passage(passage).cost;
                    let turns = ((cost - c.production).max(0) + prod - 1) / prod;
                    let mut body = passage_text(game, passage, None);
                    body.push_str(if sites == 0 {
                        match passage {
                            Passage::Canal => "\n\nNo site on this city's land: it needs flat land next to water.",
                            Passage::Tunnel => "\n\nNo mountain on this city's land to dig through.",
                            Passage::Bridge => "\n\nNo site on this city's land: it needs shallow water with land on two sides that don't touch.",
                            Passage::Fort | Passage::Watchtower => "\n\nNo site: it needs open land or hills of yours near the city.",
                        }
                    } else {
                        "\n\nClick, then pick a highlighted tile on the map. Shift-click to build it next."
                    });
                    let placing = placement.0.is_some_and(|pl| pl.city == id && pl.passage == passage);
                    let mut line = p.spawn(Node { width: percent(100), column_gap: px(6), align_items: AlignItems::Center, ..default() });
                    line.with_children(|line| {
                        line.spawn((
                            row_button(
                                icons.get(passage_icon(passage)),
                                format!("{} ({sites} sites)", passage.name()),
                                turns_tag(&icons, turns),
                                ButtonState { active: placing && !placement.0.is_some_and(|pl| pl.buy), disabled: sites == 0 },
                                Tooltip::new(passage.name(), body),
                            ),
                            observe(
                                move |_: On<Activate>, keys: Res<ButtonInput<KeyCode>>, session: Res<Session>, mut placing: ResMut<crate::placement::Placing>| {
                                    if session.game.passage_sites(id, passage).is_empty() {
                                        return;
                                    }
                                    let front = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
                                    placing.0 = Some(crate::placement::Placement { city: id, passage, front, buy: false });
                                },
                            ),
                        ));
                        line.spawn(passage_buy_button(&icons, game, id, passage, placing && placement.0.is_some_and(|pl| pl.buy)));
                    });
                }

                // Projects: production into another yield, for as long as you like.
                p.spawn((ui::text("Projects", 12.5, palette::ACCENT), Node { margin: UiRect::top(px(4)), ..default() }));
                for project in Project::ALL {
                    let b = Build::Project(project);
                    let mut queue: Vec<Build> = c.queue.iter().copied().filter(|b| !matches!(b, Build::Project(_))).collect();
                    queue.push(b);
                    let mut tip = build_tooltip(game, b);
                    tip.body.push_str("\nIt starts once the rest of the queue is built.");
                    p.spawn((
                        row_button(
                            icons.get(build_icon(game, b)),
                            game.build_name(b).to_string(),
                            (ui::text(project_gain(project, y.production), 12.5, palette::TEXT_DIM), Pickable::IGNORE),
                            ButtonState { active: c.queue.contains(&b), disabled: false },
                            tip,
                        ),
                        observe(set_queue(id, queue)),
                    ));
                }

                // Governor.
                p.spawn(ui::section("Governor"));
                p.spawn(Node { column_gap: px(8), ..default() }).with_children(|row| {
                    for focus in Focus::ALL {
                        let (icon, body) = match focus {
                            Focus::Balanced => (Icon::Balanced, "Works a mix of food and production."),
                            Focus::Food => (Icon::Food, "Favours food so the city grows quickly."),
                            Focus::Production => (Icon::Production, "Favours production to build faster."),
                            Focus::Gold => (Icon::Gold, "Favours gold."),
                        };
                        row.spawn((
                            bevy::ui_widgets::Button,
                            bevy::picking::hover::Hovered::default(),
                            Node {
                                flex_grow: 1.0,
                                flex_basis: px(0),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: px(3),
                                padding: UiRect::axes(px(4), px(6)),
                                border: UiRect::all(px(1.5)),
                                border_radius: BorderRadius::all(px(6)),
                                ..default()
                            },
                            BackgroundColor(palette::BUTTON),
                            BorderColor::all(palette::BUTTON_BORDER),
                            ButtonState { active: c.focus == focus, disabled: false },
                            Tooltip::new(format!("Focus: {}", focus.name()), format!("{body}\nThe governor picks which tiles the citizens work.")),
                            children![
                                ui::icon(icons.get(icon), 28.0),
                                (ui::text(focus.name(), 12.0, palette::TEXT), Pickable::IGNORE),
                            ],
                            observe(move |_: On<Activate>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>| {
                                let _ = session.command(Command::SetFocus { city: id, focus }, &mut out);
                            }),
                        ));
                    }
                });
                let auto = c.auto_queue;
                p.spawn((
                    row_button(
                        icons.get(Icon::Auto),
                        "Governor picks if I don't".into(),
                        ui::icon(icons.get(if auto { Icon::Check } else { Icon::Close }), 18.0),
                        ButtonState { active: auto, disabled: false },
                        Tooltip::new(
                            format!("Automatic production: {}", if auto { "On" } else { "Off" }),
                            "If the queue is still empty when the turn ends, the governor picks what to build. Your own choices always come first.",
                        ),
                    ),
                    observe(move |_: On<Activate>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>| {
                        let _ = session.command(Command::SetAutoQueue { city: id, on: !auto }, &mut out);
                    }),
                ));
                if !c.buildings.is_empty() {
                    p.spawn(ui::section("Buildings"));
                    p.spawn(Node { column_gap: px(8), flex_wrap: FlexWrap::Wrap, row_gap: px(8), ..default() }).with_children(|row| {
                        for &b in &c.buildings {
                            let d = game.rules.building(b);
                            row.spawn((
                                Node {
                                    width: px(44),
                                    height: px(44),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(palette::PANEL_LIGHT),
                                BorderColor::all(palette::BUTTON_BORDER),
                                Tooltip::new(d.name.clone(), d.description.clone()),
                                children![ui::icon(icons.get(Icon::building(&d.id)), 32.0)],
                            ));
                        }
                    });
                }
            });
        });
}

/// The city's defence: its strength, walls, reach and health, and its garrison.
fn defence(p: &mut ChildSpawnerCommands, icons: &Icons, game: &Game, id: CityId) {
    let c = &game.cities[&id];
    p.spawn(ui::section("Defence"));
    let walls = game.city_walls(id);
    let range = game.city_range(id);
    p.spawn(Node { justify_content: JustifyContent::SpaceBetween, padding: UiRect::horizontal(px(4)), ..default() }).with_children(|row| {
        row.spawn((
            ui::stat(icons, Icon::Strength, game.city_strength(id).to_string(), 16.0, palette::TEXT),
            Tooltip::new(
                "Strength",
                format!(
                    "The city defends itself: {} for its era, plus one per citizen, plus {} from its garrison (half a soldier's strength).",
                    sim::combat::CITY_BASE_STRENGTH + sim::combat::CITY_ERA_STRENGTH * game.era(c.owner) as i32,
                    game.garrison_strength(id)
                ),
            ),
        ));
        row.spawn((
            ui::stat(icons, Icon::Walls, format!("+{walls}%"), 16.0, if walls > 0 { palette::TEXT } else { palette::TEXT_DIM }),
            Tooltip::new(
                "Walls",
                "Defensive buildings add to the city's strength and to its shot.",
            ),
        ));
        row.spawn((
            ui::stat(icons, Icon::Archer, format!("{range}"), 16.0, palette::TEXT),
            Tooltip::new("Fire", format!("Every turn the city fires at one enemy within {range} tile{}.", if range == 1 { "" } else { "s" })),
        ));
        let hp_color = if c.hp * 2 < sim::combat::CITY_MAX_HP { palette::DANGER } else { palette::TEXT };
        row.spawn((
            ui::stat(icons, Icon::Health, format!("{}/{}", c.hp, sim::combat::CITY_MAX_HP), 16.0, hp_color),
            Tooltip::new(
                "Walls' health",
                "Attacks bring it down; at 0 a melee unit takes the city. It repairs 15 a turn, 5 while enemies stand next to it, none on a turn it was attacked.",
            ),
        ));
    });
    match game.garrison(id) {
        Some(g) => {
            let def = game.unit_def(&game.units[&g]);
            let name = def.name.clone();
            p.spawn((
                row_button(
                    icons.get(Icon::unit(&def.art)),
                    format!("Garrison: {name} (+{} strength)", game.garrison_strength(id)),
                    ui::icon(icons.get(Icon::Moves), 16.0),
                    ButtonState::default(),
                    Tooltip::new(
                        format!("Garrison: {name}"),
                        format!(
                            "A soldier standing in the city adds {}% of its strength to the city's. It takes no damage while the city stands, and is pushed out if it falls. Click to select it.",
                            sim::combat::GARRISON_PERCENT
                        ),
                    ),
                ),
                observe(move |_: On<Activate>, mut selected: ResMut<crate::play::Selected>, mut city: ResMut<SelectedCity>| {
                    city.0 = None;
                    selected.0 = Some(g);
                }),
            ));
        }
        None => {
            p.spawn(ui::text(
                "No garrison: a soldier in the city adds half its strength. Select units and right-click the city to send them to defend it.",
                12.5,
                palette::TEXT_DIM,
            ));
        }
    }
}

/// A labelled progress bar with an icon: "Grows in 2 turns ... 12/15".
fn meter(icons: &Icons, which: Icon, label: String, amount: String, fraction: f32, color: Color) -> impl Bundle {
    (
        Node { align_items: AlignItems::Center, column_gap: px(10), padding: UiRect::vertical(px(2)), ..default() },
        children![
            ui::icon(icons.get(which), 26.0),
            (
                Node { flex_direction: FlexDirection::Column, row_gap: px(4), flex_grow: 1.0, ..default() },
                children![
                    (
                        Node { justify_content: JustifyContent::SpaceBetween, ..default() },
                        children![ui::text(label, 13.5, palette::TEXT), ui::text(amount, 12.5, palette::TEXT_DIM)],
                    ),
                    ui::progress_bar(fraction, color, 7.0),
                ],
            ),
        ],
    )
}
