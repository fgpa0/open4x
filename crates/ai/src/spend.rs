//! Spending gold. A computer player keeps a reserve (for deals and
//! emergencies; wealth-loving leaders keep more) and spends the rest:
//! - **Emergencies first**, reserve or not: a threatened city buys the
//!   unit it's building.
//! - **Upgrades**: old units that gain the most strength for the gold, a few
//!   a turn (keeping half the reserve at war).
//! - **At war** (or short of the army it wants), it buys the units its
//!   cities are building. The Sleeping Giant, once attacked, turns its
//!   whole treasury into an army.
//! - **Otherwise** it finishes the build that would take longest, and buys
//!   land for cities that have outgrown theirs.
//!
//! At most one purchase of each kind per turn, so gold isn't dumped all at once.

use sim::city::{Build, CityId};
use sim::civ::AgendaKind;
use sim::game::{Command, Event, Game, PlayerId, UnitId};

use crate::strategy::Strategy;

/// Gold kept back: this much, plus this much per city.
const RESERVE: i32 = 60;
const RESERVE_PER_CITY: i32 = 25;
/// Rushing a build needs this many turns to be saved.
const WORTH_RUSHING: i32 = 3;

pub fn spend(game: &mut Game, s: &Strategy) -> Vec<Event> {
    let me = s.me;
    let mut events = Vec::new();
    let cities: Vec<CityId> = s.my_cities(game).map(|c| c.id).collect();
    let wealth = s.drives.wealth as i32;
    let reserve = (RESERVE + RESERVE_PER_CITY * cities.len() as i32) * (10 + wealth) / 10;
    let gold = |game: &Game| game.players[me as usize].gold;
    let buy = |game: &mut Game, cmd: Command, events: &mut Vec<Event>| match game.apply(me, cmd) {
        Ok(e) => {
            events.extend(e);
            true
        }
        Err(_) => false,
    };

    // The units our cities are building that gold could finish now, cheapest first.
    let units_under_way = |game: &Game, only: Option<CityId>| {
        let mut v: Vec<(i32, CityId, Build)> = cities
            .iter()
            .copied()
            .filter(|&c| only.is_none_or(|o| o == c))
            .filter_map(|c| {
                let b = *game.cities[&c].queue.first()?;
                let military = matches!(b, Build::Unit(u) if game.rules.unit(u).class == sim::rules::UnitClass::Military);
                (military && game.buy_problem(c, b).is_none()).then(|| (game.buy_price(c, b), c, b))
            })
            .collect();
        v.sort_by_key(|&(price, c, _)| (price, c));
        v
    };
    // Under threat: the unit a threatened city is building, whatever the reserve says.
    for &city in s.threats.keys() {
        if let Some(&(price, c, b)) = units_under_way(game, Some(city)).first() {
            if gold(game) >= price {
                buy(game, Command::Buy { city: c, build: b }, &mut events);
            }
        }
    }

    let at_war = (0..game.players.len() as PlayerId).any(|q| game.at_war(me, q));
    upgrade(game, me, if at_war { reserve / 2 } else { reserve }, &mut events);

    // The army: pay for the recruits waiting for the war chest.
    let giant = s.agenda == AgendaKind::SleepingGiant && (0..game.players.len() as PlayerId).any(|q| q != me && s.grudge(game, q) > 0.0);
    if (at_war || giant) && game.military_units(me) < s.want_military {
        let keep = if giant { 0 } else { reserve / 2 };
        let rounds = if giant { cities.len() } else { 1 };
        for _ in 0..rounds {
            match units_under_way(game, None).first().copied() {
                Some((price, city, b)) if gold(game) - price >= keep => {
                    buy(game, Command::Buy { city, build: b }, &mut events);
                }
                _ => break,
            }
        }
        return events;
    }

    // Peace: rush the build that would take longest.
    let rush = cities
        .iter()
        .filter_map(|&c| {
            let b = *game.cities[&c].queue.first()?;
            let turns = game.turns_to_build(c)?;
            (turns >= WORTH_RUSHING && game.buy_problem(c, b).is_none()).then_some((turns, c, b))
        })
        .max_by_key(|&(turns, c, _)| (turns, std::cmp::Reverse(c)));
    if let Some((_, city, b)) = rush {
        if gold(game) - game.buy_price(city, b) >= reserve {
            buy(game, Command::Buy { city, build: b }, &mut events);
        }
    }

    // Land for a city with more citizens than tiles to work, the best tile
    // for its price.
    let tile = cities
        .iter()
        .filter(|&&c| {
            let owned = game.district_tiles(c).len() as i32;
            game.cities[&c].population + 1 >= owned
        })
        .flat_map(|&c| game.buyable_tiles(c).into_iter().map(move |t| (c, t)))
        .map(|(c, t)| {
            let y = game.tile_yields(me, t);
            let value = (y.food * 3 + y.production * 2 + y.gold) as f32 / game.tile_price(c, t) as f32;
            (value, c, t)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0).then(b.2.cmp(&a.2)));
    if let Some((_, city, tile)) = tile {
        if gold(game) - game.tile_price(city, tile) >= reserve {
            buy(game, Command::BuyTile { city, tile }, &mut events);
        }
    }
    events
}

