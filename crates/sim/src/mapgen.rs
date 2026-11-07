//! Procedural map generation.
//!
//! 1. Continents from noise, relief from a simple plate model (see `tectonics`):
//!    mountain chains along colliding plates, flanked by hills, then plains.
//! 2. Climate modelled on Earth: temperature falls with latitude and altitude;
//!    rainfall follows the global circulation belts (wet equator, dry subtropics
//!    around 25-30°, wet mid-latitudes, dry poles) and is carried inland by the
//!    prevailing winds (easterly trades, mid-latitude westerlies, polar easterlies),
//!    so windward coasts are wet, interiors dry and mountains cast rain shadows.
//! 3. Biomes from temperature x rainfall, then forests, jungle, marsh, sea ice.
//! 4. Rivers rise at the foot of mountains and follow drainage to the sea.
//! 5. Resources (see `resource.rs`).

use crate::climate::{self, base_temperature, latitude};
use crate::earth;
use crate::hex::{MapGrid, TileId};
use crate::shapes;
pub use crate::shapes::MapType;
use crate::tectonics;
use crate::map::{Elevation, Map, Tile};
use crate::rules::{FeatureId, Rules, TerrainId};
use noise::{Fbm, MultiFractal, NoiseFn, Perlin, RidgedMulti};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::f64::consts::TAU;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MapSettings {
    pub width: i32,
    pub height: i32,
    pub seed: u64,
    /// The layout of land and sea.
    #[serde(default)]
    pub kind: MapType,
    /// Fraction of tiles that are land (0..1).
    pub land_fraction: f32,
    /// Fraction of land tiles that are mountains.
    pub mountain_fraction: f32,
    /// Fraction of land tiles that are hills.
    pub hill_fraction: f32,
    /// Rivers per 1000 land tiles.
    pub river_density: f32,
    /// Shifts every temperature (°C): negative for an ice-age world.
    pub temperature_offset: f32,
    /// Multiplies rainfall: below 1 for a dry world.
    pub rainfall: f32,
}

impl Default for MapSettings {
    fn default() -> Self {
        Self {
            width: 128,
            height: 80,
            seed: 1,
            kind: MapType::Fractal,
            land_fraction: 0.38,
            mountain_fraction: 0.07,
            hill_fraction: 0.16,
            river_density: 20.0,
            temperature_offset: 0.0,
            rainfall: 1.0,
        }
    }
}

/// Terrain and feature ids the generator places.
struct Ids {
    ocean: TerrainId,
    coast: TerrainId,
    grassland: TerrainId,
    plains: TerrainId,
    desert: TerrainId,
    tundra: TerrainId,
    snow: TerrainId,
    forest: Option<FeatureId>,
    jungle: Option<FeatureId>,
    marsh: Option<FeatureId>,
    oasis: Option<FeatureId>,
    lake: Option<TerrainId>,
}

impl Ids {
    fn new(rules: &Rules) -> Self {
        let t = |id: &str| {
            rules
                .terrain_id(id)
                .unwrap_or_else(|| panic!("map generator needs terrain '{id}' in terrain.ron"))
        };
        Self {
            ocean: t("ocean"),
            coast: t("coast"),
            grassland: t("grassland"),
            plains: t("plains"),
            desert: t("desert"),
            tundra: t("tundra"),
            snow: t("snow"),
            forest: rules.feature_id("forest"),
            jungle: rules.feature_id("jungle"),
            marsh: rules.feature_id("marsh"),
            oasis: rules.feature_id("oasis"),
            lake: rules.terrain_id("lake"),
        }
    }
}

/// Samples noise on a cylinder so the map wraps east-west seamlessly.
fn cyl(n: &impl NoiseFn<f64, 3>, x: f64, y: f64, w: f64, scale: f64) -> f64 {
    let a = x / w * TAU;
    let r = w / TAU * scale;
    n.get([a.cos() * r, a.sin() * r, y * scale])
}

/// Value at the given quantile (0..1) of `values`.
fn quantile(values: &[f64], q: f32) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i = ((v.len() as f32 * q) as usize).min(v.len() - 1);
    v[i]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Piecewise-linear interpolation through (x, y) points sorted by x.
fn curve(points: &[(f32, f32)], x: f32) -> f32 {
    if x <= points[0].0 {
        return points[0].1;
    }
    for w in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    points[points.len() - 1].1
}

/// Rainfall potential by latitude from the global circulation cells: the
/// equatorial rain belt (ITCZ), subtropical high-pressure deserts, the mid-latitude
/// storm tracks and the dry polar highs.
fn rainfall_belt(lat: f32) -> f32 {
    curve(
        &[(0.0, 1.0), (8.0, 0.95), (16.0, 0.6), (24.0, 0.28), (30.0, 0.3), (40.0, 0.62), (52.0, 0.8), (62.0, 0.6), (75.0, 0.32), (90.0, 0.15)],
        lat.abs(),
    )
}

/// Prevailing surface wind direction along a row: -1 blows westward (trade
/// winds, polar easterlies), +1 eastward (westerlies).
fn wind_direction(lat: f32) -> i32 {
    let a = lat.abs();
    if (30.0..60.0).contains(&a) { 1 } else { -1 }
}

/// The seed every Earth map is made with: Earth is one fixed world (at each
/// size and sea level), whatever the game's seed.
pub const EARTH_SEED: u64 = 1;

