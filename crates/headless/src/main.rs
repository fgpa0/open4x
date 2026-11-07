//! Runs games without a window, for benchmarking and balancing:
//! `cargo run --release -p headless -- [turns] [players] [seed] [turn limit]`
//! (FOURX_MAP=pangaea etc. picks the map type; FOURX_TRUE_STARTS=1 starts
//! civs in their homelands on the Earth map).

use sim::game::{Command, Event, Game, GameSetup};
use sim::{mapgen, rules::Rules};
use std::time::Instant;

fn main() {
    let arg = |i: usize, default: u64| std::env::args().nth(i).and_then(|s| s.parse().ok()).unwrap_or(default);
    let (turns, players, seed) = (arg(1, 100) as u32, arg(2, 4) as usize, arg(3, 1));
    let turn_limit = std::env::args().nth(4).and_then(|s| s.parse().ok());
    let rules = Rules::load_default().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let t = Instant::now();
    let kind = std::env::var("FOURX_MAP").ok().map_or(mapgen::MapType::default(), |m| {
        mapgen::MapType::from_id(&m).unwrap_or_else(|| {
            eprintln!("unknown map type '{m}'");
            std::process::exit(1);
        })
    });
    let map = mapgen::generate(&rules, &mapgen::MapSettings { seed, kind, ..Default::default() });
    println!("map {}x{} in {:.0} ms", map.grid.width, map.grid.height, t.elapsed().as_secs_f64() * 1000.0);

    // Every player is computer-controlled here (player 0 included), with
    // civs picked from the seed.
    let mut setup = GameSetup::random(&rules, players, &[], seed);
    setup.options.turn_limit = turn_limit;
    setup.options.true_starts = std::env::var("FOURX_TRUE_STARTS").is_ok_and(|v| v == "1");
    let mut game = Game::with_setup(rules, map, &setup);
    let mut worst = 0.0f64;
    // Units removed counts deaths, captured civilians and settlers founding cities.
    let (mut fights, mut removed, mut captured) = (0, 0, 0);
    // Unit-turns under Take and Defend orders, and the attacks made under them.
    let (mut take_turns, mut defend_turns, mut take_attacks, mut defend_attacks) = (0u32, 0u32, 0u32, 0u32);
    let (mut posts_lost, mut fort_shots) = (0u32, 0u32);
    let order_of = |game: &Game, unit: sim::game::UnitId| game.units.get(&unit).map(|u| u.order);
    let (mut wars, mut peaces, mut deals, mut traded, mut alliances, mut borders) = (0, 0, 0, 0, 0, 0);
    let (mut bought, mut tiles, mut spent) = (0, 0, 0);
    let mut resources = 0;
    let (mut upgrades, mut upgrade_gold) = (0, 0);
    let (mut sales, mut foreign_sales, mut trade_gold, mut plundered) = (0, 0, 0, 0);
    let mut canals = 0;
    // The turn each player reached each era, and finished the tree.
    let mut eras = vec![vec![None; game.rules.techs.eras.len()]; players];
    let mut finished = vec![None; players];
    let mut generals = 0;
    // Attacks by ships, of them on cities, cities taken by ships, ships lost.
    let mut naval = (0u32, 0u32, 0u32, 0u32);
    // Ships lost to city fire, to ships, to land units, attacking.
    let mut sunk_by = (0u32, 0u32, 0u32, 0u32);
    let mut disbanded = 0;
    // Military units per civ still in the game.
    let military = |game: &Game| {
        let alive = game.players.iter().filter(|p| !p.eliminated).count();
        game.units.values().filter(|u| game.unit_def(u).class == sim::rules::UnitClass::Military).count() as f32 / alive.max(1) as f32
    };
    let mut mid_military = None;
    let mut land_at: Vec<String> = Vec::new();
    let t = Instant::now();
    // FOURX_AI_LOG=n prints every computer player's strategy every n turns.
    let war_log = std::env::var("FOURX_WAR_LOG").is_ok();
    // With the war log: how often an AI changed target city against the same enemy while the old one still stood.
    let mut targets = vec![None; players];
    let mut switches = 0;
    let mut close_switches = 0;
    let log_every: u32 = std::env::var("FOURX_AI_LOG").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    for _ in 0..turns {
        if log_every > 0 && game.turn.is_multiple_of(log_every) {
            println!("turn {}", game.turn);
            for p in 0..players as u8 {
                println!("  {}", ai::strategy::Strategy::assess(&game, p).report(&game));
            }
        }
        let turn = Instant::now();
        let mut events = Vec::new();
        for p in 0..players as u8 {
            let e = ai::take_turn(&mut game, p);
            if war_log {
                log_war_events(&game, &e);
            }
            events.extend(e);
        }
        if war_log {
            for (p, before) in targets.iter_mut().enumerate() {
                let now = ai::strategy::Strategy::assess(&game, p as u8).war.map(|w| (w.enemy, w.target));
                if let (Some((e0, t0)), Some((e1, t1))) = (*before, now) {
                    if e0 == e1 && t0 != t1 && game.cities.get(&t0).is_some_and(|c| c.owner == e0) {
                        switches += 1;
                        let n = |c: sim::city::CityId| game.cities.get(&c).map_or("?".to_string(), |c| format!("{} hp {} garrison {}", c.name, c.hp, game.garrison(c.id).is_some()));
                        let d = |c: sim::city::CityId| attackers_near(&game, p as u8, c);
                        if d(t0).is_some_and(|d| d <= 10) {
                            close_switches += 1;
                        }
                        println!("turn {}: {} switches {} -> {} (nearest attacker {:?} -> {:?} tiles)", game.turn, game.players[p].name, n(t0), n(t1), d(t0), d(t1));
                    }
                }
                *before = now;
            }
            if game.turn.is_multiple_of(5) {
                log_fronts(&game);
            }
        }
        for u in game.units.values() {
            take_turns += matches!(u.order, sim::game::Order::Take { .. }) as u32;
            defend_turns += matches!(u.order, sim::game::Order::Defend { .. }) as u32;
        }
        for e in &events {
            if let Event::Combat(r) = e {
                if let sim::combat::Attacker::Unit { id, .. } = r.attacker {
                    take_attacks += matches!(order_of(&game, id), Some(sim::game::Order::Take { .. })) as u32;
                    defend_attacks += matches!(order_of(&game, id), Some(sim::game::Order::Defend { .. })) as u32;
                }
            }
        }
        events.extend(game.apply(0, Command::EndTurn).expect("end turn"));
        for e in events {
            match e {
                Event::Combat(r) => {
                    fights += 1;
                    fort_shots += matches!(r.attacker, sim::combat::Attacker::Fort(_)) as u32;
                    let sea = |k: sim::rules::UnitTypeId| game.rules.unit(k).domain == sim::rules::Domain::Sea;
                    if let sim::combat::Attacker::Unit { kind, .. } = r.attacker {
                        if sea(kind) {
                            naval.0 += 1;
                            naval.1 += r.city.is_some() as u32;
                            naval.2 += (r.city.is_some() && r.defender_destroyed) as u32;
                        }
                    }
                    if r.defender_kind.is_some_and(sea) && r.defender_destroyed {
                        naval.3 += 1;
                        match r.attacker {
                            sim::combat::Attacker::City(_) => sunk_by.0 += 1,
                            sim::combat::Attacker::Unit { kind, .. } if sea(kind) => sunk_by.1 += 1,
                            _ => sunk_by.2 += 1,
                        }
                    }
                    // Ships lost attacking.
                    if let sim::combat::Attacker::Unit { kind, .. } = r.attacker {
                        if sea(kind) && r.attacker_died {
                            naval.3 += 1;
                            sunk_by.3 += 1;
                        }
                    }
                }
                Event::UnitRemoved { .. } => removed += 1,
                Event::CityCaptured { .. } => captured += 1,
                Event::PostDestroyed { .. } => posts_lost += 1,
                Event::CityBuilt { build: sim::city::Build::Unit(k), .. } => {
                    generals += game.rules.unit(k).has(sim::rules::Ability::General) as u32;
                }
                Event::UnitDisbanded { .. } => disbanded += 1,
                Event::WarDeclared { .. } => wars += 1,
                Event::DealMade { from, to, deal } => {
                    deals += 1;
                    peaces += deal.peace as u32;
                    alliances += deal.alliance as u32;
                    borders += deal.give.iter().chain(&deal.get).any(|&i| i == sim::diplomacy::Item::OpenBorders) as u32;
                    resources += deal.give.iter().chain(&deal.get).any(|i| matches!(i, sim::diplomacy::Item::Resource(_))) as u32;
                    if log_every > 0 {
                        println!("turn {}: {} and {} agree: {:?}", game.turn, game.players[from as usize].name, game.players[to as usize].name, deal);
                    }
                }
                Event::CityTraded { .. } => traded += 1,
                Event::Traded(r) => {
                    sales += 1;
                    foreign_sales += (r.partner_gold > 0) as u32;
                    trade_gold += r.gold + r.partner_gold;
                }
                Event::Plundered { .. } => plundered += 1,
                Event::TerrainChanged { .. } => canals += 1,
                Event::Bought { gold, .. } => {
                    bought += 1;
                    spent += gold;
                }
                Event::TileBought { gold, .. } => {
                    tiles += 1;
                    spent += gold;
                }
                Event::UnitUpgraded { gold, .. } => {
                    upgrades += 1;
                    upgrade_gold += gold;
                }
                Event::TechResearched { player, tech } => {
                    let p = player as usize;
                    eras[p][game.rules.tech_era(tech) as usize].get_or_insert(game.turn);
                    if game.players[p].techs.iter().all(|&t| t) {
                        finished[p] = Some(game.turn);
                    }
                }
                Event::PlayerEliminated { player } => println!("turn {}: {} eliminated", game.turn, game.players[player as usize].name),
                Event::GameOver(v) => println!("turn {}: {} wins by {}", v.turn, game.players[v.winner as usize].name, v.kind.name()),
                _ => {}
            }
        }
        worst = worst.max(turn.elapsed().as_secs_f64());
        if [50, 100, 150, 200].contains(&game.turn) && !game.cities.is_empty() {
            let owned = game.territory.iter().filter(|t| t.is_some()).count();
            land_at.push(format!("{:.1} at turn {}", owned as f64 / game.cities.len() as f64, game.turn));
        }
        if game.turn == turns / 2 {
            mid_military = Some((game.turn, military(&game)));
        }
    }
    let total = t.elapsed().as_secs_f64();
    println!(
        "{turns} turns, {players} players: {:.2} ms/turn average, {:.2} ms worst",
        total * 1000.0 / turns as f64,
        worst * 1000.0
    );
    if war_log {
        println!("war log: {switches} target switches against the same enemy, {close_switches} with an attacker within 10 tiles of the old target");
    }
    println!("combat: {fights} attacks, {removed} units removed, {captured} cities captured");
    println!("navy: {} attacks by ships ({} on cities, {} cities taken from the sea), {} ships lost in fights (city fire {}, ships {}, land units {}, attacking {})", naval.0, naval.1, naval.2, naval.3, sunk_by.0, sunk_by.1, sunk_by.2, sunk_by.3);
    if let Some((turn, n)) = mid_military {
        println!("units: {n:.1} military units per civ at turn {turn}, {:.1} at the end; {generals} Generals and Admirals built, {disbanded} units disbanded for upkeep", military(&game));
    }
    println!("borders: tiles owned per city {}", land_at.join(", "));
    println!("orders: {take_turns} unit-turns sent to take cities ({take_attacks} attacks), {defend_turns} defending ({defend_attacks} attacks)");
    let towers = game.works.iter().filter(|w| w.watchtower).count();
    println!(
        "posts: {} forts and {towers} watchtowers standing at the end, {posts_lost} pulled down in wars, {fort_shots} shots from forts",
        game.forts().count()
    );
    println!("diplomacy: {wars} wars declared, {peaces} peace treaties, {alliances} alliances, {borders} with open borders, {deals} deals, {traded} cities traded");
    println!("gold: {bought} builds and {tiles} tiles bought for {spent} gold, {upgrades} units upgraded for {upgrade_gold} gold");
    println!("trade: {sales} sales ({foreign_sales} abroad) for {trade_gold} gold, {plundered} merchants plundered");
    let improved = game.works.iter().filter(|w| w.improvement.is_some()).count();
    let roads = game.works.iter().filter(|w| w.road).count();
    let tunnels = game.works.iter().filter(|w| w.tunnel).count();
    let bridges = game.works.iter().filter(|w| w.bridge).count();
    println!("land: {improved} improvements, {roads} road tiles, {canals} tiles dug out for canals, {tunnels} tunnels, {bridges} bridges, {resources} deals with resources");
    // FOURX_ARMY_LOG=1 lists every military unit's order and every city's defence at the end.
    if std::env::var("FOURX_ARMY_LOG").is_ok_and(|v| v == "1") {
        for u in game.units.values().filter(|u| game.unit_def(u).class == sim::rules::UnitClass::Military) {
            let city = game.city_at(u.tile).map_or(String::new(), |c| format!(" in {}", c.name));
            let general = if game.unit_def(u).has(sim::rules::Ability::General) {
                let traits: Vec<&str> = u.traits.iter().map(|&t| game.rules.experience.trait_def(t).name.as_str()).collect();
                format!(", level {} [{}]", game.general_level(u), traits.join(", "))
            } else {
                String::new()
            };
            println!(
                "  unit {:4} of {:12} {:14} hp {:3} at {:5}{city}, order {:?}, xp {}{general}",
                u.id,
                game.players[u.owner as usize].name,
                game.unit_def(u).id,
                u.hp,
                u.tile,
                u.order,
                u.xp
            );
        }
        for c in game.cities.values() {
            println!(
                "  city {:14} of {:12} hp {:3} strength {:2} walls {:3} garrison {}{}",
                c.name,
                game.players[c.owner as usize].name,
                c.hp,
                game.city_strength(c.id),
                game.city_walls(c.id),
                game.garrison(c.id).is_some(),
                if game.besieged(c.id) { ", besieged" } else { "" }
            );
        }
    }
    for (i, p) in game.players.iter().enumerate() {
        let cities: Vec<_> = game.cities.values().filter(|c| c.owner as usize == i).collect();
        let pop: i32 = cities.iter().map(|c| c.population).sum();
        let units = game.units.values().filter(|u| u.owner as usize == i).count();
        let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
        for u in game.units.values().filter(|u| u.owner as usize == i) {
            *kinds.entry(game.unit_def(u).id.as_str()).or_default() += 1;
        }
        println!(
            "  {:12} score {:4}  cities {:2}  population {:3}  units {:2}  gold {:5}  explored {:5}  techs {:2} (eras from turn {}{})  luxuries {}  {:?}{}",
            p.name,
            game.score(i as u8).total(),
            cities.len(),
            pop,
            units,
            p.gold,
            p.explored.iter().filter(|&&e| e).count(),
            p.techs.iter().filter(|&&t| t).count(),
            eras[i].iter().skip(1).map(|t| t.map_or("-".to_string(), |t| t.to_string())).collect::<Vec<_>>().join("/"),
            finished[i].map_or(String::new(), |t| format!(", tree done {t}")),
            game.luxuries(i as u8),
            kinds,
            if p.eliminated { "  (eliminated)" } else { "" }
        );
    }
}

