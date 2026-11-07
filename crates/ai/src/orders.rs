//! Group orders, carried out the same way for the human player and the
//! computer players: [`Order::Take`] and [`Order::Defend`] (Goto and Hunt
//! are the sim's own). Nothing is stored between turns: each turn the units
//! with the same order make up the group, and what they do follows from
//! where they stand.
//!
//! **Take city.** The group gathers on a tile just out of the city's fire
//! (the *muster point*, on the side the group comes from). Once most of it
//! is there (or the fight has begun), it strikes together: ranged and siege
//! units close to their range and batter the walls, melee units wait just
//! out of reach until the walls are low, then storm; a melee unit next to a
//! city at 0 takes it. Badly wounded units go back to our nearest city to
//! heal and return. The order ends (and the units ask for orders again)
//! when the city is ours or someone else's, or at peace.
//!
//! **Defend city.** One of the group stands in the city as its garrison
//! (unless another of our soldiers already does); the others stand guard
//! next to it, dug in, and strike enemies that come within a few tiles when
//! the fight is good. The order ends if the city is lost.

use std::collections::BTreeMap;

use sim::city::CityId;
use sim::combat::{CombatPreview, Target};
use sim::game::{Command, Event, Game, Order, PlayerId, Slot, UnitId};
use sim::hex::TileId;
use sim::rules::{Domain, UnitClass};

use crate::fight;

/// Below this health a unit leaves to heal...
const HEAL_BELOW: i32 = 40;
/// ...and it stays in the city until this.
const HEALED: i32 = 90;
/// Members within this many tiles beyond the city's fire count as
/// gathered (all round the city, not only by the muster point: units that
/// meet its defenders on the way spread out).
const GATHERED: i32 = 3;
/// The share (percent) of the group near the city that must have gathered
/// before it strikes; units further than [`STRAGGLERS`] from the city (on
/// their way from home) don't hold the others back...
const GATHER_SHARE: usize = 70;
const STRAGGLERS: i32 = 12;
/// ...and this many gathered strike whatever the rest are doing.
const ENOUGH: usize = 6;
/// The first volley at the walls waits for this many ranged units.
const VOLLEY: usize = 3;
/// Melee units storm once the walls are down to this (or the group has no
/// ranged units to bring them down).
const STORM_AT: i32 = 35;
/// Defenders strike enemies this close to their city.
const DEFEND_RADIUS: i32 = 3;
/// How bold the human's units are (computer players use their leader's).
pub const HUMAN_BOLDNESS: f32 = 0.5;

/// Carries out `player`'s Take and Defend orders for this turn.
pub fn run(game: &mut Game, player: PlayerId, boldness: f32) -> Vec<Event> {
    let mut events = Vec::new();
    let mut take: BTreeMap<CityId, Vec<UnitId>> = BTreeMap::new();
    let mut defend: BTreeMap<CityId, Vec<UnitId>> = BTreeMap::new();
    for u in game.units.values().filter(|u| u.owner == player) {
        match u.order {
            Order::Take { city } => take.entry(city).or_default().push(u.id),
            Order::Defend { city } => defend.entry(city).or_default().push(u.id),
            _ => {}
        }
    }
    for (city, members) in defend {
        run_defend(game, player, city, &members, boldness, &mut events);
    }
    for (city, members) in take {
        run_take(game, player, city, &members, boldness, &mut events);
    }
    events
}

/// Ends the order of every member that still exists.
fn done(game: &mut Game, player: PlayerId, members: &[UnitId], events: &mut Vec<Event>) {
    for &unit in members {
        if game.units.contains_key(&unit) {
            events.extend(game.apply(player, Command::SetOrder { unit, order: Order::None }).unwrap_or_default());
        }
    }
}

fn alive(game: &Game, members: &[UnitId]) -> Vec<UnitId> {
    members.iter().copied().filter(|m| game.units.contains_key(m)).collect()
}

/// Moves `unit` towards the free tile nearest `to` within `radius` of it
/// that it can reach (the tile itself first), keeping its order. Returns
/// whether it found a way.
pub fn go_near(game: &mut Game, unit: UnitId, to: TileId, radius: i32, events: &mut Vec<Event>) -> bool {
    let Some(u) = game.units.get(&unit) else { return false };
    if u.tile == to {
        return true;
    }
    let from = u.tile;
    let grid = &game.map.grid;
    let mut near: Vec<TileId> = grid.within(to, radius).into_iter().filter(|&t| game.can_stop_at(u, t)).collect();
    near.sort_by_key(|&t| (grid.distance(t, to), grid.distance(t, from), t));
    // Standing within reach already is as good as any tile further off.
    if near.first().is_some_and(|&t| grid.distance(t, to) >= grid.distance(from, to)) && grid.distance(from, to) <= radius {
        return true;
    }
    let Some(&nearest) = near.first() else { return false };
    // One search: moving there finds the way, or finds there's none.
    match game.advance(unit, nearest) {
        Ok(e) => {
            events.extend(e);
            return true;
        }
        Err(sim::game::CommandError::Unreachable) => {}
        // No moves left: whether it could get there at all.
        Err(_) if game.path_for(unit, nearest).is_some() => return true,
        Err(_) => {}
    }
    // Out of reach: the nearest of the others it can get to soon, found in
    // one bounded search (a failed search scans everything the unit can
    // reach, too slow to repeat for each tile, every turn, for each unit
    // that's cut off).
    let reach = game.reach_for(unit, GO_NEAR_TURNS);
    let Some(t) = near.into_iter().skip(1).find(|t| reach.contains_key(t)) else { return false };
    match game.advance(unit, t) {
        Ok(e) => {
            events.extend(e);
            true
        }
        Err(sim::game::CommandError::Unreachable) => false,
        // No moves left: it can get there later.
        Err(_) => true,
    }
}

