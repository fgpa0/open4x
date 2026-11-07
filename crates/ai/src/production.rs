//! What computer players' cities build. The strategy decides how big an
//! army it wants; the missing units are built in the most productive
//! cities, keeping the army's mix of infantry, ranged units, cavalry and (in
//! sieges) siege engines close to what the leader favours (see `army` in
//! civs.ron), plus a General for a big attack and an Admiral for a navy.
//! Threatened cities without a garrison raise one first, then walls. When a
//! queue runs empty, settlers go out while there is good land, then Workers
//! and Merchants while there are too few, then a canal that saves ships a
//! long way round, otherwise the building the leader values most (science, gold, production...) is built.

use sim::city::{Build, CityId};
use sim::civ::AgendaKind;
use sim::game::{Command, Event, Game, Order, PlayerId};
use sim::hex::TileId;
use sim::civ::CombatCond;
use sim::rules::{Ability, Domain, UnitClass, UnitDef, UnitTypeId};

use crate::strategy::Strategy;

/// What a military unit is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Melee,
    Ranged,
    /// Ranged with a bonus against cities.
    Siege,
    /// Mounted melee units.
    Cavalry,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Melee, Role::Ranged, Role::Cavalry, Role::Siege];

    /// How much a leader likes this kind of unit (0..10).
    pub fn liking(self, army: &sim::civ::ArmyMix) -> u8 {
        match self {
            Role::Melee => army.melee,
            Role::Ranged => army.ranged,
            Role::Cavalry => army.cavalry,
            Role::Siege => army.siege,
        }
    }
}

/// The share of the army `s.me` wants in each role it can build, adding up
/// to 1: the leader's mix, with siege engines (almost) only for sieges, at
/// least a quarter of them and of ranged units then, and at least a third infantry, to hold and
/// take cities.
pub fn army_shares(game: &Game, s: &Strategy, besieging: bool) -> Vec<(Role, f32)> {
    let weight = |r: Role| {
        let w = r.liking(&s.army) as f32;
        match r {
            Role::Siege if !besieging => w * 0.1,
            // Horsemen don't storm walls.
            Role::Cavalry if besieging => w * 0.4,
            _ => w,
        }
    };
    let roles: Vec<Role> = Role::ALL.into_iter().filter(|&r| best_unit(game, s.me, r).is_some()).collect();
    let total: f32 = roles.iter().map(|&r| weight(r)).sum::<f32>().max(0.1);
    let mut shares: Vec<(Role, f32)> = roles.iter().map(|&r| (r, weight(r) / total)).collect();
    at_least(&mut shares, Role::Melee, MIN_INFANTRY);
    if besieging {
        at_least(&mut shares, Role::Siege, MIN_SIEGE);
        at_least(&mut shares, Role::Ranged, MIN_SIEGE);
    }
    shares
}

/// Raises `role`'s share to at least `min`, taking from the others in
/// proportion.
fn at_least(shares: &mut [(Role, f32)], role: Role, min: f32) {
    let Some(i) = shares.iter().position(|(r, _)| *r == role) else { return };
    let rest = 1.0 - shares[i].1;
    if shares[i].1 >= min || rest <= 0.0 {
        return;
    }
    for (j, (_, share)) in shares.iter_mut().enumerate() {
        *share = if j == i { min } else { *share * (1.0 - min) / rest };
    }
}

/// Infantry makes up at least this share of an army, and siege engines (and
/// ranged units) this much each of one besieging a city.
const MIN_INFANTRY: f32 = 0.34;
const MIN_SIEGE: f32 = 0.25;

pub fn role(def: &UnitDef) -> Role {
    if def.ranged == 0 && def.has(Ability::Mounted) {
        Role::Cavalry
    } else if def.ranged == 0 {
        Role::Melee
    } else if def.bonuses.iter().any(|b| b.when == CombatCond::AttackingCities) {
        Role::Siege
    } else {
        Role::Ranged
    }
}

