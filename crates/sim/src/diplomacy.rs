//! Diplomacy as trade. Two players make a [`Deal`]: one proposes it and the
//! other accepts or refuses. A deal can include treaties for both sides:
//! peace (required between players at war) and an alliance. Each side can
//! give gold now, gold every turn for [`PAYMENT_TURNS`] turns, open borders
//! for [`OPEN_BORDERS_TURNS`] turns, luxury and strategic resources for
//! [`crate::resource::RESOURCE_TURNS`] turns, and cities. A proposal stands until the
//! end of the turn after it was made, so every player gets a turn to answer.
//!
//! Peace brings a truce: neither side may declare war again for
//! [`TRUCE_TURNS`] turns. Borders are closed at peace: units may only enter
//! another player's territory at war, with open borders, or as allies.
//! Allies' borders are open both ways and they can't go to war with each
//! other. War cancels payments and open borders between the two.

use crate::city::CityId;
use crate::combat::MAX_HP;
use crate::game::{CommandError, Event, Game, PlayerId, UnitId};
use crate::hex::TileId;
use serde::{Deserialize, Serialize};

/// How long a peace treaty forbids war, in turns.
pub const TRUCE_TURNS: u32 = 20;
/// How many turns a payment of gold per turn lasts.
pub const PAYMENT_TURNS: u32 = 20;
/// How many turns open borders last.
pub const OPEN_BORDERS_TURNS: u32 = 30;
/// How far units are moved out of territory they may not be in.
const EXPEL_RANGE: i32 = 8;
/// How far units leaving a ceded city look for somewhere to stand.
const RELOCATE_RANGE: i32 = 6;

/// Something one side of a deal gives the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Item {
    Gold(i32),
    /// Gold every turn for [`PAYMENT_TURNS`] turns.
    GoldPerTurn(i32),
    /// The giver's territory opens to the other's units for [`OPEN_BORDERS_TURNS`] turns.
    OpenBorders,
    /// One copy of a luxury or strategic resource for
    /// [`crate::resource::RESOURCE_TURNS`] turns.
    Resource(crate::rules::ResourceId),
    City(CityId),
}

/// A trade between the player proposing it and the one it's proposed to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deal {
    /// The war between them ends.
    pub peace: bool,
    /// They become allies.
    #[serde(default)]
    pub alliance: bool,
    /// What the proposer gives.
    pub give: Vec<Item>,
    /// What the proposer asks for in return.
    pub get: Vec<Item>,
}

impl Deal {
    pub fn is_empty(&self) -> bool {
        !self.peace && !self.alliance && self.give.is_empty() && self.get.is_empty()
    }

    /// The same deal proposed the other way round (a counter-offer's basis).
    pub fn reversed(&self) -> Deal {
        Deal { peace: self.peace, alliance: self.alliance, give: self.get.clone(), get: self.give.clone() }
    }
}

/// A deal waiting for an answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub from: PlayerId,
    pub to: PlayerId,
    pub deal: Deal,
    /// The turn it was proposed.
    pub turn: u32,
}

/// Gold one player pays another every turn under a deal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub from: PlayerId,
    pub to: PlayerId,
    pub amount: i32,
    pub turns_left: u32,
}

impl Game {
    pub fn allied(&self, a: PlayerId, b: PlayerId) -> bool {
        a != b && self.relation(a, b).allied
    }

    /// Whether `owner` has opened their borders to `to` in a deal.
    pub fn borders_open(&self, owner: PlayerId, to: PlayerId) -> bool {
        self.relation(owner, to).borders_open_until.is_some_and(|t| t > self.turn)
    }

    /// Turns left on the open borders `owner` gave `to` (0: closed).
    pub fn borders_open_left(&self, owner: PlayerId, to: PlayerId) -> u32 {
        self.relation(owner, to).borders_open_until.map_or(0, |t| t.saturating_sub(self.turn))
    }

