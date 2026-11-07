//! Automated Workers ([`Order::Automate`]), for computer players and the
//! human's standing orders alike. Each Worker picks the job worth most for
//! the time it takes (getting there plus building): improvements by the
//! yields they add (more on tiles a city works, most for a luxury or
//! strategic resource its owner lacks), then roads between neighbouring
//! cities. It stays clear of enemies and of tiles another Worker has taken.

use std::collections::HashSet;

use sim::city::MAX_CITY_RADIUS;
use sim::game::{Command, Event, Game, Order, PlayerId, UnitId};
use sim::hex::TileId;
use sim::improvement::Work;
use sim::rules::{Ability, ImprovementId, ResourceKind, UnitClass, Yields};

/// Workers look for jobs this many turns away at most.
const MAX_TURNS: u32 = 4;
/// Cities whose road to a neighbour is looked at each turn.
const ROADS_PER_TURN: usize = 2;
/// What a road tile between two cities is worth (in yield points).
const ROAD_VALUE: f32 = 1.2;
/// Workers keep this far from enemy soldiers.
const SAFE_DISTANCE: i32 = 2;

fn worth(y: Yields) -> f32 {
    y.food as f32 * 1.6 + y.production as f32 * 1.6 + y.gold as f32
}

/// A job: where, what, and what it's worth.
#[derive(Clone, Copy, Debug)]
struct Job {
    tile: TileId,
    work: Work,
    value: f32,
}

/// Every job worth doing in `player`'s land, and roads between its cities
/// when `workers` have too little else to do, best first, by value per turn
/// of work.
fn jobs(game: &Game, player: PlayerId, workers: usize) -> Vec<Job> {
    let mut out = Vec::new();
    let worked: HashSet<TileId> = game.cities.values().filter(|c| c.owner == player).flat_map(|c| c.worked.iter().copied()).collect();
    let counts = game.resource_counts(player);
    let n = game.rules.improvements.improvements.len() as ImprovementId;
    for c in game.cities.values().filter(|c| c.owner == player) {
        for t in game.map.grid.within(c.tile, MAX_CITY_RADIUS) {
            if game.district(t) != Some(c.id) || t == c.tile {
                continue;
            }
            let now = worth(game.tile_yields(player, t));
            let lacking = game.visible_resource(player, t).filter(|&r| {
                game.rules.resource(r).kind != ResourceKind::Bonus && counts[r as usize] <= 0 && !game.resource_connected(t)
            });
            for imp in 0..n {
                let work = Work::Improvement(imp);
                if game.tile_work_problem(player, t, work).is_some() {
                    continue;
                }
                let mut gain = worth(game.yields_with_work(player, t, work)) - now;
                if worked.contains(&t) {
                    gain *= 1.5;
                }
                if lacking.is_some_and(|r| game.rules.resource_improvement(r) == Some(imp)) {
                    gain += 6.0;
                }
                if gain > 0.5 {
                    out.push(Job { tile: t, work, value: gain / game.work_turns(player, t, work) as f32 });
                }
            }
        }
    }
    let road_value = ROAD_VALUE / game.rules.improvements.road.turns as f32;
    if out.iter().filter(|j| j.value >= road_value).count() < workers * 2 {
        // Roads first; railways over the roads once the owner knows how.
        for t in road_tiles(game, player) {
            if let Some(work) = [Work::Road, Work::Railroad].into_iter().find(|&w| game.tile_work_problem(player, t, w).is_none()) {
                out.push(Job { tile: t, work, value: road_value });
            }
        }
    }
    out.sort_by(|a, b| b.value.total_cmp(&a.value).then(a.tile.cmp(&b.tile)));
    out
}