/// How many turns ahead [`go_near`] looks for another tile when the nearest
/// is out of reach.
const GO_NEAR_TURNS: u32 = 3;

/// Our city nearest `tile` that a unit like `unit` can rest in (coastal
/// for ships).
fn nearest_home(game: &Game, unit: UnitId) -> Option<TileId> {
    let u = game.units.get(&unit)?;
    let sea = game.unit_def(u).domain == Domain::Sea;
    game.cities
        .values()
        .filter(|c| c.owner == u.owner && (!sea || game.is_coastal(c.tile)))
        .map(|c| (game.map.grid.distance(c.tile, u.tile), c.tile))
        .min()
        .map(|(_, t)| t)
}

/// A wounded unit goes home and stays until healed; returns whether it's
/// busy healing.
fn heal(game: &mut Game, unit: UnitId, events: &mut Vec<Event>) -> bool {
    let Some(u) = game.units.get(&unit) else { return true };
    let in_city = game.city_at(u.tile).is_some_and(|c| c.owner == u.owner);
    if u.hp >= HEAL_BELOW && !(in_city && u.hp < HEALED) {
        return false;
    }
    if in_city {
        return true;
    }
    match nearest_home(game, unit) {
        Some(home) => go_near(game, unit, home, 1, events),
        None => false,
    }
}

/// The member the others are nearest to all together: the middle of the group.
fn middle(game: &Game, members: &[UnitId]) -> Option<TileId> {
    let tiles: Vec<TileId> = members.iter().filter_map(|m| game.units.get(m)).map(|u| u.tile).collect();
    tiles.iter().copied().min_by_key(|&t| (tiles.iter().map(|&o| game.map.grid.distance(t, o)).sum::<i32>(), t))
}

/// The land around `tile` within `radius` that can be walked to from it
/// (no boats): where units can stand to fight a city without embarking.
fn ashore(game: &Game, tile: TileId, radius: i32) -> std::collections::HashSet<TileId> {
    let grid = &game.map.grid;
    let walk = |t: TileId| game.map.land_move_cost(&game.rules, t).is_some() || game.has_bridge(t) || game.has_tunnel(t);
    let mut seen = std::collections::HashSet::from([tile]);
    let mut stack = vec![tile];
    while let Some(t) = stack.pop() {
        for n in grid.neighbors(t) {
            if grid.distance(n, tile) <= radius && walk(n) && seen.insert(n) {
                stack.push(n);
            }
        }
    }
    seen
}

/// Where a group coming from `from` gathers to take `city`: the land tile
/// just out of its fire nearest `from`, on the city's side of any water.
pub fn muster_point(game: &Game, player: PlayerId, city: CityId, from: TileId) -> Option<TileId> {
    let c = game.cities.get(&city)?;
    let out_of_fire = game.city_range(city) + 1;
    let grid = &game.map.grid;
    let land = ashore(game, c.tile, out_of_fire + 2);
    grid.within(c.tile, out_of_fire)
        .into_iter()
        .filter(|&t| grid.distance(t, c.tile) == out_of_fire)
        .filter(|t| land.contains(t) && game.city_at(*t).is_none())
        .filter(|&t| game.territory_owner(t).is_none_or(|o| o == player || o == c.owner || game.may_enter(player, o)))
        .min_by_key(|&t| (grid.distance(t, from), t))
}

