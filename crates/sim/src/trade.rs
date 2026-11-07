//! Merchants and trade routes.
//!
//! A Merchant (a unit with [`Ability::Trade`]) given a route
//! (`Command::Trade`) travels between its home city and another city for as
//! long as the route lasts: it walks out, sells its goods at the other city's
//! gates for gold, walks home, loads up and sets off again. A merchant given a
//! new route away from home goes home to load first. The route is a standing
//! order ([`Order::Trade`]) carried out at every turn end, for computer and
//! human players alike. A route set to repeat (the default) runs to the same
//! city every time; otherwise, back home after each trip, the merchant takes
//! whichever route pays best then ([`crate::game::Command::RepeatRoute`]).
//!
//! What a trip earns (see [`Game::trip_gold`]) grows with the distance, and
//! by a tenth for each citizen of the city it sells in. There's no limit on distance: a longer trip
//! pays more but takes longer, so gold per turn stays about the same, and
//! roads make every route pay better. Trading with another civ pays half as much
//! again, gives that civ a quarter of it too, and brings science if they know
//! technologies you don't. Trade at home brings the home city food and
//! production as well (as much of each as two thirds of the cargo). Markets
//! at either end and some techs and perks add more gold. Routes are ranked by
//! [`RouteOffer::worth_per_turn`], which counts food and production double.
//! A merchant can move to another of its owner's cities at once
//! ([`Game::transfer`]) to trade from there. Merchants may enter the land of anyone they're at peace with; rivals
//! at war capture them (and the gold they carry). Each city supports one
//! Merchant, and a second with a Market ([`Game::trade_capacity`]).

use crate::city::CityId;
use crate::civ::Effect;
use crate::game::{CommandError, Event, Game, Order, PlayerId, Unit, UnitId};
use crate::hex::TileId;
use crate::path::Path;
use crate::rules::Ability;
use serde::{Deserialize, Serialize};

/// A merchant's trade route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Route {
    /// Where it loads its goods (always its owner's).
    pub home: CityId,
    /// Where it sells them.
    pub to: CityId,
    /// On the way home.
    pub back: bool,
    /// Keeps trading with `to`; when false, the merchant picks the best
    /// paying route again each time it's home.
    #[serde(default = "repeats")]
    pub repeat: bool,
}

fn repeats() -> bool {
    true
}

/// A merchant sold its goods.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeReport {
    pub unit: UnitId,
    pub owner: PlayerId,
    pub home: CityId,
    pub to: CityId,
    pub gold: i32,
    /// Gold the other city's owner made (foreign routes).
    pub partner_gold: i32,
    pub science: i32,
    /// Food and production for the home city (routes at home).
    pub food: i32,
    pub production: i32,
}

/// What a trip on a route earns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TripValue {
    pub gold: i32,
    /// Gold for the other city's owner (foreign routes).
    pub partner_gold: i32,
    pub science: i32,
    /// Food and production for the home city (routes at home).
    pub food: i32,
    pub production: i32,
}

impl TripValue {
    /// What a trip is worth to the merchant's owner, in gold: food and
    /// production count double.
    pub fn worth(&self) -> i32 {
        self.gold + self.science + 2 * (self.food + self.production)
    }
}

/// A route a merchant could take, with what it would earn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteOffer {
    pub home: CityId,
    pub to: CityId,
    pub value: TripValue,
    /// Turns for the whole round trip, trading included.
    pub turns: u32,
    pub foreign: bool,
}

impl RouteOffer {
    pub fn gold_per_turn(&self) -> f32 {
        self.value.gold as f32 / self.turns.max(1) as f32
    }

    /// [`TripValue::worth`] per turn: how routes are ranked.
    pub fn worth_per_turn(&self) -> f32 {
        self.value.worth() as f32 / self.turns.max(1) as f32
    }
}

/// A merchant's way to its goal, remembered between turns (see
/// `Game::trade_paths`): the tiles from where it set off, or none when no
/// way was found, to look again on turn `retry`.
#[derive(Clone, Debug, Default)]
pub(crate) struct TradeWay {
    tiles: Vec<TileId>,
    retry: u32,
}

/// Turns a merchant that found no way waits before looking again.
const STUCK_TURNS: u32 = 3;

/// Cities closer than this are too near to trade between.
pub const MIN_DISTANCE: i32 = 3;
/// Routes looked at closely when picking the best (finding the way there is
/// the slow part).
const CANDIDATES: usize = 4;

