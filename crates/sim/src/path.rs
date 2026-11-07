//! Pathfinding for units: A* over tiles with per-turn movement points.
//!
//! Movement rule (as in most 4X games): entering a tile costs its movement
//! points, but a unit with *any* points left may always enter one more tile,
//! ending its turn there. Paths are optimal in (turns, points spent).
//!
//! Units count movement in tenths of a point ([`MOVE`] to a point), so a
//! step along a railway can cost a fraction of one; the rules and the
//! interface speak in whole points.
//!
//! Land units can embark onto water once their owner knows how (see
//! [`Mover`]); embarking and landing each take the rest of the turn. Ships
//! sail on water and can enter their owner's coastal cities.

use crate::climate::SeaIce;
use crate::hex::TileId;
use crate::map::Map;
use crate::rules::{Ability, Domain, Rules, UnitDef};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// A route for a unit, excluding its starting tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    pub tiles: Vec<TileId>,
    /// For each tile, the turn (0 = this turn) in which the unit reaches it.
    pub turns: Vec<u32>,
    /// Movement points left on arrival (in the arrival turn).
    pub left: i32,
}

impl Path {
    /// Turns needed to arrive (0 = this turn).
    pub fn arrival_turn(&self) -> u32 {
        self.turns.last().copied().unwrap_or(0)
    }
}

/// How a unit moves: its type's movement plus what its owner's techs allow.
#[derive(Clone, Debug, PartialEq)]
pub struct Mover {
    /// Movement points per turn.
    pub moves: i32,
    pub domain: Domain,
    pub ignore_terrain: bool,
    /// Land units may embark onto shallow water.
    pub embark: bool,
    /// Ships and embarked units may enter deep water.
    pub deep_water: bool,
    /// Land tiles a ship may enter: its owner's coastal cities.
    pub harbors: Vec<TileId>,
    /// For each tile, the player whose closed borders keep the unit out
    /// ([`OPEN`]: none); empty when nothing is closed. A unit already inside
    /// someone's closed territory may still move within it, to get out.
    pub closed: Vec<u8>,
    /// The point in the seasons (see `climate::season`), for ice on water.
    pub season: f32,
    /// Tiles with a tunnel, in order: land units cross these mountains.
    pub tunnels: Vec<TileId>,
    /// Tiles with a bridge, in order: land units walk over this water
    /// (ships still sail under it).
    pub bridges: Vec<TileId>,
    /// The least a step can cost (a whole point, or a railway's step once
    /// railways exist): guides the search.
    pub min_step: i32,
}

/// What a unit's movement is counted in: this many to a movement point.
/// `UnitDef::moves`, terrain costs and perks are in whole points; units,
/// [`Mover::moves`], step costs and [`Path::left`] in these.
pub const MOVE: i32 = 10;

/// What a step between two railway tiles costs (in `MOVE`s).
pub const RAIL_COST: i32 = 2;

/// Movement points for stepping into a tunnel from off the road.
pub const TUNNEL_COST: i32 = 2;

/// Whole movement points as `MOVE`s.
pub fn points(n: i32) -> i32 {
    n * MOVE
}

/// Movement as shown to players: whole points, or with a decimal when a
/// fraction is left ("2", "1.4").
pub fn show_moves(m: i32) -> String {
    if m % MOVE == 0 { format!("{}", m / MOVE) } else { format!("{:.1}", m as f32 / MOVE as f32) }
}

/// A tile no one's borders close (see [`Mover::closed`]).
pub const OPEN: u8 = u8::MAX;

impl Mover {
    /// A unit of this type with no help from techs (land units stay ashore,
    /// ships keep off the open ocean and out of cities).
    pub fn new(unit: &UnitDef) -> Mover {
        Mover {
            moves: points(unit.moves),
            domain: unit.domain,
            ignore_terrain: unit.has(Ability::IgnoreTerrainCost),
            embark: false,
            deep_water: false,
            harbors: Vec::new(),
            closed: Vec::new(),
            season: 0.0,
            tunnels: Vec::new(),
            bridges: Vec::new(),
            min_step: MOVE,
        }
    }

