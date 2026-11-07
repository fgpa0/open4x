//! Experience (`data/experience.ron`). Military units earn XP in battle
//! ([`Unit::xp`]) and rise through ranks on their own: each rank is a
//! strength bonus in every fight (a [`crate::combat::Modifier`] named after
//! it) and extra healing. XP stays with the unit when it's upgraded, and
//! stops at the last rank.
//!
//! **Generals** (and Admirals at sea) lead the units around them: every unit
//! of theirs of the same domain within the `aura` range fights a little
//! better ([`AuraRules::strength`]) and gets the leader's traits. Leaders
//! earn XP from every fight of a unit they lead, and each level lets them
//! choose a trait (`Command::ChooseTrait`): movement, sight, combat
//! bonuses, healing, more XP. A choice not made by the end of the turn is
//! made for them ([`Game::auto_traits`], the first trait in the list they
//! lack), like a city governor's.

use crate::civ::CombatCond;
use crate::game::{CommandError, Event, Game, PlayerId, Unit, UnitId};
use crate::rules::{Ability, Domain, RulesError, UnitClass};
use serde::{Deserialize, Serialize};

pub type TraitId = u8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExperienceRules {
    pub gain: XpGain,
    /// In order of the XP they need.
    pub ranks: Vec<RankDef>,
    /// The XP a General needs for each level.
    pub general_levels: Vec<u32>,
    pub aura: AuraRules,
    pub traits: Vec<TraitDef>,
}

/// Who a General leads, and how much better they fight for it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AuraRules {
    /// Tiles from the General.
    pub range: i32,
    /// Strength bonus, percent.
    pub strength: i32,
}

/// XP earned in one fight.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct XpGain {
    pub melee: u32,
    pub ranged: u32,
    pub defend: u32,
    pub kill: u32,
    pub capture: u32,
    pub general: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RankDef {
    pub name: String,
    pub xp: u32,
    /// Strength bonus, percent.
    pub strength: i32,
    /// Extra hit points healed a turn.
    #[serde(default)]
    pub heal: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraitDef {
    pub id: String,
    /// Only for Generals (`Land`) or Admirals (`Sea`); `None`: both.
    #[serde(default)]
    pub domain: Option<Domain>,
    pub name: String,
    pub effect: TraitEffect,
    #[serde(default)]
    pub description: String,
}

/// What a General's trait does for the units it leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraitEffect {
    Moves(i32),
    Sight(i32),
    Combat { percent: i32, when: CombatCond },
    /// Extra healing a turn outside cities.
    Heal(i32),
    /// Percent more XP for the members.
    Experience(i32),
}

impl ExperienceRules {
    pub(crate) fn validate(&self) -> Result<(), RulesError> {
        let invalid = |m: &str| Err(RulesError::Invalid(format!("experience.ron: {m}")));
        if self.ranks.windows(2).any(|w| w[0].xp >= w[1].xp) || self.ranks.first().is_some_and(|r| r.xp == 0) {
            return invalid("ranks must need more XP each, and some");
        }
        if self.general_levels.windows(2).any(|w| w[0] >= w[1]) || self.general_levels.first() == Some(&0) {
            return invalid("general levels must need more XP each, and some");
        }
        for d in [Domain::Land, Domain::Sea] {
            if self.traits.iter().filter(|t| t.domain.is_none_or(|t| t == d)).count() < self.general_levels.len() {
                return invalid("there must be a trait for every level, for Generals and for Admirals");
            }
        }
        if self.aura.range < 0 {
            return invalid("the aura's range can't be negative");
        }
        if self.traits.len() > u8::MAX as usize {
            return invalid("too many traits");
        }
        for (i, t) in self.traits.iter().enumerate() {
            if self.traits[..i].iter().any(|o| o.id == t.id) {
                return invalid(&format!("trait '{}' is listed twice", t.id));
            }
        }
        Ok(())
    }

    pub fn trait_def(&self, id: TraitId) -> &TraitDef {
        &self.traits[id as usize]
    }

    pub fn trait_id(&self, id: &str) -> Option<TraitId> {
        self.traits.iter().position(|t| t.id == id).map(|i| i as TraitId)
    }

    /// The rank (1 = the first in the list; 0 = none yet) at `xp`.
    pub fn rank_at(&self, xp: u32) -> usize {
        self.ranks.iter().take_while(|r| xp >= r.xp).count()
    }

    /// A General's level at `xp`.
    pub fn level_at(&self, xp: u32) -> usize {
        self.general_levels.iter().take_while(|&&l| xp >= l).count()
    }
}

