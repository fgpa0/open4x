//! Playing the game: the session (simulation state), selection, giving orders
//! and the turn flow. Everything goes through `sim` commands; the resulting
//! events are published as [`GameEvent`] messages for the renderer.

use crate::camera::CameraRig;
use crate::layout::tile_center;
use crate::loading::{AppState, InGameSet};
use crate::menu::Menu;
use crate::overlay::HoveredTile;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use sim::city::CityId;
use sim::game::{Command, CommandError, Event, Game, Order, PlayerId, UnitId};

/// The running game and which player is on this screen.
#[derive(Resource)]
pub struct Session {
    pub game: Game,
    pub me: PlayerId,
}

/// A simulation event, for systems that react to what happened.
#[derive(Message, Clone, Debug)]
pub struct GameEvent(pub Event);

/// The unit the player has selected.
#[derive(Resource, Default)]
pub struct Selected(pub Option<UnitId>);

/// Several units selected together (Ctrl-click, Shift-drag, double-click):
/// right-click gives them all one order (see `ai::group`). Empty, or the
/// one selected unit, when there's no group. The first is [`Selected`].
#[derive(Resource, Default)]
pub struct Group(pub Vec<UnitId>);

impl Group {
    /// The units an order goes to: the group, or the selected unit alone.
    pub fn members(&self, selected: &Selected) -> Vec<UnitId> {
        if self.0.len() > 1 { self.0.clone() } else { selected.0.into_iter().collect() }
    }
}

/// The city whose panel is open.
#[derive(Resource, Default)]
pub struct SelectedCity(pub Option<CityId>);

/// Request for the selected settler to found a city.
#[derive(Message, Clone, Copy)]
pub struct FoundCityRequest;

/// Request to upgrade the selected unit.
#[derive(Message, Clone, Copy)]
pub struct UpgradeRequest;

/// What a piece of news is about, for filtering the notification log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NewsKind {
    Combat,
    Cities,
    Research,
    Diplomacy,
}

/// An entry in the notification log.
pub struct NewsEntry {
    pub turn: u32,
    pub kind: NewsKind,
    pub icon: crate::icons::Icon,
    pub text: String,
    /// Where it happened (clicking the entry looks there).
    pub at: Option<sim::hex::TileId>,
    /// Bad news for us (attacks, losses, wars declared on us).
    pub alert: bool,
}

/// How important a toast is; the pop-up setting hides the less important ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastLevel {
    /// The answer to something the player just did (always shown).
    Feedback,
    News,
    Alert,
}

/// A message on screen for a few seconds.
pub struct Toast {
    pub id: u64,
    pub icon: crate::icons::Icon,
    pub text: String,
    pub level: ToastLevel,
    /// Seconds left on screen.
    pub timer: f32,
}

/// Messages to the player: toasts under the top bar (warnings such as why an
/// action isn't possible, and news) and the log of everything newsworthy.
#[derive(Resource, Default)]
pub struct Notice {
    pub toasts: Vec<Toast>,
    pub log: Vec<NewsEntry>,
    /// Log entries added since the log was last open.
    pub unread: usize,
    next_id: u64,
}

/// Toasts on screen at once; older ones make way.
const MAX_TOASTS: usize = 4;
/// Log entries kept.
const MAX_LOG: usize = 400;

impl Notice {
    /// A warning in answer to the player's action.
    pub fn show(&mut self, text: impl Into<String>) {
        self.toast(crate::icons::Icon::Warning, text.into(), ToastLevel::Feedback);
    }

    /// Feedback that something worked.
    pub fn inform(&mut self, icon: crate::icons::Icon, text: impl Into<String>) {
        self.toast(icon, text.into(), ToastLevel::Feedback);
    }

    /// News: logged, and shown if the pop-up setting allows.
    pub fn news(&mut self, turn: u32, kind: NewsKind, icon: crate::icons::Icon, text: impl Into<String>, at: Option<sim::hex::TileId>) {
        self.post(NewsEntry { turn, kind, icon, text: text.into(), at, alert: false });
    }

    /// Bad news for us.
    pub fn alert(&mut self, turn: u32, kind: NewsKind, text: impl Into<String>, at: Option<sim::hex::TileId>) {
        self.post(NewsEntry { turn, kind, icon: crate::icons::Icon::Warning, text: text.into(), at, alert: true });
    }

    fn post(&mut self, entry: NewsEntry) {
        let level = if entry.alert { ToastLevel::Alert } else { ToastLevel::News };
        self.toast(entry.icon, entry.text.clone(), level);
        self.log.push(entry);
        self.unread += 1;
        if self.log.len() > MAX_LOG {
            self.log.remove(0);
        }
    }

    fn toast(&mut self, icon: crate::icons::Icon, text: String, level: ToastLevel) {
        let timer = if level == ToastLevel::Feedback { 4.0 } else { 5.5 };
        // The same message again just stays up longer.
        if let Some(t) = self.toasts.iter_mut().find(|t| t.text == text) {
            t.timer = timer;
            return;
        }
        self.next_id += 1;
        self.toasts.push(Toast { id: self.next_id, icon, text, level, timer });
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
    }
}

/// Request to end the turn (from the keyboard or the End Turn button).
#[derive(Message, Clone, Copy)]
pub struct EndTurnRequest;

/// Request to give the selected unit an order (keyboard or unit panel).
#[derive(Message, Clone, Copy)]
pub struct OrderRequest(pub Order);

/// Request for the selected unit (and its group) to build something where
/// it stands: a given work, a road (or a railway over it, see
/// [`road_work`]), or the one in slot `n` of the unit panel's list.
#[derive(Message, Clone, Copy)]
pub enum BuildRequest {
    Work(sim::improvement::Work),
    Road,
    Slot(usize),
}

/// What the selected unit can build, in the order the unit panel lists it
/// (number keys pick from it; roads have their own key).
pub fn build_slots(game: &Game, unit: UnitId) -> Vec<sim::improvement::Work> {
    game.available_works(unit).into_iter().filter(|w| !matches!(w, sim::improvement::Work::Road | sim::improvement::Work::Railroad)).collect()
}

/// What the road key (R) builds for `unit` where it stands: a road, or a
/// railway over the road there.
pub fn road_work(game: &Game, unit: UnitId) -> Option<sim::improvement::Work> {
    use sim::improvement::Work;
    [Work::Road, Work::Railroad].into_iter().find(|&w| game.work_problem(unit, w).is_none())
}

/// Whether a unit improves land (Workers): its automatic order is to work
/// the land rather than explore.
pub fn is_worker(game: &Game, unit: UnitId) -> bool {
    game.units.get(&unit).is_some_and(|u| game.unit_def(u).has(sim::rules::Ability::BuildImprovements))
}

/// Request to select the next unit that needs orders.
#[derive(Message, Clone, Copy)]
pub struct NextUnitRequest;

