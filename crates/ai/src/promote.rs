//! Generals' traits for computer players: each level goes to what helps the
//! units it leads most now (siegecraft on the attack, steadfast defending...). A level the AI doesn't spend is spent by the sim at turn end.

use crate::strategy::Strategy;
use sim::civ::CombatCond;
use sim::experience::{generals_to_promote, TraitEffect, TraitId};
use sim::game::{Command, Event, Game, Order, UnitId};

pub fn run(game: &mut Game, s: &Strategy) -> Vec<Event> {
    let mut events = Vec::new();
    for g in generals_to_promote(game, s.me) {
        while let Some(t) = game.units.get(&g).filter(|u| game.pending_traits(u) > 0).and_then(|_| best_trait(game, s, g)) {
            match game.apply(s.me, Command::ChooseTrait { unit: g, trait_id: t }) {
                Ok(e) => events.extend(e),
                Err(_) => break,
            }
        }
    }
    events
}

/// The trait the General `general` should take next.
pub fn best_trait(game: &Game, s: &Strategy, general: UnitId) -> Option<TraitId> {
    let u = &game.units[&general];
    let at_war = s.hostile.iter().enumerate().any(|(p, &h)| h && p as u8 != s.me) || s.war.is_some();
    let attacking = matches!(u.order, Order::Take { .. });
    let level = game.general_level(u);
    let score = |e: TraitEffect| -> i32 {
        match e {
            TraitEffect::Moves(n) => n * 20,
            TraitEffect::Sight(n) => n * 8,
            TraitEffect::Heal(n) => n * if at_war { 2 } else { 1 },
            TraitEffect::Experience(p) => p * (5 - level.min(4) as i32) / 20,
            TraitEffect::Combat { percent, when } => {
                let weight = match when {
                    CombatCond::AttackingCities if at_war || attacking => 14,
                    CombatCond::AttackingCities => 5,
                    CombatCond::Defending if !attacking || !at_war => 12,
                    CombatCond::AttackingUnits if at_war => 11,
                    _ => 8,
                };
                percent * weight / 10
            }
        }
    };
    game.trait_choices(u).into_iter().max_by_key(|&t| (score(game.rules.experience.trait_def(t).effect), std::cmp::Reverse(t)))
}
