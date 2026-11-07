//! Map types: the overall layout of land and sea. Each type (except Fractal)
//! is a land mask added to the generator's height before the sea level is cut,
//! so plates, mountains, climate and rivers work the same on every type.

use crate::tectonics::tile_pos;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MapType {
    /// A few large landmasses parted by open ocean.
    Continents,
    /// One supercontinent with a few islands off its coasts.
    Pangaea,
    /// Many small and medium islands.
    Archipelago,
    /// Unshaped noise and plates: anything from continents to scattered isles.
    #[default]
    Fractal,
    /// A ring of land around one great enclosed sea.
    InlandSea,
    /// Every civ starts in an old world; a new world lies empty across the ocean.
    Terra,
    /// The real world, from `data/earth.ron` (see `earth.rs`): one fixed map
    /// whatever the seed (`mapgen::EARTH_SEED`).
    Earth,
}

impl MapType {
    /// In the order the setup screen lists them.
    pub const ALL: [MapType; 7] =
        [MapType::Continents, MapType::Pangaea, MapType::Archipelago, MapType::Fractal, MapType::InlandSea, MapType::Terra, MapType::Earth];

    pub fn id(self) -> &'static str {
        match self {
            MapType::Continents => "continents",
            MapType::Pangaea => "pangaea",
            MapType::Archipelago => "archipelago",
            MapType::Fractal => "fractal",
            MapType::InlandSea => "inland_sea",
            MapType::Terra => "terra",
            MapType::Earth => "earth",
        }
    }

    pub fn from_id(id: &str) -> Option<MapType> {
        MapType::ALL.into_iter().find(|t| t.id() == id)
    }

    /// Multiplies the land fraction: islands leave more sea between them.
    pub(crate) fn land_scale(self) -> f32 {
        match self {
            MapType::Archipelago => 0.82,
            _ => 1.0,
        }
    }
}

/// A map type's land mask.
pub(crate) struct Shape {
    /// Added to each tile's height: high where land should be, strongly
    /// negative where there must be sea.
    pub value: Vec<f64>,
    /// Where civs may start (empty: anywhere).
    pub start_land: Vec<bool>,
}

/// A Voronoi cell around a seed point: one continent or island.
struct Cell {
    x: f64,
    y: f64,
    /// How high its land rises (below 0: open sea).
    strength: f64,
    /// Terra: 0 for the old world, 1 for the new.
    world: u8,
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn shape(kind: MapType, width: i32, height: i32, seed: u64) -> Option<Shape> {
    if matches!(kind, MapType::Fractal | MapType::Earth) {
        return None;
    }
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x5A4E_0000);
    let w = width as f64;
    let h = height as f64 * 0.866;
    // Wobbles positions so coasts and channels wander.
    let warp = Fbm::<Perlin>::new(seed as u32 ^ 0xC0A5).set_octaves(3);
    let cyl = |x: f64, y: f64, f: f64, o: f64| {
        let a = x / w * TAU;
        let r = w / TAU * f;
        warp.get([a.cos() * r + o, a.sin() * r - o, y * f + o])
    };
    let dx = |a: f64, b: f64| {
        let d = (b - a).rem_euclid(w);
        if d > w / 2.0 { d - w } else { d }
    };
    let cx = rng.gen_range(0.0..w);
    // Seeds spread around the map: `count` columns, jittered, kept off the poles.
    let spread = |rng: &mut ChaCha8Rng, count: usize, from: f64, span: f64, world: u8| -> Vec<Cell> {
        let step = span / count as f64;
        (0..count)
            .map(|i| Cell {
                x: (from + step * (i as f64 + rng.gen_range(0.3..0.7))).rem_euclid(w),
                y: h * rng.gen_range(0.3..0.7),
                strength: rng.gen_range(0.85..1.0),
                world,
            })
            .collect()
    };
    let cells: Vec<Cell> = match kind {
        MapType::Continents => {
            let count = if rng.gen_bool(0.5) { 3 } else { 4 };
            spread(&mut rng, count, cx, w, 0)
        }
        MapType::Terra => {
            // The old world takes about three fifths of the globe.
            let old = if rng.gen_bool(0.5) { 2 } else { 3 };
            let mut c = spread(&mut rng, old, cx, w * 0.6, 0);
            c.extend(spread(&mut rng, 2, cx + w * 0.6, w * 0.4, 1));
            c
        }
        MapType::Archipelago => {
            let count = ((w * h) / 105.0).round().max(12.0) as usize;
            (0..count)
                .map(|_| Cell {
                    x: rng.gen_range(0.0..w),
                    y: rng.gen_range(h * 0.1..h * 0.9),
                    // A quarter of the cells stay open sea.
                    strength: if rng.gen_bool(0.25) { -1.0 } else { rng.gen_range(0.5..1.0) },
                    world: 0,
                })
                .collect()
        }
        _ => Vec::new(),
    };
    // Typical distance from a cell's seed to its edge.
    let reach = if cells.is_empty() { 1.0 } else { (w * h / cells.len() as f64).sqrt() * 0.6 };
    // Pangaea and the inland sea's ring: a chain of overlapping blobs (x, y,
    // radius), so the land has lobes and bays instead of one oval.
    let mut blobs: Vec<(f64, f64, f64)> = Vec::new();
    if matches!(kind, MapType::Pangaea | MapType::InlandSea) {
        let count = if kind == MapType::InlandSea { 9 } else { 6 };
        for i in 0..count {
            let a = i as f64 / count as f64 * TAU + rng.gen_range(-0.3..0.3);
            let (rx, ry) = if kind == MapType::InlandSea { (w * 0.27, h * 0.27) } else { (w * 0.17, h * 0.17) };
            let r = rng.gen_range(0.75..1.1) * if kind == MapType::InlandSea { h * 0.23 } else { h * 0.24 };
            blobs.push((cx + a.cos() * rx * rng.gen_range(0.8..1.2), h * 0.5 + a.sin() * ry * rng.gen_range(0.8..1.2), r));
        }
        if kind == MapType::Pangaea {
            blobs.push((cx, h * 0.5, h * 0.26));
        }
    }
    let strait = {
        let a = rng.gen_range(0.0..TAU);
        (a.cos(), a.sin())
    };
    {
    }