/// Units upgraded per turn at most.
const UPGRADES_PER_TURN: usize = 3;

/// Upgrades the units that gain the most strength for their price, keeping `keep` gold.
fn upgrade(game: &mut Game, me: PlayerId, keep: i32, events: &mut Vec<Event>) {
    let power = |game: &Game, kind| {
        let d = game.rules.unit(kind);
        d.strength.max(d.ranged)
    };
    let mut wanted: Vec<(f32, UnitId)> = game
        .units
        .values()
        .filter(|u| u.owner == me)
        .filter_map(|u| {
            let to = game.upgrade_target(me, u.kind)?;
            let price = game.upgrade_price(u.id)?;
            Some(((power(game, to) - power(game, u.kind)) as f32 / price as f32, u.id))
        })
        .collect();
    wanted.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut done = 0;
    for (_, unit) in wanted {
        if done == UPGRADES_PER_TURN {
            break;
        }
        let Some(price) = game.upgrade_price(unit) else { continue };
        if game.players[me as usize].gold - price < keep || game.upgrade_problem(unit).is_some() {
            continue;
        }
        if let Ok(e) = game.apply(me, Command::Upgrade { unit }) {
            events.extend(e);
            done += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::city::Build;
    use sim::game::GameSetup;
    use sim::mapgen::{generate, MapSettings};
    use sim::rules::{Ability, Rules};

    fn ai_with_city(civ: &str, gold: i32) -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let civs = vec![rules.civ_id("egypt").unwrap(), rules.civ_id(civ).unwrap()];
        let mut g = Game::with_setup(rules, map, &GameSetup { civs, options: Default::default(), starts: Vec::new() });
        let settler = g.units.values().find(|u| u.owner == 1 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(1, Command::FoundCity { unit: settler }).unwrap();
        g.players[1].gold = gold;
        g.players[1].techs.fill(true);
        g
    }

    #[test]
    fn a_rich_treasury_finishes_slow_builds() {
        let mut g = ai_with_city("rome", 2000);
        let city = *g.cities.keys().next().unwrap();
        let library = Build::Building(g.rules.civ_building(1, "library").unwrap());
        g.apply(1, Command::SetQueue { city, queue: vec![library] }).unwrap();
        let s = Strategy::assess(&g, 1);
        spend(&mut g, &s);
        assert!(g.cities[&city].queue.is_empty(), "bought");
        assert!(g.players[1].gold < 2000);
    }

    #[test]
    fn a_poor_one_keeps_its_reserve() {
        let mut g = ai_with_city("rome", 80);
        let city = *g.cities.keys().next().unwrap();
        let library = Build::Building(g.rules.civ_building(1, "library").unwrap());
        g.apply(1, Command::SetQueue { city, queue: vec![library] }).unwrap();
        let s = Strategy::assess(&g, 1);
        spend(&mut g, &s);
        assert_eq!(g.players[1].gold, 80);
    }
}
