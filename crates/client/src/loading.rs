//! Loading: the world is generated and all its meshes and textures are built on a
//! background thread while a loading screen shows progress. When it is done, the
//! main thread only uploads the results and spawns entities.

use crate::layout::map_bounds;
use crate::ui::palette;
use crate::wrap::WrapPeriod;
use crate::play::Session;
use crate::{overlay, terrain, vegetation, water, GameMap};
use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use sim::game::{Game, GameSetup, PlayerId};
use sim::mapgen::{self, MapSettings};
use sim::rules::Rules;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AppState {
    /// The front end (see `frontend.rs`).
    #[default]
    MainMenu,
    CivSelect,
    Setup,
    /// The list of saves, from the main menu.
    LoadGame,
    Loading,
    InGame,
}

/// Systems that need the world. They only run once loading has finished.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct InGameSet;

/// Steps of world building, with their rough share of the total time.
#[derive(Clone, Copy)]
pub enum Stage {
    Map,
    Heights,
    Textures,
    Meshes,
    Forests,
    Water,
    Grid,
}

const STAGES: [(&str, f32); 7] = [
    ("Shaping continents and climate", 0.04),
    ("Raising mountains", 0.08),
    ("Painting the land", 0.55),
    ("Building terrain", 0.18),
    ("Growing forests", 0.08),
    ("Filling the seas", 0.05),
    ("Drawing the map grid", 0.02),
];

/// Shared between the build thread and the loading screen.
#[derive(Default)]
pub struct Progress {
    stage: AtomicUsize,
    done: AtomicUsize,
    total: AtomicUsize,
}

impl Progress {
    pub fn begin(&self, stage: Stage, total: usize) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total.max(1), Ordering::Relaxed);
        self.stage.store(stage as usize, Ordering::Relaxed);
    }

    /// Marks `n` more units of the current stage as done (callable from any thread).
    pub fn advance(&self, n: usize) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }

    fn snapshot(&self) -> (&'static str, f32) {
        let stage = self.stage.load(Ordering::Relaxed).min(STAGES.len() - 1);
        let within = (self.done.load(Ordering::Relaxed) as f32 / self.total.load(Ordering::Relaxed) as f32).min(1.0);
        let before: f32 = STAGES[..stage].iter().map(|s| s.1).sum();
        (STAGES[stage].0, before + STAGES[stage].1 * within)
    }
}

/// Everything built off the main thread.
pub struct BuiltWorld {
    game: GameMap,
    session: Session,
    terrain: terrain::TerrainBuild,
    water: water::WaterBuild,
    vegetation: vegetation::VegetationBuild,
    overlay: overlay::OverlayBuild,
    seconds: f32,
}

#[derive(Resource)]
struct Loading {
    task: Task<BuiltWorld>,
    progress: Arc<Progress>,
}

/// The game to load: a new one or a saved one.
#[derive(Resource, Clone)]
pub struct WorldSpec {
    pub rules: Rules,
    pub source: WorldSource,
}

#[derive(Clone)]
pub enum WorldSource {
    /// Generate a map and start a game on it.
    New { settings: MapSettings, setup: GameSetup },
    /// Carry on with a saved game, as player `me`.
    Saved { game: Box<Game>, me: PlayerId },
}

impl WorldSpec {
    /// The civ of the player on this screen.
    pub fn my_civ(&self) -> &sim::civ::CivDef {
        match &self.source {
            WorldSource::New { setup, .. } => self.rules.civ(setup.civs[0]),
            WorldSource::Saved { game, me } => game.civ(*me),
        }
    }
}

#[derive(Component)]
struct LoadingScreen;

#[derive(Component)]
struct StageText;

#[derive(Component)]
struct ProgressFill;

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .configure_sets(Update, InGameSet.run_if(in_state(AppState::InGame)))
            .configure_sets(PostUpdate, InGameSet.run_if(in_state(AppState::InGame)))
            .add_systems(OnEnter(AppState::Loading), (start_build, spawn_screen))
            .add_systems(Update, (poll_build, fill_badge).run_if(in_state(AppState::Loading)));
    }
}