fn run_take(game: &mut Game, player: PlayerId, city: CityId, members: &[UnitId], boldness: f32, events: &mut Vec<Event>) {
    let Some(c) = game.cities.get(&city) else { return done(game, player, members, events) };
    let (owner, city_tile) = (c.owner, c.tile);
    if owner == player || !game.at_war(player, owner) {
        return done(game, player, members, events);
    }
    let members = alive(game, members);
    let is = |game: &Game, m: UnitId, slot: Slot| game.units.get(&m).is_some_and(|u| game.slot(u) == slot);
    let land: Vec<UnitId> = members.iter().copied().filter(|&m| is(game, m, Slot::Land)).collect();
    let ships: Vec<UnitId> = members.iter().copied().filter(|&m| is(game, m, Slot::Sea)).collect();
    let leaders: Vec<UnitId> = members.iter().copied().filter(|&m| is(game, m, Slot::Leader)).collect();

    // The wounded go home first; the rest are the group.
    let mut active = Vec::new();
    for &m in &land {
        if !heal(game, m, events) {
            active.push(m);
        }
    }
    let active = alive(game, &active);
    let Some(centre) = middle(game, &active).or_else(|| middle(game, &land)) else {
        run_ships(game, player, city, &ships, boldness, events);
        follow(game, &leaders, members.first().and_then(|m| game.units.get(m)).map(|u| u.tile), events);
        return;
    };
    let muster = muster_point(game, player, city, centre);
    let grid = &game.map.grid;
    let near: Vec<UnitId> = active.iter().copied().filter(|u| grid.distance(game.units[u].tile, city_tile) <= STRAGGLERS).collect();
    let fire = game.city_range(city);
    let gathered = near.iter().filter(|&&u| grid.distance(game.units[&u].tile, city_tile) <= fire + GATHERED).count();
    // Once some of the group stand in the city's fire (or the walls are
    // hit), the fight has begun: nobody falls back to gather again.
    let begun = active.iter().any(|&u| grid.distance(game.units[&u].tile, city_tile) <= fire) || game.cities[&city].hp < sim::combat::CITY_MAX_HP;
    let ready = gathered >= ENOUGH || (!near.is_empty() && gathered * 100 >= near.len() * GATHER_SHARE && near.len() * 2 >= active.len());
    let strike = active.len() <= 1 || begun || ready || muster.is_none();
    let war = war_table(game, player);

    if !strike {
        let muster = muster.expect("checked");
        for &m in &active {
            // On the way, units fight what crosses their path, but not the city.
            fight::attack_where(game, m, boldness, &|o| war[o as usize], &not_city, events);
            if game.units.contains_key(&m) && !go_near(game, m, muster, 2, events) {
                // No way there: this one can't take part.
                done(game, player, &[m], events);
            }
        }
        follow(game, &leaders, Some(muster), events);
        run_ships(game, player, city, &ships, boldness, events);
        return;
    }

    // The strike. Ranged and siege units take firing positions first, as
    // far back as their range allows; together they kill enemy soldiers
    // next to them when they can (only a kill: shots that merely wound
    // them are better spent on the walls), then batter the walls; melee
    // units stand in front of them, next to the city, and storm once the
    // walls are low.
    let ranged: Vec<UnitId> = active.iter().copied().filter(|m| game.unit_def(&game.units[m]).ranged > 0).collect();
    let melee: Vec<UnitId> = active.iter().copied().filter(|m| !ranged.contains(m)).collect();
    // The first volley waits until a few ranged units stand a step out of
    // reach (or all there are); after that, each steps in when it can shoot.
    let poised = |m: &UnitId| {
        let u = &game.units[m];
        u.moves > 0 && grid.distance(u.tile, city_tile) <= game.unit_def(u).range.max(1) + 1
    };
    let staged = ranged.iter().filter(|m| poised(m)).count();
    let coming = ranged.iter().filter(|m| grid.distance(game.units[m].tile, city_tile) <= STRAGGLERS).count();
    let volley = game.cities[&city].hp < sim::combat::CITY_MAX_HP || staged >= VOLLEY.min(coming.max(1));
    for &m in &ranged {
        firing_position(game, player, m, city, volley, events);
    }
    focus_fire(game, player, owner, &ranged, events);
    for &m in &ranged {
        shoot(game, player, m, city, boldness, events);
    }
    let storm = ranged.is_empty() || game.cities.get(&city).is_none_or(|c| c.hp <= STORM_AT);
    let covering: Vec<TileId> = alive(game, &ranged).iter().map(|m| game.units[m].tile).collect();
    for &m in &melee {
        if game.cities.get(&city).is_none_or(|c| c.owner == player) {
            break;
        }
        strike_melee(game, player, m, city, storm, &covering, boldness, events);
    }
    let rally = middle(game, &alive(game, &active)).or(muster);
    follow(game, &leaders, rally, events);
    run_ships(game, player, city, &ships, boldness, events);
    // Taken: the order is done.
    if game.cities.get(&city).is_none_or(|c| c.owner == player) {
        done(game, player, &members, events);
    }
}

/// Whom `player` is at war with, by player.
fn war_table(game: &Game, player: PlayerId) -> Vec<bool> {
    (0..game.players.len() as PlayerId).map(|o| game.at_war(player, o)).collect()
}

/// Attacks on units, not on the city.
fn not_city(_: TileId, p: &CombatPreview) -> bool {
    !matches!(p.target, Target::City(_))
}

