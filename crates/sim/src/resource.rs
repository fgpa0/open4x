//! Map resources (see `data/resources.ron`): bonus resources make better
//! tiles, luxuries add gold in every city for each kind a player has, and
//! strategic resources are needed to train some units.
//!
//! A resource is a player's once they can see it (its tech), it's inside
//! their borders and it's worked by its improvement (water resources need
//! none). Luxuries and strategic resources can be traded: each deal hands one
//! copy over for [`RESOURCE_TURNS`] turns, and lapses early if the giver no
//! longer has enough of them.

use crate::city::{Build, MAX_CITY_RADIUS};
use crate::game::{Game, PlayerId};
use crate::hex::{MapGrid, TileId};
use crate::map::{Elevation, Map, Tile};
use crate::rules::{ResourceId, ResourceKind, Rules, Yields};
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

/// How many turns a traded resource stays with the other side.
pub const RESOURCE_TURNS: u32 = 30;

/// One copy of a resource handed over in a deal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDeal {
    pub from: PlayerId,
    pub to: PlayerId,
    pub resource: ResourceId,
    pub turns_left: u32,
}

impl Game {
    /// The resource on `tile` if `player` knows how to see it.
    pub fn visible_resource(&self, player: PlayerId, tile: TileId) -> Option<ResourceId> {
        let r = self.map.tile(tile).resource?;
        self.rules.resource_tech(r).is_none_or(|t| self.knows(player, t)).then_some(r)
    }

    /// Yields the resource on `tile` gives `player`.
    pub fn resource_yields(&self, player: PlayerId, tile: TileId) -> Yields {
        self.visible_resource(player, tile).map(|r| self.rules.resource(r).yields()).unwrap_or_default()
    }

    /// Whether the resource on `tile` is worked: its improvement is built
    /// there (or it needs none) and the tile is inside someone's borders.
    pub fn resource_connected(&self, tile: TileId) -> bool {
        let Some(r) = self.map.tile(tile).resource else { return false };
        self.territory[tile as usize].is_some()
            && self.rules.resource_improvement(r).is_none_or(|i| self.improvement_at(tile) == Some(i))
    }

    /// Copies of each resource `player` has in their own land, by resource id.
    pub fn resources_owned(&self, player: PlayerId) -> Vec<i32> {
        let mut count = vec![0; self.rules.resources.resources.len()];
        for c in self.cities.values().filter(|c| c.owner == player) {
            for t in self.map.grid.within(c.tile, MAX_CITY_RADIUS) {
                if self.district(t) == Some(c.id) && self.resource_connected(t) {
                    if let Some(r) = self.visible_resource(player, t) {
                        count[r as usize] += 1;
                    }
                }
            }
        }
        count
    }

    /// Copies of each resource `player` can use: their own, plus what others
    /// give them, minus what they give away.
    pub fn resource_counts(&self, player: PlayerId) -> Vec<i32> {
        let mut count = self.resources_owned(player);
        for d in &self.resource_deals {
            if d.to == player {
                count[d.resource as usize] += 1;
            }
            if d.from == player {
                count[d.resource as usize] -= 1;
            }
        }
        count
    }

    pub fn has_resource(&self, player: PlayerId, resource: ResourceId) -> bool {
        self.resource_counts(player)[resource as usize] > 0
    }

    /// Copies `player` could still trade away (their own, less what they've
    /// already given).
    pub fn spare_resource(&self, player: PlayerId, resource: ResourceId) -> i32 {
        let given = self.resource_deals.iter().filter(|d| d.from == player && d.resource == resource).count() as i32;
        self.resources_owned(player)[resource as usize] - given
    }

    /// How many different luxuries `player` has.
    pub fn luxuries(&self, player: PlayerId) -> i32 {
        let counts = self.resource_counts(player);
        (0..counts.len()).filter(|&r| counts[r] > 0 && self.rules.resource(r as ResourceId).kind == ResourceKind::Luxury).count() as i32
    }

    /// Gold the luxuries a player has add in each of their cities.
    pub fn luxury_gold(&self, player: PlayerId) -> i32 {
        self.luxuries(player) * self.rules.resources.luxury_gold
    }

    /// Whether `player` has the strategic resource a build needs.
    pub fn has_resources_for(&self, player: PlayerId, build: Build) -> bool {
        match build {
            Build::Unit(u) => self.rules.unit_resource(u).is_none_or(|r| self.has_resource(player, r)),
            Build::Building(b) => self.rules.building_resource(b).is_none_or(|r| self.has_resource(player, r)),
            Build::Project(_) | Build::Passage(..) => true,
        }
    }