    /// `player`'s units standing in `owner`'s territory.
    pub fn guests(&self, player: PlayerId, owner: PlayerId) -> usize {
        self.units.values().filter(|u| u.owner == player && self.territory_owner(u.tile) == Some(owner)).count()
    }

    /// Whether `player`'s units may enter `owner`'s territory.
    pub fn may_enter(&self, player: PlayerId, owner: PlayerId) -> bool {
        player == owner || self.at_war(player, owner) || self.allied(player, owner) || self.borders_open(owner, player)
    }

    /// For each tile, the player whose closed borders keep `player` out
    /// (see [`crate::path::Mover::closed`]); empty if no borders are closed to them.
    pub fn closed_tiles(&self, player: PlayerId) -> Vec<u8> {
        let shut: Vec<bool> = (0..self.players.len() as PlayerId).map(|o| !self.may_enter(player, o)).collect();
        if !shut.contains(&true) {
            return Vec::new();
        }
        self.territory
            .iter()
            .map(|t| match *t {
                Some(o) if shut[o as usize] => o,
                _ => crate::path::OPEN,
            })
            .collect()
    }

    /// End of turn: units standing in territory they may no longer be in
    /// (after peace, when open borders run out, or when borders grow under
    /// them) move to the nearest tile they may be on.
    pub(crate) fn expel_trespassers(&mut self) -> Vec<Event> {
        // Merchants trade with anyone at peace.
        self.move_out(|g, u, owner| !g.may_enter(u.owner, owner) && !g.is_merchant(u))
    }

    /// When war breaks out, units in the other side's land (there by open
    /// borders) are sent home first: no attacking from inside a host's borders.
    pub(crate) fn expel_at_war(&mut self, a: PlayerId, b: PlayerId) -> Vec<Event> {
        self.move_out(|g, u, owner| {
            (u.owner == a && owner == b) || (u.owner == b && owner == a) || (!g.may_enter(u.owner, owner) && !g.is_merchant(u))
        })
    }

    /// Moves every unit standing in territory that `barred(game, unit,
    /// territory's owner)` keeps it out of to the nearest tile it may stand on.
    fn move_out(&mut self, barred: impl Fn(&Game, &crate::game::Unit, PlayerId) -> bool) -> Vec<Event> {
        let out: Vec<UnitId> = self
            .units
            .values()
            .filter(|u| self.territory_owner(u.tile).is_some_and(|o| o != u.owner && barred(self, u, o)))
            .map(|u| u.id)
            .collect();
        let mut events = Vec::new();
        let mut moved = Vec::new();
        for unit in out {
            let u = &self.units[&unit];
            let mover = self.mover(u);
            let allowed = |t: TileId| self.territory_owner(t).is_none_or(|o| o == u.owner || !barred(self, u, o));
            let mut tiles = self.map.grid.within(u.tile, EXPEL_RANGE);
            tiles.sort_by_key(|&t| (self.map.grid.distance(t, u.tile), t));
            let dest = tiles.into_iter().find(|&t| allowed(t) && mover.can_enter(&self.map, &self.rules, t) && self.can_stop_at(u, t));
            if let Some(t) = dest {
                moved.push(u.owner);
                let u = self.units.get_mut(&unit).expect("exists");
                u.tile = t;
                u.order = crate::game::Order::None;
                events.push(Event::UnitMoved { unit, path: vec![t] });
            }
        }
        moved.sort();
        moved.dedup();
        for p in moved {
            events.extend(self.update_visibility(p));
        }
        events
    }

    /// Whose closed borders stop `unit` reaching `to`: the first territory on
    /// the way that it could cross if those borders were open (`None` if it
    /// can get there already, or couldn't even then).
    pub fn borders_in_the_way(&self, unit: UnitId, to: TileId) -> Option<PlayerId> {
        let u = self.units.get(&unit)?;
        if self.path_for(unit, to).is_some() {
            return None;
        }
        let open = crate::path::Mover { closed: Vec::new(), ..self.mover(u) };
        let path = self.path_with(u, to, &open)?;
        path.tiles.iter().filter_map(|&t| self.territory_owner(t)).find(|&o| !self.may_enter(u.owner, o))
    }