impl Game {
    /// Whether a unit runs trade routes.
    pub fn is_merchant(&self, unit: &Unit) -> bool {
        self.unit_def(unit).has(Ability::Trade)
    }

    /// `player`'s merchants.
    pub fn merchants(&self, player: PlayerId) -> impl Iterator<Item = &Unit> {
        self.units.values().filter(move |u| u.owner == player && self.is_merchant(u))
    }

    /// How many merchants `player`'s cities support: one each, and another
    /// for each city with a Market.
    pub fn trade_capacity(&self, player: PlayerId) -> usize {
        self.cities
            .values()
            .filter(|c| c.owner == player)
            .map(|c| 1 + c.buildings.iter().any(|&b| self.rules.building_is(b, "market")) as usize)
            .sum()
    }

    /// Whether `player` has room for another merchant.
    pub fn merchant_room(&self, player: PlayerId) -> bool {
        self.merchants(player).count() < self.trade_capacity(player)
    }

    /// The home a merchant gets with a new route: its owner's nearest city.
    pub fn merchant_home(&self, unit: UnitId) -> Option<CityId> {
        let u = self.units.get(&unit)?;
        self.nearest_city(u.owner, u.tile)
    }

    /// What one trip from `home` to `to` earns `owner`.
    pub fn trip_gold(&self, owner: PlayerId, home: CityId, to: CityId) -> TripValue {
        let (Some(h), Some(d)) = (self.cities.get(&home), self.cities.get(&to)) else { return TripValue::default() };
        let bonus = |c: &crate::city::City| c.buildings.iter().map(|&b| self.rules.building(b).trade_gold).sum::<i32>();
        let foreign = d.owner != owner;
        let distance = self.map.grid.distance(h.tile, d.tile);
        let mut percent = 100;
        percent += if foreign { 50 } else { 0 };
        percent += bonus(h) + bonus(d);
        percent += self.civ_effects(owner).map(|e| if let Effect::TradeGold(p) = e { *p } else { 0 }).sum::<i32>();
        // Distance sets the cargo, the buyers' city its price: each citizen there adds a tenth.
        let cargo = 4 + 3 * distance / 2;
        let gold = cargo * (10 + d.population) * percent / 1000;
        // Goods bought at home feed and supply the home city.
        let supplies = if foreign { 0 } else { cargo * 2 / 3 };
        // Ideas travel with the goods when the other side knows more.
        let mine = &self.players[owner as usize].techs;
        let learns = foreign && self.players[d.owner as usize].techs.iter().zip(mine).any(|(&theirs, &ours)| theirs && !ours);
        TripValue {
            gold,
            partner_gold: if foreign { gold / 4 } else { 0 },
            science: if learns { gold / 3 } else { 0 },
            food: supplies,
            production: supplies,
        }
    }

