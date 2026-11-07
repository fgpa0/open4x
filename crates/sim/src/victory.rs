//! Elimination, score and victory.
//!
//! A player is eliminated once they have no cities and no unit that could
//! found one; whatever units they have left are removed. Someone wins by
//! **domination** when they hold every original capital (each player's first
//! city) and every other player still in the game has founded one, or when
//! everyone else is eliminated. With a turn limit, the highest **score** wins
//! once the last turn ends. Play can go on after a victory, but only the first
//! one counts.

use crate::game::{Event, Game, PlayerId};
use crate::rules::Ability;
use serde::{Deserialize, Serialize};

/// Score per city, citizen, technology and every [`LAND_PER_POINT`] tiles of territory.
pub const CITY_POINTS: i32 = 10;
pub const CITIZEN_POINTS: i32 = 4;
pub const TECH_POINTS: i32 = 6;
pub const LAND_PER_POINT: i32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VictoryKind {
    /// Took every capital, or outlasted everyone.
    Domination,
    /// Had the highest score when the turn limit ran out.
    Score,
}

impl VictoryKind {
    pub fn name(self) -> &'static str {
        match self {
            VictoryKind::Domination => "Domination",
            VictoryKind::Score => "Score",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Victory {
    pub winner: PlayerId,
    pub kind: VictoryKind,
    pub turn: u32,
}

/// A player's score and where it comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Score {
    pub cities: i32,
    pub population: i32,
    /// Tiles of territory.
    pub land: i32,
    pub techs: i32,
}

impl Score {
    pub fn total(&self) -> i32 {
        self.cities * CITY_POINTS + self.population * CITIZEN_POINTS + self.techs * TECH_POINTS + self.land / LAND_PER_POINT
    }
}

impl Game {
    pub fn score(&self, player: PlayerId) -> Score {
        let mut s = Score::default();
        for c in self.cities.values().filter(|c| c.owner == player) {
            s.cities += 1;
            s.population += c.population;
        }
        s.land = self.territory.iter().filter(|&&t| t == Some(player)).count() as i32;
        s.techs = self.players[player as usize].techs.iter().filter(|&&k| k).count() as i32;
        s
    }

    /// Players still in the game.
    pub fn alive(&self) -> impl Iterator<Item = PlayerId> + '_ {
        (0..self.players.len() as PlayerId).filter(|&p| !self.players[p as usize].eliminated)
    }

    /// The city that was `player`'s first, wherever it is now.
    pub fn original_capital(&self, player: PlayerId) -> Option<&crate::city::City> {
        self.cities.values().find(|c| c.capital_of == Some(player))
    }

    /// Eliminates players who are out, then checks for a winner. Runs after
    /// attacks and at the end of each turn.
    pub(crate) fn check_game_over(&mut self) -> Vec<Event> {
        let mut events = self.eliminate();
        if self.victory.is_none() {
            if let Some(v) = self.find_victory() {
                self.victory = Some(v);
                events.push(Event::GameOver(v));
            }
        }
        events
    }

    fn eliminate(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let out: Vec<PlayerId> = self
            .alive()
            .filter(|&p| {
                !self.cities.values().any(|c| c.owner == p)
                    && !self.units.values().any(|u| u.owner == p && self.unit_def(u).has(Ability::FoundCity))
            })
            .collect();
        for p in out {
            let left: Vec<_> = self.units.values().filter(|u| u.owner == p).map(|u| u.id).collect();
            for unit in left {
                self.units.remove(&unit);
                events.push(Event::UnitRemoved { unit });
            }
            self.players[p as usize].eliminated = true;
            for q in 0..self.players.len() as PlayerId {
                self.relation_mut(p, q).at_war = false;
                self.relation_mut(q, p).at_war = false;
            }
            self.players[p as usize].visible.fill(0);
            events.push(Event::PlayerEliminated { player: p });
        }
        events
    }

