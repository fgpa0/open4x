//! The AI's land forces, given the same group orders a player gives
//! (carried out by `orders.rs` for everyone):
//! - **Defend:** every city keeps a defender (its garrison), more where
//!   enemies are near it and in the cities facing the war target; at peace
//!   the rest stand by in the cities nearest our rivals.
//! - **Take:** at war, every soldier not needed at home is sent to take the
//!   target city. The group gathers out of its fire, then strikes.
//!
//! Generals go with the attack, or stay at the capital. Scouts keep exploring.

use std::collections::BTreeMap;

use sim::city::CityId;
use sim::game::{Command, Event, Game, Order, UnitId};
use sim::hex::TileId;
use sim::rules::{Domain, UnitClass};

use crate::strategy::{unit_power, Strategy};

/// Most defenders a threatened city calls for.
const MAX_DEFENDERS: usize = 4;
/// At war, this many cities nearest the enemy keep a second defender.
const FRONT_CITIES: usize = 2;
/// Soldiers further than this from a city don't come to defend it.
const DEFENCE_RANGE: i32 = 12;

pub fn run(game: &mut Game, s: &Strategy) -> Vec<Event> {
    let me = s.me;
    let mut events = Vec::new();
    let soldiers: Vec<UnitId> = game
        .units
        .values()
        .filter(|u| u.owner == me && is_soldier(game, u) && !is_general(game, u) && !is_scout(game, u.id))
        .map(|u| u.id)
        .collect();
    let want = defenders_wanted(game, s);
    // Who defends what now.
    let mut defending: BTreeMap<CityId, Vec<UnitId>> = BTreeMap::new();
    let mut free = Vec::new();
    for &u in &soldiers {
        match game.units[&u].order {
            Order::Defend { city } if want.contains_key(&city) => defending.entry(city).or_default().push(u),
            Order::Take { .. } => {}
            _ => free.push(u),
        }
    }
    // Too many defenders somewhere: the extra ones are free again (the
    // furthest from the city first).
    for (city, units) in defending.iter_mut() {
        let n = want.get(city).copied().unwrap_or(0);
        if units.len() > n {
            let tile = game.cities[city].tile;
            units.sort_by_key(|u| (game.map.grid.distance(game.units[u].tile, tile), *u));
            free.extend(units.drain(n..));
        }
    }
    // Missing defenders: the nearest free soldiers, else (under threat) the
    // nearest attackers.
    let mut cities: Vec<(CityId, usize)> = want.iter().map(|(&c, &n)| (c, n)).collect();
    cities.sort_by_key(|&(c, n)| (std::cmp::Reverse(s.threats.get(&c).map_or(0, |t| *t as i32)), std::cmp::Reverse(n), c));
    for (city, n) in cities {
        let tile = game.cities[&city].tile;
        let have = defending.get(&city).map_or(0, |v| v.len());
        for _ in have..n {
            let nearest = |pool: &[UnitId]| {
                pool.iter()
                    .copied()
                    .filter(|u| s.land[game.units[u].tile as usize] == s.land[tile as usize])
                    .map(|u| (game.map.grid.distance(game.units[&u].tile, tile), u))
                    .filter(|&(d, _)| d <= DEFENCE_RANGE)
                    .min()
                    .map(|(_, u)| u)
            };
            let pick = nearest(&free).or_else(|| {
                if !s.threats.contains_key(&city) {
                    return None;
                }
                let attackers: Vec<UnitId> = soldiers.iter().copied().filter(|u| matches!(game.units[u].order, Order::Take { .. })).collect();
                nearest(&attackers)
            });
            let Some(u) = pick else { break };
            free.retain(|&f| f != u);
            defending.entry(city).or_default().push(u);
            events.extend(order(game, s, u, Order::Defend { city }));
        }
    }
    // The rest: at war, take the target; at peace, stand by facing our rivals.
    let target = s.war.map(|w| w.target).filter(|&c| game.cities.contains_key(&c));
    for u in free {
        let at = game.units[&u].tile;
        let o = match target {
            Some(city) if s.land[at as usize] == s.land[game.cities[&city].tile as usize] => Order::Take { city },
            _ => match standby(game, s, at) {
                Some(city) => Order::Defend { city },
                None => continue,
            },
        };
        events.extend(order(game, s, u, o));
    }
    // Attackers still marching on an old target switch to the new one.
    if let Some(city) = target {
        let old: Vec<UnitId> =
            soldiers.iter().copied().filter(|u| game.units.get(u).is_some_and(|x| matches!(x.order, Order::Take { city: c } if c != city))).collect();
        for u in old {
            events.extend(order(game, s, u, Order::Take { city }));
        }
    }
    events.extend(place_generals(game, s));
    events
}