    /// Turn end: traded resources run out, or go back when the giver lost
    /// the copies they gave.
    pub(crate) fn process_resource_deals(&mut self) {
        for d in &mut self.resource_deals {
            d.turns_left = d.turns_left.saturating_sub(1);
        }
        self.resource_deals.retain(|d| d.turns_left > 0);
        // The newest deals lapse first.
        let mut i = self.resource_deals.len();
        while i > 0 {
            i -= 1;
            let d = self.resource_deals[i];
            if self.spare_resource(d.from, d.resource) < 0 {
                self.resource_deals.remove(i);
            }
        }
    }
}

impl Rules {
    /// Whether resource `r` can appear on a tile (see resources.ron).
    pub fn resource_fits(&self, r: ResourceId, t: &Tile) -> bool {
        let def = self.resource(r);
        let terrain = self.terrain(t.terrain);
        let terrain_ok = if def.terrains.is_empty() { !terrain.water } else { def.terrains.contains(&terrain.id) };
        let feature_ok = match t.feature {
            None => def.features.is_empty(),
            Some(f) => def.features.contains(&self.feature(f).id),
        };
        let elevations: &[Elevation] = if def.elevations.is_empty() { &[Elevation::Flat] } else { &def.elevations };
        let temperature_ok = def.temperature.is_none_or(|(lo, hi)| (lo..=hi).contains(&t.temperature));
        terrain_ok && feature_ok && elevations.contains(&t.elevation) && temperature_ok
    }
}

/// Whether a tile and its neighbours are free of resources.
fn room(grid: &MapGrid, tiles: &[Tile], id: TileId) -> bool {
    tiles[id as usize].resource.is_none() && grid.neighbors(id).all(|n| tiles[n as usize].resource.is_none())
}

/// Scatters resources over a generated map: luxuries in small clusters first,
/// then strategic and bonus resources, never next to another resource
/// (except within a cluster).
pub fn place_resources(rules: &Rules, map: &mut Map) {
    let mut rng = ChaCha8Rng::seed_from_u64(map.seed ^ 0x2e50_u64.rotate_left(40));
    let grid = map.grid;
    let land = map.tiles.iter().filter(|t| !rules.terrain(t.terrain).water).count() as i32;
    let ids: Vec<ResourceId> = (0..rules.resources.resources.len() as ResourceId).collect();
    let mut order: Vec<TileId> = (0..map.tiles.len() as TileId).collect();
    for kind in [ResourceKind::Luxury, ResourceKind::Strategic, ResourceKind::Bonus] {
        for &r in ids.iter().filter(|&&r| rules.resource(r).kind == kind) {
            let def = rules.resource(r);
            order.shuffle(&mut rng);
            let tiles = &mut map.tiles;
            if kind == ResourceKind::Luxury {
                let clusters = (land * def.abundance + 500) / 1000;
                let mut placed = 0;
                for &t in &order {
                    if placed >= clusters {
                        break;
                    }
                    if !rules.resource_fits(r, &tiles[t as usize]) || !room(&grid, tiles, t) {
                        continue;
                    }
                    tiles[t as usize].resource = Some(r);
                    let extra = rng.gen_range(1..=3);
                    let mut near: Vec<TileId> = grid.within(t, 2);
                    near.shuffle(&mut rng);
                    let mut added = 0;
                    for n in near {
                        if added < extra && tiles[n as usize].resource.is_none() && rules.resource_fits(r, &tiles[n as usize]) {
                            tiles[n as usize].resource = Some(r);
                            added += 1;
                        }
                    }
                    placed += 1;
                }
            } else {
                for &t in &order {
                    if rules.resource_fits(r, &tiles[t as usize]) && room(&grid, tiles, t) && rng.gen_range(0..100) < def.abundance {
                        tiles[t as usize].resource = Some(r);
                    }
                }
            }
        }
    }
}

impl Game {
    /// Desert tiles around a start that become oases (at most 3, nearest
    /// first) until the tiles within 2 yield `player` about 2 food each.
    pub(crate) fn start_oases(&self, player: PlayerId, start: TileId) -> Vec<TileId> {
        let rules = &self.rules;
        let map = &self.map;
        let (Some(oasis), Some(desert)) = (rules.feature_id("oasis"), rules.terrain_id("desert")) else {
            return Vec::new();
        };
        let mut area = map.grid.within(start, 2);
        let mut short = 2 * area.len() as i32 - 2 - area.iter().map(|&t| (map.yields(rules, t) + self.civ_tile_bonus(player, t)).food).sum::<i32>();
        area.sort_by_key(|&t| (map.grid.distance(start, t), t));
        let food = rules.feature(oasis).food;
        let mut out = Vec::new();
        for t in area {
            if short <= 0 || out.len() == 3 {
                break;
            }
            let tile = map.tile(t);
            if t != start && tile.terrain == desert && tile.elevation == Elevation::Flat && tile.feature.is_none() && tile.resource.is_none() {
                out.push(t);
                short -= food;
            }
        }
        out
    }

