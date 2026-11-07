//! Tile improvements, roads and resources on the map: procedural models for
//! farms, mines, pastures, camps, plantations and quarries, dirt roads that
//! join neighbouring road tiles, tunnel portals and stone-lined canals, the resources themselves (animals, crops,
//! stone, fish) and small badges for the resources the player can see. Also the words the panels use for builds and resources.

use crate::ambience::HerdAnimal;
use crate::camera::CameraRig;
use crate::icons::{Icon, Icons};
use crate::layout::tile_center;
use crate::loading::InGameSet;
use crate::play::{GameEvent, Session};
use crate::procmesh::{at, blob, cone, cylinder, lin, Model};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::ui::{self, Tooltip};
use crate::wrap::{nearest_shift, WrapPeriod, Wrapped};
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sim::game::{Event, Game, PlayerId};
use sim::hex::TileId;
use sim::improvement::Work;
use sim::rules::{ImprovementId, ResourceId, ResourceKind, Yields};
use std::collections::HashMap;

pub struct ImprovementsPlugin;

impl Plugin for ImprovementsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_works, spin_sails, sync_badges, place_badges).chain().after(crate::play::PlaySet).after(crate::terraform::ReshapeSet).in_set(InGameSet).in_set(WorksSet),
        );
    }
}

// --- Words -------------------------------------------------------------------

pub fn work_name(game: &Game, work: Work) -> String {
    match work {
        Work::Road => "Road".into(),
        Work::Railroad => "Railroad".into(),
        Work::Improvement(i) => game.rules.improvement(i).name.clone(),
    }
}

pub fn work_icon(game: &Game, work: Work) -> Icon {
    match work {
        Work::Road => Icon::Road,
        Work::Railroad => Icon::Railroad,
        Work::Improvement(i) => Icon::improvement(&game.rules.improvement(i).art),
    }
}

fn yields_change(before: Yields, after: Yields) -> String {
    let mut parts = Vec::new();
    for (d, what) in [(after.food - before.food, "food"), (after.production - before.production, "production"), (after.gold - before.gold, "gold")] {
        if d != 0 {
            parts.push(format!("{d:+} {what}"));
        }
    }
    if parts.is_empty() { "no change in yields".into() } else { parts.join(", ") }
}

/// The tooltip for building `work` on `tile`: how long it takes, what it
/// changes and what it connects.
pub fn work_tooltip(game: &Game, me: PlayerId, tile: TileId, work: Work) -> Tooltip {
    let left = game.work_left(me, tile, work);
    let turns = if left == 1 { "1 turn".to_string() } else { format!("{left} turns") };
    let mut body = match work {
        Work::Road => "Moving from one road tile (or city) to the next costs 1, whatever the terrain.".to_string(),
        Work::Railroad => format!(
            "A railway over the road: moving from one railway tile (or city) to the next costs {}.",
            sim::path::show_moves(sim::path::RAIL_COST)
        ),
        Work::Improvement(i) => {
            let def = game.rules.improvement(i);
            let change = yields_change(game.tile_yields(me, tile), game.yields_with_work(me, tile, work));
            let mut b = format!("{} This tile: {change}.", def.description);
            if game.work_clears(me, tile, work) {
                if let Some(f) = game.map.tile(tile).feature.map(|f| game.rules.feature(f)) {
                    b.push_str(&format!(" Clears the {}", f.name));
                    match (f.clear_production, game.cities.values().filter(|c| c.owner == me).min_by_key(|c| game.map.grid.distance(c.tile, tile))) {
                        (p, Some(c)) if p > 0 => b.push_str(&format!(" (+{p} production for {}).", c.name)),
                        _ => b.push('.'),
                    }
                }
            }
            if let Some(r) = game.visible_resource(me, tile).filter(|&r| game.rules.resource_improvement(r) == Some(i)) {
                b.push_str(&format!(" Works the {}.", game.rules.resource(r).name));
            }
            if game.improvement_at(tile).is_some_and(|old| old != i) {
                b.push_str(&format!(" Replaces the {}.", game.rules.improvement(game.improvement_at(tile).unwrap()).name));
            }
            b
        }
    };
    if game.works[tile as usize].progress.is_some_and(|(w, _)| w == work) {
        body.push_str(" Work already started here counts.");
    }
    Tooltip::new(format!("Build {}: {turns}", work_name(game, work)), body)
}

fn kind_name(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Bonus => "bonus resource",
        ResourceKind::Luxury => "luxury",
        ResourceKind::Strategic => "strategic resource",
    }
}

/// One line about a resource on a tile: kind, yields and whether it's worked.
pub fn resource_line(game: &Game, me: PlayerId, tile: TileId, r: ResourceId) -> String {
    let def = game.rules.resource(r);
    let mut line = format!("{} ({})", def.name, kind_name(def.kind));
    match game.rules.resource_improvement(r) {
        Some(i) if game.improvement_at(tile) == Some(i) => line.push_str(" · worked"),
        Some(i) => line.push_str(&format!(" · needs a {}", game.rules.improvement(i).name)),
        None => {}
    }
    let _ = me;
    line
}

/// What a resource is good for, for tooltips.
pub fn resource_use(game: &Game, r: ResourceId) -> String {
    let def = game.rules.resource(r);
    match def.kind {
        ResourceKind::Bonus => "Better yields where it's worked.".into(),
        ResourceKind::Luxury => format!(
            "Each different luxury you have adds {} gold in every city. Trade spares to other civs.",
            game.rules.resources.luxury_gold
        ),
        ResourceKind::Strategic => {
            let units: Vec<&str> = (0..game.rules.units.units.len() as sim::rules::UnitTypeId)
                .filter(|&u| game.rules.unit_resource(u) == Some(r))
                .map(|u| game.rules.unit(u).name.as_str())
                .collect();
            format!("Needed to train {}.", units.join(" and "))
        }
    }
}

// --- Models ------------------------------------------------------------------

/// The models on a tile: an improvement and/or a road. Rebuilt when what's
/// on the tile changes.
#[derive(Component)]
struct WorksView {
    tile: TileId,
    look: Look,
    /// Its turning windmill sails.
    spinners: Vec<Entity>,
    /// Its grazing animals.
    animals: Vec<Entity>,
}

/// Windmill sails, turning about their hub.
#[derive(Component)]
struct Spinner;

fn spin_sails(time: Res<Time>, mut sails: Query<&mut Transform, With<Spinner>>) {
    for mut tf in &mut sails {
        tf.rotate_local_z(-time.delta_secs() * 0.7);
    }
}

/// Spawns a tile's windmill sails as children of its works.
fn spawn_spinners(commands: &mut Commands, parent: Entity, spinners: &[Transform], mesh: &Handle<Mesh>, material: &Handle<crate::vegetation::VegetationMaterial>) -> Vec<Entity> {
    spinners
        .iter()
        .map(|t| commands.spawn((Spinner, Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), *t, ChildOf(parent))).id())
        .collect()
}

/// Spawns a tile's animals as children of its works (`ambience::herds`
/// gives them their models and moves them).
/// The systems that spawn and despawn improvement models and their animals:
/// anything adding to those entities runs after it, so it never touches one
/// despawned the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorksSet;

