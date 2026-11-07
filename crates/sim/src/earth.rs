//! The Earth map type: real land, relief, great rivers and regional climate
//! from `data/earth.ron`, sampled at any map size. The map's columns run
//! east from 180° W and its rows from 90° N to 90° S (see
//! [`crate::climate::latitude`]), so every tile has a real longitude and
//! latitude and the generator's climate model works on true latitudes.

use crate::hex::{MapGrid, TileId};
use serde::Deserialize;

pub const COLS: usize = 720;
pub const ROWS: usize = 180 * 2;
/// Degrees per cell.
const CELL: f64 = 360.0 / COLS as f64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum RangeKind {
    /// Peaks along the path, foothills beside it.
    Mountains,
    /// Hills only.
    Hills,
}

/// A mountain range or upland.
#[derive(Clone, Debug, Deserialize)]
pub struct Range {
    pub name: String,
    pub kind: RangeKind,
    /// Half-width in degrees.
    pub width: f64,
    /// (longitude, latitude) points along its crest.
    pub path: Vec<(f64, f64)>,
}

/// A great river from source to mouth, or a strait to keep open.
#[derive(Clone, Debug, Deserialize)]
pub struct River {
    pub name: String,
    pub path: Vec<(f64, f64)>,
}

/// A regional climate correction: adds `temperature` (°C) and `moisture` at
/// `centre`, fading to nothing at the rim of an ellipse of `size` half-axes.
#[derive(Clone, Debug, Deserialize)]
pub struct ClimatePatch {
    pub name: String,
    pub centre: (f64, f64),
    pub size: (f64, f64),
    #[serde(default)]
    pub temperature: f32,
    #[serde(default)]
    pub moisture: f32,
}

/// `earth.ron` as written (see its header for the format).
#[derive(Deserialize)]
struct EarthFile {
    land: Vec<String>,
    masses: Vec<String>,
    #[serde(default)]
    straits: Vec<River>,
    ranges: Vec<Range>,
    rivers: Vec<River>,
    #[serde(default)]
    climate: Vec<ClimatePatch>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "EarthFile")]
pub struct Earth {
    /// `ROWS` × `COLS` cells: land share in ninths (0..=9), or `LAKE`.
    land: Vec<u8>,
    /// The landmass covering most of each cell (0: none).
    masses: Vec<u8>,
    /// Water passages within one landmass that stay open at every size.
    pub straits: Vec<River>,
    pub ranges: Vec<Range>,
    pub rivers: Vec<River>,
    pub climate: Vec<ClimatePatch>,
}

const LAKE: u8 = 10;

/// What covers a tile: shares of land and of lake (0..1), and the landmass
/// with most of the land.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cover {
    pub land: f64,
    pub lake: f64,
    /// The share of land around the tile's centre (the inner half of its
    /// footprint), which keeps thin peninsulas.
    pub core: f64,
    pub mass: Option<u8>,
}

/// How high a tile's relief rises: above 0 inside a mountain range's crest,
/// or among hills.
#[derive(Clone, Copy, Debug, Default)]
pub struct Relief {
    pub mountain: f64,
    pub hill: f64,
}

/// Decodes run-length encoded rows: a symbol, then its count when above 1.
fn decode(rows: &[String], what: &str, symbol: impl Fn(char) -> Option<u8>) -> Result<Vec<u8>, String> {
    if rows.len() != ROWS {
        return Err(format!("earth.ron: {what} has {} rows, expected {ROWS}", rows.len()));
    }
    let mut out = Vec::with_capacity(ROWS * COLS);
    for (i, row) in rows.iter().enumerate() {
        let start = out.len();
        let mut chars = row.chars().peekable();
        while let Some(c) = chars.next() {
            let v = symbol(c).ok_or_else(|| format!("earth.ron: {what} row {i} has an unknown cell '{c}'"))?;
            let mut n = 0usize;
            while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                n = n * 10 + d as usize;
                chars.next();
            }
            out.extend(std::iter::repeat_n(v, n.max(1)));
        }
        if out.len() - start != COLS {
            return Err(format!("earth.ron: {what} row {i} has {} cells, expected {COLS}", out.len() - start));
        }
    }
    Ok(out)
}

impl TryFrom<EarthFile> for Earth {
    type Error = String;

