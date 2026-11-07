//! Bakes the terrain textures from the map on the CPU:
//!  - albedo: each tile's ground style (chosen from its terrain and climate) as a
//!    procedural material, with borders blended by warped noise so the hex grid
//!    never shows as hard edges
//!  - roughness (Bevy's metallic-roughness layout)
//!  - climate data for the terrain shader: snow line, rock mask and frost

use super::height::{smoothstep, HeightField, HeightSampler, WATER_LEVEL};
use crate::layout::{hex_at, hex_center, map_bounds};
use crate::loading::{Progress, Stage};
use crate::wrapnoise::Cylinder;
use bevy::math::{Vec2, Vec3};
use noise::{Fbm, MultiFractal, Perlin};
use rayon::prelude::*;
use sim::map::{Map, Tile};
use sim::climate::{self, base_temperature};
use sim::rules::Rules;

/// Ground styles. Chosen per tile from its terrain/feature art and its climate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ground {
    Grass,
    BorealGrass,
    TropicalGrass,
    Plains,
    ColdSteppe,
    Savanna,
    Desert,
    ColdDesert,
    Tundra,
    FrozenTundra,
    Glacier,
    Seabed,
    TaigaFloor,
    ForestFloor,
    JungleFloor,
    Marsh,
    Oasis,
}

const GROUNDS: [Ground; 17] = [
    Ground::Grass,
    Ground::BorealGrass,
    Ground::TropicalGrass,
    Ground::Plains,
    Ground::ColdSteppe,
    Ground::Savanna,
    Ground::Desert,
    Ground::ColdDesert,
    Ground::Tundra,
    Ground::FrozenTundra,
    Ground::Glacier,
    Ground::Seabed,
    Ground::TaigaFloor,
    Ground::ForestFloor,
    Ground::JungleFloor,
    Ground::Marsh,
    Ground::Oasis,
];

/// Base ground of a tile from its terrain art and mean temperature (°C).
fn base_ground(art: &str, t: f32) -> Ground {
    match art {
        "grassland" if t < climate::BOREAL => Ground::BorealGrass,
        "grassland" if t >= climate::SUBTROPICAL + 1.0 => Ground::TropicalGrass,
        "grassland" => Ground::Grass,
        "plains" if t < climate::BOREAL => Ground::ColdSteppe,
        "plains" if t >= climate::SUBTROPICAL + 1.0 => Ground::Savanna,
        "plains" => Ground::Plains,
        "desert" if t < 14.0 => Ground::ColdDesert,
        "desert" => Ground::Desert,
        "tundra" if t < (climate::TUNDRA + climate::ICE_CAP) / 2.0 => Ground::FrozenTundra,
        "tundra" => Ground::Tundra,
        "snow" => Ground::Glacier,
        "ocean" | "coast" | "lake" => Ground::Seabed,
        _ => Ground::Plains,
    }
}