/// Gives `unit` an order (if it hasn't got it already).
fn order(game: &mut Game, s: &Strategy, unit: UnitId, order: Order) -> Vec<Event> {
    if game.units.get(&unit).is_none_or(|u| u.order == order) {
        return Vec::new();
    }
    game.apply(s.me, Command::SetOrder { unit, order }).unwrap_or_default()
}

/// How many defenders each of our cities wants: one, more for the cities
/// enemies are near (by their strength) and the ones nearest the war target.
fn defenders_wanted(game: &Game, s: &Strategy) -> BTreeMap<CityId, usize> {
    let mut want: BTreeMap<CityId, usize> = s.my_cities(game).map(|c| (c.id, 1)).collect();
    let typical = game
        .units
        .values()
        .filter(|u| u.owner == s.me && is_soldier(game, u) && !is_general(game, u))
        .map(|u| unit_power(game, u))
        .fold(10.0f32, f32::max);
    for (&city, &threat) in &s.threats {
        if let Some(n) = want.get_mut(&city) {
            *n = (1 + (threat / typical).ceil() as usize).min(MAX_DEFENDERS);
        }
    }
    if let Some(w) = s.war {
        let enemy: Vec<TileId> = game.cities.values().filter(|c| c.owner == w.enemy).map(|c| c.tile).collect();
        let mut front: Vec<(i32, CityId)> = s
            .my_cities(game)
            .map(|c| (enemy.iter().map(|&t| game.map.grid.distance(t, c.tile)).min().unwrap_or(i32::MAX), c.id))
            .collect();
        front.sort();
        for (_, city) in front.into_iter().take(FRONT_CITIES) {
            let n = want.get_mut(&city).expect("ours");
            *n = (*n).max(2);
        }
    }
    want
}

/// The city of ours a spare soldier at `at` stands by in: the one on its
/// landmass nearest a rival's city.
fn standby(game: &Game, s: &Strategy, at: TileId) -> Option<CityId> {
    let theirs: Vec<TileId> = game.cities.values().filter(|c| c.owner != s.me && s.knows_city(game, c)).map(|c| c.tile).collect();
    s.my_cities(game)
        .filter(|c| s.land[c.tile as usize] == s.land[at as usize])
        .map(|c| (theirs.iter().map(|&t| game.map.grid.distance(t, c.tile)).min().unwrap_or(0), game.map.grid.distance(c.tile, at), c.id))
        .min()
        .map(|(_, _, c)| c)
}

/// A military land unit (not a ship).
pub fn is_soldier(game: &Game, u: &sim::game::Unit) -> bool {
    let def = game.unit_def(u);
    def.class == UnitClass::Military && def.domain == Domain::Land
}

/// A General: it doesn't fight, it leads the units around it.
pub fn is_general(game: &Game, u: &sim::game::Unit) -> bool {
    game.unit_def(u).has(sim::rules::Ability::General)
}

/// Generals go with the attack on the war target, or defend the capital.
fn place_generals(game: &mut Game, s: &Strategy) -> Vec<Event> {
    let me = s.me;
    let mut events = Vec::new();
    let generals: Vec<UnitId> = game.units.values().filter(|u| u.owner == me && is_general(game, u) && is_soldier(game, u)).map(|u| u.id).collect();
    let attackers = game.units.values().filter(|u| u.owner == me && matches!(u.order, Order::Take { .. })).count();
    let target = s.war.map(|w| w.target).filter(|c| game.cities.contains_key(c) && attackers >= 3);
    let capital = s.my_cities(game).find(|c| c.capital_of == Some(me)).or_else(|| s.my_cities(game).next()).map(|c| c.id);
    for g in generals {
        let o = match (target, capital) {
            (Some(city), _) => Order::Take { city },
            (None, Some(city)) => Order::Defend { city },
            _ => continue,
        };
        events.extend(order(game, s, g, o));
    }
    events
}

pub fn is_scout(game: &Game, unit: UnitId) -> bool {
    is_recon(&game.rules, game.units[&unit].kind)
}

/// Whether units of `kind` explore rather than fight: Scouts and what they
/// upgrade to (or a civ's replacement for them).
pub fn is_recon(rules: &sim::rules::Rules, kind: sim::rules::UnitTypeId) -> bool {
    let base = rules.unit_replaces(kind).unwrap_or(kind);
    matches!(rules.unit(base).id.as_str(), "scout" | "explorer")
}
