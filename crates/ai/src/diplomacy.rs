//! Trade. A computer player values everything in a deal on one scale:
//! - **Gold** counts less the more it has (a full treasury makes each coin
//!   worth less), gold per turn a little less again (it may stop).
//! - **Cities** by what they produce over the years, more when they're on our
//!   land; giving one away costs more than getting one is worth.
//! - **Resources**: a luxury it lacks by the gold it would add in its cities
//!   while the deal lasts, a strategic resource it lacks by whether it can
//!   train the units that need it (more at war); a spare copy is worth
//!   little either way. It never hands strategic resources to someone it
//!   means to fight.
//! - **Open borders**: a little either way, but never for someone it holds a
//!   grudge against or means to fight. Getting them is worth a lot more when
//!   their land is in the way: of the city its army marches on, or of the
//!   ground its explorers can't reach (see [`Strategy::passage`]).
//! - **An alliance** by how much bigger (army and cities) the partner is and
//!   by personality: cautious leaders welcome a strong protector, proud and
//!   warlike ones value their independence, and agendas weigh in (see
//!   [`alliance_value`]). A shared enemy helps; never with someone it means
//!   to fight.
//! - **Peace** costs nothing in a war it isn't pursuing. It's worth paying for
//!   when losing (to spare the threatened cities), and it costs about the
//!   city it's marching on in a war it wants and is winning, less as the war
//!   drags on, until after [`WEARY`] turns even the winner settles for nothing.
//!
//! It accepts a deal worth at least nothing to it, never makes peace while a
//! war is young, and leaders who never forgive won't make peace with anyone
//! who took their cities. In wars it no longer wants, it proposes peace,
//! adding gold when the other side would want more (it knows what another
//! computer player would accept), proposes alliances against a shared
//! enemy, and asks for open borders when someone's land is in its way,
//! offering its own in return (and gold, if another computer player would
//! want more). It also trades for luxuries and strategic resources it
//! lacks, offering a spare one of its own or gold. Everything goes through the same commands
//! a player uses.

use sim::city::CityId;
use sim::civ::AgendaKind;
use sim::diplomacy::{Deal, Item, PAYMENT_TURNS};
use sim::game::{Command, Event, Game, PlayerId};
use sim::resource::RESOURCE_TURNS;
use sim::rules::{ResourceId, ResourceKind};

use crate::strategy::Strategy;

/// Wars last at least this many turns before peace is on the table.
const MIN_WAR: u32 = 10;
/// After this many more turns at war, even a winner makes peace for nothing.
const WEARY: u32 = 80;
/// Turns between proposals to the same player.
const ASK_AGAIN: u32 = 10;
/// A city is worth this many turns of its yields.
const CITY_TURNS: f32 = 25.0;
/// Gold per turn is worth this share of its total (it may stop early).
const PAYMENT_TRUST: f32 = 0.75;
/// Giving a city away costs this much more than its worth.
const RELUCTANCE: f32 = 1.3;
/// What open borders are worth (getting them) or cost (giving them).
const BORDERS: f32 = 30.0;
/// With this much gold, each coin is worth half as much.
const COMFORT: f32 = 500.0;

/// What one gold is worth to `me`, by how much it already has.
pub fn gold_worth(game: &Game, me: PlayerId) -> f32 {
    COMFORT / (COMFORT + game.players[me as usize].gold.max(0) as f32)
}

/// Why a computer player turns down a deal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Refusal {
    TooSoon,
    NeverForgives,
    /// They mean to fight us, or hold a grudge.
    NoFriendship,
    /// They only ally with someone stronger than themselves.
    TooWeak,
    /// The deal is worth about this much gold too little to them.
    WantMore(f32),
}

impl Refusal {
    /// The reason, as the leader would put it.
    pub fn reason(self) -> String {
        match self {
            Refusal::TooSoon => "This war has only just begun.".into(),
            Refusal::NeverForgives => "They never forgive the cities you took from them.".into(),
            Refusal::NoFriendship => "They have no wish to be friends with you.".into(),
            Refusal::TooWeak => "They only ally with the strong.".into(),
            Refusal::WantMore(gap) => format!("They want more: about {} gold's worth.", gap.ceil() as i32),
        }
    }
}

/// What a city is worth to `me`, in gold.
pub fn city_value(game: &Game, s: &Strategy, city: CityId) -> f32 {
    let c = &game.cities[&city];
    let out = game.city_output(city);
    let y = out.yields;
    let per_turn = (y.food + y.production + y.gold + out.science + out.culture) as f32;
    let value = per_turn * CITY_TURNS + c.population as f32 * 10.0;
    // Cities overseas or far away are hard to hold.
    if c.owner == s.me || s.on_our_land(game, c.tile) { value } else { value * 0.6 }
}

