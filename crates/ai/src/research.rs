//! What computer players research: the goal whose unlocks the leader values
//! most for the science it costs. Military units count for more at war or
//! under threat, walls for defensive leaders, libraries for scientific ones.
//! Sailing the seas matters most to civs with no rival or room on their own land.

use sim::city::Build;
use sim::civ::Effect;
use sim::game::{Command, Event, Game};
use sim::rules::{Domain, TechId, UnitClass, UnitTypeId};

use crate::strategy::Strategy;

pub fn choose(game: &mut Game, s: &Strategy) -> Vec<Event> {
    let me = s.me;
    if !game.players[me as usize].research_path.is_empty() {
        return Vec::new();
    }
    let best = (0..game.rules.tech_count() as TechId)
        .filter(|&t| !game.knows(me, t))
        .map(|t| {
            // Prerequisites come with the goal, both their cost and their unlocks.
            let path = game.research_path(me, t);
            let cost: i32 = path.iter().map(|&p| game.research_left(me, p)).sum();
            let worth: f32 = path.iter().map(|&p| value(game, s, p)).sum();
            (worth / cost.max(1) as f32, t)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
    match best {
        Some((_, tech)) => game.apply(me, Command::SetResearch { tech }).unwrap_or_default(),
        None => Vec::new(),
    }
}

/// How much the leader wants what `tech` unlocks.
fn value(game: &Game, s: &Strategy, tech: TechId) -> f32 {
    let me = s.me;
    let d = |v: u8| v as f32 / 10.0;
    let dr = &s.drives;
    let fighting = s.war.is_some() || !s.threats.is_empty();
    let mut value = 1.0;
    let rules = &game.rules;
    for u in 0..rules.units.units.len() as UnitTypeId {
        let def = rules.unit(u);
        if rules.unit_tech(u) != Some(tech) || def.class != UnitClass::Military || !game.civ_builds(me, Build::Unit(u)) {
            continue;
        }
        if def.domain == Domain::Sea {
            let ports = s.my_cities(game).any(|c| game.is_coastal(c.tile));
            value += if ports { 1.0 + if fighting { 2.0 } else { 0.0 } } else { 0.0 };
            continue;
        }
        // Worth more the more it beats what we can build now.
        let role = crate::production::role(def);
        let now = crate::production::best_unit(game, me, role).map_or(0, |b| {
            let b = rules.unit(b);
            b.strength.max(b.ranged)
        });
        let gain = (def.strength.max(def.ranged) - now).max(2) as f32 / 4.0;
        let siege = if role == crate::production::Role::Siege && s.war.is_some_and(|w| !w.raid) { 6.0 } else { 0.0 };
        // Troops the leader favours are worth more to it.
        let liking = 0.4 + 0.12 * role.liking(&s.army) as f32;
        value += gain * liking * (1.0 + 3.0 * d(dr.military) + if fighting { 4.0 } else { 0.0 } + siege);
    }
    // Embarking and crossing the ocean: the way to rivals and new land overseas.
    let isolated = s.rivals.is_empty() && game.turn > 25;
    let boxed = s.free_sites == 0 && game.turn > 25;
    for _ in &rules.techs.techs[tech as usize].grants {
        value += 1.0 + if isolated { 5.0 * (0.5 + d(dr.military).max(d(dr.opportunism))) } else { 0.0 } + if boxed { 4.0 * (0.5 + d(dr.expansion)) } else { 0.0 };
    }
    // Lasting bonuses, worth more the more cities they help.
    let cities = s.my_cities(game).count() as f32;
    for e in &rules.tech(tech).effects {
        value += match e {
            Effect::TileYield { food, production, gold, .. } => {
                cities * 0.5 * (*food as f32 * (0.5 + d(dr.expansion)) + *production as f32 * (0.5 + d(dr.builder)) + *gold as f32 * (0.3 + d(dr.wealth)))
            }
            Effect::BuildingBonus { food, production, gold, science, culture, .. } => {
                cities
                    * 0.5
                    * (*science as f32 * (0.5 + 1.5 * d(dr.science))
                        + *gold as f32 * (0.3 + d(dr.wealth))
                        + *production as f32 * (0.5 + d(dr.builder))
                        + *food as f32 * (0.5 + d(dr.expansion))
                        + *culture as f32 * 0.4)
            }
            Effect::BuildingProduction { percent, .. } => cities * *percent as f32 / 25.0 * (0.5 + d(dr.builder)),
            _ => 1.0,
        };
    }
    for b in 0..rules.buildings.buildings.len() as u8 {
        let def = rules.building(b);
        if rules.building_tech(b) != Some(tech) || !game.civ_builds(me, Build::Building(b)) {
            continue;
        }
        value += def.science as f32 * (0.5 + 1.5 * d(dr.science))
            + def.gold as f32 * (0.3 + d(dr.wealth))
            + def.production as f32 * (0.5 + d(dr.builder))
            + def.food as f32 * (0.5 + d(dr.expansion))
            + def.culture as f32 * 0.4 * (0.5 + d(dr.builder))
            + def.defense as f32 / 20.0 * (d(dr.defense) + if fighting { 1.5 } else { 0.0 });
    }
    value
}