pub fn generate(rules: &Rules, s: &MapSettings) -> Map {
    let fixed;
    let s = if s.kind == MapType::Earth && s.seed != EARTH_SEED {
        fixed = MapSettings { seed: EARTH_SEED, ..s.clone() };
        &fixed
    } else {
        s
    };
    let ids = Ids::new(rules);
    let grid = MapGrid::new(s.width, s.height);
    let n = grid.len();
    let seed = s.seed as u32;
    let mut rng = ChaCha8Rng::seed_from_u64(s.seed);

    // --- Relief -------------------------------------------------------------
    let continents = Fbm::<Perlin>::new(seed).set_octaves(6).set_persistence(0.5);
    let ridges = RidgedMulti::<Perlin>::new(seed.wrapping_add(1)).set_octaves(4);
    let climate_noise = Fbm::<Perlin>::new(seed.wrapping_add(3)).set_octaves(3);
    let highland_noise = Fbm::<Perlin>::new(seed.wrapping_add(4)).set_octaves(3);
    let crust = tectonics::simulate(s.width, s.height, s.seed);
    let shape = shapes::shape(s.kind, s.width, s.height, s.seed);
    // The real world: land and relief from data, noise only for detail.
    let earth = (s.kind == MapType::Earth).then(|| earth_layers(rules, &grid, s));

    let w = s.width as f64;
    let h = s.height as f64;
    let mut height = vec![0.0f64; n];
    // How strongly each tile is pushed up into mountains: plate uplift, varied a
    // little so ranges have individual peaks.
    let mut mountain_score = vec![0.0f64; n];
    for (id, hgt) in height.iter_mut().enumerate() {
        let (col, row) = grid.offset(id as TileId);
        let (x, y) = tectonics::tile_pos(col, row);
        let lat = (row as f64 + 0.5) / h * 2.0 - 1.0;
        let c = crust[id];
        let base = cyl(&continents, x, y, w, 0.045);
        // Keep the very poles mostly ocean, but leave high latitudes plenty of land.
        let polar = lat.abs().powf(10.0) * 0.6;
        *hgt = match (&shape, &earth) {
            (_, Some(e)) => {
                let detail = base * 0.6 + c.uplift * 0.2;
                let v = e.cover[id].land + e.cover[id].lake - e.threshold;
                if v > 0.0 {
                    (0.02 + v * 0.25 + e.relief[id].mountain.max(0.0) * 0.5 + e.relief[id].hill.max(0.0) * 0.15 + detail * 0.12).max(0.005)
                } else {
                    (v * 0.6 - 0.04 + detail * 0.08).min(-0.005)
                }
            }
            (None, None) => base * 0.85 + c.continental * 0.18 + c.uplift * 0.45 + c.ancient * 0.12 - c.rift * 0.2 - polar,
            // The map type's mask sets the layout; noise and plates add the
            // detail, fading out where the mask demands sea (at -1).
            (Some(shape), None) => {
                let m = shape.value[id];
                let detail = base * 0.6 + c.continental * 0.06 + c.uplift * 0.4 + c.ancient * 0.1 - c.rift * 0.1;
                m + detail * (m + 1.0).clamp(0.0, 1.0) - polar
            }
        };
        // Active ranges dominate; ancient seams give lower ranges and hill belts
        // across plate interiors.
        mountain_score[id] = match &earth {
            Some(e) => e.relief[id].mountain + cyl(&ridges, x, y, w, 0.15) * 0.1,
            None => (c.uplift + c.ancient * 0.5) * (0.85 + cyl(&ridges, x, y, w, 0.15) * 0.3),
        };
    }

    let sea_level = if earth.is_some() { 0.0 } else { quantile(&height, 1.0 - s.land_fraction * s.kind.land_scale()) };
    let land_scores: Vec<f64> =
        (0..n).filter(|&i| height[i] > sea_level).map(|i| mountain_score[i]).collect();
    // Mountains: the strongest uplift on land (never on weak uplift, even if that
    // means fewer mountains). Hills: the next band, i.e. foothills flanking ranges.
    let mountain_level = quantile(&land_scores, 1.0 - s.mountain_fraction).max(0.3);
    let hill_level = quantile(&land_scores, 1.0 - s.mountain_fraction - s.hill_fraction * 0.7).max(0.12);
    let (hmin, hmax) = height.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));

    let mut tiles: Vec<Tile> = (0..n)
        .map(|id| {
            let hgt = height[id];
            let land = hgt > sea_level;
            let (col, row) = grid.offset(id as TileId);
            let (x, y) = tectonics::tile_pos(col, row);
            // Old, worn-down uplands away from active ranges supply the rest of
            // the hills (think Scottish highlands or the Appalachians).
            let highland = cyl(&highland_noise, x, y, w, 0.08) > if earth.is_some() { 0.45 } else { 0.38 };
            let elevation = if !land {
                Elevation::Flat
            } else if let Some(e) = &earth {
                if e.relief[id].mountain > 0.0 {
                    Elevation::Mountains
                } else if e.relief[id].hill > 0.0 || highland {
                    Elevation::Hills
                } else {
                    Elevation::Flat
                }
            } else if mountain_score[id] >= mountain_level {
                Elevation::Mountains
            } else if mountain_score[id] >= hill_level || highland {
                Elevation::Hills
            } else {
                Elevation::Flat
            };
            Tile {
                terrain: if land { ids.grassland } else { ids.ocean },
                elevation,
                feature: None,
                rivers: 0,
                height: ((hgt - hmin) / (hmax - hmin)) as f32,
                temperature: 0.0,
                moisture: 0.0,
                resource: None,
            }
        })
        .collect();
    let is_land = |tiles: &[Tile], id: usize| tiles[id].terrain != ids.ocean;
    if earth.is_none() {
        grow_small_ranges(&grid, &mut tiles, &mountain_score, &ids, &mut rng);
        fill_mountain_gaps(&grid, &mut tiles, &mountain_score, &ids, &mut rng);
        ensure_relief(&grid, &mut tiles, &mountain_score, &ids);
    }
    thin_ranges(&grid, &mut tiles, &mountain_score);

    // --- Temperature ----------------------------------------------------------
    for id in 0..n {
        let (col, row) = grid.offset(id as TileId);
        let lat = latitude(row, s.height);
        let land_height = ((height[id] - sea_level) / (hmax - sea_level)).max(0.0) as f32;
        let lapse = match tiles[id].elevation {
            Elevation::Mountains => 13.0,
            Elevation::Hills => 4.5,
            Elevation::Flat => 0.0,
        } + land_height * 6.0;
        let x = col as f64 + if row & 1 == 1 { 0.5 } else { 0.0 };
        let wobble = cyl(&climate_noise, x, row as f64 * 0.866, w, 0.06) as f32 * 4.0;
        let local = earth.as_ref().map_or(0.0, |e| e.climate[id].0);
        tiles[id].temperature = base_temperature(lat) - lapse + wobble + local + s.temperature_offset;
    }

    // --- Rainfall: humidity carried by the prevailing winds -----------------
    let mut humidity = vec![0.0f32; n];
    for row in 0..s.height {
        let lat = latitude(row, s.height);
        let dir = wind_direction(lat);
        let idx = |col: i32| (row * s.width + col.rem_euclid(s.width)) as usize;
        // Two laps around the world so the air arriving at every tile has history.
        let mut air = 0.5f32;
        for lap in 0..2 {
            for i in 0..s.width {
                let id = idx(i * dir);
                let t = &tiles[id];
                if !is_land(&tiles, id) {
                    // Evaporation: warm seas give more moisture.
                    let warmth = smoothstep(-10.0, 28.0, t.temperature);
                    air += (1.0 - air) * (0.08 + 0.22 * warmth);
                } else {
                    // Rain falls steadily inland and heavily where air is forced up.
                    // Over warm land much of it evaporates again (forests recycle
                    // rain), which keeps tropical interiors like the Amazon wet.
                    let recycled = 0.75 * smoothstep(12.0, 26.0, t.temperature);
                    let lift = match t.elevation {
                        Elevation::Mountains => 0.55,
                        Elevation::Hills => 0.16 * (1.0 - recycled * 0.6),
                        Elevation::Flat => 0.07 * (1.0 - recycled),
                    };
                    air -= air * lift;
                }
                if lap == 1 {
                    humidity[id] = air;
                }
            }
        }
    }
    // Weather systems also mix air north-south; smooth the per-row result.
    for _ in 0..3 {
        let prev = humidity.clone();
        for id in 0..n {
            let (sum, count) = grid
                .neighbors(id as TileId)
                .fold((prev[id] * 2.0, 2.0), |(s, c), nb| (s + prev[nb as usize], c + 1.0));
            humidity[id] = sum / count;
        }
    }
    for id in 0..n {
        let (col, row) = grid.offset(id as TileId);
        let lat = latitude(row, s.height);
        let x = col as f64 + if row & 1 == 1 { 0.5 } else { 0.0 };
        let wobble = cyl(&climate_noise, x + 300.0, row as f64 * 0.866, w, 0.08) as f32 * 0.12;
        let local = earth.as_ref().map_or(0.0, |e| e.climate[id].1);
        let m = rainfall_belt(lat) * (0.15 + 1.1 * humidity[id]) * s.rainfall + wobble + local;
        tiles[id].moisture = climate::effective_moisture(m.clamp(0.0, 1.0), tiles[id].temperature);
    }

    // --- Biomes ----------------------------------------------------------------
    for id in 0..n {
        if !is_land(&tiles, id) {
            continue;
        }
        let (t, m) = (tiles[id].temperature, tiles[id].moisture);
        tiles[id].terrain = if t < climate::ICE_CAP {
            ids.snow // ice caps and polar desert
        } else if t < climate::TUNDRA {
            ids.tundra
        } else if t < climate::BOREAL {
            // Boreal belt: taiga on moist ground, cold steppe where dry.
            if m < 0.25 { ids.plains } else { ids.grassland }
        } else if t < climate::SUBTROPICAL {
            // Temperate: cold deserts (Gobi), steppe, meadow.
            if m < 0.16 {
                ids.desert
            } else if m < 0.38 {
                ids.plains
            } else {
                ids.grassland
            }
        } else {
            // Subtropical and tropical: hot desert, savanna, wet grassland.
            if m < 0.24 {
                ids.desert
            } else if m < 0.5 {
                ids.plains
            } else {
                ids.grassland
            }
        };
    }

    match (&earth, ids.lake) {
        (Some(e), Some(lake)) => {
            for (id, t) in tiles.iter_mut().enumerate() {
                if is_land_tile(t, &ids) && e.cover[id].lake >= 0.4 && e.cover[id].lake >= e.cover[id].land {
                    t.terrain = lake;
                    t.elevation = Elevation::Flat;
                }
            }
        }
        (Some(_), None) => {}
        (None, _) => place_lakes(&grid, &mut tiles, &ids, &mut rng),
    }

    // Shallow water next to land becomes coast (land only: coast made on
    // the way doesn't spread it further).
    for id in 0..n {
        if tiles[id].terrain == ids.ocean && grid.neighbors(id as TileId).any(|nb| is_land_tile(&tiles[nb as usize], &ids)) {
            tiles[id].terrain = ids.coast;
        }
    }

    if earth.is_some() {
        place_earth_rivers(rules, &grid, &mut tiles, &ids);
        // Smaller rivers between the great ones.
        let fewer = MapSettings { river_density: s.river_density * 0.45, ..s.clone() };
        place_rivers(&grid, &mut tiles, &ids, &fewer, &mut rng);
    } else {
        place_rivers(&grid, &mut tiles, &ids, s, &mut rng);
    }
    place_features(rules, &grid, &mut tiles, &ids, &mut rng);

    let start_land = shape.map(|s| s.start_land).unwrap_or_default();
    let mut map = Map { grid, tiles, seed: s.seed, start_land, kind: s.kind };
    crate::resource::place_resources(rules, &mut map);
    map
}