impl Game {
    fn xp_rules(&self) -> &ExperienceRules {
        &self.rules.experience
    }

    fn is_general(&self, u: &Unit) -> bool {
        self.unit_def(u).has(Ability::General)
    }

    /// The unit's rank (0: none), see [`ExperienceRules::rank_at`]. Generals have levels instead.
    pub fn rank(&self, u: &Unit) -> usize {
        if self.is_general(u) { 0 } else { self.xp_rules().rank_at(u.xp) }
    }

    pub fn rank_def(&self, u: &Unit) -> Option<&RankDef> {
        self.rank(u).checked_sub(1).map(|r| &self.xp_rules().ranks[r])
    }

    /// The XP the unit's next rank or level needs (`None`: it has the last).
    pub fn next_xp(&self, u: &Unit) -> Option<u32> {
        let x = self.xp_rules();
        if self.is_general(u) {
            x.general_levels.get(x.level_at(u.xp)).copied()
        } else {
            x.ranks.get(x.rank_at(u.xp)).map(|r| r.xp)
        }
    }

    /// A General's level (0 for other units).
    pub fn general_level(&self, u: &Unit) -> usize {
        if self.is_general(u) { self.xp_rules().level_at(u.xp) } else { 0 }
    }

    /// Traits a General may still choose now (levels without one).
    pub fn pending_traits(&self, u: &Unit) -> usize {
        self.general_level(u).saturating_sub(u.traits.len())
    }

    /// Traits the General (or Admiral) doesn't have yet and may take, in the list's order.
    pub fn trait_choices(&self, u: &Unit) -> Vec<TraitId> {
        let domain = self.unit_def(u).domain;
        let x = self.xp_rules();
        (0..x.traits.len() as TraitId).filter(|&t| !u.traits.contains(&t) && x.trait_def(t).domain.is_none_or(|d| d == domain)).collect()
    }

