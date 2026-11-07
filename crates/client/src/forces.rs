//! Commanding many units: the group panel (in place of the unit card when
//! several units are selected: what they are, their health and orders) and
//! the Forces screen (from the top bar, or O: every group by what it's
//! doing, taking a city, defending one, on the move, and who's waiting).
//! Clicking a group there selects it, so a right-click commands it again.
//! Also a unit's rank and a General's level and traits, shown in the unit card.

use crate::icons::{Icon, Icons};
use crate::loading::InGameSet;
use crate::play::{GameEvent, Group, Notice, OrderRequest, Selected, Session};
use crate::ui::{self, palette, ButtonState, Tooltip};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use sim::game::{Command, Game, Order, Unit, UnitId};
use sim::rules::{UnitClass, UnitTypeId};
use std::collections::BTreeMap;

/// The Forces screen is open.
#[derive(Resource, Default)]
pub struct ForcesOpen(pub bool);

#[derive(Component)]
struct GroupPanel;

#[derive(Component)]
struct ForcesList;

/// The overview's scrolling body (its scroll is kept across rebuilds).
#[derive(Component)]
struct ForcesScroll;

pub struct ForcesPlugin;

impl Plugin for ForcesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ForcesOpen>()
            .add_systems(Update, toggle.in_set(InGameSet))
            .add_systems(Update, (rebuild_group_panel, rebuild_forces).chain().after(crate::play::PlaySet).in_set(InGameSet));
    }
}

/// O opens and closes the Forces screen.
fn toggle(keys: Res<ButtonInput<KeyCode>>, mut open: ResMut<ForcesOpen>) {
    if keys.just_pressed(KeyCode::KeyO) {
        open.0 = !open.0;
    }
}

/// A unit's rank (or a General's level) and a tooltip explaining experience.
pub fn xp_stat(game: &Game, u: &Unit) -> (String, String) {
    let x = &game.rules.experience;
    let general = game.unit_def(u).has(sim::rules::Ability::General);
    if general {
        let level = game.general_level(u);
        let next = game.next_xp(u).map_or("the last level".to_string(), |n| format!("level {} at {n} XP", level + 1));
        return (
            format!("Level {level}"),
            format!(
                "{} XP; next: {next}.\nYour units within {} tiles fight {}% better and share this leader's traits. Leaders learn from every fight of a unit they lead ({} XP, more for kills and captures), and each level is a trait to choose.",
                u.xp,
                x.aura.range,
                x.aura.strength,
                x.gain.general
            ),
        );
    }
    let name = game.rank_def(u).map_or("Recruit".to_string(), |r| r.name.clone());
    let next = match x.ranks.get(game.rank(u)) {
        Some(r) => format!("{} at {} XP ({})", r.name, r.xp, rank_bonus(r)),
        None => "the last rank".into(),
    };
    let ranks: Vec<String> = x.ranks.iter().map(|r| format!("{} ({} XP): {}", r.name, r.xp, rank_bonus(r))).collect();
    (
        name,
        format!(
            "{} XP; next: {next}.\nUnits learn in battle: {} XP for a melee blow, {} for a shot, {} for holding out when attacked, more for kills and captures. They keep it when upgraded.\nRanks: {}.",
            u.xp,
            x.gain.melee,
            x.gain.ranged,
            x.gain.defend,
            ranks.join("; ")
        ),
    )
}

fn rank_bonus(r: &sim::experience::RankDef) -> String {
    let mut s = format!("+{}% strength", r.strength);
    if r.heal > 0 {
        s.push_str(&format!(", heals {} more a turn", r.heal));
    }
    s
}

