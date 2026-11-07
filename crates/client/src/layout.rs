//! Conversion between hex coordinates and world space.
//! Hexes are pointy-top with circumradius 1; the map lies in the XZ plane, Y is up.

use bevy::math::Vec2;
use sim::hex::{Hex, MapGrid, TileId};

pub const SQRT3: f32 = 1.732_050_8;

/// World-space XZ centre of a hex (unwrapped: columns outside the map stay outside).
pub fn hex_center(h: Hex) -> Vec2 {
    Vec2::new(SQRT3 * (h.q as f32 + h.r as f32 * 0.5), 1.5 * h.r as f32)
}

pub fn tile_center(grid: &MapGrid, id: TileId) -> Vec2 {
    hex_center(grid.hex(id))
}

/// The hex containing a world-space XZ point.
pub fn hex_at(p: Vec2) -> Hex {
    let q = SQRT3 / 3.0 * p.x - p.y / 3.0;
    let r = 2.0 / 3.0 * p.y;
    cube_round(q, r)
}

fn cube_round(q: f32, r: f32) -> Hex {
    let s = -q - r;
    let (mut rq, mut rr, rs) = (q.round(), r.round(), s.round());
    let (dq, dr, ds) = ((rq - q).abs(), (rr - r).abs(), (rs - s).abs());
    if dq > dr && dq > ds {
        rq = -rr - rs;
    } else if dr > ds {
        rr = -rq - rs;
    }
    Hex::new(rq as i32, rr as i32)
}

/// World-space rectangle covered by the map: x in [min.x, max.x), z in [min.y, max.y].
/// The X extent is exactly one wrap period.
pub fn map_bounds(grid: &MapGrid) -> (Vec2, Vec2) {
    let min = Vec2::new(-SQRT3 * 0.5, -1.0);
    let max = Vec2::new(min.x + SQRT3 * grid.width as f32, 1.5 * (grid.height - 1) as f32 + 1.0);
    (min, max)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_roundtrip() {
        for q in -5..5 {
            for r in -5..5 {
                let h = Hex::new(q, r);
                assert_eq!(hex_at(hex_center(h)), h);
                assert_eq!(hex_at(hex_center(h) + Vec2::new(0.4, -0.3)), h);
            }
        }
    }
}
