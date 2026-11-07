//! Water surface: a glossy, translucent plane whose colour, transparency and shore
//! foam are computed per pixel by a custom shader from the depth of the terrain
//! below. Two scrolling layers of a procedural normal map make the waves.

use crate::image_util::rgba_image;
use crate::layout::{hex_center, map_bounds};
use crate::loading::AppState;
use crate::terrain::height::{gauss, HeightField, HeightSampler, Neighborhood, WATER_LEVEL};
use crate::wrap::Wrapped;
use sim::map::Map;
use sim::rules::Rules;
use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;
use std::f64::consts::TAU;

/// Water grid vertices per world unit (colour is per pixel, so this can be coarse).
const RESOLUTION: f32 = 2.0;
/// Quads per side of one water chunk.
const CHUNK: usize = 32;
/// Open sea beyond the north and south map edges, in world units.
const MARGIN: f32 = 60.0;

type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExtension>;

#[derive(Clone, Copy, ShaderType, Debug)]
struct WaterParams {
    origin: Vec2,
    size: Vec2,
    shallow: Vec4,
    mid: Vec4,
    deep: Vec4,
    foam: Vec4,
    wave_tiles: Vec2,
    fog: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
struct WaterExtension {
    #[uniform(100)]
    params: WaterParams,
    #[texture(101)]
    #[sampler(102)]
    heights: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    waves: Handle<Image>,
    #[texture(105)]
    #[sampler(106)]
    fog: Handle<Image>,
    #[texture(107)]
    #[sampler(108)]
    weather: Handle<Image>,
}

impl MaterialExtension for WaterExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://client/shaders/water.wesl".into()
    }
}

#[derive(Component)]
struct Water;

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/water.wesl");
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default())
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(Update, update_ice.after(crate::play::PlaySet).in_set(crate::loading::InGameSet));
    }
}

/// Linear RGBA from sRGB components.
fn lin(r: f32, g: f32, b: f32, a: f32) -> Vec4 {
    let l = Color::srgb(r, g, b).to_linear();
    Vec4::new(l.red, l.green, l.blue, a)
}

/// Converts an f32 to IEEE half-precision bits (range and precision are ample for heights).
fn f16_bits(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mant = bits & 0x7f_ffff;
    if exp <= 0 {
        sign
    } else if exp >= 31 {
        sign | 0x7c00
    } else {
        sign | ((exp as u16) << 10) | (mant >> 13) as u16
    }
}

/// Data for the water shader on the heightfield grid: R = terrain height,
/// G = sea ice cover (0 open, 0.5 thin, 1 pack; see `IceCover`, which fills it
/// in each turn). Also each texel's position warped like the coastline, so the
/// ice edge follows it.
fn water_data_texture(hf: &HeightField, map: &Map, rules: &Rules) -> (Image, Vec<u16>, Vec<Vec2>) {
    let sampler = HeightSampler::new(map, rules);
    let rows: Vec<(Vec<u16>, Vec<Vec2>)> = (0..hf.rows)
        .into_par_iter()
        .map(|r| {
            (0..hf.cols)
                .map(|c| {
                    let p = hf.origin + Vec2::new(c as f32, r as f32) * hf.step;
                    (f16_bits(hf.at(c as isize, r as isize)), p + sampler.border_warp(p))
                })
                .unzip()
        })
        .collect();
    let (heights, warped): (Vec<Vec<u16>>, Vec<Vec<Vec2>>) = rows.into_iter().unzip();
    let (heights, warped) = (heights.concat(), warped.concat());
    let mut image = Image::new(
        Extent3d { width: hf.cols as u32, height: hf.rows as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        ice_texels(&heights, &vec![0.0; heights.len()], &vec![0.0; heights.len()]),
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..Default::default()
    });
    (image, heights, warped)
}

/// The water texture's bytes: heights and ice cover.
/// The water's data texture: height of the ground below, ice cover, and how
/// still the water is (1: no waves), per heightfield sample.
fn ice_texels(heights: &[u16], cover: &[f32], still: &[f32]) -> Vec<u8> {
    heights
        .par_iter()
        .zip(cover.par_iter())
        .zip(still.par_iter())
        .flat_map_iter(|((&h, &c), &s)| {
            let [a, b] = h.to_le_bytes();
            let [x, y] = f16_bits(c).to_le_bytes();
            let [u, v] = f16_bits(s).to_le_bytes();
            [a, b, x, y, u, v, 0, 0]
        })
        .collect()
}

impl IceCover {
    /// Works the ice out again now (the water itself changed).
    pub(crate) fn refresh(&mut self) {
        self.turn = None;
    }

