//! Where bridges run: the deck's line across its tile, from the middle of
//! one shore edge to the middle of another (bent at the tile's centre when
//! they aren't opposite), and its height along the way, rising from the
//! shores to arches tall enough for boats. Shared by the bridge's model
//! (`improvements.rs`, from props made in `art/models/bridges.py`, whose
//! deck this must match) and land units walking over it (`units.rs`); ships
//! stay on the water and moor beside the bridge.

use crate::layout::{hex_at, tile_center};
use crate::loading::InGameSet;
use crate::play::{PlaySet, Session};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::wrap::nearest_shift;
use bevy::prelude::*;
use sim::game::Game;
use sim::hex::TileId;
use sim::improvement::Passage;
use std::collections::HashMap;

/// The deck's height above the water at its middle and at the shores (as
/// `TOP` and `END` in `bridges.py`, which is in game units x 100).
const DECK_TOP: f32 = 0.36;
const DECK_END: f32 = 0.05;
/// The deck's width, parapets included.
const DECK_WIDTH: f32 = 0.3;
/// How far beside the deck units still count as on it (they stand a little
/// off its line while walking past each other).
const DECK_REACH: f32 = DECK_WIDTH * 0.5 + 0.1;

/// How far from a tile's centre to an edge's midpoint: an arm's length in
/// `bridges.py` (`ARM`), which arms are stretched from to meet the shore.
pub const ARM: f32 = crate::layout::SQRT3 * 0.5;
/// The shortest and longest an arm may be stretched to.
const MIN_REACH: f32 = 0.6;
const MAX_REACH: f32 = 1.85;
/// Ground this far above the water is the shore an arm lands on.
const SHORE: f32 = 0.03;

/// How far from `centre` an arm towards edge `dir` must reach to land on
/// the drawn shore: coasts are smoothed, so it can lie well inside the land
/// tile, or reach into the water one.
fn reach(terrain: &Terrain, centre: Vec2, dir: usize) -> f32 {
    let d = edge_point(dir).normalize();
    let mut r = 0.4;
    while r < MAX_REACH {
        if terrain.heights.sample(centre + d * r) >= WATER_LEVEL + SHORE {
            // A little into the bank, so the end doesn't stop short of it.
            return (r + 0.06).clamp(MIN_REACH, MAX_REACH);
        }
        r += 0.03;
    }
    MAX_REACH
}

/// The two shore edges (indices into `sim::hex::DIRECTIONS`) a bridge on
/// `tile` joins: neighbours with land (or another bridge) in separate
/// stretches, by the shortest crossing to the drawn shores, straighter and
/// with roads first when they're about as short.
pub fn ends(game: &Game, terrain: &Terrain, tile: TileId) -> [usize; 2] {
    let centre = tile_center(&game.map.grid, tile);
    let hex = game.map.grid.hex(tile);
    let around: Vec<Option<TileId>> = (0..6).map(|d| game.map.grid.tile(hex.neighbor(d))).collect();
    // The sim's own idea of a shore (not a mountain), so we pick a pair it allows.
    let dry: Vec<bool> = around.iter().map(|n| n.is_some_and(|n| game.bridge_land(n))).collect();
    let road = |d: usize| if around[d].is_some_and(|n| game.has_road(n)) { 0.1 } else { 0.0 };
    let length: Vec<f32> = (0..6).map(|d| if dry[d] { reach(terrain, centre, d) } else { 0.0 }).collect();
    // Water on both ways round between them: separate stretches of land.
    let wet_between = |a: usize, b: usize| (a + 1..b).any(|d| !dry[d]) && (b + 1..a + 6).any(|d| !dry[d % 6]);
    let mut best = None;
    for a in 0..6 {
        for b in a + 1..6 {
            if dry[a] && dry[b] && wet_between(a, b) {
                let apart = (b - a).min(6 - (b - a)) as f32;
                let score = apart * 0.25 + road(a) + road(b) - length[a] - length[b];
                if best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, [a, b]));
                }
            }
        }
    }
    // A bridge always has two such sides when built; straight across otherwise.
    best.map_or([0, 3], |(_, e)| e)
}

/// A bridge's deck: positions are relative to its tile's centre and go by
/// `t`, 0 at the first shore, 0.5 at the centre and 1 at the other shore.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub ends: [Vec2; 2],
}

impl Span {
    /// The deck joining the shores towards edges `dirs`, each arm reaching
    /// as far as the drawn shore.
    pub fn new(terrain: &Terrain, centre: Vec2, dirs: [usize; 2]) -> Span {
        Span { ends: dirs.map(|d| edge_point(d).normalize() * reach(terrain, centre, d)) }
    }

    /// The point on the deck's line at `t`.
    pub fn point(&self, t: f32) -> Vec2 {
        if t < 0.5 {
            self.ends[0] * (1.0 - 2.0 * t)
        } else {
            self.ends[1] * (2.0 * t - 1.0)
        }
    }

    /// Whether the deck runs straight across (opposite shores).
    pub fn straight(&self) -> bool {
        self.ends[0].normalize_or_zero().dot(self.ends[1].normalize_or_zero()) < -0.9
    }