fn build_world(spec: WorldSpec, progress: &Progress) -> BuiltWorld {
    let start = Instant::now();
    let mut lap = Instant::now();
    let mut timings = Vec::new();
    let mut mark = |name: &str| {
        timings.push(format!("{name} {:.0} ms", lap.elapsed().as_secs_f64() * 1000.0));
        lap = Instant::now();
    };
    progress.begin(Stage::Map, 1);
    let rules = spec.rules;
    let session = match spec.source {
        WorldSource::New { settings, setup } => {
            let map = mapgen::generate(&rules, &settings);
            mark("map");
            Session { game: Game::with_setup(rules.clone(), map, &setup), me: 0 }
        }
        WorldSource::Saved { game, me } => Session { game: *game, me },
    };
    let map = session.game.map.clone();
    mark("players");
    let terrain = terrain::build(&map, &rules, progress);
    mark("terrain");
    progress.begin(Stage::Forests, 1);
    let vegetation = vegetation::build(&map, &rules, &terrain.heights);
    mark("vegetation");
    progress.begin(Stage::Water, 1);
    let water = water::build(&map, &rules, &terrain.heights);
    mark("water");
    progress.begin(Stage::Grid, 1);
    let overlay = overlay::build(&map, &terrain.heights);
    mark("grid");
    progress.advance(1);
    info!("world build: {}", timings.join(", "));
    BuiltWorld {
        game: GameMap { map, rules },
        session,
        terrain,
        water,
        vegetation,
        overlay,
        seconds: start.elapsed().as_secs_f32(),
    }
}

fn start_build(mut commands: Commands, spec: Res<WorldSpec>) {
    let progress = Arc::new(Progress::default());
    let (spec, shared) = (spec.clone(), progress.clone());
    let task = AsyncComputeTaskPool::get().spawn(async move { build_world(spec, &shared) });
    commands.insert_resource(Loading { task, progress });
}

fn poll_build(
    mut commands: Commands,
    loading: Option<ResMut<Loading>>,
    mut stage_text: Single<&mut Text, With<StageText>>,
    mut fill: Single<&mut Node, With<ProgressFill>>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(mut loading) = loading else { return };
    let (label, fraction) = loading.progress.snapshot();
    stage_text.0 = format!("{label}...");
    fill.width = percent(fraction * 100.0);

    let Some(world) = block_on(future::poll_once(&mut loading.task)) else { return };
    info!("world built in {:.2} s", world.seconds);
    let (min, max) = map_bounds(&world.game.map.grid);
    commands.insert_resource(WrapPeriod(max.x - min.x));
    commands.insert_resource(terrain::Terrain { heights: world.terrain.heights.clone(), build_ms: world.seconds * 1000.0 });
    commands.insert_resource(world.terrain);
    commands.insert_resource(world.water);
    commands.insert_resource(world.vegetation);
    commands.insert_resource(world.overlay);
    commands.insert_resource(world.game);
    commands.insert_resource(world.session);
    commands.remove_resource::<Loading>();
    next.set(AppState::InGame);
}

/// Where the player's civ badge goes (filled in once the icons exist: when
/// starting straight into a game, this screen spawns before they are made).
#[derive(Component)]
struct BadgeSlot;

fn fill_badge(mut commands: Commands, slot: Query<Entity, With<BadgeSlot>>, spec: Res<WorldSpec>, icons: Option<Res<crate::icons::Icons>>) {
    let Some(icons) = icons else { return };
    for e in &slot {
        let civ = spec.my_civ();
        commands.entity(e).remove::<BadgeSlot>().with_child(crate::frontend::civ_badge(&icons, Some(civ), 150.0));
    }
}

fn spawn_screen(mut commands: Commands, spec: Res<WorldSpec>) {
    let civ = spec.my_civ();
    commands
        .spawn((
            LoadingScreen,
            DespawnOnExit(AppState::Loading),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(18),
                ..default()
            },
            BackgroundColor(Color::srgb(0.06, 0.055, 0.05)),
            GlobalZIndex(100),
        ))
        .with_children(|root| {
            // Who the player leads, while the world takes shape.
            root.spawn((BadgeSlot, Node { width: px(150), height: px(150), ..default() }));
            root.spawn((
                Text::new(civ.name.clone()),
                crate::ui::heading_font(56.0),
                TextColor(palette::ACCENT),
                TextShadow::default(),
            ));
            root.spawn(crate::ui::text(format!("{}, {}", civ.leader, civ.leader_title), 19.0, palette::TEXT));
            root.spawn((
                crate::ui::text(civ.description.clone(), 16.0, palette::TEXT_DIM),
                Node { max_width: px(640), margin: UiRect::new(px(0), px(0), px(6), px(26)), ..default() },
                TextLayout::justify(Justify::Center),
            ));
            root.spawn((
                StageText,
                Text::new("Preparing..."),
                TextFont { font_size: FontSize::Px(18.0), ..default() },
                TextColor(palette::TEXT),
            ));
            root.spawn((
                Node {
                    width: px(420),
                    height: px(10),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(5)),
                    ..default()
                },
                BackgroundColor(palette::BUTTON),
                BorderColor::all(palette::PANEL_BORDER),
            ))
            .with_child((
                ProgressFill,
                Node { width: percent(0), height: percent(100), border_radius: BorderRadius::all(px(5)), ..default() },
                BackgroundColor(palette::ACCENT),
            ));
        });
}
