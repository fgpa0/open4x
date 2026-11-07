//! Paints a generated world as a small atlas-style image: biome colours with
//! relief shading, organic coasts, a sea that deepens offshore, and rivers.
//! Used for the map preview and the map type pictures in game setup. Runs off
//! the main thread.

use crate::layout::{hex_at, hex_center, map_bounds, tile_center};
use crate::image_util::rgba_image;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use sim::hex::Hex;
use sim::map::{Elevation, Map};
use sim::mapgen::{self, MapSettings};
use sim::rules::Rules;
use tiny_skia::{LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

type Rgb = [f32; 3];

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale(c: Rgb, f: f32) -> Rgb {
    [c[0] * f, c[1] * f, c[2] * f]
}

/// How far the seasons have swung on the first turn.
fn first_season() -> f32 {
    sim::climate::season(sim::climate::year_phase(1))
}

/// The flat colour of a tile, before shading.
pub(crate) fn tile_color(map: &Map, rules: &Rules, id: u32) -> Rgb {
    let t = map.tile(id);
    let terrain = rules.terrain(t.terrain).id.as_str();
    let feature = t.feature.map(|f| rules.feature(f).id.as_str());
    let mut c = match terrain {
        "ocean" => mix([22.0, 48.0, 82.0], [40.0, 86.0, 122.0], t.height * 2.2),
        "coast" => [58.0, 118.0, 146.0],
        "lake" => [64.0, 128.0, 156.0],
        "grassland" => [108.0, 144.0, 66.0],
        "plains" => [160.0, 152.0, 86.0],
        "desert" => [216.0, 192.0, 132.0],
        "tundra" => [138.0, 136.0, 112.0],
        "snow" => [236.0, 238.0, 242.0],
        _ => [120.0, 120.0, 120.0],
    };
    match feature {
        Some("forest") => c = mix(c, [46.0, 86.0, 44.0], 0.7),
        Some("jungle") => c = mix(c, [32.0, 98.0, 50.0], 0.75),
        Some("marsh") => c = mix(c, [84.0, 112.0, 84.0], 0.6),
        Some("oasis") => c = [86.0, 152.0, 92.0],
        _ => {}
    }
    // Ice as it lies on the first turn.
    if map.sea_ice(rules, id, first_season()) == sim::climate::SeaIce::Pack {
        c = [216.0, 230.0, 238.0];
    }
    match t.elevation {
        Elevation::Flat => c,
        Elevation::Hills => mix(c, [140.0, 122.0, 90.0], 0.3),
        Elevation::Mountains => {
            if t.temperature < 2.0 || t.height > 0.85 {
                [232.0, 232.0, 236.0]
            } else {
                [124.0, 114.0, 104.0]
            }
        }
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What the painter blends between tile centres: one value per tile for
/// each channel.
struct Channels {
    /// Ground colour on land (terrain, lightly tinted by marsh).
    ground: Vec<Rgb>,
    /// 1 on land.
    land: Vec<f32>,
    /// 1 where pack ice lies on the sea.
    ice: Vec<f32>,
    /// Height for shading: low plains, hills, mountains.
    relief: Vec<f32>,
    /// How rocky: 1 on mountains, some on hills.
    rock: Vec<f32>,
    /// Snow on the peaks (cold or very high mountains).
    snow: Vec<f32>,
    /// Tree cover: forest and jungle colours, weighted by how dense.
    canopy: Vec<f32>,
    jungle: Vec<f32>,
    /// Water: tiles from the nearest land (capped), for the depth ramp.
    depth: Vec<f32>,
}

fn channels(map: &Map, rules: &Rules) -> Channels {
    let n = map.tiles.len();
    let mut c = Channels {
        ground: vec![[0.0; 3]; n],
        land: vec![0.0; n],
        ice: vec![0.0; n],
        relief: vec![0.0; n],
        rock: vec![0.0; n],
        snow: vec![0.0; n],
        canopy: vec![0.0; n],
        jungle: vec![0.0; n],
        depth: vec![0.0; n],
    };
    // Distance from land, breadth first.
    let mut queue = std::collections::VecDeque::new();
    let mut dist = vec![u8::MAX; n];
    for id in 0..n as u32 {
        if !map.is_water(rules, id) {
            dist[id as usize] = 0;
            queue.push_back(id);
        }
    }
    while let Some(id) = queue.pop_front() {
        let d = dist[id as usize];
        for nb in map.grid.neighbors(id) {
            if dist[nb as usize] == u8::MAX {
                dist[nb as usize] = d + 1;
                queue.push_back(nb);
            }
        }
    }
    for id in 0..n as u32 {
        let i = id as usize;
        let t = map.tile(id);
        let terrain = rules.terrain(t.terrain).id.as_str();
        let feature = t.feature.map(|f| rules.feature(f).id.as_str());
        c.depth[i] = dist[i].min(7) as f32;
        if map.is_water(rules, id) {
            c.ice[i] = (map.sea_ice(rules, id, first_season()) == sim::climate::SeaIce::Pack) as u8 as f32;
            continue;
        }
        c.land[i] = 1.0;
        let mut g = match terrain {
            "grassland" => [104.0, 140.0, 70.0],
            "plains" => [158.0, 150.0, 92.0],
            "desert" => [214.0, 190.0, 138.0],
            "tundra" => [140.0, 138.0, 118.0],
            "snow" => [232.0, 236.0, 240.0],
            _ => [120.0, 120.0, 110.0],
        };
        if feature == Some("marsh") {
            g = mix(g, [96.0, 120.0, 92.0], 0.6);
        }
        c.ground[i] = g;
        match feature {
            Some("forest") => c.canopy[i] = 1.0,
            Some("jungle") => {
                c.canopy[i] = 1.0;
                c.jungle[i] = 1.0;
            }
            _ => {}
        }
        let (relief, rock) = match t.elevation {
            Elevation::Flat => (0.05 + t.height * 0.12, 0.0),
            Elevation::Hills => (0.4, 0.25),
            Elevation::Mountains => (1.0, 1.0),
        };
        c.relief[i] = relief;
        c.rock[i] = rock;
        if t.elevation == Elevation::Mountains && (t.temperature < 6.0 || t.height > 0.8) {
            c.snow[i] = 1.0;
        }
    }
    c
}

/// Paints `map` into an RGBA image `width` pixels wide (height follows the
/// map's proportions), like a shaded atlas. Every pixel blends the tiles
/// around it with a smooth falloff, looked up through warp noise so coasts
/// and the edges of forests and deserts wander instead of following hexes;
/// relief is shaded from the north-west, the sea deepens away from land.
pub fn paint(map: &Map, rules: &Rules, width: u32) -> Painted {
    use crate::wrapnoise::Cylinder;
    use noise::{Perlin, RidgedMulti, MultiFractal};

    let grid = &map.grid;
    let (min, max) = map_bounds(grid);
    let size = max - min;
    let height = ((width as f32) * size.y / size.x).round().max(1.0) as u32;
    let (w, h) = (width as usize, height as usize);
    let per_px = size.x / width as f32;
    let ch = channels(map, rules);

    let seed = map.seed as u32;
    let cyl = Cylinder::new(grid.width as f32 * 1.732);
    let warp_noise = Perlin::new(seed ^ 0x3A9);
    let fine = Perlin::new(seed ^ 0x51C);
    let ridges = RidgedMulti::<Perlin>::new(seed ^ 0x7E1).set_octaves(3);
    let warp = |p: Vec2| cyl.vec2(&warp_noise, p, 0.35) * 0.8 + cyl.vec2(&warp_noise, p, 1.2) * 0.35;

    // Per pixel, the blended channels.
    let n = w * h;
    let mut ground = vec![[0.0f32; 3]; n];
    let (mut land, mut ice, mut relief, mut rock) = (vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]);
    let (mut snow, mut canopy, mut jungle, mut depth) = (vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]);
    let mut height_px = vec![0.0f32; n];
    let mut under = vec![0u32; n];
    let rows = grid.height;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let p = min + Vec2::new((x as f32 + 0.5) * per_px, (y as f32 + 0.5) * per_px);
            let q = p + warp(p);
            // The hex under the point (rows clamped at the poles) and its ring.
            let (col, row) = hex_at(q).to_offset();
            let centre = Hex::from_offset(col, row.clamp(0, rows - 1));
            under[i] = grid.tile(centre).unwrap_or(0);
            let mut sum = 0.0;
            let mut acc = [0.0f32; 12];
            for hex in std::iter::once(centre).chain((0..6).map(|d| centre.neighbor(d))) {
                let Some(t) = grid.tile(hex) else { continue };
                let d2 = (hex_center(hex) - q).length_squared();
                let wt = (-d2 / 0.95).exp();
                let t = t as usize;
                sum += wt;
                let g = if ch.land[t] > 0.0 { ch.ground[t] } else { [0.0; 3] };
                for (k, v) in [g[0], g[1], g[2], ch.land[t], ch.ice[t], ch.relief[t], ch.rock[t], ch.snow[t], ch.canopy[t], ch.jungle[t], ch.depth[t]]
                    .into_iter()
                    .enumerate()
                {
                    acc[k] += v * wt;
                }
            }
            let sum = sum.max(1e-6);
            let l = acc[3] / sum;
            // Ground colour averages over land tiles only, so coasts don't go dark.
            let lw = acc[3].max(1e-6);
            ground[i] = [acc[0] / lw, acc[1] / lw, acc[2] / lw];
            // Fine noise roughens the coast and the edges of the ice.
            let rough = cyl.get(&fine, q, 1.8, 0.0) * 0.08;
            land[i] = l + rough;
            ice[i] = acc[4] / sum + rough;
            relief[i] = acc[5] / sum;
            rock[i] = acc[6] / sum;
            snow[i] = acc[7] / sum;
            canopy[i] = acc[8] / lw;
            jungle[i] = acc[9] / lw;
            depth[i] = acc[10] / sum;
            // Ridges on mountains, gentle swells elsewhere.
            let ridge = cyl.get(&ridges, q, 0.9, 5.0);
            height_px[i] = relief[i] + rock[i] * (ridge * 0.35 + 0.1) + cyl.get(&fine, q, 0.8, 9.0) * 0.025;
        }
    }

    let mut pixmap = Pixmap::new(width, height).expect("non-empty");
    let hex_px = 1.732 / per_px;
    for (i, px) in pixmap.pixels_mut().iter_mut().enumerate() {
        let (x, y) = (i % w, i / w);
        let at = |dx: isize, dy: isize| {
            let xx = (x as isize + dx).rem_euclid(w as isize) as usize;
            let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
            height_px[yy * w + xx]
        };
        let p = min + Vec2::new((x as f32 + 0.5) * per_px, (y as f32 + 0.5) * per_px);
        let q = p + warp(p);
        let speck = cyl.get(&fine, q, 3.5, 21.0);

        // Land: ground, trees, rock and snow, then shading.
        let mut l = ground[i];
        let tone = 1.0 + cyl.get(&fine, q, 0.25, 33.0) * 0.07;
        l = scale(l, tone);
        let trees = mix([62.0, 98.0, 52.0], [38.0, 102.0, 58.0], jungle[i]);
        l = mix(l, scale(trees, 0.88 + speck * 0.18), smoothstep(0.25, 0.75, canopy[i]) * 0.85);
        l = mix(l, [126.0, 116.0, 102.0], smoothstep(0.2, 0.8, rock[i]) * 0.75);
        let peaks = smoothstep(0.35, 0.75, snow[i]) * smoothstep(0.8, 1.1, height_px[i]) * 0.9;
        l = mix(l, [240.0, 242.0, 246.0], peaks);
        // Light from the north-west; steeper relief shades more.
        let slope = (at(-1, -1) - at(1, 1)) * hex_px * 0.6;
        l = scale(l, (1.0 + slope * 0.4).clamp(0.62, 1.35));

        // Sea: turquoise shallows to deep blue, with pack ice on top.
        let d = depth[i];
        let mut s = if d < 1.5 {
            mix([86.0, 160.0, 168.0], [50.0, 112.0, 146.0], smoothstep(0.6, 1.5, d))
        } else {
            mix([50.0, 112.0, 146.0], [24.0, 54.0, 92.0], smoothstep(1.5, 5.5, d))
        };
        s = scale(s, 1.0 + cyl.get(&fine, q, 0.3, 44.0) * 0.06);
        // Pack ice thins out into floes at its edge.
        let ice_cover = smoothstep(0.3, 0.75, ice[i] + cyl.get(&fine, q, 1.1, 61.0) * 0.25);
        s = mix(s, scale([226.0, 236.0, 242.0], 0.94 + speck * 0.06), ice_cover);

        // The coast: a crisp edge with a pale fringe of surf on the sea side.
        let m = land[i];
        // About a pixel of the land mask, for antialiasing.
        let aa = (0.6 / hex_px).clamp(0.02, 0.1);
        let surf = smoothstep(0.3, 0.5 - aa, m) * (1.0 - smoothstep(0.5 - aa, 0.5, m));
        s = mix(s, [170.0, 206.0, 204.0], surf * 0.5 * (1.0 - ice_cover));
        let mut c = mix(s, l, smoothstep(0.5 - aa, 0.5 + aa, m));
        let edge = 1.0 - ((m - 0.5).abs() / (aa * 1.5)).min(1.0);
        c = mix(c, [52.0, 62.0, 56.0], edge * 0.35);
        let ch = |v: f32| v.clamp(0.0, 255.0) as u8;
        *px = tiny_skia::PremultipliedColorU8::from_rgba(ch(c[0]), ch(c[1]), ch(c[2]), 255).expect("opaque");
    }

    // Rivers: from each tile's centre towards the neighbours they flow to,
    // shifted by the same warp as the land so they stay on it.
    let mut paint = Paint::default();
    paint.set_color_rgba8(66, 132, 164, 230);
    paint.anti_alias = true;
    let stroke = Stroke { width: (hex_px * 0.13).max(0.8), line_cap: LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
    let to_px = |p: Vec2| -> (f32, f32) { ((p - warp(p) - min) / per_px).into() };
    let mut pb = PathBuilder::new();
    for id in 0..grid.len() as u32 {
        let rivers = map.tile(id).rivers;
        if rivers == 0 || map.is_water(rules, id) {
            continue;
        }
        // Like the 3D rivers: one smooth bend through the centre, or arms that
        // meet there.
        let c = tile_center(grid, id);
        let edges: Vec<(f32, f32)> = (0..6)
            .filter(|d| rivers & (1 << d) != 0)
            .map(|d| to_px((c + hex_center(grid.hex(id).neighbor(d))) * 0.5))
            .collect();
        let mid = to_px(c);
        if let [a, b] = edges[..] {
            pb.move_to(a.0, a.1);
            pb.quad_to(mid.0, mid.1, b.0, b.1);
        } else {
            for e in edges {
                pb.move_to(e.0, e.1);
                pb.line_to(mid.0, mid.1);
            }
        }
    }
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
    // The world wraps east-west: roll the picture so its edges fall in the
    // middle of the widest stretch of open sea, keeping landmasses whole and
    // a supercontinent in the middle.
    let cols = grid.width as usize;
    let mut land_cols = vec![0usize; cols];
    for t in (0..grid.len() as u32).filter(|&t| !map.is_water(rules, t)) {
        land_cols[grid.offset(t).0 as usize] += 1;
    }
    // Columns with only a few islands' worth of land count as open sea; if
    // there are none, the emptiest columns do.
    let fewest = land_cols.iter().copied().min().unwrap_or(0);
    let open = |c: usize| land_cols[c % cols] <= fewest.max(grid.height as usize / 20);
    let (mut best, mut run) = ((0, 0), 0);
    for c in 0..cols * 2 {
        run = if open(c) { run + 1 } else { 0 };
        if run > best.1 && run <= cols {
            best = (c, run);
        }
    }
    let seam = (best.0 + cols * 2 - best.1 / 2) % cols;
    let shift = ((seam as f32 + 0.5) / cols as f32 * w as f32).round() as isize;
    let pixels = pixmap.pixels();
    let mut data = Vec::with_capacity(n * 4);
    let mut tiles = Vec::with_capacity(n);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + (x as isize + shift).rem_euclid(w as isize) as usize;
            let p = pixels[i];
            data.extend([p.red(), p.green(), p.blue(), 255]);
            tiles.push(under[i]);
        }
    }
    Painted { data, width, height, tiles }
}

