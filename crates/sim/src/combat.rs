//! Combat. Deterministic, so the preview the player sees is exactly what
//! happens. Each side's strength is its base strength with percentage bonuses
//! (terrain, fortification, walls), scaled down by damage taken. The damage
//! dealt depends on the ratio of the two strengths: equal strengths deal 30
//! damage each way. Ranged attackers take no damage back.
//!
//! Melee attackers that destroy the last defender move into the tile. Cities
//! have their own hit points; an attack from next to a city that leaves it at
//! 0 captures it, whatever the attacker (melee, ranged or a ship), and the
//! attacker moves in. Civilians caught alone are captured; so are those in a
//! city that falls, while its military units are pushed out to the nearest
//! free tiles (ships to water), and lost only with nowhere to go.
//!
//! At sea: embarked land units can't attack and defend at half strength;
//! land units can't fight their way onto water (bridges aside).

use crate::city::CityId;
use crate::civ::{CombatCond, Effect};
use crate::game::{CommandError, Event, Game, Order, PlayerId, Unit, UnitId};
use crate::hex::TileId;
use crate::map::Elevation;
use crate::path::Path;
use crate::rules::{Ability, Domain, UnitClass, UnitTypeId};
use serde::{Deserialize, Serialize};

pub const MAX_HP: i32 = 100;
pub const CITY_MAX_HP: i32 = 100;
/// Damage each side deals when strengths are equal.
const BASE_DAMAGE: f32 = 30.0;
const FORTIFY_BONUS: i32 = 25;
/// Strength change for land units at sea.
const EMBARKED_PENALTY: i32 = -50;
/// A city's shot is this share (percent) of its strength (its walls' archers,
/// not the whole city)...
const STRIKE_PERCENT: i32 = 60;
/// ...and never deals more than this, so it wounds besiegers rather than
/// killing a healthy one outright.
const STRIKE_MAX_DAMAGE: i32 = 40;
/// How far units in a falling city may flee.
const FLEE_RANGE: i32 = 2;
/// A city's strength on its own: this, plus [`CITY_ERA_STRENGTH`] for each
/// era its owner has reached, plus one per citizen.
pub const CITY_BASE_STRENGTH: i32 = 12;
pub const CITY_ERA_STRENGTH: i32 = 7;
/// What a city repairs each turn, besieged (enemies next to it) or not; a
/// city attacked that turn repairs nothing.
const CITY_REPAIR: i32 = 15;
const CITY_REPAIR_BESIEGED: i32 = 5;
/// The share of its garrison's strength (percent) a city adds to its own.
pub const GARRISON_PERCENT: i32 = 50;

/// What an attack hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Unit(UnitId),
    City(CityId),
    /// Civilians alone on a tile: captured without a fight.
    Civilians,
}

/// A labelled percentage bonus (or penalty) to one side's strength.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Modifier {
    pub label: String,
    pub percent: i32,
}

