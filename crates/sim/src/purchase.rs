//! Spending gold: buying units and buildings outright, and buying tiles to
//! push a city's borders out.
//!
//! A build costs gold for the production still missing, a little more than
//! twice as much and steeper for big builds (see [`gold_for`]). Buying what
//! the city is building now uses the production already stored for it, so a
//! half-finished build costs less than half; buying anything else costs the
//! full price and leaves the stored production for the build in progress.
//! Bought units appear at once but can only move next turn.
//!
//! Tiles next to a city's territory and within its reach can be bought
//! (see [`Game::tile_price`]): dearer further out, the better the tile, and
//! the more land the city already has.

use crate::city::{Build, CityId, MAX_CITY_RADIUS};
use crate::civ::Effect;
use crate::game::{Event, Game, PlayerId, UnitId};
use crate::hex::TileId;
use crate::rules::{Ability, UnitClass, UnitTypeId};

/// Gold that buys `production` outright.
pub fn gold_for(production: i32) -> i32 {
    let p = production.max(1);
    2 * p + p * p / 100
}

/// Tiles a city starts with (its centre and neighbours): only land beyond
/// these makes further tiles dearer.
const FIRST_TILES: i32 = 7;

impl Game {
    /// Production still missing for `build` in `city`: all of it, unless it's
    /// the build in progress (then its stored production counts).
    pub fn missing_production(&self, city: CityId, build: Build) -> i32 {
        let c = &self.cities[&city];
        let stored = if c.queue.first() == Some(&build) { c.production } else { 0 };
        (self.build_cost(build) - stored).max(1)
    }

    /// Gold `city`'s owner pays to buy `build` there now.
    pub fn buy_price(&self, city: CityId, build: Build) -> i32 {
        let owner = self.cities[&city].owner;
        let discount: i32 = self.civ_effects(owner).map(|e| if let Effect::BuyDiscount(p) = e { *p } else { 0 }).sum();
        let price = gold_for(self.missing_production(city, build));
        price - price * discount.clamp(0, 90) / 100
    }