    /// Why the General `unit` can't take `t` now, if it can't.
    pub fn trait_problem(&self, unit: UnitId, t: TraitId) -> Option<&'static str> {
        let u = self.units.get(&unit)?;
        if !self.is_general(u) {
            return Some("only Generals and Admirals have traits");
        }
        if t as usize >= self.xp_rules().traits.len() {
            return Some("no such trait");
        }
        if u.traits.contains(&t) {
            return Some("already has it");
        }
        if self.xp_rules().trait_def(t).domain.is_some_and(|d| d != self.unit_def(u).domain) {
            return Some(if self.unit_def(u).domain == Domain::Sea { "that trait is for Generals" } else { "that trait is for Admirals" });
        }
        if self.pending_traits(u) == 0 {
            return Some("needs more experience");
        }
        None
    }

    /// `Command::ChooseTrait`.
    pub(crate) fn choose_trait(&mut self, unit: UnitId, t: TraitId) -> Result<Vec<Event>, CommandError> {
        if let Some(why) = self.trait_problem(unit, t) {
            return Err(CommandError::CantPromote(why));
        }
        self.units.get_mut(&unit).expect("checked").traits.push(t);
        let owner = self.units[&unit].owner;
        let mut events = vec![Event::TraitChosen { unit, trait_id: t }];
        if matches!(self.xp_rules().trait_def(t).effect, TraitEffect::Sight(_)) {
            events.extend(self.update_visibility(owner));
        }
        Ok(events)
    }

    /// End of turn: every General with a level to spend takes the first
    /// trait in the list it lacks.
    pub(crate) fn auto_traits(&mut self) -> Vec<Event> {
        let generals: Vec<UnitId> = self.units.values().filter(|u| self.pending_traits(u) > 0).map(|u| u.id).collect();
        let mut events = Vec::new();
        for g in generals {
            while let Some(t) = self.units.get(&g).filter(|u| self.pending_traits(u) > 0).and_then(|u| self.trait_choices(u).first().copied()) {
                events.extend(self.choose_trait(g, t).unwrap_or_default());
            }
        }
        events
    }

    /// Gives a General (or an Admiral) without a name the next one from its civ's list.
    pub(crate) fn name_general(&mut self, unit: UnitId) {
        let Some(u) = self.units.get(&unit).filter(|u| u.name.is_none() && self.is_general(u)) else { return };
        let player = u.owner;
        let sea = self.unit_def(u).domain == Domain::Sea;
        let p = &mut self.players[player as usize];
        let counter = if sea { &mut p.admirals_named } else { &mut p.generals_named };
        let n = *counter as usize;
        *counter += 1;
        let civ = self.civ(player);
        let name = if sea { civ.admirals.get(n) } else { civ.generals.get(n) }
            .cloned()
            .unwrap_or_else(|| format!("{} {}", if sea { "Admiral" } else { "General" }, n + 1));
        self.units.get_mut(&unit).expect("exists").name = Some(name);
    }

    /// The General (or Admiral) leading `u`: the highest in level of its
    /// owner's within the aura's range, of the same domain (`None` for a
    /// General itself, or a civilian).
    pub fn leader_of(&self, u: &Unit) -> Option<UnitId> {
        let def = self.unit_def(u);
        if def.class != UnitClass::Military || def.has(Ability::General) {
            return None;
        }
        let range = self.xp_rules().aura.range;
        self.units
            .values()
            .filter(|g| g.owner == u.owner && self.is_general(g) && self.unit_def(g).domain == def.domain)
            .filter(|g| self.map.grid.distance(g.tile, u.tile) <= range)
            .max_by_key(|g| (g.xp, std::cmp::Reverse(g.id)))
            .map(|g| g.id)
    }

    /// The effects of the traits of the General leading `u`.
    pub fn leader_traits(&self, u: &Unit) -> Vec<(TraitId, TraitEffect)> {
        let Some(g) = self.leader_of(u) else { return Vec::new() };
        self.units[&g].traits.iter().map(|&t| (t, self.xp_rules().trait_def(t).effect)).collect()
    }

    fn trait_sum(&self, u: &Unit, f: impl Fn(TraitEffect) -> i32) -> i32 {
        self.leader_traits(u).into_iter().map(|(_, e)| f(e)).sum()
    }

    /// Extra movement from the leading General's traits.
    pub fn aura_moves(&self, u: &Unit) -> i32 {
        self.trait_sum(u, |e| if let TraitEffect::Moves(n) = e { n } else { 0 })
    }

    /// The unit's sight on flat land (its type's plus the General's traits).
    pub fn unit_sight(&self, u: &Unit) -> i32 {
        self.unit_def(u).sight + self.trait_sum(u, |e| if let TraitEffect::Sight(n) = e { n } else { 0 })
    }

    /// Extra healing a turn from rank and traits (`in_city`: trait healing
    /// only counts outside cities).
    pub(crate) fn xp_heal(&self, u: &Unit, in_city: bool) -> i32 {
        let rank = self.rank_def(u).map_or(0, |r| r.heal);
        let field = if in_city { 0 } else { self.trait_sum(u, |e| if let TraitEffect::Heal(n) = e { n } else { 0 }) };
        rank + field
    }

    /// Gives `unit` `amount` XP (more with a Drillmaster General, up to its
    /// last rank or level); announces a new rank or level.
    pub(crate) fn gain_xp(&mut self, unit: UnitId, amount: u32) -> Vec<Event> {
        let Some(u) = self.units.get(&unit) else { return Vec::new() };
        let def = self.unit_def(u);
        if amount == 0 || def.class != UnitClass::Military {
            return Vec::new();
        }
        let x = self.xp_rules();
        let general = self.is_general(u);
        let (cap, before) = if general {
            (x.general_levels.last().copied().unwrap_or(0), x.level_at(u.xp))
        } else {
            (x.ranks.last().map_or(0, |r| r.xp), x.rank_at(u.xp))
        };
        let bonus = if general { 0 } else { self.trait_sum(u, |e| if let TraitEffect::Experience(p) = e { p } else { 0 }) };
        let gained = amount * (100 + bonus.max(-100) as u32) / 100;
        let xp = (u.xp + gained).min(cap);
        let after = if general { x.level_at(xp) } else { x.rank_at(xp) };
        self.units.get_mut(&unit).expect("exists").xp = xp;
        match (after > before, general) {
            (true, false) => vec![Event::Promoted { unit, rank: after as u8 }],
            (true, true) => vec![Event::GeneralPromoted { unit, level: after as u8 }],
            _ => Vec::new(),
        }
    }

    /// XP for one fight: the attacker (`ranged` or not) and the defending
    /// unit if it lived (`None` for a city), and the Generals leading either;
    /// `kill` whether the defender died and `captured` whether a city fell.
    pub(crate) fn battle_xp(&mut self, attacker: UnitId, ranged: bool, defender: Option<UnitId>, kill: bool, captured: bool) -> Vec<Event> {
        let g = self.xp_rules().gain;
        let bonus = if kill { g.kill } else { 0 } + if captured { g.capture } else { 0 };
        let leaders: Vec<Option<UnitId>> =
            [Some(attacker), defender].iter().map(|m| m.and_then(|m| self.units.get(&m)).and_then(|u| self.leader_of(u))).collect();
        let mut events = self.gain_xp(attacker, if ranged { g.ranged } else { g.melee } + bonus);
        if let Some(d) = defender {
            events.extend(self.gain_xp(d, g.defend));
        }
        if let Some(gen) = leaders[0] {
            events.extend(self.gain_xp(gen, g.general + bonus));
        }
        if let Some(gen) = leaders[1] {
            events.extend(self.gain_xp(gen, g.general));
        }
        events
    }
}

