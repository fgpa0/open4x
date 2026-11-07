//! How well starts match civs' start biases over many maps:
//! `cargo run --release -p sim --example start_bias [maps] [players]`.
//! Per civ: average count within 2 tiles of the start of desert, tundra,
//! taiga (boreal forest), jungle, marsh, hills, mountains and lakes, how often it starts on
//! the coast or a river, the start's temperature and food.

use sim::{
    game::{Game, GameSetup},
    map::Elevation,
    mapgen::{generate, MapSettings},
    rules::{Ability, Rules},
};

fn main() {
    let rules = Rules::load_default().unwrap();
    let mut args = std::env::args().skip(1).map(|a| a.parse::<u64>().unwrap());
    let maps = args.next().unwrap_or(12);
    let players = args.next().unwrap_or(6) as usize;
    let civs = rules.civs.civs.len();
    // Per civ: starts, [desert, tundra, taiga, jungle, marsh, hills, mountains, lake, coast, river, temperature, food].
    let mut sums = vec![(0u32, [0f32; 12]); civs];
    for seed in 1..=maps {
        let map = generate(&rules, &MapSettings { seed, ..Default::default() });
        let setup = GameSetup::random(&rules, players, &[], seed);
        let g = Game::with_setup(rules.clone(), map, &setup);
        for (p, player) in g.players.iter().enumerate() {
            let Some(start) = g.units.values().find(|u| u.owner == p as u8 && g.unit_def(u).has(Ability::FoundCity)).map(|u| u.tile) else {
                continue;
            };
            let area = g.map.grid.within(start, 2);
            let count = |f: &dyn Fn(&sim::map::Tile) -> bool| area.iter().filter(|&&t| f(g.map.tile(t))).count() as f32;
            let terrain = |t: &sim::map::Tile, id: &str| rules.terrain(t.terrain).id == id;
            let feature = |t: &sim::map::Tile, id: &str| t.feature.is_some_and(|f| rules.feature(f).id == id);
            let food: i32 = area.iter().map(|&t| g.tile_yields(p as u8, t).food).sum();
            let s = &mut sums[player.civ as usize];
            s.0 += 1;
            for (i, v) in [
                count(&|t| terrain(t, "desert")),
                count(&|t| terrain(t, "tundra")),
                count(&|t| feature(t, "forest") && (sim::climate::TUNDRA..sim::climate::BOREAL).contains(&t.temperature)),
                count(&|t| feature(t, "jungle")),
                count(&|t| feature(t, "marsh")),
                count(&|t| t.elevation == Elevation::Hills),
                count(&|t| t.elevation == Elevation::Mountains),
                count(&|t| terrain(t, "lake")),
                g.is_coastal(start) as u8 as f32,
                g.map.has_river(start) as u8 as f32,
                g.map.tile(start).temperature,
                food as f32,
            ]
            .into_iter()
            .enumerate()
            {
                s.1[i] += v;
            }
        }
    }
    println!("{:<12} {:>3} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}  bias", "civ", "n", "desert", "tundra", "taiga", "jungle", "marsh", "hills", "mount", "lake", "coast%", "river%", "temp", "food");
    for (c, (n, s)) in sums.iter().enumerate() {
        if *n == 0 {
            continue;
        }
        let def = &rules.civs.civs[c];
        let avg = |i: usize| s[i] / *n as f32;
        println!(
            "{:<12} {:>3} {:>6.1} {:>6.1} {:>6.1} {:>6.1} {:>6.1} {:>6.1} {:>6.1} {:>6.1} {:>6.0} {:>6.0} {:>6.1} {:>6.1}  {:?}",
            def.id, n, avg(0), avg(1), avg(2), avg(3), avg(4), avg(5), avg(6), avg(7), avg(8) * 100.0, avg(9) * 100.0, avg(10), avg(11), def.start_bias
        );
    }
}
