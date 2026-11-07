//! The computer player's view of the world each turn: how strong everyone is,
//! which of its cities are threatened, whom it fights, and whether it wants
//! to start a war. Wars come from the leader's drives and agenda (see
//! `data/civs.ron`): land hunger when boxed in, opportunism against weak
//! armies, grudges from being attacked, all held back early in the game by
//! patience and by the odds the leader's risk tolerance demands.
//!
//! Nothing is remembered between turns except what the simulation records
//! (who attacked whom), so the AI works the same after loading a save.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, HashSet};

use sim::city::CityId;
use sim::civ::{AgendaKind, Drives};
use sim::game::{Game, PlayerId, Unit};
use sim::hex::TileId;
use sim::improvement::Passage;
use sim::rules::UnitClass;

/// How close (in tiles) an enemy unit must be to a city to threaten it.
pub const THREAT_RADIUS: i32 = 3;
/// Players whose cities are this close, or no further than the nearest
/// rival's, are full neighbours; `NEIGHBOUR_FALLOFF` tiles further they don't
/// care about each other.
const NEIGHBOUR_DISTANCE: f32 = 20.0;
const NEIGHBOUR_FALLOFF: f32 = 16.0;
/// Rivals this far away are never neighbours, however empty the map.
const FARTHEST_NEIGHBOUR: f32 = 60.0;
/// War desire needed to start a war.
const WAR_DESIRE: f32 = 4.0;
/// The enemy city nearest our biggest army, if it's this close, is the one
/// we're marching on: it counts this much cheaper as a target.
const COMMITTED: i32 = 10;
const COMMITMENT: f32 = 60.0;
/// An enemy city across the water costs up to this much more as a target
/// when the enemy's navy is stronger than ours (our armies would sail past
/// it), by how much stronger.
const OVERSEAS_NAVY_COST: f32 = 45.0;
/// How long, in turns, an attack we made keeps a war going.
const MOMENTUM_TURNS: u32 = 12;
/// What passing through someone's land is worth, in gold, when it stands
/// between our army and the city it marches on...
const ARMY_PASSAGE: f32 = 150.0;
/// ...or between our explorers, who have run out of places to go, and
/// ground we've never seen.
const EXPLORE_PASSAGE: f32 = 40.0;
/// A city site worth settling (see [`crate::settle::site_value`]).
pub const GOOD_SITE: i32 = 50;

/// An offensive war.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct War {
    pub enemy: PlayerId,
    /// The city the army marches on.
    pub target: CityId,
    /// Raiders hunt units in enemy land rather than besiege the city.
    pub raid: bool,
}

pub struct Strategy {
    pub me: PlayerId,
    pub drives: Drives,
    /// The troops the leader favours.
    pub army: sim::civ::ArmyMix,
    pub agenda: AgendaKind,
    /// Army strength of every player.
    pub power: Vec<f32>,
    /// Players we're at war with (or about to be: see [`Strategy::war`]).
    pub hostile: Vec<bool>,
    pub war: Option<War>,
    /// Strength of the hostile units near each of our cities that has any.
    pub threats: BTreeMap<CityId, f32>,
    /// How many military units we want.
    pub want_military: usize,
    /// Good city sites left near our cities.
    pub free_sites: usize,
    /// Which area our land units can reach every tile in (`u16::MAX`:
    /// impassable for them); see [`landmasses`].
    pub land: Vec<u16>,
    /// Players we could march on: we've met them and know one of their
    /// cities on our landmass.
    pub rivals: Vec<PlayerId>,
    /// Distance from our nearest city to each player's nearest known city.
    distance: Vec<Option<i32>>,
    /// Tiles with other players' units or cities (as at the start of the turn).
    pub foreign: HashSet<TileId>,
    /// Cities whose tile or territory we've seen.
    known_cities: HashSet<CityId>,
    /// Tiles units were sent to this turn, so others pick different ones.
    pub claimed: RefCell<HashSet<TileId>>,
    /// What passing through each player's land is worth (see [`Strategy::passage`]).
    passage: OnceCell<Vec<f32>>,
}

