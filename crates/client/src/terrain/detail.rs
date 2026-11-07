//! Tileable detail texture for the terrain shader: RGB is a normal map of rocky,
//! eroded micro-relief, A is "cavity" (0.5 = flat, lower in cracks and hollows,
//! higher on edges), used to darken crevices and break up snow.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin, RidgedMulti};
use rayon::prelude::*;
use std::f64::consts::TAU;

/// Samples `n` on a 4D torus so the result tiles seamlessly in both directions.
fn torus(n: &impl NoiseFn<f64, 4>, u: f64, v: f64, freq: f64) -> f64 {
    let r = freq / TAU;
    let (a, b) = (u * TAU, v * TAU);
    n.get([a.cos() * r, a.sin() * r, b.cos() * r, b.sin() * r])
}

pub fn detail_texture(size: usize) -> Vec<u8> {
    let ridged = RidgedMulti::<Perlin>::new(91).set_octaves(6);
    let fbm = Fbm::<Perlin>::new(92).set_octaves(6);
    let heights: Vec<f32> = (0..size * size)
        .into_par_iter()
        .map(|i| {
            let (u, v) = ((i % size) as f64 / size as f64, (i / size) as f64 / size as f64);
            // Cracked rock: ridged noise for crests and fissures, fbm for lumps.
            (torus(&ridged, u, v, 8.0) * 0.6 + torus(&fbm, u, v, 16.0) * 0.4) as f32
        })
        .collect();
    let at = |x: isize, y: isize| {
        let s = size as isize;
        heights[(y.rem_euclid(s) * s + x.rem_euclid(s)) as usize]
    };
    // A broad blur of the height for the cavity term.
    let radius = 10isize;
    // Height differences between neighbouring texels shrink as the texture grows;
    // scale so the normals' strength is independent of resolution.
    let strength = size as f32 * 0.035;
    let mut out = vec![0u8; size * size * 4];
    out.par_chunks_mut(size * 4).enumerate().for_each(|(y, row)| {
        let y = y as isize;
        for x in 0..size as isize {
            let nx = (at(x - 1, y) - at(x + 1, y)) * strength;
            let ny = (at(x, y - 1) - at(x, y + 1)) * strength;
            let len = (nx * nx + ny * ny + 1.0).sqrt();
            let mut sum = 0.0;
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    sum += at(x + dx, y + dy);
                }
            }
            let mean = sum / ((2 * radius + 1) * (2 * radius + 1)) as f32;
            let cavity = (0.5 + (at(x, y) - mean) * 2.2).clamp(0.0, 1.0);
            let px = &mut row[x as usize * 4..x as usize * 4 + 4];
            px[0] = ((nx / len * 0.5 + 0.5) * 255.0) as u8;
            px[1] = ((ny / len * 0.5 + 0.5) * 255.0) as u8;
            px[2] = ((1.0 / len * 0.5 + 0.5) * 255.0) as u8;
            px[3] = (cavity * 255.0) as u8;
        }
    });
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn detail_has_real_relief() {
        let tex = super::detail_texture(128);
        let spread = |ch: usize| {
            let v: Vec<f32> = tex.chunks(4).map(|p| p[ch] as f32 / 255.0).collect();
            let mean = v.iter().sum::<f32>() / v.len() as f32;
            (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt()
        };
        // Normals must actually tilt, and the cavity must vary.
        assert!(spread(0) > 0.08, "normal x spread {}", spread(0));
        assert!(spread(3) > 0.08, "cavity spread {}", spread(3));
    }
}
