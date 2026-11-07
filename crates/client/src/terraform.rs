//! The land changing under the player's eyes: when a tile's terrain changes
//! in the game (a canal digs land out into water, `Event::TerrainChanged`),
//! the world around it is built again from the map, the same way it was
//! built at load: the heightfield (which units, models and picking read),
//! the terrain chunks, the terrain textures and their mipmaps, the water
//! surface and its depth texture, and the ice. Plants follow in
//! `vegetation.rs`; improvements, the yields overlay and the strategic view
//! rebuild on the same event. So a dug tile is real sea, with real shores.

use crate::layout::{map_bounds, tile_center};
use crate::loading::InGameSet;
use crate::play::{GameEvent, Session};
use crate::terrain::height::HeightSampler;
use crate::terrain::{ClimateMap, Terrain, TerrainChunk, TerrainMaterial};
use crate::water::{IceCover, WaterGrid};
use bevy::prelude::*;
use rayon::prelude::*;
use sim::game::Event;
use std::time::Instant;

/// How far from a changed tile's centre the land may look different: its
/// shape blends with tiles up to two away, and the coast is warped a little.
pub const REACH: f32 = 5.0;

/// Runs before anything that reads the land's height after it changed.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReshapeSet;

pub struct TerraformPlugin;

impl Plugin for TerraformPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, reshape.in_set(ReshapeSet).after(crate::play::PlaySet).in_set(InGameSet));
    }
}

fn reshape(
    mut commands: Commands,
    session: Res<Session>,
    mut events: MessageReader<GameEvent>,
    mut terrain: ResMut<Terrain>,
    mut shown: ResMut<crate::GameMap>,
    grid: Option<ResMut<WaterGrid>>,
    ice: Option<ResMut<IceCover>>,
    chunks: Query<(&TerrainChunk, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Assets<TerrainMaterial>>,
    climate: Res<ClimateMap>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut tiles: Vec<_> = events.read().filter_map(|GameEvent(e)| if let Event::TerrainChanged { tile } = e { Some(*tile) } else { None }).collect();
    if tiles.is_empty() {
        return;
    }
    tiles.sort();
    tiles.dedup();
    let (Some(mut grid), Some(mut ice)) = (grid, ice) else { return };
    let started = Instant::now();
    let game = &session.game;
    // The rendering systems' copy of the map.
    for &t in &tiles {
        shown.map.tiles[t as usize] = game.map.tiles[t as usize];
    }
    let map = &shown.map;
    let rules = &shown.rules;
    let sampler = HeightSampler::new(map, rules);
    let hf = &mut terrain.heights;
    let period = hf.cols as f32 * hf.step;
    let wrap = |dx: f32| (dx + period * 0.5).rem_euclid(period) - period * 0.5;
    let areas: Vec<(Vec2, Vec2)> = tiles
        .iter()
        .map(|&t| {
            let c = tile_center(&map.grid, t);
            (c - Vec2::splat(REACH), c + Vec2::splat(REACH))
        })
        .collect();

    // The heightfield, from the height function again.
    for &(a, b) in &areas {
        let (c0, c1) = (((a.x - hf.origin.x) / hf.step).floor() as isize, ((b.x - hf.origin.x) / hf.step).ceil() as isize);
        let (r0, r1) = (((a.y - hf.origin.y) / hf.step).floor().max(0.0) as usize, (((b.y - hf.origin.y) / hf.step).ceil() as usize).min(hf.rows - 1));
        let (cols, origin, step) = (hf.cols, hf.origin, hf.step);
        let fresh: Vec<(usize, f32)> = (r0..=r1)
            .into_par_iter()
            .flat_map_iter(|r| {
                let sampler = &sampler;
                (c0..=c1).map(move |c| {
                    let cw = c.rem_euclid(cols as isize) as usize;
                    (r * cols + cw, sampler.height(origin + Vec2::new(cw as f32, r as f32) * step))
                })
            })
            .collect();
        for (i, h) in fresh {
            hf.data[i] = h;
        }
    }
    let t_heights = started.elapsed();

    // Terrain chunks over the changed land; mountain meshes are finer than
    // the heightfield, so they're sampled from the height function.
    let bounds = map_bounds(&map.grid);
    let fine = |p: Vec2| sampler.height(p);
    let overlaps = |(a0, a1): (Vec2, Vec2), (b0, b1): (Vec2, Vec2)| {
        let dx = wrap((a0.x + a1.x) * 0.5 - (b0.x + b1.x) * 0.5).abs();
        dx * 2.0 < (a1.x - a0.x) + (b1.x - b0.x) && a0.y < b1.y && b0.y < a1.y
    };
    let hf = &terrain.heights;
    let stale: Vec<(TerrainChunk, Handle<Mesh>)> =
        chunks.iter().filter(|(c, _)| areas.iter().any(|&a| overlaps(a, c.footprint(hf.step)))).map(|(c, m)| (*c, m.0.clone())).collect();
    let fresh: Vec<Mesh> = stale.par_iter().map(|(c, _)| c.mesh(hf, bounds, &fine)).collect();
    for ((_, handle), mesh) in stale.into_iter().zip(fresh) {
        if let Some(mut m) = meshes.get_mut(&handle) {
            *m = mesh;
        }
    }
    let t_meshes = started.elapsed();

    // The terrain's textures: the land repainted where it changed, and
    // their mipmaps after it.
    if let Some((_, material)) = materials.iter().next() {
        let handles = [material.base.base_color_texture.clone(), material.base.metallic_roughness_texture.clone(), Some(climate.0.clone())];
        let mut data: Vec<(Handle<Image>, Vec<u8>, u32, u32)> = Vec::new();
        for h in handles.into_iter().flatten() {
            let Some(mut image) = images.get_mut(&h) else { continue };
            let (w, ht) = (image.width(), image.height());
            if let Some(d) = image.data.take() {
                data.push((h, d, w, ht));
            }
        }
        if let [(_, albedo, w, h), (_, rough, ..), (_, clim, ..)] = &mut data[..] {
            let (w, h) = (*w, *h);
            for &area in &areas {
                let r = crate::terrain::texture::rebake(map, rules, hf, crate::terrain::TEXELS_PER_UNIT, area, albedo, rough, clim);
                for d in [&mut *albedo, &mut *rough, &mut *clim] {
                    crate::image_util::update_mips(d, w, h, r.x0, r.x1, r.y0, r.y1);
                }
            }
        }
        for (h, d, ..) in data {
            if let Some(mut image) = images.get_mut(&h) {
                image.data = Some(d);
            }
        }
    }
    let t_textures = started.elapsed();

    // The sea over the dug land, and its depths and ice.
    for &area in &areas {
        grid.extend(hf, area, &|_| 0.0, &mut ice, &mut commands, &mut meshes);
    }
    ice.refresh();
    info!(
        "terraform: {} tiles, heights {:.0} ms, meshes {:.0} ms, textures {:.0} ms, water {:.0} ms",
        tiles.len(),
        t_heights.as_secs_f64() * 1000.0,
        (t_meshes - t_heights).as_secs_f64() * 1000.0,
        (t_textures - t_meshes).as_secs_f64() * 1000.0,
        (started.elapsed() - t_textures).as_secs_f64() * 1000.0
    );
}