    /// Whether the unit can be on `tile` at all.
    pub fn can_enter(&self, map: &Map, rules: &Rules, tile: TileId) -> bool {
        self.terrain_cost(map, rules, tile).is_some()
    }

    fn terrain_cost(&self, map: &Map, rules: &Rules, tile: TileId) -> Option<i32> {
        if !map.is_water(rules, tile) {
            return match self.domain {
                Domain::Land => match map.land_move_cost(rules, tile) {
                    Some(c) => Some(points(if self.ignore_terrain { 1 } else { c })),
                    None if self.mountain(map, rules, tile) && self.tunnels.binary_search(&tile).is_ok() => {
                        Some(points(if self.ignore_terrain { 1 } else { TUNNEL_COST }))
                    }
                    None => None,
                },
                Domain::Sea => self.harbors.contains(&tile).then_some(MOVE),
            };
        }
        if self.domain == Domain::Land && self.bridge(tile) {
            return Some(MOVE);
        }
        let t = map.tile(tile);
        if t.feature.is_some_and(|f| rules.feature(f).impassable) {
            return None;
        }
        let allowed = (self.domain == Domain::Sea || self.embark) && (self.deep_water || !rules.terrain(t.terrain).deep);
        if !allowed {
            return None;
        }
        match map.sea_ice(rules, tile, self.season) {
            SeaIce::Open => Some(MOVE),
            SeaIce::Thin => Some(points(rules.terrain.sea_ice.thin_move_cost)),
            SeaIce::Pack => None,
        }
    }

    fn bridge(&self, tile: TileId) -> bool {
        self.bridges.binary_search(&tile).is_ok()
    }

    /// Whether a land unit on `tile` would be at sea (water no bridge spans).
    fn at_sea(&self, map: &Map, rules: &Rules, tile: TileId) -> bool {
        map.is_water(rules, tile) && !self.bridge(tile)
    }

    /// A mountain only impassable for being one (a tunnel can go through it).
    fn mountain(&self, map: &Map, rules: &Rules, tile: TileId) -> bool {
        let t = map.tile(tile);
        t.elevation == crate::map::Elevation::Mountains && t.feature.is_none_or(|f| !rules.feature(f).impassable)
    }

    /// Movement points to step from `from` onto the neighbouring `to`, and
    /// whether the step ends the turn (embarking or landing); `None` if the
    /// unit can't go there.
    pub fn step(&self, map: &Map, rules: &Rules, from: TileId, to: TileId) -> Option<(i32, bool)> {
        let closed = |t: TileId| self.closed.get(t as usize).copied().unwrap_or(OPEN);
        if closed(to) != OPEN && closed(to) != closed(from) {
            return None;
        }
        let cost = self.terrain_cost(map, rules, to)?;
        let switch = self.domain == Domain::Land && self.at_sea(map, rules, from) != self.at_sea(map, rules, to);
        Some((cost, switch))
    }
}

/// Search state: turns elapsed and points left, ordered so fewer turns and then
/// more points remaining is better.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Cost {
    turns: u32,
    spent: i32,
}

/// Finds the best path from `from` to `to`.
///
/// `moves_left` is what the unit has this turn; `max_moves` what it gets each
/// turn. `blocked(tile)` excludes tiles (e.g. foreign units); the destination
/// itself must not be blocked.
pub fn find_path(
    map: &Map,
    rules: &Rules,
    unit: &Mover,
    from: TileId,
    to: TileId,
    moves_left: i32,
    blocked: impl Fn(TileId) -> bool,
) -> Option<Path> {
    find_path_zoc(map, rules, unit, from, to, moves_left, blocked, |_| false, |_, _| None, |_| false)
}

