//! Research: cities make science, which goes into the technology at the front
//! of the player's research path. Choosing any tech as a goal researches its
//! missing prerequisites first. A new era opens only once every tech of the
//! era before it is known (see `Rules::tech_needs`). Progress is kept per tech, so switching away
//! and back loses nothing. A player who hasn't chosen anything by the end of
//! the turn gets the cheapest available tech.

use crate::city::Build;
use crate::civ::Effect;
use crate::game::{CommandError, Event, Game, PlayerId};
use crate::rules::TechId;

impl Game {
    pub fn knows(&self, player: PlayerId, tech: TechId) -> bool {
        self.players[player as usize].techs[tech as usize]
    }

    /// Not known yet, and every prerequisite is (the whole previous era included).
    pub fn can_research(&self, player: PlayerId, tech: TechId) -> bool {
        !self.knows(player, tech) && self.rules.tech_needs(tech).iter().all(|&r| self.knows(player, r))
    }

    /// Whether the player has the tech a build needs.
    pub fn has_tech_for(&self, player: PlayerId, build: Build) -> bool {
        let tech = match build {
            Build::Unit(u) => self.rules.unit_tech(u),
            Build::Building(b) => self.rules.building_tech(b),
            Build::Project(_) => None,
            Build::Passage(p, _) => Some(self.rules.passage_tech(p)),
        };
        tech.is_none_or(|t| self.knows(player, t))
    }

    /// The latest era a player has reached: that of the latest-era tech they know.
    pub fn era(&self, player: PlayerId) -> crate::rules::EraId {
        let known = &self.players[player as usize].techs;
        (0..self.rules.tech_count()).filter(|&t| known[t]).map(|t| self.rules.tech_era(t as TechId)).max().unwrap_or(0)
    }

    pub fn science_per_turn(&self, player: PlayerId) -> i32 {
        self.cities.values().filter(|c| c.owner == player).map(|c| self.city_output(c.id).science).sum()
    }

    /// The techs to research, in order, to reach `goal` (ending with it).
    /// Empty if the goal is already known.
    pub fn research_path(&self, player: PlayerId, goal: TechId) -> Vec<TechId> {
        let mut needed = vec![false; self.rules.tech_count()];
        let mut stack = vec![goal];
        while let Some(t) = stack.pop() {
            if needed[t as usize] || self.knows(player, t) {
                continue;
            }
            needed[t as usize] = true;
            stack.extend_from_slice(self.rules.tech_needs(t));
        }
        // Prerequisites always come earlier in the rules, so id order is a valid order.
        (0..self.rules.tech_count() as TechId).filter(|&t| needed[t as usize]).collect()
    }

    /// Science a tech costs `player` (cheaper with perks when others know it).
    pub fn tech_cost(&self, player: PlayerId, tech: TechId) -> i32 {
        let cost = self.rules.tech(tech).cost;
        let known_elsewhere = (0..self.players.len() as PlayerId).any(|p| p != player && self.knows(p, tech));
        if !known_elsewhere {
            return cost;
        }
        let discount: i32 = self.civ_effects(player).map(|e| if let Effect::TechDiscount(d) = e { *d } else { 0 }).sum();
        cost - cost * discount.min(90) / 100
    }

    /// Science still needed to finish a tech.
    pub fn research_left(&self, player: PlayerId, tech: TechId) -> i32 {
        (self.tech_cost(player, tech) - self.players[player as usize].research[tech as usize]).max(0)
    }

    /// Turns to research a tech and any missing prerequisites at the current
    /// rate (`None` without science).
    pub fn turns_to_research(&self, player: PlayerId, tech: TechId) -> Option<i32> {
        let rate = self.science_per_turn(player);
        if rate <= 0 {
            return None;
        }
        let stored = self.players[player as usize].science_stored;
        let left: i32 = self.research_path(player, tech).iter().map(|&t| self.research_left(player, t)).sum();
        Some(((left - stored).max(0) + rate - 1) / rate)
    }

    pub(crate) fn set_research(&mut self, player: PlayerId, goal: TechId) -> Result<Vec<Event>, CommandError> {
        if goal as usize >= self.rules.tech_count() {
            return Err(CommandError::NoSuchTech);
        }
        if self.knows(player, goal) {
            return Err(CommandError::AlreadyKnown);
        }
        self.players[player as usize].research_path = self.research_path(player, goal);
        Ok(vec![Event::ResearchChanged { player }])
    }

    /// What a player who hasn't chosen researches: the cheapest available tech.
    fn auto_research(&self, player: PlayerId) -> Option<TechId> {
        (0..self.rules.tech_count() as TechId)
            .filter(|&t| self.can_research(player, t))
            .min_by_key(|&t| (self.rules.tech(t).cost, t))
    }