fn spawn_animals(commands: &mut Commands, parent: Entity, animals: Vec<HerdAnimal>) -> Vec<Entity> {
    animals
        .into_iter()
        .map(|a| {
            let t = Transform::from_xyz(a.home.x, 0.0, a.home.y).with_rotation(Quat::from_rotation_y(a.yaw)).with_scale(Vec3::splat(a.size));
            commands.spawn((a, t, Visibility::default(), ChildOf(parent))).id()
        })
        .collect()
}

/// What a tile's models show: the improvement (and the resource it works,
/// for animals and crops), the resource on its own when the improvement
/// doesn't show it, and which neighbours its road leads to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Look {
    improvement: Option<(ImprovementId, Option<ResourceId>)>,
    resource: Option<ResourceId>,
    roads: Option<u8>,
    /// Railway branches towards neighbours with one (cities count).
    rails: Option<u8>,
    tunnel: bool,
    /// A bridge, with the two shores it joins and the era of its style
    /// (its land's owner's).
    bridge: Option<([usize; 2], sim::rules::EraId)>,
    /// A fort (true) or watchtower, in its owner's colour.
    post: Option<(bool, [u8; 3])>,
}

/// Improvements whose model already shows the resource they work.
const SHOWS_RESOURCE: [&str; 3] = ["farm", "pasture", "plantation"];

/// What `me` knows is on a tile.
fn look(game: &Game, terrain: &Terrain, me: PlayerId, tile: TileId) -> Option<Look> {
    if !game.players[me as usize].explored[tile as usize] {
        return None;
    }
    let improvement = game.improvement_at(tile).map(|i| (i, game.visible_resource(me, tile)));
    let drawn_by_improvement = improvement.is_some_and(|(i, _)| SHOWS_RESOURCE.contains(&game.rules.improvement(i).art.as_str()));
    let resource = game.visible_resource(me, tile).filter(|_| !drawn_by_improvement && game.city_at(tile).is_none());
    // Roads go towards each neighbour with a road; a city's road only shows
    // as the roads leading into it (the town covers its centre).
    let roads = (game.has_road(tile) && game.city_at(tile).is_none()).then(|| {
        let mut mask = 0u8;
        for (i, n) in neighbors_by_direction(game, tile).into_iter().enumerate() {
            if n.is_some_and(|n| game.has_road(n)) {
                mask |= 1 << i;
            }
        }
        mask
    });
    let rails = (game.has_rail(tile) && game.city_at(tile).is_none()).then(|| {
        neighbors_by_direction(game, tile).into_iter().enumerate().filter(|(_, n)| n.is_some_and(|n| game.has_rail(n))).fold(0u8, |m, (i, _)| m | 1 << i)
    });
    let tunnel = game.has_tunnel(tile);
    let bridge = game.has_bridge(tile).then(|| (crate::bridges::ends(game, terrain, tile), bridge_era(game, tile)));
    let color = game.territory_owner(tile).map_or([200, 200, 200], |o| game.players[o as usize].color);
    let post = if game.has_fort(tile) { Some((true, color)) } else { game.has_watchtower(tile).then_some((false, color)) };
    (improvement.is_some() || resource.is_some() || roads.is_some() || post.is_some()).then_some(Look { improvement, resource, roads, rails, tunnel, bridge, post })
}

/// The era a bridge on `tile` is built in: its land's owner's, else the
/// earliest bridges can be built in.
pub fn bridge_era(game: &Game, tile: TileId) -> sim::rules::EraId {
    let earliest = game.rules.tech_era(game.rules.bridge_tech());
    game.territory_owner(tile).map_or(earliest, |p| game.era(p).max(earliest))
}

/// A tile's neighbours, in a fixed order of directions.
fn neighbors_by_direction(game: &Game, tile: TileId) -> [Option<TileId>; 6] {
    let grid = &game.map.grid;
    let h = grid.hex(tile);
    let mut out = [None; 6];
    for (i, d) in sim::hex::DIRECTIONS.iter().enumerate() {
        out[i] = grid.tile(sim::hex::Hex::new(h.q + d.q, h.r + d.r));
    }
    out
}

fn sync_works(
    mut commands: Commands,
    session: Res<Session>,
    terrain: Res<Terrain>,
    scenery: Res<crate::vegetation::Scenery>,
    mut events: MessageReader<GameEvent>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut views: Query<(Entity, &mut WorksView, &Mesh3d, &mut Transform)>,
    mut sails: Local<Option<Handle<Mesh>>>,
) {
    let relevant = events.read().any(|GameEvent(e)| {
        matches!(e, Event::TileImproved { .. } | Event::TerrainChanged { .. } | Event::Explored { .. } | Event::CityFounded { .. } | Event::TechResearched { .. })
    });
    if !relevant && !session.is_added() {
        return;
    }
    let game = &session.game;
    let me = session.me;
    let sails = sails
        .get_or_insert_with(|| {
            let mut m = crate::models::prop("windmill_sails").clone();
            let bare = crate::vegetation::snow_tag(crate::vegetation::Surface::Bare, 0.5);
            m.map_alpha_from(0, |_| bare);
            meshes.add(m.into_mesh())
        })
        .clone();
    let mut shown: HashMap<TileId, Entity> = HashMap::new();
    for (entity, mut view, mesh, mut transform) in &mut views {
        match look(game, &terrain, me, view.tile) {
            None => {
                commands.entity(entity).despawn();
            }
            Some(l) => {
                shown.insert(view.tile, entity);
                if l != view.look {
                    view.look = l;
                    let (model, spinners, animals) = tile_model(game, &terrain, view.tile, l);
                    if let Some(mut m) = meshes.get_mut(&mesh.0) {
                        *m = model.into_mesh();
                    }
                    for e in view.spinners.drain(..) {
                        commands.entity(e).despawn();
                    }
                    view.spinners = spawn_spinners(&mut commands, entity, &spinners, &sails, &scenery.0);
                    for e in view.animals.drain(..) {
                        commands.entity(e).despawn();
                    }
                    view.animals = spawn_animals(&mut commands, entity, animals);
                    // The ground may have changed under it (a canal cut).
                    transform.translation.y = terrain.heights.sample(tile_center(&game.map.grid, view.tile)).max(WATER_LEVEL);
                }
            }
        }
    }
    for tile in 0..game.map.tiles.len() as TileId {
        if shown.contains_key(&tile) {
            continue;
        }
        let Some(l) = look(game, &terrain, me, tile) else { continue };
        let centre = tile_center(&game.map.grid, tile);
        let y = terrain.heights.sample(centre).max(WATER_LEVEL);
        let (model, spinners, animals) = tile_model(game, &terrain, tile, l);
        let entity = commands
            .spawn((
                Mesh3d(meshes.add(model.into_mesh())),
                MeshMaterial3d(scenery.0.clone()),
                Transform::from_xyz(centre.x, y, centre.y),
                Wrapped { anchor: centre.x, base: centre.x },
            ))
            .id();
        let spinners = spawn_spinners(&mut commands, entity, &spinners, &sails, &scenery.0);
        let animals = spawn_animals(&mut commands, entity, animals);
        commands.entity(entity).insert(WorksView { tile, look: l, spinners, animals });
    }
}

