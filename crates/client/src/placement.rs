//! Choosing where a city digs a canal or tunnel, or builds a bridge. Picking
//! one in the city panel starts placement: every tile of the city's land
//! where it could go is tinted (drawn over trees and hills, like Civ VI's
//! district placement), the one under the cursor brighter, and a banner
//! says what it would do.
//! The hovered site shows a glowing ghost of what would be built there.
//! Clicking a tinted tile queues the dig there; right-click or Esc cancels.
//! Passages our cities are working on show as fainter ghosts until
//! they're done; while the city panel is open, its queued digs are ringed.

use crate::icons::Icons;
use crate::layout::tile_center;
use crate::loading::InGameSet;
use crate::overlay::{corner, overlay_material, HoveredTile, XRay, XRayMaterial};
use crate::play::{GameEvent, Notice, SelectedCity, Session};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::ui::{self, palette};
use crate::wrap::{nearest_shift, WrapPeriod, Wrapped};
use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use sim::city::{Build, CityId};
use sim::game::Command;
use sim::hex::TileId;
use sim::improvement::Passage;

/// A canal or tunnel waiting for the player to pick its tile.
#[derive(Resource, Default)]
pub struct Placing(pub Option<Placement>);

#[derive(Clone, Copy, PartialEq)]
pub struct Placement {
    pub city: CityId,
    pub passage: Passage,
    /// Queue it first (Shift-click) rather than last.
    pub front: bool,
    /// Buy it on the chosen site instead of queueing it.
    pub buy: bool,
}

/// The tinted sites (and the brighter one under the cursor).
#[derive(Component)]
struct SiteTint;

#[derive(Component)]
struct Banner;

/// What the tint shows: the sites, the one under the cursor, and for what.
type TintShown = (Vec<TileId>, Option<TileId>, Passage);

/// A see-through canal or tunnel: the one under the cursor while placing,
/// or one being dug.
#[derive(Component)]
struct Ghost;

/// The ghosts' materials: bright for the site under the cursor, faint for
/// those being dug.
#[derive(Default)]
struct GhostLooks {
    hover: Option<Handle<XRayMaterial>>,
    planned: Option<Handle<XRayMaterial>>,
}

pub struct PlacementPlugin;

impl Plugin for PlacementPlugin {
    fn build(&self, app: &mut App) {
        // A bought canal is dug the moment it's placed, so placing runs before
        // the world is reshaped: everything reacting to the new terrain
        // (trees, works, yields) must see the event after the new heights,
        // or trees on the dug tile stay floating at the old ground's height.
        app.init_resource::<Placing>().add_systems(
            Update,
            (
                (cancel, place).chain().after(crate::play::PlaySet).before(crate::terraform::ReshapeSet),
                (tint_sites, ghosts, banner, ring_queued).chain().after(place).after(crate::terraform::ReshapeSet),
            )
                .in_set(InGameSet),
        );
    }
}

/// What digging here would do, for the banner.
fn site_note(session: &Session, p: Passage, tile: TileId) -> String {
    let game = &session.game;
    match p {
        Passage::Canal if !game.canal_joins(tile) => {
            let lost = [
                game.map.tile(tile).resource.map(|r| game.rules.resource(r).name.clone()),
                game.improvement_at(tile).map(|i| game.rules.improvement(i).name.clone()),
                game.map.tile(tile).feature.map(|f| game.rules.feature(f).name.clone()),
            ];
            let lost: Vec<String> = lost.into_iter().flatten().collect();
            let mut note = "Digs this land out into water".to_string();
            if !lost.is_empty() {
                note.push_str(&format!(" (losing its {})", lost.join(", ")));
            }
            note
        }
        Passage::Canal => match game.canal_shortcut(tile) {
            None => "Joins two separate waters".into(),
            Some(d) => format!("Saves ships {} tiles going round", d.saturating_sub(2)),
        },
        Passage::Tunnel => "Takes a road through the mountains".into(),
        Passage::Fort => "Holds the land around it".into(),
        Passage::Watchtower => "Watches over the land around it".into(),
        Passage::Bridge => match game.bridge_shortcut(tile) {
            None => "Joins two separate lands".into(),
            Some(d) => format!("Saves armies {} tiles going round", d.saturating_sub(2)),
        },
    }
}

/// What a city does to make `p`: "Choose where Amsterdam builds its Bridge".
fn verb(p: Passage) -> &'static str {
    match p {
        Passage::Bridge | Passage::Fort | Passage::Watchtower => "builds",
        Passage::Canal | Passage::Tunnel => "digs",
    }
}

