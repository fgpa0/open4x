//! Continuous terrain height built from the hex map.
//!
//! The coastline is the zero line of a smooth land/water field, which by
//! construction lies on hex edges, so what you see matches each tile's type.
//! Adjacent mountain tiles merge into one massif with ridgelines crossing tiles,
//! and rivers are carved below the water line. Everything is periodic in X so the
//! map wraps seamlessly.

use crate::layout::{hex_at, hex_center, map_bounds};
use crate::wrapnoise::Cylinder;
use bevy::math::Vec2;
use noise::{Fbm, MultiFractal, Perlin, RidgedMulti};
use rayon::prelude::*;
use sim::hex::{Hex, DIRECTIONS};
use sim::map::{Elevation, Map};
use sim::rules::Rules;

/// Water surface height. Everything below is under water.
pub const WATER_LEVEL: f32 = 0.0;

/// Per-tile data the height function needs, precomputed once.
#[derive(Clone, Copy)]
struct TileShape {
    land: bool,
    /// Ice sheet (snow terrain): raised with broad, smooth undulations.
    ice_sheet: bool,
    /// Ground height for land, seabed depth (negative) for water.
    level: f32,
    hill: f32,
    /// Summit height of a mountain tile (0 if not a mountain).
    mountain: f32,
    /// Summit offset from the tile centre, so ranges aren't laid out on a grid.
    summit: Vec2,
}

const OFF_MAP: TileShape =
    TileShape { land: false, ice_sheet: false, level: -0.85, hill: 0.0, mountain: 0.0, summit: Vec2::ZERO };

/// Deterministic pseudo-random value in 0..1 for tile `i` and channel `k`.
fn tile_random(i: u32, k: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ k.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65_535.0
}

/// Blend weights of the tiles around a point.
pub struct Neighborhood {
    pub hexes: [Hex; 19],
}

impl Neighborhood {
    /// The hex containing `p` and every hex within distance 2.
    pub fn around(p: Vec2) -> Self {
        let home = hex_at(p);
        let mut hexes = [home; 19];
        let mut i = 1;
        for d in 0..6 {
            hexes[i] = home.neighbor(d);
            i += 1;
        }
        for d in 0..6 {
            let a = home.neighbor(d).neighbor(d);
            hexes[i] = a;
            hexes[i + 1] = a.neighbor((d + 2) % 6);
            i += 2;
        }
        Self { hexes }
    }
}

pub fn gauss(d2: f32, sigma: f32) -> f32 {
    (-d2 / (2.0 * sigma * sigma)).exp()
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Distance from `p` to segment `a`-`b`.
fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Smooth fields describing the terrain types around a point.
pub struct Fields {
    /// > 0 on land, < 0 on water; zero on the coastline (hex edges).
    pub land: f32,
    /// 0..1 share of mountain tiles nearby.
    pub mountain: f32,
    /// Smoothly blended hill height (0 away from hills, ~0.3-0.45 inside hill country).
    pub hill: f32,
}

pub struct HeightSampler<'a> {
    map: &'a Map,
    shapes: Vec<TileShape>,
    /// River centre lines per tile, as polylines relative to the tile centre.
    rivers: Vec<Vec<Vec<Vec2>>>,
    cyl: Cylinder,
    warp: Fbm<Perlin>,
    detail: Fbm<Perlin>,
    ridges: RidgedMulti<Perlin>,
    meander: Fbm<Perlin>,
    erosion: Perlin,
}