/// [`find_path`] with zones of control: entering a tile where `zoc(tile)` is
/// true uses up the rest of the turn's movement. Steps where `cheap(from,
/// to)` gives a cost, the step costs that if it's less (roads, railways,
/// perks). The unit may pass
/// through tiles where `no_stop(tile)` is true (its own units of the same
/// kind) but can't end a turn there.
pub fn find_path_zoc(
    map: &Map,
    rules: &Rules,
    unit: &Mover,
    from: TileId,
    to: TileId,
    moves_left: i32,
    blocked: impl Fn(TileId) -> bool,
    zoc: impl Fn(TileId) -> bool,
    cheap: impl Fn(TileId, TileId) -> Option<i32>,
    no_stop: impl Fn(TileId) -> bool,
) -> Option<Path> {
    if from == to || blocked(to) || !unit.can_enter(map, rules, to) {
        return None;
    }
    let max = unit.moves.max(1);
    let grid = &map.grid;
    // Start as if `max - moves_left` points were already spent this turn
    // (negative with bonus movement).
    let start = Cost { turns: 0, spent: max - moves_left.max(0) };
    let key = |c: Cost| c.turns as i64 * max as i64 + c.spent as i64;
    // No step costs less than `min_step`, so this never overestimates.
    let h = |t: TileId| grid.distance(t, to) as i64 * unit.min_step.clamp(1, max) as i64;

    let mut best: HashMap<TileId, Cost> = HashMap::new();
    let mut came_from: HashMap<TileId, TileId> = HashMap::new();
    let mut open = BinaryHeap::new();
    best.insert(from, start);
    open.push(Reverse((key(start) + h(from), key(start), from)));

    while let Some(Reverse((_, k, tile))) = open.pop() {
        if tile == to {
            break;
        }
        if best.get(&tile).is_some_and(|c| key(*c) < k) {
            continue;
        }
        let here = best[&tile];
        for nb in grid.neighbors(tile) {
            if nb != to && blocked(nb) {
                continue;
            }
            let Some((cost, ends)) = unit.step(map, rules, tile, nb) else { continue };
            let cost = cheap(tile, nb).map_or(cost, |c| c.min(cost));
            // Out of points: wait for the next turn first.
            let mut next = if here.spent >= max { Cost { turns: here.turns + 1, spent: 0 } } else { here };
            // May always enter with some points left; that ends the turn, as
            // do embarking, landing and stepping into an enemy's zone of control.
            next.spent = if ends || zoc(nb) { max } else { (next.spent + cost).min(max) };
            if next.spent >= max && nb != to && no_stop(nb) {
                continue;
            }
            if best.get(&nb).is_none_or(|c| next < *c) {
                best.insert(nb, next);
                came_from.insert(nb, tile);
                open.push(Reverse((key(next) + h(nb), key(next), nb)));
            }
        }
    }

    if !best.contains_key(&to) {
        return None;
    }
    let mut tiles = vec![to];
    let mut cur = to;
    while let Some(&prev) = came_from.get(&cur) {
        if prev == from {
            break;
        }
        tiles.push(prev);
        cur = prev;
    }
    tiles.reverse();
    let turns = tiles.iter().map(|t| best[t].turns).collect();
    let left = (max - best[&to].spent).max(0);
    Some(Path { tiles, turns, left })
}