/// Each kind of passage's colour on the map (its sites, ghosts and rings).
fn hue(p: Passage) -> Color {
    match p {
        Passage::Canal => Color::srgb(0.35, 0.8, 1.0),
        Passage::Tunnel => Color::srgb(1.0, 0.78, 0.35),
        Passage::Bridge => Color::srgb(0.55, 1.0, 0.45),
        Passage::Fort => Color::srgb(1.0, 0.45, 0.4),
        Passage::Watchtower => Color::srgb(0.85, 0.6, 1.0),
    }
}

/// Right-click, a new game or the city going away ends placement (Esc is
/// handled with the menus, in `menu.rs`). Dev aid: FOURX_PLACE=canal|tunnel|bridge
/// gives us every tech and starts placing one for our city with the most
/// sites, looking at it (use with FOURX_TURNS).
fn cancel(
    mut session: ResMut<Session>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut placing: ResMut<Placing>,
    mut rig: ResMut<crate::camera::CameraRig>,
) {
    if session.is_added() {
        placing.0 = None;
        let Ok(kind) = std::env::var("FOURX_PLACE") else { return };
        let passage = match kind.as_str() {
            "tunnel" => Passage::Tunnel,
            "bridge" => Passage::Bridge,
            "fort" => Passage::Fort,
            "watchtower" => Passage::Watchtower,
            _ => Passage::Canal,
        };
        let me = session.me;
        let game = &mut session.game;
        game.players[me as usize].techs.fill(true);
        // Our cities' borders out to their full reach, for more sites.
        let mine: Vec<(CityId, TileId)> = game.cities.values().filter(|c| c.owner == me).map(|c| (c.id, c.tile)).collect();
        for (_, tile) in mine {
            for t in game.map.grid.within(tile, sim::city::MAX_CITY_RADIUS) {
                if game.territory[t as usize].is_none() {
                    game.territory[t as usize] = Some(me);
                }
            }
        }
        game.refresh_districts();
        game.update_visibility(me);
        let game = &session.game;
        let best = game.cities.values().filter(|c| c.owner == me).max_by_key(|c| (game.passage_sites(c.id, passage).len(), c.id));
        if let Some(c) = best {
            let sites = game.passage_sites(c.id, passage);
            info!("FOURX_PLACE: {} has {} sites", c.name, sites.len());
            for t in sites {
                let p = tile_center(&game.map.grid, t);
                info!("FOURX_PLACE: site at {:.1},{:.1}", p.x, p.y);
            }
            placing.0 = Some(Placement { city: c.id, passage, front: false, buy: false });
            rig.focus = tile_center(&game.map.grid, c.tile);
        }
        return;
    }
    let Some(p) = placing.0 else { return };
    let gone = session.game.cities.get(&p.city).is_none_or(|c| c.owner != session.me);
    if gone || mouse.just_pressed(MouseButton::Right) {
        placing.0 = None;
    }
}

/// A left click on a site queues the dig there (a drag pans the map instead).
fn place(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    mut pressed: Local<Option<Vec2>>,
    hover: Res<HoverMap>,
    nodes: Query<(), With<Node>>,
    hovered: Res<HoveredTile>,
    mut placing: ResMut<Placing>,
    mut session: ResMut<Session>,
    mut notice: ResMut<Notice>,
    mut out: MessageWriter<GameEvent>,
) {
    let Some(p) = placing.0 else { return };
    let cursor = window.cursor_position();
    if mouse.just_pressed(MouseButton::Left) {
        *pressed = cursor.filter(|_| !crate::ui::pointer_over_ui(&hover, &nodes));
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let click = pressed.take().zip(cursor).is_some_and(|(a, b)| a.distance(b) < 6.0);
    if !click {
        return;
    }
    let Some(tile) = hovered.0 else { return };
    if let Some(why) = session.game.site_problem(p.city, p.passage, tile) {
        notice.show(format!("Can't build a {} there: {why}.", p.passage.name().to_lowercase()));
        return;
    }
    let build = Build::Passage(p.passage, tile);
    if p.buy {
        let price = session.game.buy_price(p.city, build);
        match session.command(Command::Buy { city: p.city, build }, &mut out) {
            Ok(()) => {
                notice.inform(crate::icons::Icon::Gold, format!("Bought a {} for {price} gold.", p.passage.name().to_lowercase()));
                placing.0 = None;
            }
            Err(e) => notice.show(format!("Can't buy it: {e}.")),
        }
        return;
    }
    let c = &session.game.cities[&p.city];
    // Choosing something to build ends a project.
    let mut queue: Vec<Build> = c.queue.iter().copied().filter(|b| !matches!(b, Build::Project(_))).collect();
    if p.front {
        queue.insert(0, build);
    } else {
        queue.push(build);
    }
    match session.command(Command::SetQueue { city: p.city, queue }, &mut out) {
        Ok(()) => placing.0 = None,
        Err(e) => notice.show(format!("Can't build there: {e}.")),
    }
}

/// Filled hexes over `tiles`, following the ground (or the water surface).
fn hex_fill(terrain: &Terrain, game: &sim::game::Game, tiles: &[TileId]) -> Mesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for &t in tiles {
        let c = tile_center(&game.map.grid, t);
        let y = |p: Vec2| terrain.heights.sample(p).max(WATER_LEVEL) + 0.04;
        let base = positions.len() as u32;
        positions.push([c.x, y(c), c.y]);
        for i in 0..6 {
            let p = c + corner(i, 0.06);
            positions.push([p.x, y(p), p.y]);
        }
        for i in 0..6u32 {
            indices.extend_from_slice(&[base, base + 1 + i, base + 1 + (i + 1) % 6]);
        }
    }
    let n = positions.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
        .with_inserted_indices(Indices::U32(indices))
}

