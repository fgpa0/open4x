//! Seasons, advancing with turns (the day runs in real time, see daylight.rs).
//! The sim knows the season too (`sim::climate`), for ice on water; this
//! module eases the shown season between turns and drives the visuals.
//!
//! One temperature model drives all snow: a place's mean sea-level
//! temperature, colder with height, swinging with the seasons (little in the
//! tropics, a lot near the poles, less over the sea, opposite in the two
//! hemispheres). The terrain's climate texture holds the mean and the swing;
//! the shaders work out the snow lying from them, the height and the season
//! (lagged a little, so snow lingers into spring). The weather uses the same
//! temperatures to decide between rain and snow.
//!
//! `FOURX_SEASON=0.0..1.0` pins the time of year (the middle of spring at 0,
//! of summer at 0.25, autumn 0.5, winter 0.75, in the north).

use crate::loading::{AppState, InGameSet};
use crate::play::Session;
use bevy::prelude::*;
use std::f32::consts::TAU;

use sim::climate::{year_phase, TURNS_PER_YEAR, YEAR_START as START};
/// How far snow lags the temperature (a fraction of the year).
const SNOW_LAG: f32 = 0.04;
/// Turns' worth of season shown per second while catching up after a turn.
const CATCH_UP: f32 = 0.4;

pub struct SeasonPlugin;

impl Plugin for SeasonPlugin {
    fn build(&self, app: &mut App) {
        let pinned = std::env::var("FOURX_SEASON").ok().and_then(|s| s.parse::<f32>().ok()).map(|s| s.rem_euclid(1.0));
        app.insert_resource(Season { shown: START, pinned })
            .add_systems(OnEnter(AppState::InGame), snap)
            .add_systems(Update, follow_turns.after(crate::play::PlaySet).in_set(InGameSet));
    }
}

/// The time of year shown: years since the start of the game (the fraction
/// is the time of year), easing towards the current turn's.
#[derive(Resource)]
pub struct Season {
    pub shown: f32,
    pinned: Option<f32>,
}

impl Season {
    fn target(&self, turn: u32) -> f32 {
        match self.pinned {
            Some(p) => p,
            None => year_phase(turn),
        }
    }

    /// The seasonal swing's sign and size now (1 at midsummer in the north).
    pub fn now(&self) -> f32 {
        (self.shown * TAU).sin()
    }

    /// The swing on a turn (where the season is heading, while it eases).
    pub fn at_turn(&self, turn: u32) -> f32 {
        (self.target(turn) * TAU).sin()
    }

    /// Whether the north is warming (positive) or cooling, at the same lag.
    pub fn trend(&self) -> f32 {
        ((self.shown - SNOW_LAG) * TAU).cos()
    }

    /// The same, a little earlier: what snow lying follows.
    pub fn for_snow(&self) -> f32 {
        ((self.shown - SNOW_LAG) * TAU).sin()
    }

    /// "Late spring", "Midwinter", ... in the north (the south is opposite).
    pub fn name(&self) -> &'static str {
        self.name_at(1.0)
    }

    /// The season's name at a latitude (half a year apart in the south).
    pub fn name_at(&self, lat: f32) -> &'static str {
        const NAMES: [&str; 12] = [
            "Early spring", "Spring", "Late spring",
            "Early summer", "Midsummer", "Late summer",
            "Early autumn", "Autumn", "Late autumn",
            "Early winter", "Midwinter", "Late winter",
        ];
        // Each season is centred on its quarter of the year.
        let shown = self.shown + if lat < 0.0 { 0.5 } else { 0.0 };
        NAMES[((shown + 0.125).rem_euclid(1.0) * 12.0) as usize % 12]
    }

    /// Year of the game, from 1.
    pub fn year(&self) -> u32 {
        ((self.shown - START).max(0.0) + 1e-4) as u32 + 1
    }
}

fn snap(mut season: ResMut<Season>, session: Res<Session>) {
    season.shown = season.target(session.game.turn);
}

fn follow_turns(time: Res<Time>, session: Res<Session>, mut season: ResMut<Season>) {
    let target = season.target(session.game.turn);
    if season.shown == target {
        return;
    }
    let step = CATCH_UP / TURNS_PER_YEAR as f32 * time.delta_secs();
    // Loading another game or pinning can move it backwards: jump there.
    season.shown = if target < season.shown || target - season.shown > 0.5 { target } else { (season.shown + step).min(target) };
}

/// Latitude (degrees, north positive) at world z (rows are 1.5 units apart).
pub fn latitude(z: f32, map_rows: i32) -> f32 {
    90.0 - (z / 1.5 + 0.5) / map_rows as f32 * 180.0
}

pub use sim::climate::swing;

/// A tile's mean temperature at sea level (mountain tiles are 13 °C colder
/// than their surroundings, see mapgen).
pub fn sea_level_temperature(tile: &sim::map::Tile) -> f32 {
    tile.temperature + if tile.elevation == sim::map::Elevation::Mountains { 13.0 } else { 0.0 }
}

/// A tile's temperature this turn (°C), its yearly mean and its latitude.
pub fn temperature_now(game: &sim::game::Game, tile: sim::hex::TileId, season: &Season) -> (f32, f32, f32) {
    let t = game.map.tile(tile);
    let lat = latitude(crate::layout::tile_center(&game.map.grid, tile).y, game.map.grid.height);
    let land = if game.map.is_water(&game.rules, tile) { 0.0 } else { 1.0 };
    (t.temperature + swing(lat, land) * season.at_turn(game.turn), t.temperature, lat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_year() {
        let at = |shown| Season { shown, pinned: None };
        assert_eq!(at(0.0).name(), "Spring");
        assert_eq!(at(START).name(), "Late spring");
        assert_eq!(at(0.26).name(), "Midsummer");
        assert_eq!(at(0.76).name(), "Midwinter");
        assert_eq!(at(1.26).name(), "Midsummer");
        assert_eq!(at(0.26).name_at(-30.0), "Midwinter");
        assert_eq!(at(START).year(), 1);
        assert_eq!(at(START + 1.0).year(), 2);
    }
}