/// Strips stop on slopes steeper than this (radians).
const MAX_SLOPE: f32 = 0.35;
/// How far around a point, and how much lower than that ground, counts as
/// being in a channel.
const CHANNEL_REACH: f32 = 0.2;
const CHANNEL_DEPTH: f32 = 0.05;

/// Props are drawn oversized, like units and towns: sizes and heights are
/// scaled by this, positions aren't.
const K: f32 = 3.0;

/// Builds a tile's models in coordinates relative to its centre (at ground
/// height), following the terrain.
struct Builder<'a> {
    m: Model,
    terrain: &'a Terrain,
    centre: Vec2,
    base: f32,
    /// Extra size for props on top of `K` (resources are drawn larger).
    scale: f32,
    /// Windmill sails to spin, placed relative to the tile's centre.
    spinners: Vec<Transform>,
    /// Animals that graze and wander (models of their own).
    animals: Vec<HerdAnimal>,
}

impl Builder<'_> {
    /// The size factor props are drawn at.
    fn k(&self) -> f32 {
        K * self.scale
    }

    fn ground(&self, p: Vec2) -> f32 {
        self.terrain.heights.sample(self.centre + p).max(WATER_LEVEL) - self.base
    }

    /// Whether `p` lies in a dip: well below the ground around it, as in a
    /// gorge or a river bed.
    fn in_channel(&self, p: Vec2) -> bool {
        let h = self.ground(p);
        let around = (0..6)
            .map(|i| {
                let a = i as f32 * std::f32::consts::FRAC_PI_3;
                self.ground(p + Vec2::new(a.cos(), a.sin()) * CHANNEL_REACH)
            })
            .sum::<f32>()
            / 6.0;
        around - h > CHANNEL_DEPTH
    }

    /// A transform at `p` (relative to the centre), `y` above the ground there.
    fn on(&self, p: Vec2, y: f32) -> Transform {
        at(p.x, self.ground(p) + y * self.k(), p.y).with_scale(Vec3::splat(self.k()))
    }

    fn add(&mut self, mesh: Mesh, color: Vec4, t: Transform) {
        self.m.add(mesh, color, t);
    }

    /// A prop made in Blender (`art/models/works.py`) standing at `p`,
    /// turned by `yaw` and sized by `size` on top of the builder's scale.
    fn prop(&mut self, name: &str, p: Vec2, yaw: f32, size: f32) {
        let mut t = self.on(p, 0.0).with_rotation(Quat::from_rotation_y(yaw));
        t.scale *= size;
        self.m.append(crate::models::prop(name), &t, Vec4::ONE);
    }

    /// [`Self::prop`] with its team-coloured surfaces in `team`.
    fn team_prop(&mut self, name: &str, p: Vec2, yaw: f32, size: f32, team: Vec4) {
        let mut t = self.on(p, 0.0).with_rotation(Quat::from_rotation_y(yaw));
        t.scale *= size;
        self.m.append_with(crate::models::prop(name), &t, |c| match crate::models::Flag::of(c.w) {
            crate::models::Flag::Team => Some((c * team).with_w(1.0)),
            _ => Some(c.with_w(1.0)),
        });
    }

    /// A flat strip on the ground from `a` to `b`, `w` wide, split so it
    /// follows the slope.
    fn strip(&mut self, a: Vec2, b: Vec2, w: f32, lift: f32, color: Vec4) {
        let len = a.distance(b);
        let n = (len / 0.08).ceil().max(1.0) as usize;
        let dir = (b - a) / len.max(1e-4);
        let side = Vec2::new(-dir.y, dir.x) * w * 0.5;
        let yaw = -dir.y.atan2(dir.x);
        for i in 0..n {
            let (p0, p1) = (a + dir * len * i as f32 / n as f32, a + dir * len * (i + 1) as f32 / n as f32);
            let mid = (p0 + p1) * 0.5;
            let piece = len / n as f32;
            // Tilted to follow the ground along and across the strip.
            let pitch = (self.ground(p1) - self.ground(p0)).atan2(piece);
            let roll = (self.ground(mid + side) - self.ground(mid - side)).atan2(w.max(1e-3));
            // Nothing grows (or is paved) on slopes this steep, or down in
            // a river's channel.
            let steep = pitch.abs().max(roll.abs());
            if steep > MAX_SLOPE || self.in_channel(mid) {
                continue;
            }
            let rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(pitch) * Quat::from_rotation_x(-roll);
            let t = at(mid.x, self.ground(mid) + lift + steep * 0.03, mid.y).with_rotation(rotation);
            self.m.add(Cuboid::new(piece * 1.06, 0.012, w).mesh().build(), color, t);
        }
    }
}

/// The vertex alpha that marks surfaces snow doesn't settle on (the steep
/// stone of tunnel portals) until `tile_model` tags them.
const WATER_MARK: f32 = 0.5;

/// A tile's models, tagged so snow settles on fields, roads and roofs (see
/// the plants' shader).
fn tile_model(game: &Game, terrain: &Terrain, tile: TileId, look: Look) -> (Model, Vec<Transform>, Vec<HerdAnimal>) {
    let (mut m, spinners, animals) = build_tile_model(game, terrain, tile, look);
    let ground = terrain.heights.sample(tile_center(&game.map.grid, tile)).max(WATER_LEVEL);
    let tag = crate::vegetation::snow_tag(crate::vegetation::Surface::Roof, ground);
    // Tunnel portals (marked with a lower alpha) stay clear of snow.
    let bare = crate::vegetation::snow_tag(crate::vegetation::Surface::Bare, ground);
    m.map_alpha_from(0, |a| if a < WATER_MARK + 0.01 { bare } else { tag });
    (m, spinners, animals)
}

/// A passage as it would look on `tile` once built, for its ghost
/// (placement and while it's being built): its stonework, and for a canal
/// the water surface it will leave (the land isn't dug yet). Returns the
/// mesh and where it goes; colours are opaque (the ghost's material makes it see-through).
pub fn ghost_model(game: &Game, terrain: &Terrain, tile: TileId, p: sim::improvement::Passage) -> (Mesh, Vec3) {
    let centre = tile_center(&game.map.grid, tile);
    let base = terrain.heights.sample(centre).max(WATER_LEVEL);
    let mut b = Builder { m: Model::default(), terrain, centre, base, scale: 1.0, spinners: Vec::new(), animals: Vec::new() };
    match p {
        sim::improvement::Passage::Canal => {
            // The sea's surface over the whole tile, seen through the land.
            let hex = RegularPolygon::new(0.97, 6).mesh().build();
            b.add(hex, lin(70, 150, 190), at(0.0, WATER_LEVEL - base, 0.0).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)));
        }
        sim::improvement::Passage::Tunnel => {
            let mask = neighbors_by_direction(game, tile)
                .into_iter()
                .enumerate()
                .filter(|(_, n)| n.is_some_and(|n| game.has_road(n)))
                .fold(0u8, |m, (i, _)| m | 1 << i);
            tunnel(&mut b, mask);
        }
        sim::improvement::Passage::Bridge => {
            let span = crate::bridges::Span::new(terrain, centre, crate::bridges::ends(game, terrain, tile));
            bridge(&mut b, span, bridge_era(game, tile));
        }
        sim::improvement::Passage::Fort => b.prop("fort", Vec2::ZERO, 0.0, 1.0),
        sim::improvement::Passage::Watchtower => b.prop("watchtower", Vec2::new(0.3, -0.22), 0.4, 1.2),
    }
    b.m.map_alpha_from(0, |_| 1.0);
    (b.m.into_mesh(), Vec3::new(centre.x, base, centre.y))
}

