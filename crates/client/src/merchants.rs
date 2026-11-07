//! The trade routes screen: every merchant you have and where it trades,
//! what foreign merchants bring your cities, and, for a merchant being given
//! a route, every city it could trade with and what each would earn (see
//! `sim::trade`). It opens from the top bar's trade button, from a
//! Merchant's Trade Route button (or E), by itself when a merchant without a
//! route is selected (merchants can't be moved, only given routes), and
//! closes with Esc. Also the news
//! of sales, plundered merchants and routes that ended.

use crate::icons::{Icon, Icons};
use crate::loading::InGameSet;
use crate::play::{GameEvent, NewsKind, Notice, Selected, Session};
use crate::ui::{self, palette, Tooltip};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use sim::city::CityId;
use sim::game::{Command, Event, Game, Order, PlayerId, UnitId};
use sim::trade::{Route, RouteOffer};

/// Whether the screen is open, and for which merchant it's choosing a route
/// (or, `moving`, a city to move to).
#[derive(Resource, Default)]
pub struct MerchantScreen {
    pub open: bool,
    pub unit: Option<UnitId>,
    pub moving: bool,
}

impl MerchantScreen {
    /// Opens the screen to choose a route for `unit`.
    pub fn choose(&mut self, unit: UnitId) {
        *self = MerchantScreen { open: true, unit: Some(unit), moving: false };
    }

    /// Opens the screen to choose a city for `unit` to move to (its unit
    /// card's Move to City).
    pub fn transfer(&mut self, unit: UnitId) {
        *self = MerchantScreen { open: true, unit: Some(unit), moving: true };
    }

    pub fn overview(&mut self) {
        *self = MerchantScreen { open: true, unit: None, moving: false };
    }
}

#[derive(Component)]
struct MerchantRoot;

/// A merchant, its route (if it has one) and what that pays now.
type MerchantRoute = (UnitId, Option<(Route, Option<RouteOffer>)>);

pub struct MerchantsPlugin;

impl Plugin for MerchantsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MerchantScreen>()
            .add_systems(Update, (reset, announce, ask_route, refresh).chain().after(crate::play::PlaySet).in_set(InGameSet));
    }
}

/// A new game starts with the screen closed. Dev aid: FOURX_MERCHANTS=1
/// opens it to choose a route for the merchant `play::start_game` put in
/// our first city (`move`: a city to move it to).
fn reset(session: Res<Session>, mut screen: ResMut<MerchantScreen>) {
    if !session.is_added() {
        return;
    }
    *screen = MerchantScreen::default();
    let dev = std::env::var("FOURX_MERCHANTS").ok();
    // The newest: the one `dev_merchant` put there.
    if let Some(unit) = session.game.merchants(session.me).map(|u| u.id).max() {
        match dev.as_deref() {
            Some("1") => screen.choose(unit),
            Some("move") => screen.transfer(unit),
            _ => {}
        }
    }
}

/// A merchant waiting for a route that gets selected (it needs attention, or
/// was clicked) opens the screen to choose one, unless the screen itself just
/// selected it (its Show button).
fn ask_route(session: Res<Session>, selected: Res<Selected>, mut screen: ResMut<MerchantScreen>) {
    if !selected.is_changed() || (screen.is_changed() && !screen.open) {
        return;
    }
    let game = &session.game;
    let idle = selected.0.and_then(|id| game.units.get(&id)).filter(|u| u.owner == session.me && game.is_merchant(u) && u.order == Order::None);
    if let Some(u) = idle {
        if screen.unit != Some(u.id) {
            screen.choose(u.id);
        }
    }
}

