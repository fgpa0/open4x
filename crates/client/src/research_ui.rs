//! Research: a tracker under the top bar (what is being researched, how long
//! it takes, what it unlocks) and the technology tree screen (T), where
//! clicking any technology sets it as the goal.

use crate::icons::{Icon, Icons};
use crate::loading::InGameSet;
use crate::menu::Menu;
use crate::play::{GameEvent, NewsKind, Notice, Session};
use crate::ui::{self, palette, Tooltip};
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use sim::city::Build;
use sim::civ::Effect;
use sim::game::{Command, Event, Game, PlayerId};
use sim::rules::{TechGrant, TechId};

/// Whether the technology tree is open.
#[derive(Resource, Default)]
pub struct TechTreeOpen(pub bool);

#[derive(Component)]
struct Tracker;

#[derive(Component)]
struct TreeScreen;

pub struct ResearchUiPlugin;

impl Plugin for ResearchUiPlugin {
    fn build(&self, app: &mut App) {
        // Dev aid: FOURX_TREE=1 opens the technology tree at the start.
        let open = std::env::var("FOURX_TREE").is_ok_and(|v| !v.is_empty());
        app.insert_resource(TechTreeOpen(open)).add_systems(
            Update,
            (toggle_tree, announce, rebuild_tracker, rebuild_tree).chain().after(crate::play::PlaySet).in_set(InGameSet),
        );
    }
}

/// What a technology unlocks, as builds.
pub fn unlocks(game: &Game, me: PlayerId, tech: TechId) -> Vec<Build> {
    let r = &game.rules;
    // Only our own civ's versions: not other civs' unique units and buildings.
    let civ = game.players[me as usize].civ;
    let units = (0..r.units.units.len() as u8)
        .filter(|&u| r.unit_tech(u) == Some(tech) && r.civ_can_build_unit(civ, u))
        .map(Build::Unit);
    let buildings = (0..r.buildings.buildings.len() as u8)
        .filter(|&b| r.building_tech(b) == Some(tech) && r.civ_can_build_building(civ, b))
        .map(Build::Building);
    units.chain(buildings).collect()
}

fn build_icon(game: &Game, b: Build) -> Icon {
    crate::city_panel::build_icon(game, b)
}

/// What else a technology brings besides builds: improvements Workers can
/// make, resources it reveals and lasting bonuses, with an icon for each and
/// a name for the first two.
fn extras(game: &Game, tech: TechId) -> Vec<(Icon, Option<String>)> {
    let r = &game.rules;
    let mut out = Vec::new();
    for i in 0..r.improvements.improvements.len() as u8 {
        if r.improvement_tech(i) == Some(tech) {
            let def = r.improvement(i);
            out.push((Icon::improvement(&def.art), Some(def.name.clone())));
        }
    }
    for p in sim::improvement::Passage::ALL {
        if r.passage_tech(p) == tech {
            out.push((crate::city_panel::passage_icon(p), Some(p.name().to_string())));
        }
    }
    for res in 0..r.resources.resources.len() as u8 {
        let def = r.resource(res);
        if r.resource_tech(res) == Some(tech) && def.kind == sim::rules::ResourceKind::Strategic {
            out.push((Icon::resource(&def.id), Some(format!("reveals {}", def.name))));
        }
    }
    for e in &r.tech(tech).effects {
        let icon = match e {
            Effect::TileYield { when, .. } => when
                .iter()
                .find_map(|c| match c {
                    sim::civ::TileCond::Improvement(id) => r.improvement_id(id).map(|i| Icon::improvement(&r.improvement(i).art)),
                    _ => None,
                })
                .unwrap_or(Icon::Food),
            Effect::BuildingBonus { building, .. } => Icon::building(building),
            Effect::BuildingProduction { .. } => Icon::Production,
            _ => Icon::Science,
        };
        out.push((icon, None));
    }
    out
}

/// A tech's picture: its first unlock, or a flask.
fn tech_icon(game: &Game, me: PlayerId, tech: TechId) -> Icon {
    match unlocks(game, me, tech).first() {
        Some(&b) => build_icon(game, b),
        None => extras(game, tech).first().map_or(Icon::Science, |e| e.0),
    }
}