    let n = (width * height) as usize;
    let mut value = Vec::with_capacity(n);
    let mut start_land = Vec::new();
    for row in 0..height {
        for col in 0..width {
            let (x, y) = tile_pos(col, row);
            let px = x + cyl(x, y, 0.04, 0.0) * 12.0 + cyl(x, y, 0.12, 20.0) * 3.0;
            let py = y + cyl(x, y, 0.04, 50.0) * 12.0 + cyl(x, y, 0.12, 70.0) * 3.0;
            match kind {
                MapType::Pangaea | MapType::InlandSea => {
                    // 1 at a blob's centre, 0 at its rim.
                    let land = blobs
                        .iter()
                        .map(|&(bx, by, r)| 1.0 - ((dx(bx, px) / 1.3).powi(2) + (by - py).powi(2)).sqrt() / r)
                        .fold(f64::MIN, f64::max);
                    let mut v = smoothstep(-0.3, 0.5, land) * 1.25 - 0.3;
                    if kind == MapType::InlandSea {
                        // The sea fills the middle of the ring, and a band of
                        // land always closes around it...
                        let e = ((dx(cx, px) / (w * 0.17)).powi(2) + ((py - h * 0.5) / (h * 0.2)).powi(2)).sqrt();
                        v -= (1.0 - smoothstep(0.75, 1.15, e)) * 1.6;
                        if (1.15..1.8).contains(&e) {
                            v = v.max(1.1);
                        }
                        // ...but for one narrow strait out to the ocean.
                        let (qx, qy) = (dx(cx, px), py - h * 0.5);
                        let along = qx * strait.0 + qy * strait.1;
                        if (0.6..2.6).contains(&e) && along > 0.0 && (qx * strait.1 - qy * strait.0).abs() < 1.1 {
                            v = -1.0;
                        }
                    }
                    value.push(v);
                }
                _ => {
                    let (mut i1, mut i2, mut d1, mut d2) = (0, 0, f64::MAX, f64::MAX);
                    for (i, c) in cells.iter().enumerate() {
                        let d = (dx(c.x, px).powi(2) + (c.y - py).powi(2)).sqrt();
                        if d < d1 {
                            (i2, d2) = (i1, d1);
                            (i1, d1) = (i, d);
                        } else if d < d2 {
                            (i2, d2) = (i, d);
                        }
                    }
                    let (a, b) = (&cells[i1], &cells[i2]);
                    // Distance to the cell's edge, where a channel of sea runs.
                    let edge = (d2 - d1) * 0.5;
                    let (gap, span) = match kind {
                        MapType::Archipelago => (1.2, 1.8),
                        // The ocean between the old and new worlds is wider.
                        MapType::Terra if a.world != b.world => (5.0, 5.0),
                        _ => (2.0, 4.0),
                    };
                    let open = smoothstep(gap, gap + span, edge);
                    let v = if a.strength < 0.0 {
                        -0.9 - 0.1 * (1.0 - open)
                    } else {
                        // Highest near the seed, so each cell holds one landmass;
                        // continents fall off faster north-south than east-west.
                        let squash = if kind == MapType::Archipelago { 1.0 } else { 3.0 };
                        let far = (dx(a.x, px).powi(2) + ((a.y - py) * squash).powi(2)).sqrt() / reach;
                        let bump = a.strength * (1.3 - far * 0.7).max(-0.7);
                        -1.0 + (1.0 + bump) * open
                    };
                    value.push(v);
                    if kind == MapType::Terra {
                        start_land.push(a.world == 0);
                    }
                }
            }
        }
    }
    Some(Shape { value, start_land })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Ability;
    use crate::game::{Game, GameSetup};
    use crate::hex::TileId;
    use crate::map::Map;
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Rules;