/// Dev aid for `play::start_game`: FOURX_MERCHANTS=1|move|go|idle has us meet every
/// civ and know where their cities are, and puts a Merchant in our first
/// city (use with FOURX_TURNS so cities exist). `go` sends it on the best
/// route, `idle` stands it beside the city. Returns it.
pub fn dev_merchant(game: &mut Game, me: PlayerId) -> Option<UnitId> {
    let mode = std::env::var("FOURX_MERCHANTS").ok()?;
    let n = game.players.len();
    for p in 0..n {
        game.players[me as usize].met.resize(n, false);
        game.players[me as usize].met[p] = true;
        game.players[p].met.resize(n, false);
        game.players[p].met[me as usize] = true;
    }
    let tiles: Vec<_> = game.cities.values().flat_map(|c| game.map.grid.within(c.tile, 1)).collect();
    for t in tiles {
        game.players[me as usize].explored[t as usize] = true;
    }
    let home = game.cities.values().filter(|c| c.owner == me).map(|c| c.tile).next()?;
    let unit = game.spawn_unit(me, game.rules.unit_type_id("merchant")?, home);
    match mode.as_str() {
        "go" => {
            if let Some(best) = game.route_offers(unit).first() {
                let _ = game.apply(me, Command::Trade { unit, to: best.to });
            }
        }
        "idle" => {
            let free = |t: sim::hex::TileId| game.map.land_move_cost(&game.rules, t).is_some() && game.units_at(t).next().is_none();
            let mut near = game.map.grid.within(home, 2);
            near.sort_by_key(|&t| (game.map.tile(t).feature.is_some(), game.map.grid.distance(t, home), t));
            let beside = near.into_iter().find(|&t| t != home && free(t));
            game.units.get_mut(&unit).expect("spawned").tile = beside.unwrap_or(home);
        }
        _ => {}
    }
    game.update_visibility(me);
    let p = crate::layout::tile_center(&game.map.grid, game.units[&unit].tile);
    info!("FOURX_MERCHANTS: merchant at {:.1},{:.1}", p.x, p.y);
    Some(unit)
}

/// One line of news per turn for the sales, and news of merchants lost.
fn announce(session: Res<Session>, mut events: MessageReader<GameEvent>, mut notice: ResMut<Notice>) {
    let game = &session.game;
    let me = session.me;
    let (mut sales, mut gold, mut science, mut hosted, mut supplies) = (0, 0, 0, 0, (0, 0));
    for GameEvent(e) in events.read() {
        match *e {
            Event::Traded(r) if r.owner == me => {
                sales += 1;
                gold += r.gold;
                science += r.science;
                supplies.0 += r.food;
                supplies.1 += r.production;
            }
            Event::Traded(r) if game.cities.get(&r.to).is_some_and(|c| c.owner == me) => hosted += r.partner_gold,
            Event::RouteEnded { unit, city } => {
                if game.units.get(&unit).is_some_and(|u| u.owner == me) {
                    let to = game.cities.get(&city).map_or("its partner".into(), |c| c.name.clone());
                    let at = game.units.get(&unit).map(|u| u.tile);
                    notice.alert(game.turn, NewsKind::Cities, format!("A merchant's route to {to} ended: it needs a new one"), at);
                }
            }
            Event::RouteChanged { unit, from, to } => {
                if let Some(u) = game.units.get(&unit).filter(|u| u.owner == me) {
                    let name = |c| game.cities.get(&c).map_or("?".to_string(), |c| c.name.clone());
                    let text = format!("A merchant now trades with {} instead of {}: it pays better", name(to), name(from));
                    notice.news(game.turn, NewsKind::Cities, Icon::Trade, text, Some(u.tile));
                }
            }
            Event::Plundered { by, from, gold } if from == me => {
                notice.alert(game.turn, NewsKind::Combat, format!("{} captured one of your merchants and {gold} gold of goods", game.civ(by).name), None);
            }
            Event::Plundered { by, from, gold } if by == me => {
                notice.news(game.turn, NewsKind::Combat, Icon::Trade, format!("You captured a {} merchant with {gold} gold of goods", game.civ(from).adjective), None);
            }
            _ => {}
        }
    }
    if sales > 0 || hosted > 0 {
        let mut text = match sales {
            0 => String::new(),
            1 => format!("A merchant sold its goods: +{gold} gold"),
            n => format!("{n} merchants sold their goods: +{gold} gold"),
        };
        if science > 0 {
            text.push_str(&format!(", +{science} science"));
        }
        if supplies != (0, 0) {
            text.push_str(&format!(", +{} food and +{} production at home", supplies.0, supplies.1));
        }
        if hosted > 0 {
            text.push_str(&format!("{}+{hosted} gold from foreign merchants", if text.is_empty() { "" } else { "; " }));
        }
        notice.news(game.turn, NewsKind::Cities, Icon::Trade, text, None);
    }
}

