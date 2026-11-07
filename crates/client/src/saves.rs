//! Saving and loading games: save files in the user's data directory,
//! autosaves at the start of a turn (how often is a setting), the list of
//! saves shown by the main menu and the pause menu, and tearing the world down
//! when leaving a game (for loading another or going back to the main menu).

use crate::frontend::{civ_badge, RulesRes};
use crate::icons::{Icon, Icons};
use crate::loading::{AppState, InGameSet, WorldSource, WorldSpec};
use crate::menu::Menu;
use crate::play::{GameEvent, Notice, Session};
use crate::settings::Settings;
use crate::ui::{self, palette, ButtonState, Tooltip};
use bevy::prelude::*;
use bevy::tasks::IoTaskPool;
use bevy::ui_widgets::{observe, Activate};
use sim::game::{Event, Game};
use sim::save::SaveMeta;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Autosaves kept; older ones are deleted.
const AUTOSAVES_KEPT: usize = 5;

/// Where saves live: `$XDG_DATA_HOME/open4x/saves` or `~/.local/share/open4x/saves`
/// (see [`crate::settings::app_dir`] for other systems).
pub fn saves_dir() -> PathBuf {
    crate::settings::app_dir("XDG_DATA_HOME", ".local/share").unwrap_or_else(|| PathBuf::from("open4x")).join("saves")
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A save on disk.
#[derive(Clone, Debug)]
pub struct SaveEntry {
    pub path: PathBuf,
    pub meta: SaveMeta,
}

/// Every readable save, newest first.
pub fn list_saves() -> Vec<SaveEntry> {
    let Ok(dir) = std::fs::read_dir(saves_dir()) else { return Vec::new() };
    let mut saves: Vec<SaveEntry> = dir
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sav"))
        .filter_map(|path| {
            let mut file = std::io::BufReader::new(std::fs::File::open(&path).ok()?);
            let meta = sim::save::read_meta(&mut file).ok()?;
            Some(SaveEntry { path, meta })
        })
        .collect();
    saves.sort_by(|a, b| b.meta.saved_at.cmp(&a.meta.saved_at).then(b.path.cmp(&a.path)));
    saves
}

/// Writes `game` to a new save file; returns its path.
fn write(game: &Game, me: sim::game::PlayerId, autosave: bool, at: u64) -> Result<PathBuf, String> {
    let meta = game.save_meta(me, autosave, at);
    let dir = saves_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let name = if autosave { format!("autosave-{at}-t{}.sav", game.turn) } else { format!("{}-t{}-{at}.sav", meta.civ, game.turn) };
    let path = dir.join(name);
    // Write to a temporary file first, so a crash never leaves half a save.
    let tmp = path.with_extension("tmp");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
    game.write_save(&meta, &mut out).map_err(|e| e.to_string())?;
    drop(out);
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Saves the running game now.
pub fn save_now(session: &Session) -> Result<PathBuf, String> {
    write(&session.game, session.me, false, now())
}

/// Reads a save into a world to load.
pub fn load(path: &Path, rules: &sim::rules::Rules) -> Result<WorldSpec, String> {
    let mut file = std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?);
    let (meta, game) = Game::read_save(rules.clone(), &mut file).map_err(|e| e.to_string())?;
    Ok(WorldSpec { rules: rules.clone(), source: WorldSource::Saved { game: Box::new(game), me: meta.me } })
}

/// "3 minutes ago", "yesterday", "12 days ago".
fn ago(then: u64) -> String {
    let s = now().saturating_sub(then);
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match s {
        0..60 => "just now".into(),
        60..3600 => plural(s / 60, "minute"),
        3600..86400 => plural(s / 3600, "hour"),
        86400..172800 => "yesterday".into(),
        _ => plural(s / 86400, "day"),
    }
}

pub struct SavesPlugin;

impl Plugin for SavesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SaveList>()
            .add_systems(OnEnter(AppState::Loading), remember_persistent)
            .add_systems(OnExit(AppState::InGame), teardown)
            .add_systems(Update, autosave.after(crate::play::PlaySet).in_set(InGameSet))
            .add_systems(OnEnter(Menu::Load), open_list)
            .add_systems(OnEnter(AppState::LoadGame), open_list)
            .add_systems(OnExit(Menu::Load), close_list)
            .add_systems(OnExit(AppState::LoadGame), close_list)
            .add_systems(Update, rebuild_list.run_if(showing_list));
    }
}

fn showing_list(menu: Res<State<Menu>>, state: Res<State<AppState>>) -> bool {
    *menu.get() == Menu::Load || *state.get() == AppState::LoadGame
}

/// Autosaves at the start of every n-th turn, in the background.
fn autosave(mut events: MessageReader<GameEvent>, session: Res<Session>, settings: Res<Settings>) {
    let started = events.read().any(|GameEvent(e)| matches!(e, Event::TurnStarted { .. }));
    let every = settings.autosave_every;
    if !started || every == 0 || !session.game.turn.is_multiple_of(every) {
        return;
    }
    let game = session.game.clone();
    let me = session.me;
    IoTaskPool::get()
        .spawn(async move {
            match write(&game, me, true, now()) {
                Ok(_) => prune_autosaves(),
                Err(e) => warn!("autosave failed: {e}"),
            }
        })
        .detach();
}