    fn map(rules: &Rules, kind: MapType, seed: u64) -> Map {
        generate(rules, &MapSettings { kind, seed, ..Default::default() })
    }

    /// Connected regions of land, largest first.
    fn landmasses(rules: &Rules, map: &Map) -> Vec<Vec<TileId>> {
        let mut seen = vec![false; map.tiles.len()];
        let mut out = Vec::new();
        for start in 0..map.tiles.len() as TileId {
            if seen[start as usize] || map.is_water(rules, start) {
                continue;
            }
            let (mut stack, mut region) = (vec![start], Vec::new());
            seen[start as usize] = true;
            while let Some(id) = stack.pop() {
                region.push(id);
                for nb in map.grid.neighbors(id) {
                    if !seen[nb as usize] && !map.is_water(rules, nb) {
                        seen[nb as usize] = true;
                        stack.push(nb);
                    }
                }
            }
            out.push(region);
        }
        out.sort_by_key(|r| std::cmp::Reverse(r.len()));
        out
    }

    #[test]
    fn each_type_has_its_layout() {
        let rules = Rules::load_default().unwrap();
        for seed in [3, 7, 11] {
            for kind in MapType::ALL {
                let m = map(&rules, kind, seed);
                let land = landmasses(&rules, &m);
                let total: usize = land.iter().map(|r| r.len()).sum();
                let big = land.iter().filter(|r| r.len() * 10 >= total).count();
                let share = land[0].len() as f32 / total as f32;
                let what = format!("{kind:?} seed {seed}: {big} big landmasses, largest {share:.2}");
                match kind {
                    MapType::Continents => assert!((3..=4).contains(&big) && share < 0.5, "{what}"),
                    MapType::Pangaea => assert!(share > 0.85, "{what}"),
                    MapType::Archipelago => assert!(land.len() >= 15 && share < 0.4, "{what}"),
                    MapType::Fractal | MapType::Earth => {}
                    MapType::InlandSea => {
                        // One ring of land around a sea, which may reach the
                        // ocean through a strait: with straits closed (water
                        // near land counted as land) the sea stands apart.
                        assert!(share > 0.8, "{what}");
                        let open = |t: TileId| m.grid.within(t, 2).iter().all(|&o| m.is_water(&rules, o));
                        let mut seen = vec![false; m.tiles.len()];
                        let mut seas = Vec::new();
                        for start in (0..m.tiles.len() as TileId).filter(|&t| open(t)) {
                            if seen[start as usize] {
                                continue;
                            }
                            let (mut stack, mut size) = (vec![start], 0);
                            seen[start as usize] = true;
                            while let Some(id) = stack.pop() {
                                size += 1;
                                for nb in m.grid.neighbors(id) {
                                    if !seen[nb as usize] && open(nb) {
                                        seen[nb as usize] = true;
                                        stack.push(nb);
                                    }
                                }
                            }
                            seas.push(size);
                        }
                        seas.sort_unstable_by(|a, b| b.cmp(a));
                        assert!(seas.len() >= 2 && seas[1] >= 60, "{what}, seas {:?}", &seas[..seas.len().min(3)]);
                    }
                    MapType::Terra => {
                        // Old and new worlds never share a landmass, and the new
                        // world is a good part of the land.
                        let new: usize = land.iter().flatten().filter(|&&t| !m.start_land[t as usize]).count();
                        assert!(new * 4 >= total, "{what}: new world {new} of {total}");
                        for r in land.iter().filter(|r| r.len() >= 20) {
                            let old = r.iter().filter(|&&t| m.start_land[t as usize]).count();
                            assert!(old == 0 || old == r.len(), "{what}: a landmass of {} with {old} old-world tiles", r.len());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_type_seats_six_players() {
        let rules = Rules::load_default().unwrap();
        for kind in MapType::ALL {
            let m = map(&rules, kind, 5);
            let setup = GameSetup::random(&rules, 6, &[], 5);
            let g = Game::with_setup(rules.clone(), m, &setup);
            for p in 0..6 {
                let settler = g.units.values().find(|u| u.owner == p && g.unit_def(u).has(Ability::FoundCity));
                let settler = settler.unwrap_or_else(|| panic!("{kind:?}: player {p} has no start"));
                if kind == MapType::Terra {
                    assert!(g.map.start_land[settler.tile as usize], "player {p} starts in the new world");
                }
            }
        }
    }
}