/// "3 turns", "1 turn".
fn turns(n: u32) -> String {
    if n == 1 { "1 turn".into() } else { format!("{n} turns") }
}

/// The name of a city and, if it isn't ours, whose it is.
fn city_label(game: &Game, me: PlayerId, city: CityId) -> String {
    match game.cities.get(&city) {
        Some(c) if c.owner == me => c.name.clone(),
        Some(c) => format!("{} ({})", c.name, game.civ(c.owner).name),
        None => "a lost city".into(),
    }
}

/// Why a route pays what it does, for its tooltip.
fn breakdown(game: &Game, me: PlayerId, offer: &RouteOffer) -> String {
    let (home, to) = (&game.cities[&offer.home], &game.cities[&offer.to]);
    let distance = game.map.grid.distance(home.tile, to.tile);
    let market = |c: &sim::city::City| c.buildings.iter().any(|&b| game.rules.building_is(b, "market"));
    let mut lines = vec![format!(
        "Each trip sells goods worth {} gold: more the farther it goes ({distance} tiles), and a tenth more for each citizen there (size {}).",
        offer.value.gold, to.population
    )];
    if offer.foreign {
        lines.push(format!("Trade abroad pays +50%, and {} earns {} gold per trip too.", game.civ(to.owner).name, offer.value.partner_gold));
    }
    for (c, place) in [(home, "home"), (to, "there")] {
        if market(c) {
            lines.push(format!("A Market in {} ({place}): +25%.", c.name));
        }
    }
    let perks: i32 = game.civ_effects(me).map(|e| if let sim::civ::Effect::TradeGold(p) = e { *p } else { 0 }).sum();
    if perks > 0 {
        lines.push(format!("Your techs and perks: +{perks}%."));
    }
    if offer.value.science > 0 {
        lines.push(format!("They know technologies you don't: +{} science per trip.", offer.value.science));
    }
    if offer.value.food > 0 || offer.value.production > 0 {
        lines.push(format!(
            "Trade at home also brings {} +{} food and +{} production per trip.",
            home.name, offer.value.food, offer.value.production
        ));
    }
    lines.push(format!(
        "A round trip takes about {}, trading at each end included. Roads make it faster.",
        turns(offer.turns)
    ));
    lines.push(format!(
        "All told it's worth {:.1} gold a turn, counting food and production double: routes are ranked by that.",
        offer.worth_per_turn()
    ));
    lines.join("\n")
}

