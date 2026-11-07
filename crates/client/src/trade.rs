//! Trade: the negotiating table with another civ, opened from its panel (see
//! `diplomacy.rs`) or when a computer player proposes a deal to us. You put
//! together what each side gives and see whether they would accept before
//! proposing. If they want more, they propose back what they would accept;
//! their offer can be accepted, refused, or changed into a counter-offer of
//! your own, as many rounds as you like.

use crate::icons::{Icon, Icons};
use crate::loading::InGameSet;
use crate::play::{GameEvent, NewsKind, Notice, Session};
use crate::ui::{self, palette, ButtonState, Tooltip};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use sim::diplomacy::{Deal, Item, OPEN_BORDERS_TURNS, PAYMENT_TURNS, TRUCE_TURNS};
use sim::game::{Command, Event, Game, PlayerId};
use sim::resource::RESOURCE_TURNS;

/// The deal being put together, and with whom (`None`: the screen is closed).
#[derive(Resource, Default)]
pub struct TradeScreen {
    pub with: Option<PlayerId>,
    /// From our side: `give` is ours, `get` theirs.
    pub deal: Deal,
    /// The deal on the table is their proposal, unchanged (we may accept it).
    theirs: bool,
    /// When their proposal answers ours: what we proposed (to show what they changed).
    before: Option<Deal>,
    /// What happened to the last proposal.
    answer: Option<String>,
}

impl TradeScreen {
    /// Opens the table with `p`: their standing proposal if they made one,
    /// otherwise a fresh deal (with peace in it when at war).
    pub fn open(&mut self, game: &Game, me: PlayerId, p: PlayerId) {
        *self = match game.proposal(p, me) {
            Some(prop) => TradeScreen { with: Some(p), deal: prop.deal.reversed(), theirs: true, ..default() },
            None => TradeScreen { with: Some(p), deal: Deal { peace: game.at_war(me, p), ..default() }, ..default() },
        };
    }

    /// The deal was changed: it's ours to propose now.
    fn edited(&mut self) {
        self.theirs = false;
        self.before = None;
        self.answer = None;
    }
}

#[derive(Component)]
struct TradeRoot;

pub struct TradePlugin;

impl Plugin for TradePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TradeScreen>()
            .add_systems(Update, (reset, announce, refresh_trade).chain().after(crate::play::PlaySet).in_set(InGameSet));
    }
}

/// A new game starts with nothing open. Dev aid: FOURX_TRADE=1 opens the
/// trade screen with the first rival (met, at war, with some gold on both
/// sides); FOURX_TRADE=incoming has that rival propose peace and gold instead;
/// FOURX_TRADE=counter makes the war 20 turns old with the rival much
/// stronger, and puts peace and an alliance on the table (proposing it brings
/// a counter-offer asking for gold).
fn reset(mut session: ResMut<Session>, mut trade: ResMut<TradeScreen>) {
    if !session.is_added() {
        return;
    }
    *trade = TradeScreen::default();
    let Ok(mode) = std::env::var("FOURX_TRADE") else { return };
    let me = session.me;
    let them = (me + 1) % session.game.players.len() as PlayerId;
    let game = &mut session.game;
    game.players[me as usize].met[them as usize] = true;
    game.players[them as usize].met[me as usize] = true;
    game.players[me as usize].gold = 240;
    game.players[them as usize].gold = 180;
    let _ = game.apply(me, Command::DeclareWar { on: them });
    if mode == "counter" {
        for (a, b) in [(me, them), (them, me)] {
            game.players[a as usize].relations[b as usize].war_since = Some(0);
        }
        game.turn += 20;
        let at = game.units.values().find(|u| u.owner == them).map(|u| u.tile);
        if let (Some(at), Some(warrior)) = (at, game.rules.unit_type_id("warrior")) {
            for _ in 0..10 {
                game.spawn_unit(them, warrior, at);
            }
        }
        trade.open(game, me, them);
        trade.deal.alliance = true;
    } else if mode == "incoming" {
        let deal = Deal { peace: true, give: vec![Item::Gold(60), Item::GoldPerTurn(3)], ..default() };
        if game.apply(them, Command::ProposeDeal { to: me, deal }).is_ok() {
            trade.open(game, me, them);
        }
    } else {
        trade.open(game, me, them);
        trade.deal.give.push(Item::Gold(50));
    }
}