/// Moves a ranged unit to where it shoots at the city's walls from (or
/// at the units guarding it, once the walls are down or there's no room by
/// them): see [`best_post`]. It steps in only when it can shoot the same
/// turn, and only once the `volley` begins; until then it waits a step
/// out of reach, so the ranged units don't come under the city's fire one
/// by one.
fn firing_position(game: &mut Game, player: PlayerId, unit: UnitId, city: CityId, volley: bool, events: &mut Vec<Event>) {
    let Some(c) = game.cities.get(&city) else { return };
    let (tile, owner, walls) = (c.tile, c.owner, c.hp);
    let Some(u) = game.units.get(&unit) else { return };
    if u.moves <= 0 {
        return;
    }
    let here = u.tile;
    let def = game.unit_def(u);
    let reach = def.range.max(1);
    let shoots_after_moving = !def.has(sim::rules::Ability::NoAttackAfterMove);
    let threats = soldiers_near(game, player, owner, tile, reach + 4);
    let guards: Vec<TileId> = threats.iter().copied().filter(|&t| game.map.grid.distance(t, tile) <= 2).collect();
    let fire = (tile, game.city_range(city));
    let walls_target = [tile];
    let mut targets: &[TileId] = if walls > 0 { &walls_target } else { &guards };
    let near = Near::new(unit);
    let mut dest = best_post(game, &near, targets, reach, 1, &threats, Some(fire));
    if dest.is_none() && walls > 0 {
        targets = &guards;
        dest = best_post(game, &near, targets, reach, 1, &threats, Some(fire));
    }
    let Some(dest) = dest.filter(|&d| d != here) else { return };
    // Already a step away, it goes in even if it can't shoot until next
    // turn (rough ground takes all its moves): waiting wouldn't help.
    let poised = targets.iter().any(|&t| game.map.grid.distance(here, t) <= reach + 1);
    let shoots = volley && game.path_for(unit, dest).is_some_and(|p| p.arrival_turn() == 0 && (poised || (p.left > 0 && shoots_after_moving)));
    let dest = if shoots {
        Some(dest)
    } else {
        // A step out of reach, ready.
        best_post(game, &near, targets, reach + 1, reach + 1, &threats, Some(fire))
    };
    if let Some(dest) = dest.filter(|&d| d != here) {
        if let Ok(e) = game.advance(unit, dest) {
            events.extend(e);
        }
    }
}

/// Focus fire: the enemy soldiers next to our ranged units (`shooters`)
/// that those in reach can kill this turn, the weakest first, each shot at
/// until it falls.
fn focus_fire(game: &mut Game, player: PlayerId, owner: PlayerId, shooters: &[UnitId], events: &mut Vec<Event>) {
    let ours: Vec<TileId> = alive(game, shooters).iter().map(|m| game.units[m].tile).collect();
    let mut threats: Vec<(i32, TileId)> = ours
        .iter()
        .flat_map(|&t| soldiers_near(game, player, owner, t, 1))
        .filter_map(|t| game.units_at(t).filter(|u| u.owner == owner).map(|u| u.hp).min().map(|hp| (hp, t)))
        .collect();
    threats.sort();
    threats.dedup();
    for (hp, t) in threats {
        let able = |game: &Game, s: UnitId| game.units.contains_key(&s) && matches!(game.attack_target(s, t), Ok(Target::Unit(_)));
        let damage: i32 = shooters.iter().filter(|&&s| able(game, s)).filter_map(|&s| game.combat_preview(s, t).ok()).map(|p| p.damage_dealt).sum();
        if damage < hp {
            continue;
        }
        for &s in shooters {
            if !game.units_at(t).any(|u| u.owner == owner) {
                break;
            }
            if able(game, s) {
                if let Ok(e) = game.apply(player, Command::Attack { unit: s, tile: t }) {
                    events.extend(e);
                }
            }
        }
    }
}

/// A ranged unit's shot, from where it stands: the walls while they're up,
/// else the best fight against the city's owner.
fn shoot(game: &mut Game, player: PlayerId, unit: UnitId, city: CityId, boldness: f32, events: &mut Vec<Event>) {
    let Some(c) = game.cities.get(&city) else { return };
    let (tile, owner, walls) = (c.tile, c.owner, c.hp);
    if !game.units.contains_key(&unit) {
        return;
    }
    if walls > 0 && game.attack_target(unit, tile).is_ok() {
        if let Ok(e) = game.apply(player, Command::Attack { unit, tile }) {
            events.extend(e);
        }
        return;
    }
    fight::attack_where(game, unit, boldness, &move |o| o == owner, &not_city, events);
}

/// A ranged unit's part, all in one (for ships): take position and shoot.
fn strike_ranged(game: &mut Game, player: PlayerId, unit: UnitId, city: CityId, boldness: f32, events: &mut Vec<Event>) {
    firing_position(game, player, unit, city, true, events);
    shoot(game, player, unit, city, boldness, events);
}

/// The soldiers of `owner` that `player` can see within `radius` of
/// `tile` (not in a city: those are its garrison).
fn soldiers_near(game: &Game, player: PlayerId, owner: PlayerId, tile: TileId, radius: i32) -> Vec<TileId> {
    let sees = &game.players[player as usize];
    game.map
        .grid
        .within(tile, radius)
        .into_iter()
        .filter(|&t| sees.sees(t) && game.city_at(t).is_none())
        .filter(|&t| game.units_at(t).any(|u| u.owner == owner && game.unit_def(u).class == UnitClass::Military))
        .collect()
}

/// How a tile `unit` could stand on suits it (lower is better): its
/// terrain's defence, and for a ranged unit (`reach` > 1) shooting at one
/// of `targets`, standing as far back as its range allows, with no enemy
/// soldier (`threats`) next to it and out of the city's fire (`fire`:
/// the city's tile and range) where it can be.
fn post_cost(game: &Game, unit: &sim::game::Unit, t: TileId, targets: &[TileId], reach: i32, threats: &[TileId], fire: Option<(TileId, i32)>) -> i32 {
    let grid = &game.map.grid;
    let sea = game.unit_def(unit).domain == Domain::Sea;
    let mut cost = if sea { 0 } else { -game.map.defense_bonus(&game.rules, t) / 2 };
    if reach > 1 {
        let to_target = targets.iter().map(|&o| grid.distance(t, o)).min().unwrap_or(reach);
        cost += 30 * (reach - to_target);
        cost += 60 * threats.iter().filter(|&&o| grid.distance(t, o) <= 1).count() as i32;
        if fire.is_some_and(|(c, range)| grid.distance(t, c) <= range) {
            cost += 20;
        }
    }
    if t == unit.tile {
        // Staying put keeps it dug in.
        cost -= 10;
    }
    cost
}