/// A unit's ranged strength against cities, with its bonuses.
pub fn city_attack(def: &UnitDef) -> f32 {
    let bonus: i32 = def.bonuses.iter().filter(|b| b.when == CombatCond::AttackingCities).map(|b| b.percent).sum();
    def.ranged as f32 * (1.0 + bonus as f32 / 100.0)
}

/// The civ's strongest available military land unit for a role.
pub fn best_unit(game: &Game, me: PlayerId, want: Role) -> Option<UnitTypeId> {
    let rules = &game.rules;
    (0..rules.units.units.len() as UnitTypeId)
        .filter(|&u| {
            let def = rules.unit(u);
            def.class == UnitClass::Military
                && def.domain == Domain::Land
                && !crate::military::is_recon(rules, u)
                && role(def) == want
                && game.civ_builds(me, Build::Unit(u))
                && game.has_tech_for(me, Build::Unit(u))
                && game.has_resources_for(me, Build::Unit(u))
        })
        .max_by_key(|&u| {
            let def = rules.unit(u);
            (def.strength.max(def.ranged), std::cmp::Reverse(def.cost), std::cmp::Reverse(u))
        })
}

/// The civ's strongest available ship (ranged ones first).
pub fn best_ship(game: &Game, me: PlayerId) -> Option<UnitTypeId> {
    let rules = &game.rules;
    (0..rules.units.units.len() as UnitTypeId)
        .filter(|&u| rules.unit(u).domain == Domain::Sea && !rules.unit(u).has(Ability::General) && game.civ_builds(me, Build::Unit(u)) && game.has_tech_for(me, Build::Unit(u)))
        .max_by_key(|&u| {
            let def = rules.unit(u);
            (def.ranged > 0, def.strength.max(def.ranged), std::cmp::Reverse(u))
        })
}

fn is_settler(game: &Game, b: Build) -> bool {
    matches!(b, Build::Unit(u) if game.rules.unit(u).has(Ability::FoundCity))
}