    /// Why `city` couldn't buy `build` now (its owner's gold included), if anything stops it.
    pub fn buy_problem(&self, city: CityId, build: Build) -> Option<&'static str> {
        let c = self.cities.get(&city)?;
        let owner = c.owner;
        if let Build::Project(_) = build {
            return Some("projects can't be bought");
        }
        if matches!(build, Build::Building(b) if c.has(b)) {
            return Some("already built here");
        }
        if !self.has_tech_for(owner, build) || !self.civ_builds(owner, build) {
            return Some("not available yet");
        }
        if let Some(why) = self.place_problem(city, build) {
            return Some(why);
        }
        if !self.has_resources_for(owner, build) {
            return Some("needs a strategic resource you don't have");
        }
        if let Build::Unit(kind) = build {
            let def = self.rules.unit(kind);
            if def.class == UnitClass::Military && self.at_unit_limit(owner) {
                return Some("your army is at its size limit");
            }
            if def.has(Ability::Trade) && !self.merchant_room(owner) {
                return Some("each city supports one Merchant, two with a Market");
            }
            if def.has(Ability::FoundCity) && c.population < 2 {
                return Some("a settler needs a citizen to spare");
            }
            if self.spawn_tile(city, kind).is_none() {
                return Some("no room next to the city");
            }
        }
        if self.players[owner as usize].gold < self.buy_price(city, build) {
            return Some("not enough gold");
        }
        None
    }

    pub(crate) fn buy(&mut self, city: CityId, build: Build) -> Vec<Event> {
        let price = self.buy_price(city, build);
        let cost = self.build_cost(build);
        let c = self.cities.get_mut(&city).expect("checked by caller");
        let owner = c.owner;
        if c.queue.first() == Some(&build) {
            c.production = (c.production - cost).max(0);
            c.queue.remove(0);
        } else if let Build::Building(_) | Build::Passage(..) = build {
            c.queue.retain(|&b| b != build);
        }
        self.players[owner as usize].gold -= price;
        let mut events = Vec::new();
        match build {
            Build::Building(b) => self.cities.get_mut(&city).expect("exists").buildings.push(b),
            Build::Project(_) => unreachable!("checked by caller"),
            Build::Passage(p, tile) => events.extend(self.build_passage(tile, p)),
            Build::Unit(kind) => {
                let tile = self.spawn_tile(city, kind).expect("checked by caller");
                let unit = self.spawn_unit(owner, kind, tile);
                // Bought, not trained: it moves from next turn.
                let xp = self.trained_xp(city, kind);
                let u = self.units.get_mut(&unit).expect("just spawned");
                u.moves = 0;
                u.xp = xp;
                if self.rules.unit(kind).has(Ability::FoundCity) {
                    self.cities.get_mut(&city).expect("exists").population -= 1;
                }
                events.push(Event::UnitSpawned { unit });
                events.extend(self.update_visibility(owner));
            }
        }
        self.assign_worked_tiles(city);
        events.push(Event::Bought { city, build, gold: price });
        events.push(Event::CityChanged { city });
        events
    }

    /// The best unit `player` could turn a unit of type `kind` into: the
    /// furthest step along its upgrade line (in the player's civ's version)
    /// they can build now. `None` if there's nothing better yet.
    pub fn upgrade_target(&self, player: PlayerId, kind: UnitTypeId) -> Option<UnitTypeId> {
        let mut best = None;
        let mut next = self.rules.unit_upgrade(kind);
        while let Some(step) = next {
            let mine = self.civ_unit(player, &self.rules.unit(step).id).unwrap_or(step);
            let b = Build::Unit(mine);
            if self.has_tech_for(player, b) && self.has_resources_for(player, b) {
                best = Some(mine);
            }
            next = self.rules.unit_upgrade(step);
        }
        best
    }

    /// Gold to upgrade a unit to its [`Game::upgrade_target`]: what the
    /// difference in production would cost to buy, at least 20.
    pub fn upgrade_price(&self, unit: UnitId) -> Option<i32> {
        let u = self.units.get(&unit)?;
        let to = self.upgrade_target(u.owner, u.kind)?;
        Some(gold_for(self.rules.unit(to).cost - self.rules.unit(u.kind).cost).max(20))
    }

    /// Why a unit couldn't be upgraded now (its owner's gold included), if anything stops it.
    pub fn upgrade_problem(&self, unit: UnitId) -> Option<&'static str> {
        let u = self.units.get(&unit)?;
        let Some(price) = self.upgrade_price(unit) else { return Some("nothing better to upgrade to yet") };
        if self.territory_owner(u.tile) != Some(u.owner) {
            return Some("units upgrade only inside your borders");
        }
        if self.is_embarked(u) {
            return Some("embarked units can't upgrade");
        }
        if u.moves <= 0 {
            return Some("no moves left this turn");
        }
        if self.players[u.owner as usize].gold < price {
            return Some("not enough gold");
        }
        None
    }

    /// `Command::Upgrade`: the unit becomes its upgrade, keeping its wounds,
    /// and spends the rest of the turn refitting.
    pub(crate) fn upgrade(&mut self, unit: UnitId) -> Vec<Event> {
        let price = self.upgrade_price(unit).expect("checked by caller");
        let u = &self.units[&unit];
        let (owner, from) = (u.owner, u.kind);
        let to = self.upgrade_target(owner, from).expect("checked by caller");
        self.players[owner as usize].gold -= price;
        let u = self.units.get_mut(&unit).expect("exists");
        u.kind = to;
        u.moves = 0;
        let mut events = vec![Event::UnitUpgraded { unit, from, gold: price }];
        events.extend(self.update_visibility(owner));
        events
    }

    /// Tiles `city` could buy: unclaimed, within its reach and next to its territory.
    pub fn buyable_tiles(&self, city: CityId) -> Vec<TileId> {
        let Some(c) = self.cities.get(&city) else { return Vec::new() };
        self.map
            .grid
            .within(c.tile, MAX_CITY_RADIUS)
            .into_iter()
            .filter(|&t| self.territory[t as usize].is_none() && self.map.grid.neighbors(t).any(|n| self.district(n) == Some(city)))
            .collect()
    }

    /// Gold `city`'s owner pays for `tile`: dearer further from the city, the
    /// better its yields, and the more tiles the city already has.
    pub fn tile_price(&self, city: CityId, tile: TileId) -> i32 {
        let c = &self.cities[&city];
        let y = self.tile_yields(c.owner, tile);
        let owned = self.district_tiles(city).len() as i32;
        10 + 10 * self.map.grid.distance(c.tile, tile) + 4 * (y.food + y.production + y.gold).max(0) + 5 * (owned - FIRST_TILES).max(0)
    }

    /// Why `city` couldn't buy `tile` now, if anything stops it.
    pub fn tile_problem(&self, city: CityId, tile: TileId) -> Option<&'static str> {
        let c = self.cities.get(&city)?;
        if (tile as usize) >= self.territory.len() {
            return Some("no such tile");
        }
        if self.territory[tile as usize].is_some() {
            return Some("that tile belongs to a city already");
        }
        if !self.buyable_tiles(city).contains(&tile) {
            return Some("only tiles next to the city's borders, within its reach");
        }
        if self.players[c.owner as usize].gold < self.tile_price(city, tile) {
            return Some("not enough gold");
        }
        None
    }

    pub(crate) fn buy_tile(&mut self, city: CityId, tile: TileId) -> Vec<Event> {
        let price = self.tile_price(city, tile);
        let owner = self.cities[&city].owner;
        self.players[owner as usize].gold -= price;
        self.set_territory(tile, Some(owner));
        self.assign_worked_tiles(city);
        let mut events = vec![Event::TileBought { city, tile, gold: price }, Event::BordersChanged { player: owner }, Event::CityChanged { city }];
        events.extend(self.update_visibility(owner));
        events
    }

    /// Gold a unit of `kind` costs its owner each turn.
    pub fn unit_upkeep(&self, kind: UnitTypeId) -> i32 {
        let def = self.rules.unit(kind);
        if def.class != UnitClass::Military {
            return 0;
        }
        (def.cost + 20) / 60
    }

    /// What `player`'s military units cost in gold per turn, before the
    /// allowance (see [`Self::upkeep`]).
    pub fn gross_upkeep(&self, player: PlayerId) -> i32 {
        self.units.values().filter(|u| u.owner == player).map(|u| self.unit_upkeep(u.kind)).sum()
    }

    /// Upkeep `player` is spared each turn: [`FREE_UPKEEP`] and one per city.
    pub fn upkeep_allowance(&self, player: PlayerId) -> i32 {
        FREE_UPKEEP + self.cities.values().filter(|c| c.owner == player).count() as i32
    }

    /// What `player` pays in gold per turn for its military units.
    pub fn upkeep(&self, player: PlayerId) -> i32 {
        (self.gross_upkeep(player) - self.upkeep_allowance(player)).max(0)
    }

    /// End of turn: every player pays its units' upkeep. A treasury that
    /// can't pay disbands its dearest unit (and the debt is forgiven).
    pub(crate) fn process_upkeep(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        for player in 0..self.players.len() as PlayerId {
            if self.players[player as usize].eliminated {
                continue;
            }
            let cost = self.upkeep(player);
            let p = &mut self.players[player as usize];
            p.gold -= cost;
            if p.gold >= 0 {
                continue;
            }
            p.gold = 0;
            let dearest: Option<UnitId> = self
                .units
                .values()
                .filter(|u| u.owner == player && self.unit_upkeep(u.kind) > 0)
                .max_by_key(|u| (self.unit_upkeep(u.kind), u.id))
                .map(|u| u.id);
            if let Some(id) = dearest {
                let kind = self.units[&id].kind;
                self.units.remove(&id);
                events.push(Event::UnitRemoved { unit: id });
                events.push(Event::UnitDisbanded { player, kind });
                events.extend(self.update_visibility(player));
            }
        }
        events
    }
}