/// Ground under a feature, if the feature changes it.
fn feature_ground(art: &str, t: f32) -> Option<Ground> {
    match art {
        "forest" if t < climate::BOREAL => Some(Ground::TaigaFloor),
        "forest" => Some(Ground::ForestFloor),
        "jungle" => Some(Ground::JungleFloor),
        "marsh" => Some(Ground::Marsh),
        "oasis" => Some(Ground::Oasis),
        _ => None,
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Vec3 {
    Vec3::new(r as f32, g as f32, b as f32) / 255.0
}

struct Noises {
    cyl: Cylinder,
    fine: Fbm<Perlin>,
    patch: Fbm<Perlin>,
    grain: Perlin,
    strata: Fbm<Perlin>,
}

impl Noises {
    fn new(seed: u32, cyl: Cylinder) -> Self {
        Self {
            cyl,
            fine: Fbm::<Perlin>::new(seed.wrapping_add(21)).set_octaves(3),
            patch: Fbm::<Perlin>::new(seed.wrapping_add(22)).set_octaves(4),
            grain: Perlin::new(seed.wrapping_add(23)),
            strata: Fbm::<Perlin>::new(seed.wrapping_add(24)).set_octaves(2),
        }
    }
}

/// Colour (sRGB, 0..1) and roughness of a ground style at a point.
fn ground_color(g: Ground, p: Vec2, n: &Noises) -> (Vec3, f32) {
    let patch = n.cyl.get(&n.patch, p, 0.8, 0.0);
    let small = n.cyl.get(&n.patch, p, 4.0, 17.0);
    let grain = n.cyl.get(&n.grain, p, 14.0, 0.0);
    let mix3 = |a: Vec3, b: Vec3, c: Vec3, t1: f32, t2: f32| a.lerp(b, t1).lerp(c, t2);
    let (c, r) = match g {
        Ground::Grass => (
            mix3(rgb(74, 128, 38), rgb(112, 148, 50), rgb(52, 100, 32), smoothstep(-0.2, 0.5, patch), smoothstep(0.1, 0.6, small) * 0.5),
            0.92,
        ),
        // Cool, dark, slightly blue-green northern meadow with bare patches.
        Ground::BorealGrass => (
            mix3(rgb(64, 98, 48), rgb(92, 104, 62), rgb(108, 98, 72), smoothstep(-0.2, 0.5, patch), smoothstep(0.3, 0.7, small) * 0.5),
            0.92,
        ),
        // Lush, saturated tropical grass.
        Ground::TropicalGrass => (
            mix3(rgb(58, 132, 30), rgb(96, 150, 36), rgb(40, 108, 26), smoothstep(-0.2, 0.5, patch), smoothstep(0.1, 0.6, small) * 0.5),
            0.9,
        ),
        // Temperate steppe: golden grass with green and earth patches.
        Ground::Plains => (
            mix3(rgb(176, 160, 74), rgb(120, 136, 52), rgb(146, 116, 68), smoothstep(-0.3, 0.4, patch), smoothstep(0.35, 0.7, small) * 0.6),
            0.92,
        ),
        // Cold steppe: pale, dry, grey-olive.
        Ground::ColdSteppe => (
            mix3(rgb(146, 140, 98), rgb(118, 120, 84), rgb(128, 118, 100), smoothstep(-0.3, 0.4, patch), smoothstep(0.35, 0.7, small) * 0.6),
            0.93,
        ),
        // Savanna: dry yellow grass over red earth.
        Ground::Savanna => (
            mix3(rgb(196, 168, 86), rgb(168, 150, 70), rgb(170, 104, 62), smoothstep(-0.3, 0.4, patch), smoothstep(0.4, 0.75, small) * 0.7),
            0.93,
        ),
        Ground::Desert => {
            let ripple = ((p.x * 5.0 + p.y * 2.0 + patch * 6.0).sin() * 0.5 + 0.5).powf(3.0);
            (
                rgb(222, 186, 126).lerp(rgb(198, 148, 92), smoothstep(-0.2, 0.6, patch) * 0.6 + ripple * 0.15).lerp(
                    rgb(172, 124, 88),
                    smoothstep(0.45, 0.75, small) * 0.7,
                ),
                0.95,
            )
        }
        // Gravel and dust: Gobi-like grey-beige.
        Ground::ColdDesert => (
            mix3(rgb(178, 164, 132), rgb(150, 140, 120), rgb(120, 112, 104), smoothstep(-0.2, 0.6, patch), smoothstep(0.3, 0.7, small) * 0.6),
            0.95,
        ),
        // Moss, lichen and stones.
        Ground::Tundra => (
            mix3(rgb(110, 112, 78), rgb(132, 126, 112), rgb(150, 132, 92), smoothstep(0.0, 0.6, patch) * 0.7, smoothstep(0.3, 0.7, small) * 0.4),
            0.9,
        ),
        // Frozen, grey-brown ground; the shader adds frost and snow patches.
        Ground::FrozenTundra => (
            mix3(rgb(124, 122, 110), rgb(150, 148, 140), rgb(104, 100, 90), smoothstep(0.0, 0.6, patch) * 0.7, smoothstep(0.3, 0.7, small) * 0.4),
            0.88,
        ),
        // Glacier ice: blue-white, shows where snow slides off.
        Ground::Glacier => (rgb(214, 228, 240).lerp(rgb(170, 196, 220), smoothstep(-0.3, 0.5, patch) * 0.6), 0.45),
        Ground::Seabed => (rgb(170, 160, 120).lerp(rgb(120, 130, 100), smoothstep(-0.2, 0.5, patch)), 0.8),
        // Needles and dark moss.
        Ground::TaigaFloor => (rgb(52, 62, 40).lerp(rgb(80, 70, 50), smoothstep(0.0, 0.6, small) * 0.6), 0.95),
        Ground::ForestFloor => (rgb(58, 76, 34).lerp(rgb(84, 74, 44), smoothstep(0.0, 0.6, small) * 0.6), 0.95),
        Ground::JungleFloor => (rgb(34, 66, 26).lerp(rgb(58, 84, 28), smoothstep(-0.2, 0.5, small)), 0.9),
        Ground::Marsh => {
            let wet = smoothstep(0.15, 0.3, small);
            (mix3(rgb(92, 104, 52), rgb(70, 72, 46), rgb(44, 62, 58), smoothstep(-0.2, 0.4, patch), wet), 0.9 - wet * 0.75)
        }
        Ground::Oasis => (rgb(72, 128, 50), 0.85),
    };
    (c * (1.0 + grain * 0.08), r)
}

/// Height (world units) of the tree line for a sea-level temperature: low in the
/// boreal north, high in the tropics.
pub fn treeline_height(sea_temp: f32) -> f32 {
    ((sea_temp + 6.0) / 22.0 * 1.5).clamp(0.2, 2.0)
}

/// Bare rock with regional variety: grey granite, red sandstone, ochre and dark
/// basalt, blended by broad noise so each range has its own character.
fn regional_rock(p: Vec2, h: f32, n: &Noises) -> Vec3 {
    let a = n.cyl.get(&n.strata, p, 0.06, 131.0);
    let b = n.cyl.get(&n.strata, p, 0.06, 177.0);
    let granite = rgb(136, 132, 128);
    let sandstone = rgb(168, 98, 70);
    let ochre = rgb(176, 138, 84);
    let basalt = rgb(78, 76, 82);
    let base = granite
        .lerp(sandstone, smoothstep(0.15, 0.45, a))
        .lerp(ochre, smoothstep(0.2, 0.5, b) * 0.7)
        .lerp(basalt, smoothstep(0.25, 0.55, -a) * 0.8);
    base * (rock_color(p, h, n) / rgb(118, 112, 104)).clamp(Vec3::splat(0.6), Vec3::splat(1.5))
}

fn rock_color(p: Vec2, h: f32, n: &Noises) -> Vec3 {
    let s = n.cyl.get(&n.strata, p, 0.7, 0.0);
    let band = ((h * 14.0 + s * 3.0).sin() * 0.5 + 0.5) * 0.16;
    let grain = n.cyl.get(&n.grain, p, 18.0, 3.0);
    let scree = n.cyl.get(&n.patch, p, 2.5, 41.0);
    rgb(122, 114, 104)
        .lerp(rgb(88, 84, 82), smoothstep(-0.3, 0.4, s))
        .lerp(rgb(146, 136, 120), smoothstep(0.2, 0.6, scree) * 0.5)
        * (0.9 + band + grain * 0.1)
}

/// Slow-to-evaluate fields precomputed on a coarse grid and sampled bilinearly
/// while baking texels.
struct FieldGrid {
    origin: Vec2,
    step: f32,
    cols: usize,
    rows: usize,
    /// (mountain, hill, river distance) per grid point.
    data: Vec<[f32; 3]>,
}

impl FieldGrid {
    fn build(sampler: &HeightSampler, map: &Map, per_unit: f32) -> Self {
        Self::build_area(sampler, map, per_unit, None)
    }

    /// The grid, worked out only inside `area` (and a step around it) when
    /// one is given: the rest is left empty, for baking just that area.
    fn build_area(sampler: &HeightSampler, map: &Map, per_unit: f32, area: Option<(Vec2, Vec2)>) -> Self {
        let (min, max) = map_bounds(&map.grid);
        let cols = ((max.x - min.x) * per_unit).round() as usize;
        let step = (max.x - min.x) / cols as f32;
        let rows = ((max.y - min.y) / step).ceil() as usize + 1;
        let period = max.x - min.x;
        let inside = |p: Vec2| {
            area.is_none_or(|(a, b)| {
                let mid = (a.x + b.x) * 0.5;
                let dx = ((p.x - mid) + period * 0.5).rem_euclid(period) - period * 0.5;
                dx.abs() <= (b.x - a.x) * 0.5 + step * 2.0 && p.y >= a.y - step * 2.0 && p.y <= b.y + step * 2.0
            })
        };
        let data = (0..rows)
            .into_par_iter()
            .flat_map_iter(|r| {
                (0..cols).map(move |c| {
                    let p = min + Vec2::new(c as f32, r as f32) * step;
                    if !inside(p) {
                        return [0.0; 3];
                    }
                    let f = sampler.fields(p);
                    [f.mountain, f.hill, sampler.river_distance(p)]
                })
            })
            .collect();
        Self { origin: min, step, cols, rows, data }
    }

    fn sample(&self, p: Vec2) -> [f32; 3] {
        let g = (p - self.origin) / self.step;
        let (c, r) = (g.x.floor() as isize, g.y.floor() as isize);
        let (fx, fy) = (g.x - c as f32, (g.y - r as f32).clamp(0.0, 1.0));
        let at = |c: isize, r: isize| {
            let c = c.rem_euclid(self.cols as isize) as usize;
            let r = r.clamp(0, self.rows as isize - 1) as usize;
            self.data[r * self.cols + c]
        };
        let (a, b, c2, d) = (at(c, r), at(c + 1, r), at(c, r + 1), at(c + 1, r + 1));
        std::array::from_fn(|i| {
            let top = a[i] * (1.0 - fx) + b[i] * fx;
            let bottom = c2[i] * (1.0 - fx) + d[i] * fx;
            top * (1.0 - fy) + bottom * fy
        })
    }
}

/// Per-tile inputs to the bake.
#[derive(Clone, Copy)]
struct TileLook {
    ground: Ground,
    feature: Option<Ground>,
    /// 0..1 how frozen the ground is (greyer sand).
    frost: f32,
    moisture: f32,
    /// Mean temperature at sea level (see `season::sea_level_temperature`).
    temperature: f32,
    land: f32,
}

fn tile_look(t: &Tile, rules: &Rules) -> TileLook {
    let temp = t.temperature;
    TileLook {
        ground: base_ground(&rules.terrain(t.terrain).art, temp),
        feature: t.feature.and_then(|f| feature_ground(&rules.feature(f).art, temp)),
        moisture: t.moisture,
        temperature: crate::season::sea_level_temperature(t),
        land: if rules.terrain(t.terrain).water { 0.0 } else { 1.0 },
        frost: if rules.terrain(t.terrain).id == "snow" {
            1.0
        } else {
            // Tundra keeps mostly bare, mossy ground with snow patches; only near
            // the ice cap does snow take over.
            smoothstep(climate::TUNDRA - 2.0, climate::ICE_CAP - 3.0, temp) * 0.8
        },
    }
}

pub struct TerrainTextures {
    pub width: u32,
    pub height: u32,
    /// RGBA8 sRGB.
    pub albedo: Vec<u8>,
    /// RGBA8 linear: G = roughness, B = metallic (Bevy's metallic-roughness layout).
    pub roughness: Vec<u8>,
    /// RGBA8 linear: R = mean sea-level temperature (°C + 40) / 80, G = rock
    /// mask, B = broad noise, A = seasonal swing (°C + 30) / 60. The shaders
    /// work out the snow lying from these (see season.rs).
    pub climate: Vec<u8>,
}

pub fn bake(map: &Map, rules: &Rules, hf: &HeightField, texels_per_unit: f32, progress: &Progress) -> TerrainTextures {
    let sampler = HeightSampler::new(map, rules);
    let n = Noises::new(map.seed as u32, sampler.cylinder());
    let (width, height, min, texel) = texture_grid(map, texels_per_unit);
    let fields = FieldGrid::build(&sampler, map, 8.0);
    let looks: Vec<TileLook> = map.tiles.iter().map(|t| tile_look(t, rules)).collect();
    let ctx = Ctx { map, looks: &looks, hf, sampler: &sampler, fields: &fields, n: &n };
    progress.begin(Stage::Textures, height as usize);

    let rows: Vec<[Vec<u8>; 3]> = (0..height)
        .into_par_iter()
        .map(|y| {
            let mut out = [
                Vec::with_capacity(width as usize * 4),
                Vec::with_capacity(width as usize * 4),
                Vec::with_capacity(width as usize * 4),
            ];
            for x in 0..width {
                let [a, r, c] = ctx.texel(min + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * texel);
                out[0].extend_from_slice(&a);
                out[1].extend_from_slice(&r);
                out[2].extend_from_slice(&c);
            }
            progress.advance(1);
            out
        })
        .collect();

    let size = (width * height * 4) as usize;
    let (mut albedo, mut roughness, mut climate) =
        (Vec::with_capacity(size), Vec::with_capacity(size), Vec::with_capacity(size));
    for [a, r, c] in rows {
        albedo.extend(a);
        roughness.extend(r);
        climate.extend(c);
    }
    TerrainTextures { width, height, albedo, roughness, climate }
}

/// The terrain textures' size, the map's corner and one texel's size.
fn texture_grid(map: &Map, texels_per_unit: f32) -> (u32, u32, Vec2, Vec2) {
    let (min, max) = map_bounds(&map.grid);
    let width = ((max.x - min.x) * texels_per_unit).round() as u32;
    let height = ((max.y - min.y) * texels_per_unit).round() as u32;
    // Exact texel size so the texture repeats exactly once per wrap period.
    (width, height, min, (max - min) / Vec2::new(width as f32, height as f32))
}

/// A rectangle of texels (x wraps: `x0` may be negative or `x1` past the width).
#[derive(Clone, Copy, Debug)]
pub struct TexelRect {
    pub x0: i64,
    pub x1: i64,
    pub y0: u32,
    pub y1: u32,
}

/// Bakes the textures again inside `area` (world XZ) after the land there
/// changed, writing straight into the base level of each texture's data
/// (`albedo`, `roughness`, `climate`, as laid out by [`bake`]). Returns
/// the texels it rewrote, so their mipmaps can follow.
pub fn rebake(
    map: &Map,
    rules: &Rules,
    hf: &HeightField,
    texels_per_unit: f32,
    area: (Vec2, Vec2),
    albedo: &mut [u8],
    roughness: &mut [u8],
    climate: &mut [u8],
) -> TexelRect {
    let sampler = HeightSampler::new(map, rules);
    let n = Noises::new(map.seed as u32, sampler.cylinder());
    let (width, height, min, texel) = texture_grid(map, texels_per_unit);
    let fields = FieldGrid::build_area(&sampler, map, 8.0, Some(area));
    let looks: Vec<TileLook> = map.tiles.iter().map(|t| tile_look(t, rules)).collect();
    let ctx = Ctx { map, looks: &looks, hf, sampler: &sampler, fields: &fields, n: &n };
    let rect = TexelRect {
        x0: ((area.0.x - min.x) / texel.x).floor() as i64,
        x1: ((area.1.x - min.x) / texel.x).ceil() as i64,
        y0: ((area.0.y - min.y) / texel.y).floor().max(0.0) as u32,
        y1: (((area.1.y - min.y) / texel.y).ceil() as u32).min(height - 1),
    };
    let texels: Vec<(usize, [[u8; 4]; 3])> = (rect.y0..=rect.y1)
        .into_par_iter()
        .flat_map_iter(|y| {
            let ctx = &ctx;
            (rect.x0..=rect.x1).map(move |x| {
                let xw = x.rem_euclid(width as i64) as u32;
                let p = min + Vec2::new(xw as f32 + 0.5, y as f32 + 0.5) * texel;
                (((y * width + xw) * 4) as usize, ctx.texel(p))
            })
        })
        .collect();
    {
        for (i, [a, r, c]) in texels {
            albedo[i..i + 4].copy_from_slice(&a);
            roughness[i..i + 4].copy_from_slice(&r);
            climate[i..i + 4].copy_from_slice(&c);
        }
    }
    rect
}

struct Ctx<'a> {
    map: &'a Map,
    looks: &'a [TileLook],
    hf: &'a HeightField,
    sampler: &'a HeightSampler<'a>,
    fields: &'a FieldGrid,
    n: &'a Noises,
}

struct Texel {
    colour: Vec3,
    roughness: f32,
    /// Mean sea-level temperature and seasonal swing (°C).
    temperature: f32,
    swing: f32,
    rock: f32,
    /// Broad noise (-1..1) the shader uses to break up snow edges.
    weather: f32,
}

impl Ctx<'_> {
    /// One texel of each texture (albedo, roughness, climate) at `p`.
    fn texel(&self, p: Vec2) -> [[u8; 4]; 3] {
        let s = self.shade(p);
        let c = s.colour.clamp(Vec3::ZERO, Vec3::ONE) * 255.0;
        let unit = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [
            [c.x as u8, c.y as u8, c.z as u8, 255],
            [0, (s.roughness.clamp(0.0, 1.0) * 255.0) as u8, 0, 255],
            [unit((s.temperature + 40.0) / 80.0), unit(s.rock), unit(s.weather * 0.5 + 0.5), unit((s.swing + 30.0) / 60.0)],
        ]
    }

    /// Mountain surface by altitude zone: forest or scrub below the tree line,
    /// alpine meadow above it, scree aprons below cliffs and bare rock on steep
    /// faces; gullies darker and greener, ridges brighter.
    fn mountain_color(&self, p: Vec2, h: f32, slope: f32, sea_temp: f32, moisture: f32, rock_tint: Vec3) -> Vec3 {
        let n = self.n;
        // Curvature: negative in gullies and valleys, positive on ridges.
        let r = 0.3;
        let around = (self.hf.sample(p + Vec2::X * r)
            + self.hf.sample(p - Vec2::X * r)
            + self.hf.sample(p + Vec2::Y * r)
            + self.hf.sample(p - Vec2::Y * r))
            * 0.25;
        let curvature = (h - around).clamp(-0.25, 0.25);
        let gully = smoothstep(0.0, -0.12, curvature);
        let ridge = smoothstep(0.0, 0.12, curvature);
        let patch = n.cyl.get(&n.patch, p, 2.2, 61.0);

        let rock = regional_rock(p, h, n) * rock_tint;
        let tree_line = treeline_height(sea_temp) + patch * 0.12 + gully * 0.15;
        let dry = 1.0 - smoothstep(0.2, 0.45, moisture);
        // Vegetation colour by climate: boreal conifers, temperate forest, tropical
        // cloud forest; dry ranges get olive scrub instead.
        let forest = rgb(40, 62, 44)
            .lerp(rgb(52, 82, 40), smoothstep(4.0, 14.0, sea_temp))
            .lerp(rgb(38, 96, 40), smoothstep(18.0, 26.0, sea_temp))
            .lerp(rgb(124, 116, 74), dry);
        let meadow = rgb(126, 136, 78).lerp(rgb(150, 136, 96), dry);
        let scree = rgb(166, 158, 146) * rock_tint;

        // Slopes here are steeper than real ones (relief is exaggerated for
        // readability), so "gentle" covers what would be ordinary mountainsides.
        let gentle = 1.0 - smoothstep(1.0, 1.9, slope);
        let below = 1.0 - smoothstep(tree_line - 0.1, tree_line + 0.1, h);
        let alpine = (1.0 - below) * (1.0 - smoothstep(tree_line + 0.35, tree_line + 0.7, h));

        let mut c = rock;
        // Scree fans on moderate slopes, often just below cliffs.
        let scree_w = smoothstep(0.7, 1.1, slope) * (1.0 - smoothstep(1.6, 2.2, slope)) * smoothstep(-0.2, 0.3, patch);
        c = c.lerp(scree, scree_w * 0.7);
        c = c.lerp(meadow, alpine * gentle * smoothstep(-0.3, 0.2, patch + gully));
        c = c.lerp(forest, below * (gentle * 0.85 + gully * 0.5).min(1.0));
        // Shade gullies, lift ridges: reads as relief even from far away.
        c * (1.0 - gully * 0.3 + ridge * 0.15)
    }

    fn shade(&self, p: Vec2) -> Texel {
        let n = self.n;
        // Borders use the same warp as the height model plus a little fine
        // raggedness, so each terrain stays inside its own hex.
        let pw = p + self.sampler.border_warp(p) + n.cyl.vec2(&n.fine, p, 3.5) * 0.09;
        let home = hex_at(pw);

        let mut weights = [0.0f32; GROUNDS.len()];
        let mut feature_w = [0.0f32; GROUNDS.len()];
        let mut frost = 0.0;
        let mut moisture = 0.0;
        let mut temperature = 0.0;
        let mut land = 0.0;
        let mut frost_w = 0.0;
        for h in std::iter::once(home).chain((0..6).map(|d| home.neighbor(d))) {
            let d2 = pw.distance_squared(hex_center(h));
            let look = match self.map.grid.tile(h) {
                Some(id) => self.looks[id as usize],
                None => TileLook { ground: Ground::Seabed, feature: None, frost: 0.0, moisture: 0.5, temperature: -30.0, land: 0.0 },
            };
            let g = (-d2 / (2.0 * 0.36 * 0.36)).exp();
            weights[look.ground as usize] += g;
            frost += look.frost * g;
            moisture += look.moisture * g;
            temperature += look.temperature * g;
            land += look.land * g;
            frost_w += g;
            if let Some(f) = look.feature {
                // Features cover a slightly smaller area than the whole tile.
                feature_w[f as usize] += (-d2 / (2.0 * 0.33 * 0.33)).exp();
            }
        }
        let frost = frost / frost_w;
        let moisture = moisture / frost_w;
        let temperature = temperature / frost_w;
        let land = land / frost_w;

        // Sharpen the blend so borders are crisp but irregular.
        let mut colour = Vec3::ZERO;
        let mut rough = 0.0;
        let mut total = 0.0;
        for (i, &w) in weights.iter().enumerate() {
            let w = w * w * w;
            if w > 1e-5 {
                let (c, r) = ground_color(GROUNDS[i], p, n);
                colour += c * w;
                rough += r * w;
                total += w;
            }
        }
        colour /= total;
        rough /= total;

        for (i, &w) in feature_w.iter().enumerate() {
            let w = smoothstep(0.25, 0.75, w);
            if w > 0.0 {
                let (c, r) = ground_color(GROUNDS[i], p, n);
                colour = colour.lerp(c, w);
                rough += (r - rough) * w;
            }
        }

        let h = self.hf.sample(p);
        let slope = self.hf.gradient(p).length();
        let grain = n.cyl.get(&n.grain, p, 9.0, 5.0);
        let [mountain, hill_field, river_dist] = self.fields.sample(p);
        // Continuous latitude (rows are 1.5 units apart), so climate lines don't step.
        let lat = 90.0 - (p.y / 1.5 + 0.5) / self.map.grid.height as f32 * 180.0;
        let weather = n.cyl.get(&n.patch, p, 0.25, 77.0);
        let swing = crate::season::swing(lat, land);
        let massif = smoothstep(0.2, 0.7, mountain);

        if h < WATER_LEVEL {
            // Seabed: sandy shallows fading to dark depths (the water adds its own tint).
            let depth = WATER_LEVEL - h;
            let river_bed = 1.0 - smoothstep(0.15, 0.3, river_dist);
            let shallow = rgb(200, 186, 140).lerp(rgb(52, 70, 56), river_bed);
            colour = shallow.lerp(rgb(30, 58, 70), smoothstep(0.03, 0.6, depth));
            return Texel { colour, roughness: 0.8, temperature, swing, rock: 0.0, weather };
        }

        // Hills: drier, browner tops with rocky outcrops.
        let hill = smoothstep(0.08, 0.3, hill_field);
        if hill > 0.0 {
            let outcrop = smoothstep(0.25, 0.45, n.cyl.get(&n.patch, p, 3.0, 23.0));
            let hill_col = (colour * Vec3::new(0.95, 0.9, 0.82)).lerp(rock_color(p, h, n), outcrop * 0.7);
            colour = colour.lerp(hill_col, hill * smoothstep(0.15, 0.4, h));
        }

        // Mountains: altitude zones like real ranges.
        let sea_temp = base_temperature(lat);
        let warm = smoothstep(14.0, 24.0, sea_temp);
        let rock_tint = Vec3::ONE.lerp(Vec3::new(1.1, 0.96, 0.82), warm);
        if massif > 0.0 {
            colour = colour.lerp(self.mountain_color(p, h, slope, sea_temp, moisture, rock_tint), massif * smoothstep(0.2, 0.4, h));
        }

        // Beaches and river banks near the water line.
        let shore = 1.0 - smoothstep(0.015, 0.045 + grain * 0.015, h);
        let river_bank = 1.0 - smoothstep(0.1, 0.3, river_dist);
        if river_bank > 0.0 {
            colour = colour.lerp(rgb(86, 88, 52), river_bank * 0.5 * shore.max(0.4));
            rough -= river_bank * 0.3;
        }
        let sand = if frost > 0.5 { rgb(150, 146, 136) } else { rgb(214, 198, 150) };
        colour = colour.lerp(sand, shore * (1.0 - river_bank));

        // Exposed rock on steep slopes (not at the waterline step).
        let rock = smoothstep(0.6 + grain * 0.15, 1.2, slope) * smoothstep(0.06, 0.16, h);
        colour = colour.lerp(rock_color(p, h, n) * rock_tint, rock);
        rough += (0.85 - rough) * rock.max(massif);

        // Fine variation so large areas never look flat.
        colour *= 1.0 + grain * 0.04;
        Texel { colour, roughness: rough, temperature, swing, rock: massif.max(rock), weather }
    }
}
