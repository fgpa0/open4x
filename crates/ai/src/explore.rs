//! Automatic exploration: each exploring unit heads for the reachable tile that
//! reveals the most unexplored ground for the time it takes to get there,
//! avoiding spots other explorers are already heading for.

use sim::game::{Event, Game, Order, PlayerId, UnitId};
use sim::hex::TileId;
use sim::map::Elevation;
use sim::path;

/// How many turns ahead an explorer looks for unexplored ground.
const NEAR_TURNS: u32 = 2;
const FAR_TURNS: u32 = 8;
/// Last resort, for the far corners of a continent.
const FARTHEST_TURNS: u32 = 25;

pub fn explore(game: &mut Game, player: PlayerId) -> Vec<Event> {
    let explorers: Vec<UnitId> = game
        .units
        .values()
        .filter(|u| u.owner == player && u.order == Order::Explore && u.moves > 0)
        .map(|u| u.id)
        .collect();
    let mut events = Vec::new();
    let mut claimed: Vec<TileId> = Vec::new();
    for unit in explorers {
        let destination = best_destination(game, unit, NEAR_TURNS, &claimed)
            .or_else(|| best_destination(game, unit, FAR_TURNS, &claimed))
            .or_else(|| best_destination(game, unit, FARTHEST_TURNS, &claimed));
        match destination {
            Some(dest) => {
                claimed.push(dest);
                if let Ok(e) = game.advance(unit, dest) {
                    events.extend(e);
                }
            }
            // Nothing left to explore within reach: hand control back.
            None => {
                if let Ok(e) = game.apply(player, sim::game::Command::SetOrder { unit, order: Order::None }) {
                    events.extend(e);
                }
            }
        }
    }
    events
}

/// Unexplored tiles a unit standing on `tile` would see.
fn reveal_count(game: &Game, player: PlayerId, tile: TileId, sight: i32) -> usize {
    let sight = sight + if game.map.tile(tile).elevation != Elevation::Flat { 1 } else { 0 };
    let explored = &game.players[player as usize].explored;
    game.map.grid.within(tile, sight).iter().filter(|&&t| !explored[t as usize]).count()
}

fn best_destination(game: &Game, unit: UnitId, max_turns: u32, claimed: &[TileId]) -> Option<TileId> {
    let u = &game.units[&unit];
    let def = game.rules.unit(u.kind);
    let owner = u.owner;
    let foreign: std::collections::HashSet<TileId> = game.units.values().filter(|o| o.owner != owner && !game.passes(u, o)).map(|o| o.tile).collect();
    let reach = path::reachable(&game.map, &game.rules, &game.mover(u), u.tile, u.moves, max_turns, |t| foreign.contains(&t));
    let mut best: Option<(f32, TileId)> = None;
    for (&tile, &(turns, spent)) in &reach {
        if tile == u.tile || !game.can_stop_at(u, tile) {
            continue;
        }
        let reveal = reveal_count(game, owner, tile, def.sight);
        if reveal == 0 {
            continue;
        }
        // Spread explorers out.
        if claimed.iter().any(|&c| game.map.grid.distance(c, tile) <= def.sight) {
            continue;
        }
        let time = 1.0 + turns as f32 + spent as f32 / sim::path::points(def.moves.max(1)) as f32;
        let score = reveal as f32 / time;
        if best.is_none_or(|(s, t)| score > s || (score == s && tile < t)) {
            best = Some((score, tile));
        }
    }
    best.map(|(_, t)| t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::game::Command;
    use sim::mapgen::{generate, MapSettings};
    use sim::rules::Rules;

    #[test]
    fn explorers_keep_revealing_the_map() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut game = Game::new(rules, map, 1);
        let scout = game.units.values().find(|u| u.owner == 0 && game.rules.unit(u.kind).id == "scout").unwrap().id;
        game.apply(0, Command::SetOrder { unit: scout, order: Order::Explore }).unwrap();
        let explored = |g: &Game| g.players[0].explored.iter().filter(|&&e| e).count();
        let start = explored(&game);
        for _ in 0..15 {
            crate::run_standing_orders(&mut game, 0);
            game.apply(0, Command::EndTurn).unwrap();
        }
        assert!(explored(&game) > start * 3, "explored {} -> {}", start, explored(&game));
    }

    #[test]
    fn exploration_is_deterministic() {
        let run = || {
            let rules = Rules::load_default().unwrap();
            let map = generate(&rules, &MapSettings::default());
            let mut game = Game::new(rules, map, 2);
            for _ in 0..8 {
                for p in 1..3 {
                    crate::take_turn(&mut game, p);
                }
                game.apply(0, Command::EndTurn).unwrap();
            }
            game.units.values().map(|u| u.tile).collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }
}