/// Rough fighting value of a unit: its best strength, scaled by its health
/// the same way combat scales it.
pub fn unit_power(game: &Game, u: &Unit) -> f32 {
    let def = game.unit_def(u);
    if def.class != UnitClass::Military {
        return 0.0;
    }
    let rank = 1.0 + game.rank_def(u).map_or(0, |r| r.strength) as f32 / 100.0;
    def.strength.max(def.ranged) as f32 * rank * (0.5 + 0.5 * u.hp as f32 / sim::combat::MAX_HP as f32)
}

/// Damage a unit of strength `attack` deals to one of strength `defense`
/// (the simulation's formula).
pub fn expected_damage(attack: f32, defense: f32) -> f32 {
    (30.0 * (attack / defense.max(0.1)).powf(1.3)).clamp(1.0, 100.0)
}

/// A city's effective defensive strength, as an attacker would face it.
pub fn city_defense(game: &Game, city: CityId) -> f32 {
    let c = &game.cities[&city];
    let bonus = game.city_walls(city) + game.map.defense_bonus(&game.rules, c.tile);
    let hp = 0.5 + 0.5 * c.hp as f32 / sim::combat::CITY_MAX_HP as f32;
    game.city_strength(city) as f32 * (1.0 + bonus as f32 / 100.0) * hp
}

/// Labels the areas `me`'s land units can get around: they can travel
/// between tiles with the same label, over water too once they can embark
/// (and through tunnels and over bridges).
fn landmasses(game: &Game, me: PlayerId) -> Vec<u16> {
    let n = game.map.tiles.len();
    let mut label = vec![u16::MAX; n];
    let mut next = 0u16;
    let mut stack = Vec::new();
    let mover = sim::path::Mover {
        embark: game.grants(me, sim::rules::TechGrant::Embark),
        deep_water: game.grants(me, sim::rules::TechGrant::DeepWater),
        tunnels: game.passage_tiles(Passage::Tunnel),
        bridges: game.passage_tiles(Passage::Bridge),
        ..sim::path::Mover::new(game.rules.unit(game.rules.unit_type_id("warrior").unwrap_or(0)))
    };
    let open = |t: TileId| mover.can_enter(&game.map, &game.rules, t);
    for start in 0..n as TileId {
        if label[start as usize] != u16::MAX || !open(start) {
            continue;
        }
        label[start as usize] = next;
        stack.push(start);
        while let Some(t) = stack.pop() {
            for nb in game.map.grid.neighbors(t) {
                if label[nb as usize] == u16::MAX && open(nb) {
                    label[nb as usize] = next;
                    stack.push(nb);
                }
            }
        }
        next = next.saturating_add(1);
    }
    label
}

impl Strategy {
    pub fn assess(game: &Game, me: PlayerId) -> Strategy {
        let civ = game.civ(me);
        let mut s = Strategy {
            me,
            drives: civ.drives,
            army: civ.army,
            agenda: civ.agenda.kind,
            power: vec![0.0; game.players.len()],
            hostile: vec![false; game.players.len()],
            war: None,
            threats: BTreeMap::new(),
            want_military: 0,
            free_sites: 0,
            land: landmasses(game, me),
            rivals: Vec::new(),
            distance: Vec::new(),
            foreign: game
                .units
                .values()
                .filter(|u| u.owner != me)
                .map(|u| u.tile)
                .chain(game.cities.values().filter(|c| c.owner != me).map(|c| c.tile))
                .collect(),
            claimed: RefCell::new(HashSet::new()),
            passage: OnceCell::new(),
            known_cities: game
                .territory
                .iter()
                .zip(&game.players[me as usize].explored)
                .enumerate()
                .filter_map(|(t, (_, &seen))| game.district(t as TileId).filter(|_| seen))
                .collect(),
        };
        s.distance = (0..game.players.len() as PlayerId).map(|q| s.measure_distance(game, q)).collect();
        s.rivals = (0..game.players.len() as PlayerId)
            .filter(|&q| q != me && game.has_met(me, q))
            .filter(|&q| game.cities.values().any(|c| c.owner == q && s.knows_city(game, c) && s.on_our_land(game, c.tile)))
            .collect();
        for u in game.units.values() {
            s.power[u.owner as usize] += unit_power(game, u);
        }
        s.free_sites = s.count_free_sites(game);
        for q in 0..game.players.len() as PlayerId {
            s.hostile[q as usize] = game.at_war(me, q);
        }
        s.war = s.choose_war(game);
        if let Some(w) = s.war {
            s.hostile[w.enemy as usize] = true;
        }
        s.threats = s.find_threats(game);
        s.want_military = s.military_wanted(game);
        s
    }