/// The Earth map type's data sampled on the map's tiles.
struct EarthLayers {
    cover: Vec<earth::Cover>,
    relief: Vec<earth::Relief>,
    /// (temperature, moisture) corrections.
    climate: Vec<(f32, f32)>,
    /// The share of land that makes a tile land: lower with a low sea level.
    threshold: f64,
}

fn earth_layers(rules: &Rules, grid: &MapGrid, s: &MapSettings) -> EarthLayers {
    let e = &rules.earth;
    let tiles = 0..grid.len() as TileId;
    let default = MapSettings::default().land_fraction;
    let threshold = (0.42 + (default - s.land_fraction) as f64 * 1.25).clamp(0.15, 0.75);
    let mut cover: Vec<earth::Cover> = tiles.clone().map(|t| e.cover(grid, t)).collect();
    // Land where most of the tile is, or most of its middle (thin peninsulas).
    for c in cover.iter_mut() {
        if c.land + c.lake < threshold && c.core >= 0.55 + (threshold - 0.42) && c.land >= 0.2 {
            c.land = threshold + 0.01;
        }
    }
    let land = |c: &earth::Cover| c.land + c.lake >= threshold;
    // Peninsulas keep their tips (the toe of Italy): a tile with a fifth
    // land or more, reaching out from a single land neighbour, joins.
    for _ in 0..3 {
        let tips: Vec<TileId> = tiles
            .clone()
            .filter(|&t| {
                let c = cover[t as usize];
                !land(&c)
                    && c.land >= 0.22
                    && c.mass.is_some()
                    && grid.neighbors(t).filter(|&n| land(&cover[n as usize]) && cover[n as usize].mass == c.mass).count() == 1
            })
            .collect();
        for t in tips {
            cover[t as usize].land = threshold + 0.01;
        }
    }
    // Thin land (Italy, Panama, Malaya) keeps its shape: a tile with some
    // land joins when it links two parts of its landmass around it.
    loop {
        let mut joins: Vec<(f64, TileId)> = tiles
            .clone()
            .filter(|&t| {
                let c = cover[t as usize];
                if land(&c) || c.land < 0.2 || c.mass.is_none() {
                    return false;
                }
                let ring: Vec<bool> = (0..6)
                    .map(|d| grid.tile(grid.hex(t).neighbor(d)).is_some_and(|n| land(&cover[n as usize]) && cover[n as usize].mass == c.mass))
                    .collect();
                (0..6).filter(|&d| ring[d] && !ring[(d + 5) % 6]).count() >= 2
            })
            .map(|t| (cover[t as usize].land, t))
            .collect();
        let Some(&(_, t)) = joins.iter().max_by(|a, b| a.0.total_cmp(&b.0)) else { break };
        joins.clear();
        cover[t as usize].land = threshold + 0.01;
    }
    // Islands too small for a tile of their own still get their best one,
    // unless a small one's would touch other land (the toe of Italy wins
    // over Sicily).
    let mut best: Vec<Option<(f64, TileId)>> = vec![None; 256];
    let mut area = [0.0f64; 256];
    let mut has = [false; 256];
    for t in tiles.clone() {
        let c = cover[t as usize];
        let Some(m) = c.mass else { continue };
        area[m as usize] += c.land;
        has[m as usize] |= land(&c);
        if best[m as usize].is_none_or(|(l, _)| c.land > l) {
            best[m as usize] = Some((c.land, t));
        }
    }
    for m in 0..256 {
        if let (Some((_, t)), false) = (best[m], has[m]) {
            let alone = grid.neighbors(t).all(|n| !land(&cover[n as usize]));
            if area[m] >= 0.6 || (area[m] >= 0.35 && alone) {
                cover[t as usize].land = threshold + 0.01;
            }
        }
    }
    // Straits stay open, at the cost of the land beside them.
    for strait in &e.straits {
        for t in earth::trace(grid, &strait.path) {
            cover[t as usize] = earth::Cover::default();
        }
    }
    // Landmasses stay apart (Britain from France, Malaya from Sumatra):
    // where two touch, the tile with less land goes, but never the last of
    // its landmass, so islands keep a tile.
    let mut size = [0usize; 256];
    for c in cover.iter().filter(|c| land(c)) {
        size[c.mass.unwrap_or(0) as usize] += 1;
    }
    for t in tiles.clone() {
        for nb in grid.neighbors(t) {
            let (c, o) = (cover[t as usize], cover[nb as usize]);
            let (Some(m), Some(n)) = (c.mass.filter(|_| land(&c)), o.mass.filter(|_| land(&o))) else { continue };
            if m == n {
                continue;
            }
            let last = |m: u8| size[m as usize] <= 1;
            let drop_this = if last(m) != last(n) { last(n) } else { (c.land, size[m as usize]) < (o.land, size[n as usize]) };
            let (gone, mass) = if drop_this { (t, m) } else { (nb, n) };
            cover[gone as usize] = earth::Cover::default();
            size[mass as usize] -= 1;
            if gone == t {
                break;
            }
        }
    }
    EarthLayers {
        cover,
        relief: tiles.clone().map(|t| e.relief(grid, t)).collect(),
        climate: tiles.map(|t| e.climate(grid, t)).collect(),
        threshold,
    }
}

