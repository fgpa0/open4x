//! Attacks: units take fights the combat preview says are good (captures,
//! kills, or trades in their favour) against players they're at war with.
//! Risk-tolerant leaders accept worse trades.

use sim::combat::{CombatPreview, Target};
use sim::game::{Command, Event, Game, Order, PlayerId, UnitId};
use sim::hex::TileId;
use sim::rules::UnitClass;

use crate::strategy::Strategy;

/// How much an attack is worth, or `None` if it isn't worth making.
/// `boldness` (0..1) lowers the trade needed.
pub fn score(p: &CombatPreview, boldness: f32) -> Option<i32> {
    if p.attacker_dies() {
        return None;
    }
    let ratio = 1.5 - 0.6 * boldness;
    let healthy_after = 40.0 - 20.0 * boldness;
    let good_trade = p.damage_dealt as f32 >= p.damage_taken as f32 * ratio && (p.attacker_hp - p.damage_taken) as f32 >= healthy_after;
    match p.target {
        Target::Civilians => Some(200),
        Target::City(_) if p.captures => Some(500),
        // Ranged units take nothing back, so any hit on a city's walls counts,
        // and wearing a city down beats picking at its defenders.
        Target::City(_) if p.ranged => (p.damage_dealt > 0).then(|| p.damage_dealt * 3 + 100),
        // Storming the walls: worth it for a good trade.
        Target::City(_) if good_trade => Some(p.damage_dealt * 2 - p.damage_taken + 50),
        Target::City(_) => None,
        _ if p.target_destroyed() => Some(300 - p.damage_taken),
        // A good trade that leaves the attacker healthy enough to carry on.
        _ if good_trade => Some(p.damage_dealt - p.damage_taken),
        _ => None,
    }
}

/// Who owns what an attack on `tile` would hit.
fn defender(game: &Game, p: &CombatPreview, tile: TileId, attacker: PlayerId) -> Option<PlayerId> {
    match p.target {
        Target::Unit(id) => game.units.get(&id).map(|u| u.owner),
        Target::City(id) => game.cities.get(&id).map(|c| c.owner),
        Target::Civilians => game.units_attacked(attacker, tile).first().map(|u| u.owner),
    }
}

/// Whether `s.me` is willing to attack what's on `tile` (owned by `owner`):
/// only players it's at war with.
pub fn fair_game(s: &Strategy, owner: PlayerId) -> bool {
    s.hostile[owner as usize]
}

/// `unit`'s best worthwhile attack on a tile `wanted` allows, against a
/// player `fair` allows, if any.
pub fn best_attack_where(
    game: &Game,
    unit: UnitId,
    boldness: f32,
    fair: &dyn Fn(PlayerId) -> bool,
    wanted: &dyn Fn(TileId, &CombatPreview) -> bool,
) -> Option<TileId> {
    let u = game.units.get(&unit)?;
    let me = u.owner;
    let reach = game.unit_def(u).range.max(1);
    game.map
        .grid
        .within(u.tile, reach)
        .into_iter()
        .filter_map(|t| {
            let p = game.combat_preview(unit, t).ok()?;
            let owner = defender(game, &p, t, me)?;
            if !fair(owner) || !wanted(t, &p) {
                return None;
            }
            score(&p, boldness).map(|v| (v, t))
        })
        .max_by_key(|&(v, t)| (v, std::cmp::Reverse(t)))
        .map(|(_, t)| t)
}

/// Attacks with `unit` while it has a worthwhile target (some units attack
/// twice) on a tile `wanted` allows; returns whether it did.
pub fn attack_where(
    game: &mut Game,
    unit: UnitId,
    boldness: f32,
    fair: &dyn Fn(PlayerId) -> bool,
    wanted: &dyn Fn(TileId, &CombatPreview) -> bool,
    events: &mut Vec<Event>,
) -> bool {
    let Some(me) = game.units.get(&unit).map(|u| u.owner) else { return false };
    let mut attacked = false;
    while let Some(tile) = best_attack_where(game, unit, boldness, fair, wanted) {
        match game.apply(me, Command::Attack { unit, tile }) {
            Ok(e) => {
                events.extend(e);
                attacked = true;
            }
            Err(_) => break,
        }
        if !game.units.contains_key(&unit) {
            break;
        }
    }
    attacked
}

/// Attacks with `unit` if it has a worthwhile target; returns whether it did.
pub fn try_attack(game: &mut Game, s: &Strategy, unit: UnitId, boldness: f32, events: &mut Vec<Event>) -> bool {
    attack_where(game, unit, boldness, &|o| fair_game(s, o), &|_, _| true, events)
}

/// Every military unit of `s.me` without a standing order to take or
/// defend a city (those fight as their order says, see `orders.rs`) makes
/// its best worthwhile attacks.
pub fn attack_targets(game: &mut Game, s: &Strategy) -> Vec<Event> {
    let mut events = Vec::new();
    let boldness = s.drives.risk as f32 / 10.0;
    let units: Vec<UnitId> = game
        .units
        .values()
        .filter(|u| u.owner == s.me && u.moves > 0 && game.unit_def(u).class == UnitClass::Military)
        .filter(|u| !matches!(u.order, Order::Take { .. } | Order::Defend { .. }))
        .map(|u| u.id)
        .collect();
    for unit in units {
        try_attack(game, s, unit, boldness, &mut events);
    }
    events
}