impl<'a> HeightSampler<'a> {
    pub fn new(map: &'a Map, rules: &Rules) -> Self {
        let seed = map.seed as u32;
        let shapes = map
            .tiles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let def = rules.terrain(t.terrain);
                let i = i as u32;
                let jitter = tile_random(i, 0);
                TileShape {
                    land: !def.water,
                    ice_sheet: def.id == "snow",
                    level: if def.id == "snow" {
                        0.24
                    } else if !def.water {
                        0.13
                    } else if def.id == "coast" || def.id == "lake" {
                        -0.28
                    } else {
                        -0.85
                    },
                    hill: if t.elevation == Elevation::Hills { 0.3 + jitter * 0.14 } else { 0.0 },
                    mountain: if t.elevation == Elevation::Mountains { 1.25 + jitter * 1.1 } else { 0.0 },
                    summit: (Vec2::new(tile_random(i, 1), tile_random(i, 2)) - 0.5) * 0.7,
                }
            })
            .collect();
        let (min, max) = map_bounds(&map.grid);
        let rivers = (0..map.tiles.len() as u32).map(|id| river_curves(map, id)).collect();
        Self {
            map,
            shapes,
            rivers,
            cyl: Cylinder::new(max.x - min.x),
            warp: Fbm::<Perlin>::new(seed.wrapping_add(10)).set_octaves(3),
            detail: Fbm::<Perlin>::new(seed.wrapping_add(11)).set_octaves(4),
            ridges: RidgedMulti::<Perlin>::new(seed.wrapping_add(12)).set_octaves(5),
            meander: Fbm::<Perlin>::new(seed.wrapping_add(13)).set_octaves(2),
            erosion: Perlin::new(seed.wrapping_add(14)),
        }
    }

    pub fn cylinder(&self) -> Cylinder {
        self.cyl
    }

    fn shape(&self, h: Hex) -> TileShape {
        match self.map.grid.tile(h) {
            Some(id) => self.shapes[id as usize],
            // Beyond the north/south edge: deep ocean.
            None => OFF_MAP,
        }
    }

    /// Small warp applied to the coastline and terrain borders: enough to look
    /// natural, small enough that borders stay on the right hex edges.
    pub fn border_warp(&self, p: Vec2) -> Vec2 {
        // Broad meanders plus finer raggedness; tile centres always stay correct.
        self.cyl.vec2(&self.warp, p, 0.4) * 0.3 + self.cyl.vec2(&self.warp, p, 1.6) * 0.12
    }

    pub fn fields(&self, p: Vec2) -> Fields {
        let pw = p + self.border_warp(p);
        let (mut land, mut lw) = (0.0, 0.0);
        let (mut mountain, mut mw) = (0.0, 0.0);
        let (mut hill, mut hw) = (0.0, 0.0);
        for h in Neighborhood::around(pw).hexes {
            let s = self.shape(h);
            let d2 = pw.distance_squared(hex_center(h));
            let g = gauss(d2, 0.46);
            land += g * if s.land { 1.0 } else { -1.0 };
            lw += g;
            let gm = gauss(d2, 0.5);
            mountain += gm * if s.mountain > 0.0 { 1.0 } else { 0.0 };
            mw += gm;
            // Normalised blend, so neighbouring hill tiles merge into one stretch
            // of hill country instead of a separate mound per tile.
            let gh = gauss(d2, 0.6);
            hill += s.hill * gh;
            hw += gh;
        }
        Fields { land: land / lw, mountain: mountain / mw, hill: hill / hw }
    }

    /// Distance from `p` to the nearest river centre line.
    pub fn river_distance(&self, p: Vec2) -> f32 {
        // A small, fine wiggle on top of the curved course.
        let p = p + self.cyl.vec2(&self.meander, p, 3.0) * 0.07;
        let home = hex_at(p);
        let mut best = f32::MAX;
        for h in std::iter::once(home).chain((0..6).map(|d| home.neighbor(d))) {
            let Some(id) = self.map.grid.tile(h) else { continue };
            let curves = &self.rivers[id as usize];
            if curves.is_empty() {
                continue;
            }
            let local = p - hex_center(h);
            for line in curves {
                for seg in line.windows(2) {
                    best = best.min(seg_dist(local, seg[0], seg[1]));
                }
            }
        }
        best
    }

    /// Ridged noise whose octaves are damped where the terrain is already steep,
    /// which gives the look of water erosion: sharp crests, smooth gullies and
    /// valleys between them. Returns roughly 0..1.
    fn eroded(&self, p: Vec2, octaves: u32, base_freq: f32) -> f32 {
        let mut sum = 0.0;
        let mut norm = 0.0;
        let mut amp = 1.0;
        let mut freq = base_freq;
        let mut slope = Vec2::ZERO;
        let e = 0.01;
        for i in 0..octaves {
            let o = i as f64 * 19.7;
            let n = self.cyl.get(&self.erosion, p, freq as f64, o);
            let nx = self.cyl.get(&self.erosion, p + Vec2::X * e / freq, freq as f64, o);
            let nz = self.cyl.get(&self.erosion, p + Vec2::Y * e / freq, freq as f64, o);
            let grad = Vec2::new(nx - n, nz - n) / e;
            // Ridges: 1 at the zero crossings of the noise.
            let ridge = 1.0 - n.abs();
            slope += grad * amp * if n > 0.0 { -1.0 } else { 1.0 };
            sum += amp * ridge * ridge / (1.0 + slope.length_squared() * 0.6);
            norm += amp;
            amp *= 0.5;
            freq *= 2.1;
        }
        sum / norm
    }

    /// Terrain height at a world XZ point.
    pub fn height(&self, p: Vec2) -> f32 {
        let pw = p + self.border_warp(p);
        let f = self.fields(p);

        // Base level: land heights blended among land tiles, depths among water tiles.
        let (mut lsum, mut lw, mut wsum, mut ww) = (0.0, 0.0, 0.0, 0.0);
        let mut ice_sheet = 0.0;
        let mut peak: f32 = 0.0;
        for h in Neighborhood::around(pw).hexes {
            let s = self.shape(h);
            let d2 = pw.distance_squared(hex_center(h));
            let g = gauss(d2, if s.land { 0.5 } else { 0.7 });
            if s.land {
                lsum += s.level * g;
                lw += g;
                ice_sheet += if s.ice_sheet { g } else { 0.0 };
            } else {
                wsum += s.level * g;
                ww += g;
            }
            // Each mountain tile has its own summit so tiles stay recognisable.
            if s.mountain > 0.0 {
                let d = pw.distance(hex_center(h) + s.summit);
                let t = (1.0 - d / 1.45).max(0.0);
                peak = peak.max(s.mountain * t.powf(1.7));
            }
        }
        let land_level = if lw > 0.0 { lsum / lw } else { 0.13 };
        let sea_level = if ww > 0.0 { wsum / ww } else { -0.28 };
        let detail = self.cyl.get(&self.detail, p, 1.3, 0.0);

        // A tiny step at the coastline keeps the beach just clear of the water
        // surface; the water fades out entirely at near-zero depth (see water.rs),
        // so the two never fight over the same pixels.
        if f.land <= 0.0 {
            let depth = smoothstep(0.0, 0.75, -f.land);
            return -0.006 + sea_level * depth + detail * 0.06 * depth;
        }

        let on_land = smoothstep(0.0, 0.6, f.land);
        // Gentle rolling ground, so even wide plains have some undulation.
        let rolling = self.cyl.get(&self.warp, p, 0.22, 91.0) * 0.07 + 0.035;
        let mut height = 0.004 + land_level * on_land + (detail * 0.03 + rolling) * on_land;
        if lw > 0.0 && ice_sheet > 0.0 {
            // Wind-shaped domes and hollows on ice sheets.
            let swell = self.cyl.get(&self.detail, p, 0.45, 31.0);
            height += (ice_sheet / lw) * on_land * (swell * 0.12 + 0.04);
        }

        // Hills: rolling hill country with ridges and valleys from erosion noise,
        // blended across neighbouring hill tiles rather than a mound per tile.
        if f.hill > 0.01 {
            let rp = p + self.cyl.vec2(&self.warp, p, 0.5) * 0.5;
            height += f.hill * (0.35 + self.eroded(rp, 5, 0.42) * 1.9 + detail * 0.15) * on_land;
        }

        // Mountains: summits rising from a shared massif, then carved by erosion so
        // ridges and gullies run down from the peaks. Erosion cuts deeper the
        // higher the ground, like real ranges.
        let massif = smoothstep(0.08, 0.85, f.mountain) * on_land;
        if massif > 0.0 {
            let rp = p + self.cyl.vec2(&self.warp, p, 0.35) * 0.6;
            let ridge = self.cyl.get(&self.ridges, rp, 0.5, 3.0) * 0.5 + 0.5;
            let uplift = massif * (0.35 + ridge * 0.35);
            let shape = uplift.max(peak * massif.sqrt());
            let erosion = self.eroded(rp, 7, 0.75);
            height += shape * (0.55 + erosion * 0.75) - shape * shape * 0.04;
        }

        // Rivers: carve a channel below the water line with soft banks.
        let rd = self.river_distance(p);
        if rd < 0.4 {
            let channel = smoothstep(0.07, 0.3, rd);
            let floor = WATER_LEVEL - 0.07;
            height = floor + (height - floor) * channel;
        }
        height
    }
}

