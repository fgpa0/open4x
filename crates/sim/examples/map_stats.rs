//! Prints mountain range statistics: `cargo run -p sim --example map_stats [seed]`.

use sim::{hex::TileId, map::Elevation, mapgen, rules::Rules};

fn main() {
    let rules = Rules::load_default().unwrap();
    let seed = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7);
    let map = mapgen::generate(&rules, &mapgen::MapSettings { seed, ..Default::default() });
    let mut seen = vec![false; map.tiles.len()];
    let mut ranges = Vec::new();
    for start in 0..map.tiles.len() {
        if seen[start] || map.tiles[start].elevation != Elevation::Mountains {
            continue;
        }
        let mut stack = vec![start as TileId];
        seen[start] = true;
        let mut size = 0;
        while let Some(id) = stack.pop() {
            size += 1;
            for nb in map.grid.neighbors(id) {
                if !seen[nb as usize] && map.tile(nb).elevation == Elevation::Mountains {
                    seen[nb as usize] = true;
                    stack.push(nb);
                }
            }
        }
        ranges.push(size);
    }
    ranges.sort_unstable_by(|a, b| b.cmp(a));
    let total: usize = ranges.iter().sum();
    let hills = map.tiles.iter().filter(|t| t.elevation == Elevation::Hills).count();
    let rivers = map.tiles.iter().filter(|t| t.rivers != 0).count();
    println!("mountains {total}, hills {hills}, river tiles {rivers}, ranges {}, sizes {:?}", ranges.len(), ranges);
}