    /// Makes sure every start has enough food (oases in the desert, see
    /// [`Game::start_oases`]), some food resources nearby, a luxury within
    /// reach and a strategic resource not too far off, adding them where the
    /// map has none.
    pub(crate) fn balance_start(&mut self, player: PlayerId, start: TileId) {
        if let Some(oasis) = self.rules.feature_id("oasis") {
            for t in self.start_oases(player, start) {
                self.map.tiles[t as usize].feature = Some(oasis);
            }
        }
        let rules = &self.rules;
        let grid = self.map.grid;
        let kinds = |kind: ResourceKind, food: bool| -> Vec<ResourceId> {
            (0..rules.resources.resources.len() as ResourceId)
                .filter(|&r| rules.resource(r).kind == kind && (!food || rules.resource(r).food > 0))
                .collect()
        };
        let wants = [
            (kinds(ResourceKind::Bonus, true), 2, 2),
            (kinds(ResourceKind::Luxury, false), 3, 1),
            (kinds(ResourceKind::Strategic, false), 5, 1),
        ];
        for (options, radius, needed) in wants {
            let mut area: Vec<TileId> = grid.within(start, radius).into_iter().filter(|&t| t != start).collect();
            let have = area.iter().filter(|&&t| self.map.tile(t).resource.is_some_and(|r| options.contains(&r))).count();
            // Nearest first, so starts get them within easy reach.
            area.sort_by_key(|&t| (grid.distance(start, t), t));
            for _ in have..needed {
                let spot = area.iter().find_map(|&t| {
                    let tile = self.map.tile(t);
                    let free = tile.resource.is_none() && grid.neighbors(t).all(|n| n == start || self.map.tile(n).resource.is_none());
                    free.then(|| options.iter().copied().find(|&r| rules.resource_fits(r, tile)).map(|r| (t, r))).flatten()
                });
                let Some((t, r)) = spot else { break };
                self.map.tiles[t as usize].resource = Some(r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Command, GameOptions, GameSetup};
    use crate::improvement::Work;
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Ability;

    #[test]
    fn maps_have_every_kind_of_resource() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let land = map.tiles.iter().filter(|t| !rules.terrain(t.terrain).water).count();
        let with = map.tiles.iter().filter(|t| t.resource.is_some()).count();
        assert!(with * 100 / land >= 4 && with * 100 / land <= 20, "{with} resources on {land} land tiles");
        for kind in [ResourceKind::Bonus, ResourceKind::Luxury, ResourceKind::Strategic] {
            assert!(map.tiles.iter().any(|t| t.resource.is_some_and(|r| rules.resource(r).kind == kind)), "{kind:?}");
        }
        for (i, t) in map.tiles.iter().enumerate() {
            if let Some(r) = t.resource {
                assert!(rules.resource_fits(r, t), "{} on tile {i}", rules.resource(r).id);
            }
        }
    }

    fn game() -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let civs = ["netherlands", "rome"].iter().map(|c| rules.civ_id(c).unwrap()).collect();
        let mut g = Game::with_setup(rules, map, &GameSetup { civs, options: GameOptions::default(), starts: Vec::new() });
        for p in &mut g.players {
            p.techs.fill(true);
        }
        g
    }

    #[test]
    fn starts_have_resources_nearby() {
        let g = game();
        for p in 0..2 {
            let start = g.units.values().find(|u| u.owner == p && g.unit_def(u).has(Ability::FoundCity)).unwrap().tile;
            let kind_within = |kind: ResourceKind, r: i32| {
                g.map.grid.within(start, r).iter().any(|&t| g.map.tile(t).resource.is_some_and(|x| g.rules.resource(x).kind == kind))
            };
            assert!(kind_within(ResourceKind::Bonus, 2) && kind_within(ResourceKind::Luxury, 3) && kind_within(ResourceKind::Strategic, 5));
        }
    }

    #[test]
    fn horses_unlock_horsemen_and_can_be_traded() {
        let mut g = game();
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let city = *g.cities.keys().next().unwrap();
        let horses = g.rules.resource_id("horses").unwrap();
        let pasture = g.rules.improvement_id("pasture").unwrap();
        // Knights replace Horsemen once known, and need Horses just the same.
        let horseman = Build::Unit(g.rules.unit_type_id("knight").unwrap());
        // Clear away any resources the city has, and put horses next to it.
        for t in g.map.grid.within(g.cities[&city].tile, 3) {
            g.map.tiles[t as usize].resource = None;
        }
        let tile = g.map.grid.neighbors(g.cities[&city].tile).find(|&t| g.district(t) == Some(city)).unwrap();
        g.map.tiles[tile as usize].resource = Some(horses);
        assert!(!g.available_builds(city).contains(&horseman));
        assert!(g.apply(0, Command::SetQueue { city, queue: vec![horseman] }).is_err());
        // Not connected until the pasture is built.
        assert_eq!(g.resource_counts(0)[horses as usize], 0);
        let worker = g.spawn_unit(0, g.rules.unit_type_id("worker").unwrap(), tile);
        g.apply(0, Command::Build { unit: worker, work: Work::Improvement(pasture) }).unwrap();
        for _ in 0..5 {
            g.apply(0, Command::EndTurn).unwrap();
        }
        assert_eq!(g.improvement_at(tile), Some(pasture));
        assert!(g.has_resource(0, horses));
        assert!(g.has_resources_for(0, horseman));

        // Handed to Rome: Rome has it, we don't.
        g.players[0].met[1] = true;
        g.players[1].met[0] = true;
        let deal = crate::diplomacy::Deal { give: vec![crate::diplomacy::Item::Resource(horses)], ..Default::default() };
        g.apply(0, Command::ProposeDeal { to: 1, deal }).unwrap();
        g.apply(1, Command::AcceptDeal { from: 0 }).unwrap();
        assert!(g.has_resource(1, horses) && !g.has_resource(0, horses));
        assert_eq!(g.spare_resource(0, horses), 0);
        // Losing the pasture ends the deal.
        g.works[tile as usize].improvement = None;
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.resource_deals.is_empty());
    }