/// The best tile for `unit` to strike one of `targets` from (within
/// `reach`, no nearer than `nearest` to any), weighing how soon it gets there (a turn outweighs everything
/// else) against [`post_cost`]. `None`: nowhere free.
fn best_post(game: &Game, near: &Near, targets: &[TileId], reach: i32, nearest: i32, threats: &[TileId], fire: Option<(TileId, i32)>) -> Option<TileId> {
    let u = game.units.get(&near.unit)?;
    let &first = targets.first()?;
    let sea = game.unit_def(u).domain == Domain::Sea;
    let land = (!sea).then(|| ashore(game, first, reach + 3));
    let grid = &game.map.grid;
    let mut tiles: Vec<TileId> = targets.iter().flat_map(|&t| grid.within(t, reach)).collect();
    tiles.sort();
    tiles.dedup();
    let mut tiles: Vec<(i32, i32, TileId)> = tiles
        .into_iter()
        .filter(|&t| targets.iter().all(|&o| grid.distance(t, o) >= nearest) && land.as_ref().is_none_or(|l| l.contains(&t)))
        .filter(|&t| t == u.tile || game.can_stop_at(u, t))
        .map(|t| (post_cost(game, u, t, targets, reach, threats, fire), grid.distance(t, u.tile), t))
        .collect();
    // The few best and the few nearest (each a path search).
    tiles.sort();
    let mut shortlist: Vec<TileId> = tiles.iter().take(6).map(|&(_, _, t)| t).collect();
    tiles.sort_by_key(|&(c, d, t)| (d, c, t));
    shortlist.extend(tiles.iter().take(6).map(|&(_, _, t)| t));
    let mut seen = std::collections::HashSet::new();
    shortlist.retain(|t| seen.insert(*t));
    let turns = turns_to(game, near, &shortlist);
    shortlist
        .into_iter()
        .zip(turns)
        .filter_map(|(t, turns)| Some((turns? * 100 + post_cost(game, u, t, targets, reach, threats, fire), t)))
        .min()
        .map(|(_, t)| t)
}

/// Where a unit can get within a few turns, searched once when first
/// needed and shared by the choices made for it this turn.
struct Near {
    unit: UnitId,
    reach: std::cell::OnceCell<std::collections::HashMap<TileId, u32>>,
}

impl Near {
    /// How far ahead the search looks, in turns.
    const TURNS: u32 = 4;

    fn new(unit: UnitId) -> Near {
        Near { unit, reach: std::cell::OnceCell::new() }
    }

    fn reach(&self, game: &Game) -> &std::collections::HashMap<TileId, u32> {
        self.reach.get_or_init(|| game.reach_for(self.unit, Self::TURNS))
    }
}

/// How many turns of moving the unit needs to stand on each of `tiles` (0
/// where it is, 1 this turn...; `None`: it can't get there, or isn't looked
/// at). Those within a few turns come from one search ([`Near`]); if none
/// is that close, path searches to the nearest few it can get to
/// (searches that fail cost the most).
fn turns_to(game: &Game, near: &Near, tiles: &[TileId]) -> Vec<Option<i32>> {
    const FAR_CANDIDATES: usize = 3;
    let Some(u) = game.units.get(&near.unit) else { return vec![None; tiles.len()] };
    let here = u.tile;
    let reach = near.reach(game);
    let close: Vec<Option<i32>> = tiles.iter().map(|&t| if t == here { Some(0) } else { reach.get(&t).map(|&n| n as i32 + 1) }).collect();
    if close.iter().any(|n| n.is_some_and(|n| n > 0)) || tiles.iter().all(|&t| t == here) {
        return close;
    }
    let grid = &game.map.grid;
    let mut order: Vec<usize> = (0..tiles.len()).collect();
    order.sort_by_key(|&i| (grid.distance(tiles[i], here), i));
    let mut out = vec![None; tiles.len()];
    let mut found = 0;
    for i in order {
        if found == FAR_CANDIDATES {
            break;
        }
        out[i] = if tiles[i] == here { Some(0) } else { game.path_for(near.unit, tiles[i]).map(|p| p.arrival_turn() as i32 + 1) };
        found += out[i].is_some() as usize;
    }
    out
}