fn is_land_tile(t: &Tile, ids: &Ids) -> bool {
    t.terrain != ids.ocean && t.terrain != ids.coast && Some(t.terrain) != ids.lake
}

/// Traces the great rivers of `earth.ron` onto the map. A river starts at
/// the first land below the mountains on its path and ends where it first meets water; if its
/// mouth falls inland at this size, it runs on to the nearest sea. Mountains
/// in its way become hills: valleys through the range.
fn place_earth_rivers(rules: &Rules, grid: &MapGrid, tiles: &mut [Tile], ids: &Ids) {
    let water = |t: &Tile| !is_land_tile(t, ids);
    let dir_to = |a: TileId, b: TileId| (0..6).find(|&d| grid.tile(grid.hex(a).neighbor(d)) == Some(b));
    for river in &rules.earth.rivers {
        let mut path: Vec<TileId> = earth::trace(grid, &river.path)
            .into_iter()
            .skip_while(|&t| water(&tiles[t as usize]) || tiles[t as usize].elevation == Elevation::Mountains)
            .collect();
        if let Some(end) = path.iter().position(|&t| water(&tiles[t as usize])) {
            path.truncate(end + 1);
        } else {
            // On to the sea, by the shortest way.
            while let Some(&last) = path.last() {
                if path.len() > 200 {
                    break;
                }
                let next = grid
                    .within(last, 6)
                    .into_iter()
                    .filter(|&o| water(&tiles[o as usize]) && tiles[o as usize].terrain != ids.ocean)
                    .min_by_key(|&o| grid.distance(last, o));
                let Some(sea) = next else { break };
                let step = grid.neighbors(last).filter(|n| !path.contains(n)).min_by_key(|&n| grid.distance(n, sea));
                let Some(step) = step else { break };
                path.push(step);
                if water(&tiles[step as usize]) {
                    break;
                }
            }
        }
        if path.len() < 3 || !path.last().is_some_and(|&t| water(&tiles[t as usize])) {
            continue;
        }
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if tiles[a as usize].elevation == Elevation::Mountains {
                tiles[a as usize].elevation = Elevation::Hills;
            }
            if let Some(d) = dir_to(a, b) {
                tiles[a as usize].rivers |= 1 << d;
                if !water(&tiles[b as usize]) {
                    tiles[b as usize].rivers |= 1 << ((d + 3) % 6);
                }
            }
        }
    }
}