    /// What open borders from `q` would be worth to us beyond the usual
    /// (0 if we may enter already or don't need to): their land is in the
    /// way of the city our army marches on, or our explorers have nowhere
    /// left to go and their land borders ground we've never seen. Worked out
    /// once per turn.
    pub fn passage(&self, game: &Game, q: PlayerId) -> f32 {
        self.passage.get_or_init(|| self.measure_passage(game))[q as usize]
    }

    fn measure_passage(&self, game: &Game) -> Vec<f32> {
        let me = self.me;
        let mut need = vec![0.0; game.players.len()];
        if let Some(w) = self.war {
            let target = game.cities[&w.target].tile;
            let land = self.land[target as usize];
            let soldier = game
                .units
                .values()
                .filter(|u| u.owner == me && crate::military::is_soldier(game, u) && self.land[u.tile as usize] == land)
                .min_by_key(|u| (game.map.grid.distance(u.tile, target), u.id));
            if let Some(u) = soldier {
                let next_to = game.map.grid.neighbors(target).filter(|&t| game.can_stop_at(u, t)).min_by_key(|&t| (game.map.grid.distance(u.tile, t), t));
                // The enemy's own borders open with the war.
                if let Some(q) = next_to.and_then(|t| game.borders_in_the_way(u.id, t)).filter(|&q| q != w.enemy) {
                    need[q as usize] += ARMY_PASSAGE;
                }
            }
        }
        // Explorers asleep: nothing they can reach is left to see.
        let stuck = game.units.values().any(|u| u.owner == me && u.order == sim::game::Order::Sleep && crate::military::is_scout(game, u.id));
        if stuck {
            let explored = &game.players[me as usize].explored;
            for (t, owner) in game.territory.iter().enumerate() {
                let Some(q) = *owner else { continue };
                let edge = explored[t] && game.map.grid.neighbors(t as TileId).any(|n| !explored[n as usize]);
                if q != me && need[q as usize] < EXPLORE_PASSAGE && edge {
                    need[q as usize] += EXPLORE_PASSAGE;
                }
            }
        }
        for (q, n) in need.iter_mut().enumerate() {
            if game.may_enter(me, q as PlayerId) || !game.has_met(me, q as PlayerId) {
                *n = 0.0;
            }
        }
        need
    }

    /// One line about the strategy and each rival, for balancing runs.
    pub fn report(&self, game: &Game) -> String {
        let me = self.me;
        let land = game.units.values().find(|u| u.owner == me).map(|u| self.land[u.tile as usize]);
        let mut out = format!(
            "land {:?} {:10} power {:5.0} units {:2}/{:2} sites {} war {:?}",
            land,
            game.players[me as usize].name,
            self.power[me as usize],
            game.military_units(me),
            self.want_military,
            self.free_sites,
            self.war.map(|w| (w.enemy, game.cities[&w.target].name.clone(), w.raid)),
        );
        let passage: Vec<String> =
            (0..game.players.len() as PlayerId).filter(|&q| self.passage(game, q) > 0.0).map(|q| format!("{q}:{:.0}", self.passage(game, q))).collect();
        if !passage.is_empty() {
            out += &format!(" passage {}", passage.join(","));
        }
        for &q in &self.rivals {
            let (desire, odds) = self.war_desire(game, q);
            let ratio = self.power[me as usize] / self.power[q as usize].max(5.0);
            out += &format!(
                " | {} d{} desire {:.1} odds {:.2}/{:.2}",
                q,
                self.city_distance(q).unwrap_or(-1),
                desire,
                ratio,
                odds
            );
        }
        out
    }

    fn d(&self, drive: u8) -> f32 {
        drive as f32 / 10.0
    }

