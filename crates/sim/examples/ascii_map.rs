//! Prints a generated map as ASCII, then its landmasses:
//! `cargo run -p sim --example ascii_map [seed] [map type] [size]` (a type id
//! such as `pangaea`; size `small`, `standard` or `large`).

use sim::{map::Elevation, mapgen, rules::Rules};

fn main() {
    let rules = Rules::load_default().unwrap();
    let seed = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let kind = std::env::args().nth(2).map(|k| mapgen::MapType::from_id(&k).expect("unknown map type")).unwrap_or_default();
    let (width, height) = match std::env::args().nth(3).as_deref() {
        Some("small") => (96, 60),
        Some("large") => (160, 100),
        _ => (128, 80),
    };
    let map = mapgen::generate(&rules, &mapgen::MapSettings { seed, kind, width, height, ..Default::default() });
    for row in 0..map.grid.height {
        let mut line = String::new();
        if row & 1 == 1 {
            line.push(' ');
        }
        for col in 0..map.grid.width {
            let t = map.tile((row * map.grid.width + col) as u32);
            let id = (row * map.grid.width + col) as usize;
            let c = match (t.elevation, rules.terrain(t.terrain).id.as_str()) {
                (Elevation::Mountains, _) => '^',
                _ if t.feature.is_some_and(|f| rules.feature(f).id == "ice") => '#',
                (_, "ocean") => ' ',
                _ if !map.start_land.is_empty() && !map.start_land[id] && !map.is_water(&rules, id as u32) => '+',
                (_, "coast") => '.',
                (_, "lake") => 'L',
                _ if t.rivers != 0 => '~',
                (Elevation::Hills, _) => 'n',
                _ => match t.feature.map(|f| rules.feature(f).id.as_str()) {
                    Some("forest") if t.temperature < 6.0 => 'T',
                    Some("forest") => 'f',
                    Some("jungle") => 'j',
                    Some("marsh") => 'm',
                    Some("oasis") => 'o',
                    Some("ice") => '#',
                    _ => match rules.terrain(t.terrain).id.as_str() {
                        "grassland" => 'g',
                        "plains" => 'p',
                        "desert" => 'd',
                        "tundra" => 't',
                        "snow" => '*',
                        _ => '?',
                    },
                },
            };
            line.push(c);
            line.push(' ');
        }
        println!("{line}");
    }
    let land = map.tiles.iter().filter(|t| !rules.terrain(t.terrain).water).count();
    let mut seen = vec![false; map.tiles.len()];
    let mut masses = Vec::new();
    for start in 0..map.tiles.len() as u32 {
        if seen[start as usize] || map.is_water(&rules, start) {
            continue;
        }
        let (mut stack, mut size) = (vec![start], 0);
        seen[start as usize] = true;
        while let Some(id) = stack.pop() {
            size += 1;
            for nb in map.grid.neighbors(id) {
                if !seen[nb as usize] && !map.is_water(&rules, nb) {
                    seen[nb as usize] = true;
                    stack.push(nb);
                }
            }
        }
        masses.push(size);
    }
    masses.sort_unstable_by(|a, b| b.cmp(a));
    println!("land {:.0}%, {} landmasses: {:?}", land as f32 * 100.0 / map.tiles.len() as f32, masses.len(), &masses[..masses.len().min(20)]);
}