fn place_features(rules: &Rules, grid: &MapGrid, tiles: &mut [Tile], ids: &Ids, rng: &mut ChaCha8Rng) {
    let allowed = |f: Option<FeatureId>, t: &Tile| -> Option<FeatureId> {
        let f = f?;
        let terrain = &rules.terrain(t.terrain).id;
        rules.feature(f).allowed_on.iter().any(|a| a == terrain).then_some(f)
    };
    for id in 0..tiles.len() {
        let t = tiles[id];
        let roll: f32 = rng.gen();
        let water = t.terrain == ids.ocean || t.terrain == ids.coast || Some(t.terrain) == ids.lake;
        let (temp, m) = (t.temperature, t.moisture);
        tiles[id].feature = if water {
            // Sea ice comes and goes with the seasons (see Map::sea_ice).
            None
        } else if t.elevation == Elevation::Mountains {
            None
        } else if temp >= climate::RAINFOREST && m > 0.6 && roll < smoothstep(0.6, 0.75, m) * 0.9 {
            allowed(ids.jungle, &t)
        } else if t.elevation == Elevation::Flat
            && m > 0.6
            && (t.rivers != 0 || grid.neighbors(id as TileId).any(|nb| tiles[nb as usize].terrain == ids.coast))
            && roll < 0.18
        {
            allowed(ids.marsh, &t)
        } else if (climate::TUNDRA..climate::BOREAL).contains(&temp) && roll < smoothstep(0.1, 0.4, m) * 0.93 {
            // Taiga: the boreal belt is mostly unbroken forest, as in Canada,
            // Scandinavia and Siberia.
            allowed(ids.forest, &t)
        } else if temp >= climate::BOREAL && roll < smoothstep(0.38, 0.65, m) * 0.88 {
            // Broadleaf and mixed forest further south.
            allowed(ids.forest, &t)
        } else if t.terrain == ids.desert && roll < 0.035 {
            allowed(ids.oasis, &t)
        } else {
            None
        };
    }
}

/// Connected land regions (continents and islands), as lists of tiles.
fn landmasses(grid: &MapGrid, tiles: &[Tile], ids: &Ids) -> Vec<Vec<TileId>> {
    let mut seen = vec![false; tiles.len()];
    let mut out = Vec::new();
    for start in 0..tiles.len() {
        if seen[start] || tiles[start].terrain == ids.ocean {
            continue;
        }
        let mut region = Vec::new();
        let mut stack = vec![start as TileId];
        seen[start] = true;
        while let Some(id) = stack.pop() {
            region.push(id);
            for nb in grid.neighbors(id) {
                if !seen[nb as usize] && tiles[nb as usize].terrain != ids.ocean {
                    seen[nb as usize] = true;
                    stack.push(nb);
                }
            }
        }
        out.push(region);
    }
    out
}

/// Lone peaks and two-tile fragments (left where a range breaks up) grow into
/// small ranges of 3-5 tiles along their strongest uplift.
fn grow_small_ranges(grid: &MapGrid, tiles: &mut [Tile], score: &[f64], ids: &Ids, rng: &mut ChaCha8Rng) {
    let mut seen = vec![false; tiles.len()];
    for start in 0..tiles.len() {
        if seen[start] || tiles[start].elevation != Elevation::Mountains {
            continue;
        }
        let mut group = vec![start as TileId];
        seen[start] = true;
        let mut i = 0;
        while i < group.len() {
            for nb in grid.neighbors(group[i]) {
                if !seen[nb as usize] && tiles[nb as usize].elevation == Elevation::Mountains {
                    seen[nb as usize] = true;
                    group.push(nb);
                }
            }
            i += 1;
        }
        if group.len() >= 3 {
            continue;
        }
        let target = rng.gen_range(3..=5);
        while group.len() < target {
            let next = group
                .iter()
                .flat_map(|&id| grid.neighbors(id))
                .filter(|&nb| {
                    let t = &tiles[nb as usize];
                    t.terrain != ids.ocean && t.elevation != Elevation::Mountains
                })
                .max_by(|&a, &b| score[a as usize].total_cmp(&score[b as usize]));
            let Some(next) = next else { break };
            tiles[next as usize].elevation = Elevation::Mountains;
            seen[next as usize] = true;
            group.push(next);
        }
    }
}

/// Thin ranges play better than wide massifs: they act as barriers with passes
/// rather than blocking whole regions. Mountains inside thick blobs (4+ mountain
/// neighbours) become hills, weakest uplift first, leaving ridge lines one or two
/// tiles wide flanked by foothills.
fn thin_ranges(grid: &MapGrid, tiles: &mut [Tile], score: &[f64]) {
    let mountain_neighbors = |tiles: &[Tile], id: TileId| {
        grid.neighbors(id).filter(|&nb| tiles[nb as usize].elevation == Elevation::Mountains).count()
    };
    loop {
        let mut thick: Vec<TileId> = (0..tiles.len() as TileId)
            .filter(|&id| tiles[id as usize].elevation == Elevation::Mountains && mountain_neighbors(tiles, id) >= 4)
            .collect();
        if thick.is_empty() {
            break;
        }
        thick.sort_by(|&a, &b| score[a as usize].total_cmp(&score[b as usize]));
        for id in thick {
            // Re-check: earlier demotions may already have thinned this spot.
            if mountain_neighbors(tiles, id) >= 4 {
                tiles[id as usize].elevation = Elevation::Hills;
            }
        }
    }
}

/// No large stretch of land goes without mountains: wherever land is more than
/// `MAX_GAP` tiles from the nearest mountain, a small range (4-7 tiles, with hill
/// flanks) rises on the strongest uplift in that stretch.
fn fill_mountain_gaps(grid: &MapGrid, tiles: &mut [Tile], score: &[f64], ids: &Ids, rng: &mut ChaCha8Rng) {
    const MAX_GAP: u32 = 8;
    let land = |t: &Tile| t.terrain != ids.ocean;
    for _ in 0..64 {
        // Distance over land from every tile to the nearest mountain.
        let mut dist = vec![u32::MAX; tiles.len()];
        let mut queue = VecDeque::new();
        for (id, t) in tiles.iter().enumerate() {
            if t.elevation == Elevation::Mountains {
                dist[id] = 0;
                queue.push_back(id as TileId);
            }
        }
        while let Some(id) = queue.pop_front() {
            for nb in grid.neighbors(id) {
                if dist[nb as usize] == u32::MAX && land(&tiles[nb as usize]) {
                    dist[nb as usize] = dist[id as usize] + 1;
                    queue.push_back(nb);
                }
            }
        }
        // The farthest-from-mountains land, ignoring small islands' coasts.
        let far: Vec<TileId> = (0..tiles.len() as TileId)
            .filter(|&id| {
                let t = &tiles[id as usize];
                land(t) && dist[id as usize] > MAX_GAP && grid.neighbors(id).all(|nb| land(&tiles[nb as usize]))
            })
            .collect();
        // Seed the new range where uplift is strongest, among tiles deep in the gap.
        let Some(&seed) = far
            .iter()
            .filter(|&&id| dist[id as usize] > MAX_GAP + 2 || far.len() < 30)
            .max_by(|&&a, &&b| score[a as usize].total_cmp(&score[b as usize]))
        else {
            break;
        };
        let mut range = vec![seed];
        tiles[seed as usize].elevation = Elevation::Mountains;
        let size = rng.gen_range(4..=7);
        while range.len() < size {
            let next = range
                .iter()
                .flat_map(|&id| grid.neighbors(id))
                .filter(|&nb| land(&tiles[nb as usize]) && tiles[nb as usize].elevation != Elevation::Mountains)
                .max_by(|&a, &b| score[a as usize].total_cmp(&score[b as usize]));
            let Some(next) = next else { break };
            tiles[next as usize].elevation = Elevation::Mountains;
            range.push(next);
        }
        for &id in &range {
            for nb in grid.neighbors(id) {
                let t = &mut tiles[nb as usize];
                if land(t) && t.elevation == Elevation::Flat && rng.gen_bool(0.6) {
                    t.elevation = Elevation::Hills;
                }
            }
        }
    }
}

