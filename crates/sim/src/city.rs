//! Cities: founding, territory, worked tiles, growth, production and borders.
//!
//! Each turn a city's governor picks which tiles its citizens work (following
//! the city's focus), food grows the population, production finishes the front
//! of the build queue, gold goes to the treasury and culture slowly pushes the
//! borders outwards. When the queue runs empty and automation is on, the
//! governor picks something sensible to build; the player can always override.
//!
//! A project ([`Project`]) at the end of the queue turns the city's production
//! into food, gold, science or culture every turn, until something else is
//! queued. The human player's governor never builds units: with nothing left
//! to build it starts a project.

use crate::civ::{CityCond, Effect};
use crate::game::{Event, Game, PlayerId, UnitId};
use crate::hex::TileId;
use crate::map::Elevation;
use crate::rules::{Ability, BuildingId, Domain, UnitClass, UnitTypeId, Yields};
use serde::{Deserialize, Serialize};

pub type CityId = u32;

/// Cities must be at least this many tiles apart.
pub const MIN_CITY_DISTANCE: i32 = 4;
/// Furthest a city's borders (and worked tiles) reach.
pub const MAX_CITY_RADIUS: i32 = 3;
/// Food each citizen eats per turn.
pub const FOOD_PER_CITIZEN: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Build {
    Unit(UnitTypeId),
    Building(BuildingId),
    /// Repeats until something else is queued; never finishes.
    Project(Project),
    /// A canal or tunnel on a tile of the city's land (see `improvement.rs`).
    Passage(crate::improvement::Passage, TileId),
}

/// What a project turns the city's production into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Project {
    Food,
    Gold,
    Science,
    Culture,
}

impl Project {
    pub const ALL: [Project; 4] = [Project::Food, Project::Gold, Project::Science, Project::Culture];

    pub fn name(self) -> &'static str {
        match self {
            Project::Food => "Harvest",
            Project::Gold => "Wealth",
            Project::Science => "Research",
            Project::Culture => "Festivals",
        }
    }

    /// Of each point of production, the percentage that becomes the project's yield.
    pub fn percent(self) -> i32 {
        match self {
            Project::Food => 30,
            Project::Gold | Project::Science | Project::Culture => 50,
        }
    }

    /// The yield `production` points of production turn into (rounded up, so
    /// a small city still gets something).
    pub fn output(self, production: i32) -> i32 {
        (production.max(0) * self.percent() + 99) / 100
    }
}

/// What the governor favours when choosing worked tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Focus {
    Balanced,
    Food,
    Production,
    Gold,
}

impl Focus {
    pub const ALL: [Focus; 4] = [Focus::Balanced, Focus::Food, Focus::Production, Focus::Gold];

