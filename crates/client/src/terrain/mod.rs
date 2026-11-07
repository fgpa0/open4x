//! Terrain rendering: chunked heightfield meshes with two levels of detail, a
//! baked procedural albedo, and a shader adding rock detail, snow and frost.

pub mod detail;
pub mod height;
pub mod texture;

use crate::fog::FogOfWar;
use crate::image_util::rgba_image;
use crate::layout::map_bounds;
use crate::loading::{AppState, Progress, Stage};
use crate::lod::Lod;
use crate::wrap::Wrapped;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use height::{HeightField, HeightSampler};
use rayon::prelude::*;
use sim::map::Map;
use sim::rules::Rules;
use std::time::Instant;

/// Heightfield samples per world unit (a hex is ~1.7 units wide).
const MESH_RESOLUTION: f32 = 7.0;
pub(crate) const TEXELS_PER_UNIT: f32 = 20.0;
/// Heightfield cells per side of one terrain chunk.
const CHUNK: usize = 64;
/// Camera distance at which chunks switch to the coarse mesh.
pub const LOD_DISTANCE: f32 = 58.0;
/// Depth of the skirt hanging below each chunk's edges, hiding cracks between
/// chunks drawn at different resolutions.
const SKIRT: f32 = 0.6;

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct TerrainParams {
    snow_color: Vec4,
    detail_scale: Vec2,
    fog: Vec4,
    /// x: the season for snow (see `season::Season::for_snow`).
    season: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct TerrainExtension {
    #[uniform(100)]
    params: TerrainParams,
    #[texture(101)]
    #[sampler(102)]
    climate: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    detail: Handle<Image>,
    #[texture(105)]
    #[sampler(106)]
    fog: Handle<Image>,
    #[texture(107)]
    #[sampler(108)]
    weather: Handle<Image>,
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://client/shaders/terrain.wesl".into()
    }
}

fn follow_season(season: Res<crate::season::Season>, mut materials: ResMut<Assets<TerrainMaterial>>) {
    for (_, m) in materials.iter_mut() {
        m.extension.params.season.x = season.for_snow();
    }
}

/// The terrain's climate texture (see `texture.rs`), shared with the plants.
#[derive(Resource, Default)]
pub struct ClimateMap(pub Handle<Image>);

#[derive(Resource)]
pub struct Terrain {
    pub heights: HeightField,
    /// How long generation took, for the debug screen.
    pub build_ms: f32,
}

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        // Embedded from the crate root so the asset path is `client/shaders/...`.
        crate::embed_terrain_shader(app);
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default())
            .init_resource::<ClimateMap>()
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(Update, follow_season.run_if(resource_changed::<crate::season::Season>));
    }
}

struct ChunkMeshes {
    near: Mesh,
    far: Mesh,
    center: Vec3,
    footprint: (Vec2, Vec2),
    shape: TerrainChunk,
}

/// How a terrain chunk's mesh was built, so it can be built again when the
/// land changes (canals dug into it; see `terraform.rs`).
#[derive(Component, Clone, Copy)]
pub struct TerrainChunk {
    pub origin: Vec2,
    /// Height samples across and down (at the heightfield's step).
    pub cols: usize,
    pub rows: usize,
    /// Mountains get a finer near mesh, sampled from the height function.
    pub mountainous: bool,
    pub near: bool,
}

impl TerrainChunk {
    pub fn footprint(&self, step: f32) -> (Vec2, Vec2) {
        (self.origin, self.origin + Vec2::new(self.cols as f32, self.rows as f32) * step)
    }

    /// The chunk's mesh, with heights from `fine` (finer mountain meshes) or
    /// the heightfield `hf`.
    pub fn mesh(&self, hf: &HeightField, bounds: (Vec2, Vec2), fine: &dyn Fn(Vec2) -> f32) -> Mesh {
        let (min, max) = bounds;
        let uv = |p: Vec2| [(p.x - min.x) / (max.x - min.x), (p.y - min.y) / (max.y - min.y)];
        let (o, w, h, step) = (self.origin, self.cols, self.rows, hf.step);
        match (self.mountainous, self.near) {
            (true, true) => grid_mesh(o, step / 2.0, w * 2, h * 2, fine, uv),
            (true, false) | (false, true) => grid_mesh(o, step, w, h, |p| hf.sample(p), uv),
            (false, false) => grid_mesh(o, step * 2.0, w.div_ceil(2), h.div_ceil(2), |p| hf.sample(p), uv),
        }
    }
}