/// Every tile reachable within `max_turns` extra turns, with its cost. Used by
/// automation (exploring) to weigh destinations.
pub fn reachable(
    map: &Map,
    rules: &Rules,
    unit: &Mover,
    from: TileId,
    moves_left: i32,
    max_turns: u32,
    blocked: impl Fn(TileId) -> bool,
) -> HashMap<TileId, (u32, i32)> {
    let max = unit.moves.max(1);
    let start = Cost { turns: 0, spent: max - moves_left.max(0) };
    let mut best: HashMap<TileId, Cost> = HashMap::new();
    let mut open = BinaryHeap::new();
    best.insert(from, start);
    open.push(Reverse((start, from)));
    while let Some(Reverse((here, tile))) = open.pop() {
        if best.get(&tile).is_some_and(|c| *c < here) {
            continue;
        }
        for nb in map.grid.neighbors(tile) {
            if blocked(nb) {
                continue;
            }
            let Some((cost, ends)) = unit.step(map, rules, tile, nb) else { continue };
            let mut next = if here.spent >= max { Cost { turns: here.turns + 1, spent: 0 } } else { here };
            next.spent = if ends { max } else { (next.spent + cost).min(max) };
            if next.turns > max_turns {
                continue;
            }
            if best.get(&nb).is_none_or(|c| next < *c) {
                best.insert(nb, next);
                open.push(Reverse((next, nb)));
            }
        }
    }
    best.into_iter().map(|(t, c)| (t, (c.turns, c.spent))).collect()
}