    pub fn name(self) -> &'static str {
        match self {
            Focus::Balanced => "Balanced",
            Focus::Food => "Growth",
            Focus::Production => "Production",
            Focus::Gold => "Gold",
        }
    }

    /// The project the governor falls back on when nothing is left to build.
    pub fn project(self) -> Project {
        match self {
            Focus::Food => Project::Food,
            Focus::Gold => Project::Gold,
            Focus::Balanced | Focus::Production => Project::Science,
        }
    }

    /// Weights for (food, production, gold).
    fn weights(self) -> (f32, f32, f32) {
        match self {
            Focus::Balanced => (1.0, 1.1, 0.6),
            Focus::Food => (2.0, 0.8, 0.5),
            Focus::Production => (1.0, 2.0, 0.5),
            Focus::Gold => (1.0, 0.8, 2.0),
        }
    }

    /// Food surplus the governor aims for; beyond it, more food is worth little.
    fn food_target(self, population: i32) -> i32 {
        match self {
            Focus::Balanced => 2 + population / 3,
            Focus::Food => 100,
            Focus::Production => 1,
            Focus::Gold => 2,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct City {
    pub id: CityId,
    pub owner: PlayerId,
    pub name: String,
    pub tile: TileId,
    pub population: i32,
    /// Food stored towards the next citizen.
    pub food: i32,
    /// Production stored towards the front of the queue.
    pub production: i32,
    /// Culture stored towards the next border expansion.
    pub culture: i32,
    pub buildings: Vec<BuildingId>,
    pub queue: Vec<Build>,
    /// Tiles worked by citizens (excluding the city centre, which is always worked).
    pub worked: Vec<TileId>,
    pub focus: Focus,
    /// The governor refills the queue when it runs empty.
    pub auto_queue: bool,
    /// Border expansions so far (each one costs more culture).
    pub expansions: u32,
    /// Hit points, out of [`crate::combat::CITY_MAX_HP`].
    pub hp: i32,
    /// Attacked this turn (cities under attack don't repair).
    #[serde(default)]
    pub attacked: bool,
    /// The player whose first city this was (their original capital).
    #[serde(default)]
    pub capital_of: Option<PlayerId>,
    /// The turn its owner took it and who from: they may raze it until that
    /// turn ends (see `Command::RazeCity`).
    #[serde(default)]
    pub captured: Option<(u32, PlayerId)>,
}

/// What a city makes per turn, after buildings and upkeep.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CityOutput {
    /// Total yields from tiles and buildings.
    pub yields: Yields,
    /// Food left after feeding the citizens (negative means starving).
    pub food_surplus: i32,
    pub culture: i32,
    pub science: i32,
}

impl City {
    pub fn has(&self, b: BuildingId) -> bool {
        self.buildings.contains(&b)
    }
}

/// Food needed for the next citizen.
pub fn growth_threshold(population: i32) -> i32 {
    15 + 8 * (population - 1) + (population - 1).pow(2)
}

/// Culture needed for the next border expansion.
pub fn expansion_threshold(expansions: u32) -> i32 {
    6 + 6 * expansions as i32 + (expansions * expansions) as i32
}

impl Game {
    pub fn build_cost(&self, build: Build) -> i32 {
        match build {
            Build::Unit(u) => self.rules.unit(u).cost,
            Build::Building(b) => self.rules.building(b).cost,
            Build::Project(_) => 0,
            Build::Passage(p, _) => self.rules.passage(p).cost,
        }
    }

    pub fn build_name(&self, build: Build) -> &str {
        match build {
            Build::Unit(u) => &self.rules.unit(u).name,
            Build::Building(b) => &self.rules.building(b).name,
            Build::Project(p) => p.name(),
            Build::Passage(p, _) => p.name(),
        }
    }

    pub fn city_at(&self, tile: TileId) -> Option<&City> {
        self.cities.values().find(|c| c.tile == tile)
    }

    /// The city that works `tile`: its owner's nearest city within reach.
    pub fn district(&self, tile: TileId) -> Option<CityId> {
        self.districts.get(tile as usize).copied().flatten()
    }

    /// Owned tiles worked by `city` (its district), centre included.
    pub fn district_tiles(&self, city: CityId) -> Vec<TileId> {
        let Some(c) = self.cities.get(&city) else { return Vec::new() };
        self.map.grid.within(c.tile, MAX_CITY_RADIUS).into_iter().filter(|&t| self.district(t) == Some(city)).collect()
    }

    /// Works out every tile's district again from `territory` and the cities:
    /// the owner's nearest city within [`MAX_CITY_RADIUS`], the older one on a
    /// tie. Run after any change to who owns land or where cities are.
    pub fn refresh_districts(&mut self) {
        let tiles = self.territory.len();
        let mut best = vec![u8::MAX; tiles];
        let mut districts = vec![None; tiles];
        for c in self.cities.values() {
            for t in self.map.grid.within(c.tile, MAX_CITY_RADIUS) {
                let d = self.map.grid.distance(c.tile, t) as u8;
                if self.territory[t as usize] == Some(c.owner) && d < best[t as usize] {
                    best[t as usize] = d;
                    districts[t as usize] = Some(c.id);
                }
            }
        }
        self.districts = districts;
    }

    /// A city handed over in a deal takes the land it worked with it (its
    /// district as it was), then works its tiles for the new owner.
    pub(crate) fn hand_over_district(&mut self, city: CityId, to: PlayerId) {
        for t in self.district_tiles(city) {
            self.territory[t as usize] = Some(to);
        }
        self.refresh_districts();
        self.cities.get_mut(&city).expect("exists").worked.clear();
        self.assign_worked_tiles(city);
    }

    /// Sets who owns `tile` and keeps the districts in step.
    pub fn set_territory(&mut self, tile: TileId, owner: Option<PlayerId>) {
        self.territory[tile as usize] = owner;
        self.refresh_districts();
    }

    /// Why a city can't be founded on `tile` by `player`, if it can't.
    pub fn found_city_problem(&self, player: PlayerId, tile: TileId) -> Option<&'static str> {
        let t = self.map.tile(tile);
        if self.rules.terrain(t.terrain).water || t.elevation == Elevation::Mountains {
            return Some("cities can't be built here");
        }
        if self.cities.values().any(|c| self.map.grid.distance(c.tile, tile) < MIN_CITY_DISTANCE) {
            return Some("too close to another city");
        }
        if self.territory_owner(tile).is_some_and(|o| o != player) {
            return Some("inside another player's borders");
        }
        None
    }

    /// Whether `player`'s civ builds this at all (not another civ's unique,
    /// not a standard one its own unique replaces).
    pub fn civ_builds(&self, player: PlayerId, build: Build) -> bool {
        let civ = self.players[player as usize].civ;
        match build {
            Build::Unit(u) => self.rules.civ_can_build_unit(civ, u),
            Build::Building(b) => self.rules.civ_can_build_building(civ, b),
            Build::Project(_) | Build::Passage(..) => true,
        }
    }

    /// Builds that `city` can currently produce: its civ's units and
    /// buildings it doesn't have (or have queued) yet, whose technology the
    /// owner knows.
    pub fn available_builds(&self, city: CityId) -> Vec<Build> {
        let c = &self.cities[&city];
        let capped = self.at_unit_limit(c.owner);
        let mut out: Vec<Build> = (0..self.rules.units.units.len() as UnitTypeId)
            .filter(|&u| !capped || self.rules.unit(u).class != UnitClass::Military)
            .filter(|&u| !self.rules.unit(u).has(Ability::Trade) || self.merchant_room(c.owner))
            .filter(|&u| self.upgrade_target(c.owner, u).is_none())
            .map(Build::Unit)
            .collect();
        out.extend(
            (0..self.rules.buildings.buildings.len() as BuildingId)
                .filter(|&b| !c.has(b) && !c.queue.contains(&Build::Building(b)))
                .map(Build::Building),
        );
        out.retain(|&b| {
            self.has_tech_for(c.owner, b) && self.civ_builds(c.owner, b) && self.builds_here(city, b) && self.has_resources_for(c.owner, b)
        });
        out
    }

    /// Whether `city` is somewhere it could build this (see [`Game::place_problem`]).
    pub fn builds_here(&self, city: CityId, build: Build) -> bool {
        self.place_problem(city, build).is_none()
    }

    /// Why `city` can't build this, whatever its owner knows: ships need the
    /// coast, and some buildings need another one first.
    pub fn place_problem(&self, city: CityId, build: Build) -> Option<&'static str> {
        let c = &self.cities[&city];
        match build {
            Build::Unit(u) if self.rules.unit(u).domain == Domain::Sea && !self.is_coastal(c.tile) => Some("ships are built only in coastal cities"),
            Build::Building(b) if self.rules.building(b).coastal && !self.is_coastal(c.tile) => Some("only in coastal cities"),
            Build::Passage(p, tile) => self.passage_problem(city, p, tile),
            Build::Building(b) => self
                .rules
                .building_requires(b)
                .filter(|&r| !c.buildings.iter().any(|&h| self.rules.building_is(h, &self.rules.building(r).id)))
                .map(|_| "needs another building in the city first"),
            _ => None,
        }
    }

    pub(crate) fn found_city(&mut self, unit: UnitId) -> Vec<Event> {
        let u = self.units.remove(&unit).expect("checked by caller");
        let id = self.next_city;
        self.next_city += 1;
        let civ = self.players[u.owner as usize].civ;
        let names = &self.rules.civ(civ).cities;
        let n = self.players[u.owner as usize].cities_founded as usize;
        let capital_of = (n == 0).then_some(u.owner);
        let name = if n < names.len() { names[n].clone() } else { format!("New {}", names[n % names.len()]) };
        self.players[u.owner as usize].cities_founded += 1;
        let free: Vec<BuildingId> = self
            .civ_effects(u.owner)
            .filter_map(|e| if let Effect::FreeBuilding(b) = e { self.civ_building(u.owner, b) } else { None })
            .collect();
        let mut city = City {
            id,
            owner: u.owner,
            name,
            tile: u.tile,
            population: 1,
            food: 0,
            production: 0,
            culture: 0,
            buildings: free,
            queue: Vec::new(),
            worked: Vec::new(),
            focus: Focus::Balanced,
            auto_queue: true,
            expansions: 0,
            hp: crate::combat::CITY_MAX_HP,
            attacked: false,
            capital_of,
            captured: None,
        };
        // Initial territory: the centre and its neighbours not owned by others.
        for t in self.map.grid.within(u.tile, 1) {
            if self.territory[t as usize].is_none() {
                self.territory[t as usize] = Some(u.owner);
            }
        }
        city.worked.clear();
        self.cities.insert(id, city);
        self.refresh_districts();
        self.works[u.tile as usize].road = true;
        let owner = u.owner;
        self.rail_stations(owner);
        self.road_to_capital(id);
        self.assign_worked_tiles(id);
        // The queue starts empty: the owner chooses, or the governor does when
        // the turn ends (see `process_city`).
        let owner = u.owner;
        let mut events = vec![
            Event::UnitRemoved { unit },
            Event::CityFounded { city: id },
            Event::BordersChanged { player: owner },
        ];
        events.extend(self.update_visibility(owner));
        events
    }

    /// What a tile yields for `player`: terrain, the resources they can
    /// see, improvements and civ perks.
    pub fn tile_yields(&self, player: PlayerId, tile: TileId) -> Yields {
        self.map.yields(&self.rules, tile)
            + self.resource_yields(player, tile)
            + self.improvement_yields(tile)
            + self.civ_tile_bonus(player, tile)
    }

    /// Yields of a tile when worked by `city` (with its buildings' tile
    /// bonuses; the city centre gets a guaranteed minimum).
    pub fn worked_tile_yields(&self, city: CityId, tile: TileId) -> Yields {
        let c = &self.cities[&city];
        let mut y = self.tile_yields(c.owner, tile);
        for &b in &c.buildings {
            if let Some(bonus) = &self.rules.building(b).tile_bonus {
                if self.rules.tile_matches(&self.map, tile, self.improvement_at(tile), &bonus.when) {
                    y += Yields { food: bonus.food, production: bonus.production, gold: bonus.gold };
                }
            }
        }
        if tile == c.tile {
            Yields { food: y.food.max(2), production: y.production.max(2), gold: y.gold.max(1) }
        } else {
            y
        }
    }

    pub fn city_output(&self, city: CityId) -> CityOutput {
        let c = &self.cities[&city];
        let mut yields = self.worked_tile_yields(city, c.tile);
        for &t in &c.worked {
            yields += self.worked_tile_yields(city, t);
        }
        let mut culture = 1 + c.population / 3;
        // Citizens think: one science each, plus one for the city itself.
        let mut science = 1 + c.population;
        for &b in &c.buildings {
            let def = self.rules.building(b);
            yields += Yields { food: def.food, production: def.production, gold: def.gold };
            culture += def.culture;
            science += def.science;
        }
        yields.gold += self.luxury_gold(c.owner);
        let mut gold_percent = 0;
        for e in self.civ_effects(c.owner) {
            match e {
                Effect::CityYield { when: CityCond::Coastal, food, production, gold } if self.is_coastal(c.tile) => {
                    yields += Yields { food: *food, production: *production, gold: *gold };
                }
                Effect::BuildingBonus { building, food, production, gold, science: s, culture: k } => {
                    if c.buildings.iter().any(|&b| self.rules.building_is(b, building)) {
                        yields += Yields { food: *food, production: *production, gold: *gold };
                        science += s;
                        culture += k;
                    }
                }
                Effect::GoldPerCity(g) => yields.gold += g,
                Effect::GoldPercent(p) => gold_percent += p,
                _ => {}
            }
        }
        let mut science_from_project = 0;
        let mut culture_from_project = 0;
        if let Some(&Build::Project(p)) = c.queue.first() {
            let made = p.output(yields.production);
            match p {
                Project::Food => yields.food += made,
                Project::Gold => yields.gold += made,
                Project::Science => science_from_project = made,
                Project::Culture => culture_from_project = made,
            }
        }
        science += science_from_project;
        culture += culture_from_project;
        yields.gold += yields.gold * gold_percent / 100;
        CityOutput { yields, food_surplus: yields.food - FOOD_PER_CITIZEN * c.population, culture, science }
    }

    /// Production a city puts into `build` per turn, with bonuses towards it.
    pub fn production_towards(&self, city: CityId, build: Build) -> i32 {
        let out = self.city_output(city);
        self.production_towards_from(city, build, out.yields.production)
    }

    /// [`Self::production_towards`] with `base` production kept by the city.
    fn production_towards_from(&self, city: CityId, build: Build, base: i32) -> i32 {
        if let Build::Project(_) = build {
            return 0;
        }
        let c = &self.cities[&city];
        let percent: i32 = match build {
            Build::Unit(u) if self.rules.unit(u).class == UnitClass::Military => {
                let def = self.rules.unit(u);
                c.buildings.iter().map(|&b| self.rules.building(b)).filter(|b| b.units.matches(def)).map(|b| b.military_production).sum()
            }
            Build::Unit(_) | Build::Project(_) | Build::Passage(..) => 0,
            Build::Building(_) => self
                .civ_effects(c.owner)
                .map(|e| match e {
                    Effect::BuildingProduction { percent, min_population } if c.population >= *min_population => *percent,
                    _ => 0,
                })
                .sum(),
        };
        base + base * percent / 100
    }

    /// Experience a unit of `kind` starts with when trained or bought in `city`.
    pub fn trained_xp(&self, city: CityId, kind: UnitTypeId) -> u32 {
        let def = self.rules.unit(kind);
        if def.has(Ability::General) {
            return 0;
        }
        self.cities[&city].buildings.iter().map(|&b| self.rules.building(b)).filter(|b| b.units.matches(def)).map(|b| b.unit_xp).sum()
    }

    /// Turns until the city grows (`None` if it isn't growing).
    pub fn turns_to_grow(&self, city: CityId) -> Option<i32> {
        let c = &self.cities[&city];
        let surplus = self.city_output(city).food_surplus;
        (surplus > 0).then(|| ((growth_threshold(c.population) - c.food).max(0) + surplus - 1) / surplus)
    }

    /// Turns until the city's borders next grow (`None` if it makes no culture).
    pub fn turns_to_expand(&self, city: CityId) -> Option<i32> {
        let c = &self.cities[&city];
        let culture = self.city_output(city).culture;
        (culture > 0).then(|| ((expansion_threshold(c.expansions) - c.culture).max(0) + culture - 1) / culture)
    }

    /// Turns until the front of the queue is finished.
    pub fn turns_to_build(&self, city: CityId) -> Option<i32> {
        let c = &self.cities[&city];
        let build = *c.queue.first().filter(|b| !matches!(b, Build::Project(_)))?;
        let prod = self.production_towards(city, build).max(1);
        Some(((self.build_cost(build) - c.production).max(0) + prod - 1) / prod)
    }

    /// Tiles a city could work: inside its borders, within reach, not another
    /// city's centre or worked by another city.
    fn workable_tiles(&self, city: CityId) -> Vec<TileId> {
        let c = &self.cities[&city];
        self.map
            .grid
            .within(c.tile, MAX_CITY_RADIUS)
            .into_iter()
            .filter(|&t| {
                t != c.tile
                    && self.district(t) == Some(city)
                    && self.city_at(t).is_none()
                    && !self.cities.values().any(|o| o.id != city && o.worked.contains(&t))
            })
            .collect()
    }

    /// The governor: puts each citizen on the best available tile for the focus,
    /// making sure the city doesn't starve if it can help it.
    pub fn assign_worked_tiles(&mut self, city: CityId) {
        let mut candidates = self.workable_tiles(city);
        let c = &self.cities[&city];
        let (wf, wp, wg) = c.focus.weights();
        let population = c.population;
        let mut food = self.worked_tile_yields(city, c.tile).food
            + c.buildings.iter().map(|&b| self.rules.building(b).food).sum::<i32>();
        let target = c.focus.food_target(population);
        let mut worked = Vec::new();
        for i in 0..population {
            // Expected surplus if this and the remaining citizens work average
            // (2 food) tiles. Below the target, food comes first; above it, extra
            // food is worth little and the focus decides.
            let surplus = food + (population - i) * FOOD_PER_CITIZEN - FOOD_PER_CITIZEN * population;
            let (fw, pw, gw) = if surplus < target { (3.0, 1.0, 0.3) } else { (wf * 0.35, wp, wg) };
            let best = candidates.iter().enumerate().max_by(|(_, &a), (_, &b)| {
                let score = |t: TileId| {
                    let y = self.worked_tile_yields(city, t);
                    y.food as f32 * fw + y.production as f32 * pw + y.gold as f32 * gw
                };
                score(a).total_cmp(&score(b)).then(b.cmp(&a))
            });
            let Some((idx, &tile)) = best else { break };
            candidates.swap_remove(idx);
            food += self.worked_tile_yields(city, tile).food;
            worked.push(tile);
        }
        self.cities.get_mut(&city).expect("exists").worked = worked;
    }

    /// When the queue is empty at the end of a turn, the governor picks the
    /// next thing to build.
    fn refill_queue(&mut self, city: CityId) {
        let c = &self.cities[&city];
        if !c.queue.is_empty() || !c.auto_queue {
            return;
        }
        let owner = c.owner;
        // The human's governor leaves units to the player.
        let human = self.players[owner as usize].human;
        let unit = |id: &str| self.civ_unit(owner, id).map(Build::Unit).filter(|&b| self.has_tech_for(owner, b));
        let building = |id: &str| {
            self.civ_building(owner, id)
                .filter(|&b| !c.has(b))
                .map(Build::Building)
                .filter(|&b| self.has_tech_for(owner, b) && self.builds_here(city, b))
        };
        let my_cities = self.cities.values().filter(|o| o.owner == owner).count();
        let settlers_planned = self.units.values().filter(|u| u.owner == owner && self.unit_def(u).has(Ability::FoundCity)).count()
            + self.cities.values().filter(|o| o.owner == owner && o.queue.first().is_some_and(|b| Some(*b) == unit("settler"))).count();

        let pick = if !human && c.population >= 3 && my_cities + settlers_planned < 6 && settlers_planned == 0 {
            unit("settler")
        } else {
            let order: &[&str] = match c.focus {
                Focus::Food => &["granary", "monument", "harbor", "aqueduct", "library", "workshop", "market", "amphitheater", "temple"],
                Focus::Production => &["workshop", "monument", "granary", "library", "market", "harbor", "aqueduct", "amphitheater", "temple"],
                Focus::Gold => &["market", "bank", "monument", "workshop", "granary", "library", "harbor", "temple", "amphitheater"],
                Focus::Balanced => &["monument", "granary", "library", "workshop", "market", "harbor", "temple", "aqueduct", "amphitheater"],
            };
            // Then whatever comes later, the same for every focus.
            let later: &[&str] = &[
                "shrine", "university", "bank", "aqueduct", "caravanserai", "shipyard", "observatory", "opera_house", "factory", "public_school",
                "stock_exchange", "museum", "hospital", "supermarket", "power_plant", "research_lab", "broadcast_tower", "lighthouse", "walls",
                "castle", "star_fort", "arsenal", "military_base",
            ];
            order
                .iter()
                .chain(later)
                .find_map(|b| building(b))
                // Everything built: the human's cities start a project; computer
                // players' production then turns into gold.
                .or_else(|| human.then(|| Build::Project(c.focus.project())))
        };
        if let Some(b) = pick {
            self.cities.get_mut(&city).expect("exists").queue.push(b);
        }
    }

    /// A free tile at or next to the city for a new unit of `kind`.
    pub(crate) fn spawn_tile(&self, city: CityId, kind: UnitTypeId) -> Option<TileId> {
        let c = &self.cities[&city];
        let probe = crate::game::Unit {
            id: 0,
            owner: c.owner,
            kind,
            tile: c.tile,
            moves: 0,
            order: crate::game::Order::None,
            hp: crate::combat::MAX_HP,
            attacks: 0,
            moved: false,
            alerted: None,
            name: None,
            xp: 0,
            traits: Vec::new(),
        };
        // Land units start ashore; ships in the harbour or on the water next to it.
        let mover = self.mover(&probe);
        let fits = |t: TileId| match mover.domain {
            Domain::Land => self.map.land_move_cost(&self.rules, t).is_some(),
            Domain::Sea => mover.can_enter(&self.map, &self.rules, t),
        };
        std::iter::once(c.tile).chain(self.map.grid.neighbors(c.tile)).find(|&t| fits(t) && self.can_stop_at(&probe, t))
    }

    /// One turn of a city: work tiles, grow, build, earn gold and culture.
    pub(crate) fn process_city(&mut self, city: CityId) -> Vec<Event> {
        let mut events = Vec::new();
        self.assign_worked_tiles(city);
        let out = self.city_output(city);
        let keeps_food = self.cities[&city].buildings.iter().map(|&b| self.rules.building(b).keeps_food).max().unwrap_or(0);

        // Growth (or starvation).
        let c = self.cities.get_mut(&city).expect("exists");
        c.food += out.food_surplus;
        if c.food >= growth_threshold(c.population) {
            let kept = growth_threshold(c.population) * keeps_food / 100;
            c.food = (c.food - growth_threshold(c.population)).max(0) + kept;
            c.population += 1;
            events.push(Event::CityGrew { city, population: c.population });
        } else if c.food < 0 {
            c.food = 0;
            if c.population > 1 {
                c.population -= 1;
                events.push(Event::CityGrew { city, population: c.population });
            }
        }
        let owner = c.owner;
        self.players[owner as usize].gold += out.yields.gold;

        // Production. The governor fills an empty queue only now, so the owner
        // has the whole turn to choose. With nothing to build, half of it
        // becomes gold. A project takes it all (its yield is in `city_output`).
        // Canals and tunnels whose site is gone (lost land, a canal there
        // already) are dropped; production carries over as usual.
        let gone: Vec<Build> =
            self.cities[&city].queue.iter().copied().filter(|&b| matches!(b, Build::Passage(..)) && !self.builds_here(city, b)).collect();
        self.cities.get_mut(&city).expect("exists").queue.retain(|b| !gone.contains(b));
        self.refill_queue(city);
        let on_project = matches!(self.cities[&city].queue.first(), Some(Build::Project(_)));
        // Stored production is never lost: like in Civ, cancelling or switching
        // carries it over to whatever is built next.
        let kept = out.yields.production;
        let towards = self.cities[&city].queue.first().map(|&b| self.production_towards_from(city, b, kept));
        let c = self.cities.get_mut(&city).expect("exists");
        match towards {
            None => self.players[owner as usize].gold += kept / 2,
            Some(p) => c.production += p,
        }
        if let Some(&build) = c.queue.first().filter(|_| !on_project) {
            let cost = self.build_cost(build);
            if self.cities[&city].production >= cost {
                let done = match build {
                    Build::Project(_) => false,
                    Build::Passage(p, tile) => {
                        events.extend(self.build_passage(tile, p));
                        true
                    }
                    Build::Building(_) | Build::Unit(_) if !self.has_resources_for(owner, build) => false,
                    Build::Building(b) => {
                        self.cities.get_mut(&city).expect("exists").buildings.push(b);
                        true
                    }
                    Build::Unit(kind) if self.rules.unit(kind).class == UnitClass::Military && self.at_unit_limit(owner) => false,
                    // Merchants wait for a city to support them.
                    Build::Unit(kind) if self.rules.unit(kind).has(Ability::Trade) && !self.merchant_room(owner) => false,
                    Build::Unit(kind) => {
                        let settler = self.rules.unit(kind).has(Ability::FoundCity);
                        let pop = self.cities[&city].population;
                        match self.spawn_tile(city, kind) {
                            // A settler takes a citizen with it, so needs one to spare.
                            Some(_) if settler && pop < 2 => false,
                            Some(tile) => {
                                let unit = self.spawn_unit(owner, kind, tile);
                                self.units.get_mut(&unit).expect("just spawned").xp = self.trained_xp(city, kind);
                                events.push(Event::UnitSpawned { unit });
                                if settler {
                                    self.cities.get_mut(&city).expect("exists").population -= 1;
                                }
                                true
                            }
                            None => false,
                        }
                    }
                };
                if done {
                    let c = self.cities.get_mut(&city).expect("exists");
                    c.production -= cost;
                    c.queue.remove(0);
                    events.push(Event::CityBuilt { city, build });
                } else {
                    // Can't finish yet: keep the production, don't overflow forever.
                    let c = self.cities.get_mut(&city).expect("exists");
                    c.production = c.production.min(cost);
                }
            }
        }

        // Culture and border growth.
        let c = self.cities.get_mut(&city).expect("exists");
        c.culture += out.culture;
        if c.culture >= expansion_threshold(c.expansions) {
            if let Some(tile) = self.next_border_tile(city) {
                let c = self.cities.get_mut(&city).expect("exists");
                c.culture -= expansion_threshold(c.expansions);
                c.expansions += 1;
                self.set_territory(tile, Some(owner));
                events.push(Event::BordersChanged { player: owner });
            }
        }
        self.assign_worked_tiles(city);
        events.push(Event::CityChanged { city });
        events
    }

    /// The tile the city's borders grow into next: the most valuable unclaimed
    /// tile next to its territory.
    pub fn next_border_tile(&self, city: CityId) -> Option<TileId> {
        let c = self.cities.get(&city)?;
        self.map
            .grid
            .within(c.tile, MAX_CITY_RADIUS)
            .into_iter()
            .filter(|&t| self.territory[t as usize].is_none() && self.map.grid.neighbors(t).any(|n| self.district(n) == Some(city)))
            .max_by_key(|&t| {
                let y = self.tile_yields(c.owner, t);
                // Prefer good tiles, then closer ones.
                (y.food * 3 + y.production * 2 + y.gold) * 10 - self.map.grid.distance(c.tile, t) * 4 - t as i32 % 3
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Command, CommandError};
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Rules;

    fn game_with_city() -> (Game, CityId) {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut game = Game::new(rules, map, 1);
        // City mechanics are tested apart from research: everything is known.
        game.players[0].techs.fill(true);
        let settler = game.units.values().find(|u| u.owner == 0 && game.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        let events = game.apply(0, Command::FoundCity { unit: settler }).unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::CityFounded { .. })));
        assert!(!game.units.contains_key(&settler), "settler is used up");
        let city = *game.cities.keys().next().unwrap();
        (game, city)
    }

    fn end_turns(game: &mut Game, n: u32) {
        for _ in 0..n {
            game.apply(0, Command::EndTurn).unwrap();
        }
    }

    #[test]
    fn founding_claims_territory_and_leaves_the_choice_to_the_owner() {
        let (mut game, city) = game_with_city();
        let c = &game.cities[&city];
        assert_eq!(c.population, 1);
        let owned = game.district_tiles(city).len();
        assert!(owned >= 6, "only {owned} tiles claimed");
        assert!(c.queue.is_empty(), "the owner picks first");
        assert_eq!(c.worked.len(), 1, "one citizen works one tile");
        // Nothing chosen by the end of the turn: the governor picks, and this
        // turn's production already goes into it.
        game.apply(0, Command::EndTurn).unwrap();
        let c = &game.cities[&city];
        assert!(!c.queue.is_empty(), "governor should queue something");
        assert!(c.production > 0, "the turn's production was lost");
    }

    #[test]
    fn production_carries_over_when_cancelled() {
        let (mut game, city) = game_with_city();
        let walls = Build::Building(game.rules.building_id("walls").unwrap());
        let monument = Build::Building(game.rules.building_id("monument").unwrap());
        game.apply(0, Command::SetAutoQueue { city, on: false }).unwrap();
        game.apply(0, Command::SetQueue { city, queue: vec![walls] }).unwrap();
        game.apply(0, Command::EndTurn).unwrap();
        let stored = game.cities[&city].production;
        assert!(stored > 0);
        // Cancel and leave the queue empty for a turn: the progress stays.
        game.apply(0, Command::SetQueue { city, queue: vec![] }).unwrap();
        game.apply(0, Command::EndTurn).unwrap();
        assert_eq!(game.cities[&city].production, stored);
        // The next pick gets it.
        game.apply(0, Command::SetQueue { city, queue: vec![monument] }).unwrap();
        game.apply(0, Command::EndTurn).unwrap();
        assert!(game.cities[&city].production > stored, "progress went missing");
    }

    #[test]
    fn governor_leaves_a_chosen_queue_alone() {
        let (mut game, city) = game_with_city();
        let walls = Build::Building(game.rules.building_id("walls").unwrap());
        game.apply(0, Command::SetQueue { city, queue: vec![walls] }).unwrap();
        game.apply(0, Command::EndTurn).unwrap();
        assert_eq!(game.cities[&city].queue, vec![walls]);
    }

    #[test]
    fn projects_repeat_and_convert_production() {
        let (mut game, city) = game_with_city();
        let before = game.city_output(city);
        let wealth = Build::Project(Project::Gold);
        game.apply(0, Command::SetQueue { city, queue: vec![wealth] }).unwrap();
        let out = game.city_output(city);
        assert_eq!(out.yields.gold, before.yields.gold + Project::Gold.output(before.yields.production));
        let stored = game.cities[&city].production;
        end_turns(&mut game, 5);
        assert_eq!(game.cities[&city].queue, vec![wealth], "it never finishes");
        assert_eq!(game.cities[&city].production, stored, "no production is stored");
        assert_eq!(game.turns_to_build(city), None);
        // A project only comes last: it runs once the rest is built.
        let walls = Build::Building(game.rules.building_id("walls").unwrap());
        assert!(game.apply(0, Command::SetQueue { city, queue: vec![wealth, walls] }).is_err());
        assert!(game.apply(0, Command::SetQueue { city, queue: vec![wealth, wealth] }).is_err());
        game.apply(0, Command::SetQueue { city, queue: vec![walls] }).unwrap();
        let gold = game.city_output(city).yields.gold;
        game.apply(0, Command::SetQueue { city, queue: vec![walls, wealth] }).unwrap();
        assert_eq!(game.city_output(city).yields.gold, gold, "not while building the walls");
    }

    #[test]
    fn the_players_governor_builds_no_units() {
        let (mut game, city) = game_with_city();
        // Everything already built: the governor starts the focus's project.
        let all: Vec<_> = (0..game.rules.buildings.buildings.len() as BuildingId).collect();
        game.cities.get_mut(&city).unwrap().buildings = all;
        let units = game.units.len();
        end_turns(&mut game, 30);
        assert_eq!(game.units.len(), units, "no units built");
        assert_eq!(game.cities[&city].queue, vec![Build::Project(Focus::Balanced.project())]);
    }

    #[test]
    fn barracks_train_veterans_and_stables_speed_up_horsemen() {
        let (mut game, city) = game_with_city();
        let b = |g: &Game, id: &str| g.rules.building_id(id).unwrap();
        let u = |g: &Game, id: &str| g.rules.unit_type_id(id).unwrap();
        let (spearman, horseman, galley, general) = (u(&game, "spearman"), u(&game, "horseman"), u(&game, "galley"), u(&game, "general"));
        assert_eq!(game.trained_xp(city, spearman), 0);
        let (barracks, armory, stable) = (b(&game, "barracks"), b(&game, "armory"), b(&game, "stable"));
        game.cities.get_mut(&city).unwrap().buildings.extend([barracks, armory]);
        assert_eq!(game.trained_xp(city, spearman), 24, "barracks and armory add up");
        assert_eq!(game.trained_xp(city, galley), 0, "only land units");
        assert_eq!(game.trained_xp(city, general), 0, "Generals don't get levels from drill");
        // Trained there, it starts as a Veteran.
        game.cities.get_mut(&city).unwrap().queue = vec![Build::Unit(spearman)];
        game.cities.get_mut(&city).unwrap().production = 1000;
        let before: Vec<_> = game.units.keys().copied().collect();
        end_turns(&mut game, 1);
        let new = game.units.values().find(|x| !before.contains(&x.id) && x.kind == spearman).expect("trained");
        assert_eq!(new.xp, 24);
        assert_eq!(game.rank(new), 1);
        // The stable speeds up mounted units only (the armory all land units).
        let spear = game.production_towards(city, Build::Unit(spearman));
        let horse = game.production_towards(city, Build::Unit(horseman));
        assert_eq!(spear, horse);
        game.cities.get_mut(&city).unwrap().buildings.push(stable);
        let (spear, horse) = (game.production_towards(city, Build::Unit(spearman)), game.production_towards(city, Build::Unit(horseman)));
        assert!(horse > spear, "{horse} > {spear}");
    }

    #[test]
    fn cities_need_space() {
        let (mut game, city) = game_with_city();
        let tile = game.cities[&city].tile;
        let near = game.map.grid.within(tile, 2).into_iter().find(|&t| game.map.land_move_cost(&game.rules, t).is_some() && t != tile).unwrap();
        let kind = game.rules.unit_type_id("settler").unwrap();
        let settler = game.spawn_unit(0, kind, near);
        let err = game.apply(0, Command::FoundCity { unit: settler }).unwrap_err();
        assert_eq!(err, CommandError::CantFoundCity("too close to another city"));
    }

    #[test]
    fn city_grows_builds_and_expands() {
        let (mut game, city) = game_with_city();
        let units_before = game.units.len();
        end_turns(&mut game, 40);
        let c = &game.cities[&city];
        assert!(c.population >= 3, "population {}", c.population);
        assert!(game.units.len() > units_before || !c.buildings.is_empty(), "built nothing in 40 turns");
        assert!(c.expansions > 0, "borders never grew");
        assert!(game.players[0].gold > 0);
        // Worked tiles are inside the city's own territory.
        for &t in &c.worked {
            assert_eq!(game.district(t), Some(city));
        }
        assert_eq!(c.worked.len() as i32, c.population.min(game.workable_tiles(city).len() as i32 + c.worked.len() as i32));
    }

    #[test]
    fn focus_changes_what_is_worked() {
        let (mut game, city) = game_with_city();
        end_turns(&mut game, 30);
        game.apply(0, Command::SetFocus { city, focus: Focus::Production }).unwrap();
        let prod = game.city_output(city).yields.production;
        game.apply(0, Command::SetFocus { city, focus: Focus::Food }).unwrap();
        let food_prod = game.city_output(city).yields.production;
        assert!(prod >= food_prod);
    }

    #[test]
    fn player_queue_overrides_governor() {
        let (mut game, city) = game_with_city();
        let walls = Build::Building(game.rules.building_id("walls").unwrap());
        game.apply(0, Command::SetQueue { city, queue: vec![walls] }).unwrap();
        assert_eq!(game.cities[&city].queue, vec![walls]);
        end_turns(&mut game, 60);
        assert!(game.cities[&city].has(game.rules.building_id("walls").unwrap()));
    }
}