pub fn plan(game: &mut Game, s: &Strategy) -> Vec<Event> {
    let me = s.me;
    let mut events = Vec::new();
    let cities: Vec<CityId> = s.my_cities(game).map(|c| c.id).collect();
    let mut queues: Vec<(CityId, Vec<Build>)> = cities.iter().map(|&c| (c, game.cities[&c].queue.clone())).collect();
    // Units whose strategic resource is gone can't be finished.
    for (_, q) in &mut queues {
        q.retain(|&b| game.has_resources_for(me, b));
    }

    let capped = game.at_unit_limit(me);
    // The army, by role, against the mix the leader wants.
    let units: Vec<_> = game.units.values().filter(|u| u.owner == me && crate::military::is_soldier(game, u) && !crate::military::is_general(game, u)).collect();
    let besieging = s.war.is_some_and(|w| !w.raid);
    let shares = army_shares(game, s, besieging);
    let mut counts: Vec<usize> = shares.iter().map(|(r, _)| units.iter().filter(|u| role(game.unit_def(u)) == *r).count()).collect();
    let mut total = units.len();
    // Threatened cities without a garrison raise one first, then walls.
    let walls = game.civ_building(me, "walls");
    for (city, queue) in &mut queues {
        let Some(&threat) = s.threats.get(city) else { continue };
        let garrisoned = game.garrison(*city).is_some();
        if !capped && !garrisoned && !queue.first().is_some_and(|&b| is_military(game, b)) {
            if let Some(b) = best_unit(game, me, Role::Melee) {
                queue.insert(0, Build::Unit(b));
            }
        }
        if let Some(w) = walls.map(Build::Building) {
            let ok = game.has_tech_for(me, w) && !game.cities[city].has(walls.unwrap()) && !queue.contains(&w);
            if ok && threat >= 10.0 {
                let at = if garrisoned { 0 } else { 1.min(queue.len()) };
                queue.insert(at, w);
            }
        }
    }

    // The treasury must keep what's built.
    let income: i32 = s.my_cities(game).map(|c| game.city_output(c.id).yields.gold).sum();
    let broke = income - game.upkeep(me) < 1 && game.players[me as usize].gold < 100;
    // Missing military units go to the most productive cities.
    let building = queues.iter().filter(|(_, q)| q.first().is_some_and(|&b| is_military(game, b) && !is_ship(game, b))).count();
    let mut deficit = s.want_military.saturating_sub(total + building);
    if (game.turn < 12 && s.threats.is_empty()) || broke {
        deficit = 0; // The opening is for growing and settling.
    }
    // The role furthest below its share of the army (with this unit added).
    let mut next_unit = |game: &Game| {
        let gap = |i: usize| shares[i].1 * (total + 1) as f32 - counts[i] as f32;
        let i = (0..shares.len()).max_by(|&a, &b| gap(a).total_cmp(&gap(b)).then(b.cmp(&a)))?;
        let pick = best_unit(game, me, shares[i].0)?;
        total += 1;
        counts[i] += 1;
        Some(Build::Unit(pick))
    };
    let busy = |game: &Game, q: &[Build]| q.first().is_some_and(|&b| is_military(game, b) || is_settler(game, b));
    let mut by_production: Vec<usize> = (0..queues.len()).collect();
    by_production.sort_by_key(|&i| (std::cmp::Reverse(game.city_output(queues[i].0).yields.production), queues[i].0));
    if !capped && deficit > 0 {
        for &i in &by_production {
            if deficit == 0 {
                break;
            }
            // Don't interrupt units or settlers already under way.
            if busy(game, &queues[i].1) {
                continue;
            }
            if let Some(b) = next_unit(game) {
                queues[i].1.insert(0, b);
                deficit -= 1;
            }
        }
    }

    // A General for a big attack, an Admiral for a navy of a few ships.
    let leader = |domain: Domain| (0..game.rules.units.units.len() as UnitTypeId).find(|&k| game.rules.unit(k).has(Ability::General) && game.rules.unit(k).domain == domain);
    let leaders = |domain: Domain| {
        game.units.values().filter(|u| u.owner == me && game.unit_def(u).has(Ability::General) && game.unit_def(u).domain == domain).count()
            + queues.iter().filter(|(_, q)| q.iter().any(|&b| matches!(b, Build::Unit(k) if game.rules.unit(k).has(Ability::General) && game.rules.unit(k).domain == domain))).count()
    };
    let attackers = game.units.values().filter(|u| u.owner == me && matches!(u.order, Order::Take { .. })).count();
    let ships_now = game.units.values().filter(|u| u.owner == me && game.unit_def(u).domain == Domain::Sea && !game.unit_def(u).has(Ability::General)).count();
    let wants = [(Domain::Land, attackers / GENERAL_PER, leaders(Domain::Land)), (Domain::Sea, usize::from(ships_now >= ADMIRAL_FROM), leaders(Domain::Sea))];
    for (domain, wanted, have) in wants {
        let Some(kind) = leader(domain).map(Build::Unit) else { continue };
        if capped || broke || have >= wanted.min(MAX_LEADERS) || !game.has_tech_for(me, kind) {
            continue;
        }
        if let Some(&i) = by_production.iter().find(|&&i| !busy(game, &queues[i].1) && game.builds_here(queues[i].0, kind)) {
            queues[i].1.insert(0, kind);
        }
    }

    // Ships, in the most productive coastal cities.
    let ships = ships_now + queues.iter().filter(|(_, q)| q.iter().any(|&b| is_ship(game, b))).count();
    let mut ship_deficit = crate::navy::ships_wanted(game, s).saturating_sub(ships);
    if let (false, false, Some(ship)) = (capped, broke, best_ship(game, me).map(Build::Unit)) {
        for &i in &by_production {
            if ship_deficit == 0 {
                break;
            }
            if !game.builds_here(queues[i].0, ship) || busy(game, &queues[i].1) {
                continue;
            }
            queues[i].1.insert(0, ship);
            ship_deficit -= 1;
        }
    }

    // Empty queues: settlers while there is room, Workers while there are
    // too few, else the most valued building.
    let is_worker = |b: &Build| matches!(b, Build::Unit(u) if game.rules.unit(*u).has(Ability::BuildImprovements));
    let mut workers = crate::work::workers(game, me) + queues.iter().filter(|(_, q)| q.iter().any(is_worker)).count();
    let workers_wanted = crate::work::workers_wanted(game, me, s.drives.builder);
    let mut settlers = game.units.values().filter(|u| u.owner == me && game.unit_def(u).has(Ability::FoundCity)).count()
        + queues.iter().filter(|(_, q)| q.iter().any(|&b| is_settler(game, b))).count();
    let is_merchant = |b: &Build| matches!(b, Build::Unit(u) if game.rules.unit(*u).has(Ability::Trade));
    let mut merchants = game.merchants(me).count() + queues.iter().map(|(_, q)| q.iter().filter(|b| is_merchant(b)).count()).sum::<usize>();
    let mut queues_passages: Vec<Build> =
        queues.iter().flat_map(|(_, q)| q.iter().copied()).filter(|b| matches!(b, Build::Passage(..))).collect();
    let expansion = s.drives.expansion as usize;
    let max_cities = 3 + (expansion * 6).div_ceil(10);
    for (city, queue) in &mut queues {
        if !queue.is_empty() {
            continue;
        }
        let c = &game.cities[city];
        let min_pop = if expansion >= 7 { 2 } else { 3 };
        let room = cities.len() + settlers < max_cities && s.free_sites > settlers;
        let settler = game.civ_unit(me, "settler").map(Build::Unit).filter(|_| {
            room && c.population >= min_pop && settlers < 1 + expansion / 4 && !s.threats.contains_key(city)
        });
        let worker = game.civ_unit(me, "worker").map(Build::Unit).filter(|_| {
            workers < workers_wanted && c.population >= 2 && !s.threats.contains_key(city) && game.turn >= 8
        });
        let merchant = game.civ_unit(me, "merchant").map(Build::Unit).filter(|&b| {
            merchants < game.trade_capacity(me) && c.population >= 3 && !s.threats.contains_key(city) && game.has_tech_for(me, b)
        });
        if let Some(b) = settler {
            queue.push(b);
            settlers += 1;
        } else if let Some(b) = worker {
            queue.push(b);
            workers += 1;
        } else if let Some(b) = merchant {
            queue.push(b);
            merchants += 1;
        } else if let Some(builds) = passage(game, *city, &queues_passages) {
            queue.extend(&builds);
            queues_passages.extend(builds);
        } else if let Some(b) = fort(game, s, *city, &queues_passages) {
            queue.push(b);
            queues_passages.push(b);
        } else if let Some(b) = best_building(game, s, *city) {
            queue.push(b);
        }
    }

    for (city, queue) in queues {
        if game.cities[&city].queue != queue {
            if let Ok(e) = game.apply(me, Command::SetQueue { city, queue }) {
                events.extend(e);
            }
        }
    }
    events
}