/// A melee unit's part: strike enemy soldiers by the city (those next to
/// our ranged units, `covering`, first), stand guard in front of the ranged
/// units until the walls are low (`storm`), then storm, and walk in once
/// they're down.
fn strike_melee(game: &mut Game, player: PlayerId, unit: UnitId, city: CityId, storm: bool, covering: &[TileId], boldness: f32, events: &mut Vec<Event>) {
    let Some(c) = game.cities.get(&city) else { return };
    let (tile, owner, walls) = (c.tile, c.owner, c.hp);
    let fair = move |o: PlayerId| o == owner;
    if walls > 0 {
        let grid = &game.map.grid;
        let by_ours: std::collections::HashSet<TileId> = covering.iter().flat_map(|&t| grid.neighbors(t)).collect();
        let at_our_archers = |t: TileId, p: &CombatPreview| not_city(t, p) && by_ours.contains(&t);
        if fight::attack_where(game, unit, boldness, &fair, &at_our_archers, events) || fight::attack_where(game, unit, boldness, &fair, &not_city, events) {
            return;
        }
    }
    if !game.units.contains_key(&unit) {
        return;
    }
    if !storm && walls > 0 {
        if !screen(game, player, unit, city, covering, events) {
            // Nobody to screen: just out of the city's reach, ready.
            let out = game.city_range(city) + 1;
            go_near(game, unit, tile, out, events);
        }
        return;
    }
    let next_door = game.units.get(&unit).is_some_and(|u| game.map.grid.distance(u.tile, tile) == 1 && !game.is_embarked(u));
    if !next_door {
        if let Some(dest) = best_post(game, &Near::new(unit), &[tile], 1, 1, &[], None) {
            if let Ok(e) = game.advance(unit, dest) {
                events.extend(e);
            }
        }
    }
    let Some(p) = game.units.get(&unit).and_then(|_| game.combat_preview(unit, tile).ok()) else { return };
    // A city at 0 is walked into; otherwise storm whenever the unit comes
    // out of it fit to fight on (the wounded go home to heal).
    if p.captures || (!p.attacker_dies() && p.attacker_hp - p.damage_taken >= HEAL_BELOW) {
        if let Ok(e) = game.apply(player, Command::Attack { unit, tile }) {
            events.extend(e);
        }
    }
}

/// Moves a melee unit in front of our ranged units (`covering`): onto a
/// tile next to one of them that's no further from the city than it is,
/// the more of them it covers the better, on good ground, so enemies have
/// to go through it to reach them. Returns whether it found a place.
fn screen(game: &mut Game, player: PlayerId, unit: UnitId, city: CityId, covering: &[TileId], events: &mut Vec<Event>) -> bool {
    let Some(u) = game.units.get(&unit) else { return false };
    let Some(c) = game.cities.get(&city) else { return false };
    let (tile, owner) = (c.tile, c.owner);
    let grid = &game.map.grid;
    let land = ashore(game, tile, game.city_range(city) + 4);
    let threats = soldiers_near(game, player, owner, tile, 4);
    let mut tiles: Vec<TileId> = covering
        .iter()
        .flat_map(|&r| grid.neighbors(r).filter(move |&t| grid.distance(t, tile) == 1))
        .filter(|&t| t != tile && land.contains(&t) && !covering.contains(&t))
        .filter(|&t| t == u.tile || game.can_stop_at(u, t))
        .collect();
    tiles.sort();
    tiles.dedup();
    let cost = |t: TileId| {
        let covered = covering.iter().filter(|&&r| grid.distance(r, t) == 1).count() as i32;
        let facing = threats.iter().filter(|&&o| grid.distance(o, t) <= 2).count() as i32;
        let stay = if t == u.tile { 10 } else { 0 };
        -40 * covered - 10 * facing - game.map.defense_bonus(&game.rules, t) / 2 - stay
    };
    let mut ranked: Vec<(i32, i32, TileId)> = tiles.into_iter().map(|t| (cost(t), grid.distance(t, u.tile), t)).collect();
    ranked.sort();
    ranked.truncate(6);
    let turns = turns_to(game, &Near::new(unit), &ranked.iter().map(|&(_, _, t)| t).collect::<Vec<_>>());
    let best = ranked.into_iter().zip(turns).filter_map(|((c, _, t), turns)| Some((turns? * 100 + c, t))).min().map(|(_, t)| t);
    let Some(dest) = best else { return false };
    if dest != u.tile {
        if let Ok(e) = game.advance(unit, dest) {
            events.extend(e);
        }
    }
    true
}

/// Ships sent to take a city: bombard it (ranged) or storm it once its
/// walls are down (melee), and go home to repair when badly hurt.
fn run_ships(game: &mut Game, player: PlayerId, city: CityId, ships: &[UnitId], boldness: f32, events: &mut Vec<Event>) {
    for &s in ships {
        if heal(game, s, events) || !game.units.contains_key(&s) {
            continue;
        }
        if game.unit_def(&game.units[&s]).ranged > 0 {
            strike_ranged(game, player, s, city, boldness, events);
        } else {
            strike_melee(game, player, s, city, true, &[], boldness, events);
        }
    }
}

/// Generals and Admirals stay with their group: on `at`, or beside it.
fn follow(game: &mut Game, leaders: &[UnitId], at: Option<TileId>, events: &mut Vec<Event>) {
    let Some(at) = at else { return };
    for &g in leaders {
        go_near(game, g, at, 1, events);
    }
}