fn refresh(
    mut commands: Commands,
    session: Res<Session>,
    screen: Res<MerchantScreen>,
    icons: Res<Icons>,
    roots: Query<Entity, With<MerchantRoot>>,
) {
    if !screen.is_changed() && !session.is_changed() {
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
    let mine: Vec<_> = game.merchants(me).collect();
    let choosing = screen.unit.filter(|id| game.units.get(id).is_some_and(|u| u.owner == me && game.is_merchant(u)));
    // What each merchant's route earns now.
    let routes: Vec<MerchantRoute> = mine
        .iter()
        .map(|u| {
            let r = match u.order {
                Order::Trade(r) => Some((r, game.route_offer(u, r.home, r.to))),
                _ => None,
            };
            (u.id, r)
        })
        .collect();
    let income: f32 = routes.iter().filter_map(|(_, r)| r.and_then(|(_, o)| o)).map(|o| o.gold_per_turn()).sum();

    commands
        .spawn((
            MerchantRoot,
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
            GlobalZIndex(8),
        ))
        .with_children(|root| {
            root.spawn(ui::panel_with(Node {
                width: px(780),
                max_height: percent(84),
                align_items: AlignItems::Stretch,
                row_gap: px(8),
                ..default()
            }))
            .with_children(|panel| {
                // Title, totals and close.
                panel.spawn(Node { align_items: AlignItems::Center, column_gap: px(12), ..default() }).with_children(|head| {
                    head.spawn(ui::icon(icons.get(Icon::Trade), 40.0));
                    head.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: px(2), ..default() }).with_children(|t| {
                        t.spawn(ui::heading("Trade Routes", 26.0, palette::ACCENT));
                        let earning = if income > 0.0 { format!("about +{income:.0} gold per turn from trade") } else { "no trade yet".into() };
                        t.spawn(ui::text(
                            format!("Merchants {} / {}  ·  {earning}", mine.len(), game.trade_capacity(me)),
                            14.0,
                            palette::TEXT_DIM,
                        ));
                    });
                    head.spawn((
                        ui::icon_button(icons.get(Icon::Close), 30.0, Tooltip::title("Close (Esc)")),
                        observe(|_: On<Activate>, mut screen: ResMut<MerchantScreen>| *screen = MerchantScreen::default()),
                    ));
                });
                panel
                    .spawn((
                        Node { flex_direction: FlexDirection::Column, row_gap: px(6), overflow: Overflow::scroll_y(), padding: UiRect::right(px(6)), ..default() },
                        ScrollPosition::default(),
                    ))
                    .with_children(|body| {
                        match choosing {
                            Some(unit) if screen.moving => {
                                if let Some(home) = game.merchant_home(unit) {
                                    transfer_choice(body, game, me, &icons, unit, home, true);
                                }
                            }
                            Some(unit) => route_choice(body, game, me, &icons, unit),
                            None => {}
                        }
                        merchant_list(body, game, me, &icons, &routes, choosing);
                        foreign_list(body, game, me, &icons);
                    });
            });
        });
}