    /// The deck's surface height at `t` (absolute): up from each shore in
    /// a hump to `DECK_TOP` in the middle (`deck` in `bridges.py`).
    pub fn deck(&self, t: f32) -> f32 {
        let u = (1.0 - 2.0 * t).abs().min(1.0);
        WATER_LEVEL + DECK_END + (DECK_TOP - DECK_END) * (std::f32::consts::FRAC_PI_2 * u).cos().max(0.0).powf(0.6)
    }

    /// The side of the middle pier away from both arms (straight across:
    /// away from the camera), where the era's showpiece stands.
    pub fn outside(&self) -> Vec2 {
        if self.straight() {
            let along = (self.ends[1] - self.ends[0]).normalize_or(Vec2::X);
            let side = Vec2::new(-along.y, along.x);
            if side.y > 0.0 { -side } else { side }
        } else {
            -(self.ends[0] + self.ends[1]).normalize_or(Vec2::Y)
        }
    }

    /// The nearest `t` on the deck to `p` and how far `p` is from it.
    pub fn project(&self, p: Vec2) -> (f32, f32) {
        let arm = |end: Vec2| {
            let s = (p.dot(end) / end.length_squared()).clamp(0.0, 1.0);
            (s, p.distance(end * s))
        };
        let (s0, d0) = arm(self.ends[0]);
        let (s1, d1) = arm(self.ends[1]);
        if d0 <= d1 {
            (0.5 - s0 * 0.5, d0)
        } else {
            (0.5 + s1 * 0.5, d1)
        }
    }

    /// The end of the deck nearest `p`, as a `t`.
    pub fn end_towards(&self, p: Vec2) -> f32 {
        if p.distance(self.ends[0]) <= p.distance(self.ends[1]) {
            0.0
        } else {
            1.0
        }
    }

    /// Points to walk through along the deck from `a` to `b` (both as `t`):
    /// the centre when they're on different arms of a bent deck, then `b`.
    pub fn walk(&self, a: f32, b: f32) -> Vec<f32> {
        let crosses = (a < 0.5) != (b < 0.5);
        if crosses && !self.straight() {
            vec![0.5, b]
        } else {
            vec![b]
        }
    }

    /// Where a ship on the bridge's tile moors: beside the deck, away from
    /// the camera, and along from the middle pier's showpiece.
    pub fn mooring(&self) -> Vec2 {
        let along = (self.ends[1] - self.ends[0]).normalize_or(Vec2::X);
        let mut side = Vec2::new(-along.y, along.x);
        if side.y > 0.0 {
            side = -side;
        }
        side * 0.5 + along * 0.2
    }
}

/// The bridges `me` has explored, by tile, with their decks.
#[derive(Resource, Default)]
pub struct Bridges(pub HashMap<TileId, Span>);

impl Bridges {
    /// The deck height under a land unit at world position `p`, if it's on
    /// a bridge (arms can reach into the tiles next to a bridge's).
    pub fn deck_at(&self, game: &Game, period: f32, p: Vec2) -> Option<f32> {
        let grid = &game.map.grid;
        let tile = grid.tile(hex_at(p))?;
        std::iter::once(tile).chain(grid.neighbors(tile)).find_map(|t| {
            let span = self.0.get(&t)?;
            let mut centre = tile_center(grid, t);
            centre.x += nearest_shift(centre.x, p.x, period);
            let (t, off) = span.project(p - centre);
            (off <= DECK_REACH).then(|| span.deck(t))
        })
    }

    /// Where land units walk: the bridge's deck, else the ground (or the water).
    pub fn ground(&self, game: &Game, terrain: &Terrain, period: f32, p: Vec2) -> f32 {
        self.deck_at(game, period, p).unwrap_or_else(|| terrain.heights.sample(p).max(WATER_LEVEL))
    }
}

pub struct BridgesPlugin;

impl Plugin for BridgesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bridges>().add_systems(Update, track.after(PlaySet).in_set(InGameSet));
    }
}

/// Keeps [`Bridges`] in step with the game.
fn track(session: Res<Session>, terrain: Res<Terrain>, mut bridges: ResMut<Bridges>) {
    if !session.is_changed() && !terrain.is_changed() {
        return;
    }
    let game = &session.game;
    let explored = &game.players[session.me as usize].explored;
    let now: HashMap<TileId, Span> = game
        .passage_tiles(Passage::Bridge)
        .into_iter()
        .filter(|&t| explored[t as usize])
        .map(|t| (t, Span::new(&terrain, tile_center(&game.map.grid, t), ends(game, &terrain, t))))
        .collect();
    if now != bridges.0 {
        bridges.0 = now;
    }
}

/// Edge midpoints in the order of `sim::hex::DIRECTIONS`.
pub fn edge_point(i: usize) -> Vec2 {
    let d = sim::hex::DIRECTIONS[i];
    crate::layout::hex_center(sim::hex::Hex::new(d.q, d.r)) * 0.5
}
