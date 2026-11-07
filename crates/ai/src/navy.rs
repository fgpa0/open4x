//! Ships, given the same group orders as land units (carried out by
//! `orders.rs`): at war with a coastal target on their sea, they're sent to
//! take it (ranged ships batter the walls, melee ships storm once they're
//! down); otherwise each guards our port nearest it on its sea, striking
//! enemy ships that come close. Admirals go with the ships attacking, or
//! stay in our biggest port.

use std::collections::VecDeque;

use sim::city::CityId;
use sim::game::{Command, Event, Game, Order, UnitId};
use sim::hex::TileId;
use sim::rules::Domain;

use crate::military::is_general;
use crate::strategy::Strategy;

/// No sea (land, or a tile with no water next to it).
const NO_SEA: u16 = u16::MAX;
/// The most ships the AI wants.
const MOST_SHIPS: usize = 14;

/// How many ships we want. It grows with how much the sea matters to us
/// ([`sea_stake`]: our coastal cities, rivals across the water): at peace a
/// small guard once we're established, more for a seafaring empire; at war
/// more again, most of all against a coastal city overseas, up to what two
/// [`MOST_SHIPS`].
pub fn ships_wanted(game: &Game, s: &Strategy) -> usize {
    let ports = s.my_cities(game).filter(|c| game.is_coastal(c.tile)).count();
    if ports == 0 {
        return 0;
    }
    let shore = shores(game);
    let stake = sea_stake(game, s, &shore);
    let peace = if game.turn >= PEACE_FLEET_FROM { (1.0 + ports as f32 * 0.5 * stake).round() as usize } else { 0 };
    let most = MOST_SHIPS;
    let wanted = match s.war {
        Some(w) if game.is_coastal(game.cities[&w.target].tile) => {
            let overseas = !own_shores(game, s, &shore).contains(&shore[game.cities[&w.target].tile as usize]);
            peace + 2 + ports / 2 + if overseas { 2 } else { 0 }
        }
        Some(_) => peace + 1,
        None => peace,
    };
    wanted.min(most)
}

/// From this turn on we keep ships at peace.
const PEACE_FLEET_FROM: u32 = 30;

/// How much the sea matters to us, 0 to 1: half the share of our cities on
/// the coast, half the share of the rival cities we know that stand on land
/// we have no city on (reachable only by sea).
pub fn sea_stake(game: &Game, s: &Strategy, shore: &[u32]) -> f32 {
    let mine: Vec<_> = s.my_cities(game).collect();
    if mine.is_empty() {
        return 0.0;
    }
    let coastal = mine.iter().filter(|c| game.is_coastal(c.tile)).count() as f32 / mine.len() as f32;
    let ours = own_shores(game, s, shore);
    let rivals: Vec<TileId> = game.cities.values().filter(|c| c.owner != s.me && s.knows_city(game, c)).map(|c| c.tile).collect();
    let overseas = if rivals.is_empty() { 0.0 } else { rivals.iter().filter(|&&t| !ours.contains(&shore[t as usize])).count() as f32 / rivals.len() as f32 };
    0.5 * coastal + 0.5 * overseas
}