    /// Turns until `a` and `b` may go to war again (0: they may).
    pub fn truce_left(&self, a: PlayerId, b: PlayerId) -> u32 {
        self.relation(a, b).truce_until.map_or(0, |t| t.saturating_sub(self.turn))
    }

    /// Turns `a` and `b` have been at war (0 if they're at peace).
    pub fn war_length(&self, a: PlayerId, b: PlayerId) -> u32 {
        if !self.at_war(a, b) {
            return 0;
        }
        self.relation(a, b).war_since.map_or(0, |t| self.turn.saturating_sub(t))
    }

    /// The deal `from` has proposed to `to`, if it still stands.
    pub fn proposal(&self, from: PlayerId, to: PlayerId) -> Option<&Proposal> {
        self.proposals.iter().find(|p| p.from == from && p.to == to && p.turn + 1 >= self.turn)
    }

    /// Whether a city can be traded by its owner: not their original capital,
    /// and not their last city.
    pub fn tradeable_city(&self, city: CityId) -> bool {
        let Some(c) = self.cities.get(&city) else { return false };
        c.capital_of != Some(c.owner) && self.cities.values().filter(|o| o.owner == c.owner).count() > 1
    }

    /// Why `from` and `to` couldn't make `deal` now, if anything stops them.
    pub fn deal_problem(&self, from: PlayerId, to: PlayerId, deal: &Deal) -> Option<&'static str> {
        let n = self.players.len();
        if from == to || from as usize >= n || to as usize >= n {
            return Some("no such player");
        }
        if self.players[from as usize].eliminated || self.players[to as usize].eliminated {
            return Some("they're out of the game");
        }
        if !self.has_met(from, to) {
            return Some("you haven't met them");
        }
        if deal.is_empty() {
            return Some("the deal is empty");
        }
        let war = self.at_war(from, to);
        if war && !deal.peace {
            return Some("at war, a deal must include peace");
        }
        if deal.peace && !war {
            return Some("you're already at peace");
        }
        if deal.alliance && self.allied(from, to) {
            return Some("you're allies already");
        }
        self.side_problem(from, to, deal, &deal.give).or_else(|| self.side_problem(to, from, deal, &deal.get))
    }

    /// Why `giver` couldn't hand `taker` `items` in `deal`.
    fn side_problem(&self, giver: PlayerId, taker: PlayerId, deal: &Deal, items: &[Item]) -> Option<&'static str> {
        let borders = items.iter().filter(|&&i| i == Item::OpenBorders).count();
        if borders > 1 {
            return Some("open borders are listed twice");
        }
        if borders == 1 && (deal.alliance || self.allied(giver, taker)) {
            return Some("allies' borders are open already");
        }
        if borders == 1 && self.borders_open(giver, taker) {
            return Some("those borders are open already");
        }
        let gold: i32 = items.iter().map(|i| if let Item::Gold(g) = i { *g } else { 0 }).sum();
        if items.iter().any(|i| matches!(i, Item::Gold(g) | Item::GoldPerTurn(g) if *g <= 0)) {
            return Some("amounts of gold must be positive");
        }
        if gold > self.players[giver as usize].gold {
            return Some("not enough gold");
        }
        if items.iter().filter(|i| matches!(i, Item::GoldPerTurn(_))).count() > 1 {
            return Some("only one payment per turn each way");
        }
        let cities: Vec<CityId> = items.iter().filter_map(|i| if let Item::City(c) = i { Some(*c) } else { None }).collect();
        for (i, &c) in cities.iter().enumerate() {
            if cities[..i].contains(&c) {
                return Some("a city is listed twice");
            }
            if self.cities.get(&c).is_none_or(|city| city.owner != giver) {
                return Some("that city isn't theirs to give");
            }
            if !self.tradeable_city(c) {
                return Some("capitals and last cities can't be traded");
            }
        }
        let resources: Vec<_> = items.iter().filter_map(|i| if let Item::Resource(r) = i { Some(*r) } else { None }).collect();
        for (i, &r) in resources.iter().enumerate() {
            if resources[..i].contains(&r) {
                return Some("a resource is listed twice");
            }
            if r as usize >= self.rules.resources.resources.len() || self.rules.resource(r).kind == crate::rules::ResourceKind::Bonus {
                return Some("only luxuries and strategic resources can be traded");
            }
            if self.spare_resource(giver, r) < 1 {
                return Some("they have none of that to spare");
            }
        }
        let owned = self.cities.values().filter(|c| c.owner == giver).count();
        if !cities.is_empty() && cities.len() >= owned {
            return Some("they must keep a city");
        }
        None
    }

    pub(crate) fn propose_deal(&mut self, from: PlayerId, to: PlayerId, deal: Deal) -> Result<Vec<Event>, CommandError> {
        if let Some(why) = self.deal_problem(from, to, &deal) {
            return Err(CommandError::BadDeal(why));
        }
        let turn = self.turn;
        self.proposals.retain(|p| !(p.from == from && p.to == to));
        self.proposals.push(Proposal { from, to, deal, turn });
        self.relation_mut(from, to).last_proposal = Some(turn);
        Ok(vec![Event::DealProposed { from, to }])
    }

    pub(crate) fn accept_deal(&mut self, to: PlayerId, from: PlayerId) -> Result<Vec<Event>, CommandError> {
        let deal = self.proposal(from, to).ok_or(CommandError::NoDeal)?.deal.clone();
        if let Some(why) = self.deal_problem(from, to, &deal) {
            return Err(CommandError::BadDeal(why));
        }
        self.proposals.retain(|p| !(p.from == from && p.to == to));
        let mut events = Vec::new();
        let turn = self.turn;
        for (a, b) in [(from, to), (to, from)] {
            let r = self.relation_mut(a, b);
            if deal.peace {
                r.at_war = false;
                r.war_since = None;
                r.truce_until = Some(turn + TRUCE_TURNS);
            }
            r.allied |= deal.alliance;
        }
        for (giver, taker, items) in [(from, to, &deal.give), (to, from, &deal.get)] {
            for &item in items {
                match item {
                    Item::Gold(g) => {
                        self.players[giver as usize].gold -= g;
                        self.players[taker as usize].gold += g;
                    }
                    Item::GoldPerTurn(amount) => {
                        self.payments.push(Payment { from: giver, to: taker, amount, turns_left: PAYMENT_TURNS })
                    }
                    Item::OpenBorders => self.relation_mut(giver, taker).borders_open_until = Some(turn + OPEN_BORDERS_TURNS),
                    Item::Resource(resource) => self.resource_deals.push(crate::resource::ResourceDeal {
                        from: giver,
                        to: taker,
                        resource,
                        turns_left: crate::resource::RESOURCE_TURNS,
                    }),
                    Item::City(city) => events.extend(self.cede_city(city, taker)),
                }
            }
        }
        events.insert(0, Event::DealMade { from, to, deal });
        Ok(events)
    }

    pub(crate) fn refuse_deal(&mut self, to: PlayerId, from: PlayerId) -> Result<Vec<Event>, CommandError> {
        self.proposal(from, to).ok_or(CommandError::NoDeal)?;
        self.proposals.retain(|p| !(p.from == from && p.to == to));
        Ok(vec![Event::DealRefused { by: to, from }])
    }

    /// Hands a city to `to`: as when captured, but whole (no lost citizen or
    /// damage). Its owner's units inside move out to the nearest free tile.
    fn cede_city(&mut self, city: CityId, to: PlayerId) -> Vec<Event> {
        let (tile, from) = (self.cities[&city].tile, self.cities[&city].owner);
        let civ = self.players[to as usize].civ;
        let rules = &self.rules;
        let c = self.cities.get_mut(&city).expect("checked");
        c.buildings.retain(|&b| rules.building_civ(b).is_none_or(|o| o == civ));
        c.owner = to;
        c.captured = None;
        c.queue.clear();
        c.auto_queue = true;
        c.hp = c.hp.max(MAX_HP / 2);
        self.hand_over_district(city, to);
        let mut events = Vec::new();
        let leaving: Vec<UnitId> = self.units_at(tile).filter(|u| u.owner == from).map(|u| u.id).collect();
        for unit in leaving {
            match self.free_tile_near(unit, tile) {
                Some(t) => {
                    self.units.get_mut(&unit).expect("exists").tile = t;
                    events.push(Event::UnitMoved { unit, path: vec![t] });
                }
                None => {
                    self.units.remove(&unit);
                    events.push(Event::UnitRemoved { unit });
                }
            }
        }
        events.push(Event::CityTraded { city, from, to });
        events.push(Event::BordersChanged { player: from });
        events.push(Event::BordersChanged { player: to });
        for p in [from, to] {
            events.extend(self.update_visibility(p));
        }
        events
    }

    /// The nearest tile to `around` where `unit` could stand.
    fn free_tile_near(&self, unit: UnitId, around: TileId) -> Option<TileId> {
        let u = &self.units[&unit];
        let mover = self.mover(u);
        let mut tiles = self.map.grid.within(around, RELOCATE_RANGE);
        tiles.sort_by_key(|&t| (self.map.grid.distance(t, around), t));
        tiles.into_iter().find(|&t| t != around && mover.can_enter(&self.map, &self.rules, t) && self.can_stop_at(u, t))
    }

    /// End of turn: payments under deals.
    pub(crate) fn process_payments(&mut self) {
        let mut payments = std::mem::take(&mut self.payments);
        payments.retain(|p| !self.players[p.from as usize].eliminated && !self.players[p.to as usize].eliminated);
        for p in &mut payments {
            // Paid in full when there's gold to pay it.
            let paid = p.amount.min(self.players[p.from as usize].gold.max(0));
            self.players[p.from as usize].gold -= paid;
            self.players[p.to as usize].gold += paid;
            p.turns_left -= 1;
        }
        payments.retain(|p| p.turns_left > 0);
        self.payments = payments;
    }

    /// End of turn: proposals that nobody answered lapse.
    pub(crate) fn expire_proposals(&mut self) {
        let turn = self.turn;
        self.proposals.retain(|p| p.turn + 1 >= turn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Command;
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::{Ability, Rules};

    /// Two players who have met, each with a capital and a second city.
    fn two_civs() -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 1);
        g.players[0].met[1] = true;
        g.players[1].met[0] = true;
        let settler = g.rules.unit_type_id("settler").unwrap();
        for p in 0..2 {
            let first = g.units.values().find(|u| u.owner == p && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
            g.apply(p, Command::FoundCity { unit: first }).unwrap();
            let site = (0..g.map.tiles.len() as TileId).find(|&t| g.found_city_problem(p, t).is_none()).unwrap();
            let second = g.spawn_unit(p, settler, site);
            g.apply(p, Command::FoundCity { unit: second }).unwrap();
        }
        g
    }

    fn second_city(g: &Game, p: PlayerId) -> CityId {
        g.cities.values().find(|c| c.owner == p && c.capital_of.is_none()).unwrap().id
    }

    #[test]
    fn peace_needs_a_proposal_and_an_answer() {
        let mut g = two_civs();
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        let gold = Deal { give: vec![Item::Gold(10)], ..Default::default() };
        g.players[0].gold = 50;
        assert_eq!(g.apply(0, Command::ProposeDeal { to: 1, deal: gold }), Err(CommandError::BadDeal("at war, a deal must include peace")));
        let peace = Deal { peace: true, ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal: peace }).unwrap();
        assert!(g.at_war(0, 1), "a proposal alone changes nothing");
        // Answered in the other player's next turn.
        g.apply(0, Command::EndTurn).unwrap();
        let events = g.apply(1, Command::AcceptDeal { from: 0 }).unwrap();
        assert!(matches!(events[0], Event::DealMade { from: 0, to: 1, .. }));
        assert!(!g.at_war(0, 1));
        assert_eq!(g.truce_left(0, 1), TRUCE_TURNS);
        assert_eq!(g.apply(1, Command::DeclareWar { on: 0 }), Err(CommandError::Truce));
    }

    #[test]
    fn proposals_lapse_and_can_be_refused() {
        let mut g = two_civs();
        g.players[0].gold = 100;
        let deal = Deal { give: vec![Item::Gold(40)], ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal: deal.clone() }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert_eq!(g.apply(1, Command::AcceptDeal { from: 0 }), Err(CommandError::NoDeal), "lapsed");
        g.apply(0, Command::ProposeDeal { to: 1, deal }).unwrap();
        let events = g.apply(1, Command::RefuseDeal { from: 0 }).unwrap();
        assert!(matches!(events[..], [Event::DealRefused { by: 1, from: 0 }]));
        assert_eq!(g.apply(1, Command::AcceptDeal { from: 0 }), Err(CommandError::NoDeal));
    }

    #[test]
    fn gold_and_payments_change_hands() {
        let mut g = two_civs();
        g.players[0].gold = 100;
        g.players[1].gold = 0;
        let deal = Deal { give: vec![Item::Gold(60), Item::GoldPerTurn(5)], ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal }).unwrap();
        g.apply(1, Command::AcceptDeal { from: 0 }).unwrap();
        assert_eq!((g.players[0].gold, g.players[1].gold), (40, 60));
        let before = g.players[1].gold;
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.players[1].gold >= before + 5, "paid each turn");
        assert_eq!(g.payments[0].turns_left, PAYMENT_TURNS - 1);
        // War ends payments.
        g.relation_mut(0, 1).truce_until = None;
        g.apply(1, Command::DeclareWar { on: 0 }).unwrap();
        assert!(g.payments.is_empty());
    }

    #[test]
    fn cities_can_be_traded_but_not_capitals() {
        let mut g = two_civs();
        let capital = g.original_capital(1).unwrap().id;
        let bad = Deal { get: vec![Item::City(capital)], ..Default::default() };
        assert_eq!(g.deal_problem(0, 1, &bad), Some("capitals and last cities can't be traded"));
        let city = second_city(&g, 1);
        let tile = g.cities[&city].tile;
        let warrior = g.spawn_unit(1, g.rules.unit_type_id("warrior").unwrap(), tile);
        g.players[0].gold = 300;
        let deal = Deal { give: vec![Item::Gold(300)], get: vec![Item::City(city)], ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal }).unwrap();
        let events = g.apply(1, Command::AcceptDeal { from: 0 }).unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::CityTraded { from: 1, to: 0, .. })));
        assert_eq!(g.cities[&city].owner, 0);
        let w = &g.units[&warrior];
        assert!(w.tile != tile, "the old owner's garrison walked out");
        // The seller has only their capital left now.
        let again = Deal { get: vec![Item::City(capital)], ..Default::default() };
        assert!(g.deal_problem(0, 1, &again).is_some());
    }

    /// A tile in `owner`'s territory that `unit` could stand on, next to a
    /// tile outside it.
    fn border_crossing(g: &Game, owner: PlayerId) -> (TileId, TileId) {
        (0..g.map.tiles.len() as TileId)
            .filter(|&t| g.territory_owner(t) == Some(owner) && g.map.land_move_cost(&g.rules, t).is_some() && g.city_at(t).is_none())
            .find_map(|t| {
                g.map.grid.neighbors(t).find(|&n| g.territory_owner(n).is_none() && g.map.land_move_cost(&g.rules, n).is_some()).map(|n| (t, n))
            })
            .expect("a border")
    }

    #[test]
    fn borders_are_closed_at_peace_and_open_by_deal() {
        let mut g = two_civs();
        let (inside, outside) = border_crossing(&g, 1);
        let scout = g.spawn_unit(0, g.rules.unit_type_id("scout").unwrap(), outside);
        assert!(g.path_for(scout, inside).is_none(), "closed borders");
        let deal = Deal { get: vec![Item::OpenBorders], ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal }).unwrap();
        g.apply(1, Command::AcceptDeal { from: 0 }).unwrap();
        assert!(g.borders_open(1, 0) && !g.borders_open(0, 1), "one way");
        g.apply(0, Command::MoveUnit { unit: scout, to: inside }).unwrap();
        assert_eq!(g.units[&scout].tile, inside);
        // When they run out, the scout is shown the way out.
        for _ in 0..OPEN_BORDERS_TURNS {
            g.apply(0, Command::EndTurn).unwrap();
        }
        assert!(!g.borders_open(1, 0));
        let at = g.units[&scout].tile;
        assert_ne!(g.territory_owner(at), Some(1), "expelled");
    }

    #[test]
    fn war_sends_guests_home() {
        let mut g = two_civs();
        let (inside, outside) = border_crossing(&g, 1);
        let scout = g.spawn_unit(0, g.rules.unit_type_id("scout").unwrap(), outside);
        assert_eq!(g.borders_in_the_way(scout, inside), Some(1));
        let deal = Deal { get: vec![Item::OpenBorders], ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal }).unwrap();
        g.apply(1, Command::AcceptDeal { from: 0 }).unwrap();
        assert_eq!(g.borders_in_the_way(scout, inside), None, "open now");
        g.apply(0, Command::MoveUnit { unit: scout, to: inside }).unwrap();
        assert_eq!(g.units[&scout].tile, inside);
        // Declaring war from inside their borders doesn't leave us there.
        let events = g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::UnitMoved { unit, .. } if *unit == scout)));
        assert_ne!(g.territory_owner(g.units[&scout].tile), Some(1));
    }

    #[test]
    fn allies_share_their_land_and_keep_the_peace() {
        let mut g = two_civs();
        let deal = Deal { alliance: true, ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal }).unwrap();
        g.apply(1, Command::AcceptDeal { from: 0 }).unwrap();
        assert!(g.allied(0, 1) && g.allied(1, 0));
        assert!(g.may_enter(0, 1) && g.may_enter(1, 0));
        assert_eq!(g.apply(1, Command::DeclareWar { on: 0 }), Err(CommandError::Allied));
        let borders = Deal { give: vec![Item::OpenBorders], ..Default::default() };
        assert_eq!(g.deal_problem(0, 1, &borders), Some("allies' borders are open already"));
    }

    #[test]
    fn at_war_an_alliance_needs_peace_too() {
        let mut g = two_civs();
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        let alone = Deal { alliance: true, ..Default::default() };
        assert_eq!(g.deal_problem(0, 1, &alone), Some("at war, a deal must include peace"));
        let both = Deal { alliance: true, peace: true, ..Default::default() };
        assert_eq!(g.deal_problem(0, 1, &both), None);
    }

    #[test]
    fn deals_are_checked_again_when_accepted() {
        let mut g = two_civs();
        g.players[0].gold = 50;
        let deal = Deal { give: vec![Item::Gold(50)], ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal }).unwrap();
        g.players[0].gold = 10;
        assert_eq!(g.apply(1, Command::AcceptDeal { from: 0 }), Err(CommandError::BadDeal("not enough gold")));
    }
}
