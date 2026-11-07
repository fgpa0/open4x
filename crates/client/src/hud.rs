//! Tile info card: when the cursor rests on a map tile, a small card next to
//! it shows the terrain, its yields, defence and movement cost, who owns it and
//! which units stand on it.

use crate::icons::{Icon, Icons};
use crate::loading::InGameSet;
use crate::overlay::HoveredTile;
use crate::play::Session;
use crate::ui::{self, palette};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use sim::climate::SeaIce;
use sim::combat::{CombatPreview, Modifier, Target};
use sim::game::{PlayerId, UnitId};
use sim::rules::TechGrant;
use sim::hex::TileId;
use sim::map::Elevation;

#[derive(Component)]
struct TileCard;

/// Seconds the cursor must rest before the card appears.
const DELAY: f32 = 0.45;

#[derive(Default)]
struct Rest {
    cursor: Vec2,
    still: f32,
    /// The tile the card shows, and whether it shows a combat preview.
    shown: Option<(TileId, bool)>,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_card).add_systems(Update, update_card.after(crate::play::PlaySet).in_set(InGameSet));
    }
}

fn spawn_card(mut commands: Commands) {
    commands.spawn((
        TileCard,
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            min_width: px(190),
            padding: UiRect::axes(px(12), px(10)),
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::all(px(7)),
            ..default()
        },
        ui::frame(),
        Visibility::Hidden,
        GlobalZIndex(25),
        Pickable::IGNORE,
    ));
}

fn update_card(
    mut commands: Commands,
    time: Res<Time>,
    hovered: Res<HoveredTile>,
    session: Res<Session>,
    icons: Res<Icons>,
    window: Single<&Window>,
    ui_scale: Res<UiScale>,
    mouse: Res<ButtonInput<MouseButton>>,
    hover: Res<HoverMap>,
    nodes: Query<(), With<Node>>,
    menu: Res<State<crate::menu::Menu>>,
    selected: Res<crate::play::Selected>,
    mut rest: Local<Rest>,
    mut card: Single<(Entity, &mut Node, &mut Visibility, &ComputedNode), With<TileCard>>,
) {
    let (entity, node, vis, computed) = &mut *card;
    let over_ui = hover.get(&PointerId::Mouse).is_some_and(|hits| hits.keys().any(|&e| nodes.contains(e) && e != *entity));
    let Some(cursor) = window.cursor_position() else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    if cursor.distance(rest.cursor) > 3.0 || mouse.pressed(MouseButton::Left) || mouse.pressed(MouseButton::Right) {
        rest.still = 0.0;
        rest.cursor = cursor;
    } else {
        rest.still += time.delta_secs();
    }
    let usable = !over_ui && *menu.get() == crate::menu::Menu::Closed;
    // With one of our units selected, hovering something it can attack shows
    // the combat preview right away; otherwise tile info after a short rest.
    let attacker = selected.0.filter(|u| session.game.units.get(u).is_some_and(|u| u.owner == session.me));
    let combat = match (attacker, hovered.0) {
        (Some(u), Some(t)) if usable => match session.game.peace_blocks_attack(u, t) {
            // At peace: show what attacking would do, and that it means war.
            Some(on) => session.game.combat_preview_at_war(u, t).ok().map(|p| (u, t, p, Some(on))),
            None => session.game.combat_preview(u, t).ok().map(|p| (u, t, p, None)),
        },
        _ => None,
    };
    let tile = match &combat {
        Some((_, t, _, _)) => Some(*t),
        None => hovered.0.filter(|_| usable && rest.still >= DELAY),
    };
    let Some(tile) = tile else {
        vis.set_if_neq(Visibility::Hidden);
        rest.shown = None;
        return;
    };
    let key = Some((tile, combat.is_some()));
    if rest.shown != key || session.is_changed() || selected.is_changed() {
        rest.shown = key;
        commands.entity(*entity).despawn_related::<Children>();
        match combat {
            Some((unit, _, preview, war_on)) => {
                commands.entity(*entity).with_children(|c| fill_combat(c, &session, &icons, unit, &preview, war_on));
            }
            None => {
                commands.entity(*entity).with_children(|c| fill(c, &session, &icons, tile));
            }
        }
    }
    vis.set_if_neq(Visibility::Inherited);
    let scale = ui_scale.0;
    let size = computed.size() * computed.inverse_scale_factor() * scale;
    let mut x = cursor.x + 20.0;
    let mut y = cursor.y + 20.0;
    if x + size.x > window.width() - 8.0 {
        x = cursor.x - size.x - 14.0;
    }
    if y + size.y > window.height() - 8.0 {
        y = cursor.y - size.y - 14.0;
    }
    node.left = px(x / scale);
    node.top = px(y / scale);
}

