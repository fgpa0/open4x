//! Tile improvements and roads, built by Workers (see `data/improvements.ron`).
//!
//! A Worker given a [`Work`] order (`Command::Build`) stays on its tile and
//! adds a turn of work at every turn end until it's done. An automated Worker
//! ([`Order::Automate`]) told to build carries on automatically afterwards. Progress is kept on
//! the tile, so a Worker that leaves (or another that takes over) loses
//! nothing unless something else is started there. Improvements go only
//! inside their owner's borders and add their yields to the tile; building
//! one on a forest, jungle or marsh clears it first (more turns, and some
//! production for the nearest city), unless the tile's resource needs it.
//!
//! Roads can also go outside anyone's borders. Moving between two road tiles
//! costs 1 movement whatever the terrain. Every city comes with a road.
//! Railways are laid over roads once their owner knows the tech: a step
//! between two railway tiles costs a fifth of a point
//! ([`crate::path::RAIL_COST`]). A city has one once its owner knows how.
//!
//! Tunnels, canals and bridges ([`Passage`]) are city builds
//! (`Build::Passage`), placed on a tile of the city's own land. A tunnel
//! takes land units through a mountain (and carries a road). A canal digs
//! out a tile of flat land by the water (see [`Game::canal_fits`]) and turns
//! it into shallow water for good ([`Game::terraform`]): a chain of them
//! makes a channel as wide and long as you like.
//! A bridge spans shallow water between two separate stretches of land (see
//! [`Game::bridge_fits`]) and carries a road: land units walk over it
//! without embarking, and ships still sail under it. Everyone uses them all,
//! wherever their borders let them go.

use crate::city::CityId;
use crate::civ::Effect;
use crate::game::{CommandError, Event, Game, Order, PlayerId, UnitId};
use crate::hex::TileId;
use crate::map::Elevation;
use crate::rules::{Ability, ImprovementId, Yields};
use serde::{Deserialize, Serialize};

/// How far [`Game::canal_shortcut`] and [`Game::bridge_shortcut`] look for
/// another way round.
pub const SHORTCUT_SEARCH: i32 = 16;

/// How far land units on a tile that's dug out look for land to stand on.
const DIG_RELOCATE_RANGE: i32 = 4;

/// What a Worker builds on its tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Work {
    Improvement(ImprovementId),
    Road,
    /// A railway over the tile's road.
    Railroad,
}

/// Something a city builds on a tile of its land: a passage (canal, tunnel,
/// bridge) or a border post (fort, watchtower).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Passage {
    Canal,
    Tunnel,
    Bridge,
    Fort,
    Watchtower,
}

impl Passage {
    pub const ALL: [Passage; 5] = [Passage::Canal, Passage::Tunnel, Passage::Bridge, Passage::Fort, Passage::Watchtower];

    pub fn name(self) -> &'static str {
        match self {
            Passage::Canal => "Canal",
            Passage::Tunnel => "Tunnel",
            Passage::Bridge => "Bridge",
            Passage::Fort => "Fort",
            Passage::Watchtower => "Watchtower",
        }
    }

    /// A fort or a watchtower (built on the owner's land further out, see
    /// `PassageDef::reach`), not a way through.
    pub fn is_post(self) -> bool {
        matches!(self, Passage::Fort | Passage::Watchtower)
    }
}

impl crate::rules::Rules {
    pub fn passage(&self, p: Passage) -> &crate::rules::PassageDef {
        match p {
            Passage::Canal => &self.improvements.canal,
            Passage::Tunnel => &self.improvements.tunnel,
            Passage::Bridge => &self.improvements.bridge,
            Passage::Fort => &self.improvements.fort,
            Passage::Watchtower => &self.improvements.watchtower,
        }
    }

    pub fn passage_tech(&self, p: Passage) -> crate::rules::TechId {
        match p {
            Passage::Canal => self.canal_tech(),
            Passage::Tunnel => self.tunnel_tech(),
            Passage::Bridge => self.bridge_tech(),
            Passage::Fort => self.fort_tech(),
            Passage::Watchtower => self.watchtower_tech(),
        }
    }
}

/// What has been built on a tile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileWorks {
    pub improvement: Option<ImprovementId>,
    pub road: bool,
    /// A railway over the road.
    #[serde(default)]
    pub rail: bool,
    /// A tunnel through the mountain (see the module docs).
    #[serde(default)]
    pub tunnel: bool,
    /// A bridge land units walk over (ships sail under it).
    #[serde(default)]
    pub bridge: bool,
    /// A fort or a watchtower (see `Passage::is_post`).
    #[serde(default)]
    pub fort: bool,
    #[serde(default)]
    pub watchtower: bool,
    /// Work started here and turns put into it so far.
    pub progress: Option<(Work, i32)>,
}

impl Game {
    /// Forts and watchtowers with an enemy unit standing on them come down
    /// (end of turn).
    pub(crate) fn pull_down_posts(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        for t in 0..self.works.len() as TileId {
            let w = &self.works[t as usize];
            if !w.fort && !w.watchtower {
                continue;
            }
            let Some(owner) = self.territory[t as usize] else { continue };
            let by = self.units_at(t).find(|u| self.at_war(owner, u.owner) && self.unit_def(u).class == crate::rules::UnitClass::Military && !self.is_embarked(u)).map(|u| u.owner);
            if let Some(by) = by {
                let w = &mut self.works[t as usize];
                let fort = w.fort;
                (w.fort, w.watchtower) = (false, false);
                events.push(Event::TileImproved { tile: t });
                events.push(Event::PostDestroyed { tile: t, owner, by, fort });
                events.extend(self.update_visibility(owner));
            }
        }
        events
    }

    /// The improvement on a tile.
    pub fn improvement_at(&self, tile: TileId) -> Option<ImprovementId> {
        self.works[tile as usize].improvement
    }

    /// Whether a tile has a road (every city does).
    pub fn has_road(&self, tile: TileId) -> bool {
        self.works[tile as usize].road
    }

    /// Whether a tile has a railway (a city does once its owner knows how).
    pub fn has_rail(&self, tile: TileId) -> bool {
        self.works[tile as usize].rail
    }

    /// Lays railways in `player`'s cities (when they learn how, or found one).
    pub fn rail_stations(&mut self, player: PlayerId) {
        if !self.knows(player, self.rules.rail_tech()) {
            return;
        }
        let tiles: Vec<TileId> = self.cities.values().filter(|c| c.owner == player).map(|c| c.tile).collect();
        for t in tiles {
            self.works[t as usize].rail = true;
        }
    }

    /// Whether a tile has a tunnel.
    pub fn has_tunnel(&self, tile: TileId) -> bool {
        self.works[tile as usize].tunnel
    }

    /// Whether a tile has a bridge.
    pub fn has_bridge(&self, tile: TileId) -> bool {
        self.works[tile as usize].bridge
    }

    pub fn has_fort(&self, tile: TileId) -> bool {
        self.works.get(tile as usize).is_some_and(|w| w.fort)
    }

    pub fn has_watchtower(&self, tile: TileId) -> bool {
        self.works.get(tile as usize).is_some_and(|w| w.watchtower)
    }