fn build_tile_model(game: &Game, terrain: &Terrain, tile: TileId, look: Look) -> (Model, Vec<Transform>, Vec<HerdAnimal>) {
    let centre = tile_center(&game.map.grid, tile);
    let base = terrain.heights.sample(centre).max(WATER_LEVEL);
    let mut b = Builder { m: Model::default(), terrain, centre, base, scale: 1.0, spinners: Vec::new(), animals: Vec::new() };
    let mut rng = ChaCha8Rng::seed_from_u64(tile as u64 ^ 0x1A9D);
    match (look.roads, look.bridge) {
        (_, Some((ends, era))) => bridge(&mut b, crate::bridges::Span::new(terrain, centre, ends), era),
        (Some(mask), _) if look.tunnel => tunnel(&mut b, mask),
        (Some(mask), _) => road(&mut b, mask),
        (None, _) => {}
    }
    if let (Some(mask), None, false) = (look.rails, look.bridge, look.tunnel) {
        railway(&mut b, mask);
    }
    // A fort takes the middle of the tile (its improvement isn't drawn); a
    // watchtower stands to one side of it.
    if let Some((fort, [r, g, bl])) = look.post {
        let team = lin(r, g, bl);
        if fort {
            b.team_prop("fort", Vec2::ZERO, 0.0, 1.0, team);
        } else {
            b.team_prop("watchtower", Vec2::new(0.3, -0.22), 0.4, 1.2, team);
        }
    }
    if let Some((imp, resource)) = look.improvement.filter(|_| !look.post.is_some_and(|p| p.0)) {
        let art = game.rules.improvement(imp).art.as_str();
        let res = resource.map(|r| game.rules.resource(r).id.as_str());
        // Leave the road's middle free when there is one.
        let off = if look.roads.is_some() { Vec2::new(0.18, 0.12) } else { Vec2::ZERO };
        match art {
            "farm" => farm(&mut b, &mut rng, res),
            "mine" => mine(&mut b, &mut rng, off),
            "pasture" => pasture(&mut b, &mut rng, res, off),
            "camp" => camp(&mut b, &mut rng, off),
            "plantation" => plantation(&mut b, &mut rng, res),
            "quarry" => quarry(&mut b, &mut rng, off),
            "lumber_mill" => lumber_mill(&mut b, &mut rng, off),
            "fishing_boats" => fishing_boats(&mut b, &mut rng),
            "oil_well" => oil_well(&mut b, &mut rng, off),
            _ => {}
        }
    }
    if look.resource.is_some_and(|r| game.rules.resource(r).id == "wheat") {
        // Wild grain covers the whole tile; a farm turns it into fields.
        wild_wheat(&mut b, &mut rng, look.roads.is_some());
    } else if let Some(r) = look.resource {
        // In the lower left: clear of units and yield icons (the middle) and
        // the resource badge (lower right); further out beside an improvement.
        let at = if look.improvement.is_some() || look.roads.is_some() { Vec2::new(-0.38, 0.34) } else { Vec2::new(-0.28, 0.3) };
        // Luxuries stand out most.
        let def = game.rules.resource(r);
        let scale = if def.kind == ResourceKind::Luxury { 2.0 } else { 1.6 };
        resource(&mut b, &mut rng, &def.id, at, scale);
    }
    (b.m, b.spinners, b.animals)
}

/// A tile of wild grain: a golden mat over the whole hex in drifts of riper
/// and greener growth, tufts of stalks leaning with the wind, a few poppies,
/// and a ragged edge where it gives way to the meadow.
fn wild_wheat(b: &mut Builder, rng: &mut ChaCha8Rng, road: bool) {
    use std::f32::consts::TAU;
    // Ripe heads over a darker, amber mat, so the patch stands out from
    // plains and grassland alike.
    let ripe = lin(238, 190, 74);
    let pale = lin(248, 222, 136);
    let green = lin(150, 162, 70);
    let mat = lin(204, 142, 40);
    let phase = rng.gen_range(0.0..TAU);
    // Slow waves across the tile: where the grain is riper (1) or greener (-1).
    let drift = |p: Vec2| ((p.x * 3.1 + phase).sin() + (p.y * 3.7 - phase * 0.7).sin() + (p.dot(Vec2::new(2.3, -1.9)) + phase).sin()) / 3.0;
    let tint = |d: f32| if d > 0.0 { ripe.lerp(pale, d.min(1.0)) } else { ripe.lerp(green, (-d * 1.3).min(1.0)) };
    let ok = |b: &Builder, p: Vec2| {
        !(road && p.length() < 0.12) && b.terrain.heights.sample(b.centre + p) >= WATER_LEVEL + 0.02 && !b.in_channel(p)
    };
    // The ground's tilt at `p`, so flat pieces lie on it.
    let tilt = |b: &Builder, p: Vec2| {
        let e = 0.04;
        let n = Vec3::new(b.ground(p - Vec2::X * e) - b.ground(p + Vec2::X * e), 2.0 * e, b.ground(p - Vec2::Y * e) - b.ground(p + Vec2::Y * e));
        Quat::from_rotation_arc(Vec3::Y, n.normalize())
    };
    // Points on a jittered triangular lattice inside the hex.
    let lattice = |rng: &mut ChaCha8Rng, step: f32| {
        let mut out = Vec::new();
        let rows = (0.95 / (step * 0.866)) as i32;
        let cols = (0.95 / step) as i32 + 1;
        for row in -rows..=rows {
            for col in -cols..=cols {
                let x = (col as f32 + if row % 2 == 0 { 0.0 } else { 0.5 }) * step;
                let jitter = Vec2::new(rng.gen_range(-0.35..0.35), rng.gen_range(-0.35..0.35)) * step;
                out.push(Vec2::new(x, row as f32 * step * 0.866) + jitter);
            }
        }
        out
    };

    // The mat: overlapping low discs, thinning out raggedly near the edge.
    for p in lattice(rng, 0.1) {
        let edge = hex_edge_distance(p);
        if rng.gen_range(0.6..0.84) < edge || !ok(b, p) {
            continue;
        }
        let color = mat.lerp(tint(drift(p)), 0.35) * rng.gen_range(0.85..1.0);
        let r = rng.gen_range(0.028..0.034);
        let spin = Quat::from_rotation_y(rng.gen_range(0.0..TAU));
        b.add(cylinder(r, 0.004, 9), color, b.on(p, 0.003).with_rotation(tilt(b, p) * spin));
    }
    // Tufts of stalks on it, all leaning a little the same way.
    let wind = rng.gen_range(0.0..TAU);
    let lean_axis = Vec3::new(wind.cos(), 0.0, wind.sin());
    for p in lattice(rng, 0.045) {
        let edge = hex_edge_distance(p);
        if rng.gen_range(0.62..0.9) < edge || !ok(b, p) {
            continue;
        }
        let d = drift(p);
        if d < -0.6 && rng.gen_bool(0.5) {
            continue;
        }
        let base = tint(d + 0.25);
        for _ in 0..rng.gen_range(3..6) {
            let q = p + Vec2::new(rng.gen_range(-0.014..0.014), rng.gen_range(-0.014..0.014));
            let h = rng.gen_range(0.026..0.04);
            let lean = Quat::from_axis_angle(lean_axis, rng.gen_range(0.15..0.35));
            let mut t = b.on(q, h * 0.5 + 0.002).with_rotation(lean);
            t.translation += lean * Vec3::Y * h * 0.5 * K - Vec3::Y * h * 0.5 * K;
            b.add(cone(0.0035, h, 3), base * rng.gen_range(0.95..1.15), t);
        }
        if rng.gen_bool(0.01) {
            b.add(blob(0.005, 0), lin(206, 50, 36), b.on(p, 0.03));
        }
    }
}