/// Quadratic Bézier from `a` to `b` with control point `k`, as a polyline.
fn bezier(a: Vec2, k: Vec2, b: Vec2, steps: usize) -> Vec<Vec2> {
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            a * (1.0 - t) * (1.0 - t) + k * 2.0 * t * (1.0 - t) + b * t * t
        })
        .collect()
}

/// Smooth river courses through a tile, relative to its centre.
///
/// Where a river crosses into the next tile is jittered along the shared edge;
/// both tiles compute the same point, so courses join up. Inside the tile the
/// river curves through a jittered point near the centre.
fn river_curves(map: &Map, id: u32) -> Vec<Vec<Vec2>> {
    let bits = map.tile(id).rivers;
    if bits == 0 {
        return Vec::new();
    }
    let hex = map.grid.hex(id);
    let crossings: Vec<Vec2> = (0..6)
        .filter(|d| bits & (1 << d) != 0)
        .map(|d| {
            let dir = DIRECTIONS[d];
            let to_edge = hex_center(Hex::new(dir.q, dir.r)) * 0.5;
            let along = to_edge.perp().normalize();
            let nb = map.grid.tile(hex.neighbor(d)).unwrap_or(id);
            let (lo, hi) = (id.min(nb), id.max(nb));
            let shift = (tile_random(lo ^ hi.rotate_left(16), 7) - 0.5) * 0.44;
            to_edge + along * if id == lo { shift } else { -shift }
        })
        .collect();
    let center = (Vec2::new(tile_random(id, 5), tile_random(id, 6)) - 0.5) * 0.5;
    if crossings.len() == 2 {
        // One smooth bend through the (jittered) centre.
        let (a, b) = (crossings[0], crossings[1]);
        vec![bezier(a, center * 2.0 - (a + b) * 0.5, b, 12)]
    } else {
        // Source, mouth or confluence: each arm curves in to the centre.
        crossings
            .iter()
            .enumerate()
            .map(|(i, &m)| {
                let side = (m - center).perp() * (tile_random(id, 8 + i as u32) - 0.5) * 0.5;
                bezier(m, (m + center) * 0.5 + side, center, 8)
            })
            .collect()
    }
}