    /// The water's data texture (R: the terrain's height), also used by
    /// ambient particles to fade out where they meet the ground.
    pub(crate) fn image(&self) -> Handle<Image> {
        self.image.clone()
    }
}

/// Ice on the water this turn (see `sim::Map::sea_ice`), shown in the water
/// texture's G channel: blended smoothly between tiles and eased over a couple
/// of seconds when it changes with the season.
#[derive(Resource)]
pub(crate) struct IceCover {
    image: Handle<Image>,
    heights: Vec<u16>,
    warped: Vec<Vec2>,
    target: Vec<f32>,
    shown: Vec<f32>,
    /// How still the water is at each sample (1: no waves; nothing sets it now).
    still: Vec<f32>,
    turn: Option<u32>,
    settled: bool,
}

/// Ice cover per texel: 0 open, 0.5 thin, 1 pack (beyond the poles too).
fn ice_cover(game: &sim::game::Game, warped: &[Vec2]) -> Vec<f32> {
    use sim::climate::SeaIce;
    let tiles: Vec<f32> = (0..game.map.tiles.len() as sim::hex::TileId)
        .map(|t| match game.sea_ice(t) {
            SeaIce::Open => 0.0,
            SeaIce::Thin => 0.5,
            SeaIce::Pack => 1.0,
        })
        .collect();
    warped
        .par_iter()
        .map(|&pw| {
            let (mut cover, mut weight) = (0.0, 0.0);
            for h in Neighborhood::around(pw).hexes {
                let ice = game.map.grid.tile(h).map_or(1.0, |t| tiles[t as usize]);
                let g = gauss(pw.distance_squared(hex_center(h)), 0.55);
                cover += g * ice;
                weight += g;
            }
            cover / weight
        })
        .collect()
}

/// How fast the shown ice moves towards this turn's (cover per second).
const ICE_RATE: f32 = 0.5;

fn update_ice(
    time: Res<Time>,
    session: Res<crate::play::Session>,
    ice: Option<ResMut<IceCover>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(mut ice) = ice else { return };
    let turn = session.game.turn;
    if ice.turn != Some(turn) {
        let target = ice_cover(&session.game, &ice.warped);
        // A new game (or a loaded one) starts with its ice in place.
        if ice.turn.is_none() {
            ice.shown = target.clone();
        }
        ice.target = target;
        ice.turn = Some(turn);
        ice.settled = false;
    }
    if ice.settled {
        return;
    }
    let step = ICE_RATE * time.delta_secs().min(0.1);
    let IceCover { target, shown, .. } = &mut *ice;
    let moving = shown
        .par_iter_mut()
        .zip(target.par_iter())
        .map(|(s, &t)| {
            *s += (t - *s).clamp(-step, step);
            (*s != t) as u32
        })
        .sum::<u32>();
    ice.settled = moving == 0;
    let data = ice_texels(&ice.heights, &ice.shown, &ice.still);
    if let Some(mut image) = images.get_mut(&ice.image) {
        image.data = Some(data);
    }
}

/// Tileable wave normal map: noise sampled on a 4D torus so it repeats seamlessly.
fn wave_normals(size: u32) -> Vec<u8> {
    let noise = Fbm::<Perlin>::new(7).set_octaves(4);
    let s = size as usize;
    let mut heights = vec![0.0f32; s * s];
    for y in 0..s {
        for x in 0..s {
            let (u, v) = (x as f64 / s as f64 * TAU, y as f64 / s as f64 * TAU);
            let r = 1.2;
            heights[y * s + x] = noise.get([u.cos() * r, u.sin() * r, v.cos() * r, v.sin() * r]) as f32;
        }
    }
    let mut out = Vec::with_capacity(s * s * 4);
    for y in 0..s {
        for x in 0..s {
            let h = |dx: isize, dy: isize| {
                let xx = (x as isize + dx).rem_euclid(s as isize) as usize;
                let yy = (y as isize + dy).rem_euclid(s as isize) as usize;
                heights[yy * s + xx]
            };
            let strength = 6.0;
            let n = Vec3::new((h(-1, 0) - h(1, 0)) * strength, (h(0, -1) - h(0, 1)) * strength, 1.0).normalize();
            let e = (n * 0.5 + 0.5) * 255.0;
            out.extend_from_slice(&[e.x as u8, e.y as u8, e.z as u8, 255]);
        }
    }
    out
}

/// Water built off the main thread: shader data, wave texture and chunk meshes.
/// The water surface's grid and material, kept so water can be added where
/// the land is dug away (canals; see `terraform.rs`).
#[derive(Resource, Clone)]
pub(crate) struct WaterGrid {
    min_x: f32,
    z0: f32,
    step: f32,
    cols: usize,
    rows: usize,
    /// Grid points with water over them (or near enough to need the surface).
    wet: Vec<bool>,
    material: Handle<WaterMaterial>,
}