/// A player's Generals with a trait to choose.
pub fn generals_to_promote(game: &Game, player: PlayerId) -> Vec<UnitId> {
    let mut v: Vec<UnitId> = game.units.values().filter(|u| u.owner == player && game.pending_traits(u) > 0).map(|u| u.id).collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use crate::game::{Command, Game, Order};
    use crate::hex::TileId;
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Rules;

    fn land_pair(g: &Game) -> (TileId, TileId) {
        // Open ground with no bonuses, away from everyone's starting units.
        let land = |t: TileId| {
            g.map.land_move_cost(&g.rules, t) == Some(1)
                && g.map.defense_bonus(&g.rules, t) == 0
                && g.map.tile(t).feature.is_none()
                && g.units.values().all(|u| g.map.grid.distance(u.tile, t) > 3)
        };
        for t in 0..g.map.tiles.len() as TileId {
            if land(t) {
                if let Some(n) = g.map.grid.neighbors(t).find(|&n| land(n)) {
                    return (t, n);
                }
            }
        }
        panic!("no open land");
    }

    fn game() -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        Game::new(rules, map, 2)
    }

    fn spawn(g: &mut Game, owner: u8, kind: &str, tile: TileId) -> u32 {
        let k = g.rules.unit_type_id(kind).unwrap();
        g.spawn_unit(owner, k, tile)
    }

    #[test]
    fn the_shipped_rules_load() {
        let g = game();
        let x = &g.rules.experience;
        assert!(!x.ranks.is_empty() && x.traits.len() >= x.general_levels.len());
        assert_eq!(x.rank_at(0), 0);
        assert_eq!(x.rank_at(x.ranks[0].xp), 1);
    }

    #[test]
    fn fighting_earns_xp_and_ranks_add_strength() {
        let mut g = game();
        let (a, b) = land_pair(&g);
        let ours = spawn(&mut g, 0, "warrior", a);
        let theirs = spawn(&mut g, 1, "warrior", b);
        g.update_visibility(0);
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        let before = g.combat_preview(ours, b).unwrap();
        g.apply(0, Command::Attack { unit: ours, tile: b }).unwrap();
        let gain = g.rules.experience.gain;
        assert_eq!(g.units[&ours].xp, gain.melee);
        assert_eq!(g.units[&theirs].xp, gain.defend, "the defender learns too");
        // A veteran hits harder.
        g.units.get_mut(&ours).unwrap().xp = g.rules.experience.ranks[0].xp;
        for id in [ours, theirs] {
            let u = g.units.get_mut(&id).unwrap();
            u.hp = 100;
            u.moves = crate::path::points(2);
            u.attacks = 0;
        }
        g.units.get_mut(&theirs).unwrap().xp = 0;
        let after = g.combat_preview(ours, b).unwrap();
        assert!(after.attack > before.attack);
        assert!(after.attack_mods.iter().any(|m| m.label == g.rules.experience.ranks[0].name));
    }

    #[test]
    fn xp_stops_at_the_last_rank() {
        let mut g = game();
        let (a, _) = land_pair(&g);
        let ours = spawn(&mut g, 0, "warrior", a);
        let events = g.gain_xp(ours, 10_000);
        let last = g.rules.experience.ranks.len();
        assert_eq!(g.units[&ours].xp, g.rules.experience.ranks[last - 1].xp);
        assert!(matches!(events[..], [crate::game::Event::Promoted { rank, .. }] if rank as usize == last));
    }

    #[test]
    fn generals_lead_the_units_around_them_and_level_up() {
        let mut g = game();
        let (a, b) = land_pair(&g);
        let warrior = spawn(&mut g, 0, "warrior", a);
        assert_eq!(g.leader_of(&g.units[&warrior]), None);
        // A General may share the tile with a soldier.
        let general = spawn(&mut g, 0, "general", a);
        assert!(g.can_stop_at(&g.units[&general], a));
        assert_eq!(g.leader_of(&g.units[&warrior]), Some(general));
        assert_eq!(g.units[&general].name.as_deref(), Some(g.civ(0).generals[0].as_str()));
        // Out of range, it leads no one.
        let far = g.map.grid.within(a, 6).into_iter().find(|&t| g.map.grid.distance(t, a) == 4 && g.map.land_move_cost(&g.rules, t).is_some()).unwrap();
        g.units.get_mut(&general).unwrap().tile = far;
        assert_eq!(g.leader_of(&g.units[&warrior]), None);
        g.units.get_mut(&general).unwrap().tile = b;
        let march = g.rules.experience.trait_id("forced_march").unwrap();
        assert!(g.apply(0, Command::ChooseTrait { unit: general, trait_id: march }).is_err(), "no level yet");
        let level = g.rules.experience.general_levels[0];
        g.gain_xp(general, level);
        assert_eq!(g.pending_traits(&g.units[&general]), 1);
        let moves = g.turn_moves(&g.units[&warrior]);
        g.apply(0, Command::ChooseTrait { unit: general, trait_id: march }).unwrap();
        assert_eq!(g.turn_moves(&g.units[&warrior]), moves + crate::path::MOVE);
        assert!(g.apply(0, Command::ChooseTrait { unit: general, trait_id: march }).is_err(), "once only");
        // A level not spent is spent at the end of the turn.
        g.gain_xp(general, g.rules.experience.general_levels[1]);
        g.units.get_mut(&warrior).unwrap().order = Order::Fortify;
        g.players[0].gold = 1000;
        g.apply(0, Command::EndTurn).unwrap();
        assert_eq!(g.units[&general].traits.len(), 2);
        assert_eq!(g.units[&general].traits[1], 1, "the first trait it lacked");
    }

    #[test]
    fn a_general_strengthens_the_units_it_leads_and_learns_from_their_fights() {
        let mut g = game();
        let (a, b) = land_pair(&g);
        let ours = spawn(&mut g, 0, "warrior", a);
        spawn(&mut g, 1, "warrior", b);
        g.update_visibility(0);
        g.apply(0, Command::DeclareWar { on: 1 }).unwrap();
        let alone = g.combat_preview(ours, b).unwrap();
        let general = spawn(&mut g, 0, "general", a);
        let led = g.combat_preview(ours, b).unwrap();
        assert!(led.attack > alone.attack);
        let name = g.units[&general].name.clone().unwrap();
        assert!(led.attack_mods.iter().any(|m| m.label == name && m.percent == g.rules.experience.aura.strength));
        g.apply(0, Command::Attack { unit: ours, tile: b }).unwrap();
        assert!(g.units[&general].xp >= g.rules.experience.gain.general);
    }

    #[test]
    fn admirals_lead_ships_with_naval_traits() {
        let mut g = game();
        let water = |t: TileId| g.map.is_water(&g.rules, t) && g.units.values().all(|u| g.map.grid.distance(u.tile, t) > 3);
        let sea = (0..g.map.tiles.len() as TileId).find(|&t| water(t) && g.map.grid.neighbors(t).all(|n| g.map.is_water(&g.rules, n))).unwrap();
        g.players[0].techs.fill(true);
        let galley = spawn(&mut g, 0, "galley", sea);
        let admiral = spawn(&mut g, 0, "admiral", sea);
        assert_eq!(g.leader_of(&g.units[&galley]), Some(admiral));
        let first = g.civ(0).admirals[0].clone();
        assert_eq!(g.units[&admiral].name.as_deref(), Some(first.as_str()));
        g.gain_xp(admiral, g.rules.experience.general_levels[0]);
        let x = &g.rules.experience;
        let (march, winds) = (x.trait_id("forced_march").unwrap(), x.trait_id("fair_winds").unwrap());
        assert!(!g.trait_choices(&g.units[&admiral]).contains(&march), "no forced marches at sea");
        assert!(g.apply(0, Command::ChooseTrait { unit: admiral, trait_id: march }).is_err());
        let moves = g.turn_moves(&g.units[&galley]);
        g.apply(0, Command::ChooseTrait { unit: admiral, trait_id: winds }).unwrap();
        assert_eq!(g.turn_moves(&g.units[&galley]), moves + crate::path::MOVE);
    }
}