    fn find_victory(&self) -> Option<Victory> {
        let turn = self.turn;
        if self.players.len() > 1 {
            let dominates = |p: PlayerId| {
                self.alive().all(|q| self.original_capital(q).is_some()) && self.cities.values().all(|c| c.capital_of.is_none() || c.owner == p)
            };
            let alive: Vec<PlayerId> = self.alive().collect();
            if let [only] = alive[..] {
                return Some(Victory { winner: only, kind: VictoryKind::Domination, turn });
            }
            if let Some(&p) = alive.iter().find(|&&p| dominates(p)) {
                return Some(Victory { winner: p, kind: VictoryKind::Domination, turn });
            }
        }
        // The turn counter moves on when a turn ends, so the limit's last turn
        // is over once it's passed.
        if self.options.turn_limit.is_some_and(|limit| turn > limit) {
            let winner = self.alive().max_by_key(|&p| (self.score(p).total(), std::cmp::Reverse(p)))?;
            return Some(Victory { winner, kind: VictoryKind::Score, turn });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::city::CityId;
    use crate::game::Command;
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Rules;

    /// A game where every player has founded their capital.
    fn founded(players: usize) -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, players - 1);
        for p in 0..players as PlayerId {
            let settler = g.units.values().find(|u| u.owner == p && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
            g.apply(p, Command::FoundCity { unit: settler }).unwrap();
        }
        g
    }

    fn capital(g: &Game, p: PlayerId) -> CityId {
        g.original_capital(p).unwrap().id
    }

    #[test]
    fn first_cities_are_capitals() {
        let g = founded(3);
        for p in 0..3 {
            assert_eq!(g.original_capital(p).unwrap().owner, p);
        }
        assert!(g.victory.is_none());
    }

    #[test]
    fn losing_everything_eliminates_and_the_last_one_standing_wins() {
        let mut g = founded(2);
        let city = capital(&g, 1);
        g.cities.get_mut(&city).unwrap().owner = 0;
        assert!(g.units.values().any(|u| u.owner == 1), "warrior and scout are left");
        let events = g.apply(0, Command::EndTurn).unwrap();
        assert!(g.players[1].eliminated);
        assert!(g.units.values().all(|u| u.owner != 1), "leftover units are removed");
        assert!(events.iter().any(|e| matches!(e, Event::PlayerEliminated { player: 1 })));
        let v = g.victory.unwrap();
        assert_eq!((v.winner, v.kind), (0, VictoryKind::Domination));
        assert!(events.iter().any(|e| matches!(e, Event::GameOver(_))));
        // Only the first victory counts.
        let events = g.apply(0, Command::EndTurn).unwrap();
        assert!(!events.iter().any(|e| matches!(e, Event::GameOver(_))));
    }

    #[test]
    fn a_settler_keeps_a_player_in_the_game() {
        let mut g = founded(2);
        let city = capital(&g, 1);
        g.cities.get_mut(&city).unwrap().owner = 0;
        let settler = g.rules.unit_type_id("settler").unwrap();
        let tile = g.units.values().find(|u| u.owner == 1).unwrap().tile;
        g.spawn_unit(1, settler, tile);
        g.apply(0, Command::EndTurn).unwrap();
        assert!(!g.players[1].eliminated);
        // Holding every capital still wins.
        assert_eq!(g.victory.map(|v| v.winner), Some(0));
    }

    #[test]
    fn domination_needs_every_capital() {
        let mut g = founded(3);
        let c1 = capital(&g, 1);
        g.cities.get_mut(&c1).unwrap().owner = 0;
        // Player 2 still holds their capital.
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.victory.is_none());
        let c2 = capital(&g, 2);
        g.cities.get_mut(&c2).unwrap().owner = 0;
        g.apply(0, Command::EndTurn).unwrap();
        assert_eq!(g.victory.map(|v| (v.winner, v.kind)), Some((0, VictoryKind::Domination)));
    }

    #[test]
    fn no_domination_while_someone_has_yet_to_settle() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 1);
        let settler = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(0, Command::FoundCity { unit: settler }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.victory.is_none());
    }

    #[test]
    fn the_highest_score_wins_at_the_turn_limit() {
        let mut g = founded(2);
        g.options.turn_limit = Some(3);
        g.players[1].techs.fill(true);
        assert!(g.score(1).total() > g.score(0).total());
        g.apply(0, Command::EndTurn).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert!(g.victory.is_none(), "turn 3 is still to play");
        g.apply(0, Command::EndTurn).unwrap();
        assert_eq!(g.victory, Some(Victory { winner: 1, kind: VictoryKind::Score, turn: 4 }));
    }

    #[test]
    fn score_counts_cities_people_land_and_techs() {
        let g = founded(2);
        let s = g.score(0);
        assert_eq!((s.cities, s.population), (1, 1));
        assert!(s.land >= 6);
        assert_eq!(s.total(), CITY_POINTS + CITIZEN_POINTS + s.land / LAND_PER_POINT + s.techs * TECH_POINTS);
    }
}