/// FOURX_WAR_LOG: wars, peace and every attack on a city, with the city's state after it.
fn log_war_events(game: &Game, events: &[Event]) {
    let name = |p: u8| game.players[p as usize].name.as_str();
    for e in events {
        match e {
            Event::WarDeclared { .. } | Event::CityCaptured { .. } | Event::CityRazed { .. } => println!("turn {}: {:?}", game.turn, e),
            Event::DealMade { from, to, deal } if deal.peace => println!("turn {}: peace {} / {}", game.turn, name(*from), name(*to)),
            Event::Combat(r) if r.city.is_some() => {
                let c = r.city.unwrap();
                let (cname, hp, inside) = game
                    .cities
                    .get(&c)
                    .map(|city| (city.name.as_str(), city.hp, game.garrison(c).is_some()))
                    .unwrap_or(("?", -1, false));
                println!(
                    "turn {}: {} hits {} ({}) {}: dealt {} took {}{}, city hp {} garrison {}",
                    game.turn,
                    name(r.attacker_owner),
                    cname,
                    name(r.defender_owner),
                    if r.ranged { "ranged" } else { "melee" },
                    r.damage_dealt,
                    r.damage_taken,
                    if r.attacker_died { ", died" } else { "" },
                    hp,
                    inside
                );
            }
            _ => {}
        }
    }
}