/// Request to move on to whatever needs the player next: a unit waiting for
/// orders, then research, then a city with nothing to build (see
/// `unit_panel::next_attention`). Sent when the selected unit is done, when a
/// choice is made and when a turn starts.
#[derive(Message, Clone, Copy)]
pub struct NextAttention;

/// Input handling and command processing; renderers run after it.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlaySet;

pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<GameEvent>()
            .add_message::<EndTurnRequest>()
            .add_message::<OrderRequest>()
            .add_message::<NextUnitRequest>()
            .add_message::<NextAttention>()
            .add_message::<FoundCityRequest>()
            .add_message::<UpgradeRequest>()
            .init_resource::<Group>()
            .init_resource::<BoxSelect>()
            .add_message::<BuildRequest>()
            .init_resource::<Selected>()
            .init_resource::<SelectedCity>()
            .init_resource::<Notice>()
            .init_resource::<ClickTracker>()
            .add_systems(OnEnter(AppState::InGame), start_game.after(crate::camera::center_on_map))
            .add_systems(
                Update,
                (
                    (keyboard, mouse_clicks, box_select).run_if(in_state(Menu::Closed)),
                    give_orders,
                    build,
                    found_city,
                    upgrade,
                    end_turn,
                    next_unit,
                    keep_selection_valid,
                )
                    .chain()
                    .in_set(PlaySet)
                    .in_set(InGameSet),
            );
    }
}

impl Session {
    /// Applies a command for this screen's player and publishes the events.
    pub fn command(&mut self, cmd: Command, out: &mut MessageWriter<GameEvent>) -> Result<(), CommandError> {
        let events = self.game.apply(self.me, cmd)?;
        out.write_batch(events.into_iter().map(GameEvent));
        Ok(())
    }

    pub fn publish(events: Vec<Event>, out: &mut MessageWriter<GameEvent>) {
        out.write_batch(events.into_iter().map(GameEvent));
    }
}

/// Centres the camera on a tile (smoothly).
fn look_at(rig: &mut CameraRig, session: &Session, tile: sim::hex::TileId) {
    rig.focus = tile_center(&session.game.map.grid, tile);
}

/// Dev aid: the tile `FOURX_CURSOR=duel` points the virtual cursor at.
#[derive(Resource)]
pub struct DevCursorTile(pub sim::hex::TileId);