    fn try_from(f: EarthFile) -> Result<Earth, String> {
        let land = decode(&f.land, "land", |c| match c {
            '.' => Some(0),
            'a'..='i' => Some(c as u8 - b'a' + 1),
            'L' => Some(LAKE),
            _ => None,
        })?;
        let masses = decode(&f.masses, "masses", |c| match c {
            '.' => Some(0),
            c if c.is_ascii_graphic() && !c.is_ascii_digit() && c != '"' && c != '\\' => Some(c as u8),
            _ => None,
        })?;
        for r in &f.ranges {
            if r.path.is_empty() || r.width <= 0.0 {
                return Err(format!("earth.ron: range '{}' needs a path and a width above 0", r.name));
            }
        }
        for r in f.rivers.iter().chain(&f.straits) {
            if r.path.len() < 2 {
                return Err(format!("earth.ron: '{}' needs at least two points", r.name));
            }
        }
        Ok(Earth { land, masses, straits: f.straits, ranges: f.ranges, rivers: f.rivers, climate: f.climate })
    }
}

impl Earth {
    /// The cell at a longitude and latitude.
    fn cell(&self, lon: f64, lat: f64) -> usize {
        let c = (((lon + 180.0).rem_euclid(360.0) / CELL) as usize).min(COLS - 1);
        let r = (((90.0 - lat) / CELL).max(0.0) as usize).min(ROWS - 1);
        r * COLS + c
    }

    /// Land and lake shares over a tile's footprint, and its main landmass.
    pub fn cover(&self, grid: &MapGrid, tile: TileId) -> Cover {
        const N: i32 = 8;
        let (lon, lat) = lon_lat(grid, tile);
        let (hx, hy) = half_size(grid);
        let (mut land, mut lake, mut core, mut cores) = (0.0, 0.0, 0.0, 0.0f64);
        let mut masses: Vec<(u8, f64)> = Vec::new();
        for i in 0..N {
            for j in 0..N {
                let fx = (i as f64 + 0.5) / N as f64 * 2.0 - 1.0;
                let fy = (j as f64 + 0.5) / N as f64 * 2.0 - 1.0;
                let c = self.cell(lon + fx * hx, (lat + fy * hy).clamp(-89.99, 89.99));
                let share = match self.land[c] {
                    LAKE => {
                        lake += 1.0;
                        continue;
                    }
                    v => v as f64 / 9.0,
                };
                land += share;
                if fx.abs() < 0.5 && fy.abs() < 0.5 {
                    core += share;
                    cores += 1.0;
                }
                if self.masses[c] != 0 && share > 0.0 {
                    match masses.iter_mut().find(|(m, _)| *m == self.masses[c]) {
                        Some((_, w)) => *w += share,
                        None => masses.push((self.masses[c], share)),
                    }
                }
            }
        }
        let n = (N * N) as f64;
        let mass = masses.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|&(m, _)| m);
        Cover { land: land / n, lake: lake / n, core: core / cores.max(1.0), mass }
    }

    /// The relief at a tile: each range reaches at least the tiles its crest
    /// passes through, however narrow it is.
    pub fn relief(&self, grid: &MapGrid, tile: TileId) -> Relief {
        let (lon, lat) = lon_lat(grid, tile);
        let (hx, hy) = half_size(grid);
        // A tile's radius in the same (cosine-scaled) degrees as distances.
        let tile_r = hy.max(hx * lat.to_radians().cos()) * 0.85;
        let mut out = Relief { mountain: -1.0, hill: -1.0 };
        for r in &self.ranges {
            let d = path_distance(&r.path, lon, lat);
            let crest = r.width.max(tile_r);
            match r.kind {
                RangeKind::Mountains => {
                    out.mountain = out.mountain.max(1.0 - d / crest);
                    out.hill = out.hill.max(1.0 - d / (r.width * 2.2).max(tile_r * 1.6));
                }
                RangeKind::Hills => out.hill = out.hill.max(1.0 - d / crest),
            }
        }
        out
    }

    /// The climate corrections at a tile: (temperature, moisture).
    pub fn climate(&self, grid: &MapGrid, tile: TileId) -> (f32, f32) {
        let (lon, lat) = lon_lat(grid, tile);
        self.climate.iter().fold((0.0, 0.0), |(t, m), p| {
            let dx = wrap_lon(lon - p.centre.0) / p.size.0;
            let dy = (lat - p.centre.1) / p.size.1;
            let e = (dx * dx + dy * dy).sqrt();
            if e >= 1.0 {
                return (t, m);
            }
            let k = 1.0 - e * e * (3.0 - 2.0 * e);
            (t + p.temperature * k as f32, m + p.moisture * k as f32)
        })
    }
}