/// Every sizeable landmass gets some relief, even far from active plate
/// boundaries (like the Urals or the Great Dividing Range). Missing mountains are
/// grown as a connected chain along the landmass's strongest uplift, then flanked
/// by hills.
fn ensure_relief(grid: &MapGrid, tiles: &mut [Tile], score: &[f64], ids: &Ids) {
    for region in landmasses(grid, tiles, ids) {
        if region.len() < 25 {
            continue;
        }
        let count = |tiles: &[Tile], e: Elevation| region.iter().filter(|&&id| tiles[id as usize].elevation == e).count();
        let want_mountains = (region.len() as f32 * 0.045).ceil() as usize;
        let want_hills = (region.len() as f32 * 0.14).ceil() as usize;

        let mut have = count(tiles, Elevation::Mountains);
        if have < want_mountains {
            // Seed at the strongest uplift, then repeatedly add the best neighbour.
            let mut chain: Vec<TileId> = region
                .iter()
                .copied()
                .filter(|&id| tiles[id as usize].elevation == Elevation::Mountains)
                .collect();
            if chain.is_empty() {
                let seed = *region.iter().max_by(|&&a, &&b| score[a as usize].total_cmp(&score[b as usize])).unwrap();
                tiles[seed as usize].elevation = Elevation::Mountains;
                chain.push(seed);
                have += 1;
            }
            while have < want_mountains {
                let next = chain
                    .iter()
                    .flat_map(|&id| grid.neighbors(id))
                    .filter(|&nb| tiles[nb as usize].terrain != ids.ocean && tiles[nb as usize].elevation != Elevation::Mountains)
                    .max_by(|&a, &b| score[a as usize].total_cmp(&score[b as usize]));
                let Some(next) = next else { break };
                tiles[next as usize].elevation = Elevation::Mountains;
                chain.push(next);
                have += 1;
            }
        }

        let mut hills = count(tiles, Elevation::Hills);
        if hills < want_hills {
            // Foothills first (next to mountains), then the strongest uplift.
            let mut candidates: Vec<(f64, TileId)> = region
                .iter()
                .filter(|&&id| tiles[id as usize].elevation == Elevation::Flat)
                .map(|&id| {
                    let flank = grid.neighbors(id).any(|nb| tiles[nb as usize].elevation == Elevation::Mountains);
                    (score[id as usize] + if flank { 10.0 } else { 0.0 }, id)
                })
                .collect();
            candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
            for &(_, id) in candidates.iter().take(want_hills - hills) {
                tiles[id as usize].elevation = Elevation::Hills;
                hills += 1;
            }
        }
    }
}

/// Lakes in wet, flat lowlands well away from the sea: fresh water and natural
/// end points for rivers.
fn place_lakes(grid: &MapGrid, tiles: &mut [Tile], ids: &Ids, rng: &mut ChaCha8Rng) {
    let Some(lake) = ids.lake else { return };
    let land = tiles.iter().filter(|t| t.terrain != ids.ocean).count();
    let target = land / 350;
    let mut candidates: Vec<(f32, TileId)> = (0..tiles.len() as TileId)
        .filter(|&id| {
            let t = &tiles[id as usize];
            t.terrain != ids.ocean
                && t.terrain != ids.snow
                && t.elevation == Elevation::Flat
                && t.moisture > 0.4
                && grid.within(id, 2).iter().all(|&o| tiles[o as usize].terrain != ids.ocean)
        })
        .map(|id| (tiles[id as usize].moisture + rng.gen::<f32>(), id))
        .collect();
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut placed: Vec<TileId> = Vec::new();
    for &(_, id) in &candidates {
        if placed.len() >= target {
            break;
        }
        if placed.iter().any(|&o| grid.distance(o, id) < 7) {
            continue;
        }
        tiles[id as usize].terrain = lake;
        // Some lakes spread over a neighbouring tile or two.
        let extra = rng.gen_range(0..=2);
        let nbs: Vec<TileId> = grid
            .neighbors(id)
            .filter(|&nb| tiles[nb as usize].elevation == Elevation::Flat && tiles[nb as usize].terrain != ids.ocean)
            .collect();
        for &nb in nbs.iter().take(extra) {
            tiles[nb as usize].terrain = lake;
        }
        placed.push(id);
    }
}