/// Terrain built off the main thread: heights, textures and chunk meshes.
#[derive(Resource)]
pub struct TerrainBuild {
    pub heights: HeightField,
    albedo: Image,
    roughness: Image,
    climate: Image,
    detail: Image,
    chunks: Vec<ChunkMeshes>,
}

/// Builds all terrain data. Pure CPU work, safe to run on any thread.
pub fn build(map: &Map, rules: &Rules, progress: &Progress) -> TerrainBuild {
    let t = Instant::now();
    progress.begin(Stage::Heights, 1);
    let hf = HeightField::build(map, rules, MESH_RESOLUTION);
    let t_height = t.elapsed();
    let tex = texture::bake(map, rules, &hf, TEXELS_PER_UNIT, progress);
    let t_tex = t.elapsed();

    // Chunk meshes, built in parallel. Chunks containing mountains get a
    // double-resolution near mesh sampled straight from the height function.
    let sampler = HeightSampler::new(map, rules);
    let (min, max) = map_bounds(&map.grid);
    let uv = |p: Vec2| [(p.x - min.x) / (max.x - min.x), (p.y - min.y) / (max.y - min.y)];
    let mut ranges = Vec::new();
    for r0 in (0..hf.rows - 1).step_by(CHUNK) {
        for c0 in (0..hf.cols).step_by(CHUNK) {
            ranges.push((c0, r0, CHUNK.min(hf.cols - c0), CHUNK.min(hf.rows - 1 - r0)));
        }
    }
    progress.begin(Stage::Meshes, ranges.len());
    let chunks: Vec<ChunkMeshes> = ranges
        .par_iter()
        .map(|&(c0, r0, w, h)| {
            let origin = hf.origin + Vec2::new(c0 as f32, r0 as f32) * hf.step;
            let size = Vec2::new(w as f32, h as f32) * hf.step;
            let mountainous = (0..=8).any(|i| {
                (0..=8).any(|j| sampler.fields(origin + size * Vec2::new(i as f32, j as f32) / 8.0).mountain > 0.05)
            });
            let near = if mountainous {
                grid_mesh(origin, hf.step / 2.0, w * 2, h * 2, |p| sampler.height(p), uv)
            } else {
                grid_mesh(origin, hf.step, w, h, |p| hf.sample(p), uv)
            };
            // Far mesh: mountains keep the full heightfield resolution (every other
            // vertex of the near mesh), so peaks and their shadows don't change
            // shape at the switch; flat land can be much coarser.
            let far = if mountainous {
                grid_mesh(origin, hf.step, w, h, |p| hf.sample(p), uv)
            } else {
                grid_mesh(origin, hf.step * 2.0, w.div_ceil(2), h.div_ceil(2), |p| hf.sample(p), uv)
            };
            let mid = origin + size * 0.5;
            progress.advance(1);
            ChunkMeshes {
                near,
                far,
                center: Vec3::new(mid.x, hf.sample(mid).max(0.0), mid.y),
                footprint: (origin, origin + size),
                shape: TerrainChunk { origin, cols: w, rows: h, mountainous, near: true },
            }
        })
        .collect();
    info!(
        "terrain: heights {:.0} ms, textures {}x{} {:.0} ms, meshes {:.0} ms",
        t_height.as_secs_f64() * 1000.0,
        tex.width,
        tex.height,
        (t_tex - t_height).as_secs_f64() * 1000.0,
        (t.elapsed() - t_tex).as_secs_f64() * 1000.0
    );

    // The CPU copies stay, so canals can repaint the land they dig (see `terraform.rs`).
    let kept = |mut image: Image| {
        image.asset_usage = bevy::asset::RenderAssetUsages::default();
        image
    };
    TerrainBuild {
        albedo: kept(rgba_image(tex.albedo, tex.width, tex.height, true, true, false)),
        roughness: kept(rgba_image(tex.roughness, tex.width, tex.height, false, true, false)),
        climate: kept(rgba_image(tex.climate, tex.width, tex.height, false, true, false)),
        detail: rgba_image(detail::detail_texture(512), 512, 512, false, true, true),
        heights: hf,
        chunks,
    }
}