fn tech_tooltip(game: &Game, me: PlayerId, tech: TechId) -> Tooltip {
    let def = game.rules.tech(tech);
    let mut body = def.description.clone();
    let names = |list: &mut dyn Iterator<Item = String>| list.collect::<Vec<_>>().join(", ");
    let more = extras(game, tech);
    let unlocked = names(
        &mut unlocks(game, me, tech)
            .into_iter()
            .map(|b| game.build_name(b).to_string())
            .chain(more.iter().filter_map(|e| e.1.clone())),
    );
    if !unlocked.is_empty() {
        body.push_str(&format!("\nUnlocks: {unlocked}"));
    }
    for g in &def.grants {
        body.push_str(match g {
            TechGrant::Embark => "\nLand units can embark onto coast and lakes.",
            TechGrant::DeepWater => "\nShips and embarked units can cross the open ocean.",
        });
    }
    let requires = names(&mut game.rules.tech_requires(tech).iter().map(|&r| game.rules.tech(r).name.clone()));
    if !requires.is_empty() {
        body.push_str(&format!("\nRequires: {requires}"));
    }
    if let Some((before, left)) = era_lock(game, me, game.rules.tech_era(tech)) {
        body.push_str(&format!("\nOpens once every {before} technology is known ({left} to go)."));
    }
    body.push_str(&format!("\n{}. Cost: {} science", game.rules.era(game.rules.tech_era(tech)).name, def.cost));
    if !game.knows(me, tech) {
        let path = game.research_path(me, tech);
        if path.len() > 1 {
            body.push_str(&format!("\nClick to research it and its {} missing prerequisites.", path.len() - 1));
        } else {
            body.push_str("\nClick to research it.");
        }
    }
    Tooltip::new(def.name.clone(), body)
}

/// Researches `tech` (and what it needs); the tree closes once it's chosen
/// and play moves on to whatever needs the player next.
fn set_goal(
    tech: TechId,
) -> impl Fn(On<Activate>, ResMut<Session>, MessageWriter<GameEvent>, ResMut<TechTreeOpen>, MessageWriter<crate::play::NextAttention>) {
    move |_: On<Activate>,
          mut session: ResMut<Session>,
          mut out: MessageWriter<GameEvent>,
          mut open: ResMut<TechTreeOpen>,
          mut next: MessageWriter<crate::play::NextAttention>| {
        if session.command(Command::SetResearch { tech }, &mut out).is_ok() {
            open.0 = false;
            next.write(crate::play::NextAttention);
        }
    }
}

fn toggle_tree(keys: Res<ButtonInput<KeyCode>>, menu: Res<State<Menu>>, mut open: ResMut<TechTreeOpen>) {
    if *menu.get() == Menu::Closed && keys.just_pressed(KeyCode::KeyT) {
        open.0 = !open.0;
    }
}

/// Tells the player about finished technologies, and about anyone they know
/// entering a new era.
fn announce(session: Res<Session>, mut events: MessageReader<GameEvent>, mut notice: ResMut<Notice>) {
    for GameEvent(e) in events.read() {
        if let Event::TechResearched { player, tech } = e {
            let game = &session.game;
            let era = game.rules.tech_era(*tech);
            let firsts = (0..game.rules.tech_count() as TechId).filter(|&t| game.rules.tech_era(t) == era && game.knows(*player, t)).count();
            if era > 0 && firsts == 1 {
                let name = &game.rules.era(era).name;
                if *player == session.me {
                    notice.news(game.turn, NewsKind::Research, Icon::Science, format!("We have entered the {name}"), None);
                } else if game.has_met(session.me, *player) {
                    notice.news(game.turn, NewsKind::Research, Icon::Science, format!("{} has entered the {name}", game.players[*player as usize].name), None);
                }
            }
            if *player != session.me {
                continue;
            }
            let unlocked: Vec<String> = unlocks(game, session.me, *tech).into_iter().map(|b| game.build_name(b).to_string()).collect();
            let mut text = format!("Researched {}", game.rules.tech(*tech).name);
            if !unlocked.is_empty() {
                text.push_str(&format!(": {} unlocked", unlocked.join(", ")));
            }
            notice.news(game.turn, NewsKind::Research, Icon::Science, text, None);
        }
    }
}

