//! Settling hints: while a Settler is selected, the best city sites near it
//! get a marker on the map, scored like the AI scores them (`ai::settle`)
//! but only from what we have explored. Hovering one says why it's good.

use crate::camera::CameraRig;
use crate::icons::{Icon, Icons};
use crate::layout::tile_center;
use crate::loading::InGameSet;
use crate::play::{GameEvent, Notice, Selected, Session};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::ui::{self, palette, Tooltip};
use crate::wrap::{nearest_shift, WrapPeriod};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate, Button};
use sim::game::Command;
use sim::hex::TileId;
use sim::rules::Ability;

/// How far from the Settler (in tiles) sites are suggested.
const SEARCH: i32 = 8;
/// At most this many suggestions, at least this far apart.
const MAX_SITES: usize = 4;
const SPACING: i32 = 3;
/// Sites worth less than this share of the best one aren't suggested.
const GOOD_ENOUGH: f32 = 0.8;

#[derive(Component)]
struct SiteMarker(TileId);

pub struct SiteHintsPlugin;

impl Plugin for SiteHintsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (rebuild, place).chain().after(crate::play::PlaySet).in_set(InGameSet));
    }
}

/// The best sites around `from`, best first.
fn best_sites(session: &Session, from: TileId) -> Vec<(TileId, ai::settle::SiteReport)> {
    let game = &session.game;
    let me = session.me;
    let explored = &game.players[me as usize].explored;
    let known = |t: TileId| explored[t as usize];
    let mut sites: Vec<(TileId, ai::settle::SiteReport)> = game
        .map
        .grid
        .within(from, SEARCH)
        .into_iter()
        .filter(|&t| known(t))
        .filter_map(|t| Some((t, ai::settle::site_report(game, me, t, known)?)))
        .collect();
    // Best first; nearer wins a tie.
    sites.sort_by_key(|(t, r)| (-r.value, game.map.grid.distance(from, *t), *t));
    let Some(best) = sites.first().map(|s| s.1.value) else { return Vec::new() };
    let mut picked: Vec<(TileId, ai::settle::SiteReport)> = Vec::new();
    for (t, r) in sites {
        if (r.value as f32) < best as f32 * GOOD_ENOUGH || picked.len() == MAX_SITES {
            break;
        }
        if picked.iter().all(|(p, _)| game.map.grid.distance(*p, t) >= SPACING) {
            picked.push((t, r));
        }
    }
    picked
}

/// Why a site is good, a line per reason.
fn reasons(session: &Session, r: &ai::settle::SiteReport, here: bool) -> String {
    let game = &session.game;
    let mut lines = Vec::new();
    if r.river {
        lines.push("On a river: fresh water".to_string());
    } else if r.lake {
        lines.push("By a lake: fresh water".to_string());
    }
    if r.coast {
        lines.push("On the coast: can build ships".to_string());
    }
    lines.push(format!("Land in reach: {} food, {} production, {} gold", r.food, r.production, r.gold));
    if !r.resources.is_empty() {
        let names: Vec<&str> = r.resources.iter().map(|&res| game.rules.resource(res).name.as_str()).collect();
        lines.push(format!("Resources: {}", names.join(", ")));
    }
    lines.push(if here { "\nPress B to found the city here." } else { "\nClick to send the Settler here." }.into());
    lines.join("\n")
}

fn rebuild(
    mut commands: Commands,
    session: Res<Session>,
    selected: Res<Selected>,
    icons: Res<Icons>,
    old: Query<Entity, With<SiteMarker>>,
    labels: Single<Entity, With<ui::WorldLabels>>,
) {
    if !session.is_changed() && !selected.is_changed() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    let game = &session.game;
    let Some(unit) = selected.0.and_then(|u| game.units.get(&u)) else { return };
    if unit.owner != session.me || !game.unit_def(unit).has(Ability::FoundCity) {
        return;
    }
    for (i, (tile, report)) in best_sites(&session, unit.tile).into_iter().enumerate() {
        let title = if i == 0 { "Best city site nearby" } else { "Good city site" };
        let (fill, ring) = if i == 0 {
            (Color::srgba(0.56, 0.4, 0.14, 0.95), palette::ACCENT)
        } else {
            (Color::srgba(0.3, 0.22, 0.1, 0.92), palette::ACCENT.with_alpha(0.75))
        };
        let settler = unit.id;
        commands.spawn((
            SiteMarker(tile),
            ChildOf(*labels),
            Button,
            Hovered::default(),
            ui::OwnStyle,
            observe(move |_: On<Activate>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>, mut notice: ResMut<Notice>| {
                if let Err(e) = session.command(Command::MoveUnit { unit: settler, to: tile }, &mut out) {
                    notice.show(format!("Can't move there: {e}"));
                }
            }),
            Node {
                position_type: PositionType::Absolute,
                width: px(40),
                height: px(40),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2.5)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            UiTransform::from_translation(Val2::percent(-50.0, -50.0)),
            BackgroundColor(fill),
            BorderColor::all(ring),
            BoxShadow::new(palette::ACCENT.with_alpha(0.55), px(0), px(0), px(2), px(10)),
            Visibility::Hidden,
            GlobalZIndex(-1),
            Tooltip::new(title, reasons(&session, &report, tile == unit.tile)),
            children![ui::icon(icons.get(Icon::FoundCity), 26.0)],
        ));
    }
}

/// Pins the markers above their tiles; they pulse gently to catch the eye.
fn place(
    time: Res<Time>,
    session: Res<Session>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    ui_scale: Res<UiScale>,
    strategic: Res<crate::strategic::StrategicView>,
    rig: Res<CameraRig>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    mut markers: Query<(&SiteMarker, &mut Node, &mut Visibility, &mut UiTransform)>,
) {
    let (cam, cam_tf) = *camera;
    let pulse = 1.0 + 0.06 * (time.elapsed_secs() * 3.0).sin();
    for (marker, mut node, mut vis, mut transform) in &mut markers {
        transform.scale = Vec2::splat(pulse);
        let p = tile_center(&session.game.map.grid, marker.0);
        let mut world = Vec3::new(p.x, terrain.heights.sample(p).max(WATER_LEVEL) + 0.55, p.y);
        if strategic.0 {
            world.y = crate::strategic::ground(&terrain, p) + 0.3;
        }
        world.x += nearest_shift(world.x, cam_tf.translation().x, period.0);
        let at = cam.world_to_viewport(cam_tf, world).ok().filter(|_| rig.distance < 70.0).map(|s| s / ui_scale.0);
        crate::ui::place_label(&mut node, &mut vis, at, crate::ui::ui_screen(cam, ui_scale.0), 60.0);
    }
}