/// News of deals; deals proposed to us open the table.
fn announce(session: Res<Session>, mut events: MessageReader<GameEvent>, mut notice: ResMut<Notice>, mut trade: ResMut<TradeScreen>) {
    let game = &session.game;
    let me = session.me;
    let name = |p: PlayerId| game.civ(p).name.clone();
    let (turn, kind) = (game.turn, NewsKind::Diplomacy);
    for GameEvent(e) in events.read() {
        match e {
            // A counter-offer at the open table is already on it.
            Event::DealProposed { from, to } if *to == me && trade.with != Some(*from) => trade.open(game, me, *from),
            Event::DealMade { from, to, deal } if *from == me || *to == me => {
                let other = if *from == me { *to } else { *from };
                let what = if deal.peace { format!("Peace with {}", name(other)) } else { format!("Deal made with {}", name(other)) };
                notice.news(turn, kind, Icon::Check, what, None);
            }
            Event::DealMade { from, to, deal } if deal.peace && game.has_met(me, *from) && game.has_met(me, *to) => {
                notice.news(turn, kind, Icon::Banner, format!("{} and {} made peace", name(*from), name(*to)), None);
            }
            Event::CityTraded { city, from, to } => {
                let Some(c) = game.cities.get(city) else { continue };
                if *to == me {
                    notice.news(turn, NewsKind::Cities, Icon::FoundCity, format!("{} is yours", c.name), Some(c.tile));
                } else if *from == me {
                    notice.news(turn, NewsKind::Cities, Icon::FoundCity, format!("{} now belongs to {}", c.name, name(*to)), Some(c.tile));
                }
            }
            _ => {}
        }
    }
}

/// One line describing an item.
fn describe(game: &Game, item: Item) -> String {
    match item {
        Item::Gold(g) => format!("{g} gold"),
        Item::GoldPerTurn(g) => format!("{g} gold per turn for {PAYMENT_TURNS} turns"),
        Item::OpenBorders => format!("open borders for {OPEN_BORDERS_TURNS} turns"),
        Item::Resource(r) => format!("{} for {RESOURCE_TURNS} turns", game.rules.resource(r).name),
        Item::City(c) => game.cities.get(&c).map_or("a city".into(), |c| format!("{} (population {})", c.name, c.population)),
    }
}

fn gold(items: &[Item]) -> i32 {
    items.iter().map(|i| if let Item::Gold(g) = i { *g } else { 0 }).sum()
}

fn per_turn(items: &[Item]) -> i32 {
    items.iter().map(|i| if let Item::GoldPerTurn(g) = i { *g } else { 0 }).sum()
}

/// Sets one side's gold (`per_turn`: its payment per turn) to `amount`.
fn set_gold(items: &mut Vec<Item>, amount: i32, per_turn: bool) {
    items.retain(|i| !matches!((i, per_turn), (Item::Gold(_), false) | (Item::GoldPerTurn(_), true)));
    if amount > 0 {
        items.push(if per_turn { Item::GoldPerTurn(amount) } else { Item::Gold(amount) });
    }
}

/// Which side of the deal: ours (`give`) or theirs (`get`).
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Ours,
    Theirs,
}

fn side(deal: &mut Deal, s: Side) -> &mut Vec<Item> {
    match s {
        Side::Ours => &mut deal.give,
        Side::Theirs => &mut deal.get,
    }
}