/// How far `p` is from a tile's centre towards its nearest edge (the edges
/// are at 0.866; tiles are pointy-topped, with neighbours to the east and west).
fn hex_edge_distance(p: Vec2) -> f32 {
    let a = Vec2::new(0.5, 0.866).dot(p).abs();
    let b = Vec2::new(-0.5, 0.866).dot(p).abs();
    a.max(b).max(p.x.abs())
}

/// Edge midpoints in the order of `sim::hex::DIRECTIONS`.
fn edge_point(i: usize) -> Vec2 {
    let d = sim::hex::DIRECTIONS[i];
    crate::layout::hex_center(sim::hex::Hex::new(d.q, d.r)) * 0.5
}

fn road(b: &mut Builder, mask: u8) {
    let dirt = lin(126, 98, 68);
    let edge = lin(104, 84, 60);
    let ends: Vec<Vec2> = (0..6).filter(|i| mask & (1 << i) != 0).map(edge_point).collect();
    if ends.is_empty() {
        b.strip(Vec2::new(-0.25, 0.0), Vec2::new(0.25, 0.0), 0.11, 0.006, dirt);
        return;
    }
    for &e in &ends {
        b.strip(Vec2::ZERO, e, 0.16, 0.003, edge);
        b.strip(Vec2::ZERO, e, 0.11, 0.006, dirt);
    }
    // A worn patch where roads meet.
    b.add(cylinder(0.085, 0.004, 8), dirt, at(0.0, b.ground(Vec2::ZERO) + 0.006, 0.0));
}

/// A railway along the road's branches: a bed of grey ballast, timber
/// sleepers and two steel rails.
fn railway(b: &mut Builder, mask: u8) {
    let ballast = lin(118, 112, 104);
    let sleeper = lin(70, 50, 36);
    let steel = lin(150, 154, 162);
    let ends: Vec<Vec2> = (0..6).filter(|i| mask & (1 << i) != 0).map(edge_point).collect();
    let ends = if ends.is_empty() { vec![Vec2::new(0.3, 0.0), Vec2::new(-0.3, 0.0)] } else { ends };
    for &e in &ends {
        b.strip(Vec2::ZERO, e, 0.09, 0.009, ballast);
        let dir = e.normalize_or_zero();
        let across = Vec2::new(-dir.y, dir.x);
        let n = (e.length() / 0.055) as i32;
        for k in 0..=n {
            let p = dir * (k as f32 * 0.055);
            let yaw = dir.x.atan2(dir.y);
            b.add(Cuboid::new(0.075, 0.006, 0.014).mesh().build(), sleeper, at(p.x, b.ground(p) + 0.012, p.y).with_rotation(Quat::from_rotation_y(yaw)));
        }
        for s in [-1.0f32, 1.0] {
            let o = across * 0.022 * s;
            b.strip(o, e + o, 0.007, 0.017, steel);
        }
    }
}

/// Stone portals where the road goes into the mountain and comes out again,
/// low on its flanks: towards neighbouring roads, or else along the way
/// through that climbs least. Each portal leans back into the slope, so its
/// dark arched mouth faces up towards the camera.
fn tunnel(b: &mut Builder, mask: u8) {
    let mut ends: Vec<usize> = (0..6).filter(|i| mask & (1 << i) != 0).collect();
    if ends.len() < 2 {
        // Out the far side from the one road, or along the lowest line.
        let low = |i: usize| b.ground(edge_point(i)) + b.ground(edge_point((i + 3) % 6));
        match ends.first() {
            Some(&i) => ends.push((i + 3) % 6),
            None => ends = (0..3).min_by(|&x, &y| low(x).total_cmp(&low(y))).map(|i| vec![i, i + 3]).unwrap_or_default(),
        }
    }
    let stone = lin(122, 112, 98).with_w(WATER_MARK);
    let trim = lin(92, 84, 74).with_w(WATER_MARK);
    let mouth = lin(10, 9, 8).with_w(WATER_MARK);
    let dirt = lin(126, 98, 68);
    for i in ends {
        let edge = edge_point(i);
        let out = edge.normalize();
        // Down the slope until the ground levels off a little.
        let p = [0.45f32, 0.55, 0.65, 0.75].into_iter().map(|r| out * r).find(|&p| b.ground(p) - b.ground(edge) < 0.12).unwrap_or(out * 0.75);
        b.strip(p + out * 0.04, edge, 0.11, 0.006, dirt);
        // How steeply the mountain rises behind the portal.
        let rise = ((b.ground(p - out * 0.08) - b.ground(p + out * 0.08)) / 0.16).atan().clamp(0.0, 1.2);
        let turn = Quat::from_rotation_y(out.x.atan2(out.y)) * Quat::from_rotation_x(-rise * 0.8);
        let k = b.k();
        let base = b.ground(p);
        // Local (x across, y up, z out) to the tile's coordinates.
        let place = |x: f32, y: f32, z: f32| {
            let v = turn * Vec3::new(x, y, z) * k;
            at(p.x + v.x, base + v.y, p.y + v.z)
        };
        let part = |b: &mut Builder, mesh: Mesh, color: Vec4, x: f32, y: f32, z: f32| {
            let t = place(x, y, z).with_rotation(turn).with_scale(Vec3::splat(k));
            b.add(mesh, color, t);
        };
        // The facade, half sunk into the hill, and wing walls holding the slope back.
        part(b, Cuboid::new(0.1, 0.075, 0.05).mesh().build(), stone, 0.0, 0.03, -0.02);
        for x in [-1.0f32, 1.0] {
            let wing = Cuboid::new(0.012, 0.04, 0.06).mesh().build();
            let t = place(x * 0.052, 0.015, 0.01).with_rotation(turn * Quat::from_rotation_y(-x * 0.5)).with_scale(Vec3::splat(k));
            b.add(wing, trim, t);
        }
        // The mouth: a doorway under a round arch, ringed by voussoirs and a keystone.
        part(b, cylinder(0.026, 0.012, 14), trim, 0.0, 0.034, 0.006);
        part(b, Cuboid::new(0.052, 0.034, 0.012).mesh().build(), trim, 0.0, 0.017, 0.006);
        let arch = |b: &mut Builder, r: f32, w: f32, z: f32, color: Vec4| {
            let t = place(0.0, 0.034, z).with_rotation(turn * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)).with_scale(Vec3::splat(k));
            b.add(cylinder(r, w, 14), color, t);
        };
        arch(b, 0.02, 0.014, 0.008, mouth);
        part(b, Cuboid::new(0.04, 0.034, 0.014).mesh().build(), mouth, 0.0, 0.017, 0.008);
        part(b, Cuboid::new(0.014, 0.014, 0.016).mesh().build(), stone, 0.0, 0.06, 0.006);
        part(b, Cuboid::new(0.11, 0.01, 0.06).mesh().build(), trim, 0.0, 0.07, -0.022);
    }
}