fn prune_autosaves() {
    for old in list_saves().into_iter().filter(|s| s.meta.autosave).skip(AUTOSAVES_KEPT) {
        let _ = std::fs::remove_file(&old.path);
    }
}

// --- Leaving a game ---------------------------------------------------------

/// Entities that exist before a game is built (camera, sun, window, HUD
/// pieces made at startup...). Everything else goes when the game ends.
#[derive(Resource)]
struct Persistent(HashSet<Entity>);

fn remember_persistent(mut commands: Commands, all: Query<Entity>, existing: Option<Res<Persistent>>) {
    // The first game's snapshot holds; later ones would include its leftovers.
    if existing.is_none() {
        commands.insert_resource(Persistent(all.iter().collect()));
    }
}

/// Despawns the world and in-game UI and resets per-game state, so another
/// game can be loaded or the main menu shown.
fn teardown(world: &mut World) {
    let keep = world.remove_resource::<Persistent>().map(|p| p.0).unwrap_or_default();
    let doomed: Vec<Entity> = world
        .query_filtered::<Entity, (Without<ChildOf>, Or<(With<Node>, With<Transform>)>)>()
        .iter(world)
        .filter(|e| !keep.contains(e))
        .collect();
    for e in doomed {
        world.despawn(e);
    }
    world.insert_resource(Persistent(keep));
    world.remove_resource::<Session>();
    world.insert_resource(crate::play::Selected::default());
    world.insert_resource(crate::play::SelectedCity::default());
    world.insert_resource(Notice::default());
    world.insert_resource(crate::news::NewsLog::default());
    world.insert_resource(crate::research_ui::TechTreeOpen::default());
    world.insert_resource(crate::overlay::HoveredTile::default());
    world.remove_resource::<crate::play::DevCursorTile>();
    world.resource_mut::<NextState<Menu>>().set(Menu::Closed);
}

// --- The list of saves ------------------------------------------------------

/// The saves shown, and what the list says (errors, pending deletes).
#[derive(Resource, Default)]
pub struct SaveList {
    saves: Vec<SaveEntry>,
    message: Option<String>,
    /// A save whose delete button was pressed once (press again to delete).
    confirm_delete: Option<PathBuf>,
    dirty: bool,
}

#[derive(Component)]
struct ListRoot;

fn open_list(mut list: ResMut<SaveList>) {
    *list = SaveList { saves: list_saves(), dirty: true, ..default() };
}

/// Loads a save: from the front end or from a running game (which is torn
/// down on the way to the loading screen).
fn load_save(path: &Path, commands: &mut Commands, rules: &RulesRes, list: &mut SaveList, next: &mut NextState<AppState>, menu: &mut NextState<Menu>) {
    match load(path, &rules.0) {
        Ok(spec) => {
            commands.insert_resource(spec);
            menu.set(Menu::Closed);
            next.set(AppState::Loading);
        }
        Err(e) => {
            list.message = Some(format!("Can't load this save: {e}"));
            list.dirty = true;
        }
    }
}

fn delete_save(path: PathBuf) -> impl Fn(On<Activate>, ResMut<SaveList>) {
    move |_: On<Activate>, mut list: ResMut<SaveList>| {
        if list.confirm_delete.as_ref() == Some(&path) {
            if let Err(e) = std::fs::remove_file(&path) {
                list.message = Some(format!("Can't delete: {e}"));
            }
            list.saves = list_saves();
            list.confirm_delete = None;
        } else {
            list.confirm_delete = Some(path.clone());
        }
        list.dirty = true;
    }
}

/// Rebuilds the list panel when it changes. In the front end it's a screen
/// of its own; in a game it's a menu over the map.
fn rebuild_list(
    mut commands: Commands,
    mut list: ResMut<SaveList>,
    rules: Res<RulesRes>,
    icons: Res<Icons>,
    state: Res<State<AppState>>,
    old: Query<Entity, With<ListRoot>>,
) {
    if !list.dirty {
        return;
    }
    list.dirty = false;
    for e in &old {
        commands.entity(e).despawn();
    }
    let in_game = *state.get() == AppState::InGame;
    let root = (
        ListRoot,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, if in_game { 0.45 } else { 0.0 })),
        GlobalZIndex(10),
    );
    commands.spawn(root).with_children(|root| {
        root.spawn(ui::panel_with(Node { width: px(720), max_height: percent(86), ..default() })).with_children(|panel| {
            panel.spawn(ui::title("Load Game"));
            if let Some(m) = &list.message {
                panel.spawn(ui::text(m.clone(), 14.0, palette::DANGER));
            }
            panel
                .spawn((
                    Node {
                        width: percent(100),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6),
                        overflow: Overflow::scroll_y(),
                        flex_shrink: 1.0,
                        min_height: px(0),
                        ..default()
                    },
                    ScrollPosition::default(),
                ))
                .with_children(|rows| {
                    if list.saves.is_empty() {
                        rows.spawn((
                            Node { padding: UiRect::all(px(24)), justify_content: JustifyContent::Center, ..default() },
                            children![ui::text("No saved games yet.", 16.0, palette::TEXT_DIM)],
                        ));
                    }
                    for save in &list.saves {
                        save_row(rows, &rules.0, &icons, save, list.confirm_delete.as_ref() == Some(&save.path));
                    }
                });
            panel.spawn((
                Node { width: px(220), margin: UiRect::top(px(8)), ..default() },
                children![(
                    ui::button("Back"),
                    observe(|_: On<Activate>, state: Res<State<AppState>>, mut next: ResMut<NextState<AppState>>, mut menu: ResMut<NextState<Menu>>| {
                        if *state.get() == AppState::InGame {
                            menu.set(Menu::Pause);
                        } else {
                            next.set(AppState::MainMenu);
                        }
                    }),
                )],
            ));
            panel.spawn(ui::hint(&format!("Saves are kept in {}", saves_dir().display())));
        });
    });
}