pub fn start_game(
    mut commands: Commands,
    mut session: ResMut<Session>,
    mut selected: ResMut<Selected>,
    mut selected_city: ResMut<SelectedCity>,
    mut rig: ResMut<CameraRig>,
    mut group: ResMut<Group>,
    mut out: MessageWriter<GameEvent>,
) {
    let me = session.me;
    group.0.clear();
    // Dev aid: FOURX_TURNS=n lets our non-settler units explore for n turns.
    let auto_turns: u32 = std::env::var("FOURX_TURNS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    if auto_turns > 0 {
        let ids: Vec<UnitId> = session
            .game
            .units
            .values()
            .filter(|u| u.owner == me && session.game.unit_def(u).id != "settler")
            .map(|u| u.id)
            .collect();
        for unit in ids {
            let _ = session.game.apply(me, Command::SetOrder { unit, order: Order::Explore });
        }
        // With FOURX_LOG=1 their news goes into the notification log too.
        let log = std::env::var("FOURX_LOG").is_ok();
        for _ in 0..auto_turns {
            let mut events = ai::settle::settle(&mut session.game, me);
            events.extend(ai::run_standing_orders(&mut session.game, me));
            for p in (0..session.game.players.len() as PlayerId).filter(|&p| p != me) {
                events.extend(ai::take_turn(&mut session.game, p));
            }
            events.extend(session.game.apply(me, Command::EndTurn).unwrap_or_default());
            if log {
                Session::publish(events.into_iter().filter(|e| !matches!(e, Event::UnitMoved { .. })).collect(), &mut out);
            }
        }
    }
    // Dev aid: FOURX_DUEL=1 puts enemy units next to our warrior (one fortified,
    // one wounded) and declares war on them, for testing combat;
    // FOURX_CURSOR=duel hovers the first.
    if std::env::var("FOURX_DUEL").is_ok() {
        let game = &mut session.game;
        let warrior = game.units.values().find(|u| u.owner == me && game.unit_def(u).id == "warrior").map(|u| (u.id, u.tile));
        if let Some((_, at)) = warrior {
            let free: Vec<_> = game
                .map
                .grid
                .neighbors(at)
                .filter(|&t| game.map.land_move_cost(&game.rules, t).is_some() && game.units_at(t).next().is_none())
                .collect();
            let enemy = (me + 1) % game.players.len() as PlayerId;
            for (i, (&tile, kind)) in free.iter().zip(["warrior", "archer", "worker"]).enumerate() {
                let unit = game.spawn_unit(enemy, game.rules.unit_type_id(kind).unwrap(), tile);
                let u = game.units.get_mut(&unit).unwrap();
                match i {
                    0 => u.order = Order::Fortify,
                    1 => u.hp = 45,
                    _ => {}
                }
                if i == 0 {
                    commands.insert_resource(DevCursorTile(tile));
                }
            }
            game.update_visibility(me);
            // For testing combat; FOURX_DUEL=peace stays at peace (to test
            // the declare-war prompt).
            if std::env::var("FOURX_DUEL").is_ok_and(|v| v != "peace") {
                let _ = game.apply(me, Command::DeclareWar { on: enemy });
            }
        }
    }
    // Dev aid: FOURX_NAVY=1 puts our galley, quadrireme and embarked scout on
    // the water nearest our warrior, with an enemy galley beside them, and
    // gives us every technology; the scout is selected. An Admiral leads the
    // frigate (FOURX_NAVY=fleet selects that fleet) and another stands by.
    let mut navy_focus = None;
    let mut navy_select = None;
    if std::env::var("FOURX_NAVY").is_ok() {
        let game = &mut session.game;
        game.players[me as usize].techs.fill(true);
        let start = game.units.values().find(|u| u.owner == me && game.unit_def(u).id == "warrior").map(|u| u.tile);
        let scout = game.units.values().find(|u| u.owner == me && game.unit_def(u).id == "scout").map(|u| u.id);
        if let (Some(at), Some(scout)) = (start, scout) {
            let coast = game.rules.terrain_id("coast");
            let mut water: Vec<_> = game
                .map
                .grid
                .within(at, 8)
                .into_iter()
                .filter(|&t| Some(game.map.tile(t).terrain) == coast && game.map.tile(t).feature.is_none() && game.units_at(t).next().is_none())
                .collect();
            water.sort_by_key(|&t| (game.map.grid.distance(t, at), t));
            let enemy = (me + 1) % game.players.len() as PlayerId;
            let fleet = [(me, "scout"), (me, "galley"), (me, "quadrireme"), (enemy, "galley"), (me, "caravel"), (me, "frigate"), (me, "galleon"), (me, "destroyer"), (me, "battleship")];
            let mut frigate = None;
            for (&tile, (owner, kind)) in water.iter().zip(fleet) {
                match kind {
                    "scout" => game.units.get_mut(&scout).unwrap().tile = tile,
                    _ => {
                        let id = game.spawn_unit(owner, game.rules.unit_type_id(kind).unwrap(), tile);
                        if kind == "frigate" {
                            frigate = Some(id);
                        }
                    }
                }
            }
            // An Admiral with the frigate (sharing its tile), with a level to
            // spend; a second one on his own beside it (to see the flagship).
            if let Some(f) = frigate {
                let tile = game.units[&f].tile;
                let admiral = game.rules.unit_type_id("admiral").unwrap();
                let a = game.spawn_unit(me, admiral, tile);
                game.units.get_mut(&a).unwrap().xp = 12;
                if let Some(&t) = water.iter().find(|&&t| game.units_at(t).next().is_none() && game.map.grid.distance(t, tile) <= 2) {
                    game.spawn_unit(me, admiral, t);
                    let c = tile_center(&game.map.grid, t);
                    info!("FOURX_NAVY: admiral at {:.1},{:.1}", c.x, c.y);
                }
                if std::env::var("FOURX_NAVY").is_ok_and(|v| v == "fleet") {
                    navy_select = Some(f);
                }
            }
            for p in [me, enemy] {
                game.update_visibility(p);
            }
            navy_focus = Some(navy_select.unwrap_or(scout));
            let c = tile_center(&game.map.grid, game.units[&scout].tile);
            info!("FOURX_NAVY: scout at {:.1},{:.1}", c.x, c.y);
        }
    }
    // Dev aid: FOURX_ARMY=1 puts our warrior and four more units (a General
    // among them) together on open ground, with three enemy units next to
    // them and war declared; ours are selected as a group (of every rank,
    // the General with a trait to choose). FOURX_ARMY=enemy points the
    // virtual cursor at theirs (for the combat preview).
    let mut army_focus = None;
    let mut army_group = Vec::new();
    if std::env::var("FOURX_ARMY").is_ok() {
        let game = &mut session.game;
        game.players[me as usize].techs.fill(true);
        let start = game.units.values().find(|u| u.owner == me && game.unit_def(u).id == "warrior").map(|u| u.tile);
        let free = |game: &Game, t: sim::hex::TileId| {
            game.map.land_move_cost(&game.rules, t) == Some(1) && game.map.tile(t).feature.is_none() && game.units_at(t).next().is_none()
        };
        // An open tile two steps from the start, with room around it.
        let site = start.and_then(|s| {
            game.map.grid.within(s, 9).into_iter().filter(|&t| game.map.grid.distance(t, s) >= 2 && free(game, t)).find(|&t| game.map.grid.neighbors(t).filter(|&n| free(game, n)).count() >= 4)
        });
        if let Some(at) = site {
            let near: Vec<_> = game.map.grid.neighbors(at).filter(|&t| free(game, t)).collect();
            let warrior = game.spawn_unit(me, game.rules.unit_type_id("warrior").unwrap(), at);
            let mut ours = vec![warrior];
            for (&tile, kind) in near.iter().zip(["swordsman", "archer", "catapult", "general"]) {
                ours.push(game.spawn_unit(me, game.rules.unit_type_id(kind).unwrap(), tile));
            }
            // The General stands with the warrior.
            if let Some(g) = ours.pop() {
                game.units.get_mut(&g).unwrap().tile = at;
                ours.push(g);
            }
            // As at the start of a turn.
            for &u in &ours {
                let moves = game.turn_moves(&game.units[&u]);
                let unit = game.units.get_mut(&u).unwrap();
                unit.moves = moves;
                unit.moved = false;
            }
            game.units.get_mut(&warrior).unwrap().hp = 70;
            // Veterans of every rank, and a General with a trait to choose.
            for (&u, xp) in ours.iter().zip([40, 15, 80, 0, 30]) {
                game.units.get_mut(&u).unwrap().xp = xp;
            }
            if let (Some(&g), Some(t)) = (ours.last(), game.rules.experience.trait_id("aggressor")) {
                game.units.get_mut(&g).unwrap().traits = vec![t];
            }
            let enemy = (me + 1) % game.players.len() as PlayerId;
            let far: Vec<_> = game.map.grid.within(at, 2).into_iter().filter(|&t| game.map.grid.distance(t, at) == 1 && free(game, t) && start.is_none_or(|s| game.map.grid.distance(t, s) >= 2)).collect();
            if let Some(&t) = far.first() {
                let theirs: Vec<UnitId> = ["spearman", "spearman", "archer"]
                    .iter()
                    .enumerate()
                    .map(|(i, k)| {
                        let tile = if i == 0 { t } else { game.map.grid.neighbors(t).filter(|&n| free(game, n) && game.map.grid.distance(n, at) > 1).nth(i - 1).unwrap_or(t) };
                        game.spawn_unit(enemy, game.rules.unit_type_id(k).unwrap(), tile)
                    })
                    .collect();
                game.players[enemy as usize].techs.fill(true);
                let _ = theirs;
                if std::env::var("FOURX_ARMY").is_ok_and(|v| v == "enemy") {
                    commands.insert_resource(DevCursorTile(t));
                }
            }
            for p in [me, enemy] {
                game.players[p as usize].met[(me + enemy - p) as usize] = true;
                game.update_visibility(p);
            }
            let _ = game.apply(me, Command::DeclareWar { on: enemy });
            army_focus = Some(warrior);
            army_group = ours.clone();
            let c = tile_center(&game.map.grid, at);
            info!("FOURX_ARMY: army at {:.1},{:.1}", c.x, c.y);
        }
    }
    // Dev aid: FOURX_POSTS=1 (with FOURX_TURNS) builds a fort and a
    // watchtower on our first city's land and logs where.
    if std::env::var("FOURX_POSTS").is_ok() {
        let game = &mut session.game;
        if let Some(home) = game.cities.values().find(|c| c.owner == me).map(|c| c.tile) {
            let sites: Vec<_> = game.map.grid.within(home, 3).into_iter().filter(|&t| game.map.grid.distance(t, home) == 2 && game.territory[t as usize] == Some(me) && game.post_fits(t)).collect();
            for (&t, p) in sites.iter().zip([sim::improvement::Passage::Fort, sim::improvement::Passage::Watchtower]) {
                game.build_passage(t, p);
                let c = tile_center(&game.map.grid, t);
                info!("FOURX_POSTS: {} at {:.1},{:.1}", p.name(), c.x, c.y);
            }
        }
    }
    // Dev aid: FOURX_UNITS=1 gives us every technology and 500 gold and puts
    // one of each standard land soldier on the land around our warrior, which
    // is selected (so it shows the Upgrade button).
    let mut units_focus = None;
    if std::env::var("FOURX_UNITS").is_ok() {
        let game = &mut session.game;
        game.players[me as usize].techs.fill(true);
        game.players[me as usize].gold += 500;
        let warrior = game.units.values().find(|u| u.owner == me && game.unit_def(u).id == "warrior").map(|u| (u.id, u.tile));
        if let Some((warrior, at)) = warrior {
            let kinds: Vec<_> = (0..game.rules.units.units.len() as u8)
                .filter(|&k| {
                    let d = game.rules.unit(k);
                    d.class == sim::rules::UnitClass::Military && d.domain == sim::rules::Domain::Land && d.replaces.is_none() && d.id != "warrior" && d.id != "scout"
                })
                .collect();
            let mut land: Vec<_> = game
                .map
                .grid
                .within(at, 5)
                .into_iter()
                .filter(|&t| t != at && game.map.land_move_cost(&game.rules, t).is_some() && game.units_at(t).next().is_none())
                .collect();
            land.sort_by_key(|&t| (game.map.grid.distance(t, at), t));
            for (&tile, &kind) in land.iter().zip(&kinds) {
                game.spawn_unit(me, kind, tile);
                let c = tile_center(&game.map.grid, tile);
                info!("FOURX_UNITS: {} at {:.1},{:.1}", game.rules.unit(kind).id, c.x, c.y);
            }
            game.update_visibility(me);
            units_focus = Some(warrior);
            let c = tile_center(&game.map.grid, at);
            info!("FOURX_UNITS: warrior at {:.1},{:.1}", c.x, c.y);
        }
    }
    // Dev aid: FOURX_WORKS=1 gives us every technology and builds an
    // improvement on each tile of our first city that can take one, roads to
    // its edge, and a Worker next to it (selected), for looking at the models.
    let mut works_focus = None;
    if std::env::var("FOURX_WORKS").is_ok() {
        let game = &mut session.game;
        game.players[me as usize].techs.fill(true);
        if let Some(city) = game.cities.values().find(|c| c.owner == me).map(|c| (c.id, c.tile)) {
            let worker = game.spawn_unit(me, game.rules.unit_type_id("worker").unwrap(), city.1);
            let n = game.rules.improvements.improvements.len() as u8;
            let tiles: Vec<_> = game.map.grid.within(city.1, 3).into_iter().filter(|&t| game.district(t) == Some(city.0) && t != city.1).collect();
            // Fish on the water, if there is any and no fishing ground yet, to show the boats.
            let fish = game.rules.resource_id("fish");
            if !tiles.iter().any(|&t| game.map.tile(t).resource == fish) {
                if let Some(&t) = tiles.iter().find(|&&t| game.map.is_water(&game.rules, t) && game.map.tile(t).resource.is_none()) {
                    game.map.tiles[t as usize].resource = fish;
                }
            }
            // And oil on land (flat if there is any), for the Oil Well.
            let oil = game.rules.resource_id("oil");
            if !tiles.iter().any(|&t| game.map.tile(t).resource == oil) {
                let open = |t: sim::hex::TileId| game.map.land_move_cost(&game.rules, t).is_some() && game.map.tile(t).resource.is_none();
                let flat = |t: sim::hex::TileId| open(t) && game.map.tile(t).elevation == sim::map::Elevation::Flat;
                if let Some(&t) = tiles.iter().rev().find(|&&t| flat(t)).or_else(|| tiles.iter().rev().find(|&&t| open(t))) {
                    game.map.tiles[t as usize].resource = oil;
                    let c = tile_center(&game.map.grid, t);
                    info!("FOURX_WORKS: oil at {:.1},{:.1}", c.x, c.y);
                }
            }
            for (k, &t) in tiles.iter().enumerate() {
                game.units.get_mut(&worker).unwrap().tile = t;
                // Resources' own improvements first, then the rest in turn.
                let wanted = game.visible_resource(me, t).and_then(|r| game.rules.resource_improvement(r));
                let pick = wanted.into_iter().chain((0..n).map(|i| (i + k as u8) % n)).find(|&i| game.work_problem(worker, sim::improvement::Work::Improvement(i)).is_none());
                if let Some(i) = pick {
                    if game.rules.improvement(i).water {
                        let c = tile_center(&game.map.grid, t);
                        info!("FOURX_WORKS: {} at {:.1},{:.1}", game.rules.improvement(i).name, c.x, c.y);
                    }
                    game.works[t as usize].improvement = Some(i);
                    if game.work_clears(me, t, sim::improvement::Work::Improvement(i)) {
                        game.map.tiles[t as usize].feature = None;
                    }
                }
                if k % 3 == 0 && !game.map.is_water(&game.rules, t) {
                    game.works[t as usize].road = true;
                    // Every other road carries a railway.
                    game.works[t as usize].rail = k % 2 == 0;
                }
            }
            game.rail_stations(me);
            let free = game.map.grid.neighbors(city.1).find(|&t| game.units_at(t).next().is_none() && game.map.land_move_cost(&game.rules, t).is_some());
            game.units.get_mut(&worker).unwrap().tile = free.unwrap_or(city.1);
            works_focus = Some(worker);
            let c = tile_center(&game.map.grid, city.1);
            info!("FOURX_WORKS: city at {:.1},{:.1}", c.x, c.y);
        }
    }
    let merchant_focus = crate::merchants::dev_merchant(&mut session.game, me);
    // Dev aid: FOURX_PASSAGES=1 gives us every technology, digs a canal two
    // tiles long and the tunnel nearest our first unit, wherever they'd fit,
    // and builds the nearest bridge, with a scout on it and a galley beneath.
    let mut passage_focus = None;
    let mut dug = Vec::new();
    if std::env::var("FOURX_PASSAGES").is_ok() {
        let game = &mut session.game;
        game.players[me as usize].techs.fill(true);
        let start = game.units.values().filter(|u| u.owner == me).min_by_key(|u| u.id).map(|u| u.tile);
        if let Some(at) = start {
            let mut tiles: Vec<_> = (0..game.map.tiles.len() as sim::hex::TileId).collect();
            tiles.sort_by_key(|&t| (game.map.grid.distance(t, at), t));
            // A shore tile with land behind it to dig on into.
            let inland = |t: sim::hex::TileId| game.map.grid.neighbors(t).find(|&n| game.diggable(n) && !game.map.grid.neighbors(n).any(|m| game.map.is_water(&game.rules, m)));
            let canal = tiles.iter().copied().find(|&t| game.canal_fits(t) && inland(t).is_some());
            let canal_end = canal.and_then(inland);
            // A mountain with land on two opposite sides, as a tunnel would join.
            let land = |h: sim::hex::Hex| game.map.grid.tile(h).is_some_and(|n| game.map.land_move_cost(&game.rules, n).is_some());
            let through = |t: sim::hex::TileId| (0..3).any(|d| land(game.map.grid.hex(t).neighbor(d)) && land(game.map.grid.hex(t).neighbor(d + 3)));
            let tunnel = tiles.iter().copied().find(|&t| game.tunnel_fits(t) && !game.has_tunnel(t) && through(t));
            let bridge = tiles.iter().copied().find(|&t| game.bridge_fits(t) && !game.has_bridge(t));
            for (tile, name) in [(canal, "canal"), (tunnel, "tunnel"), (bridge, "bridge")] {
                let Some(t) = tile else { continue };
                let w = &mut game.works[t as usize];
                match name {
                    "canal" => {}
                    "bridge" => (w.bridge, w.road) = (true, true),
                    _ => (w.tunnel, w.road) = (true, true),
                }
                if name == "bridge" {
                    for unit in ["scout", "galley"] {
                        if let Some(kind) = game.rules.unit_type_id(unit) {
                            game.spawn_unit(me, kind, t);
                        }
                    }
                }
                if name == "canal" {
                    let water = game.rules.canal_water();
                    for d in [Some(t), canal_end].into_iter().flatten() {
                        dug.extend(game.terraform(d, water));
                    }
                }
                // A scout next door keeps it in view.
                let beside = game.map.grid.neighbors(t).find(|&n| game.map.land_move_cost(&game.rules, n).is_some() && n != t);
                if let (Some(n), Some(scout)) = (beside, game.rules.unit_type_id("scout")) {
                    game.spawn_unit(me, scout, n);
                }
                let c = tile_center(&game.map.grid, t);
                info!("FOURX_PASSAGES: {name} at {:.1},{:.1}", c.x, c.y);
                passage_focus = passage_focus.or(Some(t));
            }
            game.rebuild_passages();
            game.update_visibility(me);
        }
    }
    // Dev aid: FOURX_DIG=tile,tile... digs those tiles out into water once
    // the world is shown, as a finished canal does.
    if let Ok(list) = std::env::var("FOURX_DIG") {
        let game = &mut session.game;
        let water = game.rules.canal_water();
        for t in list.split(',').filter_map(|s| s.trim().parse::<sim::hex::TileId>().ok()) {
            if (t as usize) < game.map.tiles.len() {
                dug.extend(game.terraform(t, water));
            }
        }
    }
    let mut events = ai::run_standing_orders(&mut session.game, me);
    events.extend(dug);
    Session::publish(events, &mut out);
    // Dev aid: FOURX_RAZE=1 (with FOURX_TURNS) takes a rival's city with a
    // warrior put next to it, to see the keep-or-raze prompt.
    if std::env::var("FOURX_RAZE").is_ok() {
        let game = &mut session.game;
        let target = game.cities.values().filter(|c| c.owner != me).min_by_key(|c| (c.capital_of.is_some(), c.id)).map(|c| (c.id, c.owner, c.tile));
        let warrior = game.rules.unit_type_id("warrior");
        if let (Some((city, them, tile)), Some(warrior)) = (target, warrior) {
            game.players[me as usize].met[them as usize] = true;
            game.players[them as usize].met[me as usize] = true;
            let _ = game.apply(me, Command::DeclareWar { on: them });
            let beside = game.map.grid.neighbors(tile).find(|&n| game.map.land_move_cost(&game.rules, n).is_some() && game.units_at(n).next().is_none());
            if let Some(n) = beside {
                let unit = game.spawn_unit(me, warrior, n);
                game.cities.get_mut(&city).expect("exists").hp = 1;
                game.update_visibility(me);
                if let Ok(events) = game.apply(me, Command::Attack { unit, tile }) {
                    Session::publish(events, &mut out);
                }
                selected_city.0 = Some(city);
            }
        }
    }
    // Dev aid: FOURX_OPEN_CITY=1 opens our first city.
    if std::env::var("FOURX_OPEN_CITY").is_ok() {
        selected_city.0 = session.game.cities.values().find(|c| c.owner == me).map(|c| c.id);
        // FOURX_BUY_TILES=1 also shows the tiles it could buy.
        if std::env::var("FOURX_BUY_TILES").is_ok() {
            commands.insert_resource(crate::land::BuyLand(true));
        }
    }
    // Dev aid: FOURX_MILITARY=1 sends our units to defend our first city
    // and opens the Forces screen. Use with FOURX_TURNS.
    if std::env::var("FOURX_MILITARY").is_ok() {
        let game = &mut session.game;
        if let Some(city) = game.cities.values().find(|c| c.owner == me).map(|c| c.id) {
            let soldiers: Vec<UnitId> = game.units.values().filter(|u| u.owner == me && game.unit_def(u).class == sim::rules::UnitClass::Military).map(|u| u.id).collect();
            for unit in soldiers {
                let _ = game.apply(me, Command::SetOrder { unit, order: Order::Defend { city } });
            }
        }
        commands.insert_resource(crate::forces::ForcesOpen(true));
    }
    // Dev aid: FOURX_GOLD=n sets our treasury (for testing purchases).
    if let Some(gold) = std::env::var("FOURX_GOLD").ok().and_then(|g| g.parse().ok()) {
        session.game.players[me as usize].gold = gold;
    }
    // Look at our capital (a loaded game), or else our first unit (the
    // settler). Nothing starts selected.
    let first = session.game.units.values().filter(|u| u.owner == me).min_by_key(|u| u.id).map(|u| (u.id, u.tile));
    let mine = || session.game.cities.values().filter(|c| c.owner == me);
    let capital = mine().find(|c| c.capital_of == Some(me)).or_else(|| mine().min_by_key(|c| c.id)).map(|c| c.tile);
    if let Some(tile) = capital.or(first.map(|f| f.1)) {
        let distance = rig.distance;
        crate::camera::snap(&mut rig, tile_center(&session.game.map.grid, tile), distance);
    }
    if std::env::var("FOURX_DUEL").is_ok() {
        // Our warrior.
        selected.0 = session.game.units.values().find(|u| u.owner == me && session.game.unit_def(u).id == "warrior").map(|u| u.id);
    }
    if let Some(tile) = passage_focus {
        let distance = rig.distance;
        crate::camera::snap(&mut rig, tile_center(&session.game.map.grid, tile), distance);
    }
    if let Some(unit) = works_focus.or(units_focus).or(merchant_focus).or(army_focus) {
        selected.0 = Some(unit);
        group.0 = army_group;
        let distance = rig.distance;
        crate::camera::snap(&mut rig, tile_center(&session.game.map.grid, session.game.units[&unit].tile), distance);
    }
    if let Some(scout) = navy_focus {
        selected.0 = Some(scout);
        let distance = rig.distance;
        crate::camera::snap(&mut rig, tile_center(&session.game.map.grid, session.game.units[&scout].tile), distance);
    }
    if let Some(c) = selected_city.0.and_then(|c| session.game.cities.get(&c)) {
        selected.0 = None;
        let distance = rig.distance;
        crate::camera::snap(&mut rig, tile_center(&session.game.map.grid, c.tile), distance);
    }
}

fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut orders: MessageWriter<OrderRequest>,
    mut end: MessageWriter<EndTurnRequest>,
    mut next: MessageWriter<NextUnitRequest>,
    mut found: MessageWriter<FoundCityRequest>,
    mut builds: MessageWriter<BuildRequest>,
    mut upgrades: MessageWriter<UpgradeRequest>,
) {
    if keys.just_pressed(KeyCode::KeyB) {
        found.write(FoundCityRequest);
    }
    if keys.just_pressed(KeyCode::KeyU) {
        upgrades.write(UpgradeRequest);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        builds.write(BuildRequest::Road);
    }
    let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7];
    if let Some(n) = digits.iter().position(|&k| keys.just_pressed(k)) {
        builds.write(BuildRequest::Slot(n));
    }
    let order = if keys.just_pressed(KeyCode::KeyE) {
        Some(Order::Explore)
    } else if keys.just_pressed(KeyCode::KeyF) {
        Some(Order::Fortify)
    } else if keys.just_pressed(KeyCode::KeyZ) {
        // Z for "zzz" (S pans the camera).
        Some(Order::Sleep)
    } else if keys.just_pressed(KeyCode::Space) {
        Some(Order::Skip)
    } else {
        None
    };
    if let Some(o) = order {
        orders.write(OrderRequest(o));
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        end.write(EndTurnRequest);
    }
    if keys.just_pressed(KeyCode::Period) || keys.just_pressed(KeyCode::Tab) {
        next.write(NextUnitRequest);
    }
}