    /// Why `unit` couldn't trade between its home (see
    /// [`Self::merchant_home`]) and `to`, short of finding the way there.
    pub fn route_problem(&self, unit: UnitId, to: CityId) -> Option<&'static str> {
        let u = self.units.get(&unit)?;
        if !self.is_merchant(u) {
            return Some("only Merchants trade");
        }
        let Some(home) = self.merchant_home(unit) else { return Some("you have no city to trade from") };
        self.pair_problem(u.owner, home, to, Some(unit))
    }

    /// Why `owner` couldn't trade between `home` and `to` (ignoring the
    /// route of `except`), if anything stops it.
    fn pair_problem(&self, owner: PlayerId, home: CityId, to: CityId, except: Option<UnitId>) -> Option<&'static str> {
        let (Some(h), Some(d)) = (self.cities.get(&home), self.cities.get(&to)) else { return Some("no such city") };
        if h.owner != owner {
            return Some("not your city");
        }
        if home == to || self.map.grid.distance(h.tile, d.tile) < MIN_DISTANCE {
            return Some("too close to home");
        }
        if !self.players[owner as usize].explored[d.tile as usize] {
            return Some("you don't know where it is");
        }
        if d.owner != owner && !self.has_met(owner, d.owner) {
            return Some("you haven't met them");
        }
        if self.at_war(owner, d.owner) {
            return Some("you're at war with them");
        }
        let taken = self.merchants(owner).any(|m| Some(m.id) != except && matches!(m.order, Order::Trade(r) if r.home == home && r.to == to));
        if taken {
            return Some("another merchant already trades between these cities");
        }
        None
    }

    /// Every route `unit` could take from its home, best paying first.
    pub fn route_offers(&self, unit: UnitId) -> Vec<RouteOffer> {
        let (Some(u), Some(home)) = (self.units.get(&unit), self.merchant_home(unit)) else { return Vec::new() };
        if !self.is_merchant(u) {
            return Vec::new();
        }
        let mut out: Vec<RouteOffer> = self
            .cities
            .keys()
            .filter(|&&to| self.pair_problem(u.owner, home, to, Some(unit)).is_none())
            .filter_map(|&to| self.route_offer(u, home, to))
            .collect();
        out.sort_by(|a, b| b.worth_per_turn().total_cmp(&a.worth_per_turn()).then(a.to.cmp(&b.to)));
        out
    }

    /// The best paying route for `unit` from its home to a city whose owner
    /// `ok` accepts. Routes are judged by the straight distance first, and
    /// only the best few are looked at closely.
    pub fn best_route(&self, unit: UnitId, ok: impl Fn(PlayerId) -> bool) -> Option<RouteOffer> {
        self.best_route_from(unit, self.merchant_home(unit)?, ok)
    }

    /// [`Self::best_route`] as if `unit`'s home were `home` (for a merchant
    /// thinking of moving there, see [`Self::transfer`]).
    pub fn best_route_from(&self, unit: UnitId, home: CityId, ok: impl Fn(PlayerId) -> bool) -> Option<RouteOffer> {
        let u = self.units.get(&unit)?;
        if !self.is_merchant(u) {
            return None;
        }
        let from = self.cities[&home].tile;
        let moves = self.unit_def(u).moves.max(1) as f32;
        // Gold per turn if the way there were straight.
        let mut guesses: Vec<(f32, CityId)> = self
            .cities
            .values()
            .filter(|c| ok(c.owner) && self.pair_problem(u.owner, home, c.id, Some(unit)).is_none())
            .map(|c| {
                let walk = (self.map.grid.distance(from, c.tile) - 1) as f32 / moves;
                (self.trip_gold(u.owner, home, c.id).worth() as f32 / (2.0 * (walk.ceil() + 1.0)), c.id)
            })
            .collect();
        guesses.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        guesses
            .into_iter()
            .take(CANDIDATES)
            .filter_map(|(_, to)| self.route_offer(u, home, to))
            .max_by(|a, b| a.worth_per_turn().total_cmp(&b.worth_per_turn()).then(b.to.cmp(&a.to)))
    }

    /// What a route from `home` to `to` would earn a merchant like `u`, if
    /// it can get there.
    pub fn route_offer(&self, u: &Unit, home: CityId, to: CityId) -> Option<RouteOffer> {
        let start = self.cities.get(&home)?.tile;
        let dest = self.cities.get(&to)?;
        let probe = Unit { tile: start, moves: self.turn_moves(u), ..u.clone() };
        let path = self.trade_path(&probe, dest.tile)?;
        // It stops at the gates, and trading takes a turn at each end.
        let walk = path.turns.iter().rev().nth(1).map_or(0, |&t| t + 1);
        Some(RouteOffer { home, to, value: self.trip_gold(u.owner, home, to), turns: 2 * (walk + 1), foreign: dest.owner != u.owner })
    }

    /// The way for a merchant to the city on `to`, through the land of anyone
    /// it isn't at war with, ending at the city itself.
    pub(crate) fn trade_path(&self, u: &Unit, to: TileId) -> Option<Path> {
        self.path_through(u, to, &self.mover(u))
    }

    /// For each tile, the player whose land a merchant of `player` keeps
    /// out of: those it's at war with (see [`crate::path::Mover::closed`]).
    pub(crate) fn trade_closed_tiles(&self, player: PlayerId) -> Vec<u8> {
        let shut: Vec<bool> = (0..self.players.len() as PlayerId).map(|o| self.at_war(player, o)).collect();
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

    /// Why `unit` couldn't move to `city` to trade from there (see
    /// [`Self::transfer`]), if anything stops it.
    pub fn transfer_problem(&self, unit: UnitId, city: CityId) -> Option<&'static str> {
        let u = self.units.get(&unit)?;
        if !self.is_merchant(u) {
            return Some("only Merchants move between cities");
        }
        let Some(c) = self.cities.get(&city).filter(|c| c.owner == u.owner) else { return Some("not your city") };
        if u.tile == c.tile {
            return Some("it's there already");
        }
        // Merchants on a route travel at the turn's end, so they have no moves left in between.
        if u.moves <= 0 && !matches!(u.order, Order::Trade(_)) {
            return Some("it has no moves left this turn");
        }
        if !self.can_stop_at(u, c.tile) {
            return Some("there's no room in that city");
        }
        None
    }

    /// `Command::Transfer`: the merchant moves to `city` at once, which
    /// becomes its home (the nearest city), ending its route and its turn.
    pub fn transfer(&mut self, unit: UnitId, city: CityId) -> Result<Vec<Event>, CommandError> {
        if let Some(why) = self.transfer_problem(unit, city) {
            return Err(CommandError::CantTrade(why));
        }
        let tile = self.cities[&city].tile;
        let mut events = self.set_order(unit, Order::None);
        self.trade_paths.remove(&unit);
        let u = self.units.get_mut(&unit).expect("checked");
        u.tile = tile;
        u.moves = 0;
        u.moved = true;
        let owner = u.owner;
        events.push(Event::UnitTransferred { unit, city });
        events.extend(self.update_visibility(owner));
        Ok(events)
    }

    /// `Command::Trade`: the merchant takes up a route from its home to `to`
    /// (going home first if it's away) and sets off.
    pub(crate) fn set_route(&mut self, unit: UnitId, to: CityId) -> Result<Vec<Event>, CommandError> {
        if let Some(why) = self.route_problem(unit, to) {
            return Err(CommandError::CantTrade(why));
        }
        let home = self.merchant_home(unit).expect("checked");
        let u = &self.units[&unit];
        if self.route_offer(u, home, to).is_none() {
            return Err(CommandError::CantTrade("there's no way there"));
        }
        let back = self.map.grid.distance(u.tile, self.cities[&home].tile) > 1;
        // A new route keeps the merchant's choice of repeating.
        let repeat = if let Order::Trade(r) = u.order { r.repeat } else { true };
        self.trade_paths.remove(&unit);
        let mut events = self.set_order(unit, Order::Trade(Route { home, to, back, repeat }));
        events.extend(self.travel(unit, true));
        Ok(events)
    }

    /// The turn end for a merchant on a route: at its goal it trades (at the
    /// far end) or loads up (at home) and turns round, which takes the turn;
    /// otherwise it travels on. A route that can no longer run ends.
    pub(crate) fn run_trade(&mut self, unit: UnitId) -> Vec<Event> {
        let Some(u) = self.units.get(&unit) else { return Vec::new() };
        let Order::Trade(route) = u.order else { return Vec::new() };
        let owner = u.owner;
        let home_ok = self.cities.get(&route.home).is_some_and(|c| c.owner == owner);
        let to_ok = self.cities.get(&route.to).is_some_and(|c| !self.at_war(owner, c.owner));
        if !home_ok || !to_ok || route.home == route.to {
            let mut events = self.set_order(unit, Order::None);
            events.push(Event::RouteEnded { unit, city: route.to });
            return events;
        }
        let goal = if route.back { route.home } else { route.to };
        if self.map.grid.distance(u.tile, self.cities[&goal].tile) <= 1 {
            let mut events = Vec::new();
            if !route.back {
                let v = self.trip_gold(owner, route.home, route.to);
                let partner = self.cities[&route.to].owner;
                self.players[owner as usize].gold += v.gold;
                self.players[owner as usize].science_stored += v.science;
                if partner != owner {
                    self.players[partner as usize].gold += v.partner_gold;
                }
                let home = self.cities.get_mut(&route.home).expect("checked");
                home.food += v.food;
                home.production += v.production;
                events.push(Event::Traded(TradeReport {
                    unit,
                    owner,
                    home: route.home,
                    to: route.to,
                    gold: v.gold,
                    partner_gold: v.partner_gold,
                    science: v.science,
                    food: v.food,
                    production: v.production,
                }));
            }
            // The way back is the way it came.
            let other = self.cities[&if route.back { route.to } else { route.home }].tile;
            let tile = u.tile;
            match self.trade_paths.get_mut(&unit).and_then(|w| Some((w.tiles.iter().position(|&t| t == tile)?, w))) {
                Some((i, way)) => {
                    way.tiles.truncate(i + 1);
                    way.tiles.reverse();
                    if way.tiles.last().is_some_and(|&t| t != other && self.map.grid.distance(t, other) == 1) {
                        way.tiles.push(other);
                    }
                }
                None => {
                    self.trade_paths.remove(&unit);
                }
            }
            let u = self.units.get_mut(&unit).expect("exists");
            u.moves = 0;
            u.order = Order::Trade(Route { back: !route.back, ..route });
            events.push(Event::OrderChanged { unit, order: u.order });
            if route.back && !route.repeat {
                events.extend(self.reconsider_route(unit));
            }
            return events;
        }
        self.travel(unit, false)
    }

    /// A merchant home from a trip that doesn't repeat its route takes the
    /// best paying one now, if that pays more than the one it has.
    fn reconsider_route(&mut self, unit: UnitId) -> Vec<Event> {
        let u = &self.units[&unit];
        let Order::Trade(route) = u.order else { return Vec::new() };
        let Some(best) = self.best_route(unit, |_| true) else { return Vec::new() };
        let now = self.route_offer(u, route.home, route.to).map_or(0.0, |o| o.gold_per_turn());
        if best.to == route.to || best.gold_per_turn() <= now {
            return Vec::new();
        }
        self.trade_paths.remove(&unit);
        let order = Order::Trade(Route { home: best.home, to: best.to, back: false, ..route });
        let mut events = self.set_order(unit, order);
        events.push(Event::RouteChanged { unit, from: route.to, to: best.to });
        events
    }

    /// `Command::RepeatRoute`: whether a merchant keeps its route or picks
    /// the best one again after each trip.
    pub(crate) fn set_repeat(&mut self, unit: UnitId, repeat: bool) -> Result<Vec<Event>, CommandError> {
        let Some(Unit { order: Order::Trade(route), .. }) = self.units.get(&unit) else {
            return Err(CommandError::CantTrade("it has no trade route"));
        };
        let order = Order::Trade(Route { repeat, ..*route });
        Ok(self.set_order(unit, order))
    }

    /// Moves a merchant towards the goal of its route as far as this turn
    /// allows, stopping at the gates of a city it can't stand in. With `see`
    /// false, the caller updates its owner's sight afterwards.
    fn travel(&mut self, unit: UnitId, see: bool) -> Vec<Event> {
        let u = &self.units[&unit];
        let Order::Trade(route) = u.order else { return Vec::new() };
        let goal = if route.back { route.home } else { route.to };
        let tile = self.cities[&goal].tile;
        if u.moves <= 0 || self.map.grid.distance(u.tile, tile) <= 1 {
            return Vec::new();
        }
        // A merchant that found no way waits a little before looking again
        // (blocked passes, frozen seas).
        if self.trade_paths.get(&unit).is_some_and(|w| w.tiles.is_empty() && w.retry > self.turn) {
            return Vec::new();
        }
        let mover = self.mover(u);
        let mut path = match self.cached_trade_path(u, tile, &mover) {
            Some(p) => p,
            None => {
                let Some(p) = self.path_through(u, tile, &mover) else {
                    self.trade_paths.insert(unit, TradeWay { tiles: Vec::new(), retry: self.turn + STUCK_TURNS });
                    return Vec::new();
                };
                let tiles = std::iter::once(u.tile).chain(p.tiles.iter().copied()).collect();
                self.trade_paths.insert(unit, TradeWay { tiles, retry: 0 });
                p
            }
        };
        if !self.can_stop_at(u, tile) {
            path.tiles.pop();
            path.turns.pop();
        }
        self.walk(unit, &path, see)
    }

    /// The rest of the way a merchant found earlier, if it still leads to
    /// `goal` from where the merchant stands and nothing foreign stands in it.
    fn cached_trade_path(&self, u: &Unit, goal: TileId, mover: &crate::path::Mover) -> Option<Path> {
        let tiles = &self.trade_paths.get(&u.id)?.tiles;
        if tiles.last() != Some(&goal) {
            return None;
        }
        let rest = &tiles[tiles.iter().position(|&t| t == u.tile)? + 1..];
        let foreign: std::collections::HashSet<TileId> = self
            .units
            .values()
            .filter(|o| o.owner != u.owner && !self.passes(u, o))
            .map(|o| o.tile)
            .chain(self.cities.values().filter(|c| c.owner != u.owner && c.tile != goal).map(|c| c.tile))
            .collect();
        let mut at = u.tile;
        for &t in rest {
            if (t != goal && foreign.contains(&t)) || self.step_cost(u, mover, at, t).is_none() {
                return None;
            }
            at = t;
        }
        // The walk goes as far as the turn's movement allows.
        Some(Path { tiles: rest.to_vec(), turns: vec![0; rest.len()], left: 0 })
    }

    /// The way a merchant is following to the goal of its route, from where
    /// it set off on this leg (`None` until it has looked for one).
    pub fn trade_way(&self, unit: UnitId) -> Option<&[TileId]> {
        self.trade_paths.get(&unit).map(|w| w.tiles.as_slice()).filter(|t| !t.is_empty())
    }

    /// The gold a captured merchant was carrying: what its trip would have earned.
    pub(crate) fn merchant_cargo(&self, unit: &Unit) -> i32 {
        match unit.order {
            Order::Trade(r) => self.trip_gold(unit.owner, r.home, r.to).gold,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Command, GameOptions, GameSetup};
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Rules;

    /// A game where players 0 and 1 have founded their first cities and
    /// player 0 has a merchant at home.
    fn trading_game() -> (Game, CityId, CityId, UnitId) {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let civs = ["rome", "egypt"].iter().map(|c| rules.civ_id(c).unwrap()).collect();
        let mut g = Game::with_setup(rules, map, &GameSetup { civs, options: GameOptions::default(), starts: Vec::new() });
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let ours = *g.cities.keys().max().unwrap();
        let home = g.cities[&ours].tile;
        let merchant = g.spawn_unit(0, g.rules.unit_type_id("merchant").unwrap(), home);
        // Player 1 founds a city 6 tiles away, somewhere the merchant can reach.
        let theirs_at = (0..g.map.tiles.len() as TileId)
            .filter(|&t| g.map.grid.distance(home, t) == 6 && g.found_city_problem(1, t).is_none())
            .find(|&t| g.trade_path(&g.units[&merchant], t).is_some())
            .expect("a site in reach");
        let settler = g.units.values().find(|u| u.owner == 1 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.units.get_mut(&settler).unwrap().tile = theirs_at;
        g.apply(1, Command::FoundCity { unit: settler }).unwrap();
        let theirs = *g.cities.keys().max().unwrap();
        (g, ours, theirs, merchant)
    }

    /// Player 0 knows where player 1's city is and has met them.
    fn introduce(g: &mut Game, to: CityId) {
        let tile = g.cities[&to].tile;
        for t in g.map.grid.within(tile, 1) {
            g.players[0].explored[t as usize] = true;
        }
        for (a, b) in [(0usize, 1usize), (1, 0)] {
            g.players[a].met.resize(2, false);
            g.players[a].met[b] = true;
        }
    }

    #[test]
    fn merchants_cant_be_moved_only_given_routes() {
        let (mut g, _, _, merchant) = trading_game();
        let next = g.map.grid.neighbors(g.units[&merchant].tile).find(|&t| g.map.land_move_cost(&g.rules, t).is_some()).unwrap();
        assert!(matches!(g.apply(0, Command::MoveUnit { unit: merchant, to: next }), Err(CommandError::CantTrade(_))));
        for order in [Order::Goto(next), Order::Explore, Order::Fortify] {
            assert!(g.apply(0, Command::SetOrder { unit: merchant, order }).is_err(), "{order:?}");
        }
        g.apply(0, Command::SetOrder { unit: merchant, order: Order::Sleep }).unwrap();
        g.apply(0, Command::SetOrder { unit: merchant, order: Order::None }).unwrap();
    }

    #[test]
    fn merchants_get_past_units_of_anyone_at_peace() {
        let (mut g, home, theirs, merchant) = trading_game();
        introduce(&mut g, theirs);
        // Their soldiers and workers stand all round our city, and our worker in it.
        let at = g.cities[&home].tile;
        let around: Vec<TileId> = g.map.grid.neighbors(at).collect();
        for (i, &t) in around.iter().enumerate() {
            let kind = g.rules.unit_type_id(if i % 2 == 0 { "warrior" } else { "worker" }).unwrap();
            g.spawn_unit(1, kind, t);
        }
        g.spawn_unit(0, g.rules.unit_type_id("worker").unwrap(), at);
        let m = &g.units[&merchant];
        assert!(around.iter().all(|&t| g.can_stop_at(m, t)), "a merchant may share their tiles");
        assert!(g.route_offers(merchant).iter().any(|o| o.to == theirs));
        g.apply(0, Command::Trade { unit: merchant, to: theirs }).unwrap();
        let out = g.units[&merchant].tile;
        assert_ne!(out, at, "it sets off through them");
        // Their soldier may stand with it too.
        let warrior = g.units.values().find(|u| u.owner == 1 && g.unit_def(u).id == "warrior").unwrap().clone();
        let alone = (0..g.map.tiles.len() as TileId)
            .find(|&t| g.map.land_move_cost(&g.rules, t).is_some() && g.city_at(t).is_none() && g.units_at(t).next().is_none())
            .unwrap();
        let m = Unit { tile: alone, ..g.units[&merchant].clone() };
        g.units.insert(merchant, m);
        assert!(g.can_stop_at(&warrior, alone), "and they may share its tile");
        // At war they're in the way again (and the route ends).
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        let m = &g.units[&merchant];
        assert!(g.units.values().filter(|o| o.owner == 1).all(|o| !g.can_stop_at(m, o.tile)));
    }

    #[test]
    fn merchants_need_met_cities_at_peace() {
        let (mut g, _, theirs, merchant) = trading_game();
        assert_eq!(g.route_problem(merchant, theirs), Some("you don't know where it is"));
        introduce(&mut g, theirs);
        assert_eq!(g.route_problem(merchant, theirs), None);
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        assert_eq!(g.route_problem(merchant, theirs), Some("you're at war with them"));
    }

    #[test]
    fn a_route_runs_on_its_own_and_pays_at_the_far_end() {
        let (mut g, home, theirs, merchant) = trading_game();
        introduce(&mut g, theirs);
        // Off the map's pathing problems: only a reachable partner counts.
        let offer = g.route_offer(&g.units[&merchant].clone(), home, theirs).expect("the two capitals are on one continent");
        assert!(offer.foreign && offer.value.gold > 0 && offer.value.partner_gold > 0);
        g.apply(0, Command::Trade { unit: merchant, to: theirs }).unwrap();
        let gold = (g.players[0].gold, g.players[1].gold);
        let mut sales = 0;
        for _ in 0..offer.turns * 2 + 2 {
            let events = g.apply(0, Command::EndTurn).unwrap();
            sales += events.iter().filter(|e| matches!(e, Event::Traded(r) if r.unit == merchant)).count();
        }
        assert!(sales >= 2, "two round trips' worth of sales, got {sales}");
        // Both sides earn (cities add gold too, so only check the merchant's share shows).
        assert!(g.players[0].gold - gold.0 >= offer.value.gold * 2);
        assert!(g.players[1].gold - gold.1 >= offer.value.partner_gold * 2);
        assert!(matches!(g.units[&merchant].order, Order::Trade(_)), "the route goes on");
    }

    #[test]
    fn a_route_that_doesnt_repeat_switches_to_a_better_one_at_home() {
        let (mut g, home, theirs, merchant) = trading_game();
        introduce(&mut g, theirs);
        g.apply(0, Command::Trade { unit: merchant, to: theirs }).unwrap();
        assert!(matches!(g.units[&merchant].order, Order::Trade(r) if r.repeat), "routes repeat by default");
        g.apply(0, Command::RepeatRoute { unit: merchant, repeat: false }).unwrap();
        // A city of ours twice as far away, reachable, pays better per trip and turn alike once it's big.
        let home_tile = g.cities[&home].tile;
        let far = (0..g.map.tiles.len() as TileId)
            .filter(|&t| (8..=10).contains(&g.map.grid.distance(home_tile, t)) && g.found_city_problem(0, t).is_none())
            .find(|&t| g.trade_path(&g.units[&merchant], t).is_some())
            .expect("a site in reach");
        let settler = g.spawn_unit(0, g.rules.unit_type_id("settler").unwrap(), far);
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let ours = *g.cities.keys().max().unwrap();
        g.cities.get_mut(&ours).unwrap().population = 30;
        let switched = (0..40).any(|_| {
            let events = g.apply(0, Command::EndTurn).unwrap();
            events.contains(&Event::RouteChanged { unit: merchant, from: theirs, to: ours })
        });
        assert!(switched, "it found the better route");
        assert!(matches!(g.units[&merchant].order, Order::Trade(r) if r.to == ours && !r.back && !r.repeat));
    }

    #[test]
    fn one_merchant_per_pair_of_cities_and_war_ends_routes() {
        let (mut g, home, theirs, merchant) = trading_game();
        introduce(&mut g, theirs);
        g.apply(0, Command::Trade { unit: merchant, to: theirs }).unwrap();
        let other = g.spawn_unit(0, g.rules.unit_type_id("merchant").unwrap(), g.cities[&home].tile);
        assert_eq!(g.route_problem(other, theirs), Some("another merchant already trades between these cities"));
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        let events = g.apply(0, Command::EndTurn).unwrap();
        assert!(events.contains(&Event::RouteEnded { unit: merchant, city: theirs }));
        assert_eq!(g.units[&merchant].order, Order::None);
    }

    #[test]
    fn trade_pays_more_far_away_abroad_and_with_markets() {
        let (mut g, home, theirs, _) = trading_game();
        introduce(&mut g, theirs);
        let abroad = g.trip_gold(0, home, theirs);
        // The same city as if it were ours.
        g.cities.get_mut(&theirs).unwrap().owner = 0;
        let at_home = g.trip_gold(0, home, theirs);
        assert!(abroad.gold > at_home.gold && at_home.partner_gold == 0);
        let market = g.rules.building_id("market").unwrap();
        g.cities.get_mut(&theirs).unwrap().buildings.push(market);
        assert!(g.trip_gold(0, home, theirs).gold > at_home.gold);
    }

    #[test]
    fn trade_at_home_supplies_the_home_city() {
        let (mut g, home, theirs, merchant) = trading_game();
        introduce(&mut g, theirs);
        let abroad = g.trip_gold(0, home, theirs);
        assert_eq!((abroad.food, abroad.production), (0, 0));
        g.cities.get_mut(&theirs).unwrap().owner = 0;
        let at_home = g.trip_gold(0, home, theirs);
        assert!(at_home.food > 0 && at_home.production > 0);
        assert!(at_home.worth() > abroad.worth(), "trade at home is worth more than its gold");
        // At the far end, the home city gets its supplies.
        let gates = g.map.grid.neighbors(g.cities[&theirs].tile).find(|&t| g.can_stop_at(&g.units[&merchant], t)).unwrap();
        let u = g.units.get_mut(&merchant).unwrap();
        u.tile = gates;
        u.order = Order::Trade(Route { home, to: theirs, back: false, repeat: true });
        let before = (g.cities[&home].food, g.cities[&home].production);
        let events = g.run_trade(merchant);
        assert!(events.iter().any(|e| matches!(e, Event::Traded(r) if r.food == at_home.food && r.production == at_home.production)));
        assert_eq!((g.cities[&home].food, g.cities[&home].production), (before.0 + at_home.food, before.1 + at_home.production));
    }

    #[test]
    fn merchants_move_to_another_of_our_cities_to_trade_from_there() {
        let (mut g, home, theirs, merchant) = trading_game();
        introduce(&mut g, theirs);
        assert_eq!(g.transfer_problem(merchant, theirs), Some("not your city"));
        assert_eq!(g.transfer_problem(merchant, home), Some("it's there already"));
        g.cities.get_mut(&theirs).unwrap().owner = 0;
        g.apply(0, Command::Trade { unit: merchant, to: theirs }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        // On the way, it's sent to the other city instead.
        g.apply(0, Command::Transfer { unit: merchant, city: theirs }).unwrap();
        let u = &g.units[&merchant];
        assert_eq!(u.tile, g.cities[&theirs].tile);
        assert_eq!((u.order, u.moves), (Order::None, 0), "the route and the turn are over");
        assert_eq!(g.merchant_home(merchant), Some(theirs), "it trades from there now");
        assert!(g.route_offers(merchant).iter().any(|o| o.home == theirs && o.to == home));
        assert_eq!(g.transfer_problem(merchant, home), Some("it has no moves left this turn"));
    }

    #[test]
    fn caravanserais_add_to_trade() {
        let (mut g, home, theirs, _) = trading_game();
        introduce(&mut g, theirs);
        let before = g.trip_gold(0, home, theirs).gold;
        let inn = g.rules.building_id("caravanserai").unwrap();
        g.cities.get_mut(&home).unwrap().buildings.push(inn);
        assert!(g.trip_gold(0, home, theirs).gold > before);
    }

    #[test]
    fn cities_support_a_merchant_each_and_another_with_a_market() {
        let (mut g, home, _, _) = trading_game();
        assert_eq!(g.trade_capacity(0), 1);
        assert!(!g.merchant_room(0));
        let market = g.rules.building_id("market").unwrap();
        g.cities.get_mut(&home).unwrap().buildings.push(market);
        assert!(g.merchant_room(0));
        let kind = g.rules.unit_type_id("merchant").unwrap();
        g.players[0].techs.fill(true);
        assert!(g.available_builds(home).contains(&crate::city::Build::Unit(kind)));
        g.spawn_unit(0, kind, g.cities[&home].tile);
        assert!(!g.available_builds(home).contains(&crate::city::Build::Unit(kind)));
    }
}