/// A General's level, its traits and, with a level to spend, a button for
/// each trait it could take.
pub fn spawn_general(parent: &mut ChildSpawnerCommands, game: &Game, icons: &Icons, general: UnitId, mine: bool) {
    let u = &game.units[&general];
    let x = &game.rules.experience;
    let (level, tip) = xp_stat(game, u);
    let title = game.unit_def(u).name.clone();
    let name = u.name.clone().unwrap_or_else(|| title.clone());
    parent.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(5), ..default() }).with_children(|col| {
        col.spawn((
            Node { column_gap: px(6), align_items: AlignItems::Center, ..default() },
            Tooltip::new(format!("{name}, {level}"), tip),
        ))
        .with_children(|row| {
            row.spawn(ui::icon(icons.get(Icon::unit(&game.unit_def(u).art)), 18.0));
            row.spawn(ui::bold(format!("{} {} · {}", title.to_uppercase(), name.to_uppercase(), level.to_uppercase()), 11.0, palette::TEXT_DIM));
            let progress = game.next_xp(u).map_or(format!("{} XP", u.xp), |n| format!("{}/{n} XP", u.xp));
            row.spawn(ui::text(progress, 11.5, palette::TEXT_DIM));
        });
        let led = game.units.values().filter(|o| game.leader_of(o) == Some(general)).count();
        col.spawn(ui::text(
            format!("Leads {led} {} within {} tiles: +{}% strength and his traits.", if led == 1 { "unit" } else { "units" }, x.aura.range, x.aura.strength),
            12.0,
            palette::TEXT_DIM,
        ));
        if !u.traits.is_empty() {
            col.spawn(Node { column_gap: px(5), row_gap: px(5), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
                for &t in &u.traits {
                    let d = x.trait_def(t);
                    row.spawn((
                        Node {
                            padding: UiRect::new(px(8), px(8), px(3), px(3)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(10)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.85, 0.65, 0.25, 0.14)),
                        BorderColor::all(palette::ACCENT.with_alpha(0.6)),
                        Tooltip::new(d.name.clone(), d.description.clone()),
                        children![(ui::text(d.name.clone(), 12.0, palette::ACCENT), Pickable::IGNORE)],
                    ));
                }
            });
        }
        let pending = game.pending_traits(u);
        if pending > 0 && mine {
            col.spawn(ui::text(
                if pending > 1 {
                    format!("Choose {pending} traits (chosen for you at the end of the turn):")
                } else {
                    "Choose a trait (chosen for you at the end of the turn):".to_string()
                },
                12.5,
                palette::ACCENT,
            ));
            col.spawn(Node { column_gap: px(5), row_gap: px(5), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
                for t in game.trait_choices(u) {
                    let d = x.trait_def(t);
                    row.spawn((
                        ui::text_button(&d.name),
                        Tooltip::new(d.name.clone(), d.description.clone()),
                        observe(move |_: On<Activate>, mut session: ResMut<Session>, mut out: MessageWriter<GameEvent>, mut notice: ResMut<Notice>| {
                            if let Err(e) = session.command(Command::ChooseTrait { unit: general, trait_id: t }, &mut out) {
                                notice.show(format!("Can't take that trait: {e}."));
                            }
                        }),
                    ));
                }
            });
        }
    });
}

/// Each kind of unit in `units` once, with how many, strongest first.
fn kinds(game: &Game, units: &[UnitId]) -> Vec<(UnitTypeId, usize)> {
    let mut out: Vec<(UnitTypeId, usize)> = Vec::new();
    for u in units.iter().filter_map(|u| game.units.get(u)) {
        match out.iter_mut().find(|(k, _)| *k == u.kind) {
            Some((_, n)) => *n += 1,
            None => out.push((u.kind, 1)),
        }
    }
    out.sort_by_key(|&(k, n)| {
        let d = game.rules.unit(k);
        (std::cmp::Reverse(d.strength.max(d.ranged)), std::cmp::Reverse(n), k)
    });
    out
}

/// What a group's orders are, in a few words ("4 taking Rome, 1 waiting").
fn orders_summary(game: &Game, units: &[UnitId]) -> String {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for u in units.iter().filter_map(|u| game.units.get(u)) {
        let city = |c| game.cities.get(&c).map_or("a city".to_string(), |c| c.name.clone());
        let what = match u.order {
            Order::Take { city: c } => format!("taking {}", city(c)),
            Order::Defend { city: c } => format!("defending {}", city(c)),
            Order::Hunt { .. } => "hunting".into(),
            Order::Goto(_) => "on the move".into(),
            Order::Fortify => "fortified".into(),
            Order::Sleep => "asleep".into(),
            Order::Explore => "exploring".into(),
            Order::None if u.moves > 0 => "waiting for orders".into(),
            _ => "done for the turn".into(),
        };
        *counts.entry(what).or_default() += 1;
    }
    let mut parts: Vec<(usize, String)> = counts.into_iter().map(|(w, n)| (n, w)).collect();
    parts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    parts.into_iter().map(|(n, w)| format!("{n} {w}")).collect::<Vec<_>>().join(", ")
}