/// The research tracker under the top bar (top left).
fn rebuild_tracker(
    mut commands: Commands,
    session: Res<Session>,
    icons: Res<Icons>,
    old: Query<Entity, With<Tracker>>,
    mut built: Local<bool>,
) {
    if *built && !session.is_changed() {
        return;
    }
    *built = true;
    for e in &old {
        commands.entity(e).despawn();
    }
    let game = &session.game;
    let me = session.me;
    let player = &game.players[me as usize];
    let current = player.research_path.first().copied();

    commands
        .spawn((
            Tracker,
            bevy::ui_widgets::Button,
            bevy::picking::hover::Hovered::default(),
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::topbar::HEIGHT + 10.0),
                left: px(12),
                width: px(270),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: UiRect::axes(px(12), px(10)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            ui::frame(),
            Tooltip::new("Research", "Open the technology tree (T)."),
            observe(|_: On<Activate>, mut open: ResMut<TechTreeOpen>| open.0 = true),
        ))
        .with_children(|p| {
            let Some(tech) = current else {
                let all_known = player.techs.iter().all(|&k| k);
                p.spawn((
                    Node { align_items: AlignItems::Center, column_gap: px(10), ..default() },
                    Pickable::IGNORE,
                    children![
                        ui::icon(icons.get(if all_known { Icon::Check } else { Icon::Science }), 36.0),
                        (
                            Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() },
                            Pickable::IGNORE,
                            children![
                                (
                                    ui::heading(if all_known { "All Researched" } else { "Choose Research" }, 15.0, palette::ACCENT),
                                    Pickable::IGNORE
                                ),
                                (
                                    ui::text(
                                        if all_known { "Every technology is known." } else { "Picked for you at turn end if you don't." },
                                        12.0,
                                        palette::TEXT_DIM
                                    ),
                                    Pickable::IGNORE
                                ),
                            ],
                        ),
                    ],
                ));
                return;
            };
            let def = game.rules.tech(tech);
            let done = player.research[tech as usize];
            let turns = game.turns_to_research(me, tech);
            p.spawn((Node { align_items: AlignItems::Center, column_gap: px(10), ..default() }, Pickable::IGNORE)).with_children(|row| {
                row.spawn((
                    Node {
                        width: px(44),
                        height: px(44),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.1, 0.13, 0.18)),
                    BorderColor::all(palette::SCIENCE),
                    Pickable::IGNORE,
                    children![ui::icon(icons.get(tech_icon(game, me, tech)), 30.0)],
                ));
                row.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(4), flex_grow: 1.0, ..default() }, Pickable::IGNORE))
                    .with_children(|col| {
                        col.spawn((
                            Node { justify_content: JustifyContent::SpaceBetween, ..default() },
                            Pickable::IGNORE,
                            children![
                                (ui::bold(def.name.clone(), 15.0, palette::TEXT), Pickable::IGNORE),
                                (
                                    Node { align_items: AlignItems::Center, column_gap: px(3), ..default() },
                                    Pickable::IGNORE,
                                    children![
                                        ui::icon(icons.get(Icon::Turns), 15.0),
                                        (ui::bold(turns.map_or("-".into(), |t| t.to_string()), 13.0, palette::TEXT), Pickable::IGNORE),
                                    ],
                                ),
                            ],
                        ));
                        col.spawn((ui::progress_bar(done as f32 / def.cost as f32, palette::SCIENCE, 7.0), Pickable::IGNORE));
                    });
            });
            let unlocked = unlocks(game, me, tech);
            if !unlocked.is_empty() || player.research_path.len() > 1 {
                p.spawn((Node { align_items: AlignItems::Center, column_gap: px(5), ..default() }, Pickable::IGNORE)).with_children(|row| {
                    for b in unlocked {
                        row.spawn(ui::icon(icons.get(build_icon(game, b)), 20.0));
                    }
                    if let Some(&goal) = player.research_path.last().filter(|_| player.research_path.len() > 1) {
                        row.spawn((
                            ui::text(format!("Goal: {}", game.rules.tech(goal).name), 12.0, palette::TEXT_DIM),
                            Node { margin: UiRect::left(px(6)), ..default() },
                            Pickable::IGNORE,
                        ));
                    }
                });
            }
        });
}