/// Rivers rise next to mountains (preferring wet ground) and follow the drainage
/// gradient, always getting closer to the sea, until they reach it or join
/// another river.
fn place_rivers(grid: &MapGrid, tiles: &mut [Tile], ids: &Ids, s: &MapSettings, rng: &mut ChaCha8Rng) {
    let n = tiles.len();
    // Lakes count as water: rivers may end in them.
    let water = |t: &Tile| t.terrain == ids.ocean || t.terrain == ids.coast || Some(t.terrain) == ids.lake;

    // Distance (in tiles) from every land tile to the sea.
    let mut to_sea = vec![u32::MAX; n];
    let mut queue = VecDeque::new();
    for id in 0..n {
        if water(&tiles[id]) {
            to_sea[id] = 0;
            queue.push_back(id as TileId);
        }
    }
    while let Some(id) = queue.pop_front() {
        for nb in grid.neighbors(id) {
            let nbu = nb as usize;
            if to_sea[nbu] == u32::MAX && tiles[nbu].elevation != Elevation::Mountains {
                to_sea[nbu] = to_sea[id as usize] + 1;
                queue.push_back(nb);
            }
        }
    }

    // Candidate sources: land at the foot of a mountain, a few tiles inland,
    // scored by wetness. Rivers always rise in the mountains.
    let mut sources: Vec<(f32, TileId, TileId)> = (0..n as TileId)
        .filter_map(|id| {
            let t = &tiles[id as usize];
            if water(t) || t.elevation == Elevation::Mountains || to_sea[id as usize] < 3 || to_sea[id as usize] == u32::MAX
            {
                return None;
            }
            let mountain = grid.neighbors(id).find(|&nb| tiles[nb as usize].elevation == Elevation::Mountains)?;
            let score = t.moisture + rng.gen::<f32>() * 0.4 - if t.temperature < climate::ICE_CAP { 1.0 } else { 0.0 };
            Some((score, id, mountain))
        })
        .collect();
    sources.sort_by(|a, b| b.0.total_cmp(&a.0));

    let land = tiles.iter().filter(|t| !water(t)).count();
    let target = (land as f32 / 1000.0 * s.river_density).round() as usize;
    let mut made = 0;
    let mut started: Vec<TileId> = Vec::new();
    for &(score, start, mountain) in &sources {
        if made >= target || score < 0.15 {
            break;
        }
        if tiles[start as usize].rivers != 0 || started.iter().any(|&o| grid.distance(o, start) < 4) {
            continue;
        }
        let mut path = vec![start];
        let mut cur = start;
        let mut ok = false;
        while to_sea[cur as usize] > 0 {
            // Step to a neighbour closer to the sea, preferring lower ground,
            // with a little randomness so rivers meander between equal choices.
            let d = to_sea[cur as usize];
            // Prefer lower ground, with enough randomness that rivers wander, and
            // allow sideways steps (same distance to the sea) now and then.
            let candidates: Vec<(f32, TileId)> = grid
                .neighbors(cur)
                .filter(|&nb| {
                    let nd = to_sea[nb as usize];
                    nd < d || (nd == d && !path.contains(&nb) && path.len() % 3 == 1)
                })
                .map(|nb| {
                    let sideways = if to_sea[nb as usize] == d { 0.06 } else { 0.0 };
                    (tiles[nb as usize].height + rng.gen::<f32>() * 0.12 + sideways, nb)
                })
                .collect();
            let Some(&(_, next)) = candidates.iter().min_by(|a, b| a.0.total_cmp(&b.0)) else { break };
            path.push(next);
            if water(&tiles[next as usize]) || tiles[next as usize].rivers != 0 {
                ok = true;
                break;
            }
            cur = next;
        }
        if !ok || path.len() < 4 {
            continue;
        }
        let dir_to = |a: TileId, b: TileId| (0..6).find(|&d| grid.tile(grid.hex(a).neighbor(d)) == Some(b));
        // Headwater: the source tile's channel starts at its mountain edge.
        if let Some(d) = dir_to(start, mountain) {
            tiles[start as usize].rivers |= 1 << d;
        }
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if let Some(d) = dir_to(a, b) {
                tiles[a as usize].rivers |= 1 << d;
                if !water(&tiles[b as usize]) {
                    tiles[b as usize].rivers |= 1 << ((d + 3) % 6);
                }
            }
        }
        started.push(start);
        made += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> (Rules, Map) {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        (rules, map)
    }

    #[test]
    fn generation_is_deterministic() {
        let rules = Rules::load_default().unwrap();
        let s = MapSettings::default();
        let a = generate(&rules, &s);
        let b = generate(&rules, &s);
        for (x, y) in a.tiles.iter().zip(&b.tiles) {
            assert_eq!(x.terrain, y.terrain);
            assert_eq!(x.rivers, y.rivers);
            assert_eq!(x.feature, y.feature);
        }
    }

    #[test]
    fn land_fraction_roughly_matches() {
        let (rules, map) = map();
        let land = (0..map.tiles.len() as TileId).filter(|&id| !map.is_water(&rules, id)).count();
        let frac = land as f32 / map.tiles.len() as f32;
        assert!((frac - 0.38).abs() < 0.03, "land fraction {frac}");
    }

    #[test]
    fn rivers_are_connected_from_source_to_water() {
        let (rules, map) = map();
        let river_tiles = map.tiles.iter().filter(|t| t.rivers != 0).count();
        assert!(river_tiles > 150, "only {river_tiles} river tiles");
        // Every river end is a headwater at a mountain or a mouth into water (sea
        // or lake): rivers rise only in mountains and never dead-end.
        for id in 0..map.tiles.len() as TileId {
            let t = map.tile(id);
            if t.rivers.count_ones() != 1 {
                continue;
            }
            let d = t.rivers.trailing_zeros() as usize;
            let nb = map.grid.tile(map.grid.hex(id).neighbor(d)).unwrap();
            let other = map.tile(nb);
            assert!(
                other.elevation == Elevation::Mountains || map.is_water(&rules, nb),
                "river end at tile {id} touches neither a mountain nor water"
            );
        }
    }

    #[test]
    fn climate_follows_latitude() {
        let (rules, map) = map();
        let snow = rules.terrain_id("snow").unwrap();
        let desert = rules.terrain_id("desert").unwrap();
        let mut polar_snow = 0;
        let mut tropical_snow = 0;
        let mut desert_lat = Vec::new();
        for id in 0..map.tiles.len() as TileId {
            let t = map.tile(id);
            let lat = latitude(map.grid.offset(id).1, map.grid.height).abs();
            if t.terrain == snow {
                if lat > 60.0 { polar_snow += 1 } else if lat < 35.0 { tropical_snow += 1 }
            }
            if t.terrain == desert {
                desert_lat.push(lat);
            }
        }
        assert!(polar_snow > 0, "no polar snow");
        assert_eq!(tropical_snow, 0, "snow terrain in the tropics");
        // Deserts concentrate in the subtropics and mid-latitude interiors, not at the equator.
        let near_equator = desert_lat.iter().filter(|&&l| l < 8.0).count();
        assert!(near_equator * 10 < desert_lat.len().max(1), "deserts on the equator");
    }

    #[test]
    fn mountains_form_ranges_with_foothills() {
        let (_, map) = map();
        let mountains: Vec<TileId> =
            (0..map.tiles.len() as TileId).filter(|&id| map.tile(id).elevation == Elevation::Mountains).collect();
        assert!(mountains.len() > 50, "only {} mountains", mountains.len());
        // Ranges, not isolated peaks: nearly every mountain touches another.
        let connected = mountains
            .iter()
            .filter(|&&id| map.grid.neighbors(id).any(|nb| map.tile(nb).elevation == Elevation::Mountains))
            .count();
        assert!(connected * 100 >= mountains.len() * 85, "{connected}/{} mountains in ranges", mountains.len());
        // Mountains step down through hills: most land next to a range is hills.
        let (mut flank, mut hilly) = (0, 0);
        for &id in &mountains {
            for nb in map.grid.neighbors(id) {
                let t = map.tile(nb);
                if t.elevation != Elevation::Mountains && t.height > 0.0 && t.terrain == map.tile(id).terrain {
                    flank += 1;
                    hilly += (t.elevation == Elevation::Hills) as usize;
                }
            }
        }
        assert!(hilly * 2 >= flank, "only {hilly}/{flank} range flanks are hills");
    }

    #[test]
    fn mountains_form_many_separate_ranges() {
        let (_, map) = map();
        // Connected groups of mountain tiles.
        let mut seen = vec![false; map.tiles.len()];
        let mut sizes = Vec::new();
        for start in 0..map.tiles.len() {
            if seen[start] || map.tiles[start].elevation != Elevation::Mountains {
                continue;
            }
            let (mut stack, mut size) = (vec![start as TileId], 0);
            seen[start] = true;
            while let Some(id) = stack.pop() {
                size += 1;
                for nb in map.grid.neighbors(id) {
                    if !seen[nb as usize] && map.tile(nb).elevation == Elevation::Mountains {
                        seen[nb as usize] = true;
                        stack.push(nb);
                    }
                }
            }
            sizes.push(size);
        }
        let total: usize = sizes.iter().sum();
        let ranges = sizes.iter().filter(|&&s| s >= 3).count();
        let largest = *sizes.iter().max().unwrap();
        assert!(ranges >= 20, "only {ranges} ranges of 3+ tiles");
        assert!(largest * 4 <= total, "largest range has {largest} of {total} mountain tiles");
    }

    #[test]
    fn mountain_ranges_are_thin() {
        let (_, map) = map();
        for id in 0..map.tiles.len() as TileId {
            if map.tile(id).elevation == Elevation::Mountains {
                let n = map.grid.neighbors(id).filter(|&nb| map.tile(nb).elevation == Elevation::Mountains).count();
                assert!(n <= 3, "mountain {id} has {n} mountain neighbours (range too thick)");
            }
        }
    }

    #[test]
    fn no_land_is_far_from_mountains() {
        let (rules, map) = map();
        let mut dist = vec![u32::MAX; map.tiles.len()];
        let mut queue = VecDeque::new();
        for (id, tile) in map.tiles.iter().enumerate() {
            if tile.elevation == Elevation::Mountains {
                dist[id] = 0;
                queue.push_back(id as TileId);
            }
        }
        while let Some(id) = queue.pop_front() {
            for nb in map.grid.neighbors(id) {
                if dist[nb as usize] == u32::MAX && !map.is_water(&rules, nb) {
                    dist[nb as usize] = dist[id as usize] + 1;
                    queue.push_back(nb);
                }
            }
        }
        // Interior land (not counting coasts of tiny islands) stays within reach.
        let worst = (0..map.tiles.len())
            .filter(|&id| {
                !map.is_water(&rules, id as TileId)
                    && map.grid.neighbors(id as TileId).all(|nb| !map.is_water(&rules, nb))
                    && dist[id] != u32::MAX
            })
            .map(|id| dist[id])
            .max()
            .unwrap();
        assert!(worst <= 11, "land {worst} tiles from the nearest mountain");
    }

    #[test]
    fn every_large_landmass_has_relief_and_rivers() {
        let rules = Rules::load_default().unwrap();
        let ids = Ids::new(&rules);
        let map = generate(&rules, &MapSettings::default());
        for region in landmasses(&map.grid, &map.tiles, &ids) {
            if region.len() < 60 {
                continue;
            }
            let has = |e: Elevation| region.iter().any(|&id| map.tile(id).elevation == e);
            assert!(has(Elevation::Mountains), "landmass of {} tiles without mountains", region.len());
            assert!(has(Elevation::Hills), "landmass of {} tiles without hills", region.len());
            assert!(region.iter().any(|&id| map.tile(id).rivers != 0), "landmass of {} tiles without rivers", region.len());
        }
    }

    #[test]
    fn boreal_belt_is_mostly_forest() {
        let (rules, map) = map();
        let forest = rules.feature_id("forest");
        let boreal: Vec<_> = map
            .tiles
            .iter()
            .filter(|t| {
                !rules.terrain(t.terrain).water
                    && t.elevation != Elevation::Mountains
                    && (crate::climate::TUNDRA..crate::climate::BOREAL).contains(&t.temperature)
            })
            .collect();
        let forested = boreal.iter().filter(|t| t.feature == forest).count();
        assert!(forested * 10 >= boreal.len() * 6, "only {forested}/{} boreal tiles forested", boreal.len());
    }

    #[test]
    fn cold_biomes_cover_a_real_share_of_land() {
        let (rules, map) = map();
        let land: Vec<_> = map.tiles.iter().filter(|t| !rules.terrain(t.terrain).water).collect();
        let cold = land.iter().filter(|t| t.temperature < crate::climate::BOREAL).count();
        let share = cold as f32 / land.len() as f32;
        assert!(share > 0.2, "cold land share {share}");
    }

    #[test]
    fn sea_ice_follows_the_seasons() {
        use crate::climate::SeaIce;
        let (rules, map) = map();
        let count = |season: f32, ice: SeaIce| (0..map.tiles.len()).filter(|&t| map.sea_ice(&rules, t as TileId, season) == ice).count();
        // Northern winter freezes more of the north than its summer, and the
        // polar seas stay frozen whatever the season.
        let north = |season: f32| (0..map.grid.width * 6).filter(|&t| map.sea_ice(&rules, t as TileId, season) != SeaIce::Open).count();
        assert!(north(-1.0) > north(1.0));
        let edge = |season: f32| (0..map.grid.width).filter(|&t| map.sea_ice(&rules, t as TileId, season) == SeaIce::Pack).count();
        assert!(edge(1.0) * 4 >= map.grid.width as usize * 3, "{} of {}", edge(1.0), map.grid.width);
        assert!(count(1.0, SeaIce::Pack) > 0);
        assert!(count(-1.0, SeaIce::Thin) > 0);
        for t in 0..map.tiles.len() {
            if !rules.terrain(map.tiles[t].terrain).water {
                assert_eq!(map.sea_ice(&rules, t as TileId, -1.0), SeaIce::Open);
            }
        }
    }
}