/// Distinguishes clicks from drags (the camera pans on drag).
#[derive(Resource, Default)]
struct ClickTracker {
    left: Option<Vec2>,
    right: Option<Vec2>,
    /// The last left-click on a unit of ours, and when (for double-clicks).
    last: Option<(UnitId, f64)>,
}

/// Seconds between the two clicks of a double-click.
const DOUBLE_CLICK: f64 = 0.35;

const CLICK_SLOP: f32 = 5.0;

fn mouse_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    hover: Res<HoverMap>,
    nodes: Query<(), With<Node>>,
    hovered: Res<HoveredTile>,
    mut tracker: ResMut<ClickTracker>,
    mut session: ResMut<Session>,
    mut selected: ResMut<Selected>,
    keys: Res<ButtonInput<KeyCode>>,
    mut city: ResMut<SelectedCity>,
    mut notice: ResMut<Notice>,
    mut prompt: ResMut<crate::diplomacy::WarPrompt>,
    placing: Res<crate::placement::Placing>,
    (mut merchants, mut group, time): (ResMut<crate::merchants::MerchantScreen>, ResMut<Group>, Res<Time>),
    (camera, terrain): (Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>, Res<crate::terrain::Terrain>),
    mut out: MessageWriter<GameEvent>,
) {
    // Choosing where to dig takes the clicks (see `placement.rs`).
    if placing.0.is_some() {
        tracker.left = None;
        tracker.right = None;
        return;
    }
    let Some(cursor) = window.cursor_position() else { return };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if mouse.just_pressed(MouseButton::Left) {
        // Shift-drags draw a selection box instead (see `box_select`).
        tracker.left = (!shift && !crate::ui::pointer_over_ui(&hover, &nodes)).then_some(cursor);
    }
    if mouse.just_pressed(MouseButton::Right) {
        tracker.right = (!crate::ui::pointer_over_ui(&hover, &nodes)).then_some(cursor);
    }
    let clicked = |press: Option<Vec2>| press.is_some_and(|p| p.distance(cursor) < CLICK_SLOP);

    if mouse.just_released(MouseButton::Left) && clicked(tracker.left.take()) {
        let Some(tile) = hovered.0 else { return };
        let me = session.me;
        let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let mine: Vec<UnitId> = session.game.units_at(tile).filter(|u| u.owner == me).map(|u| u.id).collect();
        let my_city = session.game.city_at(tile).filter(|c| c.owner == me).map(|c| c.id);
        // A city with no units of ours on it opens its panel, and anywhere
        // else clears the selection (Ctrl keeps the group).
        if mine.is_empty() {
            if !ctrl {
                city.0 = my_city;
                selected.0 = None;
                group.0.clear();
            }
            return;
        }
        let now = time.elapsed_secs_f64();
        let pick = match selected.0 {
            Some(cur) if mine.contains(&cur) && !ctrl => mine.iter().cycle().skip_while(|&&u| u != cur).nth(1).copied(),
            _ => mine.first().copied(),
        };
        let Some(pick) = pick else { return };
        if ctrl {
            // Ctrl-click adds a unit to the group, or takes it out.
            let mut members = group.members(&selected);
            match members.iter().position(|&u| u == pick) {
                Some(i) => {
                    members.remove(i);
                }
                None => members.push(pick),
            }
            selected.0 = members.first().copied();
            group.0 = members;
        } else if tracker.last.is_some_and(|(u, t)| u == pick && now - t < DOUBLE_CLICK) {
            // Double-click: every unit of that kind of ours on screen.
            let (camera, cam_tf) = *camera;
            let kind = session.game.units[&pick].kind;
            let size = window.size();
            let game = &session.game;
            let mut all: Vec<UnitId> = game
                .units
                .values()
                .filter(|u| u.owner == me && u.kind == kind)
                .filter(|u| on_screen(camera, cam_tf, &terrain, game, u.tile, size).is_some())
                .map(|u| u.id)
                .collect();
            all.sort_by_key(|&u| u != pick);
            selected.0 = Some(pick);
            group.0 = all;
            tracker.last = None;
            return;
        } else {
            selected.0 = Some(pick);
            group.0.clear();
        }
        tracker.last = Some((pick, now));
    }
    if mouse.just_released(MouseButton::Right) && clicked(tracker.right.take()) {
        let Some(to) = hovered.0 else { return };
        let units = group.members(&selected);
        let Some(&unit) = units.first() else { return };
        // Merchants can't be moved: where they go is their trade route.
        if units.len() == 1 && session.game.units.get(&unit).is_some_and(|u| session.game.is_merchant(u)) {
            merchants.choose(unit);
            return;
        }
        let me = session.me;
        let game = &session.game;
        // Attacking (or sending units to take) a civ we're at peace with asks first.
        let peace = units.iter().find_map(|&u| peace_blocks(game, me, u, to)).or_else(|| {
            let c = game.city_at(to).filter(|c| c.owner != me && game.has_met(me, c.owner) && !game.at_war(me, c.owner))?;
            units.iter().any(|&u| game.take_problem(u, c.id).is_none()).then_some(c.owner)
        });
        if let Some(on) = peace {
            prompt.0 = Some(crate::diplomacy::PendingAttack { units, tile: to, on });
            return;
        }
        if let Err(e) = order_units(&mut session, &units, to, &mut out) {
            notice.show(e);
        }
    }
}