/// Whether `s.me` means to fight `q` or holds a grudge against them.
fn hostile_to(game: &Game, s: &Strategy, q: PlayerId) -> bool {
    s.war.is_some_and(|w| w.enemy == q) || s.grudge(game, q) > 0.0
}

fn item_value(game: &Game, s: &Strategy, q: PlayerId, item: Item, receiving: bool) -> Result<f32, Refusal> {
    Ok(match (item, receiving) {
        (Item::OpenBorders, true) => BORDERS + s.passage(game, q),
        (Item::OpenBorders, false) if hostile_to(game, s, q) => return Err(Refusal::NoFriendship),
        (Item::OpenBorders, false) => BORDERS,
        (Item::Gold(g), _) => g as f32 * gold_worth(game, s.me),
        (Item::GoldPerTurn(g), true) => (g * PAYMENT_TURNS as i32) as f32 * PAYMENT_TRUST * gold_worth(game, s.me),
        (Item::GoldPerTurn(g), false) => (g * PAYMENT_TURNS as i32) as f32 * gold_worth(game, s.me),
        (Item::City(c), true) => game.cities.get(&c).map_or(0.0, |_| city_value(game, s, c)),
        (Item::City(c), false) => game.cities.get(&c).map_or(0.0, |_| city_value(game, s, c) * RELUCTANCE),
        (Item::Resource(r), false) if game.rules.resource(r).kind == ResourceKind::Strategic && hostile_to(game, s, q) => {
            return Err(Refusal::NoFriendship)
        }
        (Item::Resource(r), receiving) => resource_value(game, s, r, receiving),
    })
}

/// Turns between looking for resources to trade for.
const RESOURCE_CHECK: u32 = 5;
/// Copies of a resource a player must have left (not given away) before
/// they're asked for one, or offer one: one to keep and one to trade.
const SPARE_COPY: i32 = 2;
/// A copy of a spare resource: worth a little to trade on.
const SPARE: f32 = 5.0;

/// What getting (or giving away) a copy of resource `r` is worth to `s.me`.
pub fn resource_value(game: &Game, s: &Strategy, r: ResourceId, receiving: bool) -> f32 {
    let have = game.resource_counts(s.me)[r as usize];
    if (receiving && have > 0) || (!receiving && have > 1) {
        return SPARE;
    }
    match game.rules.resource(r).kind {
        ResourceKind::Bonus => 0.0,
        ResourceKind::Luxury => {
            let cities = game.cities.values().filter(|c| c.owner == s.me).count() as f32;
            cities * game.rules.resources.luxury_gold as f32 * RESOURCE_TURNS as f32 * gold_worth(game, s.me)
        }
        ResourceKind::Strategic => {
            let units = (0..game.rules.units.units.len() as sim::rules::UnitTypeId).filter(|&u| game.rules.unit_resource(u) == Some(r));
            let mut usable = false;
            let mut later = false;
            let mut liking = 0u8;
            for u in units {
                let build = sim::city::Build::Unit(u);
                if game.civ_builds(s.me, build) {
                    usable |= game.has_tech_for(s.me, build);
                    later = true;
                    liking = liking.max(crate::production::role(game.rules.unit(u)).liking(&s.army));
                }
            }
            // Leaders who favour the troops it's for want it more.
            let liking = 0.5 + liking as f32 / 10.0;
            let war = s.war.is_some() || !s.threats.is_empty();
            liking
                * match (usable, later) {
                    (true, _) if war => 120.0,
                    (true, _) => 60.0,
                    (false, true) => 15.0,
                    _ => 0.0,
                }
        }
    }
}

/// A trade for a luxury or strategic resource `s.me` lacks and `q` has to
/// spare (a second copy: nobody is asked for their only one): for a spare
/// resource of ours that `q` lacks, or gold (as much as `q`, a computer
/// player, would want and it's worth to us).
fn resource_offer(game: &Game, s: &Strategy, q: PlayerId) -> Option<Deal> {
    let me = s.me;
    // Looked at every few turns: it means sizing up the other side.
    if !(game.turn + me as u32).is_multiple_of(RESOURCE_CHECK) || !game.has_met(me, q) || game.at_war(me, q) || hostile_to(game, s, q) {
        return None;
    }
    let n = game.rules.resources.resources.len() as ResourceId;
    let ours = game.resource_counts(me);
    let theirs = game.resource_counts(q);
    let wanted = (0..n)
        .filter(|&r| game.rules.resource(r).kind != ResourceKind::Bonus && ours[r as usize] <= 0 && game.spare_resource(q, r) >= SPARE_COPY)
        .map(|r| (resource_value(game, s, r, true), r))
        .filter(|&(v, _)| v >= 20.0)
        .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))?
        .1;
    let swap = (0..n).find(|&r| {
        game.rules.resource(r).kind != ResourceKind::Bonus && ours[r as usize] > 1 && theirs[r as usize] <= 0 && game.spare_resource(me, r) >= SPARE_COPY
    });
    let deal = Deal { give: swap.map(Item::Resource).into_iter().collect(), get: vec![Item::Resource(wanted)], ..Default::default() };
    if game.deal_problem(me, q, &deal).is_some() {
        return None;
    }
    if game.players[q as usize].human {
        return offer_to_human(game, s, q, deal);
    }
    let deal = counter_offer(game, q, me, &deal)?;
    deal_value_as_proposer(game, s, q, &deal).filter(|&v| v >= 0.0).map(|_| deal)
}