    /// One turn of research for a player.
    pub(crate) fn process_research(&mut self, player: PlayerId) -> Vec<Event> {
        let mut events = Vec::new();
        let science = self.science_per_turn(player);
        let p = &mut self.players[player as usize];
        p.science_stored += science;
        if p.research_path.is_empty() && p.science_stored > 0 {
            if let Some(t) = self.auto_research(player) {
                self.players[player as usize].research_path.push(t);
                events.push(Event::ResearchChanged { player });
            }
        }
        loop {
            let p = &self.players[player as usize];
            let (Some(&tech), true) = (p.research_path.first(), p.science_stored > 0) else { break };
            let need = self.research_left(player, tech);
            let p = &mut self.players[player as usize];
            let spend = need.min(p.science_stored);
            p.research[tech as usize] += spend;
            p.science_stored -= spend;
            if spend < need {
                break;
            }
            p.techs[tech as usize] = true;
            p.research_path.remove(0);
            events.push(Event::TechResearched { player, tech });
            if tech == self.rules.rail_tech() {
                self.rail_stations(player);
            }
            // Whatever is left over waits in `science_stored` for the next choice.
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use crate::game::{Command, Game};
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Rules;

    fn game() -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings { seed: 3, ..Default::default() });
        Game::new(rules, map, 2)
    }

    #[test]
    fn goals_research_prerequisites_first() {
        let mut g = game();
        let riding = g.rules.tech_id("horseback_riding").unwrap();
        g.apply(0, Command::SetResearch { tech: riding }).unwrap();
        let names: Vec<&str> = g.players[0].research_path.iter().map(|&t| g.rules.tech(t).id.as_str()).collect();
        assert_eq!(names, ["animal_husbandry", "horseback_riding"]);
        // A later era's goal: everything it needs comes before it.
        let construction = g.rules.tech_id("construction").unwrap();
        g.apply(0, Command::SetResearch { tech: construction }).unwrap();
        for (i, &t) in g.players[0].research_path.iter().enumerate() {
            for r in g.rules.tech_needs(t) {
                assert!(g.players[0].research_path[..i].contains(r), "prerequisite after its tech");
            }
        }
    }

    #[test]
    fn a_new_era_needs_all_of_the_last_one() {
        let mut g = game();
        let wheel = g.rules.tech_id("the_wheel").unwrap();
        let ancient: Vec<_> = (0..g.rules.tech_count() as u8).filter(|&t| g.rules.tech_era(t) == 0).collect();
        // The Wheel only names Horseback Riding, but needs every Ancient tech.
        g.apply(0, Command::SetResearch { tech: wheel }).unwrap();
        let path = g.players[0].research_path.clone();
        assert_eq!(path.len(), ancient.len() + 1);
        assert!(ancient.iter().all(|t| path.contains(t)) && path.last() == Some(&wheel));
        for &t in &ancient {
            g.players[0].techs[t as usize] = t != ancient[0];
        }
        assert!(!g.can_research(0, wheel), "one Ancient tech missing");
        g.players[0].techs[ancient[0] as usize] = true;
        assert!(g.can_research(0, wheel));
    }

    #[test]
    fn science_finishes_techs_and_overflow_is_kept() {
        let mut g = game();
        let pottery = g.rules.tech_id("pottery").unwrap();
        g.apply(0, Command::SetResearch { tech: pottery }).unwrap();
        g.players[0].science_stored = g.rules.tech(pottery).cost + 5;
        let events = g.process_research(0);
        assert!(g.knows(0, pottery));
        assert!(events.contains(&crate::game::Event::TechResearched { player: 0, tech: pottery }));
        assert!(g.players[0].research_path.is_empty());
        assert_eq!(g.players[0].science_stored, 5, "overflow waits for the next choice");
        assert!(g.apply(0, Command::SetResearch { tech: pottery }).is_err(), "already known");
    }

    #[test]
    fn locked_builds_cannot_be_queued() {
        let mut g = game();
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).id == "settler").unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        let city = *g.cities.keys().next().unwrap();
        let library = crate::city::Build::Building(g.rules.building_id("library").unwrap());
        assert!(!g.available_builds(city).contains(&library));
        assert!(g.apply(0, Command::SetQueue { city, queue: vec![library] }).is_err());
        let writing = g.rules.tech_id("writing").unwrap();
        for t in g.research_path(0, writing) {
            g.players[0].techs[t as usize] = true;
        }
        assert!(g.available_builds(city).contains(&library));
        assert!(g.apply(0, Command::SetQueue { city, queue: vec![library] }).is_ok());
    }

    #[test]
    fn unchosen_research_is_picked_at_turn_end() {
        let mut g = game();
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).id == "settler").unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        assert!(g.players[0].research_path.is_empty());
        g.apply(0, Command::EndTurn).unwrap();
        assert_eq!(g.players[0].research_path.len(), 1);
        let t = g.players[0].research_path[0];
        assert!(g.players[0].research[t as usize] > 0, "the turn's science went into it");
    }
}