/// A stone bridge along `span` in an era's style (`art/models/bridges.py`):
/// an arm from the middle pier to each shore (stretched lengthwise to reach
/// the drawn shore) and the pier itself, with the era's showpiece on its
/// outer side.
fn bridge(b: &mut Builder, span: crate::bridges::Span, era: sim::rules::EraId) {
    let style = match era {
        0 | 1 => "classical",
        2 => "medieval",
        _ => "renaissance",
    };
    let y = WATER_LEVEL - b.base;
    for end in span.ends {
        let yaw = Quat::from_rotation_y(-end.y.atan2(end.x));
        let stretch = Vec3::new(end.length() / crate::bridges::ARM, 1.0, 1.0);
        b.m.append(crate::models::prop(&format!("bridge_{style}_arm")), &at(0.0, y, 0.0).with_rotation(yaw).with_scale(stretch), Vec4::ONE);
    }
    // The hub's showpiece is on its +Y side in Blender, -Z here.
    let out = span.outside();
    let yaw = Quat::from_rotation_y((-out.x).atan2(-out.y));
    b.m.append(crate::models::prop(&format!("bridge_{style}_hub")), &at(0.0, y, 0.0).with_rotation(yaw), Vec4::ONE);
}

fn farm(b: &mut Builder, rng: &mut ChaCha8Rng, res: Option<&str>) {
    use std::f32::consts::FRAC_PI_3;
    let wheat = [lin(218, 184, 82), lin(204, 166, 66)];
    let green = [lin(112, 150, 58), lin(140, 170, 72)];
    let soil = lin(116, 88, 60);
    let hedge = lin(62, 96, 46);
    // A hexagon of fields matching the tile's own (pointy-top: corners at
    // 30° + k·60°), cut into three rhombi by hedges from the middle to every
    // other corner, each rhombus halved into two plots.
    let r = 0.82;
    let turn = rng.gen_range(0..2) as f32 * FRAC_PI_3;
    let dir = |a: f32| Vec2::new(a.cos(), a.sin());
    let corner = |k: usize| dir(FRAC_PI_3 * 0.5 + k as f32 * FRAC_PI_3 + turn) * r;
    let start = rng.gen_range(0..3);
    for i in 0..3 {
        let (u, v) = (corner(2 * i), corner(2 * i + 2));
        // Rows along the rhombus's spoke edge, or along its outer edge.
        let along_spoke = rng.gen_bool(0.5);
        for half in 0..2 {
            let k = 2 * i + half;
            let kind = if res == Some("wheat") { k % 2 } else { (k + start) % 3 };
            // The plot: a parallelogram from `o`, spanned by `u` and half of `v`.
            let o = v * 0.5 * half as f32;
            let (along, across) = if along_spoke { (u, v * 0.5) } else { (v * 0.5, u) };
            // Rows from `o + f·across` along `along`, kept a little inside.
            let width = across.length() * 0.866;
            let row = |f: f32, inset: f32| {
                let s = o + across * f;
                (s + along * inset, s + along * (1.0 - inset))
            };
            let n = (width / 0.08).ceil() as usize;
            for j in 0..n {
                let (a, e) = row((j as f32 + 0.5) / n as f32, 0.03);
                b.strip(a, e, width / n as f32 * 1.1, 0.002, soil);
            }
            // The crop leaves a margin of bare soil round the plot.
            let n = (width / 0.07).round().max(2.0) as usize;
            let (w, lift, color): (f32, f32, [Vec4; 2]) = match kind {
                0 => (0.045, 0.007, wheat),
                1 => (0.045, 0.011, green),
                _ => (0.02, 0.004, [soil * 0.8; 2]),
            };
            for j in 0..n {
                let (a, e) = row(0.1 + 0.8 * (j as f32 + 0.5) / n as f32, 0.1);
                b.strip(a, e, w, lift, color[j % 2]);
            }
        }
    }
    // Hedges along the spokes and round the edge.
    for i in 0..3 {
        b.strip(Vec2::ZERO, corner(2 * i), 0.045, 0.022, hedge);
    }
    for k in 0..6 {
        b.strip(corner(k), corner(k + 1), 0.045, 0.022, hedge);
    }
    // On the flattest of a few spots on the hedges, its sails turned
    // roughly towards the viewer so they read from above.
    let slope = |p: Vec2| (b.ground(p + Vec2::X * 0.05) - b.ground(p - Vec2::X * 0.05)).abs() + (b.ground(p + Vec2::Y * 0.05) - b.ground(p - Vec2::Y * 0.05)).abs();
    let spots = [Vec2::ZERO, corner(0) * 0.4, corner(2) * 0.4, corner(4) * 0.4];
    let cost = |p: Vec2| slope(p) + if b.in_channel(p) { 1.0 } else { 0.0 };
    let spot = spots.into_iter().min_by(|a, c| cost(*a).total_cmp(&cost(*c))).expect("spots");
    windmill(b, spot, rng.gen_range(-0.5..0.5));
}

/// A tower mill, its sails facing `yaw` and turning (they are a separate
/// model, see `Spinner`).
fn windmill(b: &mut Builder, p: Vec2, yaw: f32) {
    b.prop("windmill", p, yaw, 1.0);
    // The hub, in the windmill's model space (see works.py).
    let base = b.on(p, 0.0).with_rotation(Quat::from_rotation_y(yaw));
    let hub = base.transform_point(Vec3::new(0.0, 10.0, 2.6) * 0.0105);
    b.spinners.push(Transform::from_translation(hub).with_rotation(base.rotation).with_scale(base.scale));
}

fn mine(b: &mut Builder, rng: &mut ChaCha8Rng, off: Vec2) {
    let p = off + Vec2::new(rng.gen_range(-0.08..0.08), rng.gen_range(-0.05..0.05));
    // The adit in its bank of rock, rails out of it, a cart, a spoil heap
    // and a winch shed.
    b.prop("mine_adit", p + Vec2::new(0.0, -0.04), 0.0, 1.0);
    for x in [-0.025f32, 0.025] {
        b.strip(p + Vec2::new(x, 0.08), p + Vec2::new(x, 0.42), 0.012, 0.012, lin(80, 76, 72));
    }
    b.prop("mine_cart", p + Vec2::new(0.0, 0.3), 0.0, 1.0);
    b.prop("spoil_heap", p + Vec2::new(0.3, 0.22), rng.gen_range(0.0..std::f32::consts::TAU), 1.0);
    b.prop("winch_shed", p + Vec2::new(-0.3, 0.2), 0.3, 1.0);
}

/// An animal standing at `p`, facing `yaw`.
/// An animal standing at `p`: one that grazes and wanders (see
/// `ambience::herds`) when it has a rigged model, else a still prop.
fn animal(b: &mut Builder, name: &str, p: Vec2, yaw: f32, size: f32) {
    let (scale, s) = (b.scale, size * b.scale);
    if let Some(model) = crate::ambience::herd_model(name) {
        b.animals.push(HerdAnimal { model, home: p, yaw, size: K * s, centre: b.centre, base: b.base });
        return;
    }
    b.scale = 1.0;
    b.prop(name, p, yaw, s);
    b.scale = scale;
}