/// One General for every this many units sent to take a city...
const GENERAL_PER: usize = 6;
/// ...and no more than this many Generals (or Admirals).
const MAX_LEADERS: usize = 2;
/// Ships a navy needs before the AI builds an Admiral for it.
const ADMIRAL_FROM: usize = 3;

fn is_military(game: &Game, b: Build) -> bool {
    matches!(b, Build::Unit(u) if game.rules.unit(u).class == UnitClass::Military)
}

fn is_ship(game: &Game, b: Build) -> bool {
    matches!(b, Build::Unit(u) if game.rules.unit(u).class == UnitClass::Military && game.rules.unit(u).domain == Domain::Sea)
}

/// How far round by water a canal must save ships for a city to dig it.
const CANAL_MIN_SAVING: i32 = 8;

/// The most tiles of land a computer player digs through for one canal.
const MAX_CANAL: usize = 3;

/// How many tiles of water past a canal count as a sea worth reaching when
/// there's no way round at all (not a pond).
const OPEN_WATER: usize = 30;

/// How far round over land a bridge must save land units for a city to build it.
const BRIDGE_MIN_SAVING: i32 = 6;

/// A canal or bridge worth building on `city`'s land, if there is one: a
/// bridge that joins separate lands or saves a long way round, or the tiles
/// of a canal (in digging order, from the shore) that does the same for
/// ships (`taken`: already queued). One canal at a time per player.
fn passage(game: &Game, city: CityId, taken: &[Build]) -> Option<Vec<Build>> {
    use sim::improvement::Passage;
    let owner = game.cities[&city].owner;
    let can = |p: Passage| game.has_tech_for(owner, Build::Passage(p, 0));
    let digging = taken.iter().any(|b| matches!(b, Build::Passage(Passage::Canal, _)));
    if can(Passage::Canal) && !digging {
        if let Some(tiles) = canal_plan(game, city) {
            return Some(tiles.into_iter().map(|t| Build::Passage(Passage::Canal, t)).collect());
        }
    }
    if !can(Passage::Bridge) {
        return None;
    }
    game.passage_sites(city, Passage::Bridge)
        .into_iter()
        .filter(|&t| game.bridge_shortcut(t).is_none_or(|d| d >= BRIDGE_MIN_SAVING))
        .map(|t| Build::Passage(Passage::Bridge, t))
        .find(|b| !taken.contains(b))
        .map(|b| vec![b])
}

