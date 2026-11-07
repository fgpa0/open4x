//! Settling: computer players found their first city where they start, and
//! send later settlers to the best reachable city site nearby.

use sim::city::MAX_CITY_RADIUS;
use sim::game::{Command, Event, Game, Order, PlayerId, UnitId};
use sim::hex::TileId;
use sim::path;
use sim::rules::{Ability, ResourceId};

/// How far (in turns of travel) a settler looks for a site.
const SEARCH_TURNS: u32 = 6;

/// How good a city site is: food and production around it, fresh water or
/// coast, minus tiles already claimed by someone.
pub fn site_value(game: &Game, player: PlayerId, tile: TileId) -> Option<i32> {
    site_report(game, player, tile, |_| true).map(|r| r.value)
}

/// A city site and what makes it good (for settling hints).
#[derive(Clone, Debug, PartialEq)]
pub struct SiteReport {
    pub value: i32,
    /// Food, production and gold of the unclaimed tiles in reach.
    pub food: i32,
    pub production: i32,
    pub gold: i32,
    pub river: bool,
    /// Next to a lake (fresh water) or the sea.
    pub lake: bool,
    pub coast: bool,
    /// Resources `player` can see in reach.
    pub resources: Vec<ResourceId>,
}

/// [`site_value`] with its reasons. Only tiles `known` counts are weighed
/// (the player's hints don't see through the fog; the AI passes everything).
pub fn site_report(game: &Game, player: PlayerId, tile: TileId, known: impl Fn(TileId) -> bool) -> Option<SiteReport> {
    if game.found_city_problem(player, tile).is_some() {
        return None;
    }
    let map = &game.map;
    let rules = &game.rules;
    let mut r = SiteReport { value: 0, food: 0, production: 0, gold: 0, river: false, lake: false, coast: false, resources: Vec::new() };
    for t in map.grid.within(tile, 2) {
        if !known(t) || game.territory_owner(t).is_some() {
            continue;
        }
        let y = map.yields(rules, t);
        r.value += y.food * 2 + y.production * 2 + y.gold;
        (r.food, r.production, r.gold) = (r.food + y.food, r.production + y.production, r.gold + y.gold);
        if let Some(res) = game.visible_resource(player, t) {
            if !r.resources.contains(&res) {
                r.resources.push(res);
            }
        }
    }
    r.river = map.has_river(tile);
    let water: Vec<TileId> = map.grid.neighbors(tile).filter(|&n| map.is_water(rules, n)).collect();
    let lake = rules.terrain_id("lake");
    r.lake = water.iter().any(|&n| Some(map.tile(n).terrain) == lake);
    r.coast = water.iter().any(|&n| Some(map.tile(n).terrain) != lake);
    if r.river || !water.is_empty() {
        r.value += 8;
    }
    Some(r)
}

pub fn settle(game: &mut Game, player: PlayerId) -> Vec<Event> {
    let settlers: Vec<UnitId> = game
        .units
        .values()
        .filter(|u| u.owner == player && u.moves > 0 && game.unit_def(u).has(Ability::FoundCity))
        .filter(|u| matches!(u.order, Order::None | Order::Sleep | Order::Goto(_)))
        .map(|u| u.id)
        .collect();
    let mut events = Vec::new();
    let has_city = game.cities.values().any(|c| c.owner == player);
    for unit in settlers {
        let u = &game.units[&unit];
        let here = u.tile;
        // First city: found immediately if possible (no wandering at the start).
        if !has_city && site_value(game, player, here).is_some() {
            if let Ok(e) = game.apply(player, Command::FoundCity { unit }) {
                events.extend(e);
                continue;
            }
        }
        let target = best_site(game, player, unit);
        match target {
            Some(t) if t == here => {
                if let Ok(e) = game.apply(player, Command::FoundCity { unit }) {
                    events.extend(e);
                }
            }
            Some(t) => {
                if let Ok(e) = game.apply(player, Command::MoveUnit { unit, to: t }) {
                    events.extend(e);
                }
            }
            None => {}
        }
    }
    events
}

fn best_site(game: &Game, player: PlayerId, unit: UnitId) -> Option<TileId> {
    let u = &game.units[&unit];
    let blocked = |t: TileId| game.units_at(t).any(|o| o.owner != player && !game.passes(u, o));
    let reach = path::reachable(&game.map, &game.rules, &game.mover(u), u.tile, u.moves, SEARCH_TURNS, blocked);
    // Prefer staying near our own cities (but not crowding them).
    let ours: Vec<TileId> = game.cities.values().filter(|c| c.owner == player).map(|c| c.tile).collect();
    reach
        .iter()
        .chain(std::iter::once((&u.tile, &(0, 0))))
        .filter_map(|(&t, &(turns, _))| {
            let value = site_value(game, player, t)?;
            let near = ours.iter().map(|&c| game.map.grid.distance(c, t)).min().unwrap_or(0);
            let spread = if near > MAX_CITY_RADIUS * 3 { (near - MAX_CITY_RADIUS * 3) * 3 } else { 0 };
            Some((value - turns as i32 * 4 - spread, t))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
        .map(|(_, t)| t)
}
