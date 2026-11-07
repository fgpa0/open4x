//! Computer players' merchants: each one without a route takes the one that
//! pays best per turn (`Game::best_route`), leaving out civs it's about to
//! fight. One with nowhere to go sleeps and looks again every few turns.

use sim::game::{Command, Event, Game, Order, UnitId};

use crate::strategy::Strategy;

/// How often (in turns) merchants with no route look for one again.
const RETRY: u32 = 5;

/// Sends every merchant of `s.me`'s without a route on the best one.
pub fn run(game: &mut Game, s: &Strategy) -> Vec<Event> {
    let me = s.me;
    let retry = game.turn.is_multiple_of(RETRY);
    let idle: Vec<UnitId> = game
        .merchants(me)
        .filter(|u| !matches!(u.order, Order::Trade(_)) && (u.order != Order::Sleep || retry))
        .map(|u| u.id)
        .collect();
    let mut events = Vec::new();
    for unit in idle {
        let enemy = s.war.map(|w| w.enemy);
        let best = game.best_route(unit, |p| Some(p) != enemy);
        let cmd = match best {
            Some(offer) => Command::Trade { unit, to: offer.to },
            None => Command::SetOrder { unit, order: Order::Sleep },
        };
        if let Ok(e) = game.apply(me, cmd) {
            events.extend(e);
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use sim::game::{Command, Game, Order};
    use sim::mapgen::{generate, MapSettings};
    use sim::rules::Rules;

    /// Computer players build merchants once they can and keep them trading.
    #[test]
    fn computer_players_trade() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut game = Game::new(rules, map, 3);
        let mut sales = 0;
        for _ in 0..140 {
            for p in 1..4 {
                crate::take_turn(&mut game, p);
            }
            let events = game.apply(0, Command::EndTurn).unwrap();
            sales += events.iter().filter(|e| matches!(e, sim::game::Event::Traded(_))).count();
        }
        let merchants: Vec<_> = game.units.values().filter(|u| u.owner != 0 && game.is_merchant(u)).collect();
        assert!(!merchants.is_empty(), "some merchants by turn 140");
        assert!(merchants.iter().filter(|u| matches!(u.order, Order::Trade(_))).count() * 2 >= merchants.len(), "most of them trading");
        assert!(sales > 0);
    }
}