/// Upkeep every player is spared each turn, plus one gold per city.
pub const FREE_UPKEEP: i32 = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Command, CommandError};
    use crate::mapgen::{generate, MapSettings};
    use crate::hex::TileId;
    use crate::rules::Rules;

    fn game_with_city(civ: &str) -> (Game, CityId) {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let civs = vec![rules.civ_id(civ).unwrap(), rules.civ_id("rome").unwrap()];
        let mut game = Game::with_setup(rules, map, &crate::game::GameSetup { civs, options: Default::default(), starts: Vec::new() });
        game.players[0].techs.fill(true);
        let settler = game.units.values().find(|u| u.owner == 0 && game.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        game.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let city = *game.cities.keys().next().unwrap();
        (game, city)
    }

    /// Forgets every tech from `id` on (techs are in tree order).
    fn forget_from(g: &mut Game, id: &str) {
        let from = g.rules.tech_id(id).unwrap() as usize;
        g.players[0].techs[from..].fill(false);
    }

    #[test]
    fn units_upgrade_along_their_line_for_gold() {
        let (mut g, city) = game_with_city("egypt");
        forget_from(&mut g, "feudalism");
        let kind = |id: &str| g.rules.unit_type_id(id).unwrap();
        let (warrior, spearman, pikeman) = (kind("warrior"), kind("spearman"), kind("pikeman"));
        // Warriors are no longer built once Spearmen are.
        assert!(!g.available_builds(city).contains(&Build::Unit(warrior)));
        assert!(g.available_builds(city).contains(&Build::Unit(spearman)));
        let tile = g.cities[&city].tile;
        let unit = g.spawn_unit(0, warrior, tile);
        g.units.get_mut(&unit).unwrap().hp = 60;
        let price = g.upgrade_price(unit).unwrap();
        assert_eq!(price, gold_for(g.rules.unit(spearman).cost - g.rules.unit(warrior).cost).max(20));
        g.players[0].gold = price - 1;
        assert_eq!(g.apply(0, Command::Upgrade { unit }), Err(CommandError::CantBuy("not enough gold")));
        g.players[0].gold = price;
        g.apply(0, Command::Upgrade { unit }).unwrap();
        let u = &g.units[&unit];
        assert_eq!((u.kind, u.hp, u.moves, g.players[0].gold), (spearman, 60, 0, 0), "keeps its wounds, spends the turn");
        // With Feudalism a Warrior skips straight to a Pikeman.
        let feudalism = g.rules.tech_id("feudalism").unwrap();
        g.players[0].techs[feudalism as usize] = true;
        assert_eq!(g.upgrade_target(0, warrior), Some(pikeman));
        assert_eq!(g.upgrade_target(0, pikeman), None, "nothing better yet");
        // Not outside our borders.
        let away = (0..g.map.tiles.len() as TileId).find(|&t| g.territory[t as usize].is_none() && g.map.land_move_cost(&g.rules, t).is_some()).unwrap();
        let u = g.units.get_mut(&unit).unwrap();
        (u.tile, u.moves) = (away, crate::path::points(2));
        g.players[0].gold = 1000;
        assert_eq!(g.upgrade_problem(unit), Some("units upgrade only inside your borders"));
    }

    #[test]
    fn unique_units_upgrade_like_what_they_replace() {
        let (g, _) = game_with_city("netherlands");
        let schutter = g.rules.unit_type_id("schutter").unwrap();
        assert_eq!(g.rules.unit_upgrade(schutter), g.rules.unit_type_id("pikeman"));
        // Every upgrade leads somewhere later, so lines end.
        for u in 0..g.rules.units.units.len() as UnitTypeId {
            let (mut steps, mut next) = (0, g.rules.unit_upgrade(u));
            while let Some(n) = next {
                steps += 1;
                assert!(steps < 10, "{} upgrades in a loop", g.rules.unit(u).id);
                next = g.rules.unit_upgrade(n);
            }
        }
    }

    #[test]
    fn some_buildings_need_another_first() {
        let (mut g, city) = game_with_city("egypt");
        let b = |g: &Game, id: &str| Build::Building(g.rules.building_id(id).unwrap());
        let (library, university) = (b(&g, "library"), b(&g, "university"));
        assert!(!g.available_builds(city).contains(&university));
        assert_eq!(g.apply(0, Command::SetQueue { city, queue: vec![university] }), Err(CommandError::NotAvailable));
        g.players[0].gold = 10_000;
        assert_eq!(g.buy_problem(city, university), Some("needs another building in the city first"));
        g.apply(0, Command::Buy { city, build: library }).unwrap();
        assert!(g.available_builds(city).contains(&university));
        // A civ's own walls count as walls.
        let (mut g, city) = game_with_city("korea");
        let hwaseong = g.rules.building_id("hwaseong").unwrap();
        g.cities.get_mut(&city).unwrap().buildings.push(hwaseong);
        assert!(g.available_builds(city).contains(&b(&g, "castle")));
    }

    #[test]
    fn eras_and_tech_effects() {
        let (mut g, city) = game_with_city("egypt");
        assert_eq!(g.rules.era(g.era(0)).id, "modern");
        forget_from(&mut g, "the_wheel");
        assert_eq!(g.rules.era(g.era(0)).id, "ancient");
        let library = g.rules.building_id("library").unwrap();
        g.cities.get_mut(&city).unwrap().buildings.push(library);
        let before = g.city_output(city).science;
        let press = g.rules.tech_id("printing_press").unwrap();
        g.players[0].techs[press as usize] = true;
        assert_eq!(g.city_output(city).science, before + 2, "Printing Press: +2 science from libraries");
    }

    #[test]
    fn buying_a_building_costs_gold_and_finishes_it() {
        let (mut g, city) = game_with_city("egypt");
        let library = Build::Building(g.rules.building_id("library").unwrap());
        let price = g.buy_price(city, library);
        assert_eq!(price, gold_for(g.build_cost(library)));
        g.players[0].gold = price - 1;
        assert_eq!(g.apply(0, Command::Buy { city, build: library }), Err(CommandError::CantBuy("not enough gold")));
        g.players[0].gold = price;
        g.apply(0, Command::Buy { city, build: library }).unwrap();
        assert_eq!(g.players[0].gold, 0);
        assert!(g.cities[&city].has(g.rules.building_id("library").unwrap()));
        assert_eq!(g.apply(0, Command::Buy { city, build: library }), Err(CommandError::CantBuy("already built here")));
    }

    #[test]
    fn the_build_in_progress_is_cheaper_and_uses_its_production() {
        let (mut g, city) = game_with_city("egypt");
        let walls = Build::Building(g.rules.building_id("walls").unwrap());
        let market = Build::Building(g.rules.building_id("market").unwrap());
        g.apply(0, Command::SetQueue { city, queue: vec![walls, market] }).unwrap();
        g.cities.get_mut(&city).unwrap().production = 30;
        assert!(g.buy_price(city, walls) < gold_for(g.build_cost(walls)));
        assert_eq!(g.buy_price(city, market), gold_for(g.build_cost(market)), "only the build in progress gets the production");
        g.players[0].gold = 1000;
        g.apply(0, Command::Buy { city, build: walls }).unwrap();
        let c = &g.cities[&city];
        assert_eq!((c.production, c.queue.clone()), (0, vec![market]));
        // Buying from further down the queue leaves the stored production alone.
        g.cities.get_mut(&city).unwrap().production = 20;
        let granary = Build::Building(g.rules.building_id("granary").unwrap());
        g.apply(0, Command::SetQueue { city, queue: vec![market, granary] }).unwrap();
        g.apply(0, Command::Buy { city, build: granary }).unwrap();
        let c = &g.cities[&city];
        assert_eq!((c.production, c.queue.clone()), (20, vec![market]));
    }

    #[test]
    fn bought_units_wait_a_turn() {
        let (mut g, city) = game_with_city("egypt");
        let worker = Build::Unit(g.rules.unit_type_id("worker").unwrap());
        g.players[0].gold = 1000;
        let events = g.apply(0, Command::Buy { city, build: worker }).unwrap();
        let Some(Event::UnitSpawned { unit }) = events.iter().find(|e| matches!(e, Event::UnitSpawned { .. })) else { panic!("no unit") };
        assert_eq!(g.units[unit].moves, 0);
        let settler = Build::Unit(g.rules.unit_type_id("settler").unwrap());
        assert_eq!(g.buy_problem(city, settler), Some("a settler needs a citizen to spare"));
    }

    #[test]
    fn military_units_are_built_and_bought_in_cities() {
        let (mut g, city) = game_with_city("egypt");
        let spear = Build::Unit(g.rules.unit_type_id("artillery").unwrap());
        assert!(g.available_builds(city).contains(&spear));
        g.players[0].gold = 5000;
        assert_eq!(g.buy_problem(city, spear), None);
        g.apply(0, Command::Buy { city, build: spear }).unwrap();
        assert!(g.units.values().any(|u| u.owner == 0 && g.unit_def(u).id == "artillery"));
    }

    #[test]
    fn upkeep_is_paid_in_gold_or_a_unit_goes() {
        let (mut g, _city) = game_with_city("egypt");
        let k = g.rules.unit_type_id("musketman").unwrap();
        let at = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).id == "warrior").unwrap().tile;
        // Three musketmen: more than the allowance covers.
        let near: Vec<TileId> =
            g.map.grid.neighbors(at).filter(|&t| g.map.land_move_cost(&g.rules, t).is_some() && g.units_at(t).next().is_none()).take(3).collect();
        let muskets: Vec<UnitId> = near.iter().map(|&t| g.spawn_unit(0, k, t)).collect();
        assert_eq!(g.upkeep(0), (g.gross_upkeep(0) - g.upkeep_allowance(0)).max(0));
        assert!(g.upkeep(0) >= g.unit_upkeep(k) && g.unit_upkeep(k) > 0);
        // A musketman costs exactly its upkeep.
        let mut without = g.clone();
        without.units.remove(&muskets[0]);
        g.players[0].gold = 1000;
        without.players[0].gold = 1000;
        g.apply(0, Command::EndTurn).unwrap();
        without.apply(0, Command::EndTurn).unwrap();
        assert_eq!(without.players[0].gold - g.players[0].gold, g.unit_upkeep(k));
        // Broke: the musketman goes.
        g.players[0].gold = -1000;
        let events = g.apply(0, Command::EndTurn).unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::UnitDisbanded { player: 0, kind } if *kind == k)));
        assert_eq!(g.units.values().filter(|u| u.kind == k).count(), 2);
    }

    #[test]
    fn mali_buys_for_less() {
        let (g, city) = game_with_city("mali");
        let library = Build::Building(g.rules.civ_building(0, "library").unwrap());
        let full = gold_for(g.build_cost(library));
        assert_eq!(g.buy_price(city, library), full - full / 4);
    }

    #[test]
    fn tiles_next_to_the_borders_can_be_bought() {
        let (mut g, city) = game_with_city("egypt");
        let tiles = g.buyable_tiles(city);
        assert!(!tiles.is_empty());
        let centre = g.cities[&city].tile;
        assert!(tiles.iter().all(|&t| g.map.grid.distance(t, centre) == 2), "the first ring is owned, the second is next to it");
        let tile = tiles[0];
        let price = g.tile_price(city, tile);
        g.players[0].gold = price;
        g.apply(0, Command::BuyTile { city, tile }).unwrap();
        assert_eq!(g.district(tile), Some(city));
        assert_eq!(g.players[0].gold, 0);
        // Owned now, and the next one costs more.
        assert_eq!(g.tile_problem(city, tile), Some("that tile belongs to a city already"));
        let next = g.buyable_tiles(city).into_iter().find(|&t| g.map.grid.distance(t, centre) == 2).unwrap();
        let fresh = {
            let mut h = g.clone();
            h.set_territory(tile, None);
            h.tile_price(city, next)
        };
        assert!(g.tile_price(city, next) > fresh);
        let far = g.map.grid.within(centre, 6).into_iter().find(|&t| g.map.grid.distance(t, centre) == 5).unwrap();
        g.players[0].gold = 10_000;
        assert!(g.tile_problem(city, far).is_some());
    }
}
