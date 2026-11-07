//! Buying tiles. While it's on (the "Buy tiles" toggle in the city panel),
//! every tile the open city could buy gets a gold outline and a price tag
//! anchored on the map; clicking a tag buys the tile. It turns off when the
//! city panel closes or another city opens.

use crate::icons::{Icon, Icons};
use crate::layout::tile_center;
use crate::loading::InGameSet;
use crate::play::{GameEvent, Notice, SelectedCity, Session};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::ui::{self, palette, ButtonState, Tooltip};
use crate::wrap::{nearest_shift, WrapPeriod};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate, Button};
use sim::city::CityId;
use sim::hex::TileId;

/// Whether the open city's purchasable tiles are shown with their prices.
#[derive(Resource, Default)]
pub struct BuyLand(pub bool);

#[derive(Component)]
struct PriceTag {
    tile: TileId,
}

pub struct LandPlugin;

impl Plugin for LandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuyLand>().add_systems(
            Update,
            (switch_off, rebuild_tags, place_tags, outline_tiles).chain().after(crate::play::PlaySet).in_set(InGameSet),
        );
    }
}

/// Off when the city closes or changes, or when there's nothing left to buy.
fn switch_off(selected: Res<SelectedCity>, session: Res<Session>, mut land: ResMut<BuyLand>, mut last: Local<Option<Option<CityId>>>) {
    if last.is_some_and(|c| c != selected.0) && land.0 {
        land.0 = false;
    }
    *last = Some(selected.0);
    let empty = selected.0.is_none_or(|c| session.game.buyable_tiles(c).is_empty());
    if land.0 && empty {
        land.0 = false;
    }
}

/// What a tile yields, for its tag's tooltip.
fn describe(session: &Session, tile: TileId) -> String {
    let game = &session.game;
    let y = game.tile_yields(session.me, tile);
    let name = [crate::hud::tile_name(game, tile)].into_iter().chain(crate::hud::tile_extras(game, tile)).collect::<Vec<_>>().join(", ");
    let mut parts = Vec::new();
    for (n, what) in [(y.food, "food"), (y.production, "production"), (y.gold, "gold")] {
        if n > 0 {
            parts.push(format!("{n} {what}"));
        }
    }
    let yields = if parts.is_empty() { "no yields".to_string() } else { parts.join(", ") };
    format!("{name}: {yields}.")
}

fn rebuild_tags(
    mut commands: Commands,
    session: Res<Session>,
    selected: Res<SelectedCity>,
    land: Res<BuyLand>,
    icons: Res<Icons>,
    old: Query<Entity, With<PriceTag>>,
    labels: Single<Entity, With<crate::ui::WorldLabels>>,
) {
    if !session.is_changed() && !selected.is_changed() && !land.is_changed() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some(city) = selected.0.filter(|_| land.0) else { return };
    let game = &session.game;
    let Some(c) = game.cities.get(&city).filter(|c| c.owner == session.me) else { return };
    let gold = game.players[session.me as usize].gold;
    for tile in game.buyable_tiles(city) {
        let price = game.tile_price(city, tile);
        let affordable = price <= gold;
        let mut body = format!("{}\nIt joins {}'s territory, and its citizens can work it.", describe(&session, tile), c.name);
        if !affordable {
            body.push_str(&format!("\n\nYou need {} more gold.", price - gold));
        }
        commands.spawn((
            PriceTag { tile },
            ChildOf(*labels),
            Button,
            bevy::picking::hover::Hovered::default(),
            Node {
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center,
                column_gap: px(4),
                padding: UiRect::new(px(5), px(8), px(3), px(3)),
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            // Centred on the tile.
            UiTransform::from_translation(Val2::percent(-50.0, -50.0)),
            BackgroundColor(palette::BUTTON),
            BorderColor::all(palette::BUTTON_BORDER),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), px(0), px(2), px(0), px(6)),
            ButtonState { active: false, disabled: !affordable },
            Visibility::Hidden,
            GlobalZIndex(-1),
            Tooltip::new(format!("Buy this tile for {price} gold"), body),
            children![
                ui::icon(icons.get(Icon::Gold), 15.0),
                (ui::bold(price.to_string(), 13.5, if affordable { palette::GOLD } else { palette::TEXT_DIM }), Pickable::IGNORE),
            ],
            observe(move |_: On<Activate>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>, mut notice: ResMut<Notice>| {
                if let Err(e) = session.command(sim::game::Command::BuyTile { city, tile }, &mut out) {
                    notice.show(format!("Can't buy this tile: {e}."));
                }
            }),
        ));
    }
}

/// Where a tile's centre is on screen (UI pixels), if it's in view.
fn screen_pos(
    session: &Session,
    terrain: &Terrain,
    period: f32,
    camera: (&Camera, &GlobalTransform),
    scale: f32,
    tile: TileId,
) -> Option<Vec2> {
    let (cam, cam_tf) = camera;
    let centre = tile_center(&session.game.map.grid, tile);
    let mut world = Vec3::new(centre.x, terrain.heights.sample(centre).max(WATER_LEVEL) + 0.1, centre.y);
    world.x += nearest_shift(world.x, cam_tf.translation().x, period);
    cam.world_to_viewport(cam_tf, world).ok().map(|p| p / scale)
}

fn place_tags(
    session: Res<Session>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    ui_scale: Res<UiScale>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    mut tags: Query<(&PriceTag, &mut Node, &mut Visibility)>,
) {
    let screen = crate::ui::ui_screen(camera.0, ui_scale.0);
    for (tag, mut node, mut vis) in &mut tags {
        let at = screen_pos(&session, &terrain, period.0, *camera, ui_scale.0, tag.tile);
        crate::ui::place_label(&mut node, &mut vis, at, screen, 60.0);
    }
}

/// A gold hexagon on every tile for sale.
fn outline_tiles(
    mut gizmos: Gizmos,
    session: Res<Session>,
    terrain: Res<Terrain>,
    period: Res<WrapPeriod>,
    land: Res<BuyLand>,
    selected: Res<SelectedCity>,
    camera: Single<&GlobalTransform, With<crate::camera::MainCamera>>,
) {
    let Some(city) = selected.0.filter(|_| land.0) else { return };
    let grid = &session.game.map.grid;
    let cam_x = camera.translation().x;
    for tile in session.game.buyable_tiles(city) {
        let c = tile_center(grid, tile);
        let shift = nearest_shift(c.x, cam_x, period.0);
        let point = |i: usize| {
            let p = c + crate::overlay::corner(i, 0.12);
            Vec3::new(p.x + shift, terrain.heights.sample(p).max(WATER_LEVEL) + 0.06, p.y)
        };
        gizmos.linestrip((0..=6).map(|i| point(i % 6)), palette::GOLD.with_alpha(0.85));
    }
}