// --- The technology tree ------------------------------------------------------

const CARD_W: f32 = 236.0;
const CARD_H: f32 = 92.0;
const COL_STEP: f32 = 300.0;
const ROW_STEP: f32 = 112.0;
/// Room above the cards for the era names.
const ERA_HEADER: f32 = 48.0;

/// The column of each tech: one right of its latest prerequisite, and never
/// left of where its era starts (each era begins right of the one before).
fn columns(game: &Game) -> Vec<usize> {
    let r = &game.rules;
    let mut col = vec![0; r.tech_count()];
    let mut start = 0;
    for era in 0..r.techs.eras.len() as u8 {
        let mut last = None;
        for t in (0..r.tech_count() as TechId).filter(|&t| r.tech_era(t) == era) {
            let c = r.tech_requires(t).iter().map(|&p| col[p as usize] + 1).max().unwrap_or(0).max(start);
            col[t as usize] = c;
            last = last.max(Some(c));
        }
        start = last.map_or(start, |l| l + 1);
    }
    col
}

/// Top-left corner of each tech's card: its column, rows in file order within a column.
fn layout(game: &Game) -> Vec<Vec2> {
    let cols = columns(game);
    let mut rows = vec![0; cols.iter().max().map_or(0, |&m| m + 1)];
    let mut pos = Vec::new();
    for &col in &cols {
        // Inset so the path badges on the cards' corners aren't clipped.
        pos.push(Vec2::new(12.0, 12.0 + ERA_HEADER) + Vec2::new(col as f32 * COL_STEP, rows[col] as f32 * ROW_STEP));
        rows[col] += 1;
    }
    pos
}

fn rebuild_tree(
    mut commands: Commands,
    session: Res<Session>,
    icons: Res<Icons>,
    open: Res<TechTreeOpen>,
    old: Query<Entity, With<TreeScreen>>,
) {
    if !open.is_changed() && !session.is_changed() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    if !open.0 {
        return;
    }
    let game = &session.game;
    let me = session.me;
    let player = &game.players[me as usize];
    let pos = layout(game);
    let size = pos.iter().fold(Vec2::ZERO, |m, p| m.max(*p)) + Vec2::new(CARD_W, CARD_H);
    let rate = game.science_per_turn(me);
    // Opens scrolled to what's being researched (or the first tech that can
    // be), with a column of what came before on its left.
    let focus = player.research_path.first().copied().or_else(|| (0..game.rules.tech_count() as TechId).find(|&t| game.can_research(me, t)));
    let scroll_x = focus.map_or(0.0, |t| (pos[t as usize].x - COL_STEP).max(0.0));

    commands
        .spawn((
            TreeScreen,
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::topbar::HEIGHT),
                width: percent(100),
                bottom: px(0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.03, 0.72)),
            GlobalZIndex(8),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(14),
                    padding: UiRect::axes(px(28), px(20)),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(10)),
                    max_width: percent(96),
                    max_height: percent(94),
                    ..default()
                },
                ui::frame(),
            ))
            .with_children(|panel| {
                // Header.
                panel
                    .spawn(Node { align_items: AlignItems::Center, column_gap: px(16), ..default() })
                    .with_children(|row| {
                        row.spawn(ui::icon(icons.get(Icon::Science), 34.0));
                        row.spawn((ui::heading("Technology", 28.0, palette::ACCENT), TextShadow::default()));
                        row.spawn((
                            ui::stat(&icons, Icon::Science, format!("+{rate} per turn"), 15.0, palette::SCIENCE),
                            Tooltip::new("Science", "Each city makes one science per citizen plus one; libraries and temples add more."),
                        ));
                        row.spawn(Node { flex_grow: 1.0, ..default() });
                        row.spawn(ui::text("Click a technology to research it; missing prerequisites come first.", 13.0, palette::TEXT_DIM));
                        row.spawn((
                            ui::icon_button(icons.get(Icon::Close), 32.0, Tooltip::title("Close (T or Esc)")),
                            observe(|_: On<Activate>, mut open: ResMut<TechTreeOpen>| open.0 = false),
                        ));
                    });

                // The tree: connectors first, then cards on top.
                panel
                    .spawn((
                        Node { overflow: Overflow::scroll(), min_height: px(0), flex_shrink: 1.0, ..default() },
                        ScrollPosition(Vec2::new(scroll_x, 0.0)),
                    ))
                    .with_children(|scroll| {
                        scroll
                            .spawn(Node {
                                width: px(size.x + 12.0),
                                height: px(size.y + 12.0),
                                flex_shrink: 0.0,
                                ..default()
                            })
                            .with_children(|tree| {
                                era_bands(tree, game, me, &columns(game), size.y + 12.0);
                                for t in 0..game.rules.tech_count() as TechId {
                                    for &r in game.rules.tech_requires(t) {
                                        let color = if game.knows(me, r) { palette::ACCENT.with_alpha(0.7) } else { Color::srgba(0.5, 0.46, 0.4, 0.5) };
                                        connector(tree, pos[r as usize], pos[t as usize], color);
                                    }
                                }
                                for t in 0..game.rules.tech_count() as TechId {
                                    tech_card(tree, game, me, &icons, t, pos[t as usize], player.research_path.iter().position(|&p| p == t));
                                }
                            });
                    });
            });
        });
}