/// A neighbour's city this close to one of ours makes it a border city.
const BORDER_DISTANCE: i32 = 10;
/// Forts stand at least this far apart.
const FORT_SPACING: i32 = 4;

/// A fort for `city` if it's a border city (at war, or facing a neighbour
/// with a defensive leader) without one nearby: on our land within its
/// reach, as near the neighbour's nearest city as it can be, on high ground
/// if it's as good (`taken`: posts already queued).
fn fort(game: &Game, s: &Strategy, city: CityId, taken: &[Build]) -> Option<Build> {
    use sim::improvement::Passage;
    let me = s.me;
    if !game.has_tech_for(me, Build::Passage(Passage::Fort, 0)) {
        return None;
    }
    let at = game.cities[&city].tile;
    let grid = &game.map.grid;
    let foe = game
        .cities
        .values()
        .filter(|c| c.owner != me && game.has_met(me, c.owner) && (game.at_war(me, c.owner) || s.drives.defense >= 6))
        .map(|c| (grid.distance(c.tile, at), c.tile))
        .filter(|&(d, _)| d <= BORDER_DISTANCE)
        .min()?
        .1;
    let forts: Vec<TileId> = game.forts().chain(taken.iter().filter_map(|b| match b {
        Build::Passage(Passage::Fort, t) => Some(*t),
        _ => None,
    })).collect();
    let spaced = |t: TileId| forts.iter().all(|&f| grid.distance(f, t) >= FORT_SPACING);
    game.passage_sites(city, Passage::Fort)
        .into_iter()
        .filter(|&t| grid.distance(t, at) >= 2 && spaced(t))
        .min_by_key(|&t| (grid.distance(t, foe), game.map.tile(t).elevation == sim::map::Elevation::Flat, t))
        .map(|t| Build::Passage(Passage::Fort, t))
}