/// A row of unit icons with counts.
fn composition(parent: &mut ChildSpawnerCommands, game: &Game, icons: &Icons, units: &[UnitId], size: f32) {
    parent.spawn((Node { column_gap: px(8), row_gap: px(4), flex_wrap: FlexWrap::Wrap, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|row| {
        for (k, n) in kinds(game, units) {
            let def = game.rules.unit(k);
            row.spawn((Node { column_gap: px(3), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE, Tooltip::title(def.name.clone())))
                .with_children(|c| {
                    c.spawn(ui::icon(icons.get(Icon::unit(&def.art)), size));
                    c.spawn((ui::bold(format!("×{n}"), size * 0.55, palette::TEXT), Pickable::IGNORE));
                });
        }
    });
}

/// Health of `units` together, and the most they could have.
fn health(game: &Game, units: &[UnitId]) -> (i32, i32) {
    let fighters: Vec<&Unit> = units.iter().filter_map(|u| game.units.get(u)).filter(|u| game.unit_def(u).strength > 0 || game.unit_def(u).ranged > 0).collect();
    (fighters.iter().map(|u| u.hp).sum(), fighters.len() as i32 * sim::combat::MAX_HP)
}

fn rebuild_group_panel(
    mut commands: Commands,
    session: Res<Session>,
    selected: Res<Selected>,
    group: Res<Group>,
    icons: Res<Icons>,
    old: Query<Entity, With<GroupPanel>>,
) {
    if !session.is_changed() && !selected.is_changed() && !group.is_changed() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    if group.0.len() < 2 {
        return;
    }
    let game = &session.game;
    let units: Vec<UnitId> = group.0.iter().copied().filter(|u| game.units.contains_key(u)).collect();
    let (hp, max) = health(game, &units);
    let soldiers = units.iter().filter(|u| game.unit_def(&game.units[u]).class == UnitClass::Military).count();
    commands
        .spawn((
            GroupPanel,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(16),
                left: px(16),
                width: px(440),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::new(px(14), px(18), px(14), px(16)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            ui::frame(),
        ))
        .with_children(|panel| {
            panel.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|h| {
                h.spawn(ui::heading(format!("{} units selected", units.len()), 20.0, palette::ACCENT));
                h.spawn((
                    ui::icon_button(icons.get(Icon::Close), 20.0, Tooltip::title("Clear the selection")),
                    observe(|_: On<Activate>, mut selected: ResMut<Selected>, mut group: ResMut<Group>| {
                        selected.0 = None;
                        group.0.clear();
                    }),
                ));
            });
            composition(panel, game, &icons, &units, 26.0);
            if max > 0 {
                let color = if hp * 2 < max { palette::DANGER } else { palette::FOOD };
                panel.spawn((
                    Node { flex_direction: FlexDirection::Column, row_gap: px(3), ..default() },
                    Tooltip::new("Health", "The group's hit points together. Units sent to take a city go home to heal when badly hurt, and come back."),
                ))
                .with_children(|c| {
                    c.spawn(ui::text(format!("Health {}%", hp * 100 / max), 12.5, palette::TEXT_DIM));
                    c.spawn(ui::progress_bar(hp as f32 / max as f32, color, 6.0));
                });
            }
            panel.spawn(ui::text(orders_summary(game, &units), 13.5, palette::TEXT));
            if soldiers > 0 {
                panel.spawn(ui::text(
                    "Right-click an enemy city to take it: they gather out of its fire, then strike together. Right-click one of your cities to defend it, an enemy to hunt it, or the map to go there.",
                    12.5,
                    palette::TEXT_DIM,
                ));
            }
            panel.spawn(Node { column_gap: px(8), ..default() }).with_children(|row| {
                for (icon, key, name, tip, order) in [
                    (Icon::Defense, "F", "Fortify", "Every unit digs in where it stands.", Order::Fortify),
                    (Icon::Sleep, "Z", "Sleep", "They stay put and stop asking for orders.", Order::Sleep),
                    (Icon::Skip, "Space", "Skip Turn", "They wait until next turn.", Order::Skip),
                    (Icon::Close, "", "Stop", "Cancels their orders: they wait for new ones.", Order::None),
                ] {
                    let b = row.spawn((
                        ui::icon_button_with(icons.get(icon), 42.0, Tooltip::new(name, tip), ButtonState::default()),
                        observe(move |_: On<Activate>, mut out: MessageWriter<OrderRequest>| {
                            out.write(OrderRequest(order));
                        }),
                    ));
                    if !key.is_empty() {
                        let mut b = b;
                        b.with_child(ui::hotkey_badge(key));
                    }
                }
            });
        });
}

/// One row of the Forces screen: a group doing one thing.
struct Row {
    title: String,
    detail: String,
    icon: Icon,
    units: Vec<UnitId>,
    urgent: bool,
}

fn rebuild_forces(
    mut commands: Commands,
    session: Res<Session>,
    open: Res<ForcesOpen>,
    group: Res<Group>,
    icons: Res<Icons>,
    old: Query<Entity, With<ForcesList>>,
    old_scroll: Query<&ScrollPosition, With<ForcesScroll>>,
) {
    if !session.is_changed() && !open.is_changed() && !group.is_changed() {
        return;
    }
    let scroll = old_scroll.iter().next().map_or(Vec2::ZERO, |s| s.0);
    for e in &old {
        commands.entity(e).despawn();
    }
    if !open.0 {
        return;
    }
    let game = &session.game;
    let me = session.me;
    let mine: Vec<&Unit> = game.units.values().filter(|u| u.owner == me && game.unit_def(u).class == UnitClass::Military).collect();
    let city_name = |c| game.cities.get(&c).map_or("?".to_string(), |c| c.name.clone());
    let nearest = |units: &[UnitId], tile| units.iter().filter_map(|u| game.units.get(u)).map(|u| game.map.grid.distance(u.tile, tile)).min();
    let mut take: BTreeMap<sim::city::CityId, Vec<UnitId>> = BTreeMap::new();
    let mut defend: BTreeMap<sim::city::CityId, Vec<UnitId>> = BTreeMap::new();
    let mut other: BTreeMap<&'static str, Vec<UnitId>> = BTreeMap::new();
    let waiting: std::collections::HashSet<UnitId> = game.units_needing_orders(me).map(|u| u.id).collect();
    for u in &mine {
        match u.order {
            Order::Take { city } => take.entry(city).or_default().push(u.id),
            Order::Defend { city } => defend.entry(city).or_default().push(u.id),
            Order::Hunt { .. } => other.entry("Hunting").or_default().push(u.id),
            Order::Goto(_) => other.entry("On the move").or_default().push(u.id),
            Order::Explore => other.entry("Exploring").or_default().push(u.id),
            Order::Fortify | Order::Sleep => other.entry("Fortified or asleep").or_default().push(u.id),
            _ if waiting.contains(&u.id) => other.entry("Waiting for orders").or_default().push(u.id),
            _ => other.entry("Done for the turn").or_default().push(u.id),
        }
    }
    let mut sections: Vec<(&str, Vec<Row>)> = Vec::new();
    let rows: Vec<Row> = take
        .into_iter()
        .map(|(c, units)| {
            let (hp, max) = health(game, &units);
            let city = game.cities.get(&c);
            let owner = city.map_or(String::new(), |c| game.players[c.owner as usize].name.clone());
            let walls = city.map_or(String::new(), |c| format!(", walls {}/{}", c.hp, sim::combat::CITY_MAX_HP));
            let dist = city.and_then(|c| nearest(&units, c.tile)).map_or(String::new(), |d| format!(", nearest {d} tiles away"));
            Row {
                title: format!("Taking {} ({owner})", city_name(c)),
                detail: format!("health {}%{walls}{dist}", if max > 0 { hp * 100 / max } else { 100 }),
                icon: Icon::Strength,
                units,
                urgent: false,
            }
        })
        .collect();
    if !rows.is_empty() {
        sections.push(("ON THE ATTACK", rows));
    }
    let rows: Vec<Row> = defend
        .into_iter()
        .map(|(c, units)| {
            let (hp, max) = health(game, &units);
            let threatened = game.cities.get(&c).is_some_and(|c| game.besieged(c.id));
            Row {
                title: format!("Defending {}", city_name(c)),
                detail: format!("health {}%{}", if max > 0 { hp * 100 / max } else { 100 }, if threatened { ", under siege" } else { "" }),
                icon: Icon::Defense,
                units,
                urgent: threatened,
            }
        })
        .collect();
    if !rows.is_empty() {
        sections.push(("DEFENDING", rows));
    }
    let rows: Vec<Row> = other
        .into_iter()
        .map(|(what, units)| Row {
            title: what.to_string(),
            detail: String::new(),
            icon: match what {
                "Hunting" => Icon::Strength,
                "On the move" => Icon::Moves,
                "Exploring" => Icon::Explore,
                "Fortified or asleep" => Icon::Sleep,
                "Waiting for orders" => Icon::Warning,
                _ => Icon::Skip,
            },
            urgent: what == "Waiting for orders",
            units,
        })
        .collect();
    if !rows.is_empty() {
        sections.push(("OTHERS", rows));
    }
    let upkeep = game.upkeep(me);
    commands
        .spawn((
            ForcesList,
            Node {
                position_type: PositionType::Absolute,
                top: px(108),
                right: px(16),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(14)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(10)),
                width: px(560),
                max_height: percent(72),
                ..default()
            },
            ui::frame(),
            GlobalZIndex(3),
        ))
        .with_children(|list| {
            list.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|h| {
                h.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() }).with_children(|t| {
                    t.spawn(ui::heading("Forces", 22.0, palette::ACCENT));
                    t.spawn(ui::text(format!("{} soldiers and ships · upkeep {upkeep} gold a turn", mine.len()), 12.5, palette::TEXT_DIM));
                });
                h.spawn((
                    ui::icon_button(icons.get(Icon::Close), 22.0, Tooltip::title("Close (O)")),
                    observe(|_: On<Activate>, mut open: ResMut<ForcesOpen>| open.0 = false),
                ));
            });
            list.spawn(ui::text("Click a group to select it, then right-click to give it new orders.", 12.5, palette::TEXT_DIM));
            if mine.is_empty() {
                list.spawn(ui::text("No soldiers yet: build some in your cities.", 13.0, palette::TEXT_DIM));
            }
            list.spawn((
                ForcesScroll,
                Node { flex_direction: FlexDirection::Column, row_gap: px(6), overflow: Overflow::scroll_y(), padding: UiRect::right(px(6)), ..default() },
                ScrollPosition(scroll),
            ))
            .with_children(|body| {
                for (title, rows) in sections {
                    body.spawn(Node { margin: UiRect::top(px(4)), ..default() }).with_child(ui::bold(title, 11.5, palette::TEXT_DIM));
                    for row in rows {
                        let picked = !row.units.is_empty() && row.units.iter().all(|u| group.0.contains(u));
                        let units = row.units.clone();
                        body.spawn((
                            bevy::ui_widgets::Button,
                            bevy::picking::hover::Hovered::default(),
                            Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(4),
                                padding: UiRect::new(px(10), px(10), px(6), px(6)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(6)),
                                ..default()
                            },
                            BackgroundColor(if picked { palette::BUTTON_ACTIVE } else { palette::BUTTON }),
                            BorderColor::all(if row.urgent { palette::ACCENT } else { palette::BUTTON_BORDER }),
                            observe(move |_: On<Activate>, session: Res<Session>, mut selected: ResMut<Selected>, mut group: ResMut<Group>, mut rig: ResMut<crate::camera::CameraRig>| {
                                let alive: Vec<UnitId> = units.iter().copied().filter(|u| session.game.units.contains_key(u)).collect();
                                let Some(&first) = alive.first() else { return };
                                selected.0 = Some(first);
                                group.0 = alive;
                                rig.focus = crate::layout::tile_center(&session.game.map.grid, session.game.units[&first].tile);
                            }),
                        ))
                        .with_children(|r| {
                            r.spawn((Node { column_gap: px(8), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|line| {
                                line.spawn(ui::icon(icons.get(row.icon), 18.0));
                                line.spawn((ui::bold(row.title.clone(), 14.0, palette::TEXT), Pickable::IGNORE));
                                line.spawn((ui::text(format!("{} units", row.units.len()), 12.5, palette::TEXT_DIM), Pickable::IGNORE));
                            });
                            if !row.detail.is_empty() {
                                r.spawn((ui::text(row.detail.clone(), 12.0, palette::TEXT_DIM), Pickable::IGNORE));
                            }
                            composition(r, game, &icons, &row.units, 18.0);
                        });
                    }
                }
            });
        });
}