/// The distance from `p`'s nearest unit sent to take `city` to it.
fn attackers_near(game: &Game, p: u8, city: sim::city::CityId) -> Option<i32> {
    let c = game.cities.get(&city)?;
    game.units
        .values()
        .filter(|u| u.owner == p && matches!(u.order, sim::game::Order::Take { .. }))
        .map(|u| game.map.grid.distance(u.tile, c.tile))
        .min()
}

/// FOURX_WAR_LOG: for each player at war, its units sent to take cities,
/// their health and how far the nearest is from its target.
fn log_fronts(game: &Game) {
    let n = game.players.len() as u8;
    for p in 0..n {
        let enemies: Vec<u8> = (0..n).filter(|&q| q != p && game.at_war(p, q)).collect();
        if enemies.is_empty() || game.players[p as usize].eliminated {
            continue;
        }
        let mut sent: std::collections::BTreeMap<sim::city::CityId, (usize, i32)> = Default::default();
        for u in game.units.values().filter(|u| u.owner == p) {
            if let sim::game::Order::Take { city } = u.order {
                let e = sent.entry(city).or_default();
                e.0 += 1;
                e.1 += u.hp;
            }
        }
        let parts: Vec<String> = sent
            .iter()
            .map(|(&c, &(k, hp))| {
                let name = game.cities.get(&c).map_or("?", |c| c.name.as_str());
                format!("{k} units (hp {}) on {name}, nearest {:?}", hp / k as i32, attackers_near(game, p, c))
            })
            .collect();
        let war = ai::strategy::Strategy::assess(game, p).war.map(|w| {
            let c = &game.cities[&w.target];
            format!("{}{} hp {} garrison {}", if w.raid { "raid " } else { "" }, c.name, c.hp, game.garrison(w.target).is_some())
        });
        println!("turn {}: {} target {:?} at war with {:?}: {}", game.turn, game.players[p as usize].name, war, enemies.iter().map(|&q| game.players[q as usize].name.as_str()).collect::<Vec<_>>(), parts.join(" | "));
    }
}