/// [`reachable`] with the rules of [`find_path_zoc`] (zones of control,
/// cheap steps, tiles the unit may pass but not stop on): for every tile
/// within `max_turns` extra turns, the turn the unit gets there, as the
/// best path there would say.
pub fn reachable_zoc(
    map: &Map,
    rules: &Rules,
    unit: &Mover,
    from: TileId,
    moves_left: i32,
    max_turns: u32,
    blocked: impl Fn(TileId) -> bool,
    zoc: impl Fn(TileId) -> bool,
    cheap: impl Fn(TileId, TileId) -> Option<i32>,
    no_stop: impl Fn(TileId) -> bool,
) -> HashMap<TileId, u32> {
    let max = unit.moves.max(1);
    let start = Cost { turns: 0, spent: max - moves_left.max(0) };
    let mut best: HashMap<TileId, Cost> = HashMap::new();
    let mut open = BinaryHeap::new();
    best.insert(from, start);
    open.push(Reverse((start, from)));
    while let Some(Reverse((here, tile))) = open.pop() {
        if best.get(&tile).is_some_and(|c| *c < here) {
            continue;
        }
        for nb in map.grid.neighbors(tile) {
            if blocked(nb) {
                continue;
            }
            let Some((cost, ends)) = unit.step(map, rules, tile, nb) else { continue };
            let cost = cheap(tile, nb).map_or(cost, |c| c.min(cost));
            let mut next = if here.spent >= max { Cost { turns: here.turns + 1, spent: 0 } } else { here };
            next.spent = if ends || zoc(nb) { max } else { (next.spent + cost).min(max) };
            if next.turns > max_turns || (next.spent >= max && no_stop(nb)) {
                continue;
            }
            if best.get(&nb).is_none_or(|c| next < *c) {
                best.insert(nb, next);
                open.push(Reverse((next, nb)));
            }
        }
    }
    best.remove(&from);
    best.into_iter().map(|(t, c)| (t, c.turns)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::{Hex, MapGrid};
    use crate::map::{Elevation, Tile};

    /// A small all-grassland test map with optional hills and water.
    fn test_map(rules: &Rules, hills: &[(i32, i32)], water: &[(i32, i32)]) -> Map {
        let grid = MapGrid::new(12, 10);
        let grass = rules.terrain_id("grassland").unwrap();
        let ocean = rules.terrain_id("ocean").unwrap();
        let mut tiles = vec![
            Tile {
                terrain: grass,
                elevation: Elevation::Flat,
                feature: None,
                rivers: 0,
                height: 0.5,
                temperature: 15.0,
                moisture: 0.5,
                resource: None
            };
            grid.len()
        ];
        let id = |c: i32, r: i32| grid.tile(Hex::from_offset(c, r)).unwrap() as usize;
        for &(c, r) in hills {
            tiles[id(c, r)].elevation = Elevation::Hills;
        }
        for &(c, r) in water {
            tiles[id(c, r)].terrain = ocean;
        }
        Map { grid, tiles, seed: 0, start_land: Vec::new(), kind: Default::default() }
    }

    fn at(map: &Map, c: i32, r: i32) -> TileId {
        map.grid.tile(Hex::from_offset(c, r)).unwrap()
    }

    #[test]
    fn straight_path_on_open_ground() {
        let rules = Rules::load_default().unwrap();
        let map = test_map(&rules, &[], &[]);
        let warrior = &Mover::new(rules.unit(rules.unit_type_id("warrior").unwrap()));
        let path = find_path(&map, &rules, warrior, at(&map, 2, 4), at(&map, 6, 4), points(2), |_| false).unwrap();
        assert_eq!(path.tiles.len(), 4);
        // Two tiles per turn: arrive in turn 1 (the second turn).
        assert_eq!(path.turns, vec![0, 0, 1, 1]);
    }

    #[test]
    fn hills_cost_more_and_are_avoided_when_cheaper() {
        let rules = Rules::load_default().unwrap();
        // A wall of hills with one gap.
        let wall: Vec<(i32, i32)> = (0..10).filter(|&r| r != 7).map(|r| (5, r)).collect();
        let map = test_map(&rules, &wall, &[]);
        let warrior = &Mover::new(rules.unit(rules.unit_type_id("warrior").unwrap()));
        let path = find_path(&map, &rules, warrior, at(&map, 3, 4), at(&map, 7, 4), points(2), |_| false).unwrap();
        // Crossing a hill (cost 2) is about as good as detouring; either way the
        // path must be valid and finish within 3 turns.
        assert!(path.arrival_turn() <= 2, "{path:?}");
        // A scout ignores terrain cost and walks straight through.
        let scout = &Mover::new(rules.unit(rules.unit_type_id("scout").unwrap()));
        let path = find_path(&map, &rules, scout, at(&map, 3, 4), at(&map, 7, 4), points(3), |_| false).unwrap();
        assert_eq!(path.tiles.len(), 4);
        assert_eq!(path.arrival_turn(), 1);
    }

    #[test]
    fn can_always_enter_with_points_left() {
        let rules = Rules::load_default().unwrap();
        let map = test_map(&rules, &[(4, 4)], &[]);
        let warrior = &Mover::new(rules.unit(rules.unit_type_id("warrior").unwrap()));
        // One point left, the hill costs 2: still enterable this turn.
        let path = find_path(&map, &rules, warrior, at(&map, 3, 4), at(&map, 4, 4), points(1), |_| false).unwrap();
        assert_eq!(path.turns, vec![0]);
    }

    #[test]
    fn turns_never_end_on_tiles_the_unit_cannot_stop_on() {
        let rules = Rules::load_default().unwrap();
        let map = test_map(&rules, &[(4, 4)], &[]);
        let warrior = &Mover::new(rules.unit(rules.unit_type_id("warrior").unwrap()));
        // Entering the hill uses up the turn, so with one of our units on it
        // the path goes around.
        let hill = at(&map, 4, 4);
        let path =
            find_path_zoc(&map, &rules, warrior, at(&map, 3, 4), at(&map, 6, 4), points(2), |_| false, |_| false, |_, _| None, |t| t == hill)
                .unwrap();
        assert!(!path.tiles.contains(&hill));
        // Flat tiles with our units can still be walked through.
        let flat = at(&map, 5, 5);
        let path = find_path_zoc(&map, &rules, warrior, at(&map, 4, 5), at(&map, 6, 5), points(2), |_| false, |_| false, |_, _| None, |t| t == flat)
            .unwrap();
        assert_eq!(path.tiles, vec![flat, at(&map, 6, 5)]);
    }

    #[test]
    fn water_and_blocked_tiles_are_avoided() {
        let rules = Rules::load_default().unwrap();
        let map = test_map(&rules, &[], &[(5, 3), (5, 4), (5, 5)]);
        let warrior = &Mover::new(rules.unit(rules.unit_type_id("warrior").unwrap()));
        let path = find_path(&map, &rules, warrior, at(&map, 3, 4), at(&map, 7, 4), points(2), |_| false).unwrap();
        assert!(path.tiles.iter().all(|&t| !map.is_water(&rules, t)));
        let blocked = at(&map, 4, 4);
        let path = find_path(&map, &rules, warrior, at(&map, 3, 4), at(&map, 7, 4), points(2), |t| t == blocked).unwrap();
        assert!(!path.tiles.contains(&blocked));
        assert!(find_path(&map, &rules, warrior, at(&map, 3, 4), at(&map, 5, 4), points(2), |_| false).is_none());
    }

    #[test]
    fn reachable_respects_turn_limit() {
        let rules = Rules::load_default().unwrap();
        let map = test_map(&rules, &[], &[]);
        let warrior = &Mover::new(rules.unit(rules.unit_type_id("warrior").unwrap()));
        let start = at(&map, 6, 5);
        let near = reachable(&map, &rules, warrior, start, points(2), 0, |_| false);
        // Within this turn: distance <= 2.
        assert!(near.keys().all(|&t| map.grid.distance(start, t) <= 2));
        assert_eq!(near.len(), 19);
    }

    /// Two straits of coast (with open ocean in the middle) splitting the
    /// map into two lands, one of them across the east-west wrap.
    fn strait(rules: &Rules) -> Map {
        let mut map = test_map(rules, &[], &[]);
        let coast = rules.terrain_id("coast").unwrap();
        let ocean = rules.terrain_id("ocean").unwrap();
        for r in 0..10 {
            for (c, t) in [(5, coast), (6, ocean), (7, coast), (11, coast), (0, ocean), (1, coast)] {
                let id = at(&map, c, r) as usize;
                map.tiles[id].terrain = t;
            }
        }
        map
    }

    #[test]
    fn embarking_needs_techs_and_ends_the_turn() {
        let rules = Rules::load_default().unwrap();
        let map = strait(&rules);
        let def = rules.unit(rules.unit_type_id("warrior").unwrap());
        let (from, to) = (at(&map, 3, 4), at(&map, 9, 4));
        assert!(find_path(&map, &rules, &Mover::new(def), from, to, points(2), |_| false).is_none(), "can't cross without boats");
        let coastal = Mover { embark: true, ..Mover::new(def) };
        assert!(find_path(&map, &rules, &coastal, from, to, points(2), |_| false).is_none(), "the open ocean needs more");
        let sailor = Mover { embark: true, deep_water: true, ..Mover::new(def) };
        let path = find_path(&map, &rules, &sailor, from, to, points(2), |_| false).unwrap();
        // Embarking and landing each end the turn.
        let shore = path.tiles.iter().position(|&t| map.is_water(&rules, t)).unwrap();
        assert_eq!(path.turns[shore], 0, "embarks in the first turn");
        assert_eq!(path.turns[shore + 1], 1, "{path:?}");
        let ashore = path.tiles.iter().rposition(|&t| map.is_water(&rules, t)).unwrap() + 1;
        assert_eq!(path.turns[ashore + 1], path.turns[ashore] + 1, "{path:?}");
    }

    #[test]
    fn ships_sail_on_water_and_into_their_harbors() {
        let rules = Rules::load_default().unwrap();
        let map = strait(&rules);
        let galley = Mover::new(rules.unit(rules.unit_type_id("galley").unwrap()));
        let (north, south) = (at(&map, 5, 0), at(&map, 5, 9));
        let path = find_path(&map, &rules, &galley, north, south, points(3), |_| false).unwrap();
        assert!(path.tiles.iter().all(|&t| map.is_water(&rules, t)));
        assert!(!galley.can_enter(&map, &rules, at(&map, 6, 4)), "no open ocean without Shipbuilding");
        let city = at(&map, 4, 4);
        assert!(find_path(&map, &rules, &galley, north, city, points(3), |_| false).is_none(), "land is off limits");
        let home = Mover { harbors: vec![city], ..galley };
        assert!(find_path(&map, &rules, &home, north, city, points(3), |_| false).is_some(), "but not our own city");
    }
}
