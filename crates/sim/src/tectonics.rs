//! A light-weight plate model to give relief some logic, without simulating
//! geology over time.
//!
//! The world is split into irregular plates (warped Voronoi cells on the
//! wrapping map). Each plate is oceanic or continental and drifts in some
//! direction. Where two plates converge the crust is pushed up: long mountain
//! chains along the boundary (or island arcs at sea), fading through foothills
//! into plains with distance. Where plates pull apart the ground sinks (rifts).

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::f64::consts::TAU;

struct Plate {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    continental: bool,
}

/// Per-tile output of the plate model.
#[derive(Clone, Copy, Default, Debug)]
pub struct Crust {
    /// 0..~1.5 uplift from converging plates (mountains and foothills).
    pub uplift: f64,
    /// 0..1 sinking from diverging plates.
    pub rift: f64,
    /// 0..1 old, worn ranges along ancient seams inside today's plates.
    pub ancient: f64,
    /// -1 (oceanic) .. 1 (continental), smoothed across boundaries.
    pub continental: f64,
}

/// Tile-space position (x in columns, y in row units scaled to hex spacing).
pub fn tile_pos(col: i32, row: i32) -> (f64, f64) {
    (col as f64 + if row & 1 == 1 { 0.5 } else { 0.0 }, row as f64 * 0.866)
}

pub fn simulate(width: i32, height: i32, seed: u64) -> Vec<Crust> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x7EC7_0000);
    let w = width as f64;
    let h = height as f64 * 0.866;
    let count = ((width * height) as f64 / 380.0).round().clamp(10.0, 48.0) as usize;
    let plates: Vec<Plate> = (0..count)
        .map(|_| {
            let a = rng.gen_range(0.0..TAU);
            let speed = rng.gen_range(0.4..1.0);
            Plate {
                x: rng.gen_range(0.0..w),
                y: rng.gen_range(0.0..h),
                vx: a.cos() * speed,
                vy: a.sin() * speed,
                continental: rng.gen_bool(0.45),
            }
        })
        .collect();

    // Warp positions so plate boundaries wander instead of being straight lines.
    let warp = Fbm::<Perlin>::new(seed as u32 ^ 0x51ED).set_octaves(3);
    let along = Fbm::<Perlin>::new(seed as u32 ^ 0xA10E).set_octaves(3);
    let cyl = |n: &Fbm<Perlin>, x: f64, y: f64, f: f64, o: f64| {
        let a = x / w * TAU;
        let r = w / TAU * f;
        n.get([a.cos() * r + o, a.sin() * r - o, y * f + o])
    };
    // Wrapped offset from a to b in x.
    let dx = |a: f64, b: f64| {
        let d = (b - a).rem_euclid(w);
        if d > w / 2.0 { d - w } else { d }
    };

    // Ancient seams: a finer set of old plate boundaries. Only some of them carry
    // a worn range, and how strongly varies per seam.
    let old: Vec<(f64, f64)> = (0..count * 4).map(|_| (rng.gen_range(0.0..w), rng.gen_range(0.0..h))).collect();
    let seam_strength = |a: usize, b: usize| {
        let (lo, hi) = (a.min(b) as u64, a.max(b) as u64);
        let mut x = (lo * 0x9E37_79B9 + hi * 0x85EB_CA6B) ^ seed;
        x ^= x >> 29;
        x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x ^= x >> 32;
        let r = (x % 1000) as f64 / 1000.0;
        if r < 0.45 { 0.0 } else { (r - 0.45) / 0.55 }
    };

    let mut out = Vec::with_capacity((width * height) as usize);
    for row in 0..height {
        for col in 0..width {
            let (x, y) = tile_pos(col, row);
            let px = x + cyl(&warp, x, y, 0.06, 0.0) * 7.0;
            let py = y + cyl(&warp, x, y, 0.06, 40.0) * 7.0;
            // Nearest and second-nearest plates.
            let (mut i1, mut i2, mut d1, mut d2) = (0, 0, f64::MAX, f64::MAX);
            for (i, p) in plates.iter().enumerate() {
                let d = (dx(p.x, px).powi(2) + (p.y - py).powi(2)).sqrt();
                if d < d1 {
                    (i2, d2) = (i1, d1);
                    (i1, d1) = (i, d);
                } else if d < d2 {
                    (i2, d2) = (i, d);
                }
            }
            let (a, b) = (&plates[i1], &plates[i2]);
            // Distance to the boundary (in tiles) and its normal from a towards b.
            let boundary = (d2 - d1) * 0.5;
            let (nx, ny) = (dx(a.x, b.x), b.y - a.y);
            let len = (nx * nx + ny * ny).sqrt().max(1e-6);
            let closing = ((a.vx - b.vx) * nx + (a.vy - b.vy) * ny) / len;

            // Ranges break up along their length into separate segments with low
            // gaps (passes) between them, like the separate ranges of a real
            // mountain belt.
            let strength = (0.25 + cyl(&along, x, y, 0.28, 7.0) * 1.5).clamp(0.0, 1.3);
            // Continental collisions build the widest, highest ranges.
            let both = a.continental && b.continental;
            let width_tiles = if both { 1.3 } else { 1.0 };
            let falloff = (-(boundary / width_tiles).powi(2)).exp();
            let uplift = closing.max(0.0) * falloff * strength * if both { 1.3 } else { 1.0 };
            let rift = (-closing).max(0.0) * (-(boundary / 1.4).powi(2)).exp();

            let (mut j1, mut j2, mut e1, mut e2) = (0, 0, f64::MAX, f64::MAX);
            for (j, &(ox, oy)) in old.iter().enumerate() {
                let d = (dx(ox, px).powi(2) + (oy - py).powi(2)).sqrt();
                if d < e1 {
                    (j2, e2) = (j1, e1);
                    (j1, e1) = (j, d);
                } else if d < e2 {
                    (j2, e2) = (j, d);
                }
            }
            let ancient = seam_strength(j1, j2) * (-((e2 - e1) * 0.5 / 1.1).powi(2)).exp() * strength;

            let ca = if a.continental { 1.0 } else { -1.0 };
            let cb = if b.continental { 1.0 } else { -1.0 };
            let blend = 0.5 * (1.0 - (boundary / 4.0).min(1.0));
            out.push(Crust { uplift, rift, ancient, continental: ca * (1.0 - blend) + cb * blend });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uplift_concentrates_on_boundaries() {
        let crust = simulate(96, 60, 3);
        let high = crust.iter().filter(|c| c.uplift > 0.3).count();
        // Mountains are a minority of the map, but present.
        assert!(high > 50 && high < crust.len() / 4, "{high} high tiles");
    }
}