fn refresh_trade(
    mut commands: Commands,
    session: Res<Session>,
    trade: Res<TradeScreen>,
    icons: Res<Icons>,
    roots: Query<Entity, With<TradeRoot>>,
) {
    if !trade.is_changed() && !session.is_changed() {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(them) = trade.with else { return };
    let game = &session.game;
    let me = session.me;
    let deal = &trade.deal;
    let problem = game.deal_problem(me, them, deal);
    let name = game.civ(them).name.clone();
    let (color, verdict) = match problem {
        _ if trade.theirs => (palette::ACCENT, format!("This is {name}'s offer. Accept it, refuse it, or change anything to make your own.")),
        _ if deal.is_empty() => (palette::TEXT_DIM, "Pick what each side offers from the outer columns.".to_string()),
        Some(why) => (palette::DANGER, format!("Can't propose this: {why}.")),
        None => match ai::diplomacy::verdict(game, them, me, deal) {
            Ok(()) => (palette::FOOD, format!("{name} would accept this deal.")),
            Err(r) => (palette::DANGER, format!("{name} would refuse. {}", r.reason())),
        },
    };
    commands.spawn((TradeRoot, backdrop(18))).with_children(|root| {
        root.spawn((
            Node {
                width: percent(94),
                max_width: px(1320),
                height: percent(88),
                flex_direction: FlexDirection::Column,
                row_gap: px(14),
                padding: UiRect::all(px(22)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            BackgroundColor(palette::PANEL),
            BorderColor::all(palette::PANEL_BORDER),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), px(0), px(6), px(4), px(24)),
        ))
        .with_children(|panel| {
            header(panel, game, &icons, me, them);
            panel.spawn(Node { width: percent(100), flex_grow: 1.0, min_height: px(0), column_gap: px(14), ..default() }).with_children(|cols| {
                available(cols, game, me, them, deal, Side::Ours, "You could offer".into());
                let before = trade.before.as_ref();
                offer(cols, game, deal, before, Side::Ours, "Your offer".into());
                offer(cols, game, deal, before, Side::Theirs, format!("{name}'s offer"));
                available(cols, game, me, them, deal, Side::Theirs, format!("{name} could offer"));
            });
            // The verdict, and what to do about it.
            panel
                .spawn((
                    Node { width: percent(100), padding: UiRect::axes(px(16), px(12)), border_radius: BorderRadius::all(px(6)), ..default() },
                    BackgroundColor(color.with_alpha(0.12)),
                ))
                .with_children(|v| {
                    v.spawn(ui::bold(verdict, 17.0, color));
                });
            if let Some(answer) = &trade.answer {
                panel.spawn(ui::text(answer.clone(), 14.0, palette::TEXT));
            }
            panel.spawn(Node { width: percent(100), column_gap: px(12), justify_content: JustifyContent::End, ..default() }).with_children(|row| {
                let plain = ButtonState::default();
                let button = |label: &str, width: f32, state: ButtonState, color: Color| {
                    let primary = state.active || state.disabled;
                    (
                        ui::base_button(px(width), px(44)),
                        state,
                        children![(Text::new(label), if primary { ui::font_bold(18.0) } else { ui::font(18.0) }, TextColor(color), Pickable::IGNORE)],
                    )
                };
                if trade.theirs {
                    row.spawn((
                        button("Refuse", 160.0, plain, palette::DANGER),
                        observe(move |_: On<Activate>, mut t: ResMut<TradeScreen>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>| {
                            let _ = session.command(Command::RefuseDeal { from: them }, &mut out);
                            t.edited();
                            t.answer = Some("You refused their offer. Change it and propose your own, or close the table.".into());
                        }),
                    ));
                } else {
                    row.spawn((
                        button("Clear", 160.0, plain, palette::TEXT),
                        observe(move |_: On<Activate>, mut t: ResMut<TradeScreen>, session: Res<Session>| {
                            let me = session.me;
                            t.open(&session.game, me, them);
                            t.theirs = false;
                            t.deal = Deal { peace: session.game.at_war(me, them), ..default() };
                        }),
                    ));
                }
                row.spawn((
                    button("Close", 160.0, plain, palette::TEXT),
                    observe(|_: On<Activate>, mut t: ResMut<TradeScreen>| *t = TradeScreen::default()),
                ));
                if trade.theirs {
                    row.spawn((
                        button("Accept Their Offer", 240.0, ButtonState { active: true, ..default() }, palette::FOOD),
                        Tooltip::new("Accept", "Make this deal now."),
                        observe(move |_: On<Activate>, mut t: ResMut<TradeScreen>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>| {
                            match session.command(Command::AcceptDeal { from: them }, &mut out) {
                                Ok(()) => *t = TradeScreen::default(),
                                Err(e) => {
                                    t.edited();
                                    t.answer = Some(format!("Couldn't accept: {e}."));
                                }
                            }
                        }),
                    ));
                    return;
                }
                let ok = problem.is_none() && !deal.is_empty();
                row.spawn((
                    button("Propose Deal", 220.0, ButtonState { active: ok, disabled: !ok }, if ok { palette::ACCENT } else { palette::TEXT_DIM }),
                    Tooltip::new("Propose", "Offer this deal. They answer straight away, and if they want more, they say what."),
                    observe(move |_: On<Activate>, mut t: ResMut<TradeScreen>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>| {
                        if !ok {
                            return;
                        }
                        let me = session.me;
                        if let Err(e) = session.command(Command::ProposeDeal { to: them, deal: t.deal.clone() }, &mut out) {
                            t.answer = Some(format!("Couldn't propose: {e}."));
                            return;
                        }
                        let answer = ai::diplomacy::answer(&mut session.game, them, me, true);
                        Session::publish(answer.events, &mut out);
                        match (answer.refusal, answer.counter) {
                            (None, _) => *t = TradeScreen::default(),
                            (Some(_), Some(counter)) => {
                                t.before = Some(std::mem::replace(&mut t.deal, counter));
                                t.theirs = true;
                                t.answer = Some("They want more than that. Their counter-offer is on the table, with their changes highlighted.".into());
                            }
                            (Some(r), None) => t.answer = Some(format!("They refused. {}", r.reason())),
                        }
                    }),
                ));
            });
        });
    });
}

/// Both civs side by side, with their treasuries, and where they stand.
fn header(panel: &mut ChildSpawnerCommands, game: &Game, icons: &Icons, me: PlayerId, them: PlayerId) {
    let civ_block = |p: PlayerId, right: bool| {
        let civ = game.civ(p);
        let gold = game.players[p as usize].gold;
        (civ.clone(), gold, right)
    };
    let status = if game.at_war(me, them) {
        format!("At war for {} turns", game.war_length(me, them))
    } else if game.allied(me, them) {
        "Allies".to_string()
    } else if game.truce_left(me, them) > 0 {
        format!("At peace (treaty: {} turns)", game.truce_left(me, them))
    } else {
        "At peace".to_string()
    };
    panel.spawn(Node { width: percent(100), align_items: AlignItems::Center, ..default() }).with_children(|row| {
        for (civ, gold, right) in [civ_block(me, false), civ_block(them, true)] {
            if right {
                row.spawn(Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }).with_children(|mid| {
                    mid.spawn(ui::heading("TRADE", 30.0, palette::ACCENT));
                    mid.spawn(ui::text(status.clone(), 14.0, palette::TEXT_DIM));
                });
            }
            row.spawn(Node { align_items: AlignItems::Center, column_gap: px(12), flex_direction: if right { FlexDirection::RowReverse } else { FlexDirection::Row }, ..default() })
                .with_children(|b| {
                    b.spawn(crate::frontend::civ_badge(icons, Some(&civ), 58.0));
                    b.spawn(Node { flex_direction: FlexDirection::Column, align_items: if right { AlignItems::End } else { AlignItems::Start }, row_gap: px(2), ..default() })
                        .with_children(|names| {
                            names.spawn(ui::heading(civ.name.clone(), 22.0, palette::TEXT));
                            names.spawn(ui::text(civ.leader.clone(), 13.5, palette::TEXT_DIM));
                            names.spawn(ui::stat(icons, Icon::Gold, gold.to_string(), 14.0, palette::GOLD));
                        });
                });
        }
    });
}