/// The routes a merchant could take, best paying first; clicking one sends it.
fn route_choice(body: &mut ChildSpawnerCommands, game: &Game, me: PlayerId, icons: &Icons, unit: UnitId) {
    let Some(home) = game.merchant_home(unit) else { return };
    body.spawn(ui::section(&format!("Choose a route from {}", game.cities[&home].name)));
    let current = match game.units[&unit].order {
        Order::Trade(r) => Some(r.to),
        _ => None,
    };
    let offers = game.route_offers(unit);
    if offers.is_empty() {
        // Cities it could trade with, if only it could get there.
        let partners = game.cities.keys().any(|&c| game.route_problem(unit, c).is_none());
        let name = &game.cities[&home].name;
        body.spawn(ui::hint(&if partners {
            format!("No way from {name} to any city it could trade with: seas it can't cross yet, mountains, ice or the land of civs you're at war with are in the way.")
        } else {
            "No city to trade with yet. Merchants trade with your other cities (at least 3 tiles apart) and with the cities of civs you've met and are at peace with, if they can get there.".to_string()
        }));
        transfer_choice(body, game, me, icons, unit, home, false);
        return;
    }
    body.spawn(ui::text(
        format!("A merchant goes back and forth between {} and the city you pick, as long as you let it. Every trip there sells goods for gold, and trade with your own cities brings {0} food and production too. With Repeat Route off (in its orders) it takes the best paying route again each time it's home.", game.cities[&home].name),
        13.0,
        palette::TEXT_DIM,
    ));
    let best = offers[0].worth_per_turn();
    for offer in offers {
        let to = &game.cities[&offer.to];
        let civ = game.civ(to.owner);
        let active = current == Some(offer.to);
        let tip = Tooltip::new(format!("Trade with {}", to.name), breakdown(game, me, &offer));
        body.spawn((
            ui::base_button(percent(100), px(54)),
            ui::ButtonState { active, disabled: false },
            tip,
            observe(
                move |_: On<Activate>,
                      mut session: ResMut<Session>,
                      mut screen: ResMut<MerchantScreen>,
                      mut notice: ResMut<Notice>,
                      mut out: MessageWriter<GameEvent>| {
                    match session.command(Command::Trade { unit, to: offer.to }, &mut out) {
                        Ok(()) => *screen = MerchantScreen::default(),
                        Err(e) => notice.show(format!("Can't trade there: {e}.")),
                    }
                },
            ),
        ))
        .with_children(|row| {
            row.spawn((Node { width: percent(100), align_items: AlignItems::Center, column_gap: px(12), ..default() }, Pickable::IGNORE)).with_children(
                |row| {
                    row.spawn((crate::frontend::civ_badge(icons, Some(civ), 34.0), Pickable::IGNORE));
                    row.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: px(1), ..default() }, Pickable::IGNORE))
                        .with_children(|c| {
                            c.spawn((ui::bold(to.name.clone(), 16.0, palette::TEXT), Pickable::IGNORE));
                            let whose = if offer.foreign { format!("{} · abroad", civ.name) } else { "Your city".into() };
                            let size = format!("{whose} · size {} · {} tiles", to.population, game.map.grid.distance(game.cities[&offer.home].tile, to.tile));
                            c.spawn((ui::text(size, 12.5, palette::TEXT_DIM), Pickable::IGNORE));
                        });
                    let cell = |row: &mut ChildSpawnerCommands, icon: Icon, text: String, color: Color, width: f32| {
                        row.spawn((Node { width: px(width), align_items: AlignItems::Center, column_gap: px(5), ..default() }, Pickable::IGNORE))
                            .with_children(|c| {
                                c.spawn(ui::icon(icons.get(icon), 18.0));
                                c.spawn((ui::bold(text, 14.5, color), Pickable::IGNORE));
                            });
                    };
                    cell(row, Icon::Turns, turns(offer.turns), palette::TEXT, 92.0);
                    cell(row, Icon::Gold, format!("+{} / trip", offer.value.gold), palette::GOLD, 108.0);
                    // Science from abroad, or food and production at home.
                    row.spawn((Node { width: px(112), align_items: AlignItems::Center, column_gap: px(5), ..default() }, Pickable::IGNORE))
                        .with_children(|c| {
                            let mut part = |icon: Icon, n: i32, color: Color| {
                                c.spawn(ui::icon(icons.get(icon), 18.0));
                                c.spawn((ui::bold(format!("+{n}"), 14.5, color), Pickable::IGNORE));
                            };
                            let v = offer.value;
                            if v.food > 0 || v.production > 0 {
                                part(Icon::Food, v.food, palette::FOOD);
                                part(Icon::Production, v.production, palette::PRODUCTION);
                            } else if v.science > 0 {
                                part(Icon::Science, v.science, palette::SCIENCE);
                            } else {
                                c.spawn((ui::bold("–", 14.5, palette::TEXT_DIM), Pickable::IGNORE));
                            }
                        });
                    let rate = offer.worth_per_turn();
                    let color = if rate >= best * 0.999 { palette::ACCENT } else { palette::TEXT };
                    cell(row, Icon::Trade, format!("{rate:.1} / turn"), color, 100.0);
                },
            );
        });
    }
}