impl WaterGrid {
    /// Water over the land `hf` has cut below the surface since: a patch of
    /// surface on the same grid wherever none was before inside `area`, and
    /// the shader's depths there brought up to date, with how still the
    /// water is (`still`, 1 for no waves).
    pub(crate) fn extend(
        &mut self,
        hf: &HeightField,
        area: (Vec2, Vec2),
        still: &dyn Fn(Vec2) -> f32,
        ice: &mut IceCover,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) {
        // The depth texture is on the heightfield's grid.
        let (c0, c1) = (((area.0.x - hf.origin.x) / hf.step).floor() as isize - 1, ((area.1.x - hf.origin.x) / hf.step).ceil() as isize + 1);
        let (r0, r1) = (((area.0.y - hf.origin.y) / hf.step).floor() as isize - 1, ((area.1.y - hf.origin.y) / hf.step).ceil() as isize + 1);
        for r in r0.max(0)..=r1.min(hf.rows as isize - 1) {
            for c in c0..=c1 {
                let c = c.rem_euclid(hf.cols as isize);
                let i = r as usize * hf.cols + c as usize;
                ice.heights[i] = f16_bits(hf.data[i]);
                ice.still[i] = ice.still[i].max(still(hf.origin + Vec2::new(c as f32, r as f32) * hf.step));
            }
        }
        ice.settled = false;
        // The surface: grid points newly under water, and the quads around them.
        let at = |c: usize, r: usize| Vec2::new(self.min_x + c as f32 * self.step, self.z0 + r as f32 * self.step);
        let (gc0, gc1) = (((area.0.x - self.min_x) / self.step).floor() as isize - 1, ((area.1.x - self.min_x) / self.step).ceil() as isize + 1);
        let (gr0, gr1) = (((area.0.y - self.z0) / self.step).floor() as isize - 1, ((area.1.y - self.z0) / self.step).ceil() as isize + 1);
        let rows = gr0.max(0) as usize..=(gr1.max(0) as usize).min(self.rows - 1);
        let w = self.cols;
        let before = self.wet.clone();
        for r in rows.clone() {
            for c in gc0..=gc1 {
                let c = c.rem_euclid(w as isize) as usize;
                let p = at(c, r);
                if WATER_LEVEL - hf.sample(p) > -0.25 {
                    self.wet[r * w + c] = true;
                }
            }
        }
        let had = |c: usize, r: usize| [(0, 0), (1, 0), (0, 1), (1, 1)].iter().any(|(dc, dr)| before[(r + dr) * w + (c + dc) % w]);
        let has = |c: usize, r: usize| [(0, 0), (1, 0), (0, 1), (1, 1)].iter().any(|(dc, dr)| self.wet[(r + dr) * w + (c + dc) % w]);
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut indices = Vec::new();
        let mut mid = 0.0;
        for r in rows {
            if r + 1 >= self.rows {
                continue;
            }
            for c in gc0..=gc1 {
                let cw = c.rem_euclid(w as isize) as usize;
                if had(cw, r) || !has(cw, r) {
                    continue;
                }
                // Unwrapped x, so a patch across the seam stays in one piece.
                let x = |dc: isize| self.min_x + (c + dc) as f32 * self.step;
                let z = |dr: usize| self.z0 + (r + dr) as f32 * self.step;
                let base = positions.len() as u32;
                positions.extend([[x(0), WATER_LEVEL, z(0)], [x(1), WATER_LEVEL, z(0)], [x(0), WATER_LEVEL, z(1)], [x(1), WATER_LEVEL, z(1)]]);
                indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
                mid = x(0);
            }
        }
        if positions.is_empty() {
            return;
        }
        let n = positions.len();
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
            .with_inserted_indices(Indices::U32(indices));
        commands.spawn((Water, Mesh3d(meshes.add(mesh)), MeshMaterial3d(self.material.clone()), Transform::default(), Wrapped::mesh(mid), NotShadowCaster));
    }
}

#[derive(Resource)]
pub struct WaterBuild {
    params: WaterParams,
    grid: (f32, f32, f32, usize, usize, Vec<bool>),
    data: Image,
    /// Heights (f16) and warped positions per texel, for the ice cover.
    heights: Vec<u16>,
    warped: Vec<Vec2>,
    waves: Image,
    /// Chunk meshes with the X coordinate of their centre (for wrapping).
    chunks: Vec<(Mesh, f32)>,
}