fn pasture(b: &mut Builder, rng: &mut ChaCha8Rng, res: Option<&str>, off: Vec2) {
    let wood = lin(132, 98, 62);
    // A fence around the paddock, posts and two rails.
    let r = 0.55;
    let n = 9;
    for i in 0..n {
        let a0 = i as f32 / n as f32 * std::f32::consts::TAU + 0.2;
        let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU + 0.2;
        let (p0, p1) = (Vec2::new(a0.cos(), a0.sin()) * r, Vec2::new(a1.cos(), a1.sin()) * r);
        b.add(Cuboid::new(0.007, 0.035, 0.007).mesh().build(), wood, b.on(p0, 0.017));
        let mid = (p0 + p1) * 0.5;
        let yaw = -(p1 - p0).y.atan2((p1 - p0).x);
        let len = p0.distance(p1) / K;
        for y in [0.016f32, 0.03] {
            b.add(Cuboid::new(len, 0.004, 0.004).mesh().build(), wood, b.on(mid, y).with_rotation(Quat::from_rotation_y(yaw)));
        }
    }
    let (kinds, size, count): (&[&str], f32, usize) = match res {
        Some("horses") => (&["horse"], 1.0, 3),
        Some("sheep") => (&["sheep"], 1.0, 5),
        _ => (&["cow", "cow_pied"], 1.0, 3),
    };
    for i in 0..count {
        let a = i as f32 / count as f32 * std::f32::consts::TAU + rng.gen_range(0.0..1.0);
        let p = off * 0.3 + Vec2::new(a.cos(), a.sin()) * rng.gen_range(0.12..0.32);
        animal(b, kinds[i % kinds.len()], p, rng.gen_range(0.0..std::f32::consts::TAU), size);
    }
}

fn camp(b: &mut Builder, rng: &mut ChaCha8Rng, off: Vec2) {
    for p in [Vec2::new(-0.16, -0.1), Vec2::new(0.16, -0.14)] {
        b.prop("tent", p + off, rng.gen_range(-0.6..0.6), 1.0);
    }
    b.prop("campfire", off + Vec2::new(0.0, 0.14), 0.0, 1.0);
    b.prop("drying_rack", off + Vec2::new(0.28, 0.16), rng.gen_range(-0.4..0.4), 1.0);
}

/// A sawmill shed in a clearing, a stack of logs and fresh stumps (the forest stays).
fn lumber_mill(b: &mut Builder, rng: &mut ChaCha8Rng, off: Vec2) {
    b.prop("sawmill", off + Vec2::new(-0.06, -0.04), rng.gen_range(-0.4..0.4), 1.0);
    b.prop("log_pile", off + Vec2::new(0.16, 0.12), rng.gen_range(0.0..1.0), 1.0);
    for p in [Vec2::new(-0.2, 0.14), Vec2::new(0.04, 0.2), Vec2::new(-0.18, -0.16)] {
        b.prop("stump", off + p, rng.gen_range(0.0..std::f32::consts::TAU), 1.0);
    }
}

/// A fishing ground worked from boats: a moored skiff, a weir of stakes and
/// a line of net floats, around the fish or pearl bed in the lower left.
fn fishing_boats(b: &mut Builder, rng: &mut ChaCha8Rng) {
    use std::f32::consts::TAU;
    b.prop("skiff", Vec2::new(0.24, -0.12), rng.gen_range(0.0..TAU), 1.0);
    b.prop("fish_weir", Vec2::new(-0.3, 0.3), rng.gen_range(0.0..TAU), 1.0);
    b.prop("net_floats", Vec2::new(0.12, 0.3), rng.gen_range(-0.5..0.5), 1.0);
}

fn plantation(b: &mut Builder, rng: &mut ChaCha8Rng, res: Option<&str>) {
    let crop = match res {
        Some("wine") => "vine",
        Some("silk") => "mulberry",
        Some("spices") => "spice_bush",
        Some("incense") => "incense_tree",
        Some("bananas") => "banana",
        _ => "orchard_tree",
    };
    let yaw = rng.gen_range(0.0..std::f32::consts::PI);
    let (c, s) = (yaw.cos(), yaw.sin());
    let rot = |p: Vec2| Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
    for row in 0..4 {
        b.strip(rot(Vec2::new(-0.45, -0.27 + row as f32 * 0.18)), rot(Vec2::new(0.45, -0.27 + row as f32 * 0.18)), 0.09, 0.002, lin(112, 86, 58));
        for col in 0..5 {
            let p = rot(Vec2::new(-0.36 + col as f32 * 0.18, -0.27 + row as f32 * 0.18) + Vec2::new(0.0, (col % 2) as f32 * 0.03));
            if p.length() > 0.62 {
                continue;
            }
            b.prop(crop, p, rng.gen_range(0.0..std::f32::consts::TAU), rng.gen_range(0.85..1.15));
        }
    }
}

fn quarry(b: &mut Builder, rng: &mut ChaCha8Rng, off: Vec2) {
    b.prop("quarry_cut", off + Vec2::new(-0.08, -0.04), rng.gen_range(-0.3..0.3), 1.0);
    for i in 0..5 {
        let q = off + Vec2::new(0.22 + (i % 3) as f32 * 0.07, 0.2 + (i / 3) as f32 * 0.07);
        b.prop("stone_block", q, rng.gen_range(-0.3..0.3), 1.0);
    }
    b.prop("crane", off + Vec2::new(-0.3, 0.22), rng.gen_range(0.0..std::f32::consts::TAU), 1.0);
}

/// A timber derrick over the well, a pumpjack nodding beside it, tanks and
/// barrels.
fn oil_well(b: &mut Builder, rng: &mut ChaCha8Rng, off: Vec2) {
    let p = off + Vec2::new(rng.gen_range(-0.06..0.06), rng.gen_range(-0.04..0.04));
    b.prop("derrick", p + Vec2::new(-0.12, -0.06), rng.gen_range(-0.3..0.3), 1.0);
    b.prop("pumpjack", p + Vec2::new(0.2, 0.12), rng.gen_range(-0.4..0.4), 1.0);
    b.prop("oil_tanks", p + Vec2::new(-0.28, 0.26), rng.gen_range(0.0..std::f32::consts::TAU), 1.0);
    b.prop("oil_tanks", p + Vec2::new(-0.08, 0.36), rng.gen_range(0.0..std::f32::consts::TAU), 0.8);
}

