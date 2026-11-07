//! Traffic: ox carts carrying goods along the roads between a civ's towns,
//! in its colours. One cart at a time on each road route in view, setting
//! out from either end now and then; carts are only seen where the player
//! sees.

use super::herds::Actor;
use super::{AmbienceSet, Dice, Stage};
use crate::layout::tile_center;
use crate::loading::InGameSet;
use crate::play::Session;
use crate::procmesh::lin;
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::wrap::{nearest_shift, WrapPeriod};
use bevy::prelude::*;
use rand::Rng;
use sim::game::Game;
use sim::hex::TileId;
use std::collections::{HashMap, VecDeque};

/// Longest road route between two towns, in tiles.
const MAX_ROUTE: usize = 14;
/// World units a second.
const SPEED: f32 = 0.22;
/// About a third of a tile from the ox's nose to the cart's tail.
const CART_SIZE: f32 = 0.45;

pub fn plugin(app: &mut App) {
    app.init_resource::<Routes>().add_systems(Update, (find_routes, dispatch, drive).chain().in_set(InGameSet).in_set(AmbienceSet::Emit));
}

struct Route {
    owner: sim::game::PlayerId,
    /// Tile centres along the road (unwrapped: x continues across the seam).
    points: Vec<Vec2>,
    min: Vec2,
    max: Vec2,
    /// Seconds before the next cart may set out.
    wait: f32,
    busy: bool,
}

#[derive(Resource, Default)]
struct Routes {
    list: Vec<Route>,
    /// What they were found for: roads and towns.
    key: (usize, usize),
}

/// The road routes between each civ's towns (shortest, avoiding bridges
/// and tunnels, which carts would cross badly).
fn find_routes(session: Res<Session>, period: Res<WrapPeriod>, mut routes: ResMut<Routes>) {
    if !session.is_changed() {
        return;
    }
    let game = &session.game;
    let roads = game.map.tiles.len() as TileId;
    let key = ((0..roads).filter(|t| game.has_road(*t)).count(), game.cities.len());
    if key == routes.key && !session.is_added() {
        return;
    }
    routes.key = key;
    routes.list.clear();
    let mut seen_pairs = std::collections::HashSet::new();
    for c in game.cities.values() {
        for (to, tiles) in road_paths(game, c.tile, c.owner) {
            let pair = (c.tile.min(to), c.tile.max(to));
            if !seen_pairs.insert(pair) {
                continue;
            }
            let mut points: Vec<Vec2> = Vec::with_capacity(tiles.len());
            for &t in &tiles {
                let mut p = tile_center(&game.map.grid, t);
                if let Some(prev) = points.last() {
                    p.x += nearest_shift(p.x, prev.x, period.0);
                }
                points.push(p);
            }
            let points = smooth_path(&points);
            let min = points.iter().copied().reduce(Vec2::min).unwrap_or_default();
            let max = points.iter().copied().reduce(Vec2::max).unwrap_or_default();
            routes.list.push(Route { owner: c.owner, points, min, max, wait: 0.0, busy: false });
        }
    }
}

/// Paths along roads from `from` to the owner's other towns.
fn road_paths(game: &Game, from: TileId, owner: sim::game::PlayerId) -> Vec<(TileId, Vec<TileId>)> {
    let mut prev: HashMap<TileId, TileId> = HashMap::new();
    let mut queue = VecDeque::from([(from, 0usize)]);
    prev.insert(from, from);
    let mut out = Vec::new();
    while let Some((t, d)) = queue.pop_front() {
        if t != from && game.city_at(t).is_some_and(|c| c.owner == owner) {
            let mut path = vec![t];
            let mut at = t;
            while at != from {
                at = prev[&at];
                path.push(at);
            }
            path.reverse();
            out.push((t, path));
            continue;
        }
        if d >= MAX_ROUTE {
            continue;
        }
        for n in game.map.grid.neighbors(t) {
            if prev.contains_key(&n) || !game.has_road(n) || game.has_bridge(n) || game.has_tunnel(n) {
                continue;
            }
            prev.insert(n, t);
            queue.push_back((n, d + 1));
        }
    }
    out
}

