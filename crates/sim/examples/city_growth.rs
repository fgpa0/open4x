//! Prints how a single city develops: `cargo run -p sim --example city_growth [seed]`.

use sim::game::{Command, Game};
use sim::rules::{Ability, Rules};
use sim::mapgen;

fn main() {
    let rules = Rules::load_default().unwrap();
    let seed = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let map = mapgen::generate(&rules, &mapgen::MapSettings { seed, ..Default::default() });
    let mut game = Game::new(rules, map, 1);
    let settler = game.units.values().find(|u| u.owner == 0 && game.unit_def(u).has(Ability::FoundCity)).unwrap().id;
    game.apply(0, Command::FoundCity { unit: settler }).unwrap();
    let city = *game.cities.keys().next().unwrap();
    for turn in 1..=100 {
        let events = game.apply(0, Command::EndTurn).unwrap();
        for e in events {
            if let sim::game::Event::CityBuilt { build, .. } = e {
                let name = game.build_name(build).to_string();
                println!("turn {turn:3}: built {name}");
            }
        }
        if turn == 100 && std::env::var("DETAIL").is_ok() {
            let c = &game.cities[&city];
            for t in game.district_tiles(city) {
                let t = t as usize;
                {
                    let tile = game.map.tile(t as u32);
                    let y = game.map.yields(&game.rules, t as u32);
                    let worked = if c.worked.contains(&(t as u32)) { "WORKED" } else { "" };
                    println!("  {} {:?} {:?}: {}/{}/{} {worked}", game.rules.terrain(tile.terrain).id, tile.elevation, tile.feature.map(|f| game.rules.feature(f).id.clone()), y.food, y.production, y.gold);
                }
            }
        }
        if turn % 10 == 0 {
            let c = &game.cities[&city];
            let out = game.city_output(city);
            println!(
                "turn {turn:3}: pop {} food {:+} prod {} gold {} | tiles {} | treasury {}",
                c.population,
                out.food_surplus,
                out.yields.production,
                out.yields.gold,
                game.district_tiles(city).len(),
                game.players[0].gold
            );
        }
    }
}