/// `deal` as `s.me` proposes it to the human `q`, if it's worth it to us. We
/// can't read a human's mind, but we never ask for something for nothing:
/// a deal where we'd give nothing offers half of what it's worth to us in
/// gold instead, or isn't proposed.
fn offer_to_human(game: &Game, s: &Strategy, q: PlayerId, deal: Deal) -> Option<Deal> {
    let worth = deal_value_as_proposer(game, s, q, &deal).filter(|&v| v >= 0.0)?;
    if !deal.give.is_empty() || deal.peace || deal.alliance {
        return Some(deal);
    }
    let gold = (worth * 0.5 / gold_worth(game, s.me)).min(game.players[s.me as usize].gold as f32 * 0.5) as i32;
    let deal = Deal { give: vec![Item::Gold(gold)], ..deal };
    (gold > 0 && game.deal_problem(s.me, q, &deal).is_none()).then_some(deal)
}

/// What an alliance with `q` is worth to `s.me`:
/// - **Size**: how much bigger `q` is (army and cities, averaged). A stronger
///   partner protects us, a much weaker one is a burden.
/// - **Personality**: cautious leaders (defence, low risk) value protection
///   more; proud ones (military, opportunism) their independence.
/// - **Agenda**: the Strong Take only allies upwards, the Sleeping Giant
///   likes friends, raiders want targets rather than allies, Pax Romana one
///   realm of its own, the Mercantile Rival no richer partner, and those who
///   never forgive no one who took their cities.
/// - A shared enemy is worth a lot.
fn alliance_value(game: &Game, s: &Strategy, q: PlayerId) -> Result<f32, Refusal> {
    let me = s.me;
    if hostile_to(game, s, q) || (s.agenda == AgendaKind::NeverForgive && game.relation(me, q).cities_lost > 0) {
        return Err(Refusal::NoFriendship);
    }
    let d = |v: u8| v as f32 / 10.0;
    let dr = &s.drives;
    let cities = |p: PlayerId| game.cities.values().filter(|c| c.owner == p).count().max(1) as f32;
    let army = s.power[q as usize].max(5.0) / s.power[me as usize].max(5.0);
    let bigger = ((army + cities(q) / cities(me)) / 2.0).clamp(0.25, 4.0);
    if s.agenda == AgendaKind::StrongTake && bigger < 1.0 {
        return Err(Refusal::TooWeak);
    }
    let caution = (d(dr.defense) + 1.0 - d(dr.risk)) / 2.0;
    let pride = (d(dr.military) + d(dr.opportunism)) / 2.0;
    let mut value = 80.0 * bigger.ln() * (0.5 + caution) - 40.0 - 80.0 * pride;
    if (0..game.players.len() as PlayerId).any(|o| o != q && o != me && game.at_war(me, o) && game.at_war(q, o)) {
        value += 150.0;
    }
    value += match s.agenda {
        AgendaKind::StrongTake => 40.0,
        AgendaKind::SleepingGiant => 60.0,
        AgendaKind::Plunder | AgendaKind::FlowerWars => -80.0,
        AgendaKind::PaxRomana => -60.0,
        AgendaKind::MercantileRival if game.players[q as usize].gold > game.players[me as usize].gold => -80.0,
        _ => 0.0,
    };
    Ok(value)
}