    pub fn my_cities<'a>(&self, game: &'a Game) -> impl Iterator<Item = &'a sim::city::City> + 'a {
        let me = self.me;
        game.cities.values().filter(move |c| c.owner == me)
    }

    /// Distance between our nearest city and `q`'s cities we know of.
    fn city_distance(&self, q: PlayerId) -> Option<i32> {
        self.distance[q as usize]
    }

    fn measure_distance(&self, game: &Game, q: PlayerId) -> Option<i32> {
        let theirs: Vec<TileId> = game.cities.values().filter(|c| c.owner == q && self.knows_city(game, c)).map(|c| c.tile).collect();
        self.my_cities(game).flat_map(|c| theirs.iter().map(move |&t| game.map.grid.distance(c.tile, t))).min()
    }

    /// Whether we know where `city` is: we've seen it or its borders (closed
    /// borders keep explorers out, but not from seeing them), or its owner
    /// attacked us (raiders come from somewhere).
    pub fn knows_city(&self, game: &Game, city: &sim::city::City) -> bool {
        self.known_cities.contains(&city.id) || self.grudge(game, city.owner) > 0.0
    }

    /// Whether `tile` is on the same landmass as one of our cities.
    pub fn on_our_land(&self, game: &Game, tile: TileId) -> bool {
        let l = self.land[tile as usize];
        l != u16::MAX && self.my_cities(game).any(|c| self.land[c.tile as usize] == l)
    }

    /// How angry recent attacks by `q` make us (0: no grudge).
    pub fn grudge(&self, game: &Game, q: PlayerId) -> f32 {
        let r = game.relation(self.me, q);
        let Some(last) = r.last_attacked_us else { return 0.0 };
        // Grudgeful leaders remember longer; lost cities are never quite forgotten.
        let memory = 8.0 + self.drives.grudge as f32 * 6.0;
        let fresh = (1.0 - game.turn.saturating_sub(last) as f32 / memory).max(0.0);
        let lost = r.cities_lost as f32 * self.d(self.drives.grudge) * 3.0;
        if fresh <= 0.0 && lost <= 0.0 {
            return 0.0;
        }
        fresh * (2.0 + 6.0 * self.d(self.drives.grudge)) * (1.0 + (r.attacked_us.min(10) as f32) / 5.0) + lost
    }

    /// Whether we attacked `q` recently (a war we're already fighting).
    fn momentum(&self, game: &Game, q: PlayerId) -> bool {
        game.relation(self.me, q).last_we_attacked.is_some_and(|t| game.turn.saturating_sub(t) <= MOMENTUM_TURNS)
    }

    /// Whether part of our army is already out against `q`: outside our
    /// borders and nearer their cities than ours.
    fn deployed_against(&self, game: &Game, q: PlayerId) -> bool {
        let me = self.me;
        let theirs: Vec<TileId> = game.cities.values().filter(|c| c.owner == q).map(|c| c.tile).collect();
        let ours: Vec<TileId> = self.my_cities(game).map(|c| c.tile).collect();
        let nearest = |cities: &[TileId], t: TileId| cities.iter().map(|&c| game.map.grid.distance(c, t)).min().unwrap_or(i32::MAX);
        game.units
            .values()
            .filter(|u| u.owner == me && game.unit_def(u).class == UnitClass::Military)
            .filter(|u| game.territory_owner(u.tile) != Some(me) && nearest(&theirs, u.tile) < nearest(&ours, u.tile))
            .nth(1)
            .is_some()
    }

    /// Good unclaimed city sites near our cities (sites too close to each
    /// other to both be settled count once).
    fn count_free_sites(&self, game: &Game) -> usize {
        let explored = &game.players[self.me as usize].explored;
        let mut candidates: Vec<(i32, TileId)> = Vec::new();
        let mut seen = HashSet::new();
        for c in self.my_cities(game) {
            for t in game.map.grid.within(c.tile, 7) {
                if explored[t as usize] && seen.insert(t) {
                    if let Some(v) = crate::settle::site_value(game, self.me, t).filter(|&v| v >= GOOD_SITE) {
                        candidates.push((v, t));
                    }
                }
            }
        }
        candidates.sort_by(|a, b| b.cmp(a));
        let mut picked: Vec<TileId> = Vec::new();
        for (_, t) in candidates {
            if picked.iter().all(|&o| game.map.grid.distance(o, t) >= sim::city::MIN_CITY_DISTANCE) {
                picked.push(t);
            }
        }
        picked.len()
    }

    /// How much we'd like to attack `q` now, and the odds (our army over
    /// theirs) we'd need to actually do it.
    fn war_desire(&self, game: &Game, q: PlayerId) -> (f32, f32) {
        let me = self.me;
        let dr = &self.drives;
        let Some(dist) = self.city_distance(q) else { return (0.0, f32::MAX) };
        let proximity = self.proximity(dist);
        let ratio = self.power[me as usize] / self.power[q as usize].max(5.0);
        // Patient leaders warm up to war later in the game.
        let start = 15.0 + dr.patience as f32 * 7.0;
        let ramp = ((game.turn as f32 - start) / 40.0).clamp(0.0, 1.0);

        let boxed = if self.free_sites == 0 { 1.0 } else { 0.0 };
        let land = 6.0 * self.d(dr.expansion) * boxed * proximity;
        let opportunity = 8.0 * self.d(dr.opportunism) * (ratio - 1.0).clamp(0.0, 1.5) * proximity;
        // Militaristic leaders like a war for its own sake.
        let warlike = 2.0 * self.d(dr.military) * proximity;
        let agenda = self.agenda_bonus(game, q) * proximity.max(0.3);
        let grudge = self.grudge(game, q);
        let momentum = if self.momentum(game, q) { 3.0 } else { 0.0 };

        let provoked = grudge > 0.0;
        let unprovoked = match self.agenda {
            // These leaders don't start wars; they finish them.
            AgendaKind::SleepingGiant | AgendaKind::NeverForgive => 0.0,
            _ => (land + opportunity + warlike + agenda) * ramp,
        };
        let answer = match self.agenda {
            AgendaKind::SleepingGiant | AgendaKind::NeverForgive => grudge * 1.8,
            // Forgives quickly.
            AgendaKind::GraciousConqueror | AgendaKind::StrongTake => grudge * 0.6,
            _ => grudge,
        };
        let desire = unprovoked + answer * proximity.max(0.5) + momentum;
        // Risky leaders attack at worse odds; answering an attack needs less.
        let mut odds = 1.6 - 0.08 * dr.risk as f32;
        if provoked {
            odds -= 0.3;
        }
        if self.agenda == AgendaKind::StrongTake {
            odds += 0.2; // Backs off from anyone stronger.
        }
        (desire, odds.max(0.6))
    }

    /// How much a rival `dist` tiles away counts as a neighbour (0..1).
    fn proximity(&self, dist: i32) -> f32 {
        let nearest = self.rivals.iter().filter_map(|&q| self.city_distance(q)).min().unwrap_or(dist) as f32;
        let near = nearest.max(NEIGHBOUR_DISTANCE);
        let d = dist as f32;
        (1.0 - (d - near) / NEIGHBOUR_FALLOFF).clamp(0.0, 1.0) * ((FARTHEST_NEIGHBOUR - d) / 15.0).clamp(0.0, 1.0)
    }

    /// The agenda's own reason to fight `q`.
    fn agenda_bonus(&self, game: &Game, q: PlayerId) -> f32 {
        let rivals = &self.rivals;
        let near = |mine: &dyn Fn(TileId) -> bool, theirs: &dyn Fn(TileId) -> bool, dist: i32| {
            self.my_cities(game).filter(|c| mine(c.tile)).any(|c| {
                game.cities.values().any(|o| o.owner == q && theirs(o.tile) && game.map.grid.distance(c.tile, o.tile) <= dist)
            })
        };
        match self.agenda {
            AgendaKind::MercantileRival => {
                let coast = near(&|t| game.is_coastal(t), &|t| game.is_coastal(t), 7);
                let richer = game.players[q as usize].gold > game.players[self.me as usize].gold;
                (if coast { 3.0 } else { 0.0 }) + if richer { 1.5 } else { 0.0 }
            }
            AgendaKind::PaxRomana => {
                if near(&|_| true, &|_| true, 6) { 4.0 } else { 0.0 }
            }
            AgendaKind::RiverWarden => {
                let river = |t: TileId| game.map.has_river(t);
                if near(&river, &river, 6) { 4.0 } else { 0.0 }
            }
            AgendaKind::StrongTake => {
                let weakest = rivals.iter().copied().min_by(|&a, &b| self.power[a as usize].total_cmp(&self.power[b as usize]).then(a.cmp(&b)));
                if weakest == Some(q) { 4.0 } else { 0.0 }
            }
            AgendaKind::SleepingGiant | AgendaKind::NeverForgive => 0.0,
            AgendaKind::Plunder => 2.0,
            AgendaKind::FlowerWars => {
                if self.flower_rival() == Some(q) { 5.0 } else { 0.0 }
            }
            AgendaKind::GraciousConqueror => {
                let biggest = game
                    .cities
                    .values()
                    .filter(|c| rivals.contains(&c.owner))
                    .max_by_key(|c| (c.population, std::cmp::Reverse(c.id)))
                    .map(|c| c.owner);
                if biggest == Some(q) { 3.0 } else { 0.0 }
            }
            AgendaKind::Tawantinsuyu => {
                // Small neighbours are for absorbing; smaller ones than us
                // are fair game like for anyone; our equals are left alone.
                let cities = game.cities.values().filter(|c| c.owner == q).count();
                let ours = self.my_cities(game).count();
                if cities <= 2 {
                    5.0
                } else if cities < ours {
                    0.0
                } else {
                    -3.0
                }
            }
        }
    }

    /// The Flower Wars rival: the neighbour whose cities are closest to ours.
    fn flower_rival(&self) -> Option<PlayerId> {
        self.rivals.iter().copied().filter_map(|q| self.city_distance(q).map(|d| (d, q))).min().map(|(_, q)| q)
    }

    fn choose_war(&self, game: &Game) -> Option<War> {
        let me = self.me;
        self.my_cities(game).next()?;
        let mut best: Option<(f32, PlayerId)> = None;
        // A peace treaty forbids new wars for a while, an alliance for good.
        for &q in self.rivals.iter().filter(|&&q| game.at_war(me, q) || (game.truce_left(me, q) == 0 && !game.allied(me, q))) {
            let (desire, odds) = self.war_desire(game, q);
            let ratio = self.power[me as usize] / self.power[q as usize].max(5.0);
            // Keep fighting a war we started or marched out for, even when
            // the odds slip a little.
            let committed = self.momentum(game, q) || self.deployed_against(game, q);
            let (desire, odds) = if committed { (desire + 2.0, odds - 0.25) } else { (desire, odds) };
            if desire >= WAR_DESIRE && ratio >= odds && best.is_none_or(|(b, _)| desire > b) {
                best = Some((desire, q));
            }
        }
        let (_, enemy) = best?;
        let target = self.target_city(game, enemy)?;
        let raid = matches!(self.agenda, AgendaKind::Plunder | AgendaKind::FlowerWars);
        Some(War { enemy, target, raid })
    }

    /// The enemy city to march on: near, weakly defended (by its walls and
    /// any army we can see in it) and within what our ranged units can wear
    /// down; the Gracious Conqueror favours big ones.
    fn target_city(&self, game: &Game, enemy: PlayerId) -> Option<CityId> {
        let me = self.me;
        let ranged: Vec<f32> = game
            .units
            .values()
            .filter(|u| u.owner == me && game.unit_def(u).ranged > 0)
            .map(|u| crate::production::city_attack(game.unit_def(u)))
            .collect();
        // Where our attackers stand: a city near them is closer than our
        // cities say, and the one most of them are sent to take is kept
        // (their orders are the memory that stops us switching targets
        // mid-march).
        let attackers: Vec<(TileId, CityId)> = game
            .units
            .values()
            .filter(|u| u.owner == me && game.unit_def(u).domain == sim::rules::Domain::Land)
            .filter_map(|u| match u.order {
                sim::game::Order::Take { city } => Some((u.tile, city)),
                _ => None,
            })
            .collect();
        let armies: Vec<TileId> = attackers.iter().map(|a| a.0).collect();
        let mut sent: BTreeMap<CityId, usize> = BTreeMap::new();
        for &(_, c) in &attackers {
            *sent.entry(c).or_default() += 1;
        }
        let committed = sent
            .iter()
            .filter(|(c, _)| game.cities.get(c).is_some_and(|c| c.owner == enemy))
            .max_by_key(|&(c, n)| (*n, std::cmp::Reverse(*c)))
            .filter(|(c, _)| armies.iter().any(|&t| game.map.grid.distance(t, game.cities[c].tile) <= COMMITTED))
            .map(|(&c, _)| c);
        // Cities across the water: our armies sail there, past their navy.
        let shore = crate::navy::shores(game);
        let ours = crate::navy::own_shores(game, self, &shore);
        let (our_navy, their_navy) = (crate::navy::naval_power(game, me), crate::navy::naval_power(game, enemy));
        let crossing = OVERSEAS_NAVY_COST * ((their_navy - our_navy) / (our_navy + their_navy).max(1.0)).max(0.0);
        game.cities
            .values()
            .filter(|c| c.owner == enemy && self.knows_city(game, c) && self.on_our_land(game, c.tile))
            .filter_map(|c| {
                let from_cities = self.my_cities(game).map(|m| game.map.grid.distance(m.tile, c.tile)).min()?;
                let from_armies = armies.iter().map(|&t| game.map.grid.distance(t, c.tile)).min().unwrap_or(i32::MAX);
                let dist = from_cities.min(from_armies);
                let defense = city_defense(game, c.id);
                // Damage per turn if our whole ranged force bombarded it.
                let siege: f32 = ranged.iter().map(|&a| expected_damage(a, defense)).sum();
                // The enemy units guarding it (one to a tile, they hold the
                // ground around it), counted like its walls: as the AI knows
                // every civ's power, it counts them out of sight too, so the
                // target doesn't switch as they come into view.
                let guards: f32 = game
                    .units
                    .values()
                    .filter(|u| u.owner == enemy && game.map.grid.distance(u.tile, c.tile) <= 2)
                    .map(|u| unit_power(game, u))
                    .sum();
                let mut cost = dist as f32 * 3.0 + defense + guards * 1.5 + (30.0 - siege).max(0.0) * 3.0;
                if !ours.contains(&shore[c.tile as usize]) {
                    cost += crossing;
                }
                if committed == Some(c.id) {
                    cost -= COMMITMENT;
                }
                // Walls already battered down are half the work done, if an
                // army of ours is near enough to use it before they're mended.
                if from_armies <= COMMITTED {
                    cost -= (sim::combat::CITY_MAX_HP - c.hp) as f32;
                }
                match self.agenda {
                    AgendaKind::GraciousConqueror => cost -= c.population as f32 * 3.0,
                    AgendaKind::PaxRomana => cost += dist as f32 * 3.0,
                    _ => {}
                }
                Some((cost, c.id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, c)| c)
    }

    /// Enemy military strength (that we can see) close to each of our cities.
    fn find_threats(&self, game: &Game) -> BTreeMap<CityId, f32> {
        let me = self.me;
        let sees = |t: TileId| game.players[me as usize].sees(t);
        let mut out = BTreeMap::new();
        for c in self.my_cities(game) {
            let threat: f32 = game
                .units
                .values()
                .filter(|u| u.owner != me && sees(u.tile) && game.map.grid.distance(u.tile, c.tile) <= THREAT_RADIUS)
                .filter(|u| self.hostile[u.owner as usize])
                .map(|u| unit_power(game, u))
                .sum();
            if threat > 0.0 {
                out.insert(c.id, threat);
            }
        }
        out
    }

    /// The army we want: a garrison per city, more for militaristic and
    /// defensive leaders and as the game goes on, keeping up with the
    /// neighbours, and more again at war or under threat.
    fn military_wanted(&self, game: &Game) -> usize {
        let dr = &self.drives;
        let cities = self.my_cities(game).count() as f32;
        if cities == 0.0 {
            return 0;
        }
        let era = (game.turn as f32 / 80.0).clamp(0.25, 1.0);
        let mut want = cities + cities * (0.2 + 0.9 * self.d(dr.military)) * era;
        if dr.defense >= 7 {
            want += cities * 0.5;
        }
        // Keep up with the strongest neighbour.
        let me = self.me;
        let rival = self
            .rivals
            .iter()
            .filter(|&&q| self.city_distance(q).is_some_and(|d| self.proximity(d) >= 0.5))
            .map(|&q| game.military_units(q) as f32)
            .fold(0.0, f32::max);
        want = want.max(rival * (0.4 + 0.6 * self.d(dr.military)));
        if self.war.is_some() {
            want += 3.0 + cities;
        }
        if !self.threats.is_empty() {
            want += 2.0;
        }
        let attacked = (0..game.players.len() as PlayerId).any(|q| q != me && self.grudge(game, q) > 0.0);
        if attacked && self.agenda == AgendaKind::SleepingGiant {
            want *= 1.8;
        }
        let want = want.min(cities * 4.0 + 4.0).round() as usize;
        match game.options.unit_limit {
            Some(limit) => want.min(limit as usize),
            None => want,
        }
    }
}