/// Where `tile` is on screen, if it's in view.
fn on_screen(camera: &Camera, cam_tf: &GlobalTransform, terrain: &crate::terrain::Terrain, game: &Game, tile: sim::hex::TileId, size: Vec2) -> Option<Vec2> {
    let c = tile_center(&game.map.grid, tile);
    let ground = Vec3::new(c.x, terrain.heights.sample(c).max(0.0), c.y);
    let p = camera.world_to_viewport(cam_tf, ground).ok()?;
    (p.x >= 0.0 && p.y >= 0.0 && p.x <= size.x && p.y <= size.y).then_some(p)
}

/// A Shift-drag on the map: where it started.
#[derive(Resource, Default)]
pub struct BoxSelect {
    pub from: Option<Vec2>,
    pub to: Vec2,
}

/// Shift and drag selects every unit of ours inside the box (soldiers only,
/// when there are any).
fn box_select(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window>,
    hover: Res<HoverMap>,
    nodes: Query<(), With<Node>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    terrain: Res<crate::terrain::Terrain>,
    session: Res<Session>,
    mut drag: ResMut<BoxSelect>,
    mut selected: ResMut<Selected>,
    mut group: ResMut<Group>,
) {
    let Some(cursor) = window.cursor_position() else { return };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if mouse.just_pressed(MouseButton::Left) && shift && !crate::ui::pointer_over_ui(&hover, &nodes) {
        drag.from = Some(cursor);
    }
    if drag.from.is_some() {
        drag.to = cursor;
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some(from) = drag.from.take() else { return };
    if from.distance(cursor) < CLICK_SLOP {
        return;
    }
    let (lo, hi) = (from.min(cursor), from.max(cursor));
    let (camera, cam_tf) = *camera;
    let game = &session.game;
    let size = window.size();
    let inside: Vec<&sim::game::Unit> = game
        .units
        .values()
        .filter(|u| u.owner == session.me && game.players[session.me as usize].sees(u.tile))
        .filter(|u| on_screen(camera, cam_tf, &terrain, game, u.tile, size).is_some_and(|p| p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y))
        .collect();
    let soldiers: Vec<UnitId> = inside.iter().filter(|u| game.unit_def(u).class == sim::rules::UnitClass::Military).map(|u| u.id).collect();
    let picked = if soldiers.is_empty() { inside.iter().map(|u| u.id).collect() } else { soldiers };
    if picked.is_empty() {
        return;
    }
    selected.0 = picked.first().copied();
    group.0 = picked;
}

/// A right-click order on `to`, for one unit or a group. A unit that can
/// attack what's there does so now; an enemy city further off is to be
/// taken, one of our cities defended (soldiers only), an enemy unit hunted;
/// anywhere else they go (a group spreads out around the tile).
pub fn order_units(session: &mut Session, units: &[UnitId], to: sim::hex::TileId, out: &mut MessageWriter<GameEvent>) -> Result<(), String> {
    let me = session.me;
    if let &[unit] = units {
        let game = &session.game;
        let soldier = game.units.get(&unit).is_some_and(|u| game.unit_def(u).class == sim::rules::UnitClass::Military);
        let enemy_there = game.players[me as usize].sees(to) && game.units_at(to).any(|u| u.owner != me && !(game.is_merchant(u) && !game.at_war(me, u.owner)));
        if game.attack_target(unit, to).is_ok() {
            return session.command(Command::Attack { unit, tile: to }, out).map_err(|e| format!("Can't attack: {e}"));
        }
        let city_order = ai::group::city_order(game, me, to).filter(|_| soldier);
        if city_order.is_none() {
            return if enemy_there {
                attack_or_hunt(session, unit, to, out).map_err(|e| format!("Can't attack: {e}"))
            } else {
                move_or_approach(session, unit, to, out).map_err(|e| format!("Can't move there: {e}"))
            };
        }
    }
    let events = ai::group::command(&mut session.game, me, units, to)?;
    Session::publish(events, out);
    Ok(())
}

/// Attacks `tile` if `unit` can now; otherwise sets it hunting the enemy unit
/// there, to chase it down and attack it.
pub fn attack_or_hunt(session: &mut Session, unit: UnitId, tile: sim::hex::TileId, out: &mut MessageWriter<GameEvent>) -> Result<(), String> {
    let game = &session.game;
    let why = match game.attack_target(unit, tile) {
        Ok(_) => return session.command(Command::Attack { unit, tile }, out).map_err(|e| e.to_string()),
        Err(e) => e,
    };
    match game.hunt_target(session.me, tile).filter(|_| game.city_at(tile).is_none()) {
        Some(target) => session.command(Command::Hunt { unit, target }, out).map_err(|e| e.to_string()),
        // A city out of reach: go next to it, to attack and take it from there.
        None => match game.approach_city(unit, tile) {
            Some((to, _)) => session.command(Command::MoveUnit { unit, to }, out).map_err(|e| e.to_string()),
            None => Err(why.to_string()),
        },
    }
}

/// Moves `unit` to `to`, or next to it when it's a foreign city the unit
/// can't enter (a ship can't sail into it, say).
fn move_or_approach(session: &mut Session, unit: UnitId, to: sim::hex::TileId, out: &mut MessageWriter<GameEvent>) -> Result<(), CommandError> {
    match session.command(Command::MoveUnit { unit, to }, out) {
        Err(e) => match session.game.approach_city(unit, to) {
            Some((next, _)) => session.command(Command::MoveUnit { unit, to: next }, out),
            None => Err(e),
        },
        ok => ok,
    }
}

/// The player whose peace is all that stops `unit` attacking `tile`, or
/// hunting the unit there (doing so means declaring war on them).
pub fn peace_blocks(game: &Game, me: PlayerId, unit: UnitId, tile: sim::hex::TileId) -> Option<PlayerId> {
    game.peace_blocks_attack(unit, tile).or_else(|| {
        let on = game.units[&game.hunt_target(me, tile)?].owner;
        let def = game.unit_def(game.units.get(&unit)?);
        let fighter = def.class == sim::rules::UnitClass::Military && (def.strength > 0 || def.ranged > 0);
        (fighter && game.city_at(tile).is_none() && !game.at_war(me, on)).then_some(on)
    })
}

fn give_orders(
    mut requests: MessageReader<OrderRequest>,
    mut session: ResMut<Session>,
    selected: Res<Selected>,
    group: Res<Group>,
    mut merchants: ResMut<crate::merchants::MerchantScreen>,
    mut out: MessageWriter<GameEvent>,
) {
    for &OrderRequest(order) in requests.read() {
        let units = group.members(&selected);
        if let &[unit] = units.as_slice() {
            // A merchant's automatic order is its trade route: choose one.
            if order == Order::Explore && session.game.units.get(&unit).is_some_and(|u| session.game.is_merchant(u)) {
                merchants.choose(unit);
                continue;
            }
        }
        let mut automatic = false;
        for unit in units {
            // Workers' automatic order is to improve the land.
            let order = if order == Order::Explore && is_worker(&session.game, unit) { Order::Automate } else { order };
            automatic |= session.command(Command::SetOrder { unit, order }, &mut out).is_ok() && matches!(order, Order::Explore | Order::Automate);
        }
        if automatic {
            // Start exploring (or working) right away rather than next turn.
            let me = session.me;
            let events = ai::run_standing_orders(&mut session.game, me);
            Session::publish(events, &mut out);
        }
    }
}

/// Starts a build for the selected unit. A Worker the player tells what to build is no longer automated.
fn build(
    mut requests: MessageReader<BuildRequest>,
    mut session: ResMut<Session>,
    selected: Res<Selected>,
    mut notice: ResMut<Notice>,
    mut out: MessageWriter<GameEvent>,
) {
    for &request in requests.read() {
        let Some(unit) = selected.0 else { continue };
        let work = match request {
            BuildRequest::Work(w) => w,
            BuildRequest::Road => match road_work(&session.game, unit) {
                Some(w) => w,
                None => sim::improvement::Work::Road,
            },
            BuildRequest::Slot(n) => match build_slots(&session.game, unit).get(n) {
                Some(&w) => w,
                None => continue,
            },
        };
        if !session.game.can_do_work(unit, work) {
            continue;
        }
        for u in [unit] {
            if !session.game.can_do_work(u, work) {
                continue;
            }
            if matches!(session.game.units[&u].order, Order::Automate | Order::AutoBuild(_)) {
                let _ = session.command(Command::SetOrder { unit: u, order: Order::None }, &mut out);
            }
            if let Err(e) = session.command(Command::Build { unit: u, work }, &mut out) {
                if u == unit {
                    notice.show(format!("Can't build that here: {e}."));
                }
            }
        }
    }
}

fn found_city(
    mut requests: MessageReader<FoundCityRequest>,
    mut session: ResMut<Session>,
    selected: Res<Selected>,
    mut city: ResMut<SelectedCity>,
    mut notice: ResMut<Notice>,
    mut out: MessageWriter<GameEvent>,
) {
    if requests.read().count() == 0 {
        return;
    }
    let Some(unit) = selected.0 else { return };
    let before: Vec<CityId> = session.game.cities.keys().copied().collect();
    match session.command(Command::FoundCity { unit }, &mut out) {
        // Open the new city.
        Ok(()) => city.0 = session.game.cities.keys().copied().find(|c| !before.contains(c)),
        Err(e) => notice.show(format!("Can't found a city: {e}")),
    }
}

fn upgrade(
    mut requests: MessageReader<UpgradeRequest>,
    mut session: ResMut<Session>,
    selected: Res<Selected>,
    mut notice: ResMut<Notice>,
    mut out: MessageWriter<GameEvent>,
) {
    if requests.read().count() == 0 {
        return;
    }
    let Some(unit) = selected.0.filter(|u| session.game.units.get(u).is_some_and(|u| u.owner == session.me)) else { return };
    let from = session.game.unit_def(&session.game.units[&unit]).name.clone();
    match session.command(Command::Upgrade { unit }, &mut out) {
        Ok(()) => {
            let u = &session.game.units[&unit];
            let to = session.game.unit_def(u).name.clone();
            notice.inform(crate::icons::Icon::Upgrade, format!("{from} upgraded to {to}."));
        }
        Err(e) => notice.show(format!("Can't upgrade: {e}.")),
    }
}

fn end_turn(
    mut requests: MessageReader<EndTurnRequest>,
    mut session: ResMut<Session>,
    mut out: MessageWriter<GameEvent>,
    mut selected: ResMut<Selected>,
    mut attention: MessageWriter<NextAttention>,
) {
    if requests.read().count() == 0 || session.game.players[session.me as usize].eliminated {
        return;
    }
    let me = session.me;
    let players = session.game.players.len() as PlayerId;
    for p in (0..players).filter(|&p| p != me) {
        // Published so their visible moves animate; the renderer only shows what
        // our player can see.
        let events = ai::take_turn(&mut session.game, p);
        Session::publish(events, &mut out);
    }
    if let Ok(events) = session.game.apply(me, Command::EndTurn) {
        Session::publish(events, &mut out);
    }
    let events = ai::run_standing_orders(&mut session.game, me);
    Session::publish(events, &mut out);
    // The new turn starts with nothing selected (the group goes with it),
    // then goes to the first thing that needs the player.
    selected.0 = None;
    attention.write(NextAttention);
}

/// Selects the next unit waiting for orders and looks at it.
fn next_unit(
    mut requests: MessageReader<NextUnitRequest>,
    session: Res<Session>,
    mut selected: ResMut<Selected>,
    mut rig: ResMut<CameraRig>,
) {
    if requests.read().count() == 0 {
        return;
    }
    select_next_unit(&session, &mut selected, &mut rig);
}

/// Selects the unit waiting for orders after the current selection (wrapping
/// around) and looks at it. False if no unit is waiting.
pub fn select_next_unit(session: &Session, selected: &mut Selected, rig: &mut CameraRig) -> bool {
    let waiting: Vec<(UnitId, sim::hex::TileId)> =
        session.game.units_needing_orders(session.me).map(|u| (u.id, u.tile)).collect();
    let pick = waiting
        .iter()
        .find(|(id, _)| selected.0.is_some_and(|s| *id > s))
        .or(waiting.first())
        .copied();
    if let Some((unit, tile)) = pick {
        selected.0 = Some(unit);
        look_at(rig, session, tile);
    }
    pick.is_some()
}

/// After the selected unit has used its moves or been given an order (a move
/// that takes more than this turn included), move on to the next thing that
/// needs the player. A unit with moves left stays selected.
fn keep_selection_valid(
    session: Res<Session>,
    mut selected: ResMut<Selected>,
    mut group: ResMut<Group>,
    mut events: MessageReader<GameEvent>,
    mut next: MessageWriter<NextAttention>,
) {
    // A group loses its fallen members, and goes when another unit is selected.
    if group.0.iter().any(|u| !session.game.units.contains_key(u)) {
        group.0.retain(|u| session.game.units.contains_key(u));
        if selected.0.is_some_and(|s| !session.game.units.contains_key(&s)) {
            selected.0 = group.0.first().copied();
        }
    }
    if !group.0.is_empty() && selected.0.is_none_or(|s| !group.0.contains(&s)) {
        group.0.clear();
    }
    // A group stays selected after its orders, to be commanded again.
    if group.0.len() > 1 {
        events.clear();
        return;
    }
    let Some(unit) = selected.0 else { return };
    let Some(u) = session.game.units.get(&unit) else {
        selected.0 = None;
        return;
    };
    let unit = u.id;
    let acted = events.read().any(|GameEvent(e)| match e {
        Event::UnitMoved { unit: m, .. } => *m == unit,
        Event::OrderChanged { unit: m, .. } => *m == unit,
        _ => false,
    });
    if acted && (u.moves == 0 || u.order != Order::None) {
        next.write(NextAttention);
    }
}