/// Tiles on the land route from a city to its nearest neighbour that still
/// lack a road (or, once `player` can lay them, a railway). A couple
/// of cities are looked at each turn, in turn, to keep path searches few.
fn road_tiles(game: &Game, player: PlayerId) -> Vec<TileId> {
    let Some(worker) = game.rules.unit_type_id("worker") else { return Vec::new() };
    let mover = sim::path::Mover { min_step: game.min_step(), ..sim::path::Mover::new(game.rules.unit(worker)) };
    let cities: Vec<TileId> = game.cities.values().filter(|c| c.owner == player).map(|c| c.tile).collect();
    let foreign = |t: TileId| game.territory_owner(t).is_some_and(|o| o != player);
    let rails = game.knows(player, game.rules.rail_tech());
    let mut out = Vec::new();
    let rounds = cities.len().div_ceil(ROADS_PER_TURN).max(1);
    let now = game.turn as usize % rounds;
    for (_, &a) in cities.iter().enumerate().filter(|(i, _)| i % rounds == now) {
        let near = cities.iter().copied().filter(|&b| b != a).min_by_key(|&b| (game.map.grid.distance(a, b), b));
        let Some(b) = near.filter(|&b| game.map.grid.distance(a, b) <= 8) else { continue };
        let road = |x: TileId, y: TileId| match (game.has_rail(x) && game.has_rail(y), game.has_road(x) && game.has_road(y)) {
            (true, _) => Some(sim::path::RAIL_COST),
            (_, true) => Some(sim::path::MOVE),
            _ => None,
        };
        let path = sim::path::find_path_zoc(&game.map, &game.rules, &mover, a, b, mover.moves, foreign, |_| false, road, |_| false);
        if let Some(p) = path.filter(|p| p.tiles.len() <= 10) {
            out.extend(p.tiles.into_iter().filter(|&t| !game.has_road(t) || (rails && !game.has_rail(t))));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Tiles near enemy soldiers.
fn unsafe_tiles(game: &Game, player: PlayerId) -> HashSet<TileId> {
    game.units
        .values()
        .filter(|u| game.at_war(player, u.owner) && game.unit_def(u).class == UnitClass::Military)
        .flat_map(|u| game.map.grid.within(u.tile, SAFE_DISTANCE))
        .collect()
}

/// Moves and sets to work every automated Worker of `player`'s.
pub fn run(game: &mut Game, player: PlayerId) -> Vec<Event> {
    let workers: Vec<UnitId> = game
        .units
        .values()
        .filter(|u| u.owner == player && u.order == Order::Automate && u.moves > 0)
        .map(|u| u.id)
        .collect();
    let mut events = Vec::new();
    if workers.is_empty() {
        return events;
    }
    let danger = unsafe_tiles(game, player);
    let mut taken: HashSet<TileId> = game
        .units
        .values()
        .filter(|u| u.owner == player && matches!(u.order, Order::Build(_) | Order::AutoBuild(_)))
        .map(|u| u.tile)
        .collect();
    let all = jobs(game, player, workers.len());
    // Merchants at peace are no obstacle (see `Game::passes`).
    let foreign: HashSet<TileId> =
        game.units.values().filter(|o| o.owner != player && !(game.is_merchant(o) && !game.at_war(player, o.owner))).map(|o| o.tile).collect();
    for unit in workers {
        let u = &game.units[&unit];
        let here = u.tile;
        // Turns to every tile within reach, in one search.
        let reach = sim::path::reachable(&game.map, &game.rules, &game.mover(u), here, u.moves, MAX_TURNS, |t| foreign.contains(&t));
        // Value per turn, counting the walk there.
        let best = all
            .iter()
            .filter(|j| !taken.contains(&j.tile) && !danger.contains(&j.tile))
            .filter_map(|j| {
                let walk = if j.tile == here { 0.0 } else { reach.get(&j.tile)?.0 as f32 + 1.0 };
                let turns = game.work_turns(player, j.tile, j.work) as f32;
                Some((j.value * turns / (turns + walk), *j))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.tile.cmp(&a.1.tile)));
        let Some((_, job)) = best else {
            events.extend(go_home(game, unit));
            continue;
        };
        taken.insert(job.tile);
        if job.tile != here {
            match game.advance(unit, job.tile) {
                Ok(e) => events.extend(e),
                Err(_) => continue,
            }
        }
        if game.units[&unit].tile == job.tile && game.units[&unit].moves > 0 {
            events.extend(game.apply(player, Command::Build { unit, work: job.work }).unwrap_or_default());
        }
    }
    events
}

/// Nothing to do: wait in the nearest city.
fn go_home(game: &mut Game, unit: UnitId) -> Vec<Event> {
    let u = &game.units[&unit];
    if game.city_at(u.tile).is_some() {
        return Vec::new();
    }
    let home = game.cities.values().filter(|c| c.owner == u.owner).min_by_key(|c| (game.map.grid.distance(c.tile, u.tile), c.id)).map(|c| c.tile);
    home.and_then(|h| game.advance(unit, h).ok()).unwrap_or_default()
}

/// How many Workers `player` wants: about one for every two cities, more for
/// builders.
pub fn workers_wanted(game: &Game, player: PlayerId, builder: u8) -> usize {
    let cities = game.cities.values().filter(|c| c.owner == player).count();
    (cities * (3 + builder as usize / 2)).div_ceil(10)
}

/// Workers `player` has.
pub fn workers(game: &Game, player: PlayerId) -> usize {
    game.units.values().filter(|u| u.owner == player && game.unit_def(u).has(Ability::BuildImprovements)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::mapgen::{generate, MapSettings};
    use sim::rules::Rules;

    #[test]
    fn automated_workers_improve_the_land() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 1);
        g.players[0].techs.fill(true);
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let home = g.cities.values().next().unwrap().tile;
        let worker = g.spawn_unit(0, g.rules.unit_type_id("worker").unwrap(), home);
        g.apply(0, Command::SetOrder { unit: worker, order: Order::Automate }).unwrap();
        for _ in 0..30 {
            crate::run_standing_orders(&mut g, 0);
            g.apply(0, Command::EndTurn).unwrap();
        }
        let improved = g.works.iter().filter(|w| w.improvement.is_some()).count();
        assert!(improved >= 2, "{improved} improvements after 30 turns");
        assert!(matches!(g.units[&worker].order, Order::Automate | Order::AutoBuild(_)), "still automated");
    }
}