/// What peace with `q` is worth to `s.me` (negative: what it wants for it),
/// or why it won't make peace at any price.
pub fn peace_value(game: &Game, s: &Strategy, q: PlayerId) -> Result<f32, Refusal> {
    let me = s.me;
    let age = game.war_length(me, q);
    if age < MIN_WAR {
        return Err(Refusal::TooSoon);
    }
    if s.agenda == AgendaKind::NeverForgive && game.relation(me, q).cities_lost > 0 {
        return Err(Refusal::NeverForgives);
    }
    let ratio = s.power[me as usize] / s.power[q as usize].max(5.0);
    let mut value = 0.0;
    // Losing: worth part of the cities they threaten.
    if ratio < 1.0 {
        let at_risk: f32 = s.threats.keys().map(|&c| city_value(game, s, c)).sum();
        value += at_risk * (1.0 - ratio) * 0.5;
    }
    // Winning a war we want: the prize we'd give up.
    if let Some(w) = s.war.filter(|w| w.enemy == q && ratio >= 1.0) {
        let prize = game.cities.get(&w.target).map_or(0.0, |_| city_value(game, s, w.target));
        let tired = (1.0 - age.saturating_sub(MIN_WAR) as f32 / WEARY as f32).max(0.0);
        value -= prize * (ratio.min(2.0) - 0.5) * tired;
    }
    Ok(value)
}

/// What `deal`, proposed by `from` to `s.me`, is worth to `s.me` (accepted if
/// it's at least 0), or why it's refused regardless.
pub fn deal_value(game: &Game, s: &Strategy, from: PlayerId, deal: &Deal) -> Result<f32, Refusal> {
    let mut value = if deal.peace { peace_value(game, s, from)? } else { 0.0 };
    if deal.alliance {
        value += alliance_value(game, s, from)?;
    }
    for &i in &deal.give {
        value += item_value(game, s, from, i, true)?;
    }
    for &i in &deal.get {
        value -= item_value(game, s, from, i, false)?;
    }
    Ok(value)
}

/// Whether `ai` would accept `deal` from `from` now (`Ok`), or why not.
pub fn verdict(game: &Game, ai: PlayerId, from: PlayerId, deal: &Deal) -> Result<(), Refusal> {
    judge(game, &Strategy::assess(game, ai), from, deal)
}

fn judge(game: &Game, s: &Strategy, from: PlayerId, deal: &Deal) -> Result<(), Refusal> {
    let value = deal_value(game, s, from, deal)?;
    if value >= 0.0 { Ok(()) } else { Err(Refusal::WantMore(-value / gold_worth(game, s.me))) }
}

/// How a computer player answered a proposal.
#[derive(Debug, Default)]
pub struct Answer {
    pub events: Vec<Event>,
    /// Why it refused (`None`: it accepted).
    pub refusal: Option<Refusal>,
    /// The deal it proposed back instead, as `from` would see it (what
    /// `from` gives in `give`), if it made one.
    pub counter: Option<Deal>,
}

/// `ai` answers the deal `from` proposed: accepts it, or refuses and says
/// why. With `counter`, a refusal for wanting more comes with the deal it
/// would accept instead (see [`counter_offer`]), proposed back to `from`.
pub fn answer(game: &mut Game, ai: PlayerId, from: PlayerId, counter: bool) -> Answer {
    let Some(deal) = game.proposal(from, ai).map(|p| p.deal.clone()) else { return Answer::default() };
    let why = match verdict(game, ai, from, &deal) {
        Ok(()) => return Answer { events: game.apply(ai, Command::AcceptDeal { from }).unwrap_or_default(), ..Answer::default() },
        Err(why) => why,
    };
    let mut events = game.apply(ai, Command::RefuseDeal { from }).unwrap_or_default();
    let better = if counter { counter_offer(game, ai, from, &deal) } else { None };
    if let Some(better) = &better {
        events.extend(game.apply(ai, Command::ProposeDeal { to: from, deal: better.reversed() }).unwrap_or_default());
    }
    Answer { events, refusal: Some(why), counter: better }
}

