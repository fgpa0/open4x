//! Group commands: several units given one order with a right-click, the
//! way a player commands an army. On an enemy city, every soldier is sent
//! to take it (`Order::Take`, carried out in `orders.rs`); on one of our
//! cities, to defend it (`Order::Defend`); on an enemy unit, they all hunt
//! it. Elsewhere, the first unit goes to the tile and the others to the free
//! tiles nearest to it. Civilians in the group just go there.

use sim::game::{Command, Event, Game, Order, PlayerId, Slot, UnitId};
use sim::hex::TileId;
use sim::rules::UnitClass;

/// What a right-click on `to` would give the group: the order for its
/// soldiers, if it's a city order.
pub fn city_order(game: &Game, player: PlayerId, to: TileId) -> Option<Order> {
    let c = game.city_at(to)?;
    if c.owner == player {
        Some(Order::Defend { city: c.id })
    } else if game.players[player as usize].explored[to as usize] {
        Some(Order::Take { city: c.id })
    } else {
        None
    }
}

pub fn command(game: &mut Game, player: PlayerId, members: &[UnitId], to: TileId) -> Result<Vec<Event>, String> {
    let mut events = Vec::new();
    let soldier = |game: &Game, u: UnitId| game.units.get(&u).is_some_and(|u| game.unit_def(u).class == UnitClass::Military);
    if let Some(order) = city_order(game, player, to) {
        let mut given = 0;
        let mut why = None;
        let soldiers: Vec<UnitId> = members.iter().copied().filter(|&u| soldier(game, u)).collect();
        for unit in soldiers {
            match game.apply(player, Command::SetOrder { unit, order }) {
                Ok(e) => {
                    events.extend(e);
                    given += 1;
                }
                Err(e) => why = Some(e.to_string()),
            }
        }
        if given > 0 {
            // They set off now (see `orders.rs`).
            events.extend(crate::orders::run(game, player, crate::orders::HUMAN_BOLDNESS));
            return Ok(events);
        }
        if let Some(why) = why {
            return Err(why);
        }
    }
    let enemy_there = game.players[player as usize].sees(to) && game.units_at(to).any(|u| u.owner != player && !(game.is_merchant(u) && !game.at_war(player, u.owner)));
    if enemy_there {
        let target = game.hunt_target(player, to);
        let mut hunting = false;
        for &unit in members {
            if let Some(Ok(e)) = target.map(|target| game.apply(player, Command::Hunt { unit, target })) {
                events.extend(e);
                hunting = true;
            }
        }
        return if hunting { Ok(events) } else { Err("Can't attack: no unit in the group can reach it".into()) };
    }
    // Destinations: the clicked tile, then rings around it.
    let mut free: Vec<TileId> = game.map.grid.within(to, 2);
    free.sort_by_key(|&t| game.map.grid.distance(t, to));
    let mut claimed: Vec<(TileId, Slot)> = Vec::new();
    let mut moved = false;
    for &unit in members {
        let Some(u) = game.units.get(&unit) else { continue };
        let slot = game.slot(u);
        // One unit of each kind (soldier, civilian, ship, leader) may share a tile.
        let pick = free.iter().copied().find(|&t| !claimed.contains(&(t, slot)) && game.path_for(unit, t).is_some());
        if let Some(t) = pick {
            if let Ok(e) = game.apply(player, Command::MoveUnit { unit, to: t }) {
                events.extend(e);
                claimed.push((t, slot));
                moved = true;
            }
        }
    }
    if moved {
        Ok(events)
    } else {
        Err("Can't move there".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::game::Order;
    use sim::mapgen::{generate, MapSettings};
    use sim::rules::Rules;

    #[test]
    fn groups_spread_out_around_the_destination() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 1);
        let mine: Vec<UnitId> = g.units.values().filter(|u| u.owner == 0).map(|u| u.id).collect();
        let warrior = *mine.iter().find(|&&u| g.unit_def(&g.units[&u]).id == "warrior").unwrap();
        let scout = *mine.iter().find(|&&u| g.unit_def(&g.units[&u]).id == "scout").unwrap();
        let start = g.units[&warrior].tile;
        let to = g
            .map
            .grid
            .within(start, 4)
            .into_iter()
            .find(|&t| g.map.grid.distance(t, start) == 4 && g.path_for(warrior, t).is_some() && g.path_for(scout, t).is_some())
            .unwrap();
        command(&mut g, 0, &[warrior, scout], to).unwrap();
        let goal = |u: UnitId| match g.units[&u].order {
            Order::Goto(t) => t,
            _ => g.units[&u].tile,
        };
        assert_eq!(goal(warrior), to);
        assert_ne!(goal(scout), to, "two military units can't share the tile");
        assert!(g.map.grid.distance(goal(scout), to) <= 2);
    }
}