/// A resource as it is found: animals, wild crops, rock or fish around `at`.
fn resource(b: &mut Builder, rng: &mut ChaCha8Rng, id: &str, at: Vec2, scale: f32) {
    use std::f32::consts::TAU;
    b.scale = scale;
    let near = |rng: &mut ChaCha8Rng, i: usize, n: usize, r: f32| {
        let a = i as f32 / n as f32 * TAU + rng.gen_range(0.0..0.6);
        at + Vec2::new(a.cos(), a.sin()) * r * scale * rng.gen_range(0.7..1.0)
    };
    let herd = |b: &mut Builder, rng: &mut ChaCha8Rng, kinds: &[&str], n: usize, size: f32| {
        for i in 0..n {
            let p = near(rng, i, n, 0.12);
            animal(b, kinds[i % kinds.len()], p, rng.gen_range(0.0..TAU), size);
        }
    };
    match id {
        "cattle" => herd(b, rng, &["cow", "cow_pied"], 2, 1.0),
        "sheep" => herd(b, rng, &["sheep"], 3, 1.0),
        "horses" => herd(b, rng, &["horse"], 2, 1.0),
        "deer" => herd(b, rng, &["deer"], 2, 1.0),
        "furs" => herd(b, rng, &["fox"], 3, 1.0),
        "ivory" => herd(b, rng, &["elephant"], 1, 1.0),
        "wheat" => {
            // A small field of ripe grain: rows on bare soil, a few sheaves at its edge.
            let (half_x, half_y) = (0.2 * scale, 0.14 * scale);
            for r in 0..5 {
                let y = -half_y + r as f32 * half_y * 0.5;
                b.strip(at + Vec2::new(-half_x, y), at + Vec2::new(half_x, y), 0.07 * scale, 0.002, lin(122, 92, 60));
            }
            let grain = [lin(232, 196, 86), lin(214, 172, 64)];
            for r in 0..9 {
                let y = -half_y + r as f32 * half_y * 0.25;
                b.strip(at + Vec2::new(-half_x * 0.95, y), at + Vec2::new(half_x * 0.95, y), 0.026 * scale, 0.014 * scale, grain[r % 2]);
            }
            for i in 0..3 {
                b.prop("stook", at + Vec2::new(half_x + 0.05 * scale, -half_y + i as f32 * half_y), rng.gen_range(0.0..TAU), 1.0);
            }
        }
        "bananas" | "silk" | "incense" | "wine" | "spices" => {
            let (crop, n) = match id {
                "bananas" => ("banana", 3),
                "silk" => ("mulberry", 2),
                "incense" => ("incense_tree", 2),
                "wine" => ("vine", 4),
                _ => ("spice_bush", 4),
            };
            for i in 0..n {
                b.prop(crop, near(rng, i, n, 0.13), rng.gen_range(0.0..TAU), 1.0);
            }
        }
        "stone" => b.prop("rocks", at, rng.gen_range(0.0..TAU), 1.0),
        "iron" => b.prop("iron_rocks", at, rng.gen_range(0.0..TAU), 1.0),
        "gold" => b.prop("gold_rocks", at, rng.gen_range(0.0..TAU), 1.0),
        "marble" => b.prop("marble_blocks", at, rng.gen_range(0.0..TAU), 1.0),
        "gems" => b.prop("gems", at, rng.gen_range(0.0..TAU), 1.0),
        "salt" => b.prop("salt", at, rng.gen_range(0.0..TAU), 1.0),
        "oil" => b.prop("oil_seep", at, rng.gen_range(0.0..TAU), 1.0),
        "fish" => {
            for i in 0..3 {
                b.prop("fish", near(rng, i, 3, 0.16), rng.gen_range(0.0..TAU), 1.0);
            }
        }
        "pearls" => {
            for i in 0..3 {
                b.prop("clam", near(rng, i, 3, 0.12), rng.gen_range(0.0..TAU), 1.0);
            }
        }
        _ => {}
    }
    b.scale = 1.0;
}

// --- Resource badges ---------------------------------------------------------

/// A resource's icon badge on the map.
#[derive(Component)]
struct ResourceBadge {
    tile: TileId,
}


fn badge_ring(kind: ResourceKind) -> Color {
    match kind {
        ResourceKind::Bonus => Color::srgb(0.46, 0.66, 0.36),
        ResourceKind::Luxury => Color::srgb(0.78, 0.5, 0.86),
        ResourceKind::Strategic => Color::srgb(0.86, 0.38, 0.3),
    }
}

fn sync_badges(
    mut commands: Commands,
    session: Res<Session>,
    icons: Res<Icons>,
    mut events: MessageReader<GameEvent>,
    old: Query<Entity, With<ResourceBadge>>,
    labels: Single<Entity, With<ui::WorldLabels>>,
) {
    let relevant = events.read().any(|GameEvent(e)| {
        matches!(e, Event::TileImproved { .. } | Event::TerrainChanged { .. } | Event::Explored { .. } | Event::TechResearched { .. } | Event::BordersChanged { .. })
    });
    if !relevant && !session.is_added() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    let game = &session.game;
    let me = session.me;
    let explored = &game.players[me as usize].explored;
    for t in 0..game.map.tiles.len() as TileId {
        if !explored[t as usize] || game.city_at(t).is_some() {
            continue;
        }
        let Some(r) = game.visible_resource(me, t) else { continue };
        let def = game.rules.resource(r);
        let worked = game.rules.resource_improvement(r).is_none_or(|i| game.improvement_at(t) == Some(i));
        let ring = badge_ring(def.kind);
        commands.spawn((
            ResourceBadge { tile: t },
            ChildOf(*labels),
            Node {
                position_type: PositionType::Absolute,
                width: px(34),
                height: px(34),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            UiTransform::from_translation(Val2::percent(-50.0, -50.0)),
            BackgroundColor(if worked { Color::srgba(0.12, 0.1, 0.08, 0.92) } else { Color::srgba(0.12, 0.1, 0.08, 0.7) }),
            BorderColor::all(if worked { ring } else { ring.with_alpha(0.6) }),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.45), px(0), px(1), px(0), px(4)),
            Visibility::Hidden,
            GlobalZIndex(-2),
            Tooltip::new(def.name.clone(), format!("{}. {}", resource_line(game, me, t, r), resource_use(game, r))),
            children![ui::icon(icons.get(Icon::resource(&def.id)), 26.0)],
        ));
    }
}

fn place_badges(
    session: Res<Session>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    ui_scale: Res<UiScale>,
    rig: Res<CameraRig>,
    strategic: Res<crate::strategic::StrategicView>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    mut badges: Query<(&ResourceBadge, &mut Node, &mut Visibility, &mut UiTransform)>,
) {
    let (cam, cam_tf) = *camera;
    // Badges grow as the camera comes closer, so they keep their size next
    // to the tiles (at the usual distance, 22, they're drawn as made).
    let zoom = (22.0 / rig.distance).clamp(0.6, 2.2);
    let screen = crate::ui::ui_screen(cam, ui_scale.0);
    for (badge, mut node, mut vis, mut transform) in &mut badges {
        if transform.scale != Vec2::splat(zoom) {
            transform.scale = Vec2::splat(zoom);
        }
        let grid = &session.game.map.grid;
        // In the middle of the tile, just below its yield icons.
        let p = tile_center(grid, badge.tile) + Vec2::new(0.0, 0.36);
        let mut world = Vec3::new(p.x, terrain.heights.sample(p).max(WATER_LEVEL) + 0.15, p.y);
        if strategic.0 {
            world.y = crate::strategic::ground(&terrain, p);
        }
        world.x += nearest_shift(world.x, cam_tf.translation().x, period.0);
        let at = cam.world_to_viewport(cam_tf, world).ok().map(|s| s / ui_scale.0);
        crate::ui::place_label(&mut node, &mut vis, at, screen, 60.0);
    }
}
