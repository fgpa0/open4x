//! Hex grid math. Pointy-top hexes, axial coordinates (q, r).
//! The map is stored as an "odd-r" offset rectangle that wraps horizontally.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hex {
    pub q: i32,
    pub r: i32,
}

/// Neighbour directions, starting east and going counter-clockwise.
pub const DIRECTIONS: [Hex; 6] = [
    Hex { q: 1, r: 0 },
    Hex { q: 1, r: -1 },
    Hex { q: 0, r: -1 },
    Hex { q: -1, r: 0 },
    Hex { q: -1, r: 1 },
    Hex { q: 0, r: 1 },
];

impl Hex {
    pub const fn new(q: i32, r: i32) -> Self {
        Self { q, r }
    }

    pub fn s(self) -> i32 {
        -self.q - self.r
    }

    pub fn neighbor(self, dir: usize) -> Hex {
        let d = DIRECTIONS[dir];
        Hex::new(self.q + d.q, self.r + d.r)
    }

    /// Distance ignoring map wrap. Use `MapGrid::distance` for wrapped distance.
    pub fn distance(self, other: Hex) -> i32 {
        let dq = (self.q - other.q).abs();
        let dr = (self.r - other.r).abs();
        let ds = (self.s() - other.s()).abs();
        dq.max(dr).max(ds)
    }

    /// Offset (column, row) in odd-r layout.
    pub fn to_offset(self) -> (i32, i32) {
        let col = self.q + (self.r - (self.r & 1)) / 2;
        (col, self.r)
    }

    pub fn from_offset(col: i32, row: i32) -> Hex {
        Hex::new(col - (row - (row & 1)) / 2, row)
    }
}

/// Rectangular map that wraps east-west. Tiles are addressed by a dense `TileId`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MapGrid {
    pub width: i32,
    pub height: i32,
}

pub type TileId = u32;

impl MapGrid {
    pub fn new(width: i32, height: i32) -> Self {
        assert!(width > 0 && height > 0);
        Self { width, height }
    }

    pub fn len(&self) -> usize {
        (self.width * self.height) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Normalises a hex onto the map (wrapping columns). `None` if off the top/bottom.
    pub fn tile(&self, hex: Hex) -> Option<TileId> {
        let (col, row) = hex.to_offset();
        if row < 0 || row >= self.height {
            return None;
        }
        let col = col.rem_euclid(self.width);
        Some((row * self.width + col) as TileId)
    }

    pub fn hex(&self, id: TileId) -> Hex {
        let id = id as i32;
        Hex::from_offset(id % self.width, id / self.width)
    }

    pub fn offset(&self, id: TileId) -> (i32, i32) {
        let id = id as i32;
        (id % self.width, id / self.width)
    }

    pub fn neighbors(&self, id: TileId) -> impl Iterator<Item = TileId> + '_ {
        let h = self.hex(id);
        (0..6).filter_map(move |d| self.tile(h.neighbor(d)))
    }

    /// Hex distance taking east-west wrap into account.
    pub fn distance(&self, a: TileId, b: TileId) -> i32 {
        let ha = self.hex(a);
        let hb = self.hex(b);
        // Shifting by one full map width in q keeps the same row.
        [-self.width, 0, self.width]
            .iter()
            .map(|&dq| ha.distance(Hex::new(hb.q + dq, hb.r)))
            .min()
            .unwrap()
    }

    /// All tiles within `radius` of `center`, including the center.
    pub fn within(&self, center: TileId, radius: i32) -> Vec<TileId> {
        let c = self.hex(center);
        let mut out = Vec::new();
        for dq in -radius..=radius {
            for dr in (-radius).max(-dq - radius)..=radius.min(-dq + radius) {
                if let Some(t) = self.tile(Hex::new(c.q + dq, c.r + dr)) {
                    if !out.contains(&t) {
                        out.push(t);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_roundtrip() {
        for row in -3..6 {
            for col in -3..6 {
                let h = Hex::from_offset(col, row);
                assert_eq!(h.to_offset(), (col, row));
            }
        }
    }

    #[test]
    fn neighbors_are_distance_one() {
        let g = MapGrid::new(10, 8);
        for id in 0..g.len() as TileId {
            for n in g.neighbors(id) {
                assert_eq!(g.distance(id, n), 1, "tile {id} -> {n}");
            }
        }
    }

    #[test]
    fn wraps_east_west() {
        let g = MapGrid::new(10, 8);
        let west = g.tile(Hex::from_offset(0, 2)).unwrap();
        let east = g.tile(Hex::from_offset(9, 2)).unwrap();
        assert_eq!(g.distance(west, east), 1);
        assert_eq!(g.neighbors(west).count(), 6);
    }

    #[test]
    fn edge_rows_have_fewer_neighbors() {
        let g = MapGrid::new(10, 8);
        assert!(g.neighbors(0).count() < 6);
    }

    #[test]
    fn within_radius_count() {
        let g = MapGrid::new(20, 20);
        let c = g.tile(Hex::from_offset(10, 10)).unwrap();
        assert_eq!(g.within(c, 2).len(), 19);
    }
}