/// The combat preview: both sides' strengths and bonuses, hit points before
/// and after, and the verdict.
fn fill_combat(c: &mut ChildSpawnerCommands, session: &Session, icons: &Icons, unit: UnitId, p: &CombatPreview, war_on: Option<PlayerId>) {
    let game = &session.game;
    let u = &game.units[&unit];
    let ours = game.unit_def(u);
    let title = match p.target {
        Target::Civilians => "Capture",
        Target::City(_) if p.captures && p.damage_dealt == 0 => "Take City",
        Target::City(_) if p.ranged => "Bombard City",
        Target::City(_) => "Storm City",
        _ if p.ranged => "Ranged Attack",
        _ => "Attack",
    };
    c.spawn((ui::heading(title, 17.0, palette::DANGER), Pickable::IGNORE));
    if let Some(on) = war_on {
        let text = format!("You are at peace with {}: attacking declares war.", game.civ(on).name);
        c.spawn((ui::bold(text, 13.0, palette::DANGER), Pickable::IGNORE));
    }
    if let Target::Civilians = p.target {
        c.spawn((ui::text("The enemy civilians here join you without a fight.", 13.0, palette::TEXT), Pickable::IGNORE));
        return;
    }
    let (their_icon, their_name, their_owner) = match p.target {
        Target::Unit(id) => {
            let d = &game.units[&id];
            (Icon::unit(&game.unit_def(d).art), game.unit_def(d).name.clone(), d.owner)
        }
        Target::City(id) => (Icon::FoundCity, game.cities[&id].name.clone(), game.cities[&id].owner),
        Target::Civilians => unreachable!(),
    };
    let side = |c: &mut ChildSpawnerCommands, icon: Icon, name: String, base: i32, eff: f32, mods: &[Modifier], color: Color| {
        c.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(3), width: px(150), ..default() }, Pickable::IGNORE))
            .with_children(|col| {
                col.spawn((
                    Node { align_items: AlignItems::Center, column_gap: px(6), ..default() },
                    Pickable::IGNORE,
                    children![ui::icon(icons.get(icon), 26.0), (ui::bold(name, 14.0, color), Pickable::IGNORE)],
                ));
                col.spawn((
                    Node { align_items: AlignItems::Center, column_gap: px(5), ..default() },
                    Pickable::IGNORE,
                    children![
                        ui::icon(icons.get(Icon::Strength), 18.0),
                        (ui::bold(format!("{eff:.1}"), 18.0, palette::TEXT), Pickable::IGNORE),
                        (ui::text(format!("base {base}"), 11.5, palette::TEXT_DIM), Pickable::IGNORE),
                    ],
                ));
                for m in mods {
                    let tint = if m.percent >= 0 { palette::FOOD } else { palette::DANGER };
                    col.spawn((ui::text(format!("{:+}% {}", m.percent, m.label), 12.0, tint), Pickable::IGNORE));
                }
            });
    };
    let owner_name = game.players[their_owner as usize].name.clone();
    c.spawn((Node { column_gap: px(14), ..default() }, Pickable::IGNORE)).with_children(|row| {
        side(row, Icon::unit(&ours.art), ours.name.clone(), p.attack_base, p.attack, &p.attack_mods, palette::ACCENT);
        side(row, their_icon, format!("{their_name} ({owner_name})"), p.defense_base, p.defense, &p.defense_mods, palette::DANGER);
    });
    // Hit points before and after.
    let bar = |c: &mut ChildSpawnerCommands, label: String, hp: i32, max: i32, loss: i32| {
        let after = (hp - loss).max(0);
        c.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() }, Pickable::IGNORE)).with_children(|col| {
            col.spawn((
                Node { justify_content: JustifyContent::SpaceBetween, ..default() },
                Pickable::IGNORE,
                children![
                    (ui::text(label, 12.0, palette::TEXT_DIM), Pickable::IGNORE),
                    (ui::bold(format!("{hp} → {after}"), 12.5, if loss > 0 { palette::DANGER } else { palette::TEXT }), Pickable::IGNORE),
                ],
            ));
            let color = if after * 2 < max { palette::DANGER } else { palette::FOOD };
            col.spawn((ui::progress_bar(after as f32 / max as f32, color, 6.0), Pickable::IGNORE));
        });
    };
    bar(c, format!("Your {}", ours.name), p.attacker_hp, sim::combat::MAX_HP, p.damage_taken);
    bar(c, their_name, p.target_hp, p.target_max_hp, p.damage_dealt);
    // What a city's strength is made of, and how it falls.
    if let Target::City(id) = p.target {
        let garrison = game.garrison_strength(id);
        let mut line = format!("City strength {}", game.city_own_strength(id));
        if garrison > 0 {
            line.push_str(&format!(" + {garrison} from its garrison"));
        }
        line.push_str(if p.ranged { ". Ranged units wear the walls down; only melee units take the city." } else { ". Walls at 0 and a melee unit takes it." });
        c.spawn((ui::text(line, 12.0, palette::TEXT_DIM), Pickable::IGNORE));
    }
    let (verdict, color) = if p.captures {
        ("Captures the city!", palette::FOOD)
    } else if p.attacker_dies() {
        ("Your unit would die", palette::DANGER)
    } else if p.target_destroyed() {
        ("Destroys the enemy", palette::FOOD)
    } else if p.ranged {
        ("Safe: ranged attacks take no damage", palette::FOOD)
    } else if p.damage_dealt > p.damage_taken {
        ("Favourable", palette::FOOD)
    } else if p.damage_dealt == p.damage_taken {
        ("Even trade", palette::ACCENT)
    } else {
        ("Unfavourable", palette::DANGER)
    };
    c.spawn((ui::bold(verdict, 15.0, color), Node { margin: UiRect::top(px(2)), ..default() }, Pickable::IGNORE));
    let how = if war_on.is_some() { "Right-click to declare war and attack" } else { "Right-click to attack" };
    c.spawn((ui::text(how, 11.5, palette::TEXT_DIM), Pickable::IGNORE));
}