/// Whether `era` is still closed to `me`: the previous era's name and how
/// many of its techs are left.
fn era_lock(game: &Game, me: PlayerId, era: u8) -> Option<(String, usize)> {
    let before = era.checked_sub(1)?;
    let r = &game.rules;
    let left = (0..r.tech_count() as TechId).filter(|&t| r.tech_era(t) == before && !game.knows(me, t)).count();
    (left > 0).then(|| (r.era(before).name.clone(), left))
}

/// A shaded band behind each era's columns, every other one a little
/// lighter, with the era's name on top (the player's own era in gold) and,
/// while it's closed, what opens it.
fn era_bands(tree: &mut ChildSpawnerCommands, game: &Game, me: PlayerId, cols: &[usize], height: f32) {
    let r = &game.rules;
    let mine = game.era(me);
    for era in 0..r.techs.eras.len() as u8 {
        let in_era: Vec<usize> = (0..r.tech_count()).filter(|&t| r.tech_era(t as TechId) == era).map(|t| cols[t]).collect();
        let (Some(&first), Some(&last)) = (in_era.iter().min(), in_era.iter().max()) else { continue };
        let left = first as f32 * COL_STEP - (COL_STEP - CARD_W) / 2.0 + 12.0;
        let width = (last - first + 1) as f32 * COL_STEP;
        let shade = if era % 2 == 0 { 0.035 } else { 0.0 };
        tree.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left.max(0.0)),
                top: px(0),
                width: px(width),
                height: px(height),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::top(px(6)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 0.95, 0.85, shade)),
            Pickable::IGNORE,
        ))
        .with_children(|band| {
            band.spawn((ui::heading(r.era(era).name.clone(), 18.0, if era == mine { palette::ACCENT } else { palette::TEXT_DIM }), Pickable::IGNORE));
            if let Some((before, left)) = era_lock(game, me, era) {
                let tech = if left == 1 { "technology" } else { "technologies" };
                band.spawn((ui::text(format!("Opens after the {before}: {left} {tech} to go"), 11.5, palette::TEXT_DIM), Pickable::IGNORE));
            }
        });
    }
}

/// An elbow line from the right edge of one card to the left edge of another.
fn connector(tree: &mut ChildSpawnerCommands, from: Vec2, to: Vec2, color: Color) {
    let a = from + Vec2::new(CARD_W, CARD_H / 2.0);
    let b = to + Vec2::new(0.0, CARD_H / 2.0);
    let mid = b.x - (COL_STEP - CARD_W) / 2.0;
    let w = 3.0;
    let mut seg = |x: f32, y: f32, width: f32, height: f32| {
        tree.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(x),
                top: px(y),
                width: px(width),
                height: px(height),
                ..default()
            },
            BackgroundColor(color),
            Pickable::IGNORE,
        ));
    };
    seg(a.x, a.y - w / 2.0, mid - a.x + w / 2.0, w);
    seg(mid - w / 2.0, a.y.min(b.y) - w / 2.0, w, (a.y - b.y).abs() + w);
    seg(mid, b.y - w / 2.0, b.x - mid, w);
}