fn save_row(rows: &mut ChildSpawnerCommands, rules: &sim::rules::Rules, icons: &Icons, save: &SaveEntry, confirming: bool) {
    let m = &save.meta;
    let civ = rules.civ_id(&m.civ).map(|c| rules.civ(c));
    let stripe = civ.map_or(palette::TEXT_DIM, |c| Color::srgb_u8(c.colors.primary.0, c.colors.primary.1, c.colors.primary.2));
    rows.spawn((
        Node {
            width: percent(100),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: px(12),
            padding: UiRect::axes(px(12), px(8)),
            border: UiRect::new(px(3), px(1), px(1), px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(palette::PANEL_LIGHT),
        BorderColor { left: stripe, ..BorderColor::all(palette::PANEL_BORDER.with_alpha(0.45)) },
    ))
    .with_children(|row| {
        row.spawn(civ_badge(icons, civ, 44.0));
        row.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: px(2), ..default() }).with_children(|t| {
            t.spawn(Node { column_gap: px(8), align_items: AlignItems::Center, ..default() }).with_children(|line| {
                line.spawn(ui::heading(format!("{} · Turn {}", m.civ_name, m.turn), 17.0, palette::TEXT));
                if m.autosave {
                    line.spawn((
                        Node { padding: UiRect::axes(px(6), px(1)), border_radius: BorderRadius::all(px(3)), ..default() },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
                        children![ui::bold("AUTOSAVE", 10.0, palette::TEXT_DIM)],
                    ));
                }
            });
            t.spawn(ui::text(
                format!("{} · {} players · {} × {} · {}", m.leader, m.players, m.map_width, m.map_height, ago(m.saved_at)),
                13.0,
                palette::TEXT_DIM,
            ));
        });
        let path = save.path.clone();
        row.spawn((
            Node { width: px(110), ..default() },
            children![(
                ui::button("Load"),
                observe(
                    move |_: On<Activate>,
                          mut commands: Commands,
                          rules: Res<RulesRes>,
                          mut list: ResMut<SaveList>,
                          mut next: ResMut<NextState<AppState>>,
                          mut menu: ResMut<NextState<Menu>>| {
                        load_save(&path, &mut commands, &rules, &mut list, &mut next, &mut menu)
                    },
                ),
            )],
        ));
        let tip = if confirming { Tooltip::title("Click again to delete") } else { Tooltip::title("Delete this save") };
        let state = ButtonState { active: confirming, ..default() };
        row.spawn((ui::icon_button_with(icons.get(Icon::Close), 36.0, tip, state), observe(delete_save(save.path.clone()))));
    });
}

/// Despawns the in-game load list when the menu closes (the front-end one
/// goes with its state).
fn close_list(mut commands: Commands, old: Query<Entity, With<ListRoot>>) {
    for e in &old {
        commands.entity(e).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_roundtrip_through_the_saves_folder() {
        let dir = std::env::temp_dir().join(format!("open4x-saves-test-{}", std::process::id()));
        std::env::set_var("XDG_DATA_HOME", &dir);
        let rules = sim::rules::Rules::load_default().unwrap();
        let map = sim::mapgen::generate(&rules, &sim::mapgen::MapSettings::default());
        let game = Game::new(rules.clone(), map, 1);
        let manual = write(&game, 0, false, 100).unwrap();
        for t in 0..(AUTOSAVES_KEPT as u64 + 2) {
            write(&game, 0, true, 200 + t).unwrap();
        }
        prune_autosaves();
        let saves = list_saves();
        assert_eq!(saves.iter().filter(|s| s.meta.autosave).count(), AUTOSAVES_KEPT, "old autosaves pruned");
        assert!(saves.iter().any(|s| s.path == manual), "manual saves are kept");
        assert!(saves.windows(2).all(|w| w[0].meta.saved_at >= w[1].meta.saved_at), "newest first");
        let spec = load(&manual, &rules).unwrap();
        assert!(matches!(spec.source, WorldSource::Saved { me: 0, .. }));
        let _ = std::fs::remove_dir_all(dir);
    }
}