/// Landmasses by walking (no boats): each land tile's lowest connected tile id.
pub fn shores(game: &Game) -> Vec<u32> {
    let n = game.map.tiles.len();
    let land = |t: TileId| !game.map.is_water(&game.rules, t);
    let mut id = vec![u32::MAX; n];
    for start in 0..n as TileId {
        if id[start as usize] != u32::MAX || !land(start) {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        id[start as usize] = start;
        while let Some(t) = queue.pop_front() {
            for nb in game.map.grid.neighbors(t) {
                if id[nb as usize] == u32::MAX && land(nb) {
                    id[nb as usize] = start;
                    queue.push_back(nb);
                }
            }
        }
    }
    id
}

/// The landmasses our cities stand on.
pub fn own_shores(game: &Game, s: &Strategy, shore: &[u32]) -> Vec<u32> {
    let mut v: Vec<u32> = s.my_cities(game).map(|c| shore[c.tile as usize]).collect();
    v.sort();
    v.dedup();
    v
}

/// A player's fighting strength at sea.
pub fn naval_power(game: &Game, player: sim::game::PlayerId) -> f32 {
    game.units.values().filter(|u| u.owner == player && game.unit_def(u).domain == Domain::Sea).map(|u| crate::strategy::unit_power(game, u)).sum()
}

/// Bodies of water: each water tile's sea (connected water), and for land
/// tiles (ports) the biggest sea next to them.
pub struct Seas {
    id: Vec<u16>,
    size: Vec<usize>,
}

impl Seas {
    pub fn of(game: &Game) -> Seas {
        let n = game.map.tiles.len();
        let water = |t: TileId| game.map.is_water(&game.rules, t);
        let mut id = vec![NO_SEA; n];
        let mut size = Vec::new();
        for start in 0..n as TileId {
            if id[start as usize] != NO_SEA || !water(start) {
                continue;
            }
            let sea = size.len() as u16;
            let mut count = 0;
            let mut queue = VecDeque::from([start]);
            id[start as usize] = sea;
            while let Some(t) = queue.pop_front() {
                count += 1;
                for nb in game.map.grid.neighbors(t) {
                    if id[nb as usize] == NO_SEA && water(nb) {
                        id[nb as usize] = sea;
                        queue.push_back(nb);
                    }
                }
            }
            size.push(count);
        }
        Seas { id, size }
    }

    /// The sea a tile is on (a port: the biggest one beside it).
    pub fn at(&self, game: &Game, tile: TileId) -> u16 {
        let own = self.id[tile as usize];
        if own != NO_SEA && game.map.is_water(&game.rules, tile) {
            return own;
        }
        game.map.grid.neighbors(tile).map(|t| self.id[t as usize]).filter(|&s| s != NO_SEA).max_by_key(|&s| (self.size[s as usize], std::cmp::Reverse(s))).unwrap_or(NO_SEA)
    }
}

/// Gives every ship and Admiral of `s.me` its order for the turn.
pub fn run(game: &mut Game, s: &Strategy) -> Vec<Event> {
    let me = s.me;
    let mut events = Vec::new();
    let seas = Seas::of(game);
    let target = s.war.map(|w| w.target).filter(|c| game.cities.get(c).is_some_and(|c| game.is_coastal(c.tile)));
    let target_sea = target.map(|c| seas.at(game, game.cities[&c].tile));
    let ships: Vec<UnitId> = game.units.values().filter(|u| u.owner == me && game.unit_def(u).domain == Domain::Sea && !is_general(game, u)).map(|u| u.id).collect();
    let mut attacking = 0;
    for &ship in &ships {
        let at = game.units[&ship].tile;
        let sea = seas.at(game, at);
        let order = match target {
            Some(city) if target_sea == Some(sea) && sea != NO_SEA => {
                attacking += 1;
                Order::Take { city }
            }
            _ => match port(game, s, &seas, sea, at) {
                Some(city) => Order::Defend { city },
                None => continue,
            },
        };
        events.extend(set(game, me, ship, order));
    }
    let admirals: Vec<UnitId> = game.units.values().filter(|u| u.owner == me && game.unit_def(u).domain == Domain::Sea && is_general(game, u)).map(|u| u.id).collect();
    for a in admirals {
        let at = game.units[&a].tile;
        let order = match target {
            Some(city) if attacking >= 2 && target_sea == Some(seas.at(game, at)) => Order::Take { city },
            _ => match port(game, s, &seas, seas.at(game, at), at) {
                Some(city) => Order::Defend { city },
                None => continue,
            },
        };
        events.extend(set(game, me, a, order));
    }
    events
}

fn set(game: &mut Game, me: sim::game::PlayerId, unit: UnitId, order: Order) -> Vec<Event> {
    if game.units.get(&unit).is_none_or(|u| u.order == order) {
        return Vec::new();
    }
    game.apply(me, Command::SetOrder { unit, order }).unwrap_or_default()
}

/// Our port on `sea` nearest `from` (the most threatened one first).
fn port(game: &Game, s: &Strategy, seas: &Seas, sea: u16, from: TileId) -> Option<CityId> {
    s.my_cities(game)
        .filter(|c| game.is_coastal(c.tile) && seas.at(game, c.tile) == sea)
        .min_by_key(|c| (!s.threats.contains_key(&c.id), game.map.grid.distance(c.tile, from), c.id))
        .map(|c| c.id)
}