/// Longitude difference folded into -180..180.
fn wrap_lon(d: f64) -> f64 {
    let d = (d + 180.0).rem_euclid(360.0) - 180.0;
    if d == -180.0 { 180.0 } else { d }
}

/// Half a tile's footprint in degrees: (longitude, latitude).
fn half_size(grid: &MapGrid) -> (f64, f64) {
    (180.0 / grid.width as f64, 90.0 / grid.height as f64)
}

/// The longitude and latitude of a tile's centre.
pub fn lon_lat(grid: &MapGrid, tile: TileId) -> (f64, f64) {
    let (col, row) = grid.offset(tile);
    let x = col as f64 + if row & 1 == 1 { 1.0 } else { 0.5 };
    let lon = x / grid.width as f64 * 360.0 - 180.0;
    (wrap_lon(lon), crate::climate::latitude(row, grid.height) as f64)
}

/// The tile at a longitude and latitude.
pub fn tile_at(grid: &MapGrid, lon: f64, lat: f64) -> TileId {
    let row = (((90.0 - lat) / 180.0 * grid.height as f64).floor() as i32).clamp(0, grid.height - 1);
    let x = (lon + 180.0) / 360.0 * grid.width as f64 - if row & 1 == 1 { 0.5 } else { 0.0 };
    let col = (x.floor() as i32).rem_euclid(grid.width);
    (row * grid.width + col) as TileId
}

/// Distance in degrees (longitude scaled by the cosine of latitude) from a
/// point to a path.
fn path_distance(path: &[(f64, f64)], lon: f64, lat: f64) -> f64 {
    let k = lat.to_radians().cos().max(0.05);
    // Points relative to the query, unwrapped across the date line.
    let local = |(x, y): (f64, f64)| (wrap_lon(x - lon) * k, y - lat);
    if path.len() == 1 {
        let (x, y) = local(path[0]);
        return (x * x + y * y).sqrt();
    }
    path.windows(2)
        .map(|w| {
            let (ax, ay) = local(w[0]);
            let (mut bx, by) = local(w[1]);
            // Keep the segment short if it crosses the date line.
            let span = wrap_lon(w[1].0 - w[0].0) * k;
            if (bx - ax - span).abs() > 1e-6 {
                bx = ax + span;
            }
            let (dx, dy) = (bx - ax, by - ay);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let (px, py) = (ax + dx * t, ay + dy * t);
            (px * px + py * py).sqrt()
        })
        .fold(f64::MAX, f64::min)
}

/// The tiles a path crosses, in order, each a neighbour of the last.
pub fn trace(grid: &MapGrid, path: &[(f64, f64)]) -> Vec<TileId> {
    let mut out = vec![tile_at(grid, path[0].0, path[0].1)];
    for &(lon, lat) in &path[1..] {
        let target = tile_at(grid, lon, lat);
        let mut cur = *out.last().unwrap();
        while cur != target {
            // Step to the neighbour nearest the target point.
            let next = grid
                .neighbors(cur)
                .min_by(|&a, &b| {
                    let da = grid.distance(a, target) as f64 * 10.0 + point_distance(grid, a, lon, lat);
                    let db = grid.distance(b, target) as f64 * 10.0 + point_distance(grid, b, lon, lat);
                    da.total_cmp(&db)
                })
                .expect("tiles have neighbours");
            if out.contains(&next) {
                break;
            }
            out.push(next);
            cur = next;
        }
    }
    out
}