/// Heights sampled on a regular grid covering one wrap period of the map, used
/// for the mesh, texture baking and mouse picking. Lookups wrap in X.
#[derive(Clone)]
pub struct HeightField {
    pub origin: Vec2,
    pub step: f32,
    /// Columns per wrap period (column `cols` is column 0 again).
    pub cols: usize,
    pub rows: usize,
    pub data: Vec<f32>,
}

impl HeightField {
    pub fn build(map: &Map, rules: &Rules, samples_per_unit: f32) -> Self {
        let sampler = HeightSampler::new(map, rules);
        let (min, max) = map_bounds(&map.grid);
        let cols = ((max.x - min.x) * samples_per_unit).round() as usize;
        let step = (max.x - min.x) / cols as f32;
        let rows = ((max.y - min.y) / step).ceil() as usize + 1;
        let data: Vec<f32> = (0..rows)
            .into_par_iter()
            .flat_map_iter(|r| {
                let sampler = &sampler;
                (0..cols).map(move |c| sampler.height(min + Vec2::new(c as f32, r as f32) * step))
            })
            .collect();
        Self { origin: min, step, cols, rows, data }
    }

    /// Height at grid point (c, r); c wraps, r clamps.
    pub fn at(&self, c: isize, r: isize) -> f32 {
        let c = c.rem_euclid(self.cols as isize) as usize;
        let r = r.clamp(0, self.rows as isize - 1) as usize;
        self.data[r * self.cols + c]
    }

    /// Bilinear height lookup.
    pub fn sample(&self, p: Vec2) -> f32 {
        let g = (p - self.origin) / self.step;
        let (c, r) = (g.x.floor() as isize, g.y.floor() as isize);
        let (fx, fy) = (g.x - c as f32, (g.y - r as f32).clamp(0.0, 1.0));
        let top = self.at(c, r) * (1.0 - fx) + self.at(c + 1, r) * fx;
        let bottom = self.at(c, r + 1) * (1.0 - fx) + self.at(c + 1, r + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    /// Height gradient (dh/dx, dh/dz).
    pub fn gradient(&self, p: Vec2) -> Vec2 {
        let e = self.step;
        Vec2::new(
            self.sample(p + Vec2::X * e) - self.sample(p - Vec2::X * e),
            self.sample(p + Vec2::Y * e) - self.sample(p - Vec2::Y * e),
        ) / (2.0 * e)
    }
}
