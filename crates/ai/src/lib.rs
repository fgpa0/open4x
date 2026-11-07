//! Automation shared by AI players and the human player's standing orders.
//! It only ever acts through the same commands and methods a player could use.

pub mod diplomacy;
pub mod explore;
pub mod fight;
pub mod group;
pub mod military;
pub mod navy;
pub mod orders;
pub mod production;
pub mod promote;
pub mod research;
pub mod settle;
pub mod spend;
pub mod strategy;
pub mod trade;
pub mod work;

use sim::game::{Command, Event, Game, Order, PlayerId, UnitId};
use sim::rules::{Ability, UnitClass};

/// How often (in turns) idle scouts look for unexplored ground again.
const SCOUT_RETRY: u32 = 8;

/// Carries out automated standing orders (exploring, automated Workers,
/// taking and defending cities) for `player`'s units. Called at the start of
/// each turn and when such an order is given.
pub fn run_standing_orders(game: &mut Game, player: PlayerId) -> Vec<Event> {
    let mut events = explore::explore(game, player);
    events.extend(work::run(game, player));
    events.extend(orders::run(game, player, orders::HUMAN_BOLDNESS));
    events
}

/// A computer player's turn. It sizes up the world (see [`strategy`]),
/// picks research and production, spends gold, takes good fights,
/// defends, makes war when its leader wants to (over the sea too, once its
/// units can embark) and peace when it doesn't, sends out its ships and
/// settles new cities and sends merchants out to trade. Scouts explore.
pub fn take_turn(game: &mut Game, player: PlayerId) -> Vec<Event> {
    if game.players[player as usize].eliminated {
        return Vec::new();
    }
    let mut s = strategy::Strategy::assess(game, player);
    let mut events = Vec::new();
    // Deals first: peace changes whom we fight.
    if diplomacy::run(game, &s, &mut events) {
        s = strategy::Strategy::assess(game, player);
    }
    // Going to war means declaring it first.
    if let Some(w) = s.war.filter(|w| !game.at_war(player, w.enemy)) {
        if let Ok(e) = game.apply(player, Command::DeclareWar { on: w.enemy }) {
            events.extend(e);
            s = strategy::Strategy::assess(game, player);
        }
    }
    events.extend(research::choose(game, &s));
    events.extend(promote::run(game, &s));
    events.extend(production::plan(game, &s));
    events.extend(spend::spend(game, &s));
    events.extend(fight::attack_targets(game, &s));
    events.extend(military::run(game, &s));
    events.extend(navy::run(game, &s));
    events.extend(orders::run(game, player, s.drives.risk as f32 / 10.0));
    events.extend(fight::attack_targets(game, &s));
    events.extend(settle::settle(game, player));
    events.extend(trade::run(game, &s));
    // Workers improve the land on their own. Scouts explore; once there's
    // nothing left within reach they sleep and look again every few turns.
    let retry = game.turn.is_multiple_of(SCOUT_RETRY);
    let idle: Vec<_> = game
        .units
        .values()
        .filter(|u| u.owner == player && u.moves > 0 && (u.order == Order::None || (retry && u.order == Order::Sleep)))
        .map(|u| {
            let def = game.unit_def(u);
            let keep = def.has(Ability::FoundCity) || def.has(Ability::Trade);
            (u.id, u.order, def.class, keep, def.has(Ability::BuildImprovements), military::is_scout(game, u.id))
        })
        .collect();
    for (unit, was, class, keep, worker, scout) in idle {
        let order = match (class, scout) {
            // Settlers and merchants have their own orders.
            _ if keep => continue,
            _ if worker => Order::Automate,
            (UnitClass::Civilian, _) => Order::Fortify,
            (_, true) if was == Order::None && !retry => Order::Sleep,
            (_, true) => Order::Explore,
            _ => Order::Fortify,
        };
        if let Ok(e) = game.apply(player, Command::SetOrder { unit, order }) {
            events.extend(e);
        }
    }
    events.extend(explore::explore(game, player));
    events.extend(work::run(game, player));
    // Military units get fresh orders every turn: unfinished moves would
    // otherwise carry on at the start of the next turn and use up the
    // movement the AI wants to plan with.
    let moving: Vec<UnitId> = game
        .units
        .values()
        .filter(|u| u.owner == player && matches!(u.order, Order::Goto(_)) && game.unit_def(u).class == UnitClass::Military)
        .map(|u| u.id)
        .collect();
    for unit in moving {
        events.extend(game.apply(player, Command::SetOrder { unit, order: Order::None }).unwrap_or_default());
    }
    events
}

#[cfg(test)]
mod tests {
    use sim::game::{Command, Game};
    use sim::mapgen::{generate, MapSettings};
    use sim::rules::Rules;

    #[test]
    fn computer_players_found_and_expand() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut game = Game::new(rules, map, 3);
        for _ in 0..80 {
            for p in 0..4 {
                crate::take_turn(&mut game, p);
            }
            game.apply(0, Command::EndTurn).unwrap();
        }
        for p in 1..4 {
            let cities = game.cities.values().filter(|c| c.owner == p).count();
            assert!(cities >= 2, "player {p} has {cities} cities after 80 turns");
        }
    }
}