fn point_distance(grid: &MapGrid, tile: TileId, lon: f64, lat: f64) -> f64 {
    let (x, y) = lon_lat(grid, tile);
    path_distance(&[(x, y)], lon, lat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_and_coordinates_round_trip() {
        for (w, h) in [(96, 60), (128, 80), (160, 100)] {
            let grid = MapGrid::new(w, h);
            for t in (0..grid.len() as TileId).step_by(37) {
                let (lon, lat) = lon_lat(&grid, t);
                assert_eq!(tile_at(&grid, lon, lat), t, "{w}x{h} tile {t} at {lon}, {lat}");
            }
        }
    }

    fn earth(rules: &crate::rules::Rules, width: i32, height: i32) -> crate::map::Map {
        use crate::mapgen::{generate, MapSettings, MapType};
        generate(rules, &MapSettings { kind: MapType::Earth, width, height, seed: 3, ..Default::default() })
    }

    #[test]
    fn earth_has_its_landmarks_at_every_size() {
        use crate::map::Elevation;
        let rules = crate::rules::Rules::load_default().unwrap();
        for (w, h) in [(96, 60), (128, 80), (160, 100)] {
            let map = earth(&rules, w, h);
            let at = |lon: f64, lat: f64| tile_at(&map.grid, lon, lat);
            let terrain = |lon, lat| rules.terrain(map.tile(at(lon, lat)).terrain).id.as_str();
            let land = [
                ("Sahara", 10.0, 23.0), ("Moscow", 37.6, 55.8), ("Amazon", -60.0, -5.0), ("Outback", 134.0, -25.0),
                ("Great Plains", -100.0, 40.0), ("Siberia", 100.0, 62.0), ("Deccan", 78.0, 18.0), ("Congo", 23.0, -2.0),
                ("Iberia", -4.0, 40.0), ("Anatolia", 33.0, 39.0), ("Great Britain", -1.5, 53.0), ("Japan", 139.5, 36.5),
            ];
            for (name, lon, lat) in land {
                assert!(!map.is_water(&rules, at(lon, lat)), "{w}x{h}: {name} is under water");
            }
            let sea = [
                ("Atlantic", -35.0, 30.0), ("Pacific", -140.0, 0.0), ("Indian Ocean", 75.0, -20.0), ("Mediterranean", 18.0, 35.0),
                ("Caspian", 51.0, 42.0), ("Gulf of Mexico", -90.0, 25.0), ("Bay of Bengal", 88.0, 15.0), ("Hudson Bay", -86.0, 59.0),
            ];
            for (name, lon, lat) in sea {
                assert!(map.is_water(&rules, at(lon, lat)), "{w}x{h}: the {name} is land");
            }
            assert_eq!(terrain(5.0, 25.0), "desert", "{w}x{h}: the Sahara");
            assert_eq!(terrain(-42.0, 75.0), "snow", "{w}x{h}: Greenland");
            assert_eq!(terrain(0.0, -85.0), "snow", "{w}x{h}: Antarctica");
            let near_mountains = |lon, lat| {
                map.grid.within(at(lon, lat), 1).iter().any(|&t| map.tile(t).elevation == Elevation::Mountains)
            };
            for (name, lon, lat) in [("Himalayas", 84.0, 28.5), ("Andes", -69.0, -20.0), ("Rockies", -110.0, 44.0), ("Alps", 9.0, 46.3)] {
                assert!(near_mountains(lon, lat), "{w}x{h}: no {name}");
            }
            let river = |lon, lat| map.grid.within(at(lon, lat), 1).iter().any(|&t| map.has_river(t));
            for (name, lon, lat) in [("Nile", 32.7, 26.0), ("Amazon", -62.0, -3.3), ("Mississippi", -91.0, 33.0), ("Yangtze", 112.0, 30.3)] {
                assert!(river(lon, lat), "{w}x{h}: no {name}");
            }
            let jungle = rules.feature_id("jungle");
            let amazon = map.grid.within(at(-62.0, -5.0), 3);
            let wooded = amazon.iter().filter(|&&t| map.tile(t).feature == jungle).count();
            assert!(wooded * 3 >= amazon.len(), "{w}x{h}: the Amazon has {wooded} of {} tiles of jungle", amazon.len());
        }
    }

    /// Connected regions: land and water separately, by tile.
    fn regions(rules: &crate::rules::Rules, map: &crate::map::Map) -> Vec<usize> {
        let mut comp = vec![usize::MAX; map.tiles.len()];
        let mut next = 0;
        for start in 0..map.tiles.len() as TileId {
            if comp[start as usize] != usize::MAX {
                continue;
            }
            let water = map.is_water(rules, start);
            let mut stack = vec![start];
            comp[start as usize] = next;
            while let Some(t) = stack.pop() {
                for nb in map.grid.neighbors(t) {
                    if comp[nb as usize] == usize::MAX && map.is_water(rules, nb) == water {
                        comp[nb as usize] = next;
                        stack.push(nb);
                    }
                }
            }
            next += 1;
        }
        comp
    }

    #[test]
    fn earth_keeps_islands_apart_and_seas_open() {
        let rules = crate::rules::Rules::load_default().unwrap();
        for (w, h) in [(96, 60), (128, 80), (160, 100)] {
            let map = earth(&rules, w, h);
            let comp = regions(&rules, &map);
            // The region of the point's tile or a neighbour, of land or water.
            let near = |lon, lat, water: bool| {
                let t = tile_at(&map.grid, lon, lat);
                let t = std::iter::once(t).chain(map.grid.neighbors(t)).find(|&o| map.is_water(&rules, o) == water);
                comp[t.unwrap_or_else(|| panic!("{w}x{h}: nothing of the kind near {lon}, {lat}")) as usize]
            };
            let land = |lon, lat| near(lon, lat, false);
            let region = |_: &str, lon, lat| near(lon, lat, true);
            for (a, b, what) in [
                ((-1.5, 53.0), (2.0, 47.0), "Britain and France"),
                ((-8.0, 53.0), (-1.5, 53.0), "Ireland and Britain"),
                ((80.7, 7.7), (78.0, 20.0), "Sri Lanka and India"),
                ((138.5, 36.0), (127.8, 36.5), "Japan and Korea"),
                ((146.5, -42.0), (134.0, -25.0), "Tasmania and Australia"),
                ((-150.0, 64.0), (165.0, 66.0), "Alaska and Siberia"),
                ((46.5, -19.0), (30.0, -20.0), "Madagascar and Africa"),
            ] {
                assert_ne!(land(a.0, a.1), land(b.0, b.1), "{w}x{h}: {what} are joined");
            }
            for (a, b, what) in [
                ((-100.0, 22.0), (-50.0, -10.0), "the Americas"),
                ((30.0, 27.0), (2.0, 47.0), "Africa and Europe"),
                ((-4.0, 40.0), (2.0, 47.0), "Spain and France"),
                ((100.0, 15.0), (78.0, 20.0), "Indochina and India"),
            ] {
                assert_eq!(land(a.0, a.1), land(b.0, b.1), "{w}x{h}: {what} are apart");
            }
            let ocean = region("Atlantic", -35.0, 30.0);
            for (name, lon, lat) in [
                ("Mediterranean", 18.0, 34.0),
                ("Black Sea", 34.0, 43.0),
                ("Red Sea", 38.5, 20.0),
                ("Persian Gulf", 51.0, 27.0),
                ("Baltic", 19.0, 56.5),
                ("North Sea", 3.0, 56.0),
            ] {
                assert_eq!(region(name, lon, lat), ocean, "{w}x{h}: the {name} is cut off from the ocean");
            }
        }
    }

    #[test]
    fn earth_is_the_same_for_every_seed() {
        use crate::mapgen::{generate, MapSettings, MapType};
        let rules = crate::rules::Rules::load_default().unwrap();
        let a = generate(&rules, &MapSettings { kind: MapType::Earth, seed: 5, ..Default::default() });
        let b = generate(&rules, &MapSettings { kind: MapType::Earth, seed: 912_345, ..Default::default() });
        for (x, y) in a.tiles.iter().zip(&b.tiles) {
            assert_eq!((x.terrain, x.elevation, x.feature, x.rivers, x.resource), (y.terrain, y.elevation, y.feature, y.rivers, y.resource));
        }
    }

    #[test]
    fn true_starts_put_civs_in_their_homelands() {
        use crate::game::{Game, GameSetup};
        use crate::rules::Ability;
        let rules = crate::rules::Rules::load_default().unwrap();
        for (w, h) in [(96, 60), (128, 80), (160, 100)] {
            let mut setup = GameSetup::first(&rules, rules.civ_count());
            setup.options.true_starts = true;
            let game = Game::with_setup(rules.clone(), earth(&rules, w, h), &setup);
            for (p, player) in game.players.iter().enumerate() {
                let civ = rules.civ(player.civ);
                let (lon, lat) = civ.home.expect("every civ has a home");
                let settler = game.units.values().find(|u| u.owner as usize == p && game.unit_def(u).has(Ability::FoundCity));
                let settler = settler.unwrap_or_else(|| panic!("{w}x{h}: {} has no settler", civ.name));
                let d = game.map.grid.distance(settler.tile, tile_at(&game.map.grid, lon, lat));
                // A neighbour, or the next ring where the homeland is all mountains and sea.
                assert!(d <= 2, "{w}x{h}: {} starts {d} tiles from home", civ.name);
            }
        }
    }

    #[test]
    fn distance_wraps_the_date_line() {
        let d = path_distance(&[(179.0, 0.0), (-179.0, 0.0)], 180.0, 0.0);
        assert!(d < 1e-6, "{d}");
        assert!((path_distance(&[(10.0, 0.0)], 12.0, 0.0) - 2.0).abs() < 1e-6);
    }
}