/// A painted map: RGBA pixels and the tile each pixel shows.
pub struct Painted {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub tiles: Vec<u32>,
}

/// Which tile is where on a painted map, for picking tiles on it.
pub struct PreviewMap {
    pub map: Map,
    pub width: u32,
    pub height: u32,
    /// The tile under each pixel, row by row.
    pub tiles: Vec<u32>,
    /// Each tile's centre on the picture in pixels (`None` if it isn't shown).
    pub centres: Vec<Option<Vec2>>,
}

impl PreviewMap {
    fn new(map: Map, painted: &Painted) -> PreviewMap {
        let w = painted.width as usize;
        // Mean pixel per tile; x is measured from the tile's first pixel so a
        // tile cut by the wrap-around seam stays whole.
        let mut acc: Vec<Option<(f32, Vec2, f32)>> = vec![None; map.tiles.len()];
        for (i, &t) in painted.tiles.iter().enumerate() {
            let p = Vec2::new((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
            let entry = acc[t as usize].get_or_insert((p.x, Vec2::ZERO, 0.0));
            let dx = (p.x - entry.0 + w as f32 * 0.5).rem_euclid(w as f32) - w as f32 * 0.5;
            entry.1 += Vec2::new(dx, p.y);
            entry.2 += 1.0;
        }
        let centres = acc
            .into_iter()
            .map(|a| a.map(|(x0, sum, n)| Vec2::new((x0 + sum.x / n).rem_euclid(w as f32), sum.y / n)))
            .collect();
        PreviewMap { map, width: painted.width, height: painted.height, tiles: painted.tiles.clone(), centres }
    }

    /// The tile at a point on the picture (0..1 each way).
    pub fn tile_at(&self, uv: Vec2) -> Option<u32> {
        let x = (uv.x * self.width as f32).floor();
        let y = (uv.y * self.height as f32).floor();
        if x < 0.0 || y < 0.0 || x >= self.width as f32 || y >= self.height as f32 {
            return None;
        }
        self.tiles.get(y as usize * self.width as usize + x as usize).copied()
    }

    /// Where a tile is on the picture (0..1 each way).
    pub fn uv_of(&self, tile: u32) -> Option<Vec2> {
        let c = (*self.centres.get(tile as usize)?)?;
        Some(c / Vec2::new(self.width as f32, self.height as f32))
    }
}

/// A map being generated and painted in the background.
pub struct PreviewJob(Task<(Painted, Map)>);

impl PreviewJob {
    pub fn start(rules: Rules, settings: MapSettings, width: u32) -> PreviewJob {
        PreviewJob(AsyncComputeTaskPool::get().spawn(async move {
            let map = mapgen::generate(&rules, &settings);
            (paint(&map, &rules, width), map)
        }))
    }

    /// The finished image, once it is ready.
    pub fn poll(&mut self, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        self.poll_map(images).map(|(image, _)| image)
    }

    /// The finished image and which tile is where on it, once it is ready.
    pub fn poll_map(&mut self, images: &mut Assets<Image>) -> Option<(Handle<Image>, PreviewMap)> {
        let (painted, map) = block_on(future::poll_once(&mut self.0))?;
        let layout = PreviewMap::new(map, &painted);
        let mut image = rgba_image(painted.data, painted.width, painted.height, true, true, false);
        // UI layout reads image sizes in the main world.
        image.asset_usage = RenderAssetUsages::default();
        Some((images.add(image), layout))
    }
}