/// What happened in a fight, for messages and effects (the units involved
/// may no longer exist).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Attacker {
    Unit { id: UnitId, kind: UnitTypeId },
    /// A city with walls firing at an enemy nearby.
    City(CityId),
    /// A fort on this tile firing at an enemy next to it.
    Fort(TileId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatReport {
    pub attacker: Attacker,
    pub attacker_owner: PlayerId,
    pub defender_owner: PlayerId,
    /// The defending unit's type (`None` for a city or captured civilians).
    pub defender_kind: Option<UnitTypeId>,
    pub city: Option<CityId>,
    pub from: TileId,
    pub tile: TileId,
    pub ranged: bool,
    pub damage_dealt: i32,
    pub damage_taken: i32,
    pub defender_destroyed: bool,
    pub attacker_died: bool,
}

/// The exact outcome of an attack, computed without changing anything.
#[derive(Clone, Debug, PartialEq)]
pub struct CombatPreview {
    pub target: Target,
    pub ranged: bool,
    pub attack_base: i32,
    pub defense_base: i32,
    pub attack_mods: Vec<Modifier>,
    pub defense_mods: Vec<Modifier>,
    /// Effective strengths after bonuses and damage.
    pub attack: f32,
    pub defense: f32,
    pub damage_dealt: i32,
    pub damage_taken: i32,
    pub attacker_hp: i32,
    pub target_hp: i32,
    pub target_max_hp: i32,
    /// The attack takes the city.
    pub captures: bool,
}

impl CombatPreview {
    pub fn target_destroyed(&self) -> bool {
        match self.target {
            Target::City(_) => self.captures,
            Target::Civilians => false,
            Target::Unit(_) => self.damage_dealt >= self.target_hp,
        }
    }

    pub fn attacker_dies(&self) -> bool {
        self.damage_taken >= self.attacker_hp
    }
}

/// One side of a fight, for working out bonuses.
#[derive(Clone, Copy, Debug)]
struct Side {
    owner: PlayerId,
    /// The unit fighting (`None` for a city), for its rank and General.
    id: Option<UnitId>,
    /// The unit type (`None` for a city).
    unit: Option<UnitTypeId>,
    tile: TileId,
    hp: i32,
    max_hp: i32,
}

impl Side {
    fn city(&self) -> bool {
        self.unit.is_none()
    }
}

fn effective(base: i32, mods: &[Modifier], hp: i32, max_hp: i32) -> f32 {
    let bonus: i32 = mods.iter().map(|m| m.percent).sum();
    let wounded = 0.5 + 0.5 * hp.clamp(0, max_hp) as f32 / max_hp as f32;
    (base as f32 * (1.0 + bonus as f32 / 100.0) * wounded).max(0.1)
}

/// Damage dealt by a side with strength `a` against strength `d`.
fn damage(a: f32, d: f32) -> i32 {
    (BASE_DAMAGE * (a / d).powf(1.3)).round().clamp(1.0, 100.0) as i32
}

impl Game {
    /// Combat strength of a city: its own, which grows with its owner's era
    /// and its size, plus half its garrison's (walls and other defences add
    /// their bonus on top, see [`Self::city_walls`]).
    pub fn city_strength(&self, city: CityId) -> i32 {
        self.city_own_strength(city) + self.garrison_strength(city)
    }

    /// A city's strength without its garrison.
    pub fn city_own_strength(&self, city: CityId) -> i32 {
        let c = &self.cities[&city];
        CITY_BASE_STRENGTH + CITY_ERA_STRENGTH * self.era(c.owner) as i32 + c.population
    }

    /// What the garrison adds to a city's strength: half its strength.
    pub fn garrison_strength(&self, city: CityId) -> i32 {
        self.garrison(city).map_or(0, |g| self.unit_def(&self.units[&g]).strength * GARRISON_PERCENT / 100)
    }

    /// The city's garrison: its owner's land soldier standing in it (not a
    /// General). It strengthens the city and takes no damage while the city
    /// stands; when the city falls it's pushed out (see [`Self::flee`]).
    pub fn garrison(&self, city: CityId) -> Option<UnitId> {
        let c = self.cities.get(&city)?;
        self.units_at(c.tile)
            .find(|u| u.owner == c.owner && self.slot(u) == crate::game::Slot::Land && self.unit_def(u).strength > 0)
            .map(|u| u.id)
    }

    /// The walls' defence bonus of a city (percent; 0 without walls).
    pub fn city_walls(&self, city: CityId) -> i32 {
        self.cities[&city].buildings.iter().map(|&b| self.rules.building(b).defense).sum()
    }

    /// How far a city fires at enemies each turn: next door without walls,
    /// further with them.
    pub fn city_range(&self, city: CityId) -> i32 {
        self.cities[&city].buildings.iter().map(|&b| self.rules.building(b).city_range).max().unwrap_or(0).max(1)
    }

    /// Whether a player at war with the city's owner has a military unit next to it.
    pub fn besieged(&self, city: CityId) -> bool {
        let c = &self.cities[&city];
        self.map.grid.neighbors(c.tile).any(|t| {
            self.units_at(t).any(|u| self.at_war(u.owner, c.owner) && self.unit_def(u).class == UnitClass::Military)
        })
    }

    fn unit_side(&self, id: UnitId) -> Side {
        let u = &self.units[&id];
        Side { owner: u.owner, id: Some(id), unit: Some(u.kind), tile: u.tile, hp: u.hp, max_hp: MAX_HP }
    }

    fn city_side(&self, city: CityId) -> Side {
        let c = &self.cities[&city];
        Side { owner: c.owner, id: None, unit: None, tile: c.tile, hp: c.hp, max_hp: CITY_MAX_HP }
    }

    /// Whether a combat condition holds for `me` fighting `them`. `civ` is
    /// the player whose perk it is (for conditions about their territory).
    fn combat_cond(&self, cond: CombatCond, me: &Side, them: &Side, attacking: bool, civ: PlayerId) -> bool {
        let land_by_water = |t: TileId| !self.map.is_water(&self.rules, t) && self.is_coastal(t);
        let tile = self.map.tile(me.tile);
        match cond {
            CombatCond::OwnTerritory => self.territory_owner(me.tile) == Some(me.owner),
            CombatCond::Coastal => attacking && (land_by_water(me.tile) || land_by_water(them.tile)),
            CombatCond::AttackingUnits => attacking && !them.city(),
            CombatCond::AttackingCities => attacking && them.city(),
            CombatCond::Defending => !attacking,
            CombatCond::VsWounded => them.hp < them.max_hp,
            CombatCond::OpenGround => tile.elevation == Elevation::Flat && tile.feature.is_none(),
            CombatCond::VsMounted => them.unit.is_some_and(|u| self.rules.unit(u).has(crate::rules::Ability::Mounted)),
            CombatCond::InOwnLowlands => {
                tile.elevation == Elevation::Flat
                    && (tile.rivers != 0 || self.is_coastal(me.tile))
                    && self.territory_owner(me.tile) == Some(civ)
            }
        }
    }

    /// Unit and civ bonuses for `me` fighting `them`, plus penalties from
    /// `them`'s civ.
    fn side_mods(&self, me: &Side, them: &Side, attacking: bool) -> Vec<Modifier> {
        let mut mods = Vec::new();
        if let Some(kind) = me.unit {
            let def = self.rules.unit(kind);
            for b in &def.bonuses {
                if self.combat_cond(b.when, me, them, attacking, me.owner) {
                    mods.push(Modifier { label: def.name.clone(), percent: b.percent });
                }
            }
        }
        // Its rank, and the General leading it with his traits.
        if let Some(u) = me.id.and_then(|id| self.units.get(&id)) {
            if let Some(r) = self.rank_def(u).filter(|r| r.strength != 0) {
                mods.push(Modifier { label: r.name.clone(), percent: r.strength });
            }
            if let Some(g) = self.leader_of(u) {
                let name = self.units[&g].name.clone().unwrap_or_else(|| self.unit_def(&self.units[&g]).name.clone());
                mods.push(Modifier { label: name, percent: self.rules.experience.aura.strength });
            }
            for (t, e) in self.leader_traits(u) {
                if let crate::experience::TraitEffect::Combat { percent, when } = e {
                    if self.combat_cond(when, me, them, attacking, me.owner) {
                        mods.push(Modifier { label: self.rules.experience.trait_def(t).name.clone(), percent });
                    }
                }
            }
        }
        for (owner, enemy_perk) in [(me.owner, false), (them.owner, true)] {
            let civ = self.civ(owner);
            for perk in [&civ.ability, &civ.leader_ability] {
                for e in &perk.effects {
                    let (percent, when) = match (e, enemy_perk) {
                        (Effect::Combat { percent, when }, false) | (Effect::EnemyCombat { percent, when }, true) => (*percent, *when),
                        _ => continue,
                    };
                    if self.combat_cond(when, me, them, attacking, owner) {
                        mods.push(Modifier { label: perk.name.clone(), percent });
                    }
                }
            }
        }
        mods
    }

    /// A unit's defensive strength, bonuses and hit points where it stands,
    /// against `attacker`.
    fn unit_defense(&self, id: UnitId, attacker: &Side) -> (i32, Vec<Modifier>, i32) {
        self.unit_defense_at(id, self.units[&id].hp, attacker)
    }

    /// [`Self::unit_defense`] with `hp` hit points (in the middle of a fight).
    fn unit_defense_at(&self, id: UnitId, hp: i32, attacker: &Side) -> (i32, Vec<Modifier>, i32) {
        let d = &self.units[&id];
        let mut mods = Vec::new();
        let terrain = self.map.defense_bonus(&self.rules, d.tile);
        if terrain != 0 {
            mods.push(Modifier { label: "Terrain".into(), percent: terrain });
        }
        if self.has_fort(d.tile) {
            mods.push(Modifier { label: "Fort".into(), percent: self.rules.improvements.fort.defense });
        }
        if self.is_embarked(d) {
            mods.push(Modifier { label: "Embarked".into(), percent: EMBARKED_PENALTY });
        } else if d.order == Order::Fortify || (matches!(d.order, Order::Defend { .. }) && !d.moved) {
            mods.push(Modifier { label: "Fortified".into(), percent: FORTIFY_BONUS });
        }
        let me = Side { hp, ..self.unit_side(id) };
        mods.extend(self.side_mods(&me, attacker, false));
        (self.unit_def(d).strength, mods, hp)
    }

    /// Rewards for `killer` destroying an enemy unit at `tile`: gold and
    /// production from perks.
    fn kill_rewards(&mut self, killer: PlayerId, tile: TileId) -> Vec<Event> {
        let mut events = Vec::new();
        let (mut gold, mut production) = (0, 0);
        for e in self.civ_effects(killer) {
            match e {
                Effect::KillGold(g) => gold += g,
                Effect::KillProduction(p) => production += p,
                _ => {}
            }
        }
        self.players[killer as usize].gold += gold;
        if production > 0 {
            let nearest = self
                .cities
                .values()
                .filter(|c| c.owner == killer)
                .min_by_key(|c| (self.map.grid.distance(c.tile, tile), c.id))
                .map(|c| c.id);
            if let Some(city) = nearest {
                self.cities.get_mut(&city).expect("exists").production += production;
                events.push(Event::CityChanged { city });
            }
        }
        events
    }

    /// End of turn: every city fires at the military unit of a player it's
    /// at war with, within reach, that it would hurt most (a kill first).
    /// Walls and other defences make the shot stronger.
    pub(crate) fn city_strikes(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let cities: Vec<CityId> = self.cities.keys().copied().collect();
        for city in cities {
            let range = self.city_range(city);
            let c = &self.cities[&city];
            let (owner, tile) = (c.owner, c.tile);
            let walls = [Modifier { label: "Walls".into(), percent: self.city_walls(city) }];
            let attack = effective(self.city_strength(city) * STRIKE_PERCENT / 100, &walls, c.hp, CITY_MAX_HP);
            let me = self.city_side(city);
            events.extend(self.strike_nearest(owner, tile, range, attack, &me, Attacker::City(city)));
        }
        // Forts fire too, more weakly, at enemies next to them.
        let forts: Vec<(TileId, PlayerId)> = self.forts().filter_map(|t| self.territory[t as usize].map(|o| (t, o))).collect();
        for (tile, owner) in forts {
            let strength = (CITY_BASE_STRENGTH + CITY_ERA_STRENGTH * self.era(owner) as i32) * self.rules.improvements.fort.strike / 100;
            let attack = effective(strength * STRIKE_PERCENT / 100, &[], 1, 1);
            let me = Side { owner, id: None, unit: None, tile, hp: CITY_MAX_HP, max_hp: CITY_MAX_HP };
            events.extend(self.strike_nearest(owner, tile, 1, attack, &me, Attacker::Fort(tile)));
        }
        events
    }

    /// A city's or fort's shot: at the enemy unit within `range` of `tile`
    /// it would hurt most (one it would destroy first).
    fn strike_nearest(&mut self, owner: PlayerId, tile: TileId, range: i32, attack: f32, me: &Side, attacker: Attacker) -> Vec<Event> {
        let mut events = Vec::new();
        let best = self
            .map
            .grid
            .within(tile, range)
            .into_iter()
            .filter(|&t| self.players[owner as usize].sees(t))
            .flat_map(|t| self.units_at(t).map(|u| u.id).collect::<Vec<_>>())
            .filter(|id| {
                let u = &self.units[id];
                self.at_war(owner, u.owner) && self.unit_def(u).class == UnitClass::Military
            })
            .map(|id| {
                let (base, mods, hp) = self.unit_defense(id, me);
                let dmg = damage(attack, effective(base, &mods, hp, MAX_HP)).min(STRIKE_MAX_DAMAGE).min(hp);
                (dmg >= hp, dmg, std::cmp::Reverse(id))
            })
            .max();
        let Some((kills, dmg, std::cmp::Reverse(target))) = best else { return events };
        let t = &self.units[&target];
        events.push(Event::Combat(CombatReport {
            attacker,
            attacker_owner: owner,
            defender_owner: t.owner,
            defender_kind: Some(t.kind),
            city: None,
            from: tile,
            tile: t.tile,
            ranged: true,
            damage_dealt: dmg,
            damage_taken: 0,
            defender_destroyed: kills,
            attacker_died: false,
        }));
        let (victim, at) = (t.owner, t.tile);
        if kills {
            self.units.remove(&target);
            events.push(Event::UnitRemoved { unit: target });
            events.extend(self.kill_rewards(owner, at));
            events.extend(self.update_visibility(victim));
        } else {
            let turn = self.turn;
            let t = self.units.get_mut(&target).expect("exists");
            t.hp -= dmg;
            t.alerted = Some(turn + 1);
        }
        events
    }

    /// Whether `unit` and an enemy at war that its owner can see are within
    /// striking distance of each other: either could attack the other next
    /// turn with its full moves (ignoring zones of control, so it errs on the
    /// side of warning).
    ///
    /// A unit that can't fight itself (a General, an Admiral, a civilian) only
    /// counts as threatened when an attack on its tile would hit it: not in
    /// one of its owner's cities or behind a soldier sharing its tile.
    pub fn in_striking_distance(&self, unit: UnitId) -> bool {
        let u = &self.units[&unit];
        let me = &self.players[u.owner as usize];
        let def = self.unit_def(u);
        let fights = def.class == UnitClass::Military && (def.strength > 0 || def.ranged > 0);
        let covered = !fights
            && (self.city_at(u.tile).is_some_and(|c| c.owner == u.owner)
                || self.units_at(u.tile).any(|o| o.id != u.id && o.owner == u.owner && self.unit_def(o).class == UnitClass::Military && self.unit_def(o).strength > 0));
        if covered {
            return false;
        }
        self.units
            .values()
            .any(|e| self.at_war(u.owner, e.owner) && me.sees(e.tile) && (self.could_strike(e, u) || self.could_strike(u, e)))
    }

    /// Whether `attacker` could move within reach of `target` with its full
    /// moves and attack it.
    fn could_strike(&self, attacker: &Unit, target: &Unit) -> bool {
        let def = self.unit_def(attacker);
        if def.class != UnitClass::Military || (def.strength <= 0 && def.ranged <= 0) {
            return false;
        }
        let grid = &self.map.grid;
        let reach = if def.ranged > 0 { def.range.max(1) } else { 1 };
        let moves = self.turn_moves(attacker);
        // A step costs at least `min_step` (see `path.rs`).
        if grid.distance(attacker.tile, target.tile) > moves / self.min_step() + reach {
            return false;
        }
        let mover = self.mover(attacker);
        // Where it could stand, with the most moves left there.
        let mut left = std::collections::HashMap::from([(attacker.tile, moves)]);
        let mut open = vec![attacker.tile];
        while let Some(at) = open.pop() {
            let m = left[&at];
            if m <= 0 {
                continue;
            }
            // Melee needs to be able to step onto the target's tile.
            if grid.distance(at, target.tile) <= reach && (def.ranged > 0 || self.step_cost(attacker, &mover, at, target.tile).is_some()) {
                return true;
            }
            for nb in grid.neighbors(at) {
                let Some((cost, ends)) = self.step_cost(attacker, &mover, at, nb) else { continue };
                // Entering a tile takes whatever moves are left.
                let rest = if ends { 0 } else { (m - cost).max(0) };
                if left.get(&nb).is_none_or(|&r| rest > r) && grid.distance(nb, target.tile) <= moves + reach {
                    left.insert(nb, rest);
                    open.push(nb);
                }
            }
        }
        false
    }

    /// At the start of a turn, alerts the human players' sleeping and
    /// fortified units that were attacked since their last turn or are within
    /// striking distance of an enemy (see `Unit::alerted`).
    pub(crate) fn alert_threatened(&mut self) {
        let turn = self.turn;
        let ids: Vec<UnitId> = self
            .units
            .values()
            .filter(|u| self.players[u.owner as usize].human && matches!(u.order, Order::Sleep | Order::Fortify))
            .map(|u| u.id)
            .collect();
        for id in ids {
            // Attacks before this point were marked for this turn or the next.
            let struck = self.units[&id].alerted.is_some_and(|t| t >= turn);
            if struck || self.in_striking_distance(id) {
                self.units.get_mut(&id).expect("exists").alerted = Some(turn);
            }
        }
    }

    /// The enemy unit on `tile` that `player` would hunt there: the strongest
    /// fighter, else any unit. Only units `player` can see.
    pub fn hunt_target(&self, player: PlayerId, tile: TileId) -> Option<UnitId> {
        if !self.players[player as usize].sees(tile) {
            return None;
        }
        self.units_at(tile)
            .filter(|o| o.owner != player)
            .max_by_key(|o| (self.unit_def(o).class == UnitClass::Military, self.unit_def(o).strength, std::cmp::Reverse(o.id)))
            .map(|o| o.id)
    }

    /// Where `unit` would go to attack `tile`: the tile within its reach of
    /// `tile` that it gets to soonest, and the path there (empty when it is in
    /// reach already).
    pub fn approach(&self, unit: UnitId, tile: TileId) -> Option<(TileId, Path)> {
        let u = self.units.get(&unit)?;
        let def = self.unit_def(u);
        let reach = if def.ranged > 0 { def.range.max(1) } else { 1 };
        let grid = &self.map.grid;
        let distance = grid.distance(u.tile, tile);
        if distance >= 1 && distance <= reach as _ {
            return Some((u.tile, Path { tiles: Vec::new(), turns: Vec::new(), left: u.moves }));
        }
        grid.within(tile, reach)
            .into_iter()
            .filter(|&t| t != tile)
            .filter_map(|t| self.path_for(unit, t).map(|p| (t, p)))
            .min_by_key(|(t, p)| (p.arrival_turn(), p.tiles.len(), *t))
    }

    /// Where `unit` would go to take the foreign city on `tile`, which it
    /// can't enter: the tile next to it that it gets to soonest, and the path
    /// there. `None` if it's next to the city already, the city isn't known
    /// to its owner, or no tile next to it can be reached.
    pub fn approach_city(&self, unit: UnitId, tile: TileId) -> Option<(TileId, Path)> {
        let u = self.units.get(&unit)?;
        self.city_at(tile).filter(|c| c.owner != u.owner && self.players[u.owner as usize].explored[c.tile as usize])?;
        if self.map.grid.distance(u.tile, tile) <= 1 {
            return None;
        }
        self.map
            .grid
            .neighbors(tile)
            .filter_map(|t| self.path_for(unit, t).map(|p| (t, p)))
            .min_by_key(|(t, p)| (p.arrival_turn(), p.tiles.len(), *t))
    }

    /// Gives `unit` a Hunt order on `target` and starts the chase.
    pub(crate) fn hunt(&mut self, unit: UnitId, target: UnitId) -> Result<Vec<Event>, CommandError> {
        let owner = self.units[&unit].owner;
        let t = self.units.get(&target).ok_or(CommandError::NoSuchUnit)?;
        let tile = t.tile;
        if t.owner == owner {
            return Err(CommandError::CantAttack("that's one of yours"));
        }
        if !self.players[owner as usize].sees(tile) {
            return Err(CommandError::CantAttack("you can't see it"));
        }
        if !self.at_war(owner, t.owner) {
            return Err(CommandError::NotAtWar);
        }
        let def = self.unit_def(&self.units[&unit]);
        if def.class != UnitClass::Military || (def.strength <= 0 && def.ranged <= 0) {
            return Err(CommandError::CantAttack("this unit can't fight"));
        }
        if self.approach(unit, tile).is_none() {
            return Err(CommandError::Unreachable);
        }
        let mut events = self.set_order(unit, Order::Hunt { target, last: tile });
        events.extend(self.pursue(unit));
        Ok(events)
    }

    /// One turn of a Hunt order: attack the target if it can, otherwise close
    /// in, attacking on arrival if moves are left. The hunt ends (and the unit
    /// asks for orders) when the target is gone, out of reach, no longer an
    /// enemy, or lost from sight where it was last seen.
    pub(crate) fn pursue(&mut self, unit: UnitId) -> Vec<Event> {
        let Some(u) = self.units.get(&unit) else { return Vec::new() };
        let Order::Hunt { target, last } = u.order else { return Vec::new() };
        let owner = u.owner;
        let seen = |game: &Game| game.units.get(&target).map(|t| t.tile).filter(|&t| game.players[owner as usize].sees(t));
        let tile = match self.units.get(&target) {
            Some(t) if self.at_war(owner, t.owner) => seen(self).unwrap_or(last),
            _ => return self.set_order(unit, Order::None),
        };
        self.units.get_mut(&unit).expect("exists").order = Order::Hunt { target, last: tile };
        let mut events = Vec::new();
        if seen(self).is_none_or(|t| self.attack_target(unit, t).is_err()) {
            match self.approach(unit, tile) {
                Some((dest, _)) if dest == self.units[&unit].tile => {
                    if seen(self).is_none() {
                        // Lost it.
                        return self.set_order(unit, Order::None);
                    }
                }
                Some((dest, _)) => {
                    if let Ok(e) = self.advance(unit, dest) {
                        events.extend(e);
                    }
                }
                None => return self.set_order(unit, Order::None),
            }
        }
        if let Some(t) = seen(self).filter(|&t| self.attack_target(unit, t).is_ok()) {
            if let Ok(e) = self.attack(unit, t) {
                events.extend(e);
            }
            let caught = self.units.get(&target).is_none_or(|t| t.owner == owner);
            if caught && self.units.contains_key(&unit) {
                events.extend(self.set_order(unit, Order::None));
            }
        }
        events
    }

    /// Why `unit` can't attack `tile`, or what it would hit.
    pub fn attack_target(&self, unit: UnitId, tile: TileId) -> Result<Target, CommandError> {
        self.target_of(unit, tile, true)
    }

    /// The player whose peace with `unit`'s owner is the only thing stopping
    /// it attacking `tile` (attacking would mean declaring war on them).
    pub fn peace_blocks_attack(&self, unit: UnitId, tile: TileId) -> Option<PlayerId> {
        if self.target_of(unit, tile, true) != Err(CommandError::NotAtWar) || self.target_of(unit, tile, false).is_err() {
            return None;
        }
        let me = self.units[&unit].owner;
        self.city_at(tile).map(|c| c.owner).or_else(|| self.units_at(tile).map(|u| u.owner).find(|&o| o != me))
    }

    fn target_of(&self, unit: UnitId, tile: TileId, need_war: bool) -> Result<Target, CommandError> {
        let ranged = self.can_strike(unit, tile)?;
        self.tile_target(self.units[&unit].owner, tile, need_war, ranged)
    }

    /// Why `id` can't strike `tile`, or whether its blow is a ranged one.
    fn can_strike(&self, id: UnitId, tile: TileId) -> Result<bool, CommandError> {
        let u = self.units.get(&id).ok_or(CommandError::NoSuchUnit)?;
        let def = self.unit_def(u);
        if def.class != UnitClass::Military || (def.strength <= 0 && def.ranged <= 0) {
            return Err(CommandError::CantAttack("this unit can't fight"));
        }
        let ranged = def.ranged > 0;
        if u.moves <= 0 {
            return Err(CommandError::NoMovesLeft);
        }
        if def.has(Ability::NoAttackAfterMove) && u.moved {
            return Err(CommandError::CantAttack("can't attack after moving"));
        }
        if self.is_embarked(u) {
            return Err(CommandError::CantAttack("can't attack while embarked"));
        }
        if !self.players[u.owner as usize].sees(tile) {
            return Err(CommandError::CantAttack("you can't see that tile"));
        }
        let distance = self.map.grid.distance(u.tile, tile);
        let reach = if ranged { def.range.max(1) } else { 1 };
        if distance == 0 || distance > reach {
            return Err(CommandError::CantAttack(if ranged { "out of range" } else { "not next to it" }));
        }
        let ship = def.domain == Domain::Sea;
        let walkable = self.map.land_move_cost(&self.rules, tile).is_some() || self.has_bridge(tile) || self.has_tunnel(tile);
        if !ranged && !ship && !walkable {
            return Err(CommandError::CantAttack("can't reach that tile"));
        }
        Ok(ranged)
    }

    /// The other players' units an attack by `owner` on `tile` would meet:
    /// merchants passing through (of players it isn't at war with, see
    /// [`Game::passes`]) stand aside when anyone else is there.
    pub fn units_attacked(&self, owner: PlayerId, tile: TileId) -> Vec<&crate::game::Unit> {
        let mut them: Vec<_> = self.units_at(tile).filter(|o| o.owner != owner).collect();
        let aside = |o: &crate::game::Unit| self.is_merchant(o) && !self.at_war(owner, o.owner);
        if them.iter().any(|o| !aside(o)) {
            them.retain(|o| !aside(o));
        }
        them
    }

    /// What `owner` would hit attacking `tile` (`ranged_only`: a ranged attack).
    fn tile_target(&self, owner: PlayerId, tile: TileId, need_war: bool, ranged_only: bool) -> Result<Target, CommandError> {
        if let Some(c) = self.city_at(tile).filter(|c| c.owner != owner) {
            return if !need_war || self.at_war(owner, c.owner) { Ok(Target::City(c.id)) } else { Err(CommandError::NotAtWar) };
        }
        let enemies = self.units_attacked(owner, tile);
        if need_war && enemies.first().is_some_and(|o| !self.at_war(owner, o.owner)) {
            return Err(CommandError::NotAtWar);
        }
        // The strongest defends (a ship before the army it carries).
        let strength = |o: &crate::game::Unit| if self.is_embarked(o) { self.unit_def(o).strength / 2 } else { self.unit_def(o).strength };
        let defender = enemies
            .iter()
            .filter(|o| self.unit_def(o).class == UnitClass::Military)
            .max_by_key(|o| (strength(o), std::cmp::Reverse(o.id)));
        if let Some(d) = defender {
            return Ok(Target::Unit(d.id));
        }
        if !enemies.is_empty() {
            if ranged_only {
                return Err(CommandError::CantAttack("ranged units can't capture civilians"));
            }
            return Ok(Target::Civilians);
        }
        Err(CommandError::CantAttack("nothing to attack there"))
    }

    /// What would happen if `unit` attacked `tile`.
    pub fn combat_preview(&self, unit: UnitId, tile: TileId) -> Result<CombatPreview, CommandError> {
        self.preview(unit, tile, true)
    }

    /// [`Self::combat_preview`] as if at war with whoever is there.
    pub fn combat_preview_at_war(&self, unit: UnitId, tile: TileId) -> Result<CombatPreview, CommandError> {
        self.preview(unit, tile, false)
    }

    /// One unit against another, or a city: each side's strength with its
    /// bonuses, scaled by its wounds, decides the damage both ways (none
    /// back for a ranged attack).
    fn preview(&self, unit: UnitId, tile: TileId, need_war: bool) -> Result<CombatPreview, CommandError> {
        let target = self.target_of(unit, tile, need_war)?;
        let u = &self.units[&unit];
        let def = self.unit_def(u);
        let ranged = def.ranged > 0;
        let base = if ranged { def.ranged } else { def.strength };
        let me = self.unit_side(unit);
        let none = |them: Option<Side>, attack_mods: Vec<Modifier>| CombatPreview {
            target,
            ranged,
            attack_base: base,
            defense_base: 0,
            attack: effective(base, &attack_mods, u.hp, MAX_HP),
            attack_mods,
            defense_mods: Vec::new(),
            defense: 0.0,
            damage_dealt: 0,
            damage_taken: 0,
            attacker_hp: u.hp,
            target_hp: them.map_or(0, |t| t.hp),
            target_max_hp: them.map_or(0, |t| t.max_hp),
            captures: false,
        };
        let (them, defense_base, defense_mods) = match target {
            Target::Civilians => {
                let them = self.units_attacked(u.owner, tile).first().map(|o| self.unit_side(o.id));
                let mods = them.map(|t| self.side_mods(&me, &t, true)).unwrap_or_default();
                return Ok(none(None, mods));
            }
            Target::City(id) if self.cities[&id].hp <= 0 => {
                // The walls are down: a melee unit walks in and takes it.
                if ranged {
                    return Err(CommandError::CantAttack("the walls are down already: send a melee unit to take it"));
                }
                let them = self.city_side(id);
                let mods = self.side_mods(&me, &them, true);
                return Ok(CombatPreview { captures: true, ..none(Some(them), mods) });
            }
            Target::City(id) => {
                let them = self.city_side(id);
                let mut mods = Vec::new();
                let terrain = self.map.defense_bonus(&self.rules, tile);
                if terrain != 0 {
                    mods.push(Modifier { label: "Terrain".into(), percent: terrain });
                }
                let walls = self.city_walls(id);
                if walls != 0 {
                    mods.push(Modifier { label: "Walls".into(), percent: walls });
                }
                mods.extend(self.side_mods(&them, &me, false));
                (them, self.city_strength(id), mods)
            }
            Target::Unit(id) => {
                let (b, mods, _) = self.unit_defense(id, &me);
                (self.unit_side(id), b, mods)
            }
        };
        let attack_mods = self.side_mods(&me, &them, true);
        let attack = effective(base, &attack_mods, u.hp, MAX_HP);
        let defense = effective(defense_base, &defense_mods, them.hp, them.max_hp);
        let damage_dealt = damage(attack, defense).min(them.hp.max(0));
        let damage_taken = if ranged { 0 } else { damage(defense, attack) };
        // A city falls to a melee blow that brings its walls down.
        let captures = matches!(target, Target::City(_)) && !ranged && damage_dealt >= them.hp;
        Ok(CombatPreview {
            target,
            ranged,
            attack_base: base,
            defense_base,
            attack_mods,
            defense_mods,
            attack,
            defense,
            damage_dealt,
            damage_taken,
            attacker_hp: u.hp,
            target_hp: them.hp,
            target_max_hp: them.max_hp,
            captures,
        })
    }

    pub(crate) fn attack(&mut self, unit: UnitId, tile: TileId) -> Result<Vec<Event>, CommandError> {
        let p = self.combat_preview(unit, tile)?;
        let u = &self.units[&unit];
        let (attacker, from, kind) = (u.owner, u.tile, u.kind);
        let (defender_owner, defender_kind, city) = match p.target {
            Target::Unit(id) => (self.units[&id].owner, Some(self.units[&id].kind), None),
            Target::City(id) => (self.cities[&id].owner, None, Some(id)),
            Target::Civilians => (self.units_attacked(attacker, tile).first().map_or(attacker, |o| o.owner), None, None),
        };
        let mut report = CombatReport {
            attacker: Attacker::Unit { id: unit, kind },
            attacker_owner: attacker,
            defender_owner,
            defender_kind,
            city,
            from,
            tile,
            ranged: p.ranged,
            damage_dealt: p.damage_dealt,
            damage_taken: p.damage_taken,
            defender_destroyed: false,
            attacker_died: p.attacker_dies(),
        };
        self.record_attack(attacker, defender_owner);
        let mut events = Vec::new();
        let mut affected: Vec<PlayerId> = vec![attacker, defender_owner];
        let mut advance = false;
        let mut survivor = None;
        let turn = self.turn;
        match p.target {
            Target::Civilians => {
                events.extend(self.capture_civilians(tile, attacker, &mut affected));
                advance = true;
            }
            Target::Unit(id) => {
                let d = self.units.get_mut(&id).expect("exists");
                d.hp -= p.damage_dealt;
                // Asleep or fortified, it asks for orders on its owner's next turn.
                d.alerted = Some(turn + 1);
                if d.hp <= 0 {
                    self.units.remove(&id);
                    events.push(Event::UnitRemoved { unit: id });
                    events.extend(self.kill_rewards(attacker, tile));
                    report.defender_destroyed = true;
                    let survives = !p.attacker_dies();
                    if !p.ranged && survives && self.unit_def(&self.units[&unit]).has(Ability::CaptureAsWorker) {
                        let room = self.units_at(from).all(|o| self.unit_def(o).class != UnitClass::Civilian);
                        if let Some(worker) = self.rules.unit_type_id("worker").filter(|_| room) {
                            let new = self.spawn_unit(attacker, worker, from);
                            self.units.get_mut(&new).expect("spawned").moves = 0;
                            events.push(Event::UnitSpawned { unit: new });
                        }
                    }
                    if !p.ranged {
                        // Civilians it was guarding are taken too.
                        events.extend(self.capture_civilians(tile, attacker, &mut affected));
                        advance = true;
                    }
                } else {
                    survivor = Some(id);
                }
            }
            Target::City(id) => {
                let c = self.cities.get_mut(&id).expect("exists");
                c.hp = (c.hp - p.damage_dealt).max(0);
                c.attacked = true;
                if p.captures {
                    report.defender_destroyed = true;
                    events.extend(self.capture_city(id, attacker, &mut affected));
                    advance = true;
                }
            }
        }
        // The attacker has used its attack (units that attack twice keep
        // their moves after the first); a standing order (a hunt, taking or
        // defending a city) goes on.
        let twice = self.unit_def(&self.units[&unit]).has(Ability::AttackTwice);
        let a = self.units.get_mut(&unit).expect("exists");
        a.hp -= p.damage_taken;
        a.attacks += 1;
        if !twice || a.attacks >= 2 {
            a.moves = 0;
        }
        if !matches!(a.order, Order::None | Order::Hunt { .. } | Order::Take { .. } | Order::Defend { .. }) {
            a.order = Order::None;
            events.push(Event::OrderChanged { unit, order: Order::None });
        }
        if self.units[&unit].hp <= 0 {
            self.units.remove(&unit);
            events.push(Event::UnitRemoved { unit });
        } else if advance && self.units_at(tile).all(|o| o.owner == attacker || self.passes(&self.units[&unit], o)) && self.mover(&self.units[&unit]).can_enter(&self.map, &self.rules, tile) && self.can_stop_at(&self.units[&unit], tile) {
            self.units.get_mut(&unit).expect("exists").tile = tile;
            events.push(Event::UnitMoved { unit, path: vec![tile] });
        }
        if p.target != Target::Civilians && self.units.contains_key(&unit) {
            events.extend(self.battle_xp(unit, p.ranged, survivor, report.defender_destroyed && matches!(p.target, Target::Unit(_)), p.captures));
        }
        events.insert(0, Event::Combat(report));
        affected.sort();
        affected.dedup();
        for p in affected {
            events.extend(self.update_visibility(p));
        }
        Ok(events)
    }

    /// A unit in `city` as it falls to `by` leaves for the nearest tile it
    /// may stand on within [`FLEE_RANGE`], out of `by`'s land if it can, and
    /// stops there for the turn. With nowhere to go it is lost.
    fn flee(&mut self, unit: UnitId, by: PlayerId, city: CityId) -> Vec<Event> {
        let u = &self.units[&unit];
        let mover = self.mover(u);
        let grid = &self.map.grid;
        let mut tiles: Vec<TileId> = grid.within(u.tile, FLEE_RANGE).into_iter().filter(|&t| t != u.tile).collect();
        tiles.sort_by_key(|&t| (grid.distance(t, u.tile), self.territory_owner(t) == Some(by), t));
        let to = tiles.into_iter().find(|&t| {
            mover.can_enter(&self.map, &self.rules, t) && self.can_stop_at(u, t) && self.units_at(t).all(|o| o.owner == u.owner)
        });
        let (owner, kind) = (u.owner, u.kind);
        match to {
            Some(t) => {
                let u = self.units.get_mut(&unit).expect("exists");
                u.tile = t;
                u.moves = 0;
                u.order = Order::None;
                vec![Event::UnitMoved { unit, path: vec![t] }]
            }
            None => {
                self.units.remove(&unit);
                vec![Event::UnitRemoved { unit }, Event::LostWithCity { kind, owner, city }]
            }
        }
    }

    /// Enemy civilians on `tile` change sides (as new units of the captor).
    fn capture_civilians(&mut self, tile: TileId, by: PlayerId, affected: &mut Vec<PlayerId>) -> Vec<Event> {
        let caught: Vec<(UnitId, PlayerId, u8, i32)> = self
            .units_at(tile)
            .filter(|o| o.owner != by && self.unit_def(o).class == UnitClass::Civilian && self.at_war(by, o.owner))
            .map(|o| (o.id, o.owner, o.kind, self.merchant_cargo(o)))
            .collect();
        let mut events = Vec::new();
        for (id, owner, kind, cargo) in caught {
            affected.push(owner);
            if cargo > 0 {
                self.players[by as usize].gold += cargo;
                events.push(Event::Plundered { by, from: owner, gold: cargo });
            }
            self.units.remove(&id);
            events.push(Event::UnitRemoved { unit: id });
            let new = self.spawn_unit(by, kind, tile);
            self.units.get_mut(&new).expect("spawned").moves = 0;
            events.push(Event::UnitSpawned { unit: new });
            events.push(Event::UnitCaptured { unit: new, from: owner });
        }
        events
    }

    /// A city at 0 hit points falls to `by`: its garrison is pushed out (see
    /// [`Self::flee`]), it shrinks by one and starts over under the new owner.
    fn capture_city(&mut self, city: CityId, by: PlayerId, affected: &mut Vec<PlayerId>) -> Vec<Event> {
        let tile = self.cities[&city].tile;
        let from = self.cities[&city].owner;
        let mut events = Vec::new();
        let garrison: Vec<UnitId> =
            self.units_at(tile).filter(|o| o.owner != by && self.unit_def(o).class == UnitClass::Military).map(|o| o.id).collect();
        for id in garrison {
            events.extend(self.flee(id, by, city));
        }
        events.extend(self.capture_civilians(tile, by, affected));
        let (mut gold_per_pop, mut heal, mut keep) = (0, false, false);
        for e in self.civ_effects(by) {
            match e {
                Effect::CaptureGold(g) => gold_per_pop += g,
                Effect::CaptureHeal => heal = true,
                Effect::KeepCapturedCities => keep = true,
                _ => {}
            }
        }
        self.players[by as usize].gold += gold_per_pop * self.cities[&city].population;
        self.relation_mut(from, by).cities_lost += 1;
        if heal {
            let near = self.map.grid.within(tile, 1);
            for u in self.units.values_mut().filter(|u| u.owner == by && near.contains(&u.tile)) {
                u.hp = MAX_HP;
            }
        }
        // Buildings unique to the old owner's civ are lost.
        let civ = self.players[by as usize].civ;
        let rules = &self.rules;
        let c = self.cities.get_mut(&city).expect("exists");
        c.buildings.retain(|&b| rules.building_civ(b).is_none_or(|o| o == civ));
        c.owner = by;
        c.captured = Some((self.turn, from));
        // Half its walls stand: enough to hold against the next attack or two.
        c.hp = CITY_MAX_HP / 2;
        c.queue.clear();
        c.auto_queue = true;
        if !keep {
            c.population = (c.population - 1).max(1);
            c.production = 0;
            c.food = 0;
        }
        self.hand_over_district(city, by);
        events.push(Event::CityCaptured { city, from });
        events.push(Event::BordersChanged { player: by });
        events.push(Event::BordersChanged { player: from });
        events
    }

    /// Why `city` can't be razed by its owner now, if it can't: only on the
    /// turn it was taken, never an original capital (domination needs them
    /// all standing) or the owner's last city.
    pub fn raze_problem(&self, city: CityId) -> Option<&'static str> {
        let c = self.cities.get(&city)?;
        if c.captured.is_none_or(|(turn, _)| turn != self.turn) {
            return Some("only a city taken this turn can be razed");
        }
        if c.capital_of.is_some() {
            return Some("original capitals can't be razed");
        }
        if !self.cities.values().any(|o| o.owner == c.owner && o.id != city) {
            return Some("it's your only city");
        }
        None
    }

    /// `Command::RazeCity`: the city is removed and its land goes back to
    /// nobody (improvements and roads stay). Trade routes to or from it end,
    /// and deals offering it lapse.
    pub(crate) fn raze(&mut self, city: CityId) -> Vec<Event> {
        for t in self.district_tiles(city) {
            self.territory[t as usize] = None;
        }
        let c = self.cities.remove(&city).expect("checked by caller");
        self.refresh_districts();
        let mut events = Vec::new();
        let routes: Vec<(UnitId, CityId)> = self
            .units
            .values()
            .filter_map(|u| match u.order {
                Order::Trade(r) if r.home == city || r.to == city => Some((u.id, r.to)),
                _ => None,
            })
            .collect();
        for (unit, to) in routes {
            self.trade_paths.remove(&unit);
            events.extend(self.set_order(unit, Order::None));
            events.push(Event::RouteEnded { unit, city: to });
        }
        let offered = crate::diplomacy::Item::City(city);
        self.proposals.retain(|p| !p.deal.give.contains(&offered) && !p.deal.get.contains(&offered));
        let from = c.captured.map_or(c.owner, |(_, from)| from);
        events.push(Event::CityRazed { city, name: c.name, tile: c.tile, by: c.owner, from });
        events.push(Event::BordersChanged { player: c.owner });
        events.extend(self.update_visibility(c.owner));
        events
    }

    /// End of turn: units that didn't move or fight heal (more at home), and
    /// cities that weren't attacked this turn repair. Runs before movement
    /// points are restored.
    pub(crate) fn heal(&mut self) {
        let ids: Vec<UnitId> = self.units.keys().copied().collect();
        for id in ids {
            let u = &self.units[&id];
            if u.hp >= MAX_HP || u.moved || u.attacks > 0 {
                continue;
            }
            let owner = u.owner;
            let home = self.territory_owner(u.tile) == Some(owner);
            let city = self.city_at(u.tile).filter(|c| c.owner == owner);
            // No healing out at sea (a bridge is dry land).
            let afloat = if self.unit_def(u).domain == Domain::Sea { self.map.is_water(&self.rules, u.tile) } else { self.is_embarked(u) };
            if !home && afloat {
                continue;
            }
            let field = if self.civ_has(owner, |e| matches!(e, Effect::HealAnywhere)) { 15 } else { 10 };
            let mut amount = if city.is_some() { 20 } else if home { 15 } else { field };
            if self.unit_def(u).has(Ability::FastHeal) {
                amount += 10;
            }
            amount += self.xp_heal(u, city.is_some());
            if city.is_some_and(|c| c.buildings.iter().any(|&b| self.rules.building(b).heals_units)) {
                amount = MAX_HP;
            }
            let u = self.units.get_mut(&id).expect("exists");
            u.hp = (u.hp + amount).min(MAX_HP);
        }
        let cities: Vec<CityId> = self.cities.keys().copied().collect();
        for id in cities {
            let repair = if self.cities[&id].attacked {
                0
            } else if self.besieged(id) {
                CITY_REPAIR_BESIEGED
            } else {
                CITY_REPAIR
            };
            let c = self.cities.get_mut(&id).expect("exists");
            c.hp = (c.hp + repair).min(CITY_MAX_HP);
            c.attacked = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::city::Build;
    use crate::game::Command;
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Rules;

    /// A game with our warrior next to an enemy unit of `kind` on open ground.
    fn duel(kind: &str) -> (Game, UnitId, UnitId, TileId) {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 1);
        let ours = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).id == "warrior").unwrap().id;
        // Two neighbouring open tiles without terrain bonuses, away from everyone.
        let open = |g: &Game, t: TileId| {
            g.map.land_move_cost(&g.rules, t) == Some(1)
                && g.map.defense_bonus(&g.rules, t) == 0
                && g.units.values().all(|u| g.map.grid.distance(u.tile, t) > 3)
        };
        let (at, tile) = (0..g.map.tiles.len() as TileId)
            .filter(|&t| open(&g, t))
            .find_map(|t| g.map.grid.neighbors(t).find(|&n| open(&g, n)).map(|n| (t, n)))
            .expect("open ground");
        g.units.get_mut(&ours).unwrap().tile = at;
        let enemy = g.spawn_unit(1, g.rules.unit_type_id(kind).unwrap(), tile);
        g.update_visibility(0);
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        (g, ours, enemy, tile)
    }

    #[test]
    fn enemies_in_reach_wake_sleeping_and_fortified_units() {
        let (mut g, ours, enemy, _) = duel("warrior");
        let waiting = |g: &Game| g.units_needing_orders(0).any(|u| u.id == ours);
        g.apply(0, Command::SetOrder { unit: ours, order: Order::Fortify }).unwrap();
        assert!(!waiting(&g), "fortifying answers for this turn");
        g.apply(0, Command::EndTurn).unwrap();
        assert!(waiting(&g), "an enemy next door needs attention");
        // Skipping keeps it fortified and quiet for the turn.
        let events = g.apply(0, Command::SetOrder { unit: ours, order: Order::Skip }).unwrap();
        assert!(!events.is_empty(), "the client hears it was dealt with");
        assert_eq!(g.units[&ours].order, Order::Fortify);
        assert!(!waiting(&g));
        g.apply(0, Command::SetOrder { unit: ours, order: Order::Sleep }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert!(waiting(&g), "asks again while the enemy is near");
        g.units.remove(&enemy);
        g.apply(0, Command::EndTurn).unwrap();
        assert!(!waiting(&g), "sleeps on once it's gone");
        assert_eq!(g.units[&ours].order, Order::Sleep);
    }

    #[test]
    fn targets_in_reach_wake_sleeping_units() {
        // An enemy worker can't strike, but our warrior can strike it.
        let (mut g, ours, enemy, tile) = duel("worker");
        g.apply(0, Command::SetOrder { unit: ours, order: Order::Sleep }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.is_alerted(&g.units[&ours]), "a target in reach");
        // Our sleeping worker next to it is left be.
        g.units.remove(&enemy);
        let spare = g.spawn_unit(1, g.rules.unit_type_id("worker").unwrap(), tile);
        g.units.get_mut(&ours).unwrap().kind = g.rules.unit_type_id("worker").unwrap();
        g.apply(0, Command::SetOrder { unit: ours, order: Order::Sleep }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert!(!g.is_alerted(&g.units[&ours]), "neither can strike");
        assert!(g.units.contains_key(&spare));
    }

    #[test]
    fn covered_leaders_stay_asleep() {
        let (mut g, ours, _, _) = duel("warrior");
        let at = g.units[&ours].tile;
        let general = g.spawn_unit(0, g.rules.unit_type_id("general").unwrap(), at);
        g.players[0].gold = 1000;
        g.apply(0, Command::SetOrder { unit: general, order: Order::Fortify }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert!(!g.is_alerted(&g.units[&general]), "the warrior beside it takes any attack");
        // Alone, the enemy could catch it.
        g.units.remove(&ours);
        g.apply(0, Command::SetOrder { unit: general, order: Order::Fortify }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.is_alerted(&g.units[&general]), "exposed");
    }

    #[test]
    fn far_or_peaceful_enemies_leave_sleepers_be() {
        let (mut g, ours, enemy, _) = duel("warrior");
        g.apply(0, Command::SetOrder { unit: ours, order: Order::Sleep }).unwrap();
        g.players[0].relations[1].at_war = false;
        g.players[1].relations[0].at_war = false;
        g.apply(0, Command::EndTurn).unwrap();
        assert!(!g.is_alerted(&g.units[&ours]), "not at war");
        g.players[0].relations[1].at_war = true;
        g.players[1].relations[0].at_war = true;
        let at = g.units[&ours].tile;
        let far = g.map.grid.within(at, 6).into_iter().find(|&t| g.map.grid.distance(at, t) == 6).unwrap();
        g.units.get_mut(&enemy).unwrap().tile = far;
        g.players[0].visible[far as usize] += 1;
        g.apply(0, Command::EndTurn).unwrap();
        assert!(!g.is_alerted(&g.units[&ours]), "out of reach");
    }

    #[test]
    fn attacks_wake_sleeping_units() {
        let (mut g, ours, enemy, _) = duel("warrior");
        g.apply(0, Command::SetOrder { unit: ours, order: Order::Sleep }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        g.apply(0, Command::SetOrder { unit: ours, order: Order::Sleep }).unwrap();
        let at = g.units[&ours].tile;
        g.update_visibility(1);
        g.apply(1, Command::Attack { unit: enemy, tile: at }).unwrap();
        assert!(g.units.contains_key(&ours), "survives");
        // The attacker is gone by our turn, but the attack is remembered.
        g.units.remove(&enemy);
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.units_needing_orders(0).any(|u| u.id == ours));
    }

    #[test]
    fn hunters_chase_their_target_down() {
        let (mut g, ours, enemy, _) = duel("warrior");
        g.units.remove(&enemy);
        let at = g.units[&ours].tile;
        // A worker a few tiles off, where the warrior can get to.
        let far = g
            .map
            .grid
            .within(at, 3)
            .into_iter()
            .find(|&t| g.map.grid.distance(at, t) == 3 && g.path_for(ours, t).is_some_and(|p| p.arrival_turn() <= 3))
            .expect("reachable tile");
        let worker = g.spawn_unit(1, g.rules.unit_type_id("worker").unwrap(), far);
        g.players[0].visible[far as usize] += 1;
        g.apply(0, Command::Hunt { unit: ours, target: worker }).unwrap();
        assert_eq!(g.units[&ours].order, Order::Hunt { target: worker, last: far });
        assert!(g.map.grid.distance(g.units[&ours].tile, far) < 3, "sets off at once");
        let caught = |g: &Game| g.units.get(&worker).is_none_or(|w| w.owner == 0);
        for _ in 0..6 {
            if caught(&g) {
                break;
            }
            g.apply(0, Command::EndTurn).unwrap();
        }
        assert!(caught(&g), "the worker is captured");
        assert_eq!(g.units[&ours].order, Order::None, "the hunt is over");
    }

    #[test]
    fn hunting_needs_war_and_a_fighter() {
        let (mut g, ours, enemy, _) = duel("warrior");
        let worker = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).id == "worker").map(|u| u.id);
        if let Some(worker) = worker {
            assert!(g.apply(0, Command::Hunt { unit: worker, target: enemy }).is_err());
        }
        assert!(g.apply(0, Command::Hunt { unit: ours, target: ours }).is_err());
        g.relation_mut(0, 1).at_war = false;
        g.relation_mut(1, 0).at_war = false;
        assert_eq!(g.apply(0, Command::Hunt { unit: ours, target: enemy }).unwrap_err(), CommandError::NotAtWar);
    }

    #[test]
    fn equal_units_trade_equal_damage() {
        let (mut g, ours, enemy, tile) = duel("warrior");
        let p = g.combat_preview(ours, tile).unwrap();
        assert_eq!(p.damage_dealt, 30);
        assert_eq!(p.damage_taken, 30);
        g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        assert_eq!(g.units[&enemy].hp, 70);
        assert_eq!(g.units[&ours].hp, 70);
        assert_eq!(g.units[&ours].moves, 0, "attacking ends the turn");
    }

    #[test]
    fn fortified_defenders_are_stronger() {
        let (mut g, ours, enemy, tile) = duel("warrior");
        let open = g.combat_preview(ours, tile).unwrap();
        g.units.get_mut(&enemy).unwrap().order = Order::Fortify;
        let dug_in = g.combat_preview(ours, tile).unwrap();
        assert!(dug_in.damage_dealt < open.damage_dealt);
        assert!(dug_in.damage_taken > open.damage_taken);
    }

    #[test]
    fn killing_moves_in_and_civilians_are_captured() {
        let (mut g, ours, enemy, tile) = duel("warrior");
        g.units.get_mut(&enemy).unwrap().hp = 10;
        let worker = g.spawn_unit(1, g.rules.unit_type_id("worker").unwrap(), tile);
        let events = g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        assert!(!g.units.contains_key(&enemy));
        assert!(!g.units.contains_key(&worker));
        assert_eq!(g.units[&ours].tile, tile, "the attacker advances");
        assert!(g.units_at(tile).any(|u| u.owner == 0 && g.unit_def(u).id == "worker"), "worker captured");
        assert!(events.iter().any(|e| matches!(e, Event::UnitCaptured { from: 1, .. })));
    }

    #[test]
    fn ranged_attacks_take_no_damage() {
        let (mut g, ours, _enemy, tile) = duel("warrior");
        let archer = g.rules.unit_type_id("archer").unwrap();
        g.units.get_mut(&ours).unwrap().kind = archer;
        let p = g.combat_preview(ours, tile).unwrap();
        assert!(p.ranged);
        assert_eq!(p.damage_taken, 0);
    }

    #[test]
    fn cities_fall_at_zero_hp() {
        let (mut g, ours, enemy, tile) = duel("settler");
        // Turn the enemy settler into a city next to our warrior.
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        g.update_visibility(0);
        let city = g.city_at(tile).unwrap().id;
        g.cities.get_mut(&city).unwrap().hp = 5;
        g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        assert_eq!(g.cities[&city].owner, 0, "captured");
        assert_eq!(g.units[&ours].tile, tile);
    }

    #[test]
    fn a_city_taken_this_turn_can_be_razed() {
        let (mut g, ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        g.update_visibility(0);
        let city = g.city_at(tile).unwrap().id;
        g.cities.get_mut(&city).unwrap().hp = 5;
        g.cities.get_mut(&city).unwrap().capital_of = None;
        g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        assert_eq!(g.cities[&city].captured, Some((g.turn, 1)));
        // Original capitals stand.
        g.cities.get_mut(&city).unwrap().capital_of = Some(1);
        assert!(g.apply(0, Command::RazeCity { city }).is_err());
        g.cities.get_mut(&city).unwrap().capital_of = None;
        let land = g.district_tiles(city);
        let events = g.apply(0, Command::RazeCity { city }).unwrap();
        assert!(!g.cities.contains_key(&city));
        assert!(land.iter().all(|&t| g.territory[t as usize].is_none()), "its land is nobody's");
        assert!(events.iter().any(|e| matches!(e, Event::CityRazed { by: 0, from: 1, .. })));
        assert_eq!(g.units[&ours].tile, tile, "the captor stays on the ruins");
    }

    #[test]
    fn captured_cities_are_kept_once_the_turn_ends() {
        let (mut g, ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        g.update_visibility(0);
        let city = g.city_at(tile).unwrap().id;
        g.cities.get_mut(&city).unwrap().hp = 5;
        g.cities.get_mut(&city).unwrap().capital_of = None;
        g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.apply(0, Command::RazeCity { city }).is_err());
        assert!(g.cities.contains_key(&city));
    }

    #[test]
    fn a_garrison_strengthens_its_city_and_is_pushed_out_when_it_falls() {
        let (mut g, ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let city = g.city_at(tile).unwrap().id;
        let bare = g.combat_preview(ours, tile).unwrap();
        let guard = spawn(&mut g, 1, "spearman", tile);
        assert_eq!(g.garrison(city), Some(guard));
        assert_eq!(g.city_strength(city), g.city_own_strength(city) + 14 * GARRISON_PERCENT / 100);
        let held = g.combat_preview(ours, tile).unwrap();
        assert!(held.defense > bare.defense);
        // The garrison takes no damage; the city falls at 0 all the same.
        g.cities.get_mut(&city).unwrap().hp = 5;
        g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        assert_eq!(g.cities[&city].owner, 0);
        let fled = &g.units[&guard];
        assert!(fled.tile != tile && fled.hp == MAX_HP, "pushed out, unhurt");
    }

    #[test]
    fn ranged_units_batter_the_walls_but_only_melee_takes_the_city() {
        let (mut g, ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let city = g.city_at(tile).unwrap().id;
        let at = g.map.grid.neighbors(tile).find(|&t| g.can_stop_at(&g.units[&ours], t) && g.map.land_move_cost(&g.rules, t).is_some()).unwrap();
        let archer = spawn(&mut g, 0, "archer", at);
        g.update_visibility(0);
        g.cities.get_mut(&city).unwrap().hp = 3;
        let p = g.combat_preview(archer, tile).unwrap();
        assert!(p.ranged && !p.captures && p.damage_dealt == 3);
        g.apply(0, Command::Attack { unit: archer, tile }).unwrap();
        assert_eq!(g.cities[&city].hp, 0);
        assert_eq!(g.cities[&city].owner, 1, "the archer can't take it");
        // The warrior walks in without a fight.
        let p = g.combat_preview(ours, tile).unwrap();
        assert!(p.captures && p.damage_taken == 0);
        g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        assert_eq!(g.cities[&city].owner, 0);
    }

    #[test]
    fn rubble_is_taken_from_next_door_not_shot_at() {
        let (mut g, _, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let city = g.city_at(tile).unwrap().id;
        g.cities.get_mut(&city).unwrap().hp = 0;
        let far = g.map.grid.within(tile, 2).into_iter().find(|&t| g.map.grid.distance(t, tile) == 2 && g.map.land_move_cost(&g.rules, t).is_some()).unwrap();
        let archer = spawn(&mut g, 0, "archer", far);
        g.update_visibility(0);
        // Shooting rubble would only spend the archer's turn.
        assert!(g.combat_preview(archer, tile).is_err());
        assert!(g.apply(0, Command::Attack { unit: archer, tile }).is_err());
        assert_eq!(g.units[&archer].moves, crate::path::points(g.unit_def(&g.units[&archer]).moves));
        assert_eq!(g.cities[&city].owner, 1);
    }

    #[test]
    fn cities_fire_without_walls_and_repair_less_under_siege() {
        let (mut g, ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let city = g.city_at(tile).unwrap().id;
        assert!(g.city_walls(city) == 0 && g.city_range(city) == 1);
        g.cities.get_mut(&city).unwrap().hp = 50;
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.units.get(&ours).is_none_or(|u| u.hp < MAX_HP), "it fired at the warrior next door");
        let hp = g.cities[&city].hp;
        assert!(hp > 50 && hp < 50 + CITY_REPAIR, "repairs a little while besieged: {hp}");
    }

    #[test]
    fn a_falling_citys_other_units_are_pushed_out_or_lost() {
        let (mut g, ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let city = g.city_at(tile).unwrap().id;
        // A General doesn't defend the city: it flees when it falls.
        let guard = spawn(&mut g, 1, "general", tile);
        g.cities.get_mut(&city).unwrap().hp = 0;
        let mut g2 = g.clone();
        g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        assert_eq!(g.cities[&city].owner, 0);
        let fled = &g.units[&guard];
        assert!(fled.owner == 1 && fled.tile != tile && g.map.grid.distance(fled.tile, tile) <= FLEE_RANGE, "it got out");
        // Hemmed in on every side, it's lost with the city.
        let at = g2.units[&ours].tile;
        let around: Vec<TileId> = g2.map.grid.within(tile, FLEE_RANGE).into_iter().filter(|&t| t != tile && t != at).collect();
        for t in around {
            let id = g2.spawn_unit(0, g2.rules.unit_type_id("warrior").unwrap(), t);
            g2.units.get_mut(&id).unwrap().moves = 0;
        }
        let events = g2.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        assert!(!g2.units.contains_key(&guard));
        assert!(events.iter().any(|e| matches!(e, Event::LostWithCity { owner: 1, .. })));
    }

    #[test]
    fn players_at_peace_cant_fight() {
        let (mut g, ours, _enemy, tile) = duel("warrior");
        for (a, b) in [(0, 1), (1, 0)] {
            g.relation_mut(a, b).at_war = false;
        }
        assert_eq!(g.combat_preview(ours, tile).unwrap_err(), CommandError::NotAtWar);
        assert_eq!(g.peace_blocks_attack(ours, tile), Some(1));
        assert!(g.combat_preview_at_war(ours, tile).is_ok());
        assert!(!g.in_enemy_zoc(&g.units[&ours].clone(), tile), "no zone of control at peace");
        assert_eq!(g.apply(0, Command::DeclareWar { on: 1 }).unwrap(), vec![Event::WarDeclared { by: 0, on: 1 }]);
        assert!(g.at_war(1, 0), "war goes both ways");
        assert!(g.combat_preview(ours, tile).is_ok());
        assert_eq!(g.apply(0, Command::DeclareWar { on: 1 }).unwrap_err(), CommandError::AlreadyAtWar);
    }

    #[test]
    fn attacks_are_remembered() {
        let (mut g, ours, _enemy, tile) = duel("warrior");
        g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        let theirs = g.relation(1, 0);
        assert_eq!((theirs.attacked_us, theirs.last_attacked_us), (1, Some(g.turn)));
        assert_eq!(g.relation(0, 1).we_attacked, 1);
    }

    #[test]
    fn cities_under_attack_dont_repair() {
        let (mut g, ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        g.update_visibility(0);
        let city = g.city_at(tile).unwrap().id;
        let archer = g.rules.unit_type_id("archer").unwrap();
        g.units.get_mut(&ours).unwrap().kind = archer;
        g.apply(0, Command::Attack { unit: ours, tile }).unwrap();
        let hp = g.cities[&city].hp;
        assert!(hp < CITY_MAX_HP);
        g.apply(0, Command::EndTurn).unwrap();
        assert_eq!(g.cities[&city].hp, hp, "attacked this turn");
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.cities[&city].hp > hp, "repairs once left alone");
    }

    #[test]
    fn enemies_exert_zones_of_control() {
        let (mut g, ours, _enemy, tile) = duel("warrior");
        let at = g.units[&ours].tile;
        // A tile two steps away that passes next to the enemy.
        let beyond = g
            .map
            .grid
            .neighbors(tile)
            .find(|&t| t != at && g.map.grid.distance(t, at) == 2 && g.map.land_move_cost(&g.rules, t).is_some())
            .unwrap();
        g.units.get_mut(&ours).unwrap().moves = crate::path::points(5);
        g.apply(0, Command::MoveUnit { unit: ours, to: beyond }).unwrap();
        let u = &g.units[&ours];
        assert_eq!(u.moves, 0, "stopped by the zone of control");
    }

    #[test]
    fn walled_cities_shoot_nearby_enemies() {
        let (mut g, ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let city = g.city_at(tile).unwrap().id;
        let walls = g.rules.building_id("walls").unwrap();
        g.cities.get_mut(&city).unwrap().buildings.push(walls);
        g.update_visibility(1);
        let events = g.apply(0, Command::EndTurn).unwrap();
        let shot = events.iter().find_map(|e| match e {
            Event::Combat(r) if r.attacker == Attacker::City(city) => Some(r.clone()),
            _ => None,
        });
        let shot = shot.expect("the city fired");
        assert_eq!(shot.tile, g.units[&ours].tile);
        assert!(shot.damage_dealt > 0 && shot.damage_dealt <= STRIKE_MAX_DAMAGE);
    }

    #[test]
    fn city_shots_never_kill_a_healthy_unit() {
        let (mut g, ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let city = g.city_at(tile).unwrap().id;
        let walls = g.rules.building_id("walls").unwrap();
        let c = g.cities.get_mut(&city).unwrap();
        c.buildings.push(walls);
        c.population = 30;
        // A weak unit next to a huge walled city.
        g.units.get_mut(&ours).unwrap().kind = g.rules.unit_type_id("catapult").unwrap();
        g.update_visibility(1);
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.units.contains_key(&ours), "wounded, not destroyed");
    }

    #[test]
    fn wounded_units_heal_when_resting() {
        let (mut g, ours, _, _) = duel("warrior");
        g.units.get_mut(&ours).unwrap().hp = 50;
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.units[&ours].hp > 50);
    }

    /// A two-player game at war with a shore: a land tile and two coast
    /// tiles next to it and each other, away from everyone.
    fn shore() -> (Game, TileId, TileId, TileId) {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 1);
        let coast = g.rules.terrain_id("coast").unwrap();
        let is_coast = |g: &Game, t: TileId| g.map.tile(t).terrain == coast && g.sea_ice(t) == crate::climate::SeaIce::Open;
        let far = |g: &Game, t: TileId| g.units.values().all(|u| g.map.grid.distance(u.tile, t) > 4);
        let (land, w1, w2) = (0..g.map.tiles.len() as TileId)
            .filter(|&t| g.map.land_move_cost(&g.rules, t) == Some(1) && far(&g, t))
            .find_map(|t| {
                let waters: Vec<TileId> = g.map.grid.neighbors(t).filter(|&n| is_coast(&g, n)).collect();
                waters.iter().find_map(|&a| waters.iter().find(|&&b| g.map.grid.distance(a, b) == 1).map(|&b| (t, a, b)))
            })
            .expect("a shore");
        g.players[0].met[1] = true;
        g.players[1].met[0] = true;
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        (g, land, w1, w2)
    }

    fn spawn(g: &mut Game, owner: PlayerId, kind: &str, tile: TileId) -> UnitId {
        let id = g.spawn_unit(owner, g.rules.unit_type_id(kind).unwrap(), tile);
        g.update_visibility(owner);
        id
    }

    #[test]
    fn galleys_sink_embarked_units() {
        let (mut g, _, w1, w2) = shore();
        let galley = spawn(&mut g, 0, "galley", w1);
        let swimmer = spawn(&mut g, 1, "spearman", w2);
        let p = g.combat_preview(galley, w2).unwrap();
        assert!(p.defense_mods.iter().any(|m| m.label == "Embarked"));
        g.apply(0, Command::Attack { unit: galley, tile: w2 }).unwrap();
        if !g.units.contains_key(&swimmer) {
            assert_eq!(g.units[&galley].tile, w2, "the galley sails in");
        }
        // Embarked units can't strike back.
        let rower = spawn(&mut g, 1, "warrior", w1);
        let target = g.units[&galley].tile;
        assert_eq!(g.attack_target(rower, target), Err(CommandError::CantAttack("can't attack while embarked")));
    }

    #[test]
    fn land_units_cant_attack_ships_but_ships_can_raid_the_shore() {
        let (mut g, land, w1, _) = shore();
        let warrior = spawn(&mut g, 0, "warrior", land);
        let galley = spawn(&mut g, 1, "galley", w1);
        assert_eq!(g.attack_target(warrior, w1), Err(CommandError::CantAttack("can't reach that tile")));
        assert_eq!(g.attack_target(galley, land), Ok(Target::Unit(warrior)));
        g.apply(1, Command::Attack { unit: galley, tile: land }).unwrap();
        if let Some(gal) = g.units.get(&galley) {
            assert_eq!(gal.tile, w1, "ships never land");
        }
    }

    #[test]
    fn ranged_units_bring_walls_down_from_afar() {
        let (mut g, _ours, enemy, tile) = duel("settler");
        g.apply(1, Command::FoundCity { unit: enemy }).unwrap();
        let city = g.city_at(tile).unwrap().id;
        g.cities.get_mut(&city).unwrap().hp = 5;
        let far = g.map.grid.within(tile, 2).into_iter().find(|&t| g.map.grid.distance(t, tile) == 2 && g.map.land_move_cost(&g.rules, t).is_some()).unwrap();
        let archer = spawn(&mut g, 0, "archer", far);
        g.apply(0, Command::Attack { unit: archer, tile }).unwrap();
        assert_eq!((g.cities[&city].owner, g.cities[&city].hp), (1, 0));
        assert_eq!(g.units[&archer].tile, far);
    }

    #[test]
    fn ships_take_coastal_cities() {
        let (mut g, land, w1, _) = shore();
        let settler = spawn(&mut g, 1, "settler", land);
        g.apply(1, Command::FoundCity { unit: settler }).unwrap();
        let city = g.city_at(land).unwrap().id;
        g.cities.get_mut(&city).unwrap().hp = 5;
        // A melee ship storms it; a ranged one only batters the walls.
        let caravel = spawn(&mut g, 0, "caravel", w1);
        g.apply(0, Command::Attack { unit: caravel, tile: land }).unwrap();
        assert_eq!(g.cities[&city].owner, 0, "captured");
        assert_eq!(g.units[&caravel].tile, land, "it sails into the harbour");
        // Half its walls stand.
        assert_eq!(g.cities[&city].hp, CITY_MAX_HP / 2);
    }

    #[test]
    fn units_sent_to_a_city_they_cant_enter_go_next_to_it() {
        let (mut g, land, w1, _) = shore();
        let settler = spawn(&mut g, 1, "settler", land);
        g.apply(1, Command::FoundCity { unit: settler }).unwrap();
        let frigate = spawn(&mut g, 0, "frigate", w1);
        assert_eq!(g.approach_city(frigate, land), None, "next to it already");
        let off = g.map.grid.within(land, 3).into_iter().find(|&t| g.map.grid.distance(t, land) == 3 && g.path_for(frigate, t).is_some()).expect("open sea");
        g.units.get_mut(&frigate).unwrap().tile = off;
        assert!(g.path_for(frigate, land).is_none(), "ships can't sail into foreign cities");
        let (to, path) = g.approach_city(frigate, land).expect("a way next to it");
        assert_eq!(g.map.grid.distance(to, land), 1);
        assert_eq!(path.tiles.last(), Some(&to));
    }

    #[test]
    fn ships_need_a_coastal_city() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 1);
        g.players[0].techs.fill(true);
        // The best ship there is (older ones are obsolete).
        let galley = Build::Unit(g.rules.unit_type_id("ship_of_the_line").unwrap());
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        // Found one city inland and one on the coast.
        let site = |g: &Game, coastal: bool| {
            (0..g.map.tiles.len() as TileId)
                .find(|&t| g.found_city_problem(0, t).is_none() && g.is_coastal(t) == coastal && g.map.land_move_cost(&g.rules, t).is_some())
                .unwrap()
        };
        let inland = site(&g, false);
        g.units.get_mut(&settler).unwrap().tile = inland;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let s2 = g.spawn_unit(0, g.rules.unit_type_id("settler").unwrap(), site(&g, true));
        g.apply(0, Command::FoundCity { unit: s2 }).unwrap();
        let city = |g: &Game, coastal: bool| g.cities.values().find(|c| g.is_coastal(c.tile) == coastal).unwrap().id;
        let (dry, port) = (city(&g, false), city(&g, true));
        assert!(!g.builds_here(dry, galley));
        assert!(g.apply(0, Command::SetQueue { city: dry, queue: vec![galley] }).is_err());
        assert!(g.builds_here(port, galley));
        g.apply(0, Command::SetQueue { city: port, queue: vec![galley] }).unwrap();
        g.cities.get_mut(&port).unwrap().production = 1000;
        g.players[0].gold = 1000;
        for _ in 0..2 {
            g.apply(0, Command::EndTurn).unwrap();
        }
        let ship = g.units.values().find(|u| g.unit_def(u).id == "ship_of_the_line").expect("launched");
        let tile = g.cities[&port].tile;
        assert!(ship.tile == tile || g.map.is_water(&g.rules, ship.tile));
    }

    #[test]
    fn sailing_lets_land_units_cross_water() {
        let (mut g, land, w1, _) = shore();
        let warrior = spawn(&mut g, 0, "warrior", land);
        assert!(g.path_for(warrior, w1).is_none());
        let sailing = g.rules.tech_id("sailing").unwrap();
        g.players[0].techs[sailing as usize] = true;
        let path = g.path_for(warrior, w1).unwrap();
        assert_eq!(path.tiles, vec![w1]);
        g.apply(0, Command::MoveUnit { unit: warrior, to: w1 }).unwrap();
        let u = &g.units[&warrior];
        assert_eq!((u.tile, u.moves), (w1, 0), "embarking takes the rest of the turn");
        assert!(g.is_embarked(u));
    }
}