fn run_defend(game: &mut Game, player: PlayerId, city: CityId, members: &[UnitId], boldness: f32, events: &mut Vec<Event>) {
    let Some(c) = game.cities.get(&city) else { return done(game, player, members, events) };
    if c.owner != player {
        return done(game, player, members, events);
    }
    let tile = c.tile;
    let members = alive(game, members);
    let land: Vec<UnitId> = members.iter().copied().filter(|m| game.slot(&game.units[m]) == Slot::Land).collect();
    // The garrison: a soldier of ours in the city, or the member that can
    // get there that serves best: a ranged one (it shoots from the walls),
    // else the strongest.
    let mut garrison = game.garrison(city);
    if garrison.is_none() {
        let mut by_strength = land.clone();
        by_strength.sort_by_key(|m| {
            let u = &game.units[m];
            let def = game.unit_def(u);
            (def.ranged == 0, std::cmp::Reverse(def.strength.max(def.ranged)), game.map.grid.distance(u.tile, tile), *m)
        });
        for m in by_strength {
            if game.path_for(m, tile).is_some() {
                if let Ok(e) = game.advance(m, tile) {
                    events.extend(e);
                }
                garrison = Some(m);
                break;
            }
        }
    }
    let war = war_table(game, player);
    let zone: std::collections::HashSet<TileId> = game.map.grid.within(tile, DEFEND_RADIUS).into_iter().collect();
    let in_reach = |t: TileId, _: &CombatPreview| zone.contains(&t);
    // Where trouble comes from: the nearest enemy soldier in sight.
    let threat = game
        .map
        .grid
        .within(tile, 8)
        .into_iter()
        .filter(|&t| game.players[player as usize].sees(t) && game.city_at(t).is_none())
        .filter(|&t| game.units_at(t).any(|u| war[u.owner as usize] && game.unit_def(u).class == UnitClass::Military))
        .min_by_key(|&t| (game.map.grid.distance(t, tile), t));
    // Ranged units shoot first, so the melee units' blows land on wounded enemies.
    let mut members = members;
    members.sort_by_key(|m| (game.unit_def(&game.units[m]).ranged == 0, *m));
    for &m in &members {
        let Some(u) = game.units.get(&m) else { continue };
        let slot = game.slot(u);
        if slot == Slot::Leader {
            go_near(game, m, tile, 1, events);
            continue;
        }
        // Ships guard the harbour: strike what comes close, else lie in it.
        if slot == Slot::Sea {
            if !heal(game, m, events) && !fight::attack_where(game, m, boldness, &|o| war[o as usize], &in_reach, events) {
                go_near(game, m, tile, 1, events);
            }
            continue;
        }
        let enemies = |o: PlayerId| war[o as usize];
        // The garrison only shoots: a melee blow that kills would carry it
        // out of the city.
        let ranged = game.unit_def(u).ranged > 0;
        if Some(m) == garrison {
            if ranged {
                fight::attack_where(game, m, boldness, &|o| enemies(o), &in_reach, events);
            }
            continue;
        }
        if heal(game, m, events) {
            continue;
        }
        if game.units.contains_key(&m) && fight::attack_where(game, m, boldness, &|o| enemies(o), &in_reach, events) {
            continue;
        }
        // No good fight: stand guard next to the city, melee units on the
        // side the enemy comes from, ranged units behind them.
        if let Some(u) = game.units.get(&m) {
            if threat.is_some() || game.map.grid.distance(u.tile, tile) > 1 {
                guard_post(game, m, tile, threat, events);
            }
        }
    }
}