/// The deal `ai` would accept instead of `deal` (proposed by `from`), asking
/// as little more as it can: first less of what it gives, then gold from
/// `from`, then gold per turn, then one of `from`'s cities. `None` if it
/// refuses for another reason than wanting more, or nothing would do.
pub fn counter_offer(game: &Game, ai: PlayerId, from: PlayerId, deal: &Deal) -> Option<Deal> {
    let s = Strategy::assess(game, ai);
    let gap = match judge(game, &s, from, deal) {
        Ok(()) => return Some(deal.clone()),
        Err(Refusal::WantMore(gap)) => gap,
        Err(_) => return None,
    };
    let works = |d: &Deal| game.deal_problem(from, ai, d).is_none() && judge(game, &s, from, d).is_ok();
    // Gold-like items on one side, raised by enough to close the gap (a
    // little more each try, for rounding).
    let with_gold = |deal: &Deal, theirs: bool, per_turn: bool, tries: f32| {
        let mut d = deal.clone();
        let items = if theirs { &mut d.give } else { &mut d.get };
        let now = |i: &Item| match (i, per_turn) {
            (Item::Gold(g), false) | (Item::GoldPerTurn(g), true) => Some(*g),
            _ => None,
        };
        let had = items.iter().find_map(now).unwrap_or(0);
        items.retain(|i| now(i).is_none());
        let scale = if per_turn { PAYMENT_TURNS as f32 * if theirs { PAYMENT_TRUST } else { 1.0 } } else { 1.0 };
        let step = (gap * (1.0 + 0.05 * tries) / scale).ceil() as i32;
        // Theirs grows; ours shrinks.
        let amount = if theirs { had + step } else { had - step };
        if amount > 0 {
            items.push(if per_turn { Item::GoldPerTurn(amount) } else { Item::Gold(amount) });
        }
        d
    };
    let mut tries: Vec<Deal> = Vec::new();
    for t in 0..3 {
        let t = t as f32;
        if deal.get.iter().any(|i| matches!(i, Item::Gold(_))) {
            tries.push(with_gold(deal, false, false, t));
        }
        if deal.get.iter().any(|i| matches!(i, Item::GoldPerTurn(_))) {
            tries.push(with_gold(deal, false, true, t));
        }
    }
    for t in 0..3 {
        tries.push(with_gold(deal, true, false, t as f32));
    }
    for t in 0..3 {
        tries.push(with_gold(deal, true, true, t as f32));
    }
    if let Some(d) = tries.into_iter().find(|d| works(d)) {
        return Some(d);
    }
    // A city, the least valuable that will do.
    let mut cities: Vec<(f32, sim::city::CityId)> = game
        .cities
        .values()
        .filter(|c| c.owner == from && game.tradeable_city(c.id) && game.players[ai as usize].explored[c.tile as usize])
        .filter(|c| !deal.give.contains(&Item::City(c.id)))
        .map(|c| (city_value(game, &s, c.id), c.id))
        .collect();
    cities.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    cities.into_iter().map(|(_, c)| Deal { give: [deal.give.clone(), vec![Item::City(c)]].concat(), ..deal.clone() }).find(|d| works(d))
}

/// Answers deals proposed to us and proposes peace in wars we'd rather end.
/// Returns whether a war ended.
pub fn run(game: &mut Game, s: &Strategy, events: &mut Vec<Event>) -> bool {
    let me = s.me;
    let mut peace = false;
    let mut others: Vec<PlayerId> = (0..game.players.len() as PlayerId).filter(|&q| q != me && !game.players[q as usize].eliminated).collect();
    // One request for open borders a turn, to whoever is most in the way.
    others.sort_by(|&a, &b| s.passage(game, b).total_cmp(&s.passage(game, a)).then(a.cmp(&b)));
    let mut asked_borders = false;
    for q in others {
        if game.proposal(q, me).is_some() {
            let was_war = game.at_war(me, q);
            events.extend(answer(game, me, q, false).events);
            peace |= was_war && !game.at_war(me, q);
            continue;
        }
        if game.relation(me, q).last_proposal.is_some_and(|t| game.turn < t + ASK_AGAIN) {
            continue;
        }
        let deal = if game.at_war(me, q) {
            peace_offer(game, s, q)
        } else {
            alliance_offer(game, s, q)
                .or_else(|| if asked_borders { None } else { borders_offer(game, s, q) })
                .or_else(|| resource_offer(game, s, q))
        };
        if let Some(deal) = deal {
            asked_borders |= !deal.alliance;
            events.extend(game.apply(me, Command::ProposeDeal { to: q, deal }).unwrap_or_default());
        }
    }
    peace
}

/// An alliance against a shared enemy, if `s.me` wants one with `q` and (a
/// computer player) `q` would accept it.
fn alliance_offer(game: &Game, s: &Strategy, q: PlayerId) -> Option<Deal> {
    let me = s.me;
    let common = (0..game.players.len() as PlayerId).any(|o| o != q && o != me && game.at_war(me, o) && game.at_war(q, o));
    if !common || game.allied(me, q) || !game.has_met(me, q) {
        return None;
    }
    let deal = Deal { alliance: true, ..Default::default() };
    alliance_value(game, s, q).ok().filter(|&v| v >= 0.0)?;
    if !game.players[q as usize].human && judge(game, &Strategy::assess(game, q), me, &deal).is_err() {
        return None;
    }
    Some(deal)
}