/// A tile's terrain and elevation: "Grassland Hills".
pub fn tile_name(game: &sim::game::Game, tile: TileId) -> String {
    let t = game.map.tile(tile);
    let terrain = game.rules.terrain(t.terrain).name.clone();
    match t.elevation {
        Elevation::Hills => format!("{terrain} Hills"),
        Elevation::Mountains => format!("{terrain} Mountains"),
        Elevation::Flat => terrain,
    }
}

/// A tile's feature, river, improvement and road, if any.
pub fn tile_extras(game: &sim::game::Game, tile: TileId) -> Vec<String> {
    let mut extras = Vec::new();
    if let Some(f) = game.map.tile(tile).feature {
        extras.push(game.rules.feature(f).name.clone());
    }
    if game.map.has_river(tile) {
        extras.push("River".to_string());
    }
    if let Some(i) = game.improvement_at(tile) {
        extras.push(game.rules.improvement(i).name.clone());
    }
    if game.has_road(tile) && game.city_at(tile).is_none() {
        extras.push("Road".to_string());
    }
    extras
}

fn fill(c: &mut ChildSpawnerCommands, session: &Session, icons: &Icons, tile: TileId) {
    let game = &session.game;
    let me = &game.players[session.me as usize];
    if !me.explored[tile as usize] {
        c.spawn((ui::bold("Unexplored", 15.0, palette::TEXT_DIM), Pickable::IGNORE));
        return;
    }
    let (map, rules) = (&game.map, &game.rules);
    c.spawn((ui::heading(tile_name(game, tile), 17.0, palette::ACCENT), Pickable::IGNORE));
    let extras = tile_extras(game, tile);
    if !extras.is_empty() {
        c.spawn((ui::text(extras.join(" · "), 13.0, palette::TEXT), Pickable::IGNORE));
    }

    if let Some(r) = game.visible_resource(session.me, tile) {
        c.spawn((
            Node { align_items: AlignItems::Center, column_gap: px(6), ..default() },
            Pickable::IGNORE,
            children![
                ui::icon(icons.get(Icon::resource(&rules.resource(r).id)), 20.0),
                ui::text(crate::improvements::resource_line(game, session.me, tile, r), 13.0, palette::TEXT),
            ],
        ));
    }
    if let Some((work, done)) = game.works[tile as usize].progress {
        let total = game.work_turns(session.me, tile, work);
        let name = crate::improvements::work_name(game, work);
        c.spawn((ui::text(format!("{name} under way: {done} of {total} turns"), 13.0, palette::TEXT_DIM), Pickable::IGNORE));
    }

    let y = game.tile_yields(session.me, tile);
    let dim = |v: i32, color: Color| if v > 0 { color } else { palette::TEXT_DIM };
    c.spawn((Node { column_gap: px(14), margin: UiRect::top(px(2)), ..default() }, Pickable::IGNORE)).with_children(|row| {
        row.spawn(ui::stat(icons, Icon::Food, y.food.to_string(), 15.0, dim(y.food, palette::FOOD)));
        row.spawn(ui::stat(icons, Icon::Production, y.production.to_string(), 15.0, dim(y.production, palette::PRODUCTION)));
        row.spawn(ui::stat(icons, Icon::Gold, y.gold.to_string(), 15.0, dim(y.gold, palette::GOLD)));
    });
    c.spawn((Node { column_gap: px(14), ..default() }, Pickable::IGNORE)).with_children(|row| {
        let me = session.me;
        let deep = rules.terrain(map.tile(tile).terrain).deep;
        let ice = game.sea_ice(tile);
        let moves = match map.land_move_cost(rules, tile) {
            Some(cost) if game.has_road(tile) && cost > 1 => format!("{cost} to enter, 1 by road"),
            Some(cost) => format!("{cost} to enter"),
            None if game.has_bridge(tile) => "1 to cross the bridge (ships sail under it)".into(),
            None if game.has_tunnel(tile) => format!("{} through the tunnel, 1 by road", sim::path::TUNNEL_COST),
            None if !map.is_water(rules, tile) || map.tile(tile).feature.is_some_and(|f| rules.feature(f).impassable) => "Impassable".into(),
            None if ice == SeaIce::Pack => "Pack ice: impassable until it thaws".into(),
            None if deep && !game.grants(me, TechGrant::DeepWater) => "Open sea: needs Shipbuilding".into(),
            None if !game.grants(me, TechGrant::Embark) => "Ships only: Sailing to embark".into(),
            None if ice == SeaIce::Thin => format!("Thin ice: {} to sail (embarking ends the turn)", rules.terrain.sea_ice.thin_move_cost),
            None => "1 to sail (embarking ends the turn)".into(),
        };
        row.spawn(ui::stat(icons, Icon::Moves, moves, 13.0, palette::TEXT));
        let defense = map.defense_bonus(rules, tile);
        if defense != 0 {
            row.spawn(ui::stat(icons, Icon::Defense, format!("{defense:+}%"), 13.0, palette::TEXT));
        }
    });

    if let Some(holder) = game.territory_owner(tile) {
        let owner = &game.players[holder as usize];
        let whose = if holder == session.me { "Your territory".to_string() } else { owner.name.clone() };
        let [r, g, b] = owner.color;
        let mut label = match game.district(tile).and_then(|id| game.cities.get(&id)) {
            Some(city) if city.tile == tile => format!("{} (city) · {whose}", city.name),
            Some(city) => format!("{} · {whose}", city.name),
            None => whose,
        };
        if !game.may_enter(session.me, holder) {
            label.push_str(" · borders closed");
        }
        c.spawn((
            Node { align_items: AlignItems::Center, column_gap: px(6), ..default() },
            Pickable::IGNORE,
            children![
                (
                    Node { width: px(10), height: px(10), border_radius: BorderRadius::MAX, ..default() },
                    BackgroundColor(Color::srgb_u8(r, g, b)),
                ),
                ui::text(label, 13.0, palette::TEXT),
            ],
        ));
    }
    if me.sees(tile) {
        for u in game.units_at(tile) {
            let def = game.unit_def(u);
            let owner = &game.players[u.owner as usize];
            let whose = if u.owner == session.me { String::new() } else { format!(" ({})", owner.name) };
            c.spawn((
                Node { align_items: AlignItems::Center, column_gap: px(6), ..default() },
                Pickable::IGNORE,
                children![ui::icon(icons.get(Icon::unit(&def.art)), 18.0), ui::text(format!("{}{whose}", def.name), 13.0, palette::TEXT)],
            ));
        }
    }
}