    #[test]
    fn factories_need_coal() {
        let mut g = game();
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        g.players[0].techs.fill(true);
        let factory = crate::city::Build::Building(g.rules.building_id("factory").unwrap());
        let coal = g.rules.resource_id("coal").unwrap();
        g.resource_deals.clear();
        for t in &mut g.map.tiles {
            if t.resource == Some(coal) {
                t.resource = None;
            }
        }
        assert!(!g.has_resources_for(0, factory));
        g.resource_deals.push(ResourceDeal { from: 1, to: 0, resource: coal, turns_left: 10 });
        assert!(g.has_resources_for(0, factory));
    }

    #[test]
    fn tanks_and_power_plants_need_oil() {
        let mut g = game();
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        g.players[0].techs.fill(true);
        let tank = crate::city::Build::Unit(g.rules.unit_type_id("tank").unwrap());
        let plant = crate::city::Build::Building(g.rules.building_id("power_plant").unwrap());
        let oil = g.rules.resource_id("oil").unwrap();
        g.resource_deals.clear();
        for t in &mut g.map.tiles {
            if t.resource == Some(oil) {
                t.resource = None;
            }
        }
        assert!(!g.has_resources_for(0, tank));
        assert!(!g.has_resources_for(0, plant));
        g.resource_deals.push(ResourceDeal { from: 1, to: 0, resource: oil, turns_left: 10 });
        assert!(g.has_resources_for(0, tank));
        assert!(g.has_resources_for(0, plant));
    }

    #[test]
    fn luxuries_add_gold_in_every_city() {
        let mut g = game();
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let city = *g.cities.keys().next().unwrap();
        for t in g.map.grid.within(g.cities[&city].tile, 3) {
            g.map.tiles[t as usize].resource = None;
        }
        let before = g.city_output(city).yields.gold;
        let pearls = g.rules.resource_id("pearls").unwrap();
        let coast = g.map.grid.within(g.cities[&city].tile, 1).into_iter().find(|&t| g.map.is_water(&g.rules, t));
        let Some(coast) = coast else { return };
        g.set_territory(coast, Some(0));
        g.map.tiles[coast as usize].resource = Some(pearls);
        assert_eq!(g.luxuries(0), 0, "pearls need fishing boats");
        let boats = g.rules.improvement_id("fishing_boats").unwrap();
        g.players[0].techs.fill(true);
        assert_eq!(g.tile_work_problem(0, coast, crate::improvement::Work::Improvement(boats)), None, "built on the water");
        assert!(g.tile_work_problem(0, coast, crate::improvement::Work::Road).is_some(), "no roads on water");
        g.works[coast as usize].improvement = Some(boats);
        assert_eq!(g.luxuries(0), 1);
        assert!(g.city_output(city).yields.gold >= before + g.rules.resources.luxury_gold);
    }
}