/// A column of the trade screen: a framed box with a title.
fn column_box(title: String, width: f32, highlight: bool) -> impl Bundle {
    (
        Node {
            width: percent(width),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::all(px(12)),
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::all(px(8)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        ScrollPosition::default(),
        BackgroundColor(if highlight { palette::PANEL_LIGHT } else { Color::srgba(0.0, 0.0, 0.0, 0.2) }),
        BorderColor::all(if highlight { palette::ACCENT.with_alpha(0.6) } else { palette::PANEL_BORDER.with_alpha(0.5) }),
        children![(ui::heading(title.to_uppercase(), 15.0, palette::ACCENT), Node { margin: UiRect::bottom(px(6)), ..default() })],
    )
}

/// A button that adds something to the deal, or a dimmed one explaining why it can't.
fn add_button(parent: &mut ChildSpawnerCommands, label: String, why_not: Option<&str>, tip: String, add: impl Fn(&mut Deal) + Send + Sync + 'static) {
    let disabled = why_not.is_some();
    let tip = Tooltip::new(label.clone(), why_not.map_or(tip, |w| format!("Not possible: {w}.")));
    parent.spawn((
        ui::base_button(percent(100), px(36)),
        ButtonState { disabled, ..default() },
        tip,
        children![(
            Node { width: percent(100), justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() },
            Pickable::IGNORE,
            children![
                (Text::new(label), ui::font(15.0), TextColor(if disabled { palette::TEXT_DIM } else { palette::TEXT }), Pickable::IGNORE),
                (Text::new(if disabled { "" } else { "+" }), ui::font_bold(18.0), TextColor(palette::ACCENT), Pickable::IGNORE),
            ],
        )],
        observe(move |_: On<Activate>, mut t: ResMut<TradeScreen>| {
            if !disabled {
                add(&mut t.deal);
                t.edited();
            }
        }),
    ));
}

/// What one side could still put into the deal.
fn available(cols: &mut ChildSpawnerCommands, game: &Game, me: PlayerId, them: PlayerId, deal: &Deal, s: Side, title: String) {
    let (giver, taker) = if s == Side::Ours { (me, them) } else { (them, me) };
    let items = if s == Side::Ours { &deal.give } else { &deal.get };
    let war = game.at_war(me, them);
    let allied = game.allied(me, them);
    cols.spawn(column_box(title, 22.0, false)).with_children(|col| {
        col.spawn(ui::text("Treaties", 13.0, palette::TEXT_DIM));
        let peace_why = if !war { Some("you're at peace") } else if deal.peace { Some("already in the deal") } else { None };
        add_button(col, "Peace treaty".into(), peace_why, format!("Both sides: the war ends, then no war for {TRUCE_TURNS} turns."), |d| d.peace = true);
        let alliance_why = if allied { Some("you're allies already") } else if deal.alliance { Some("already in the deal") } else { None };
        add_button(col, "Alliance".into(), alliance_why, "Both sides: borders open both ways, and no war between you.".into(), |d| d.alliance = true);
        let borders_why = if allied || deal.alliance {
            Some("allies' borders are open")
        } else if game.borders_open(giver, taker) {
            Some("these borders are open already")
        } else if items.contains(&Item::OpenBorders) {
            Some("already in the deal")
        } else {
            None
        };
        let whose = if s == Side::Ours { "Your" } else { "Their" };
        add_button(col, "Open borders".into(), borders_why, format!("{whose} territory opens to the other side's units for {OPEN_BORDERS_TURNS} turns."), move |d| {
            side(d, s).push(Item::OpenBorders)
        });

        col.spawn((ui::text("Gold", 13.0, palette::TEXT_DIM), Node { margin: UiRect::top(px(8)), ..default() }));
        let treasury = game.players[giver as usize].gold;
        let gold_why = if treasury <= 0 { Some("the treasury is empty") } else if gold(items) > 0 { Some("already in the deal: change the amount there") } else { None };
        let first = treasury.min(50);
        add_button(col, format!("Gold ({treasury} available)"), gold_why, "A sum of gold, paid now.".into(), move |d| set_gold(side(d, s), first, false));
        let per_why = if per_turn(items) > 0 { Some("already in the deal: change the amount there") } else { None };
        add_button(col, "Gold per turn".into(), per_why, format!("Gold every turn for {PAYMENT_TURNS} turns. War stops it."), move |d| set_gold(side(d, s), 5, true));

        col.spawn((ui::text("Resources", 13.0, palette::TEXT_DIM), Node { margin: UiRect::top(px(8)), ..default() }));
        let counts = game.resource_counts(taker);
        let tradeable: Vec<_> = (0..game.rules.resources.resources.len() as sim::rules::ResourceId)
            .filter(|&r| game.rules.resource(r).kind != sim::rules::ResourceKind::Bonus && game.spare_resource(giver, r) > 0)
            .collect();
        if tradeable.is_empty() {
            col.spawn(ui::hint("No luxuries or strategic resources to spare."));
        }
        for r in tradeable {
            let def = game.rules.resource(r);
            let spare = game.spare_resource(giver, r);
            let why = items.contains(&Item::Resource(r)).then_some("already in the deal");
            let has = if counts[r as usize] > 0 { " The other side has some already." } else { "" };
            let tip = format!(
                "One of {spare} for {RESOURCE_TURNS} turns. {}{has}",
                crate::improvements::resource_use(game, r)
            );
            add_button(col, format!("{} ({spare})", def.name), why, tip, move |d| side(d, s).push(Item::Resource(r)));
        }

        col.spawn((ui::text("Cities", 13.0, palette::TEXT_DIM), Node { margin: UiRect::top(px(8)), ..default() }));
        let cities: Vec<_> = game
            .cities
            .values()
            .filter(|c| c.owner == giver && game.players[me as usize].explored[c.tile as usize] && !items.contains(&Item::City(c.id)))
            .collect();
        if cities.is_empty() {
            col.spawn(ui::hint(if s == Side::Ours { "No cities." } else { "No cities you know of." }));
        }
        for c in cities {
            let id = c.id;
            let why = (!game.tradeable_city(id)).then_some(if c.capital_of == Some(giver) { "a capital stays" } else { "their last city stays" });
            add_button(col, format!("{} (population {})", c.name, c.population), why, "Hand the city over, as it is.".into(), move |d| side(d, s).push(Item::City(id)));
        }

    });
}

/// What one side offers in the deal (with the treaties both sides share).
fn offer(cols: &mut ChildSpawnerCommands, game: &Game, deal: &Deal, before: Option<&Deal>, s: Side, title: String) {
    let pick = |d: &Deal| if s == Side::Ours { d.give.clone() } else { d.get.clone() };
    let items = pick(deal);
    let earlier = before.map(pick);
    // Changed since our proposal (only when there is one).
    let new = |item: &Item| earlier.as_ref().is_some_and(|e| !e.contains(item));
    cols.spawn(column_box(title, 28.0, true)).with_children(|col| {
        let both = "Both sides".to_string();
        if deal.peace {
            offer_row(col, "Peace treaty".into(), both.clone(), None, before.is_some_and(|b| !b.peace), |d| d.peace = false);
        }
        if deal.alliance {
            offer_row(col, "Alliance".into(), both, None, before.is_some_and(|b| !b.alliance), |d| d.alliance = false);
        }
        if items.is_empty() && !deal.peace && !deal.alliance {
            col.spawn(ui::hint("Nothing."));
        }
        for item in items.iter().copied() {
            let remove = move |d: &mut Deal| side(d, s).retain(|&i| i != item);
            let changed = new(&item);
            match item {
                Item::Gold(g) => offer_row(col, format!("{g} gold"), "Paid now".into(), Some((g, false, s)), changed, remove),
                Item::GoldPerTurn(g) => offer_row(
                    col,
                    format!("{g} gold per turn"),
                    format!("For {PAYMENT_TURNS} turns ({} in all)", g * PAYMENT_TURNS as i32),
                    Some((g, true, s)),
                    changed,
                    remove,
                ),
                Item::OpenBorders => offer_row(col, "Open borders".into(), format!("For {OPEN_BORDERS_TURNS} turns"), None, changed, remove),
                Item::Resource(r) => {
                    offer_row(col, game.rules.resource(r).name.clone(), format!("For {RESOURCE_TURNS} turns"), None, changed, remove)
                }
                Item::City(_) => offer_row(col, describe(game, item), "City".into(), None, changed, remove),
            }
        }
        // What their counter-offer took out of this side.
        for item in earlier.iter().flatten().filter(|i| !items.contains(i)) {
            col.spawn(ui::text(format!("Taken out: {}", describe(game, *item)), 13.0, palette::DANGER));
        }
    });
}

/// One thing in an offer: what it is, buttons to change an amount of gold
/// (`amount`: the value, whether per turn, and whose), and a remove button.
/// `changed`: new in their counter-offer (highlighted).
fn offer_row(
    col: &mut ChildSpawnerCommands,
    label: String,
    detail: String,
    amount: Option<(i32, bool, Side)>,
    changed: bool,
    remove: impl Fn(&mut Deal) + Send + Sync + 'static,
) {
    let detail = if changed { format!("{detail} · their change") } else { detail };
    col.spawn((
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::all(px(10)),
            border: UiRect::all(px(if changed { 2.0 } else { 1.0 })),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(if changed { palette::BUTTON_ACTIVE } else { palette::BUTTON }),
        BorderColor::all(if changed { palette::ACCENT } else { palette::BUTTON_BORDER }),
    ))
    .with_children(|card| {
        card.spawn(Node { width: percent(100), align_items: AlignItems::Center, ..default() }).with_children(|top| {
            top.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, ..default() }).with_children(|t| {
                t.spawn(ui::bold(label, 16.0, palette::TEXT));
                t.spawn(ui::text(detail, 12.5, palette::TEXT_DIM));
            });
            top.spawn((
                ui::small_button("×"),
                Tooltip::title("Remove"),
                observe(move |_: On<Activate>, mut t: ResMut<TradeScreen>| {
                    remove(&mut t.deal);
                    t.edited();
                }),
            ));
        });
        let Some((value, per, s)) = amount else { return };
        let steps: &[i32] = if per { &[-5, -1, 1, 5] } else { &[-100, -10, 10, 100] };
        card.spawn(Node { column_gap: px(4), ..default() }).with_children(|row| {
            for &step in steps {
                row.spawn((
                    ui::base_button(px(50), px(28)),
                    children![(Text::new(format!("{step:+}")), ui::font(13.0), TextColor(palette::TEXT), Pickable::IGNORE)],
                    observe(move |_: On<Activate>, mut t: ResMut<TradeScreen>, session: Res<Session>| {
                        let giver = if s == Side::Ours { session.me } else { t.with.unwrap_or(0) };
                        let max = if per { i32::MAX } else { session.game.players[giver as usize].gold };
                        set_gold(side(&mut t.deal, s), (value + step).clamp(1, max.max(1)), per);
                        t.edited();
                    }),
                ));
            }
        });
    });
}

fn backdrop(z: i32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        Pickable::default(),
        GlobalZIndex(z),
    )
}