fn tech_card(tree: &mut ChildSpawnerCommands, game: &Game, me: PlayerId, icons: &Icons, t: TechId, at: Vec2, in_path: Option<usize>) {
    let def = game.rules.tech(t);
    let known = game.knows(me, t);
    let available = game.can_research(me, t);
    let progress = game.players[me as usize].research[t as usize];
    let (bg, border) = if known {
        (Color::srgb(0.13, 0.17, 0.11), palette::FOOD.with_alpha(0.7))
    } else if in_path == Some(0) {
        (Color::srgb(0.12, 0.16, 0.22), palette::SCIENCE)
    } else if in_path.is_some() {
        (Color::srgb(0.11, 0.13, 0.17), palette::SCIENCE.with_alpha(0.55))
    } else if available {
        (palette::PANEL_LIGHT, palette::BUTTON_BORDER)
    } else {
        (Color::srgb(0.09, 0.085, 0.08), Color::srgba(0.4, 0.36, 0.3, 0.5))
    };
    let text = if known || available || in_path.is_some() { palette::TEXT } else { palette::TEXT_DIM };

    let mut card = tree.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(at.x),
            top: px(at.y),
            width: px(CARD_W),
            height: px(CARD_H),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            padding: UiRect::axes(px(10), px(8)),
            border: UiRect::all(px(if in_path == Some(0) { 2.5 } else { 1.5 })),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(bg),
        BorderColor::all(border),
        tech_tooltip(game, me, t),
    ));
    if in_path == Some(0) {
        card.insert(BoxShadow::new(palette::SCIENCE.with_alpha(0.4), px(0), px(0), px(0), px(12)));
    }
    if !known {
        // Keeps its own colours (the state of the tech) instead of button styling.
        card.insert((bevy::ui_widgets::Button, ui::OwnStyle, observe(set_goal(t))));
    }
    card.with_children(|c| {
        c.spawn((Node { align_items: AlignItems::Center, column_gap: px(8), ..default() }, Pickable::IGNORE)).with_children(|row| {
            row.spawn(ui::icon(icons.get(tech_icon(game, me, t)), 30.0));
            row.spawn((ui::bold(def.name.clone(), 15.0, text), Node { flex_grow: 1.0, ..default() }, Pickable::IGNORE));
            if known {
                row.spawn(ui::icon(icons.get(Icon::Check), 20.0));
            } else if let Some(turns) = game.turns_to_research(me, t) {
                row.spawn((
                    Node { align_items: AlignItems::Center, column_gap: px(3), ..default() },
                    Pickable::IGNORE,
                    children![ui::icon(icons.get(Icon::Turns), 14.0), (ui::bold(turns.to_string(), 13.0, text), Pickable::IGNORE)],
                ));
            }
        });
        if !known && progress > 0 {
            c.spawn((ui::progress_bar(progress as f32 / def.cost as f32, palette::SCIENCE, 5.0), Pickable::IGNORE));
        }
        c.spawn((Node { align_items: AlignItems::Center, column_gap: px(4), ..default() }, Pickable::IGNORE)).with_children(|row| {
            for b in unlocks(game, me, t) {
                row.spawn(ui::icon(icons.get(build_icon(game, b)), 22.0));
            }
            for (icon, _) in extras(game, t) {
                row.spawn(ui::icon(icons.get(icon), 22.0));
            }
            // Abilities for every unit, like embarking: waves.
            for _ in &def.grants {
                row.spawn(ui::icon(icons.get(Icon::River), 22.0));
            }
            row.spawn((ui::text(format!("{} science", def.cost), 11.5, palette::TEXT_DIM), Node { margin: UiRect::left(px(6)), ..default() }, Pickable::IGNORE));
        });
        // Order in the research path.
        if let Some(i) = in_path {
            c.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(-9),
                    top: px(-9),
                    width: px(20),
                    height: px(20),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1.5)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.1, 0.13, 0.18)),
                BorderColor::all(palette::SCIENCE),
                Pickable::IGNORE,
                children![(ui::bold((i + 1).to_string(), 11.0, palette::TEXT), Pickable::IGNORE)],
            ));
        }
    });
}