/// Moves a defender to the best free tile next to the city it guards:
/// facing `threat` (the nearest enemy) for melee units, away from it for
/// ranged ones, then hills and forests. A unit stays put unless another
/// post is clearly better, so it keeps dug in.
fn guard_post(game: &mut Game, unit: UnitId, city: TileId, threat: Option<TileId>, events: &mut Vec<Event>) {
    let Some(u) = game.units.get(&unit) else { return };
    let grid = &game.map.grid;
    let ranged = game.unit_def(u).ranged > 0;
    let cost = |t: TileId| {
        let facing = threat.map_or(0, |o| if ranged { -grid.distance(t, o) } else { grid.distance(t, o) });
        let stay = if t == u.tile { 20 } else { 0 };
        facing * 30 - game.map.defense_bonus(&game.rules, t) - stay
    };
    let mut posts: Vec<TileId> = grid
        .neighbors(city)
        .filter(|&t| (t == u.tile || game.can_stop_at(u, t)) && game.map.land_move_cost(&game.rules, t).is_some())
        .collect();
    posts.sort_by_key(|&t| (cost(t), grid.distance(t, u.tile), t));
    for t in posts.into_iter().take(4) {
        if t == u.tile {
            return;
        }
        if game.path_for(unit, t).is_some() {
            if let Ok(e) = game.advance(unit, t) {
                events.extend(e);
            }
            return;
        }
    }
    if grid.distance(u.tile, city) > 1 {
        go_near(game, unit, city, 2, events);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::mapgen::{generate, MapSettings};
    use sim::rules::{Ability, Rules};

    /// Two players: Rome's city, and our units a few tiles from it, at war.
    fn siege() -> (Game, CityId, Vec<UnitId>) {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 1);
        let settler = g.units.values().find(|u| u.owner == 1 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(1, Command::FoundCity { unit: settler }).unwrap();
        let city = *g.cities.keys().next().unwrap();
        let home = g.cities[&city].tile;
        g.units.retain(|_, u| g.map.grid.distance(u.tile, home) > 8 && u.owner == 0);
        let open = |g: &Game, t: TileId| g.map.land_move_cost(&g.rules, t).is_some() && g.units_at(t).next().is_none() && g.city_at(t).is_none();
        let far: Vec<TileId> = g.map.grid.within(home, 6).into_iter().filter(|&t| g.map.grid.distance(t, home) >= 5 && open(&g, t)).collect();
        let start = far[0];
        let mut spots: Vec<TileId> = g.map.grid.within(start, 2).into_iter().filter(|&t| open(&g, t) && g.map.grid.distance(t, home) >= 4).collect();
        spots.sort_by_key(|&t| g.map.grid.distance(t, start));
        let mut ours = Vec::new();
        for (kind, &t) in ["swordsman", "swordsman", "swordsman", "catapult", "archer"].iter().zip(&spots) {
            ours.push(g.spawn_unit(0, g.rules.unit_type_id(kind).unwrap(), t));
        }
        for p in 0..2 {
            g.players[p].met.fill(true);
            g.players[p].explored.fill(true);
            g.update_visibility(p as PlayerId);
        }
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        // Enough gold for the upkeep (we have no city here).
        g.players[0].gold = 100_000;
        (g, city, ours)
    }

    #[test]
    fn a_group_sent_to_take_a_city_gathers_strikes_and_takes_it() {
        let (mut g, city, ours) = siege();
        for &unit in &ours {
            g.apply(0, Command::SetOrder { unit, order: Order::Take { city } }).unwrap();
        }
        let home = g.cities[&city].tile;
        let mut gathered_first = false;
        for turn in 0..40 {
            run(&mut g, 0, 0.5);
            let near: Vec<i32> = ours.iter().filter_map(|u| g.units.get(u)).map(|u| g.map.grid.distance(u.tile, home)).collect();
            // Nobody walks into the city's fire before the others are near.
            if turn == 0 {
                gathered_first = near.iter().all(|&d| d > g.city_range(city));
            }
            if g.cities[&city].owner == 0 {
                break;
            }
            // The archer and the catapult shoot from as far back as they can.
            for u in ours[3..].iter().filter_map(|u| g.units.get(u)) {
                assert!(g.map.grid.distance(u.tile, home) > 1, "ranged units stay out of the front line (turn {turn})");
            }
            g.apply(0, Command::EndTurn).unwrap();
        }
        assert!(gathered_first, "the group gathers out of the city's fire first");
        assert_eq!(g.cities[&city].owner, 0, "the city falls");
        assert!(ours.iter().filter_map(|u| g.units.get(u)).all(|u| u.order == Order::None), "they ask for orders again");
    }

    #[test]
    fn defenders_garrison_the_city_and_guard_it() {
        let (mut g, city, ours) = siege();
        // Defending our own city: hand Rome's city and its land to us for the test.
        g.cities.get_mut(&city).unwrap().owner = 0;
        let home = g.cities[&city].tile;
        for t in g.map.grid.within(home, 3) {
            if g.territory[t as usize] == Some(1) {
                g.set_territory(t, Some(0));
            }
        }
        for &unit in &ours[..3] {
            g.apply(0, Command::SetOrder { unit, order: Order::Defend { city } }).unwrap();
        }
        for _ in 0..6 {
            run(&mut g, 0, 0.5);
            g.apply(0, Command::EndTurn).unwrap();
        }
        assert!(g.garrison(city).is_some_and(|u| ours.contains(&u)), "one stands in the city");
        assert!(ours[..3].iter().all(|u| g.map.grid.distance(g.units[u].tile, home) <= 1), "the others next to it");
        assert!(ours[..3].iter().all(|u| g.units[u].order == Order::Defend { city }), "and they keep at it");
    }

    #[test]
    fn a_ranged_defender_garrisons_the_city() {
        let (mut g, city, ours) = siege();
        g.cities.get_mut(&city).unwrap().owner = 0;
        let home = g.cities[&city].tile;
        for t in g.map.grid.within(home, 3) {
            if g.territory[t as usize] == Some(1) {
                g.set_territory(t, Some(0));
            }
        }
        let group = [ours[0], ours[4]];
        for &unit in &group {
            g.apply(0, Command::SetOrder { unit, order: Order::Defend { city } }).unwrap();
        }
        for _ in 0..6 {
            run(&mut g, 0, 0.5);
            g.apply(0, Command::EndTurn).unwrap();
        }
        assert_eq!(g.garrison(city), Some(ours[4]), "the archer shoots from the walls");
        assert_eq!(g.map.grid.distance(g.units[&ours[0]].tile, home), 1, "the swordsman guards beside the city");
    }

    #[test]
    fn right_clicking_a_city_sends_the_group_to_take_or_defend_it() {
        let (mut g, city, ours) = siege();
        let home = g.cities[&city].tile;
        crate::group::command(&mut g, 0, &ours, home).unwrap();
        assert!(ours.iter().all(|u| g.units[u].order == Order::Take { city }));
        // Peace ends it.
        g.players[0].relations[1].at_war = false;
        g.players[1].relations[0].at_war = false;
        run(&mut g, 0, 0.5);
        assert!(ours.iter().all(|u| g.units[u].order == Order::None));
    }
}