/// Our other cities the merchant could move to at once (see
/// `Game::transfer`), with the best route from each; clicking one moves it
/// and opens its route choice from there. `asked`: opened by Move to City
/// (otherwise shown when it has no route from home).
fn transfer_choice(body: &mut ChildSpawnerCommands, game: &Game, me: PlayerId, icons: &Icons, unit: UnitId, home: CityId, asked: bool) {
    let mut cities: Vec<_> = game.cities.values().filter(|c| c.owner == me && c.id != home).collect();
    if cities.is_empty() {
        if asked {
            body.spawn(ui::section(&format!("Move the merchant from {}", game.cities[&home].name)));
            body.spawn(ui::hint("You have no other city to move it to."));
        }
        return;
    }
    cities.sort_by(|a, b| a.name.cmp(&b.name));
    body.spawn(ui::section(&if asked {
        format!("Move the merchant from {} to", game.cities[&home].name)
    } else {
        "Or move it to another city".into()
    }));
    body.spawn(ui::text(
        "It gets there at once and trades from there, but its route ends and its turn is over.",
        13.0,
        palette::TEXT_DIM,
    ));
    for c in cities {
        let city = c.id;
        let problem = game.transfer_problem(unit, city);
        let best = game.best_route_from(unit, city, |_| true);
        let body_text = match (problem, best) {
            (Some(why), _) => format!("Can't move there: {why}."),
            (None, Some(o)) => format!(
                "Move this merchant to {} at once. Its best route from there: {}, worth {:.1} gold a turn.",
                c.name,
                city_label(game, me, o.to),
                o.worth_per_turn()
            ),
            (None, None) => format!("Move this merchant to {} at once. It has no route from there yet.", c.name),
        };
        body.spawn((
            ui::base_button(percent(100), px(44)),
            ui::ButtonState { active: false, disabled: problem.is_some() },
            Tooltip::new(format!("Move to {}", c.name), body_text),
            observe(
                move |_: On<Activate>,
                      mut session: ResMut<Session>,
                      mut screen: ResMut<MerchantScreen>,
                      mut notice: ResMut<Notice>,
                      mut out: MessageWriter<GameEvent>| {
                    match session.command(Command::Transfer { unit, city }, &mut out) {
                        // Stay open, to pick a route from the new home.
                        Ok(()) => screen.choose(unit),
                        Err(e) => notice.show(format!("Can't move there: {e}.")),
                    }
                },
            ),
        ))
        .with_children(|row| {
            row.spawn((Node { width: percent(100), align_items: AlignItems::Center, column_gap: px(12), ..default() }, Pickable::IGNORE)).with_children(
                |row| {
                    row.spawn(ui::icon(icons.get(Icon::Merchant), 24.0));
                    row.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: px(1), ..default() }, Pickable::IGNORE))
                        .with_children(|col| {
                            col.spawn((ui::bold(c.name.clone(), 15.0, palette::TEXT), Pickable::IGNORE));
                            let line = match best {
                                Some(o) => format!("size {} · best route: {}", c.population, city_label(game, me, o.to)),
                                None => format!("size {} · no route from there", c.population),
                            };
                            col.spawn((ui::text(line, 12.5, palette::TEXT_DIM), Pickable::IGNORE));
                        });
                    if let Some(o) = best {
                        row.spawn((ui::stat(icons, Icon::Trade, format!("{:.1} / turn", o.worth_per_turn()), 14.0, palette::TEXT), Pickable::IGNORE));
                    }
                },
            );
        });
    }
}