/// The best canal through `city`'s land, if one saves ships enough: up to
/// [`MAX_CANAL`] tiles of land, from one shore to water that's
/// [`CANAL_MIN_SAVING`] moves further round by sea (or that the sea doesn't
/// reach, if it's more than a pond). As each tile is dug the same canal is
/// found again, shorter, so it gets finished without being remembered.
pub fn canal_plan(game: &Game, city: CityId) -> Option<Vec<sim::hex::TileId>> {
    use sim::hex::TileId;
    use std::collections::{HashMap, HashSet, VecDeque};
    let grid = &game.map.grid;
    let water = |t: TileId| game.map.is_water(&game.rules, t);
    let queued = |t: TileId| game.cities.values().any(|o| o.queue.contains(&Build::Passage(sim::improvement::Passage::Canal, t)));
    let ours = |t: TileId| game.district(t) == Some(city) && game.diggable(t) && !queued(t);
    let c = &game.cities[&city];
    let land: Vec<TileId> = grid.within(c.tile, sim::city::MAX_CITY_RADIUS).into_iter().filter(|&t| ours(t)).collect();
    let shore: Vec<TileId> = land.iter().copied().filter(|&t| grid.neighbors(t).any(water)).collect();
    let mut best: Option<(i32, Vec<TileId>)> = None;
    for &s in &shore {
        // Ways inland from `s`, at most MAX_CANAL tiles long.
        let mut parent: HashMap<TileId, TileId> = HashMap::from([(s, s)]);
        let mut depth = HashMap::from([(s, 1usize)]);
        let mut queue = VecDeque::from([s]);
        while let Some(t) = queue.pop_front() {
            if depth[&t] == MAX_CANAL {
                continue;
            }
            for n in grid.neighbors(t) {
                if !parent.contains_key(&n) && land.contains(&n) {
                    parent.insert(n, t);
                    depth.insert(n, depth[&t] + 1);
                    queue.push_back(n);
                }
            }
        }
        // How far ships go round from the water by `s` (only worked out if
        // some way inland comes out on other water).
        let start: Vec<TileId> = grid.neighbors(s).filter(|&n| water(n)).collect();
        let mut ends: Vec<(TileId, TileId)> =
            depth.keys().flat_map(|&e| grid.neighbors(e).filter(|&b| water(b) && !start.contains(&b)).map(move |b| (e, b))).collect();
        // In tile order: a hash map's order changes from run to run.
        ends.sort();
        if ends.is_empty() {
            continue;
        }
        // Only far enough to tell whether the way round is long enough.
        let far = MAX_CANAL as i32 + 1 + CANAL_MIN_SAVING;
        let mut dist: HashMap<TileId, i32> = start.iter().map(|&t| (t, 0)).collect();
        let mut frontier = start.clone();
        for d in 1..far {
            let mut next = Vec::new();
            for t in frontier {
                for n in grid.neighbors(t) {
                    if water(n) && !dist.contains_key(&n) {
                        dist.insert(n, d);
                        next.push(n);
                    }
                }
            }
            frontier = next;
        }
        let open_sea = |b: TileId| {
            let mut seen = HashSet::from([b]);
            let mut stack = vec![b];
            while let Some(t) = stack.pop() {
                for n in grid.neighbors(t) {
                    if water(n) && seen.insert(n) {
                        if seen.len() >= OPEN_WATER {
                            return true;
                        }
                        stack.push(n);
                    }
                }
            }
            false
        };
        for (e, b) in ends {
            let tiles = depth[&e] as i32;
            // Not reached: at least `far` round, or no way round at all
            // (worth it only if it's more than a pond).
            let saving = match dist.get(&b) {
                Some(&round) => round - (tiles + 1),
                None if open_sea(b) => far - (tiles + 1),
                None => continue,
            };
            if saving < CANAL_MIN_SAVING || best.as_ref().is_some_and(|(v, _)| *v >= saving - tiles) {
                continue;
            }
            let mut way = vec![e];
            while *way.last().expect("starts with e") != s {
                way.push(parent[way.last().expect("not empty")]);
            }
            way.reverse();
            best = Some((saving - tiles, way));
        }
    }
    best.map(|(_, way)| way)
}