/// Open borders from `q`, when their land is in our way: in exchange for
/// ours, and gold if `q` (a computer player) would want more and passing
/// is worth that much to us.
fn borders_offer(game: &Game, s: &Strategy, q: PlayerId) -> Option<Deal> {
    let me = s.me;
    if s.passage(game, q) <= 0.0 || game.may_enter(me, q) || !game.has_met(me, q) {
        return None;
    }
    let mutual = Deal { give: vec![Item::OpenBorders], get: vec![Item::OpenBorders], ..Default::default() };
    // If ours are open to them already, or we wouldn't open them: just ask.
    let deal = if game.borders_open(me, q) || hostile_to(game, s, q) { Deal { give: Vec::new(), ..mutual } } else { mutual };
    if game.players[q as usize].human {
        return offer_to_human(game, s, q, deal);
    }
    let deal = counter_offer(game, q, me, &deal)?;
    deal_value_as_proposer(game, s, q, &deal).filter(|&v| v >= 0.0).map(|_| deal)
}

/// What `deal`, proposed by `s.me` to `q`, is worth to `s.me`.
fn deal_value_as_proposer(game: &Game, s: &Strategy, q: PlayerId, deal: &Deal) -> Option<f32> {
    deal_value(game, s, q, &deal.reversed()).ok()
}