/// Every merchant of ours: its route and what it earns, with buttons to
/// find it on the map or give it a new route.
fn merchant_list(
    body: &mut ChildSpawnerCommands,
    game: &Game,
    me: PlayerId,
    icons: &Icons,
    routes: &[MerchantRoute],
    choosing: Option<UnitId>,
) {
    body.spawn(ui::section("Your merchants"));
    if routes.is_empty() {
        let tech = game.rules.unit_type_id("merchant").and_then(|u| game.rules.unit_tech(u));
        let how = match tech {
            Some(t) if !game.knows(me, t) => format!("Research {} to train Merchants in your cities.", game.rules.tech(t).name),
            _ => "Train Merchants in your cities: each city supports one, two with a Market.".into(),
        };
        body.spawn(ui::hint(&how));
        return;
    }
    for &(unit, route) in routes {
        let u = &game.units[&unit];
        let (line, status, color) = match route {
            Some((r, _)) => {
                let goal = if r.back { r.home } else { r.to };
                let left = game.map.grid.distance(u.tile, game.cities.get(&goal).map_or(u.tile, |c| c.tile));
                let status = if left <= 1 {
                    if r.back { "Loading goods at home".to_string() } else { "Selling goods".to_string() }
                } else if r.back {
                    format!("Heading home · {left} tiles to go")
                } else {
                    format!("On the way · {left} tiles to go")
                };
                (format!("{}  →  {}", city_label(game, me, r.home), city_label(game, me, r.to)), status, palette::TEXT)
            }
            None => ("No route".into(), "Waiting for a route".into(), palette::ACCENT),
        };
        let pays = route.and_then(|(_, o)| o);
        body.spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(12),
                padding: UiRect::axes(px(10), px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(if choosing == Some(unit) { palette::PANEL_LIGHT } else { Color::srgba(0.0, 0.0, 0.0, 0.2) }),
            BorderColor::all(palette::PANEL_BORDER.with_alpha(0.5)),
        ))
        .with_children(|row| {
            row.spawn(ui::icon(icons.get(Icon::Merchant), 30.0));
            row.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: px(1), ..default() }).with_children(|c| {
                c.spawn(ui::bold(line, 15.0, color));
                c.spawn(ui::text(status, 12.5, palette::TEXT_DIM));
            });
            if let Some(o) = pays {
                row.spawn(ui::stat(icons, Icon::Gold, format!("+{} / trip · {:.1} / turn", o.value.gold, o.gold_per_turn()), 14.0, palette::GOLD));
                if o.value.food > 0 || o.value.production > 0 {
                    row.spawn(ui::stat(icons, Icon::Food, format!("+{}", o.value.food), 14.0, palette::FOOD));
                    row.spawn(ui::stat(icons, Icon::Production, format!("+{}", o.value.production), 14.0, palette::PRODUCTION));
                }
            }
            row.spawn((
                ui::base_button(px(70), px(32)),
                Tooltip::new("Show", "Select this merchant and look at it on the map."),
                children![(Text::new("Show"), ui::font(14.0), TextColor(palette::TEXT), Pickable::IGNORE)],
                observe(
                    move |_: On<Activate>,
                          session: Res<Session>,
                          mut selected: ResMut<Selected>,
                          mut rig: ResMut<crate::camera::CameraRig>,
                          mut screen: ResMut<MerchantScreen>| {
                        if let Some(u) = session.game.units.get(&unit) {
                            selected.0 = Some(unit);
                            rig.focus = crate::layout::tile_center(&session.game.map.grid, u.tile);
                            *screen = MerchantScreen::default();
                        }
                    },
                ),
            ));
            let label = if route.is_some() { "New route" } else { "Choose route" };
            row.spawn((
                ui::base_button(px(130), px(32)),
                ui::ButtonState { active: choosing == Some(unit), disabled: false },
                Tooltip::new(label, "Pick a city for this merchant to trade with, from its nearest city."),
                children![(Text::new(label), ui::font(14.0), TextColor(palette::ACCENT), Pickable::IGNORE)],
                observe(move |_: On<Activate>, mut screen: ResMut<MerchantScreen>| screen.choose(unit)),
            ));
        });
    }
}

/// Other civs' merchants trading with our cities, and what they bring us.
fn foreign_list(body: &mut ChildSpawnerCommands, game: &Game, me: PlayerId, icons: &Icons) {
    let visiting: Vec<(PlayerId, Route)> = game
        .units
        .values()
        .filter(|u| u.owner != me && game.has_met(me, u.owner))
        .filter_map(|u| match u.order {
            Order::Trade(r) if game.cities.get(&r.to).is_some_and(|c| c.owner == me) => Some((u.owner, r)),
            _ => None,
        })
        .collect();
    if visiting.is_empty() {
        return;
    }
    body.spawn(ui::section("Foreign merchants in your cities"));
    for (owner, r) in visiting {
        let value = game.trip_gold(owner, r.home, r.to);
        body.spawn(Node { align_items: AlignItems::Center, column_gap: px(10), padding: UiRect::horizontal(px(10)), ..default() }).with_children(|row| {
            row.spawn(crate::frontend::civ_badge(icons, Some(game.civ(owner)), 26.0));
            row.spawn((
                ui::text(format!("{}  →  {}", city_label(game, me, r.home), city_label(game, me, r.to)), 14.0, palette::TEXT),
                Node { flex_grow: 1.0, ..default() },
            ));
            row.spawn(ui::stat(icons, Icon::Gold, format!("+{} / trip for you", value.partner_gold), 14.0, palette::GOLD));
        });
    }
}