/// The building this leader values most for its cost.
fn best_building(game: &Game, s: &Strategy, city: CityId) -> Option<Build> {
    let d = |v: u8| v as f32 / 10.0;
    let dr = &s.drives;
    let frontier = s.threats.contains_key(&city) || s.war.is_some();
    game.available_builds(city)
        .into_iter()
        .filter_map(|b| {
            let Build::Building(id) = b else { return None };
            let def = game.rules.building(id);
            let mut value = def.food as f32 * (0.8 + d(dr.expansion))
                + def.production as f32 * (0.8 + d(dr.builder))
                + def.gold as f32 * (0.3 + d(dr.wealth))
                + def.science as f32 * (0.4 + d(dr.science))
                + def.culture as f32 * (0.3 + 0.5 * d(dr.builder))
                + def.keeps_food as f32 / 25.0
                + def.military_production as f32 / 25.0 * d(dr.military)
                + def.unit_xp as f32 / 12.0 * d(dr.military)
                + def.trade_gold as f32 / 25.0 * (0.3 + d(dr.wealth));
            // Bonuses on the tiles the city works now.
            if let Some(bonus) = &def.tile_bonus {
                let c = &game.cities[&city];
                let tiles = c.worked.iter().filter(|&&t| game.rules.tile_matches(&game.map, t, game.improvement_at(t), &bonus.when)).count() as f32;
                value += tiles
                    * (bonus.food as f32 * (0.8 + d(dr.expansion)) + bonus.production as f32 * (0.8 + d(dr.builder)) + bonus.gold as f32 * (0.3 + d(dr.wealth)));
            }
            if def.defense > 0 {
                value += if frontier || dr.defense >= 8 { def.defense as f32 / 20.0 * (0.5 + d(dr.defense)) } else { 0.0 };
            }
            if s.agenda == AgendaKind::MercantileRival && def.gold > 0 {
                value += 1.0;
            }
            (value > 0.0).then(|| (value * 100.0 / def.cost as f32, id))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))
        .map(|(_, id)| Build::Building(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::game::GameSetup;
    use sim::mapgen::{generate, MapSettings};
    use sim::rules::Rules;

    /// Every leader's army follows its mix: with everything available,
    /// their favourite kind gets the biggest share, and infantry never
    /// falls below a third.
    #[test]
    fn armies_follow_the_leaders_taste() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let civs = ["mongols", "rome", "inca"].iter().map(|c| rules.civ_id(c).unwrap()).collect();
        let mut g = Game::with_setup(rules, map, &GameSetup { civs, options: Default::default(), starts: Vec::new() });
        let horses = g.rules.resource_id("horses").unwrap();
        for p in 0..3 {
            g.players[p].techs.fill(true);
            // Horses for everyone, as if traded.
            g.resource_deals.push(sim::resource::ResourceDeal { from: 2, to: p as u8, resource: horses, turns_left: 30 });
        }
        let share = |g: &Game, p: u8, r: Role| {
            let s = Strategy::assess(g, p);
            army_shares(g, &s, false).iter().find(|x| x.0 == r).map_or(0.0, |x| x.1)
        };
        assert!(share(&g, 0, Role::Cavalry) > share(&g, 1, Role::Cavalry), "Mongols ride more than Romans");
        assert!(share(&g, 2, Role::Ranged) > share(&g, 1, Role::Ranged), "Inca slingers");
        for p in 0..3 {
            assert!(share(&g, p, Role::Melee) >= MIN_INFANTRY - 1e-4);
            let s = Strategy::assess(&g, p);
            let total: f32 = army_shares(&g, &s, false).iter().map(|x| x.1).sum();
            assert!((total - 1.0).abs() < 1e-4);
        }
    }
}