/// Builds the water data. Pure CPU work, safe to run on any thread.
pub fn build(map: &Map, rules: &Rules, hf: &HeightField) -> WaterBuild {
    let (min, max) = map_bounds(&map.grid);
    let period = max.x - min.x;
    let cols = (period * RESOLUTION).round() as usize;
    let step = period / cols as f32;
    let z0 = min.y - MARGIN;
    let rows = ((max.y - min.y + 2.0 * MARGIN) / step).ceil() as usize;
    // Wave textures repeat a whole number of times per period so wrapping is seamless.
    let wave_tile = |size: f32| period / (period / size).round();

    let depth_at = |p: Vec2| WATER_LEVEL - hf.sample(p);
    let mut all_wet = vec![false; (rows + 1) * cols];
    for r in 0..=rows {
        for c in 0..cols {
            all_wet[r * cols + c] = depth_at(Vec2::new(min.x + c as f32 * step, z0 + r as f32 * step)) > -0.25;
        }
    }
    let mut chunks = Vec::new();
    for r0 in (0..rows).step_by(CHUNK) {
        for c0 in (0..cols).step_by(CHUNK) {
            let r1 = (r0 + CHUNK).min(rows);
            let c1 = (c0 + CHUNK).min(cols);
            let w = c1 - c0 + 1;
            let mut positions = Vec::new();
            let mut wet = Vec::new();
            for r in r0..=r1 {
                for c in c0..=c1 {
                    let p = Vec2::new(min.x + c as f32 * step, z0 + r as f32 * step);
                    positions.push([p.x, WATER_LEVEL, p.y]);
                    wet.push(depth_at(p) > -0.25);
                }
            }
            let mut indices = Vec::new();
            for r in 0..(r1 - r0) {
                for c in 0..(w - 1) {
                    let i = r * w + c;
                    // Skip quads well inside dry land.
                    if ![i, i + 1, i + w, i + w + 1].iter().any(|&k| wet[k]) {
                        continue;
                    }
                    let (i, w) = (i as u32, w as u32);
                    indices.extend_from_slice(&[i, i + w, i + 1, i + 1, i + w, i + w + 1]);
                }
            }
            if indices.is_empty() {
                continue;
            }
            let n = positions.len();
            let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
                .with_inserted_indices(Indices::U32(indices));
            chunks.push((mesh, min.x + (c0 + c1) as f32 * 0.5 * step));
        }
    }

    let (data, heights, warped) = water_data_texture(hf, map, rules);
    WaterBuild {
        grid: (min.x, z0, step, cols, rows + 1, all_wet),
        params: WaterParams {
            // Height samples sit on texel centres, so shift by half a texel.
            origin: hf.origin - Vec2::splat(hf.step * 0.5),
            size: Vec2::new(hf.cols as f32, hf.rows as f32) * hf.step,
            shallow: lin(0.25, 0.68, 0.66, 0.42),
            mid: lin(0.08, 0.42, 0.52, 0.74),
            deep: lin(0.03, 0.15, 0.28, 0.95),
            foam: lin(0.92, 0.96, 0.96, 0.9),
            wave_tiles: Vec2::new(wave_tile(5.0), wave_tile(2.3)),
            fog: crate::fog::params(&map.grid),
        },
        data,
        heights,
        warped,
        waves: rgba_image(wave_normals(256), 256, 256, false, true, true),
        chunks,
    }
}

/// Uploads the built water and spawns its chunks (main thread, fast).
fn spawn(
    mut commands: Commands,
    mut build: ResMut<WaterBuild>,
    fog: Res<crate::fog::FogOfWar>,
    weather: Res<crate::weather::WeatherMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let data = images.add(std::mem::take(&mut build.data));
    commands.insert_resource(IceCover {
        image: data.clone(),
        still: vec![0.0; build.heights.len()],
        heights: std::mem::take(&mut build.heights),
        warped: std::mem::take(&mut build.warped),
        target: Vec::new(),
        shown: Vec::new(),
        turn: None,
        settled: true,
    });
    let material = materials.add(WaterMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.07,
            reflectance: 0.6,
            ..default()
        },
        extension: WaterExtension {
            params: build.params,
            heights: data.clone(),
            waves: images.add(std::mem::take(&mut build.waves)),
            fog: fog.image.clone(),
            weather: weather.image.clone(),
        },
    });
    let (min_x, z0, step, cols, rows, wet) = std::mem::take(&mut build.grid);
    commands.insert_resource(WaterGrid { min_x, z0, step, cols, rows, wet, material: material.clone() });
    for (mesh, mid_x) in std::mem::take(&mut build.chunks) {
        commands.spawn((
            Water,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            Wrapped::mesh(mid_x),
            NotShadowCaster,
        ));
    }
    commands.remove_resource::<WaterBuild>();
}