/// Uploads the built terrain and spawns its chunks (main thread, fast).
pub fn spawn(
    mut commands: Commands,
    mut build: ResMut<TerrainBuild>,
    game: Res<crate::GameMap>,
    fog: Res<FogOfWar>,
    weather: Res<crate::weather::WeatherMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut climate_map: ResMut<ClimateMap>,
) {
    let take = |img: &mut Image| std::mem::take(img);
    let climate = images.add(take(&mut build.climate));
    climate_map.0 = climate.clone();
    let material = materials.add(TerrainMaterial {
        base: StandardMaterial {
            base_color_texture: Some(images.add(take(&mut build.albedo))),
            metallic_roughness_texture: Some(images.add(take(&mut build.roughness))),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            reflectance: 0.3,
            ..default()
        },
        extension: TerrainExtension {
            params: TerrainParams {
                snow_color: Vec4::new(0.84, 0.87, 0.92, 1.0),
                detail_scale: Vec2::new(1.3, 5.5),
                fog: crate::fog::params(&game.map.grid),
                season: Vec4::ZERO,
            },
            climate,
            detail: images.add(take(&mut build.detail)),
            fog: fog.image.clone(),
            weather: weather.image.clone(),
        },
    });
    for chunk in std::mem::take(&mut build.chunks) {
        for (mesh, near) in [(chunk.near, true), (chunk.far, false)] {
            commands.spawn((
                TerrainChunk { near, ..chunk.shape },
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                Wrapped::mesh(chunk.center.x),
                Lod { center: chunk.center, near, switch: LOD_DISTANCE },
                // Unexplored terrain is drawn as mist and casts no shadows.
                crate::fog::Footprint { min: chunk.footprint.0, max: chunk.footprint.1, hide: false },
            ));
        }
    }
    commands.remove_resource::<TerrainBuild>();
}

/// A `cols` x `rows` grid of quads starting at `origin`, with normals from the
/// height function and a skirt around the edge.
fn grid_mesh(
    origin: Vec2,
    step: f32,
    cols: usize,
    rows: usize,
    height: impl Fn(Vec2) -> f32,
    uv: impl Fn(Vec2) -> [f32; 2],
) -> Mesh {
    // Heights with a one-sample border, for normals at the edges.
    let (bw, bh) = (cols + 3, rows + 3);
    let pos = |c: isize, r: isize| origin + Vec2::new(c as f32, r as f32) * step;
    let mut heights = Vec::with_capacity(bw * bh);
    for r in -1..=(rows as isize + 1) {
        for c in -1..=(cols as isize + 1) {
            heights.push(height(pos(c, r)));
        }
    }
    let h_at = |c: isize, r: isize| heights[((r + 1) as usize) * bw + (c + 1) as usize];

    let w = cols + 1;
    let mut positions = Vec::with_capacity(w * (rows + 1) + 4 * w);
    let mut normals = Vec::with_capacity(positions.capacity());
    let mut uvs = Vec::with_capacity(positions.capacity());
    for r in 0..=rows as isize {
        for c in 0..=cols as isize {
            let p = pos(c, r);
            let n = Vec3::new(h_at(c - 1, r) - h_at(c + 1, r), 2.0 * step, h_at(c, r - 1) - h_at(c, r + 1)).normalize();
            positions.push([p.x, h_at(c, r), p.y]);
            normals.push(n.to_array());
            uvs.push(uv(p));
        }
    }
    let mut indices = Vec::with_capacity(cols * rows * 6 + (cols + rows) * 12);
    for r in 0..rows {
        for c in 0..cols {
            let i = (r * w + c) as u32;
            let w = w as u32;
            indices.extend_from_slice(&[i, i + w, i + 1, i + 1, i + w, i + w + 1]);
        }
    }

    // Skirt: duplicate the border ring lower down and stitch it to the edge.
    let mut ring: Vec<u32> = Vec::new();
    ring.extend((0..w).map(|c| c as u32));
    ring.extend((1..=rows).map(|r| (r * w + cols) as u32));
    ring.extend((0..cols).rev().map(|c| (rows * w + c) as u32));
    ring.extend((1..rows).rev().map(|r| (r * w) as u32));
    let base = positions.len() as u32;
    for &i in &ring {
        let [x, y, z] = positions[i as usize];
        positions.push([x, y - SKIRT, z]);
        normals.push(normals[i as usize]);
        uvs.push(uvs[i as usize]);
    }
    let n = ring.len() as u32;
    for k in 0..n {
        let (a, b) = (ring[k as usize], ring[((k + 1) % n) as usize]);
        let (sa, sb) = (base + k, base + (k + 1) % n);
        // Double-sided so it hides cracks whichever way the edge runs.
        indices.extend_from_slice(&[a, sa, b, b, sa, sb, a, b, sa, b, sb, sa]);
    }

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}