/// The peace `s.me` would propose to `q`, if it wants peace: peace alone,
/// or with gold when `q` (a computer player) would want more and it's worth
/// that much to us.
fn peace_offer(game: &Game, s: &Strategy, q: PlayerId) -> Option<Deal> {
    let ours = peace_value(game, s, q).ok().filter(|&v| v >= 0.0)?;
    let deal = Deal { peace: true, ..Default::default() };
    if game.players[q as usize].human {
        // We can't read a human's mind: tribute when we fear for our cities.
        let gold = (ours * 0.5 / gold_worth(game, s.me)).min(game.players[s.me as usize].gold as f32 * 0.5) as i32;
        return Some(if gold > 0 { Deal { give: vec![Item::Gold(gold)], ..deal } } else { deal });
    }
    let theirs = Strategy::assess(game, q);
    match judge(game, &theirs, s.me, &deal) {
        Ok(()) => Some(deal),
        Err(Refusal::WantMore(gap)) => {
            let gold = gap.ceil() as i32;
            let can = gold as f32 * gold_worth(game, s.me) <= ours && gold <= game.players[s.me as usize].gold;
            can.then(|| Deal { give: vec![Item::Gold(gold)], ..deal })
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::game::GameSetup;
    use sim::mapgen::{generate, MapSettings};
    use sim::rules::Rules;

    /// The human (player 0, Egypt) and an AI of `civ`, at peace.
    fn met(civ: &str) -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let civs = vec![rules.civ_id("egypt").unwrap(), rules.civ_id(civ).unwrap()];
        let mut g = Game::with_setup(rules, map, &GameSetup { civs, options: Default::default(), starts: Vec::new() });
        g.players[0].met[1] = true;
        g.players[1].met[0] = true;
        g
    }

    /// A war between the human (player 0) and an AI of `civ`, `age` turns old.
    fn war(civ: &str, age: u32) -> Game {
        let mut g = met(civ);
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        g.turn += age;
        g
    }

    fn propose(g: &mut Game, deal: Deal) -> Option<Refusal> {
        g.apply(0, Command::ProposeDeal { to: 1, deal }).unwrap();
        answer(g, 1, 0, false).refusal
    }

    fn peace() -> Deal {
        Deal { peace: true, ..Default::default() }
    }

    #[test]
    fn young_wars_go_on() {
        let mut g = war("rome", 2);
        assert_eq!(propose(&mut g, peace()), Some(Refusal::TooSoon));
        assert!(g.at_war(0, 1));
    }

    #[test]
    fn a_war_nobody_pursues_ends_for_nothing() {
        let mut g = war("rome", 20);
        assert_eq!(propose(&mut g, peace()), None);
        assert!(!g.at_war(0, 1));
    }

    #[test]
    fn some_never_forgive() {
        let mut g = war("korea", 20);
        assert_eq!(Strategy::assess(&g, 1).agenda, AgendaKind::NeverForgive, "Korea's agenda");
        g.players[1].relations[0].cities_lost = 1;
        assert_eq!(propose(&mut g, peace()), Some(Refusal::NeverForgives));
    }

    #[test]
    fn gold_for_nothing_is_welcome_and_nothing_for_gold_is_not() {
        let mut g = war("rome", 20);
        g.players[0].gold = 100;
        g.players[1].gold = 100;
        let gift = Deal { peace: true, give: vec![Item::Gold(50)], ..Default::default() };
        assert_eq!(propose(&mut g, gift), None);
        let mut g = war("rome", 20);
        g.players[1].gold = 100;
        let demand = Deal { peace: true, get: vec![Item::Gold(50)], ..Default::default() };
        assert!(matches!(propose(&mut g, demand), Some(Refusal::WantMore(gap)) if (gap - 50.0).abs() < 1.0));
    }

    #[test]
    fn the_ai_proposes_peace_in_wars_it_doesnt_want() {
        let mut g = war("rome", 20);
        let s = Strategy::assess(&g, 1);
        let mut events = Vec::new();
        run(&mut g, &s, &mut events);
        assert!(events.iter().any(|e| matches!(e, Event::DealProposed { from: 1, to: 0 })));
        // And doesn't ask again straight away.
        g.apply(0, Command::RefuseDeal { from: 1 }).unwrap();
        let mut events = Vec::new();
        run(&mut g, &s, &mut events);
        assert!(events.is_empty());
    }

    #[test]
    fn no_alliance_with_an_enemy() {
        let mut g = war("rome", 20);
        let peace_and_alliance = Deal { peace: true, alliance: true, ..Default::default() };
        // Rome holds a grudge after being attacked.
        g.players[1].relations[0].attacked_us = 2;
        g.players[1].relations[0].last_attacked_us = Some(g.turn);
        assert_eq!(propose(&mut g, peace_and_alliance), Some(Refusal::NoFriendship));
    }

    /// An alliance proposed to an AI of `civ` at peace by a partner with
    /// `theirs` warriors, while the AI has `ours`.
    fn alliance_offer(civ: &str, theirs: usize, ours: usize) -> Result<f32, Refusal> {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let civs = vec![rules.civ_id("egypt").unwrap(), rules.civ_id(civ).unwrap()];
        let mut g = Game::with_setup(rules, map, &GameSetup { civs, options: Default::default(), starts: Vec::new() });
        g.players[0].met[1] = true;
        g.players[1].met[0] = true;
        let warrior = g.rules.unit_type_id("warrior").unwrap();
        for (p, n) in [(0, theirs), (1, ours)] {
            let at = g.units.values().find(|u| u.owner == p).unwrap().tile;
            for _ in 0..n {
                g.spawn_unit(p, warrior, at);
            }
        }
        let s = Strategy::assess(&g, 1);
        deal_value(&g, &s, 0, &Deal { alliance: true, ..Default::default() })
    }

    #[test]
    fn alliances_depend_on_size_and_personality() {
        // Mali (a sleeping giant) welcomes a strong friend more than an equal.
        let strong = alliance_offer("mali", 12, 0).unwrap();
        let equal = alliance_offer("mali", 0, 0).unwrap();
        assert!(strong > equal, "{strong} vs {equal}");
        // The Mongols only ally upwards.
        assert_eq!(alliance_offer("mongols", 0, 12), Err(Refusal::TooWeak));
        assert!(alliance_offer("mongols", 12, 0).is_ok());
        // Raiders want targets, not friends.
        assert!(alliance_offer("norse", 12, 0).unwrap() < strong);
    }

    #[test]
    fn a_refusal_comes_with_a_counter_offer() {
        // Rome, at war with us and winning, wants something for peace.
        let mut g = war("rome", 20);
        g.players[0].gold = 2000;
        let at = g.units.values().find(|u| u.owner == 1).unwrap().tile;
        let warrior = g.rules.unit_type_id("warrior").unwrap();
        for _ in 0..12 {
            g.spawn_unit(1, warrior, at);
        }
        let s = Strategy::assess(&g, 1);
        assert!(peace_value(&g, &s, 0).is_ok());
        // Ask for gold with peace: too much, so it offers peace for less.
        g.players[1].gold = 300;
        let greedy = Deal { peace: true, get: vec![Item::Gold(300)], ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal: greedy }).unwrap();
        let a = answer(&mut g, 1, 0, true);
        let counter = a.counter.expect("a counter-offer");
        assert!(a.refusal.is_some());
        let asked: i32 = counter.get.iter().map(|i| if let Item::Gold(x) = i { *x } else { 0 }).sum();
        assert!(asked < 300, "{counter:?}");
        // It stands as Rome's proposal, and accepting it makes peace.
        assert!(g.proposal(1, 0).is_some());
        g.apply(0, Command::AcceptDeal { from: 1 }).unwrap();
        assert!(!g.at_war(0, 1));
    }

    #[test]
    fn explorers_with_nowhere_to_go_ask_for_open_borders() {
        // At peace: Rome's scout has run out of ground, and our city's
        // borders hide land it hasn't seen.
        let mut g = met("rome");
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(sim::rules::Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let scout = g.units.values().find(|u| u.owner == 1 && crate::military::is_scout(&g, u.id)).unwrap().id;
        g.apply(1, Command::SetOrder { unit: scout, order: sim::game::Order::Sleep }).unwrap();
        // Rome has seen our land, but not what lies beyond.
        for t in 0..g.territory.len() {
            if g.territory[t].is_some() {
                g.players[1].explored[t] = true;
            }
        }
        let s = Strategy::assess(&g, 1);
        assert!(s.passage(&g, 0) > 0.0);
        let mut events = Vec::new();
        run(&mut g, &s, &mut events);
        let deal = &g.proposal(1, 0).expect("a proposal").deal;
        assert_eq!((deal.give.clone(), deal.get.clone()), (vec![Item::OpenBorders], vec![Item::OpenBorders]), "ours for theirs");
        // Accepted, the need is gone.
        g.apply(0, Command::AcceptDeal { from: 1 }).unwrap();
        assert_eq!(Strategy::assess(&g, 1).passage(&g, 0), 0.0);
    }

    #[test]
    fn the_ai_never_asks_a_human_for_something_for_nothing() {
        let mut g = met("rome");
        let ask = Deal { get: vec![Item::OpenBorders], ..Default::default() };
        g.players[1].gold = 200;
        let s = Strategy::assess(&g, 1);
        let deal = offer_to_human(&g, &s, 0, ask.clone()).expect("pays for it");
        assert!(matches!(deal.give[..], [Item::Gold(n)] if n > 0 && n <= 100), "{deal:?}");
        // Still worth it to them.
        assert!(deal_value_as_proposer(&g, &s, 0, &deal).is_some_and(|v| v >= 0.0));
        // Nothing to pay with: no proposal.
        g.players[1].gold = 0;
        assert_eq!(offer_to_human(&g, &Strategy::assess(&g, 1), 0, ask), None);
        // A swap stays a swap.
        let swap = Deal { give: vec![Item::OpenBorders], get: vec![Item::OpenBorders], ..Default::default() };
        assert_eq!(offer_to_human(&g, &Strategy::assess(&g, 1), 0, swap.clone()), Some(swap));
    }

    #[test]
    fn no_counter_when_it_isnt_about_the_price() {
        let mut g = war("rome", 2);
        g.apply(0, Command::ProposeDeal { to: 1, deal: peace() }).unwrap();
        let a = answer(&mut g, 1, 0, true);
        assert_eq!(a.refusal, Some(Refusal::TooSoon));
        assert!(a.counter.is_none() && g.proposal(1, 0).is_none());
    }

    #[test]
    fn nobody_is_asked_for_their_only_copy() {
        let mut g = met("rome");
        for p in 0..2 {
            let settler = g.units.values().find(|u| u.owner == p && g.unit_def(u).has(sim::rules::Ability::FoundCity)).unwrap().id;
            g.apply(p, Command::FoundCity { unit: settler }).unwrap();
        }
        let city = g.cities.values().find(|c| c.owner == 0).unwrap().id;
        for c in g.cities.values().map(|c| c.tile).collect::<Vec<_>>() {
            for t in g.map.grid.within(c, 3) {
                g.map.tiles[t as usize].resource = None;
            }
        }
        let silk = g.rules.resource_id("silk").unwrap();
        let plantation = g.rules.improvement_id("plantation").unwrap();
        let ours: Vec<_> = g.map.grid.neighbors(g.cities[&city].tile).filter(|&t| g.district(t) == Some(city)).take(2).collect();
        let add = |g: &mut Game, t: sim::hex::TileId| {
            g.map.tiles[t as usize].resource = Some(silk);
            g.works[t as usize].improvement = Some(plantation);
        };
        add(&mut g, ours[0]);
        g.players[1].gold = 500;
        // A turn when Rome looks for resources.
        g.turn = RESOURCE_CHECK - 1;
        assert_eq!(g.resources_owned(0)[silk as usize], 1);
        assert_eq!(resource_offer(&g, &Strategy::assess(&g, 1), 0), None, "asked for our only Silk");
        // Rome with two cities wants it enough to ask.
        let rome = g.cities.values().find(|c| c.owner == 1).unwrap().tile;
        let settler = g.rules.unit_type_id("settler").unwrap();
        let sites: Vec<_> = g.map.grid.within(rome, 6).into_iter().filter(|&t| g.map.grid.distance(rome, t) >= 4).collect();
        let founded = sites.into_iter().any(|t| {
            let u = g.spawn_unit(1, settler, t);
            g.apply(1, Command::FoundCity { unit: u }).is_ok() || {
                g.units.remove(&u);
                false
            }
        });
        assert!(founded);
        assert_eq!(resource_offer(&g, &Strategy::assess(&g, 1), 0), None, "asked for our only Silk");
        add(&mut g, ours[1]);
        let deal = resource_offer(&g, &Strategy::assess(&g, 1), 0).expect("asks for the second Silk");
        assert_eq!(deal.get, vec![Item::Resource(silk)]);
    }
}