/// Rebuilds the tint when placement starts or ends, the game changes or the
/// cursor moves to another tile.
fn tint_sites(
    mut commands: Commands,
    session: Res<Session>,
    placing: Res<Placing>,
    hovered: Res<HoveredTile>,
    terrain: Res<Terrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<XRayMaterial>>,
    old: Query<Entity, With<SiteTint>>,
    mut shown: Local<Option<TintShown>>,
) {
    if !placing.is_changed() && !session.is_changed() && !hovered.is_changed() {
        return;
    }
    let game = &session.game;
    let now = placing.0.map(|p| {
        let sites = game.passage_sites(p.city, p.passage);
        let hovered_site = hovered.0.filter(|t| sites.contains(t));
        (sites, hovered_site, p.passage)
    });
    // Only rebuilt when that changes (a fresh mesh takes a frame to show).
    if *shown == now {
        return;
    }
    shown.clone_from(&now);
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some(p) = placing.0 else { return };
    let sites = game.passage_sites(p.city, p.passage);
    let centre = tile_center(&game.map.grid, game.cities[&p.city].tile);
    let color = hue(p.passage);
    let hovered_site = hovered.0.filter(|t| sites.contains(t));
    // The site under the cursor shows its ghost instead (see `ghosts`).
    let others: Vec<TileId> = sites.iter().copied().filter(|&t| Some(t) != hovered_site).collect();
    for (tiles, alpha) in [(others, 0.45)] {
        if tiles.is_empty() {
            continue;
        }
        commands.spawn((
            SiteTint,
            Mesh3d(meshes.add(hex_fill(&terrain, game, &tiles))),
            MeshMaterial3d(materials.add(XRayMaterial { base: overlay_material(color.with_alpha(alpha)), extension: XRay {} })),
            Transform::default(),
            Wrapped::mesh(centre.x),
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

/// Ghosts of the canals and tunnels being dug, and of the one the cursor
/// is on while placing.
fn ghosts(
    mut commands: Commands,
    session: Res<Session>,
    placing: Res<Placing>,
    hovered: Res<HoveredTile>,
    terrain: Res<Terrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<XRayMaterial>>,
    old: Query<Entity, With<Ghost>>,
    mut looks: Local<GhostLooks>,
    mut shown: Local<Vec<(TileId, Passage, bool)>>,
) {
    if !placing.is_changed() && !session.is_changed() && !hovered.is_changed() {
        return;
    }
    let game = &session.game;
    // Drawn over trees and hills, like the tint.
    let look = |alpha: f32, glow: f32| XRayMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(0.75, 0.92, 1.0, alpha),
            emissive: LinearRgba::rgb(0.25, 0.55, 0.8) * glow,
            alpha_mode: AlphaMode::Blend,
            ..default()
        },
        extension: XRay {},
    };
    let hover = looks.hover.get_or_insert_with(|| materials.add(look(0.85, 0.5))).clone();
    let planned = looks.planned.get_or_insert_with(|| materials.add(look(0.45, 0.2))).clone();
    // What to show: digs under way, and the site under the cursor.
    let mut list: Vec<(TileId, Passage, bool)> = Vec::new();
    for c in game.cities.values().filter(|c| c.owner == session.me) {
        for b in &c.queue {
            if let Build::Passage(p, t) = *b {
                list.push((t, p, false));
            }
        }
    }
    if let Some(p) = placing.0 {
        if let Some(t) = hovered.0.filter(|&t| game.site_problem(p.city, p.passage, t).is_none()) {
            list.retain(|g| g.0 != t);
            list.push((t, p.passage, true));
        }
    }
    // Only rebuilt when that changes (a fresh mesh takes a frame to show).
    if *shown == list {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    *shown = list.clone();
    for (tile, passage, hovering) in list {
        let material = if hovering { hover.clone() } else { planned.clone() };
        let (mesh, at) = crate::improvements::ghost_model(game, &terrain, tile, passage);
        commands.spawn((
            Ghost,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_translation(at),
            Wrapped { anchor: at.x, base: at.x },
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

/// "Choose where Amsterdam digs its Canal": what's being placed, and what the
/// hovered site would do.
fn banner(
    mut commands: Commands,
    session: Res<Session>,
    placing: Res<Placing>,
    hovered: Res<HoveredTile>,
    icons: Res<Icons>,
    old: Query<Entity, With<Banner>>,
    mut last_hover: Local<Option<TileId>>,
) {
    if !placing.is_changed() && !session.is_changed() && *last_hover == hovered.0 {
        return;
    }
    *last_hover = hovered.0;
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some(p) = placing.0 else { return };
    let game = &session.game;
    let city = &game.cities[&p.city].name;
    let sites = game.passage_sites(p.city, p.passage);
    let detail = match hovered.0 {
        Some(t) if sites.contains(&t) => site_note(&session, p.passage, t),
        Some(t) => game.site_problem(p.city, p.passage, t).map_or(String::new(), |why| format!("Not here: {why}")),
        None => String::new(),
    };
    let icon = crate::city_panel::passage_icon(p.passage);
    let mut heading = format!("Choose where {city} {} its {}", verb(p.passage), p.passage.name());
    if p.buy {
        let price = sites.first().map_or(0, |&t| game.buy_price(p.city, Build::Passage(p.passage, t)));
        heading.push_str(&format!(", bought for {price} gold"));
    }
    commands.spawn((
        Banner,
        Node {
            position_type: PositionType::Absolute,
            top: px(crate::topbar::HEIGHT + 60.0),
            left: percent(50),
            align_items: AlignItems::Center,
            column_gap: px(12),
            padding: UiRect::axes(px(18), px(10)),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
        ui::frame(),
        Pickable::IGNORE,
        GlobalZIndex(4),
        children![
            ui::icon(icons.get(icon), 36.0),
            (
                Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() },
                Pickable::IGNORE,
                children![
                    (ui::heading(heading, 18.0, palette::ACCENT), Pickable::IGNORE),
                    (
                        ui::text(
                            if detail.is_empty() { "Click a highlighted tile · right-click or Esc to cancel".to_string() } else { detail },
                            13.5,
                            palette::TEXT,
                        ),
                        Pickable::IGNORE,
                    ),
                ],
            ),
        ],
    ));
}

/// While a city's panel is open, rings on the tiles where it's going to dig;
/// while placing, bright outlines on every site.
fn ring_queued(
    mut gizmos: Gizmos,
    session: Res<Session>,
    selected: Res<SelectedCity>,
    placing: Res<Placing>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    camera: Single<&GlobalTransform, With<crate::camera::MainCamera>>,
) {
    let grid = &session.game.map.grid;
    let cam_x = camera.translation().x;
    let ring = |gizmos: &mut Gizmos, tile: TileId, inset: f32, color: Color| {
        let centre = tile_center(grid, tile);
        let shift = nearest_shift(centre.x, cam_x, period.0);
        let point = |i: usize| {
            let q = centre + corner(i % 6, inset);
            Vec3::new(q.x + shift, terrain.heights.sample(q).max(WATER_LEVEL) + 0.06, q.y)
        };
        gizmos.linestrip((0..=6).map(point), color);
    };
    if let Some(p) = placing.0 {
        let color = hue(p.passage).mix(&Color::WHITE, 0.3);
        for t in session.game.passage_sites(p.city, p.passage) {
            ring(&mut gizmos, t, 0.06, color);
            ring(&mut gizmos, t, 0.1, color.with_alpha(0.6));
        }
    }
    let Some(c) = selected.0.and_then(|c| session.game.cities.get(&c)) else { return };
    for b in &c.queue {
        let Build::Passage(p, tile) = *b else { continue };
        let color = hue(p).mix(&Color::WHITE, 0.15);
        ring(&mut gizmos, tile, 0.15, color);
    }
}