    /// Tiles with a fort.
    pub fn forts(&self) -> impl Iterator<Item = TileId> + '_ {
        self.works.iter().enumerate().filter(|(_, w)| w.fort).map(|(t, _)| t as TileId)
    }

    /// Whether a land unit on `tile` would be at sea, in boats: on water
    /// that no bridge spans.
    pub fn at_sea(&self, tile: TileId) -> bool {
        self.map.is_water(&self.rules, tile) && !self.has_bridge(tile)
    }

    /// Whether a canal could go on `tile` as far as the land goes: flat land
    /// (any feature there can be cleared) next to water, or next to a tile
    /// some city is digging a canal on that in turn leads to water, so a
    /// channel can be queued a tile at a time from the shore.
    pub fn canal_fits(&self, tile: TileId) -> bool {
        if !self.diggable(tile) {
            return false;
        }
        let queued: std::collections::HashSet<TileId> = self
            .cities
            .values()
            .flat_map(|c| c.queue.iter())
            .filter_map(|b| match *b {
                crate::city::Build::Passage(Passage::Canal, t) if t != tile => Some(t),
                _ => None,
            })
            .collect();
        let mut seen = std::collections::HashSet::from([tile]);
        let mut frontier = vec![tile];
        while let Some(t) = frontier.pop() {
            for n in self.map.grid.neighbors(t) {
                if self.map.is_water(&self.rules, n) {
                    return true;
                }
                if queued.contains(&n) && seen.insert(n) {
                    frontier.push(n);
                }
            }
        }
        false
    }

    /// Land a canal could be dug through: flat, not a city, and any feature
    /// on it can be cleared.
    pub fn diggable(&self, tile: TileId) -> bool {
        let t = self.map.tile(tile);
        !self.map.is_water(&self.rules, tile)
            && t.elevation == Elevation::Flat
            && t.feature.is_none_or(|f| !self.rules.feature(f).impassable && self.rules.feature(f).clear_turns > 0)
            && self.city_at(tile).is_none()
    }

    /// Whether the neighbours of `tile` for which `side` holds form two or
    /// more separate runs around it (off the map counts as neither).
    fn separate_runs(&self, tile: TileId, side: impl Fn(TileId) -> bool) -> bool {
        let hex = self.map.grid.hex(tile);
        let on: Vec<bool> = (0..6).map(|d| self.map.grid.tile(hex.neighbor(d)).is_some_and(&side)).collect();
        (0..6).filter(|&d| on[d] && !on[(d + 5) % 6]).count() >= 2
    }

    /// Land a bridge can land on: land a land unit can walk on (a tunnel's
    /// mountain too) or another bridge.
    pub fn bridge_land(&self, tile: TileId) -> bool {
        if self.map.is_water(&self.rules, tile) {
            return self.has_bridge(tile);
        }
        self.map.land_move_cost(&self.rules, tile).is_some() || self.has_tunnel(tile)
    }

    /// Whether a bridge could go on `tile` as far as the land goes: shallow
    /// water (coast or lake, not the open ocean) with land, or a bridge, on
    /// two sides that don't touch, so it joins two separate stretches of land.
    pub fn bridge_fits(&self, tile: TileId) -> bool {
        let t = self.map.tile(tile);
        self.map.is_water(&self.rules, tile)
            && !self.rules.terrain(t.terrain).deep
            && t.feature.is_none_or(|f| !self.rules.feature(f).impassable)
            && self.separate_runs(tile, |n| self.bridge_land(n))
    }

    /// What a canal on `tile` would save ships if it joined two stretches
    /// of water: the shortest way by water between them without it, in
    /// tiles (`None` when they don't meet within [`SHORTCUT_SEARCH`] tiles:
    /// separate waters, or as good as). Also `None` when it doesn't join
    /// two stretches at all; see [`Self::canal_joins`].
    pub fn canal_shortcut(&self, tile: TileId) -> Option<i32> {
        self.shortcut(tile, |t| self.map.is_water(&self.rules, t))
    }

    /// Whether digging `tile` would join two stretches of water that don't
    /// touch around it.
    pub fn canal_joins(&self, tile: TileId) -> bool {
        self.separate_runs(tile, |n| self.map.is_water(&self.rules, n))
    }

    /// What a bridge on `tile` would save land units: the shortest way over
    /// land (and other bridges) between the stretches it joins without it,
    /// like [`Self::canal_shortcut`].
    pub fn bridge_shortcut(&self, tile: TileId) -> Option<i32> {
        self.shortcut(tile, |t| self.bridge_land(t))
    }

    /// The shortest way between the separate runs of `tile`'s neighbours
    /// that `pass` holds for, over tiles it holds for, without `tile` itself.
    fn shortcut(&self, tile: TileId, pass: impl Fn(TileId) -> bool) -> Option<i32> {
        let wet = |t: TileId| t != tile && pass(t);
        let hex = self.map.grid.hex(tile);
        let ring: Vec<Option<TileId>> = (0..6).map(|d| self.map.grid.tile(hex.neighbor(d)).filter(|&n| wet(n))).collect();
        // The first stretch: the run of wet neighbours that starts after a dry one.
        let start = (0..6).find(|&d| ring[d].is_some() && ring[(d + 5) % 6].is_none())?;
        let mut first = Vec::new();
        let mut d = start;
        while let Some(t) = ring[d % 6].filter(|_| first.len() < 6) {
            first.push(t);
            d += 1;
        }
        let others: Vec<TileId> = ring.iter().flatten().copied().filter(|t| !first.contains(t)).collect();
        let mut seen: std::collections::HashSet<TileId> = first.iter().copied().collect();
        let mut frontier = first;
        for steps in 1..=SHORTCUT_SEARCH {
            let mut next = Vec::new();
            for t in frontier {
                for n in self.map.grid.neighbors(t) {
                    if wet(n) && seen.insert(n) {
                        if others.contains(&n) {
                            return Some(steps);
                        }
                        next.push(n);
                    }
                }
            }
            frontier = next;
        }
        None
    }

    /// Brings the list of passages up to date after `works` was
    /// changed directly (tests, dev tools).
    pub fn rebuild_passages(&mut self) {
        self.passages = Game::find_passages(&self.works);
    }

    /// Whether a tunnel could go on `tile` as far as the land goes: a
    /// mountain (one a glacier doesn't cover).
    pub fn tunnel_fits(&self, tile: TileId) -> bool {
        let t = self.map.tile(tile);
        t.elevation == Elevation::Mountains && !self.map.is_water(&self.rules, tile) && t.feature.is_none_or(|f| !self.rules.feature(f).impassable)
    }

    /// Why `city` couldn't dig `p` on `tile` (the tech aside), if anything
    /// stops it: it goes on its owner's land within reach of the city
    /// (worked by it or by a neighbour), on ground that fits, once,
    /// and not where another city is digging one.
    pub fn passage_problem(&self, city: CityId, p: Passage, tile: TileId) -> Option<&'static str> {
        let c = self.cities.get(&city)?;
        if p.is_post() {
            let reach = self.rules.passage(p).reach;
            if self.territory_owner(tile) != Some(c.owner) || self.map.grid.distance(tile, c.tile) > reach || self.city_at(tile).is_some() {
                return Some("only on your own land, near the city");
            }
        } else if self.territory_owner(tile) != Some(c.owner) || self.map.grid.distance(tile, c.tile) > crate::city::MAX_CITY_RADIUS || tile == c.tile {
            // Any of the owner's cities in reach may dig, not only the one
            // working the tile: a strait between two cities' land is either's.
            return Some("only on your own land, within 3 tiles of the city");
        }
        let post = self.has_fort(tile) || self.has_watchtower(tile);
        let (done, fits) = match p {
            Passage::Canal => (self.map.is_water(&self.rules, tile), self.canal_fits(tile)),
            Passage::Tunnel => (self.has_tunnel(tile), self.tunnel_fits(tile)),
            Passage::Bridge => (self.has_bridge(tile), self.bridge_fits(tile)),
            Passage::Fort | Passage::Watchtower => (post, self.post_fits(tile)),
        };
        if done {
            return Some(match p {
                Passage::Canal => "this is water already",
                Passage::Tunnel => "there's a tunnel here already",
                Passage::Bridge => "there's a bridge here already",
                Passage::Fort | Passage::Watchtower => "there's a fort or watchtower here already",
            });
        }
        if !fits {
            return Some(match p {
                Passage::Canal => "a canal is dug through flat land next to water (or next to a canal being dug)",
                Passage::Tunnel => "tunnels go only through mountains",
                Passage::Bridge => "a bridge goes over shallow water between two separate stretches of land",
                Passage::Fort | Passage::Watchtower => "it goes on open land or hills (not mountains, water, a bridge or a tunnel)",
            });
        }
        let elsewhere = self.cities.values().any(|o| o.id != city && o.queue.contains(&crate::city::Build::Passage(p, tile)));
        if elsewhere {
            return Some("another city is digging it already");
        }
        None
    }

    /// Why `city` couldn't start `p` on `tile`: as [`Self::passage_problem`],
    /// and not where it's building a passage already.
    pub fn site_problem(&self, city: CityId, p: Passage, tile: TileId) -> Option<&'static str> {
        let queued = self.cities.get(&city)?.queue.iter().any(|b| matches!(b, crate::city::Build::Passage(_, t) if *t == tile));
        if queued {
            return Some("this city is building there already");
        }
        self.passage_problem(city, p, tile)
    }

    /// Whether a fort or watchtower could stand on `tile` as far as the land
    /// goes: dry land that isn't a mountain, a tunnel or a bridge.
    pub fn post_fits(&self, tile: TileId) -> bool {
        let w = &self.works[tile as usize];
        self.map.land_move_cost(&self.rules, tile).is_some() && self.map.tile(tile).elevation != Elevation::Mountains && !w.tunnel && !w.bridge
    }

    /// How far from `city` it may build `p`.
    pub fn passage_reach(&self, p: Passage) -> i32 {
        if p.is_post() { self.rules.passage(p).reach } else { crate::city::MAX_CITY_RADIUS }
    }

    /// Every tile where `city` could start `p`.
    pub fn passage_sites(&self, city: CityId, p: Passage) -> Vec<TileId> {
        let Some(c) = self.cities.get(&city) else { return Vec::new() };
        self.map
            .grid
            .within(c.tile, self.passage_reach(p))
            .into_iter()
            .filter(|&t| self.site_problem(city, p, t).is_none())
            .collect()
    }

    /// Puts a finished passage on `tile`.
    pub fn build_passage(&mut self, tile: TileId, p: Passage) -> Vec<Event> {
        if p == Passage::Canal {
            let water = self.rules.canal_water();
            return self.terraform(tile, water);
        }
        let mut events = Vec::new();
        let w = &mut self.works[tile as usize];
        w.progress = None;
        // A tunnel carries a road through the mountain, a bridge over the water.
        match p {
            Passage::Tunnel => (w.tunnel, w.road) = (true, true),
            Passage::Bridge => (w.bridge, w.road) = (true, true),
            Passage::Fort => w.fort = true,
            Passage::Watchtower => w.watchtower = true,
            Passage::Canal => unreachable!("dug above"),
        }
        self.passages = Game::find_passages(&self.works);
        events.push(Event::TileImproved { tile });
        if p.is_post() {
            if let Some(owner) = self.territory_owner(tile) {
                events.push(Event::BordersChanged { player: owner });
                events.extend(self.update_visibility(owner));
            }
        }
        if let Some(city) = self.district(tile) {
            self.assign_worked_tiles(city);
            events.push(Event::CityChanged { city });
        }
        events
    }

    /// Turns land into `water` for good (a canal): the land's feature,
    /// resource, river, improvement and road go with it; land units standing
    /// there move to the nearest land they may stand on (or stay, in boats,
    /// if there's none near); the city whose land it is reconsiders the tiles
    /// it works, and cities beside it may now be on the coast.
    pub fn terraform(&mut self, tile: TileId, water: crate::rules::TerrainId) -> Vec<Event> {
        let t = &mut self.map.tiles[tile as usize];
        (t.terrain, t.elevation, t.feature, t.resource, t.rivers) = (water, Elevation::Flat, None, None, 0);
        self.works[tile as usize] = TileWorks::default();
        self.passages = Game::find_passages(&self.works);
        // Merchants' ways may now run differently.
        self.trade_paths.clear();
        let mut events = vec![Event::TerrainChanged { tile }];
        // Each land unit moves to the nearest land it may stand on.
        let groups: Vec<Vec<UnitId>> =
            self.units_at(tile).filter(|u| self.unit_def(u).domain == crate::rules::Domain::Land).map(|u| vec![u.id]).collect();
        let mut moved = Vec::new();
        let mut near = self.map.grid.within(tile, DIG_RELOCATE_RANGE);
        near.sort_by_key(|&t| (self.map.grid.distance(t, tile), t));
        for group in groups {
            let fits = |t: TileId| {
                !self.at_sea(t)
                    && group.iter().all(|m| {
                        let u = &self.units[m];
                        self.mover(u).can_enter(&self.map, &self.rules, t) && self.can_stop_at(u, t)
                    })
            };
            let Some(to) = near.iter().copied().find(|&t| fits(t)) else { continue };
            for unit in group {
                let u = self.units.get_mut(&unit).expect("exists");
                u.tile = to;
                moved.push(u.owner);
                events.push(Event::UnitMoved { unit, path: vec![to] });
            }
        }
        moved.sort();
        moved.dedup();
        for p in moved {
            events.extend(self.update_visibility(p));
        }
        let mut cities: Vec<CityId> = self.map.grid.neighbors(tile).filter_map(|n| self.city_at(n).map(|c| c.id)).collect();
        cities.extend(self.district(tile));
        cities.sort();
        cities.dedup();
        for city in cities {
            self.assign_worked_tiles(city);
            events.push(Event::CityChanged { city });
        }
        events
    }

    /// Tiles with a tunnel or bridge, in order (kept in step with `works`).
    pub(crate) fn find_passages(works: &[TileWorks]) -> Vec<TileId> {
        works.iter().enumerate().filter(|(_, w)| w.tunnel || w.bridge).map(|(t, _)| t as TileId).collect()
    }

    /// Tiles with a finished tunnel or bridge, in order (canals leave
    /// water, not a passage).
    pub fn passage_tiles(&self, p: Passage) -> Vec<TileId> {
        let has = |w: &TileWorks| match p {
            Passage::Canal => false,
            Passage::Tunnel => w.tunnel,
            Passage::Bridge => w.bridge,
            Passage::Fort | Passage::Watchtower => false,
        };
        self.passages.iter().copied().filter(|&t| has(&self.works[t as usize])).collect()
    }

    /// Yields the improvement on a tile adds.
    pub fn improvement_yields(&self, tile: TileId) -> Yields {
        self.improvement_at(tile).map(|i| self.rules.improvement(i).yields()).unwrap_or_default()
    }

    /// Whether `player` may build improvement `imp` on `tile` as far as the
    /// land goes (not checking who owns it or the tech).
    pub fn improvement_fits(&self, player: PlayerId, tile: TileId, imp: ImprovementId) -> bool {
        let t = self.map.tile(tile);
        let def = self.rules.improvement(imp);
        let water = self.map.is_water(&self.rules, tile);
        if def.water != water || t.elevation == Elevation::Mountains {
            return false;
        }
        if self.resource_needs(player, tile) == Some(imp) {
            return true;
        }
        if def.resource_only {
            return false;
        }
        let terrain = &self.rules.terrain(t.terrain).id;
        let elevation_ok = if def.elevations.is_empty() { t.elevation != Elevation::Mountains } else { def.elevations.contains(&t.elevation) };
        let feature_ok = match def.features.is_empty() {
            true => t.feature.is_none_or(|f| self.rules.feature(f).clear_turns > 0),
            false => t.feature.is_some_and(|f| def.features.contains(&self.rules.feature(f).id)),
        };
        elevation_ok && feature_ok && (def.terrains.is_empty() || def.terrains.contains(terrain))
    }

    /// The improvement that works the resource `player` sees on `tile`, if any.
    fn resource_needs(&self, player: PlayerId, tile: TileId) -> Option<crate::rules::ImprovementId> {
        self.visible_resource(player, tile).and_then(|r| self.rules.resource_improvement(r))
    }

    /// What `tile` would yield `player` with `work` done (a road changes nothing).
    pub fn yields_with_work(&self, player: PlayerId, tile: TileId, work: Work) -> Yields {
        let Work::Improvement(imp) = work else { return self.tile_yields(player, tile) };
        let mut y = self.tile_yields(player, tile) + self.rules.improvement(imp).yields() - self.improvement_yields(tile);
        if self.work_clears(player, tile, work) {
            // Without the feature, the land yields differently.
            let bare = crate::map::Map::yields_of(&self.rules, &crate::map::Tile { feature: None, ..*self.map.tile(tile) });
            y = y + bare - self.map.yields(&self.rules, tile);
        }
        Yields { food: y.food.max(0), production: y.production.max(0), gold: y.gold.max(0) }
    }

    /// Whether building `work` on `tile` clears its feature.
    pub fn work_clears(&self, player: PlayerId, tile: TileId, work: Work) -> bool {
        match work {
            Work::Road | Work::Railroad => false,
            Work::Improvement(imp) => self.map.tile(tile).feature.is_some_and(|f| {
                !self.rules.improvement(imp).features.contains(&self.rules.feature(f).id) && self.resource_needs(player, tile) != Some(imp)
            }),
        }
    }

    /// Turns of work `work` needs on `tile` in all.
    pub fn work_turns(&self, player: PlayerId, tile: TileId, work: Work) -> i32 {
        let clear = match self.work_clears(player, tile, work) {
            true => self.map.tile(tile).feature.map_or(0, |f| self.rules.feature(f).clear_turns),
            false => 0,
        };
        clear
            + match work {
                Work::Road => self.rules.improvements.road.turns,
                Work::Railroad => self.rules.improvements.railroad.turns,
                Work::Improvement(imp) => self.rules.improvement(imp).turns,
            }
    }

    /// Turns of work still needed (progress already made on the tile counts).
    pub fn work_left(&self, player: PlayerId, tile: TileId, work: Work) -> i32 {
        let done = match self.works[tile as usize].progress {
            Some((w, n)) if w == work => n,
            _ => 0,
        };
        (self.work_turns(player, tile, work) - done).max(1)
    }

    /// Whether a unit of this type can build `work` at all.
    pub fn can_do_work(&self, unit: UnitId, work: Work) -> bool {
        let Some(u) = self.units.get(&unit) else { return false };
        let def = self.unit_def(u);
        match work {
            Work::Improvement(_) | Work::Railroad => def.has(Ability::BuildImprovements),
            Work::Road => def.has(Ability::BuildImprovements) || def.has(Ability::BuildRoads),
        }
    }

    /// Why `unit` can't build `work` where it stands, if it can't.
    pub fn work_problem(&self, unit: UnitId, work: Work) -> Option<&'static str> {
        let Some(u) = self.units.get(&unit) else { return Some("no such unit") };
        if !self.can_do_work(unit, work) {
            return Some("this unit can't build that");
        }
        self.tile_work_problem(u.owner, u.tile, work)
    }

    /// Why a Worker of `owner`'s couldn't build `work` on `tile`, if it couldn't.
    pub fn tile_work_problem(&self, owner: PlayerId, tile: TileId, work: Work) -> Option<&'static str> {
        let on_water = matches!(work, Work::Improvement(imp) if self.rules.improvement(imp).water);
        if on_water != self.map.is_water(&self.rules, tile) || (!on_water && self.map.land_move_cost(&self.rules, tile).is_none()) {
            return Some("nothing can be built here");
        }
        if self.city_at(tile).is_some() {
            return Some("not in a city");
        }
        let territory = self.territory_owner(tile);
        let works = &self.works[tile as usize];
        match work {
            Work::Road => {
                if works.road {
                    return Some("there's a road here already");
                }
                if territory.is_some_and(|o| o != owner) {
                    return Some("not in someone else's land");
                }
            }
            Work::Railroad => {
                if works.rail {
                    return Some("there's a railway here already");
                }
                if !self.knows(owner, self.rules.rail_tech()) {
                    return Some("needs a technology you don't have");
                }
                if !works.road {
                    return Some("needs a road first");
                }
                if territory.is_some_and(|o| o != owner) {
                    return Some("not in someone else's land");
                }
            }
            Work::Improvement(imp) => {
                if works.improvement == Some(imp) {
                    return Some("already built here");
                }
                if territory != Some(owner) {
                    return Some("only inside your borders");
                }
                if self.rules.improvement_tech(imp).is_some_and(|t| !self.knows(owner, t)) {
                    return Some("needs a technology you don't have");
                }
                if !self.improvement_fits(owner, tile, imp) {
                    return Some(if self.rules.improvement(imp).resource_only {
                        "only on a resource it works"
                    } else {
                        "can't be built on this land"
                    });
                }
            }
        }
        None
    }

    /// What `unit` could build where it stands now.
    pub fn available_works(&self, unit: UnitId) -> Vec<Work> {
        let n = self.rules.improvements.improvements.len() as ImprovementId;
        (0..n).map(Work::Improvement).chain([Work::Road, Work::Railroad]).filter(|&w| self.work_problem(unit, w).is_none()).collect()
    }

    /// `Command::Build`: the unit starts (or carries on with) `work` on its tile.
    pub(crate) fn start_work(&mut self, unit: UnitId, work: Work) -> Result<Vec<Event>, CommandError> {
        if let Some(why) = self.work_problem(unit, work) {
            return Err(CommandError::CantBuild(why));
        }
        let u = self.units.get_mut(&unit).expect("checked");
        if u.moves <= 0 {
            return Err(CommandError::NoMovesLeft);
        }
        u.moves = 0;
        let order = match u.order {
            Order::Automate | Order::AutoBuild(_) => Order::AutoBuild(work),
            _ => Order::Build(work),
        };
        u.order = order;
        Ok(vec![Event::OrderChanged { unit, order }])
    }

    /// Turn end: every unit with a build order adds a turn of work.
    pub(crate) fn process_works(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let builders: Vec<(UnitId, Work, Order)> = self
            .units
            .values()
            .filter_map(|u| match u.order {
                Order::Build(w) => Some((u.id, w, Order::None)),
                Order::AutoBuild(w) => Some((u.id, w, Order::Automate)),
                _ => None,
            })
            .collect();
        for (unit, work, after) in builders {
            if self.work_problem(unit, work).is_some() {
                self.units.get_mut(&unit).expect("exists").order = after;
                events.push(Event::OrderChanged { unit, order: after });
                continue;
            }
            let (tile, owner) = (self.units[&unit].tile, self.units[&unit].owner);
            let needed = self.work_turns(owner, tile, work);
            let done = match self.works[tile as usize].progress {
                Some((w, n)) if w == work => n + 1,
                _ => 1,
            };
            if done < needed {
                self.works[tile as usize].progress = Some((work, done));
                continue;
            }
            events.extend(self.finish_work(owner, tile, work));
            self.units.get_mut(&unit).expect("exists").order = after;
            events.push(Event::OrderChanged { unit, order: after });
        }
        events
    }

    /// Puts `work` on `tile` for `owner`, clearing what it replaces.
    fn finish_work(&mut self, owner: PlayerId, tile: TileId, work: Work) -> Vec<Event> {
        let mut events = Vec::new();
        if self.work_clears(owner, tile, work) {
            let feature = self.map.tiles[tile as usize].feature.take().expect("clears a feature");
            let production = self.rules.feature(feature).clear_production;
            if let Some(city) = self.nearest_city(owner, tile).filter(|_| production > 0) {
                self.cities.get_mut(&city).expect("exists").production += production;
                events.push(Event::CityChanged { city });
            }
        }
        let w = &mut self.works[tile as usize];
        w.progress = None;
        match work {
            Work::Road => w.road = true,
            Work::Railroad => w.rail = true,
            Work::Improvement(imp) => w.improvement = Some(imp),
        }
        events.push(Event::TileImproved { tile });
        if let Some(city) = self.district(tile) {
            self.assign_worked_tiles(city);
            events.push(Event::CityChanged { city });
        }
        events
    }

    /// `player`'s city closest to `tile`.
    pub(crate) fn nearest_city(&self, player: PlayerId, tile: TileId) -> Option<CityId> {
        self.cities.values().filter(|c| c.owner == player).min_by_key(|c| (self.map.grid.distance(c.tile, tile), c.id)).map(|c| c.id)
    }

    /// Rome's perk: a new city gets a road to the capital, over land that's
    /// its own or no one's.
    pub(crate) fn road_to_capital(&mut self, city: CityId) {
        let owner = self.cities[&city].owner;
        if !self.civ_has(owner, |e| matches!(e, Effect::RoadToCapital)) {
            return;
        }
        let Some(capital) = self.cities.values().filter(|c| c.owner == owner && c.id != city).min_by_key(|c| c.id).map(|c| c.tile) else {
            return;
        };
        let Some(worker) = self.rules.unit_type_id("worker") else { return };
        let mover = crate::path::Mover::new(self.rules.unit(worker));
        let from = self.cities[&city].tile;
        let foreign = |t: TileId| self.territory_owner(t).is_some_and(|o| o != owner);
        // Roads make the search prefer existing roads, so new ones join them.
        let road = |a: TileId, b: TileId| (self.has_road(a) && self.has_road(b)).then_some(crate::path::MOVE);
        let path = crate::path::find_path_zoc(&self.map, &self.rules, &mover, from, capital, mover.moves, foreign, |_| false, road, |_| false);
        for t in path.map(|p| p.tiles).unwrap_or_default() {
            self.works[t as usize].road = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Command, GameOptions, GameSetup};
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Rules;

    /// A game with player 0's city founded and a worker next to it.
    fn game_with_worker(civ: &str) -> (Game, CityId, UnitId) {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let civs = [civ, "egypt"].iter().map(|c| rules.civ_id(c).unwrap()).collect();
        let mut g = Game::with_setup(rules, map, &GameSetup { civs, options: GameOptions::default(), starts: Vec::new() });
        g.players[0].techs.fill(true);
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let city = *g.cities.keys().next().unwrap();
        let tile = g.cities[&city].tile;
        let worker = g.spawn_unit(0, g.rules.unit_type_id("worker").unwrap(), tile);
        (g, city, worker)
    }

    fn put(g: &mut Game, unit: UnitId, tile: TileId) {
        g.units.get_mut(&unit).unwrap().tile = tile;
    }

    /// A tile of the city's land where `pred` holds.
    fn land(g: &Game, city: CityId, pred: impl Fn(&Game, TileId) -> bool) -> Option<TileId> {
        let c = &g.cities[&city];
        g.map.grid.within(c.tile, 1).into_iter().find(|&t| t != c.tile && g.district(t) == Some(city) && pred(g, t))
    }

    /// A plain grassland tile of the city's.
    fn farmland(g: &mut Game, city: CityId) -> TileId {
        let tile = land(g, city, |_, _| true).unwrap();
        let grass = g.rules.terrain_id("grassland").unwrap();
        let t = &mut g.map.tiles[tile as usize];
        (t.terrain, t.elevation, t.feature, t.resource) = (grass, Elevation::Flat, None, None);
        tile
    }

    #[test]
    fn a_lumber_mill_keeps_the_forest() {
        let (mut g, city, worker) = game_with_worker("netherlands");
        let mill = g.rules.improvement_id("lumber_mill").unwrap();
        let tile = farmland(&mut g, city);
        put(&mut g, worker, tile);
        assert!(g.work_problem(worker, Work::Improvement(mill)).is_some(), "only in forests");
        let forest = g.rules.feature_id("forest").unwrap();
        g.map.tiles[tile as usize].feature = Some(forest);
        let before = g.tile_yields(0, tile);
        g.apply(0, Command::Build { unit: worker, work: Work::Improvement(mill) }).unwrap();
        for _ in 0..6 {
            g.apply(0, Command::EndTurn).unwrap();
        }
        assert_eq!(g.improvement_at(tile), Some(mill));
        assert_eq!(g.map.tile(tile).feature, Some(forest));
        assert_eq!(g.tile_yields(0, tile).production, before.production + 1);
    }

    #[test]
    fn a_farm_takes_turns_and_adds_food() {
        let (mut g, city, worker) = game_with_worker("netherlands");
        let farm = g.rules.improvement_id("farm").unwrap();
        // Without Civil Service's extra food on farms.
        let civil_service = g.rules.tech_id("civil_service").unwrap();
        g.players[0].techs[civil_service as usize] = false;
        let tile = farmland(&mut g, city);
        put(&mut g, worker, tile);
        let before = g.tile_yields(0, tile);
        g.apply(0, Command::Build { unit: worker, work: Work::Improvement(farm) }).unwrap();
        assert_eq!(g.units[&worker].moves, 0);
        for _ in 0..4 {
            g.apply(0, Command::EndTurn).unwrap();
            assert_eq!(g.improvement_at(tile), None);
        }
        let events = g.apply(0, Command::EndTurn).unwrap();
        assert!(events.contains(&Event::TileImproved { tile }));
        assert_eq!(g.improvement_at(tile), Some(farm));
        assert_eq!(g.tile_yields(0, tile).food, before.food + 1);
        assert_eq!(g.units[&worker].order, Order::None, "done: asks for orders again");
    }

    #[test]
    fn progress_stays_on_the_tile() {
        let (mut g, city, worker) = game_with_worker("netherlands");
        let farm = g.rules.improvement_id("farm").unwrap();
        let tile = farmland(&mut g, city);
        put(&mut g, worker, tile);
        let work = Work::Improvement(farm);
        g.apply(0, Command::Build { unit: worker, work }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        g.apply(0, Command::SetOrder { unit: worker, order: Order::None }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert_eq!(g.work_left(0, tile, work), g.work_turns(0, tile, work) - 2);
    }

    #[test]
    fn improvements_need_your_land_and_the_right_ground() {
        let (mut g, city, worker) = game_with_worker("netherlands");
        let mine = g.rules.improvement_id("mine").unwrap();
        let pasture = g.rules.improvement_id("pasture").unwrap();
        let flat = land(&g, city, |g, t| g.map.tile(t).elevation == Elevation::Flat && g.map.tile(t).resource.is_none());
        if let Some(flat) = flat {
            put(&mut g, worker, flat);
            assert!(g.work_problem(worker, Work::Improvement(mine)).is_some(), "mines go on hills");
            assert!(g.work_problem(worker, Work::Improvement(pasture)).is_some(), "pastures need a resource");
        }
        let outside = (0..g.map.tiles.len() as TileId)
            .find(|&t| g.territory[t as usize].is_none() && g.map.land_move_cost(&g.rules, t).is_some())
            .unwrap();
        put(&mut g, worker, outside);
        let farm = g.rules.improvement_id("farm").unwrap();
        assert_eq!(g.work_problem(worker, Work::Improvement(farm)), Some("only inside your borders"));
        assert_eq!(g.work_problem(worker, Work::Road), None, "roads go anywhere unclaimed");
    }

    #[test]
    fn clearing_a_forest_takes_longer_and_feeds_the_city() {
        let (mut g, city, worker) = game_with_worker("netherlands");
        let forest = g.rules.feature_id("forest").unwrap();
        let farm = g.rules.improvement_id("farm").unwrap();
        let grass = g.rules.terrain_id("grassland").unwrap();
        let tile = land(&g, city, |_, _| true).unwrap();
        let t = &mut g.map.tiles[tile as usize];
        (t.terrain, t.elevation, t.feature, t.resource) = (grass, Elevation::Flat, Some(forest), None);
        put(&mut g, worker, tile);
        let work = Work::Improvement(farm);
        assert_eq!(g.work_turns(0, tile, work), 5 + 3);
        g.apply(0, Command::Build { unit: worker, work }).unwrap();
        g.apply(0, Command::SetQueue { city, queue: vec![] }).unwrap();
        g.apply(0, Command::SetAutoQueue { city, on: false }).unwrap();
        for _ in 0..7 {
            g.apply(0, Command::EndTurn).unwrap();
        }
        let stored = g.cities[&city].production;
        g.apply(0, Command::EndTurn).unwrap();
        assert_eq!(g.map.tile(tile).feature, None);
        assert_eq!(g.improvement_at(tile), Some(farm));
        assert_eq!(g.cities[&city].production, stored + 20);
    }

    #[test]
    fn roads_make_rough_ground_cheap() {
        let (mut g, city, worker) = game_with_worker("netherlands");
        let hills: Vec<TileId> = g.map.grid.neighbors(g.cities[&city].tile).collect();
        for &t in &hills {
            g.map.tiles[t as usize].elevation = Elevation::Hills;
            g.map.tiles[t as usize].feature = None;
        }
        let (a, b) = (hills[0], g.map.grid.neighbors(hills[0]).find(|n| hills.contains(n)).unwrap());
        put(&mut g, worker, a);
        let mover = g.mover(&g.units[&worker]);
        let unit = g.units[&worker].clone();
        let point = crate::path::MOVE;
        assert_eq!(g.step_cost(&unit, &mover, a, b).map(|c| c.0), Some(2 * point));
        g.works[a as usize].road = true;
        g.works[b as usize].road = true;
        assert_eq!(g.step_cost(&unit, &mover, a, b).map(|c| c.0), Some(point));
        // Cities count as roads.
        let home = g.cities[&city].tile;
        assert_eq!(g.step_cost(&unit, &mover, home, a).map(|c| c.0), Some(point));
    }

    #[test]
    fn railways_go_over_roads_and_are_fast() {
        let (mut g, city, worker) = game_with_worker("netherlands");
        let home = g.cities[&city].tile;
        let a = g.map.grid.neighbors(home).find(|&t| g.map.land_move_cost(&g.rules, t).is_some()).unwrap();
        put(&mut g, worker, a);
        let rail = g.rules.rail_tech();
        g.players[0].techs[rail as usize] = false;
        assert_eq!(g.work_problem(worker, Work::Railroad), Some("needs a technology you don't have"));
        g.players[0].techs[rail as usize] = true;
        assert_eq!(g.work_problem(worker, Work::Railroad), Some("needs a road first"));
        g.works[a as usize].road = true;
        assert_eq!(g.work_problem(worker, Work::Railroad), None);
        g.apply(0, Command::Build { unit: worker, work: Work::Railroad }).unwrap();
        for _ in 0..g.rules.improvements.railroad.turns {
            g.apply(0, Command::EndTurn).unwrap();
        }
        assert!(g.has_rail(a));
        // A city has a station once its owner knows how: founded after
        // that, or when they learn it.
        assert!(g.has_rail(home), "founded knowing Railroad");
        g.works[home as usize].rail = false;
        let mover = g.mover(&g.units[&worker]);
        let unit = g.units[&worker].clone();
        assert_eq!(g.step_cost(&unit, &mover, a, home).map(|c| c.0), Some(crate::path::MOVE), "the city has no railway yet");
        g.rail_stations(0);
        assert_eq!(g.step_cost(&unit, &mover, a, home).map(|c| c.0), Some(crate::path::RAIL_COST));
    }

    #[test]
    fn rome_links_new_cities_to_the_capital() {
        let (mut g, city, _) = game_with_worker("rome");
        let home = g.cities[&city].tile;
        let site = (0..g.map.tiles.len() as TileId)
            .filter(|&t| g.map.grid.distance(t, home) == 5 && g.found_city_problem(0, t).is_none())
            .find(|&t| g.path_for(g.units.values().find(|u| u.owner == 0).unwrap().id, t).is_some())
            .unwrap();
        let settler = g.spawn_unit(0, g.rules.unit_type_id("settler").unwrap(), site);
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let roads = g.works.iter().filter(|w| w.road).count();
        assert!(roads >= 4, "{roads} road tiles");
    }

    /// `city` digs `p` on `tile`, all at once.
    fn dig(g: &mut Game, city: CityId, p: Passage, tile: TileId) {
        let build = crate::city::Build::Passage(p, tile);
        g.apply(0, Command::SetQueue { city, queue: vec![build] }).unwrap();
        g.cities.get_mut(&city).unwrap().production = g.build_cost(build);
        g.apply(0, Command::EndTurn).unwrap();
    }

    #[test]
    fn cities_dig_tunnels_through_their_mountains_for_everyone() {
        let (mut g, city, _) = game_with_worker("netherlands");
        let tile = farmland(&mut g, city);
        g.map.tiles[tile as usize].elevation = Elevation::Mountains;
        let warrior = g.spawn_unit(0, g.rules.unit_type_id("warrior").unwrap(), g.cities[&city].tile);
        assert!(g.path_for(warrior, tile).is_none(), "mountains are impassable");
        assert!(g.passage_problem(city, Passage::Canal, tile).is_some(), "no canals in the mountains");
        assert!(g.passage_sites(city, Passage::Tunnel).contains(&tile));
        dig(&mut g, city, Passage::Tunnel, tile);
        assert!(g.has_tunnel(tile) && g.has_road(tile), "a tunnel carries a road");
        assert!(g.cities[&city].queue.is_empty());
        assert!(g.path_for(warrior, tile).is_some(), "the tunnel lets armies through");
        assert_eq!(g.passage_problem(city, Passage::Tunnel, tile), Some("there's a tunnel here already"));
    }

    #[test]
    fn any_city_in_reach_may_dig_on_its_neighbours_land() {
        let (mut g, city, _) = game_with_worker("netherlands");
        let home = g.cities[&city].tile;
        let site = (0..g.map.tiles.len() as TileId)
            .filter(|&t| g.map.grid.distance(t, home) == 5 && g.found_city_problem(0, t).is_none())
            .find(|&t| g.map.grid.within(t, 3).iter().any(|&n| g.map.grid.distance(n, home) <= 3 && !g.map.is_water(&g.rules, n)))
            .unwrap();
        let settler = g.spawn_unit(0, g.rules.unit_type_id("settler").unwrap(), site);
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let other = g.city_at(site).unwrap().id;
        // Land between them that the new city is nearer to, so it works it.
        let tile = g
            .map
            .grid
            .within(site, 2)
            .into_iter()
            .find(|&t| g.map.grid.distance(t, home) == 3 && !g.map.is_water(&g.rules, t))
            .unwrap();
        g.set_territory(tile, Some(0));
        g.map.tiles[tile as usize].elevation = Elevation::Mountains;
        assert_eq!(g.district(tile), Some(other));
        assert_eq!(g.passage_problem(city, Passage::Tunnel, tile), None, "the older city may dig there too");
        assert!(g.passage_sites(city, Passage::Tunnel).contains(&tile));
        dig(&mut g, city, Passage::Tunnel, tile);
        assert!(g.has_tunnel(tile));
    }

    #[test]
    fn canals_join_two_waters_and_let_ships_through() {
        let (mut g, city, worker) = game_with_worker("netherlands");
        let tile = farmland(&mut g, city);
        let (coast, grass) = (g.rules.terrain_id("coast").unwrap(), g.rules.terrain_id("grassland").unwrap());
        let hex = g.map.grid.hex(tile);
        let side = |d: usize| g.map.grid.tile(hex.neighbor(d)).unwrap();
        // Water on two opposite sides, away from the city.
        let c = (0..6).find(|&d| g.city_at(side(d)).is_some()).unwrap();
        let (w, e) = ((c + 1) % 6, (c + 4) % 6);
        let (west, east) = (side(w), side(e));
        for d in 0..6 {
            let n = side(d);
            if d != c {
                let t = &mut g.map.tiles[n as usize];
                (t.terrain, t.elevation, t.feature) = (if d == w || d == e { coast } else { grass }, Elevation::Flat, None);
            }
        }
        assert!(g.canal_fits(tile) && g.canal_joins(tile));
        assert!(g.canal_shortcut(tile).is_none_or(|d| d > 2), "the two coasts aren't joined nearby");
        // Water on one side only: it can still be dug, but joins nothing yet.
        g.map.tiles[east as usize].terrain = grass;
        assert!(g.canal_fits(tile) && !g.canal_joins(tile));
        g.map.tiles[east as usize].terrain = coast;
        let galley = g.spawn_unit(0, g.rules.unit_type_id("galley").unwrap(), west);
        assert!(g.path_for(galley, east).is_none() || g.path_for(galley, east).unwrap().tiles.len() > 2, "no short cut yet");
        // Not on someone else's land.
        g.set_territory(tile, None);
        assert_eq!(g.passage_problem(city, Passage::Canal, tile), Some("only on your own land, within 3 tiles of the city"));
        g.set_territory(tile, Some(0));
        let farm = g.rules.improvement_id("farm").unwrap();
        (g.works[tile as usize].improvement, g.works[tile as usize].road) = (Some(farm), true);
        dig(&mut g, city, Passage::Canal, tile);
        assert_eq!(g.map.tile(tile).terrain, g.rules.canal_water(), "the land is water now");
        assert_eq!(g.works[tile as usize], TileWorks::default(), "the farm and road went with it");
        assert!(g.cities[&city].queue.is_empty());
        assert_eq!(g.path_for(galley, east).unwrap().tiles, vec![tile, east], "ships sail through the canal");
        assert!(g.is_coastal(g.cities[&city].tile), "the city is on the water now");
        assert_eq!(g.passage_problem(city, Passage::Canal, tile), Some("this is water already"));
        put(&mut g, worker, tile);
        assert!(g.work_problem(worker, Work::Improvement(farm)).is_some(), "no farms on water");
    }

    #[test]
    fn canals_are_dug_a_tile_at_a_time_from_the_shore() {
        let (mut g, city, _) = game_with_worker("netherlands");
        let home = g.cities[&city].tile;
        let (coast, grass) = (g.rules.terrain_id("coast").unwrap(), g.rules.terrain_id("grassland").unwrap());
        // Dry land all round the city, its own, with the sea three tiles east.
        for t in g.map.grid.within(home, 4) {
            let tile = &mut g.map.tiles[t as usize];
            (tile.terrain, tile.elevation, tile.feature, tile.resource, tile.rivers) = (grass, Elevation::Flat, None, None, 0);
            if g.map.grid.distance(t, home) <= 3 {
                g.set_territory(t, Some(0));
            }
        }
        let step = |t: TileId| g.map.grid.tile(g.map.grid.hex(t).neighbor(0)).unwrap();
        let (near, far) = (step(home), step(step(home)));
        let sea = step(far);
        g.map.tiles[sea as usize].terrain = coast;
        assert!(!g.is_coastal(home));
        assert!(g.canal_fits(far), "next to the sea");
        assert!(!g.canal_fits(near), "nowhere near water yet");
        // Queued behind the tile by the sea, the next one can go too.
        let (dig_far, dig_near) = (crate::city::Build::Passage(Passage::Canal, far), crate::city::Build::Passage(Passage::Canal, near));
        g.apply(0, Command::SetQueue { city, queue: vec![dig_far, dig_near] }).unwrap();
        assert!(g.canal_fits(near), "the canal being dug leads to the sea");
        let queued = vec![dig_far, dig_near];
        let sites = g.passage_sites(city, Passage::Canal);
        assert!(!sites.contains(&far) && !sites.contains(&near), "queued already");
        g.apply(0, Command::SetQueue { city, queue: vec![dig_far, dig_near] }).unwrap();
        assert_eq!(g.cities[&city].queue, queued);
        // A warrior standing in the way climbs out onto land.
        let warrior = g.spawn_unit(0, g.rules.unit_type_id("warrior").unwrap(), far);
        let cost = g.build_cost(dig_far);
        g.cities.get_mut(&city).unwrap().production = cost;
        let events = g.apply(0, Command::EndTurn).unwrap();
        assert!(events.contains(&Event::TerrainChanged { tile: far }));
        assert!(g.map.is_water(&g.rules, far));
        let w = g.units[&warrior].tile;
        assert!(w != far && !g.map.is_water(&g.rules, w), "the warrior stands on land");
        assert_eq!(g.cities[&city].queue, vec![dig_near], "the next tile stays queued");
        g.cities.get_mut(&city).unwrap().production = cost;
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.map.is_water(&g.rules, near));
        assert!(g.is_coastal(home), "the canal brought the sea to the city");
        let galley = g.spawn_unit(0, g.rules.unit_type_id("galley").unwrap(), sea);
        assert!(g.path_for(galley, home).is_some(), "ships sail up the canal into the city");
    }

    #[test]
    fn a_queued_canal_cut_off_from_the_water_is_dropped() {
        let (mut g, city, _) = game_with_worker("netherlands");
        let home = g.cities[&city].tile;
        let (coast, grass) = (g.rules.terrain_id("coast").unwrap(), g.rules.terrain_id("grassland").unwrap());
        for t in g.map.grid.within(home, 4) {
            let tile = &mut g.map.tiles[t as usize];
            (tile.terrain, tile.elevation, tile.feature, tile.resource, tile.rivers) = (grass, Elevation::Flat, None, None, 0);
            if g.map.grid.distance(t, home) <= 3 {
                g.set_territory(t, Some(0));
            }
        }
        let step = |t: TileId| g.map.grid.tile(g.map.grid.hex(t).neighbor(0)).unwrap();
        let (near, far) = (step(home), step(step(home)));
        g.map.tiles[step(far) as usize].terrain = coast;
        let (dig_far, dig_near) = (crate::city::Build::Passage(Passage::Canal, far), crate::city::Build::Passage(Passage::Canal, near));
        g.apply(0, Command::SetQueue { city, queue: vec![dig_near, dig_far] }).unwrap();
        // Taking the tile by the sea out of the queue leaves the other leading nowhere.
        g.cities.get_mut(&city).unwrap().queue.retain(|&b| b != dig_far);
        g.apply(0, Command::EndTurn).unwrap();
        assert!(!g.cities[&city].queue.contains(&dig_near));
    }

    #[test]
    fn bridges_carry_armies_over_water_and_ships_under_them() {
        let (mut g, city, _) = game_with_worker("netherlands");
        let tile = farmland(&mut g, city);
        let (coast, grass) = (g.rules.terrain_id("coast").unwrap(), g.rules.terrain_id("grassland").unwrap());
        let hex = g.map.grid.hex(tile);
        let sides: Vec<TileId> = (0..6).map(|d| g.map.grid.tile(hex.neighbor(d)).unwrap()).collect();
        let side = |d: usize| sides[d];
        // A strait: the city on one side, land opposite, water between.
        let c = (0..6).find(|&d| g.city_at(side(d)).is_some()).unwrap();
        let far = side((c + 3) % 6);
        for d in (0..6).filter(|&d| d != c) {
            let t = &mut g.map.tiles[side(d) as usize];
            (t.terrain, t.elevation, t.feature) = (if d == (c + 3) % 6 { grass } else { coast }, Elevation::Flat, None);
        }
        let t = &mut g.map.tiles[tile as usize];
        (t.terrain, t.elevation, t.feature) = (coast, Elevation::Flat, None);
        assert!(g.bridge_fits(tile) && !g.canal_fits(tile));
        assert_eq!(g.passage_problem(city, Passage::Tunnel, tile), Some("tunnels go only through mountains"));
        // Land on one side only joins nothing.
        g.map.tiles[far as usize].terrain = coast;
        assert!(!g.bridge_fits(tile));
        g.map.tiles[far as usize].terrain = grass;

        let warrior = g.spawn_unit(0, g.rules.unit_type_id("warrior").unwrap(), g.cities[&city].tile);
        let galley = g.spawn_unit(0, g.rules.unit_type_id("galley").unwrap(), side((c + 2) % 6));
        dig(&mut g, city, Passage::Bridge, tile);
        assert!(g.has_bridge(tile) && g.has_road(tile), "a bridge carries a road");
        assert_eq!(g.passage_problem(city, Passage::Bridge, tile), Some("there's a bridge here already"));
        assert_eq!(g.path_for(warrior, far).unwrap().tiles, vec![tile, far], "armies walk over the bridge");
        g.apply(0, Command::MoveUnit { unit: warrior, to: tile }).unwrap();
        let u = &g.units[&warrior];
        assert!(u.tile == tile && !g.is_embarked(u) && u.moves > 0, "stepping onto a bridge isn't embarking");
        // Ships still sail under it, past the army standing on it.
        assert_eq!(g.path_for(galley, side((c + 4) % 6)).unwrap().tiles, vec![tile, side((c + 4) % 6)], "ships sail under the bridge");
    }

    /// Our city with land out to two tiles, and a Roman warrior at war with
    /// us two tiles away.
    fn war_on_our_land() -> (Game, CityId, TileId, UnitId) {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 1);
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(crate::rules::Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let city = *g.cities.keys().next().unwrap();
        let home = g.cities[&city].tile;
        for t in g.map.grid.within(home, 2) {
            if g.territory[t as usize].is_none() {
                g.territory[t as usize] = Some(0);
            }
        }
        g.refresh_districts();
        let at = g.map.grid.within(home, 2).into_iter().find(|&t| g.map.grid.distance(t, home) == 2 && g.map.land_move_cost(&g.rules, t).is_some()).unwrap();
        g.units.retain(|_, u| u.owner == 0 || g.map.grid.distance(u.tile, home) > 6);
        let warrior = g.spawn_unit(1, g.rules.unit_type_id("warrior").unwrap(), at);
        for (a, b) in [(0, 1), (1, 0)] {
            g.relation_mut(a, b).at_war = true;
        }
        (g, city, at, warrior)
    }

    #[test]
    fn an_enemy_on_a_fort_pulls_it_down() {
        let (mut g, _, at, _) = war_on_our_land();
        g.works[at as usize].fort = true;
        let events = g.pull_down_posts();
        assert!(!g.has_fort(at));
        assert!(events.iter().any(|e| matches!(e, Event::PostDestroyed { by: 1, fort: true, .. })));
    }

    #[test]
    fn forts_fire_at_enemies_next_to_them_and_watchtowers_see_far() {
        let (mut g, city, at, warrior) = war_on_our_land();
        let home = g.cities[&city].tile;
        let fort = g.map.grid.neighbors(at).find(|&t| g.post_fits(t) && g.city_at(t).is_none() && g.units_at(t).next().is_none() && g.map.grid.distance(t, home) >= 2).unwrap();
        g.works[fort as usize].fort = true;
        g.update_visibility(0);
        let events = g.city_strikes();
        assert!(events.iter().any(|e| matches!(e, Event::Combat(r) if r.attacker == crate::combat::Attacker::Fort(fort))));
        assert!(g.units[&warrior].hp < crate::combat::MAX_HP);
        // A watchtower sees further than the city does.
        let tower = g.map.grid.within(home, 3).into_iter().find(|&t| g.map.grid.distance(t, home) == 2 && g.post_fits(t)).unwrap();
        let before = g.players[0].visible.iter().filter(|&&v| v > 0).count();
        g.works[tower as usize].watchtower = true;
        g.update_visibility(0);
        assert!(g.players[0].visible.iter().filter(|&&v| v > 0).count() > before);
    }

    #[test]
    fn cities_build_forts_on_their_land_further_out() {
        let (mut g, city, _, _) = war_on_our_land();
        g.players[0].techs.fill(true);
        let home = g.cities[&city].tile;
        let near = g.map.grid.neighbors(home).find(|&t| g.post_fits(t)).unwrap();
        assert_eq!(g.passage_problem(city, Passage::Fort, near), None);
        let far = g.map.grid.within(home, 5).into_iter().find(|&t| g.map.grid.distance(t, home) == 5 && g.post_fits(t)).unwrap();
        assert!(g.passage_problem(city, Passage::Fort, far).is_some(), "not on nobody's land");
        g.set_territory(far, Some(0));
        assert_eq!(g.passage_problem(city, Passage::Fort, far), None, "on ours, five tiles out");
        g.build_passage(near, Passage::Fort);
        assert!(g.has_fort(near));
        assert!(g.passage_problem(city, Passage::Watchtower, near).is_some(), "one post to a tile");
    }
}