/// Rounds the corners of a path (two passes of corner cutting).
fn smooth_path(points: &[Vec2]) -> Vec<Vec2> {
    let mut p = points.to_vec();
    for _ in 0..2 {
        if p.len() < 3 {
            break;
        }
        let mut next = vec![p[0]];
        for w in p.windows(2) {
            next.push(w[0].lerp(w[1], 0.25));
            next.push(w[0].lerp(w[1], 0.75));
        }
        next.push(*p.last().unwrap());
        p = next;
    }
    p
}

/// A cart on its way along route `route`.
#[derive(Component)]
struct Cart {
    route: usize,
    /// Distance along it, and which way.
    along: f32,
    back: bool,
    length: f32,
}

fn dispatch(mut commands: Commands, stage: Res<Stage>, mut dice: ResMut<Dice>, session: Res<Session>, period: Res<WrapPeriod>, mut routes: ResMut<Routes>, carts: Query<(Entity, &Cart)>) {
    let rng = &mut dice.0;
    if !stage.on || stage.distance > 50.0 {
        for (e, _) in &carts {
            commands.entity(e).despawn();
        }
        for r in &mut routes.list {
            r.busy = false;
        }
        return;
    }
    // Carts travel by day.
    let daytime = stage.night < 0.3;
    for (i, r) in routes.list.iter_mut().enumerate() {
        r.wait -= stage.dt;
        let shift = Vec2::X * nearest_shift((r.min.x + r.max.x) * 0.5, stage.focus.x, period.0);
        let in_view = stage.sees(r.min + shift, 2.0) || stage.sees(r.max + shift, 2.0) || stage.sees((r.min + r.max) * 0.5 + shift, 2.0);
        if r.busy || r.wait > 0.0 || !daytime || !in_view {
            continue;
        }
        r.busy = true;
        r.wait = rng.gen_range(8.0..30.0);
        let length = r.points.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>();
        let color = session.game.players[r.owner as usize].color;
        commands.spawn((
            Cart { route: i, along: 0.0, back: rng.gen_bool(0.5), length },
            Actor::new("oxcart", CART_SIZE, lin(color[0], color[1], color[2]), rng.gen_range(0.0..30.0)),
            Transform::from_scale(Vec3::splat(CART_SIZE)),
            Visibility::Hidden,
        ));
    }
}

/// The point `d` along a path, and its direction.
fn along(points: &[Vec2], mut d: f32) -> (Vec2, Vec2) {
    for w in points.windows(2) {
        let len = w[0].distance(w[1]);
        if d <= len || len == 0.0 {
            let dir = (w[1] - w[0]).normalize_or(Vec2::Y);
            return (w[0] + dir * d.min(len), dir);
        }
        d -= len;
    }
    let n = points.len();
    (points[n - 1], (points[n - 1] - points[n.saturating_sub(2)]).normalize_or(Vec2::Y))
}

fn drive(
    mut commands: Commands,
    time: Res<Time>,
    stage: Res<Stage>,
    session: Res<Session>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    camera: Single<&GlobalTransform, With<crate::camera::MainCamera>>,
    mut routes: ResMut<Routes>,
    mut carts: Query<(Entity, &mut Cart, &mut Actor, &mut Transform, &mut Visibility)>,
) {
    let t = time.elapsed_secs();
    let me = &session.game.players[session.me as usize];
    for (e, mut cart, mut actor, mut tf, mut vis) in &mut carts {
        let Some(route) = routes.list.get_mut(cart.route) else {
            commands.entity(e).despawn();
            continue;
        };
        cart.along += SPEED * stage.dt;
        if cart.along >= cart.length {
            route.busy = false;
            commands.entity(e).despawn();
            continue;
        }
        let d = if cart.back { cart.length - cart.along } else { cart.along };
        let (p, mut dir) = along(&route.points, d);
        if cart.back {
            dir = -dir;
        }
        // Keep to the right of the road's middle.
        let p = p + dir.perp() * -0.035;
        let x = p.x + nearest_shift(p.x, camera.translation().x, period.0);
        tf.translation = Vec3::new(x, terrain.heights.sample(p).max(WATER_LEVEL), p.y);
        tf.rotation = Quat::from_rotation_y(dir.x.atan2(dir.y));
        actor.walk(SPEED * stage.dt, true, stage.dt, t);
        // Only where we see it.
        let tile = session.game.map.grid.tile(crate::layout::hex_at(p));
        let seen = tile.is_some_and(|t| me.sees(t));
        vis.set_if_neq(if seen { Visibility::Inherited } else { Visibility::Hidden });
    }
}
