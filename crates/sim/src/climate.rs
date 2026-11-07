//! Shared climate model: latitude, temperature and the biome thresholds used by
//! map generation and by the renderer (ground colours, vegetation).
//!
//! Bands on a default world (sea level, no weather noise):
//! ice cap > ~75°, tundra ~68-75°, boreal/taiga ~48-68°, temperate ~34-48°,
//! subtropical/tropical below ~34°.

/// Below this mean temperature (°C) land is an ice cap.
pub const ICE_CAP: f32 = -12.0;
/// Below this land is tundra.
pub const TUNDRA: f32 = -5.0;
/// Below this land is boreal: taiga forest, cold steppe, northern meadow.
pub const BOREAL: f32 = 10.0;
/// At or above this land is subtropical/tropical: savanna, hot desert, rainforest.
pub const SUBTROPICAL: f32 = 18.0;
/// At or above this (and wet) jungle grows.
pub const RAINFOREST: f32 = 20.0;

/// Turns per year: the seasons advance with the turns, 10 per season.
pub const TURNS_PER_YEAR: u32 = 40;
/// The time of year on the first turn (0 is mid-spring in the north, 0.25
/// midsummer, 0.75 midwinter): late spring.
pub const YEAR_START: f32 = 0.08;

/// The time of year on a turn: years since the start, the fraction is the time
/// of year.
pub fn year_phase(turn: u32) -> f32 {
    YEAR_START + turn.saturating_sub(1) as f32 / TURNS_PER_YEAR as f32
}

/// How far the seasons have swung at a time of year: 1 at northern midsummer,
/// -1 at northern midwinter.
pub fn season(phase: f32) -> f32 {
    (phase * std::f32::consts::TAU).sin()
}

/// The seasonal swing (°C, signed: positive means warm in the northern summer)
/// at a latitude, for land (1) or sea (0): little in the tropics, a lot near
/// the poles, less over the sea, opposite in the south.
pub fn swing(lat: f32, land: f32) -> f32 {
    let t = ((lat.abs() - 10.0) / 50.0).clamp(0.0, 1.0);
    let size = 4.0 + 16.0 * t * t * (3.0 - 2.0 * t);
    size * (0.5 + 0.5 * land) * lat.signum()
}

/// Ice on water.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeaIce {
    Open,
    /// Ships and embarked units cross it slowly.
    Thin,
    /// Nothing crosses it.
    Pack,
}

/// Latitude in degrees (+90 north edge, -90 south edge) of a map row.
pub fn latitude(row: i32, height: i32) -> f32 {
    90.0 - (row as f32 + 0.5) / height as f32 * 180.0
}

/// Converts precipitation (0..1) into effective moisture: cold climates lose far
/// less water to evaporation, so modest rain still keeps the ground wet enough for
/// vast forests (the taiga), while the same rain in the tropics means savanna.
pub fn effective_moisture(precipitation: f32, temperature: f32) -> f32 {
    let warmth = ((temperature + 5.0) / 25.0).clamp(0.0, 1.0);
    (precipitation * (1.9 - 0.9 * warmth)).clamp(0.0, 1.0)
}

/// Mean sea-level temperature (°C) by latitude, roughly Earth's zonal mean.
pub fn base_temperature(lat: f32) -> f32 {
    28.0 - 55.0 * (lat.abs() / 90.0).powf(1.8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seasons_swing_opposite_ways() {
        assert!(swing(50.0, 1.0) > 15.0 && swing(-50.0, 1.0) < -15.0);
        assert!(swing(0.0, 1.0).abs() <= 4.0);
        assert!(swing(50.0, 0.0) < swing(50.0, 1.0));
        assert!(season(year_phase(1)) > 0.0, "the first turn is in the northern spring");
        assert!((year_phase(1 + TURNS_PER_YEAR) - year_phase(1) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bands_land_at_earthlike_latitudes() {
        let lat_of = |t: f32| (0..=900).map(|i| i as f32 / 10.0).find(|&l| base_temperature(l) < t).unwrap();
        assert!((44.0..52.0).contains(&lat_of(BOREAL)), "boreal at {}", lat_of(BOREAL));
        assert!((65.0..71.0).contains(&lat_of(TUNDRA)), "tundra at {}", lat_of(TUNDRA));
        assert!((70.0..80.0).contains(&lat_of(ICE_CAP)), "ice cap at {}", lat_of(ICE_CAP));
    }
}
