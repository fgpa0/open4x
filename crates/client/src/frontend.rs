//! The front end: the main menu, choosing a civilization and setting up a new
//! game. A compass rose turns slowly behind every screen. Starting a game
//! hands a [`WorldSpec`] to the loading screen.

use crate::icons::{Icon, Icons};
use crate::loading::{AppState, WorldSource, WorldSpec};
use crate::map_preview::{PreviewJob, PreviewMap};
use crate::menu::Menu;
use crate::ui::{self, palette, ButtonState, OwnStyle, Tooltip};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate, Button};
use sim::civ::{CivDef, CivId, StartBias};
use sim::game::{Game, GameOptions, GameSetup, MIN_FIXED_SPACING};
use sim::hex::TileId;
use sim::mapgen::{MapSettings, MapType};
use std::collections::HashMap;
use sim::rules::{Rules, UnitClass};

/// The rules, for screens shown before a game exists.
#[derive(Resource, Clone)]
pub struct RulesRes(pub Rules);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapSize {
    Small,
    Standard,
    Large,
}

impl MapSize {
    const ALL: [MapSize; 3] = [MapSize::Small, MapSize::Standard, MapSize::Large];

    fn dims(self) -> (i32, i32) {
        match self {
            MapSize::Small => (96, 60),
            MapSize::Standard => (128, 80),
            MapSize::Large => (160, 100),
        }
    }

    fn name(self) -> &'static str {
        match self {
            MapSize::Small => "Small",
            MapSize::Standard => "Standard",
            MapSize::Large => "Large",
        }
    }

    /// Rivals that fit the map comfortably.
    fn rivals(self) -> usize {
        match self {
            MapSize::Small => 3,
            MapSize::Standard => 5,
            MapSize::Large => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeaLevel {
    Low,
    Normal,
    High,
}

impl SeaLevel {
    const ALL: [SeaLevel; 3] = [SeaLevel::Low, SeaLevel::Normal, SeaLevel::High];

    fn land(self) -> f32 {
        match self {
            SeaLevel::Low => 0.46,
            SeaLevel::Normal => 0.38,
            SeaLevel::High => 0.30,
        }
    }

    fn name(self) -> &'static str {
        match self {
            SeaLevel::Low => "Low (more land)",
            SeaLevel::Normal => "Normal",
            SeaLevel::High => "High (more sea)",
        }
    }
}

/// A map type's name and what it plays like.
fn map_type_text(kind: MapType) -> (&'static str, &'static str) {
    match kind {
        MapType::Continents => ("Continents", "A few large continents parted by open ocean. Reaching the others takes ships."),
        MapType::Pangaea => ("Pangaea", "One great supercontinent with a few islands off its coasts. Neighbours are close and wars are fought on land."),
        MapType::Archipelago => ("Archipelago", "Scattered islands across shallow seas. Fleets, embarking and coastal cities rule."),
        MapType::Fractal => ("Fractal", "Wild, unshaped coasts: anything from sprawling continents to strings of islands."),
        MapType::InlandSea => ("Inland Sea", "A ring of land around one great sea, which opens to the ocean through a single strait."),
        MapType::Terra => ("Terra", "Every civilization starts in the old world. A new world lies empty across the ocean, waiting to be found."),
        MapType::Earth => ("Earth", "The real world: its continents, mountain ranges, great rivers and climates. With True Start Locations, every civilization starts where it arose in history."),
    }
}

/// The seed each map type's picture in the picker is drawn from, chosen to
/// show the type at its most typical.
fn showcase_seed(kind: MapType) -> u64 {
    match kind {
        MapType::Continents => 3,
        MapType::Pangaea => 7,
        MapType::Archipelago => 7,
        MapType::Fractal => 7,
        MapType::InlandSea => 21,
        MapType::Terra => 3,
        MapType::Earth => 1,
    }
}

/// Unit limit choices (military units per player).
const UNIT_LIMITS: [Option<u32>; 6] = [None, Some(8), Some(12), Some(16), Some(24), Some(32)];
const TURN_LIMITS: [Option<u32>; 7] = [None, Some(150), Some(200), Some(300), Some(400), Some(500), Some(600)];

/// The game being set up.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct NewGame {
    /// The player's civ (`None`: random).
    pub civ: Option<CivId>,
    /// One entry per rival (`None`: random).
    pub rivals: Vec<Option<CivId>>,
    pub size: MapSize,
    pub map_type: MapType,
    pub sea: SeaLevel,
    pub unit_limit: Option<u32>,
    pub turn_limit: Option<u32>,
    /// On Earth: every civ starts in its historical homeland.
    pub true_starts: bool,
    pub seed: u64,
    /// Start tiles placed on the preview, by player slot (0 is you, then
    /// the rivals); `None` or missing: the game picks.
    pub starts: Vec<Option<TileId>>,
}

impl NewGame {
    /// FOURX_SIZE=small|standard|large picks the map size and FOURX_MAP the
    /// map type (by id, e.g. `pangaea`), for the front end and direct starts;
    /// FOURX_TRUE_STARTS=0 turns True Start Locations off on Earth.
    fn new(rules: &Rules, seed: u64) -> NewGame {
        let civ = std::env::var("FOURX_CIV").ok().and_then(|c| rules.civ_id(&c));
        let map_type = std::env::var("FOURX_MAP").ok().and_then(|m| MapType::from_id(&m)).unwrap_or(MapType::Continents);
        let size = match std::env::var("FOURX_SIZE").as_deref() {
            Ok("small") => MapSize::Small,
            Ok("large") => MapSize::Large,
            _ => MapSize::Standard,
        };
        let true_starts = std::env::var("FOURX_TRUE_STARTS").map_or(true, |v| v != "0");
        NewGame { civ, rivals: vec![None; size.rivals()], size, map_type, sea: SeaLevel::Normal, unit_limit: None, turn_limit: None, true_starts, seed, starts: Vec::new() }
    }

    /// The game dev runs start straight into: four players.
    pub fn direct(rules: &Rules, seed: u64) -> NewGame {
        NewGame { rivals: vec![None; 3], ..NewGame::new(rules, seed) }
    }

    pub fn map_settings(&self) -> MapSettings {
        let (width, height) = self.size.dims();
        MapSettings { width, height, seed: self.seed, kind: self.map_type, land_fraction: self.sea.land(), ..default() }
    }

    pub fn spec(&self, rules: &Rules) -> WorldSpec {
        let mut fixed = vec![self.civ];
        fixed.extend(&self.rivals);
        let mut setup = GameSetup::random(rules, fixed.len(), &fixed, self.seed);
        setup.options = GameOptions { unit_limit: self.unit_limit, turn_limit: self.turn_limit, true_starts: self.homes() };
        setup.starts = (0..fixed.len()).map(|slot| self.start(slot)).collect();
        WorldSpec { rules: rules.clone(), source: WorldSource::New { settings: self.map_settings(), setup } }
    }

    /// Civs chosen anywhere except rival slot `skip`.
    fn taken(&self, skip: Option<usize>) -> Vec<CivId> {
        let rivals = self.rivals.iter().enumerate().filter(|(i, _)| Some(*i) != skip).filter_map(|(_, c)| *c);
        self.civ.into_iter().chain(rivals).collect()
    }

    /// The start placed for a player slot.
    fn start(&self, slot: usize) -> Option<TileId> {
        self.starts.get(slot).copied().flatten()
    }

    /// Whether civs start in their homelands (True Start Locations on Earth).
    fn homes(&self) -> bool {
        self.true_starts && self.map_type == MapType::Earth
    }

    /// Where a slot's shield stands on the preview: its placed start, or with
    /// True Start Locations its chosen civ's homeland.
    fn shown_start(&self, rules: &Rules, slot: usize) -> Option<TileId> {
        self.start(slot).or_else(|| {
            let (lon, lat) = self.slot_civ(slot).filter(|_| self.homes()).and_then(|c| rules.civ(c).home)?;
            let (w, h) = self.size.dims();
            Some(sim::earth::tile_at(&sim::hex::MapGrid::new(w, h), lon, lat))
        })
    }

    fn set_start(&mut self, slot: usize, tile: Option<TileId>) {
        if self.starts.len() <= slot {
            self.starts.resize(slot + 1, None);
        }
        self.starts[slot] = tile;
    }

    /// The civ in a player slot (`None`: random).
    fn slot_civ(&self, slot: usize) -> Option<CivId> {
        if slot == 0 { self.civ } else { self.rivals.get(slot - 1).copied().flatten() }
    }

    fn preview_key(&self) -> (MapSize, MapType, SeaLevel, u64) {
        let seed = if self.map_type == MapType::Earth { sim::mapgen::EARTH_SEED } else { self.seed };
        (self.size, self.map_type, self.sea, seed)
    }
}

/// A seed from the clock, for new worlds.
fn fresh_seed() -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64);
    // Keep seeds short enough to read out.
    (nanos ^ (nanos >> 29)) % 1_000_000
}

pub struct FrontendPlugin;

impl Plugin for FrontendPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Preview>()
            .init_resource::<StartDrag>()
            .init_resource::<TypePicker>()
            .init_resource::<TypeThumbs>()
            .add_systems(OnEnter(AppState::Setup), open_picker_at_start)
            .add_systems(Startup, (init_new_game, start_backdrop))
            .add_systems(OnEnter(AppState::MainMenu), start_backdrop)
            .add_systems(OnEnter(AppState::Loading), despawn_backdrop)
            .add_systems(
                Update,
                (turn_compass, escape, update_preview, drag_starts, update_thumbs, rebuild_screens, style_rows).run_if(in_front_end),
            );
    }
}

fn in_front_end(state: Res<State<AppState>>) -> bool {
    matches!(state.get(), AppState::MainMenu | AppState::CivSelect | AppState::Setup | AppState::LoadGame)
}

fn init_new_game(mut commands: Commands, rules: Res<RulesRes>) {
    let seed = std::env::var("FOURX_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or_else(fresh_seed);
    commands.insert_resource(NewGame::new(&rules.0, seed));
}

// --- Backdrop -------------------------------------------------------------

#[derive(Component)]
struct Backdrop;

/// The compass rose that slowly turns behind the menus.
#[derive(Component)]
struct Compass;

/// Dark warm ground with a faint hex lattice that fades out towards the
/// edges, a soft glow behind the menu column and a vignette.
fn paint_ground(width: u32, height: u32) -> Vec<u8> {
    use tiny_skia::{Color as SkColor, GradientStop, Paint, PathBuilder, Pixmap, Point, RadialGradient, Shader, SpreadMode, Stroke, Transform};
    let mut pm = Pixmap::new(width, height).expect("non-empty");
    let (w, h) = (width as f32, height as f32);
    let radial = |cx: f32, cy: f32, r: f32, stops: Vec<GradientStop>| {
        RadialGradient::new(Point::from_xy(cx, cy), Point::from_xy(cx, cy), r, stops, SpreadMode::Pad, Transform::identity())
            .unwrap_or(Shader::SolidColor(SkColor::BLACK))
    };
    pm.fill(SkColor::from_rgba8(16, 13, 11, 255));
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, w, h).expect("non-empty");
    // Warm light from the left, where the menu is.
    let glow = radial(
        w * 0.22,
        h * 0.5,
        w * 0.7,
        vec![
            GradientStop::new(0.0, SkColor::from_rgba8(70, 50, 30, 255)),
            GradientStop::new(0.5, SkColor::from_rgba8(34, 26, 19, 255)),
            GradientStop::new(1.0, SkColor::from_rgba8(16, 13, 11, 0)),
        ],
    );
    pm.fill_rect(full, &Paint { shader: glow, anti_alias: true, ..Default::default() }, Transform::identity(), None);
    // The hex lattice, brightest around the middle.
    let r = h / 22.0;
    let (dx, dy) = (r * 1.732, r * 1.5);
    let mut pb = PathBuilder::new();
    let mut row = 0;
    let mut y = -r;
    while y < h + r {
        let mut x = if row % 2 == 0 { 0.0 } else { dx / 2.0 };
        while x < w + dx {
            for k in 0..6 {
                let a0 = (60.0 * k as f32 - 30.0f32).to_radians();
                let a1 = (60.0 * (k + 1) as f32 - 30.0f32).to_radians();
                if k == 0 {
                    pb.move_to(x + r * a0.cos(), y + r * a0.sin());
                }
                pb.line_to(x + r * a1.cos(), y + r * a1.sin());
            }
            x += dx;
        }
        y += dy;
        row += 1;
    }
    if let Some(path) = pb.finish() {
        let lines = radial(
            w * 0.5,
            h * 0.5,
            w * 0.6,
            vec![
                GradientStop::new(0.0, SkColor::from_rgba8(214, 170, 96, 36)),
                GradientStop::new(1.0, SkColor::from_rgba8(214, 170, 96, 4)),
            ],
        );
        let stroke = Stroke { width: 1.2, ..Default::default() };
        pm.stroke_path(&path, &Paint { shader: lines, anti_alias: true, ..Default::default() }, &stroke, Transform::identity(), None);
    }
    let vignette = radial(
        w * 0.5,
        h * 0.5,
        w * 0.75,
        vec![GradientStop::new(0.55, SkColor::from_rgba8(0, 0, 0, 0)), GradientStop::new(1.0, SkColor::from_rgba8(0, 0, 0, 200))],
    );
    pm.fill_rect(full, &Paint { shader: vignette, anti_alias: true, ..Default::default() }, Transform::identity(), None);
    pm.pixels().iter().flat_map(|p| [p.red(), p.green(), p.blue(), 255]).collect()
}

/// A compass rose in thin gold lines on a transparent ground.
fn paint_compass(size: u32) -> Vec<u8> {
    use tiny_skia::{Color as SkColor, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
    let mut pm = Pixmap::new(size, size).expect("non-empty");
    let s = size as f32;
    let c = s / 2.0;
    let paint = |a: u8| {
        let mut p = Paint::default();
        p.set_color(SkColor::from_rgba8(230, 190, 110, a));
        p.anti_alias = true;
        p
    };
    let stroke = |w: f32| Stroke { width: w * s / 1024.0, ..Default::default() };
    let circle = |r: f32| PathBuilder::from_circle(c, c, r * s).expect("radius > 0");
    for (r, w, a) in [(0.48, 3.0, 200), (0.465, 1.5, 160), (0.40, 1.5, 140), (0.24, 2.0, 170), (0.05, 2.0, 200)] {
        pm.stroke_path(&circle(r), &paint(a), &stroke(w), Transform::identity(), None);
    }
    // Degree ticks between the outer rings.
    let mut ticks = PathBuilder::new();
    for i in 0..360 {
        if i % 2 != 0 {
            continue;
        }
        let a = (i as f32).to_radians();
        let inner = if i % 30 == 0 { 0.40 } else if i % 10 == 0 { 0.43 } else { 0.45 };
        ticks.move_to(c + a.cos() * inner * s, c + a.sin() * inner * s);
        ticks.line_to(c + a.cos() * 0.465 * s, c + a.sin() * 0.465 * s);
    }
    pm.stroke_path(&ticks.finish().expect("ticks"), &paint(150), &stroke(1.5), Transform::identity(), None);
    // Rhumb lines.
    let mut rays = PathBuilder::new();
    for i in 0..32 {
        let a = (i as f32 * 11.25).to_radians();
        rays.move_to(c + a.cos() * 0.05 * s, c + a.sin() * 0.05 * s);
        rays.line_to(c + a.cos() * 0.40 * s, c + a.sin() * 0.40 * s);
    }
    pm.stroke_path(&rays.finish().expect("rays"), &paint(45), &stroke(1.0), Transform::identity(), None);
    // The star: four long and four short points, each half filled.
    for (i, (len, width)) in [(0.44f32, 0.06f32), (0.27, 0.045)].iter().enumerate() {
        for k in 0..4 {
            let a = (k as f32 * 90.0 + i as f32 * 45.0 - 90.0).to_radians();
            let (dir, side) = (Vec2::new(a.cos(), a.sin()), Vec2::new(-a.sin(), a.cos()));
            let tip = Vec2::splat(c) + dir * len * s;
            let left = Vec2::splat(c) + side * width * s;
            let right = Vec2::splat(c) - side * width * s;
            let mut outline = PathBuilder::new();
            outline.move_to(c, c);
            outline.line_to(left.x, left.y);
            outline.line_to(tip.x, tip.y);
            outline.line_to(right.x, right.y);
            outline.close();
            let outline = outline.finish().expect("point");
            let mut half = PathBuilder::new();
            half.move_to(c, c);
            half.line_to(left.x, left.y);
            half.line_to(tip.x, tip.y);
            half.close();
            pm.fill_path(&half.finish().expect("half"), &paint(110), FillRule::Winding, Transform::identity(), None);
            pm.stroke_path(&outline, &paint(220), &stroke(2.0), Transform::identity(), None);
        }
    }
    pm.pixels()
        .iter()
        .flat_map(|p| {
            let d = p.demultiply();
            [d.red(), d.green(), d.blue(), d.alpha()]
        })
        .collect()
}

/// Spawns the backdrop when the front end shows (at startup, or coming back
/// from a game).
fn start_backdrop(mut commands: Commands, state: Res<State<AppState>>, mut images: ResMut<Assets<Image>>, existing: Query<(), With<Backdrop>>) {
    if !matches!(state.get(), AppState::MainMenu | AppState::CivSelect | AppState::Setup | AppState::LoadGame) || !existing.is_empty() {
        return;
    }
    let mut image = |data: Vec<u8>, w: u32, h: u32| {
        let mut image = crate::image_util::rgba_image(data, w, h, true, false, false);
        image.asset_usage = bevy::asset::RenderAssetUsages::default();
        images.add(image)
    };
    let ground = image(paint_ground(1920, 1080), 1920, 1080);
    let compass = image(paint_compass(1024), 1024, 1024);
    commands.spawn((
        Backdrop,
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), overflow: Overflow::clip(), ..default() },
        ImageNode { image_mode: NodeImageMode::Stretch, ..ImageNode::new(ground) },
        GlobalZIndex(-10),
        Pickable::IGNORE,
        children![(
            Compass,
            Node { position_type: PositionType::Absolute, height: percent(125), aspect_ratio: Some(1.0), right: percent(-12), top: percent(-12.5), ..default() },
            ImageNode { color: Color::WHITE.with_alpha(0.17), ..ImageNode::new(compass) },
            UiTransform::IDENTITY,
            Pickable::IGNORE,
        )],
    ));
}

fn turn_compass(time: Res<Time>, mut compass: Query<&mut UiTransform, With<Compass>>) {
    for mut t in &mut compass {
        t.rotation = Rot2::degrees(time.elapsed_secs() * 1.5);
    }
}

/// Leaving the front end: the backdrop and the last screen go.
fn despawn_backdrop(mut commands: Commands, backdrop: Query<Entity, With<Backdrop>>, screens: Query<Entity, With<Screen>>) {
    for e in backdrop.iter().chain(&screens) {
        commands.entity(e).despawn();
    }
}

// --- Shared pieces -------------------------------------------------------

fn rgb(c: (u8, u8, u8)) -> Color {
    Color::srgb_u8(c.0, c.1, c.2)
}

/// A civ's shield: the emblem in its trim colour on its main colour.
/// `None` shows the "random civ" badge.
pub fn civ_badge(icons: &Icons, civ: Option<&CivDef>, size: f32) -> impl Bundle {
    let (main, trim, emblem) = match civ {
        Some(c) => (rgb(c.colors.primary), rgb(c.colors.secondary), Icon::emblem(&c.emblem)),
        None => (Color::srgb(0.36, 0.33, 0.29), palette::ACCENT, Icon::Random),
    };
    (
        Node {
            width: px(size),
            height: px(size),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        },
        ImageNode { color: main, ..ImageNode::new(icons.white(Icon::Banner)) },
        children![(
            ImageNode { color: trim, ..ImageNode::new(icons.white(emblem)) },
            Node { width: px(size * 0.5), height: px(size * 0.5), margin: UiRect::bottom(px(size * 0.05)), ..default() },
            Pickable::IGNORE,
        )],
    )
}

/// What a screen is built from (despawned when the screen changes).
#[derive(Component)]
struct Screen;

fn screen_root() -> impl Bundle {
    (
        Screen,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    )
}

/// Title bar of the setup screens.
fn header(title: &str, step: &str) -> impl Bundle {
    (
        Node {
            width: percent(100),
            padding: UiRect::axes(px(40), px(18)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            border: UiRect::bottom(px(1)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.05, 0.04, 0.035, 0.92)),
        BorderColor::all(palette::PANEL_BORDER.with_alpha(0.6)),
        children![
            (ui::heading(title.to_string(), 30.0, palette::ACCENT), TextShadow::default()),
            ui::text(step.to_string(), 15.0, palette::TEXT_DIM),
        ],
    )
}

/// Bottom bar with navigation buttons.
fn footer() -> impl Bundle {
    (
        Node {
            width: percent(100),
            padding: UiRect::axes(px(40), px(14)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            border: UiRect::top(px(1)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.05, 0.04, 0.035, 0.92)),
        BorderColor::all(palette::PANEL_BORDER.with_alpha(0.6)),
    )
}

/// A prominent button for the main action of a screen.
fn primary_button(label: &str, width: f32) -> impl Bundle {
    (
        Button,
        Hovered::default(),
        OwnStyle,
        PrimaryButton,
        Node {
            width: px(width),
            height: px(48),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.55, 0.4, 0.17)),
        BorderColor::all(palette::ACCENT),
        BoxShadow::new(Color::srgba(0.9, 0.7, 0.3, 0.25), px(0), px(0), px(0), px(12)),
        children![(ui::heading(label.to_string(), 19.0, Color::srgb(1.0, 0.96, 0.86)), Pickable::IGNORE)],
    )
}

#[derive(Component)]
struct PrimaryButton;

/// A standard button of a given width. Observers must go on this entity
/// itself: `Activate` doesn't bubble up to parents.
fn sized_button(label: &str, width: f32) -> impl Bundle {
    (ui::base_button(px(width), px(42)), children![(ui::text(label.to_string(), 18.0, palette::TEXT), Pickable::IGNORE)])
}

fn go(state: AppState) -> impl Fn(On<Activate>, ResMut<NextState<AppState>>) {
    move |_: On<Activate>, mut next: ResMut<NextState<AppState>>| next.set(state)
}

/// Rebuilds the current screen when it is entered or the setup changes.
fn rebuild_screens(
    mut commands: Commands,
    state: Res<State<AppState>>,
    new_game: Res<NewGame>,
    rules: Res<RulesRes>,
    icons: Res<Icons>,
    preview: Res<Preview>,
    picker: Res<TypePicker>,
    thumbs: Res<TypeThumbs>,
    old: Query<Entity, With<Screen>>,
    scroll: Query<&ScrollPosition, With<DetailScroll>>,
    mut primary: Query<(&Hovered, &mut BackgroundColor), With<PrimaryButton>>,
) {
    for (hovered, mut bg) in &mut primary {
        let c = if hovered.get() { Color::srgb(0.68, 0.5, 0.21) } else { Color::srgb(0.55, 0.4, 0.17) };
        bg.set_if_neq(BackgroundColor(c));
    }
    if !state.is_changed() && !new_game.is_changed() && !picker.is_changed() {
        return;
    }
    let kept_scroll = scroll.iter().next().map(|s| s.0).unwrap_or_default();
    for e in &old {
        commands.entity(e).despawn();
    }
    match state.get() {
        AppState::MainMenu => spawn_main_menu(&mut commands, &rules.0, &icons),
        AppState::CivSelect => spawn_civ_select(&mut commands, &rules.0, &icons, &new_game, kept_scroll),
        AppState::Setup => spawn_setup(&mut commands, &rules.0, &icons, &new_game, &preview, picker.0.then_some(&*thumbs)),
        _ => {}
    }
}

fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    menu: Res<State<Menu>>,
    mut picker: ResMut<TypePicker>,
    mut next: ResMut<NextState<AppState>>,
    mut next_menu: ResMut<NextState<Menu>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if *menu.get() != Menu::Closed {
        next_menu.set(Menu::Closed);
        return;
    }
    if picker.0 {
        picker.0 = false;
        return;
    }
    match state.get() {
        AppState::CivSelect | AppState::LoadGame => next.set(AppState::MainMenu),
        AppState::Setup => next.set(AppState::CivSelect),
        _ => {}
    }
}

// --- Main menu -------------------------------------------------------------

fn spawn_main_menu(commands: &mut Commands, rules: &Rules, icons: &Icons) {
    commands.spawn(screen_root()).with_children(|root| {
        root.spawn(Node {
            flex_direction: FlexDirection::Column,
            height: percent(100),
            justify_content: JustifyContent::Center,
            padding: UiRect::left(percent(8)),
            row_gap: px(12),
            ..default()
        })
        .with_children(|col| {
            col.spawn((ui::heading(crate::GAME_NAME, 120.0, palette::ACCENT), TextShadow { offset: Vec2::new(3.0, 4.0), ..default() }));
            col.spawn((
                ui::heading("A Game of Empires", 22.0, palette::TEXT),
                TextShadow::default(),
                Node { margin: UiRect::new(px(4), px(0), px(-14), px(34)), ..default() },
            ));
            col.spawn((Node { width: px(340), ..default() }, children![(primary_button("New Game", 340.0), observe(go(AppState::CivSelect)))]));
            // Continue: the most recent save, autosaves included.
            match crate::saves::list_saves().into_iter().next() {
                Some(latest) => {
                    let tip = Tooltip::new("Continue", format!("{}, turn {}", latest.meta.civ_name, latest.meta.turn));
                    let path = latest.path.clone();
                    col.spawn((
                        sized_button("Continue", 340.0),
                        tip,
                        observe(move |_: On<Activate>, mut commands: Commands, rules: Res<RulesRes>, mut next: ResMut<NextState<AppState>>| {
                            match crate::saves::load(&path, &rules.0) {
                                Ok(spec) => {
                                    commands.insert_resource(spec);
                                    next.set(AppState::Loading);
                                }
                                // Unreadable: show the list, which says why.
                                Err(e) => {
                                    warn!("can't continue: {e}");
                                    next.set(AppState::LoadGame);
                                }
                            }
                        }),
                    ));
                }
                None => {
                    col.spawn((
                        ui::base_button(px(340), px(42)),
                        ButtonState { disabled: true, ..default() },
                        Tooltip::new("Continue", "No saved games yet."),
                        children![(ui::text("Continue", 18.0, palette::TEXT_DIM.with_alpha(0.6)), Pickable::IGNORE)],
                    ));
                }
            }
            col.spawn((sized_button("Load Game", 340.0), observe(go(AppState::LoadGame))));
            col.spawn((
                sized_button("Settings", 340.0),
                observe(|_: On<Activate>, mut menu: ResMut<NextState<Menu>>| menu.set(Menu::Settings)),
            ));
            col.spawn((
                sized_button("Quit", 340.0),
                observe(|_: On<Activate>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                }),
            ));
        });
        // The civilizations, along the bottom.
        root.spawn(Node {
            position_type: PositionType::Absolute,
            right: px(36),
            bottom: px(28),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: px(8),
            ..default()
        })
        .with_children(|corner| {
            corner.spawn(ui::text(format!("{} civilizations", rules.civ_count()), 13.0, palette::TEXT_DIM));
            corner.spawn(Node { column_gap: px(6), ..default() }).with_children(|row| {
                for c in &rules.civs.civs {
                    row.spawn((
                        civ_badge(icons, Some(c), 38.0),
                        Tooltip::new(c.name.clone(), format!("{}, {}", c.leader, c.leader_title)),
                    ));
                }
            });
        });
        root.spawn((
            Node { position_type: PositionType::Absolute, left: percent(8), bottom: px(28), ..default() },
            children![ui::text("Early development build", 12.0, palette::TEXT_DIM)],
        ));
    });
}

// --- Choosing a civilization -----------------------------------------------

#[derive(Component)]
struct DetailScroll;

fn pick_civ(civ: Option<CivId>) -> impl Fn(On<Activate>, ResMut<NewGame>) {
    move |_: On<Activate>, mut ng: ResMut<NewGame>| {
        ng.civ = civ;
        // A rival can't be the same civ.
        for r in &mut ng.rivals {
            if civ.is_some() && *r == civ {
                *r = None;
            }
        }
    }
}

fn spawn_civ_select(commands: &mut Commands, rules: &Rules, icons: &Icons, ng: &NewGame, scroll: Vec2) {
    commands.spawn(screen_root()).with_children(|root| {
        root.spawn(header("Choose Your Civilization", "Step 1 of 2"));
        root.spawn(Node { flex_grow: 1.0, min_height: px(0), padding: UiRect::axes(px(40), px(20)), column_gap: px(20), ..default() })
            .with_children(|body| {
                // The list.
                body.spawn((
                    Node {
                        width: px(320),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6),
                        padding: UiRect::all(px(10)),
                        border: UiRect::all(px(1.5)),
                        border_radius: BorderRadius::all(px(8)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    ui::frame(),
                ))
                .with_children(|list| {
                    list.spawn(civ_row(icons, None, ng.civ.is_none()));
                    for (i, c) in rules.civs.civs.iter().enumerate() {
                        let id = i as CivId;
                        list.spawn(civ_row(icons, Some((id, c)), ng.civ == Some(id)));
                    }
                });
                // The chosen civ in depth.
                body.spawn((
                    DetailScroll,
                    Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(16),
                        padding: UiRect::axes(px(28), px(24)),
                        border: UiRect::all(px(1.5)),
                        border_radius: BorderRadius::all(px(8)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition(scroll),
                    ui::frame(),
                ))
                .with_children(|detail| match ng.civ {
                    Some(id) => civ_detail(detail, rules, icons, id),
                    None => random_detail(detail, icons),
                });
            });
        root.spawn(footer()).with_children(|bar| {
            bar.spawn((sized_button("Back", 200.0), observe(go(AppState::MainMenu))));
            bar.spawn((primary_button("Next: Game Setup", 280.0), observe(go(AppState::Setup))));
        });
    });
}

/// A civ in the list (or the "random" entry).
fn civ_row(icons: &Icons, civ: Option<(CivId, &CivDef)>, selected: bool) -> impl Bundle {
    let (name, leader, stripe) = match civ {
        Some((_, c)) => (c.name.clone(), c.leader.clone(), rgb(c.colors.primary)),
        None => ("Random".to_string(), "Leave it to fate".to_string(), palette::TEXT_DIM),
    };
    (
        Button,
        Hovered::default(),
        OwnStyle,
        Node {
            width: percent(100),
            height: px(60),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: px(12),
            padding: UiRect::new(px(10), px(10), px(0), px(0)),
            border: UiRect::new(px(4), px(1), px(1), px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(palette::BUTTON),
        BorderColor { left: stripe, ..BorderColor::all(palette::BUTTON_BORDER) },
        ButtonState { active: selected, ..default() },
        StripeColor(stripe),
        observe(pick_civ(civ.map(|(id, _)| id))),
        children![
            civ_badge(icons, civ.map(|(_, c)| c), 46.0),
            (
                Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() },
                Pickable::IGNORE,
                children![
                    (ui::heading(name, 18.0, if selected { palette::ACCENT } else { palette::TEXT }), Pickable::IGNORE),
                    (ui::text(leader, 13.0, palette::TEXT_DIM), Pickable::IGNORE),
                ],
            ),
        ],
    )
}

/// A civ row's colour stripe (rows style themselves to keep it).
#[derive(Component)]
struct StripeColor(Color);

fn style_rows(mut rows: Query<(&Hovered, &ButtonState, &StripeColor, &mut BackgroundColor, &mut BorderColor)>) {
    for (hovered, state, stripe, mut bg, mut border) in &mut rows {
        let fill = if hovered.get() {
            palette::BUTTON_HOVER
        } else if state.active {
            palette::BUTTON_ACTIVE
        } else {
            palette::BUTTON
        };
        let edge = if hovered.get() || state.active { palette::ACCENT } else { palette::BUTTON_BORDER };
        bg.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor { left: stripe.0, ..BorderColor::all(edge) });
    }
}

fn random_detail(detail: &mut ChildSpawnerCommands, icons: &Icons) {
    detail.spawn(Node { column_gap: px(24), align_items: AlignItems::Center, ..default() }).with_children(|hero| {
        hero.spawn(civ_badge(icons, None, 128.0));
        hero.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }).with_children(|t| {
            t.spawn(ui::heading("Random Civilization", 44.0, palette::ACCENT));
            t.spawn(ui::text("A civilization is picked for you when the game starts.", 17.0, palette::TEXT));
        });
    });
    detail.spawn(ui::text(
        "Every civilization has its own strengths: perks that change how its land, cities and armies work, \
         a unique unit and a unique building, and a leader with a personality of their own. \
         Pick one from the list to read about it.",
        16.0,
        palette::TEXT_DIM,
    ));
}

/// A card with a small title, used for the civ's perks and uniques.
fn card(accent: Color) -> impl Bundle {
    (
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::new(px(14), px(14), px(12), px(14)),
            border: UiRect::new(px(3), px(1), px(1), px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(palette::PANEL_LIGHT),
        BorderColor { left: accent, ..BorderColor::all(palette::PANEL_BORDER.with_alpha(0.45)) },
    )
}

fn card_label(text: &str) -> impl Bundle {
    ui::bold(text.to_uppercase(), 11.5, palette::TEXT_DIM)
}

fn planned(text: &str) -> impl Bundle {
    (
        Node { column_gap: px(6), align_items: AlignItems::FlexStart, margin: UiRect::top(px(2)), ..default() },
        children![
            (
                Node { padding: UiRect::axes(px(6), px(1)), border_radius: BorderRadius::all(px(3)), flex_shrink: 0.0, ..default() },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.07)),
                children![ui::bold("COMING LATER", 10.0, palette::TEXT_DIM)],
            ),
            ui::text(text.to_string(), 13.0, palette::TEXT_DIM),
        ],
    )
}

fn perk_card(col: &mut ChildSpawnerCommands, accent: Color, label: &str, perk: &sim::civ::Perk) {
    col.spawn(card(accent)).with_children(|c| {
        c.spawn(card_label(label));
        c.spawn(ui::heading(perk.name.clone(), 19.0, palette::ACCENT));
        c.spawn(ui::text(perk.description.clone(), 14.5, palette::TEXT));
        if let Some(p) = &perk.planned {
            c.spawn(planned(p));
        }
    });
}

/// A small icon and number, with a difference from the standard version.
fn stat_chip(icons: &Icons, icon: Icon, value: i32, base: Option<i32>, tip: &str) -> impl Bundle {
    let delta = base.map(|b| value - b).filter(|d| *d != 0);
    let delta_text = delta.map(|d| format!(" {d:+}")).unwrap_or_default();
    let delta_color = if delta.is_some_and(|d| d > 0) { palette::FOOD } else { palette::DANGER };
    (
        Node {
            align_items: AlignItems::Center,
            column_gap: px(4),
            padding: UiRect::axes(px(8), px(3)),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
        Tooltip::title(tip.to_string()),
        children![
            ui::icon(icons.get(icon), 18.0),
            (ui::bold(value.to_string(), 14.0, palette::TEXT), Pickable::IGNORE),
            (ui::bold(delta_text, 13.0, delta_color), Pickable::IGNORE),
        ],
    )
}

fn civ_detail(detail: &mut ChildSpawnerCommands, rules: &Rules, icons: &Icons, id: CivId) {
    let c = rules.civ(id);
    let accent = rgb(c.colors.primary);
    // Leader, colours and where they like to start.
    detail.spawn(Node { column_gap: px(26), align_items: AlignItems::Center, ..default() }).with_children(|hero| {
        hero.spawn(civ_badge(icons, Some(c), 132.0));
        hero.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(4), flex_grow: 1.0, ..default() }).with_children(|t| {
            t.spawn((ui::heading(c.name.clone(), 46.0, palette::ACCENT), TextShadow::default()));
            t.spawn(ui::text(format!("Led by {}, {}", c.leader, c.leader_title), 18.0, palette::TEXT));
            t.spawn(Node { column_gap: px(18), align_items: AlignItems::Center, margin: UiRect::top(px(8)), ..default() }).with_children(|row| {
                for (label, color) in [("Colour", c.colors.primary), ("Trim", c.colors.secondary)] {
                    row.spawn(Node { column_gap: px(7), align_items: AlignItems::Center, ..default() }).with_children(|sw| {
                        sw.spawn((
                            Node { width: px(20), height: px(20), border: UiRect::all(px(1.5)), border_radius: BorderRadius::MAX, ..default() },
                            BackgroundColor(rgb(color)),
                            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.5)),
                        ));
                        sw.spawn(ui::text(label, 13.5, palette::TEXT_DIM));
                    });
                }
                if !c.start_bias.is_empty() {
                    let near: Vec<&str> = c.start_bias.iter().map(|b| bias_name(*b)).collect();
                    row.spawn(ui::text(format!("Starts near: {}", near.join(", ")), 13.5, palette::TEXT_DIM));
                }
            });
        });
    });
    detail.spawn((ui::text(c.description.clone(), 16.0, Color::srgb(0.86, 0.82, 0.74)), Node { max_width: px(900), ..default() }));
    detail.spawn(Node { column_gap: px(16), align_items: AlignItems::FlexStart, ..default() }).with_children(|cols| {
        cols.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(12), flex_grow: 1.0, flex_basis: px(0), ..default() })
            .with_children(|col| {
                perk_card(col, accent, "Civilization ability", &c.ability);
                perk_card(col, accent, "Leader ability", &c.leader_ability);
                col.spawn(card(accent)).with_children(|card| {
                    card.spawn(card_label("Agenda"));
                    card.spawn(ui::heading(c.agenda.name.clone(), 19.0, palette::ACCENT));
                    card.spawn(ui::text(c.agenda.description.clone(), 14.5, palette::TEXT));
                });
            });
        cols.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(12), flex_grow: 1.0, flex_basis: px(0), ..default() })
            .with_children(|col| {
                for &u in rules.civ_unique_units(id) {
                    unit_card(col, rules, icons, accent, u);
                }
                for &b in rules.civ_unique_buildings(id) {
                    building_card(col, rules, icons, accent, b);
                }
                col.spawn(card(accent)).with_children(|card| {
                    card.spawn(card_label("Personality"));
                    card.spawn(ui::text("How this leader plays when the computer controls them.", 13.0, palette::TEXT_DIM));
                    card.spawn(Node {
                        display: Display::Grid,
                        grid_template_columns: vec![GridTrack::auto(), GridTrack::flex(1.0), GridTrack::auto(), GridTrack::flex(1.0)],
                        column_gap: px(10),
                        row_gap: px(5),
                        margin: UiRect::top(px(4)),
                        ..default()
                    })
                    .with_children(|grid| {
                        for (name, value) in c.drives.named() {
                            grid.spawn(ui::text(name, 13.0, palette::TEXT));
                            grid.spawn(Node { column_gap: px(2), align_items: AlignItems::Center, ..default() }).with_children(|bar| {
                                for i in 0..10u8 {
                                    let on = i < value;
                                    bar.spawn((
                                        Node { width: px(9), height: px(9), border_radius: BorderRadius::all(px(2)), ..default() },
                                        BackgroundColor(if on { palette::ACCENT.with_alpha(0.55 + 0.045 * i as f32) } else { Color::srgba(1.0, 1.0, 1.0, 0.08) }),
                                    ));
                                }
                            });
                        }
                    });
                    // The troops it favours, most liked first.
                    let mut troops = c.army.named();
                    troops.sort_by_key(|t| std::cmp::Reverse(t.1));
                    let favoured: Vec<&str> = troops.iter().filter(|t| t.1 >= 6).map(|t| t.0).collect();
                    let line = if favoured.is_empty() { "A balanced army".to_string() } else { format!("Favours {}", favoured.join(", ").to_lowercase()) };
                    card.spawn((ui::text(line, 13.0, palette::TEXT), Node { margin: UiRect::top(px(6)), ..default() }));
                });
            });
    });
}

fn bias_name(b: StartBias) -> &'static str {
    match b {
        StartBias::Coast => "coast",
        StartBias::River => "rivers",
        StartBias::Lake => "lakes",
        StartBias::Desert => "desert",
        StartBias::Plains => "plains",
        StartBias::Grassland => "grassland",
        StartBias::Tundra => "tundra",
        StartBias::Taiga => "taiga",
        StartBias::Hills => "hills",
        StartBias::Mountains => "mountains",
        StartBias::Forest => "forest",
        StartBias::Jungle => "jungle",
        StartBias::Marsh => "marsh",
    }
}

fn unit_card(col: &mut ChildSpawnerCommands, rules: &Rules, icons: &Icons, accent: Color, u: sim::rules::UnitTypeId) {
    let d = rules.unit(u);
    let base = rules.unit_replaces(u).map(|b| rules.unit(b));
    col.spawn(card(accent)).with_children(|card| {
        card.spawn(card_label("Unique unit"));
        card.spawn(Node { column_gap: px(12), align_items: AlignItems::Center, ..default() }).with_children(|row| {
            row.spawn(ui::icon(icons.get(Icon::unit(&d.art)), 46.0));
            row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() }).with_children(|t| {
                t.spawn(ui::heading(d.name.clone(), 19.0, palette::ACCENT));
                if let Some(b) = base {
                    t.spawn(ui::text(format!("Replaces the {}", b.name), 13.0, palette::TEXT_DIM));
                }
            });
        });
        card.spawn(Node { column_gap: px(6), flex_wrap: FlexWrap::Wrap, row_gap: px(6), ..default() }).with_children(|stats| {
            if d.class == UnitClass::Military {
                stats.spawn(stat_chip(icons, Icon::Strength, d.strength, base.map(|b| b.strength), "Strength"));
            }
            if d.ranged > 0 {
                stats.spawn(stat_chip(icons, Icon::Archer, d.ranged, base.map(|b| b.ranged), "Ranged strength"));
            }
            stats.spawn(stat_chip(icons, Icon::Moves, d.moves, base.map(|b| b.moves), "Movement"));
            stats.spawn(stat_chip(icons, Icon::Production, d.cost, base.map(|b| b.cost), "Cost"));
        });
        if !d.description.is_empty() {
            card.spawn(ui::text(d.description.clone(), 14.5, palette::TEXT));
        }
        if let Some(p) = &d.planned {
            card.spawn(planned(p));
        }
    });
}

fn building_card(col: &mut ChildSpawnerCommands, rules: &Rules, icons: &Icons, accent: Color, b: sim::rules::BuildingId) {
    let d = rules.building(b);
    let base = rules.building_replaces(b).map(|r| rules.building(r));
    col.spawn(card(accent)).with_children(|card| {
        card.spawn(card_label("Unique building"));
        card.spawn(Node { column_gap: px(12), align_items: AlignItems::Center, ..default() }).with_children(|row| {
            row.spawn(ui::icon(icons.get(Icon::building(&d.id)), 46.0));
            row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() }).with_children(|t| {
                t.spawn(ui::heading(d.name.clone(), 19.0, palette::ACCENT));
                if let Some(base) = base {
                    t.spawn(ui::text(format!("Replaces the {}", base.name), 13.0, palette::TEXT_DIM));
                }
            });
        });
        card.spawn(Node { column_gap: px(6), flex_wrap: FlexWrap::Wrap, row_gap: px(6), ..default() }).with_children(|stats| {
            let pick = |f: fn(&sim::rules::BuildingDef) -> i32| (f(d), base.map(f));
            for (icon, (value, was), tip) in [
                (Icon::Food, pick(|x| x.food), "Food"),
                (Icon::Production, pick(|x| x.production), "Production"),
                (Icon::Gold, pick(|x| x.gold), "Gold"),
                (Icon::Culture, pick(|x| x.culture), "Culture"),
                (Icon::Science, pick(|x| x.science), "Science"),
                (Icon::Defense, pick(|x| x.defense), "Defense (%)"),
            ] {
                if value != 0 || was.is_some_and(|w| w != 0) {
                    stats.spawn(stat_chip(icons, icon, value, was, tip));
                }
            }
            stats.spawn(stat_chip(icons, Icon::Production, d.cost, base.map(|b| b.cost), "Cost"));
        });
        card.spawn(ui::text(d.description.clone(), 14.5, palette::TEXT));
    });
}

// --- Game setup --------------------------------------------------------------

/// The map preview for the current setup.
#[derive(Resource, Default)]
struct Preview {
    job: Option<PreviewJob>,
    key: Option<(MapSize, MapType, SeaLevel, u64)>,
    image: Option<Handle<Image>>,
    /// Which tile is where on `image`.
    layout: Option<PreviewMap>,
}

#[derive(Component)]
struct PreviewImage;

#[derive(Component)]
struct PreviewNote;

fn update_preview(
    mut preview: ResMut<Preview>,
    mut ng: ResMut<NewGame>,
    rules: Res<RulesRes>,
    state: Res<State<AppState>>,
    mut images: ResMut<Assets<Image>>,
    mut shown: Query<(&mut ImageNode, &mut Node), With<PreviewImage>>,
    mut notes: Query<&mut Visibility, With<PreviewNote>>,
) {
    if *state.get() != AppState::Setup {
        return;
    }
    let key = Some(ng.preview_key());
    if preview.key != key {
        preview.key = key;
        preview.job = Some(PreviewJob::start(rules.0.clone(), ng.map_settings(), 760));
        preview.layout = None;
        // Starts placed on another world mean nothing on this one.
        if ng.starts.iter().any(Option::is_some) {
            ng.starts.clear();
        }
    }
    let ready = match preview.job.as_mut() {
        Some(job) => job.poll_map(&mut images),
        None => None,
    };
    if let Some((image, layout)) = ready {
        preview.job = None;
        preview.image = Some(image);
        preview.layout = Some(layout);
        // Homeland shields go on the new picture.
        if ng.homes() {
            ng.set_changed();
        }
        // Dev aid: FOURX_STARTS=slot:tile,... places starts on the first preview.
        if let Some(spec) = std::env::var("FOURX_STARTS").ok().filter(|_| ng.starts.is_empty()) {
            for (slot, tile) in spec.split(',').filter_map(|p| p.split_once(':')).filter_map(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))) {
                ng.set_start(slot, Some(tile));
            }
        }
    }
    let busy = preview.job.is_some();
    for mut v in &mut notes {
        v.set_if_neq(if busy { Visibility::Inherited } else { Visibility::Hidden });
    }
    if let Some(image) = &preview.image {
        for (mut node, mut layout) in &mut shown {
            if node.image != *image {
                node.image = image.clone();
            }
            let alpha = if busy { 0.45 } else { 1.0 };
            if node.color.alpha() != alpha {
                node.color = Color::WHITE.with_alpha(alpha);
            }
            if layout.display != Display::Flex {
                layout.display = Display::Flex;
            }
        }
    }
}

fn cycle<T: Copy + PartialEq>(values: &[T], current: T, step: i32) -> T {
    let i = values.iter().position(|v| *v == current).unwrap_or(0) as i32;
    values[(i + step).rem_euclid(values.len() as i32) as usize]
}

/// A labelled `< value >` row.
fn stepper(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    value: String,
    hint: Option<String>,
    change: impl Fn(&mut NewGame, &Rules, i32) + Clone + Send + Sync + 'static,
) {
    parent
        .spawn(Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(2), ..default() })
        .with_children(|block| {
            block.spawn(Node { width: percent(100), align_items: AlignItems::Center, column_gap: px(8), ..default() }).with_children(|row| {
                row.spawn((Node { flex_grow: 1.0, ..default() }, children![ui::label(label)]));
                for step in [-1, 1] {
                    let change = change.clone();
                    let arrow = if step < 0 { "<" } else { ">" };
                    let button = (
                        ui::small_button(arrow),
                        observe(move |_: On<Activate>, mut ng: ResMut<NewGame>, rules: Res<RulesRes>| change(&mut ng, &rules.0, step)),
                    );
                    if step < 0 {
                        row.spawn(button);
                        row.spawn((
                            Node { width: px(170), justify_content: JustifyContent::Center, ..default() },
                            children![ui::value_text(&value)],
                        ));
                    } else {
                        row.spawn(button);
                    }
                }
            });
            if let Some(h) = hint {
                block.spawn(ui::text(h, 12.5, palette::TEXT_DIM));
            }
        });
}

fn spawn_setup(commands: &mut Commands, rules: &Rules, icons: &Icons, ng: &NewGame, preview: &Preview, picker: Option<&TypeThumbs>) {
    commands.spawn(screen_root()).with_children(|root| {
        root.spawn(header("Game Setup", "Step 2 of 2"));
        root.spawn(Node { flex_grow: 1.0, min_height: px(0), padding: UiRect::axes(px(40), px(20)), column_gap: px(20), ..default() })
            .with_children(|body| {
                // The world.
                body.spawn((
                    Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(12),
                        padding: UiRect::all(px(20)),
                        border: UiRect::all(px(1.5)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    ui::frame(),
                ))
                .with_children(|world| {
                    world.spawn(ui::section("The world"));
                    world
                        .spawn((
                            Node {
                                width: percent(100),
                                flex_grow: 1.0,
                                min_height: px(0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(4)),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.07, 0.1, 0.13)),
                            BorderColor::all(palette::PANEL_BORDER.with_alpha(0.5)),
                        ))
                        .with_children(|frame| {
                            let (w, h) = ng.size.dims();
                            frame.spawn((
                                PreviewImage,
                                ImageNode::new(preview.image.clone().unwrap_or_default()),
                                Node {
                                    max_width: percent(100),
                                    max_height: percent(100),
                                    aspect_ratio: Some(w as f32 * 1.732 / (h as f32 * 1.5 + 0.5)),
                                    display: if preview.image.is_some() { Display::Flex } else { Display::None },
                                    width: percent(100),
                                    ..default()
                                },
                            ))
                            .with_children(|picture| {
                                let Some(layout) = &preview.layout else { return };
                                for slot in 0..=ng.rivals.len() {
                                    let Some(uv) = ng.shown_start(rules, slot).and_then(|t| layout.uv_of(t)) else { continue };
                                    picture.spawn(start_pin(rules, icons, ng, slot, uv));
                                }
                            });
                            frame.spawn((
                                PreviewNote,
                                Node { position_type: PositionType::Absolute, ..default() },
                                children![(ui::heading("Charting the world...", 20.0, palette::TEXT), TextShadow::default())],
                            ));
                        });
                    world.spawn(Node { align_items: AlignItems::Center, column_gap: px(10), ..default() }).with_children(|row| {
                        // Earth is one fixed world: no seed to roll.
                        if ng.map_type != MapType::Earth {
                            row.spawn(ui::text("Seed", 15.0, palette::TEXT_DIM));
                            row.spawn(ui::bold(ng.seed.to_string(), 16.0, palette::ACCENT));
                            row.spawn((
                                ui::icon_button(icons.get(Icon::Random), 34.0, Tooltip::new("Random seed", "Roll a different world.")),
                                observe(|_: On<Activate>, mut ng: ResMut<NewGame>| {
                                    let old = ng.seed;
                                    ng.seed = std::iter::repeat_with(fresh_seed).find(|&s| s != old).expect("endless");
                                }),
                            ));
                        }
                        row.spawn(Node { flex_grow: 1.0, ..default() });
                        let placed = (0..=ng.rivals.len()).any(|slot| ng.start(slot).is_some());
                        if placed {
                            row.spawn((
                                sized_button("Random starts", 170.0),
                                observe(|_: On<Activate>, mut ng: ResMut<NewGame>| ng.starts.clear()),
                            ));
                        }
                        let how = if ng.homes() {
                            "Drag a player's shield onto the map to choose their start; the others start in their homelands."
                        } else {
                            "Drag a player's shield onto the map to choose their start; the others start at random."
                        };
                        row.spawn(ui::text(
                            how,
                            13.0,
                            palette::TEXT_DIM,
                        ));
                    });
                });
                // Options and players.
                body.spawn((
                    Node {
                        width: px(470),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10),
                        padding: UiRect::all(px(20)),
                        border: UiRect::all(px(1.5)),
                        border_radius: BorderRadius::all(px(8)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    ui::frame(),
                ))
                .with_children(|opts| {
                    opts.spawn(ui::section("Options"));
                    let (w, h) = ng.size.dims();
                    stepper(opts, "Map size", ng.size.name().into(), Some(format!("{w} × {h} tiles, best for about {} rivals", ng.size.rivals())), |ng, rules, step| {
                        ng.size = cycle(&MapSize::ALL, ng.size, step);
                        let n = ng.size.rivals().min(rules.civ_count() - 1);
                        ng.rivals.resize(n, None);
                    });
                    map_type_row(opts, ng.map_type);
                    stepper(opts, "Sea level", ng.sea.name().into(), None, |ng, _, step| ng.sea = cycle(&SeaLevel::ALL, ng.sea, step));
                    if ng.map_type == MapType::Earth {
                        let (value, hint) = if ng.true_starts {
                            ("True locations", "Every civ starts where it arose in history: Rome in Italy, the Inca in the Andes.")
                        } else {
                            ("Balanced", "Civs start on good land spread around the world.")
                        };
                        stepper(opts, "Start locations", value.into(), Some(hint.into()), |ng, _, _| ng.true_starts = !ng.true_starts);
                    }
                    stepper(opts, "Rivals", ng.rivals.len().to_string(), None, |ng, rules, step| {
                        let n = (ng.rivals.len() as i32 + step).clamp(1, rules.civ_count() as i32 - 1) as usize;
                        ng.rivals.resize(n, None);
                        ng.starts.truncate(n + 1);
                    });
                    let limit = ng.unit_limit.map_or("No limit".to_string(), |n| format!("{n} units"));
                    stepper(opts, "Army size limit", limit, Some("Most military units each player may have.".into()), |ng, _, step| {
                        ng.unit_limit = cycle(&UNIT_LIMITS, ng.unit_limit, step)
                    });
                    let turns = ng.turn_limit.map_or("No limit".to_string(), |n| format!("{n} turns"));
                    let why = "The game ends after this turn and the highest score wins, unless someone conquers every capital first.";
                    stepper(opts, "Turn limit", turns, Some(why.into()), |ng, _, step| ng.turn_limit = cycle(&TURN_LIMITS, ng.turn_limit, step));
                    opts.spawn(ui::section("Players"));
                    // You.
                    let you = ng.civ.map(|c| rules.civ(c));
                    opts.spawn(player_row(icons, you, "You", true, 0, ng.start(0).is_some(), ng.homes())).with_children(|row| {
                        row.spawn((sized_button("Change", 110.0), observe(go(AppState::CivSelect))));
                    });
                    for (slot, civ) in ng.rivals.iter().enumerate() {
                        let def = civ.map(|c| rules.civ(c));
                        opts.spawn(player_row(icons, def, &format!("Rival {}", slot + 1), false, slot + 1, ng.start(slot + 1).is_some(), ng.homes())).with_children(|row| {
                            for step in [-1, 1] {
                                row.spawn((
                                    ui::small_button(if step < 0 { "<" } else { ">" }),
                                    observe(move |_: On<Activate>, mut ng: ResMut<NewGame>, rules: Res<RulesRes>| {
                                        let taken = ng.taken(Some(slot));
                                        let mut choices: Vec<Option<CivId>> = vec![None];
                                        choices.extend((0..rules.0.civ_count() as CivId).filter(|c| !taken.contains(c)).map(Some));
                                        ng.rivals[slot] = cycle(&choices, ng.rivals[slot], step);
                                    }),
                                ));
                            }
                        });
                    }
                });
            });
        root.spawn(footer()).with_children(|bar| {
            bar.spawn((sized_button("Back", 200.0), observe(go(AppState::CivSelect))));
            bar.spawn((
                primary_button("Start Game", 280.0),
                observe(|_: On<Activate>, mut commands: Commands, ng: Res<NewGame>, rules: Res<RulesRes>, mut next: ResMut<NextState<AppState>>| {
                    commands.insert_resource(ng.spec(&rules.0));
                    next.set(AppState::Loading);
                }),
            ));
        });
        if let Some(thumbs) = picker {
            spawn_type_picker(root, ng.map_type, thumbs);
        }
    });
}

// --- Choosing starts ---------------------------------------------------------

/// A player's shield in the setup list, dragged onto the map to place their start.
#[derive(Component)]
struct StartHandle(usize);

/// A placed start on the preview: dragged to move it, off the map or
/// right-clicked to make it random again.
#[derive(Component)]
struct StartPin(usize);

/// The shield following the pointer while a start is dragged.
#[derive(Component)]
struct StartGhost;

/// The player slot whose start is being dragged.
#[derive(Resource, Default)]
struct StartDrag(Option<usize>);

const PIN_SIZE: f32 = 34.0;

fn slot_name(slot: usize) -> String {
    if slot == 0 { "You".into() } else { format!("Rival {slot}") }
}

/// A placed start: the player's shield standing on its tile, named below.
fn start_pin(rules: &Rules, icons: &Icons, ng: &NewGame, slot: usize, uv: Vec2) -> impl Bundle {
    let civ = ng.slot_civ(slot).map(|c| rules.civ(c));
    let who = slot_name(slot);
    let title = civ.map_or(format!("{who}: random civ"), |c| format!("{who}: {}", c.name));
    let hint = if ng.start(slot).is_none() {
        "Its homeland. Drag to choose another start; right-click a moved start to send it home again."
    } else if ng.homes() {
        "Drag to move this start. Drag it off the map or right-click it to start in its homeland again."
    } else {
        "Drag to move this start. Drag it off the map or right-click it for a random start."
    };
    (
        StartPin(slot),
        Node {
            position_type: PositionType::Absolute,
            left: percent(uv.x * 100.0),
            top: percent(uv.y * 100.0),
            // The shield's point on the tile.
            margin: UiRect::new(px(-PIN_SIZE * 0.5), px(0), px(-PIN_SIZE), px(0)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            width: px(PIN_SIZE),
            ..default()
        },
        Tooltip::new(title, hint),
        children![
            (civ_badge(icons, civ, PIN_SIZE), Pickable::IGNORE),
            // The tile itself.
            (
                Node {
                    position_type: PositionType::Absolute,
                    top: px(PIN_SIZE - 3.0),
                    width: px(7),
                    height: px(7),
                    border: UiRect::all(px(1.5)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                BorderColor::all(Color::srgb(0.1, 0.08, 0.06)),
                Pickable::IGNORE,
            ),
            (
                Node { position_type: PositionType::Absolute, top: px(PIN_SIZE + 5.0), padding: UiRect::axes(px(4), px(0)), border_radius: BorderRadius::all(px(3)), ..default() },
                BackgroundColor(Color::srgba(0.05, 0.04, 0.035, 0.75)),
                Pickable::IGNORE,
                children![(ui::bold(who, 10.5, palette::TEXT), Pickable::IGNORE)],
            ),
        ],
    )
}

/// Whether a slot's start may go on a tile: land to walk on, not too close
/// to the other placed starts.
fn start_fits(ng: &NewGame, layout: &PreviewMap, rules: &Rules, slot: usize, tile: TileId) -> bool {
    let grid = &layout.map.grid;
    Game::may_start_at(&layout.map, rules, tile)
        && (0..=ng.rivals.len()).filter(|&o| o != slot).filter_map(|o| ng.start(o)).all(|o| grid.distance(o, tile) >= MIN_FIXED_SPACING)
}

/// Dragging shields from the player list (or placed ones) onto the map.
fn drag_starts(
    mut commands: Commands,
    (mut starts, mut drags, mut ends, mut clicks): (
        MessageReader<bevy::picking::events::PointerDragStart>,
        MessageReader<bevy::picking::events::PointerDrag>,
        MessageReader<bevy::picking::events::PointerDragEnd>,
        MessageReader<bevy::picking::events::PointerClick>,
    ),
    handles: Query<&StartHandle>,
    mut pins: Query<(&StartPin, &mut Visibility), Without<StartGhost>>,
    mut ghost: Query<(&mut Node, &mut ImageNode), With<StartGhost>>,
    ghosts: Query<Entity, With<StartGhost>>,
    picture: Query<(&ComputedNode, &UiGlobalTransform), With<PreviewImage>>,
    (window, ui_scale, preview, rules, icons): (Single<&Window>, Res<UiScale>, Res<Preview>, Res<RulesRes>, Res<Icons>),
    mut drag: ResMut<StartDrag>,
    mut ng: ResMut<NewGame>,
) {
    use bevy::picking::pointer::PointerButton;
    let primary = |b: PointerButton| b == PointerButton::Primary;
    // Where a window position is on the preview picture (0..1 each way).
    let on_picture = |pos: Vec2| -> Option<Vec2> {
        let (node, transform) = picture.iter().next()?;
        let uv = (pos * window.scale_factor() - transform.translation) / node.size() + Vec2::splat(0.5);
        (uv.cmpge(Vec2::ZERO).all() && uv.cmple(Vec2::ONE).all()).then_some(uv)
    };
    let ghost_at = |pos: Vec2| (px(pos.x / ui_scale.0 - PIN_SIZE * 0.5), px(pos.y / ui_scale.0 - PIN_SIZE));
    for e in clicks.read() {
        if let (PointerButton::Secondary, Ok((pin, _))) = (e.button, pins.get(e.entity)) {
            ng.set_start(pin.0, None);
        }
    }
    for e in starts.read() {
        if !primary(e.button) || preview.layout.is_none() {
            continue;
        }
        let slot = if let Ok(h) = handles.get(e.entity) {
            h.0
        } else if let Ok((pin, mut vis)) = pins.get_mut(e.entity) {
            *vis = Visibility::Hidden;
            pin.0
        } else {
            continue;
        };
        drag.0 = Some(slot);
        let civ = ng.slot_civ(slot).map(|c| rules.0.civ(c));
        let (left, top) = ghost_at(e.pointer.position);
        commands.spawn((Screen, StartGhost, civ_badge(&icons, civ, PIN_SIZE), GlobalZIndex(50), Pickable::IGNORE)).insert(Node {
            position_type: PositionType::Absolute,
            left,
            top,
            width: px(PIN_SIZE),
            height: px(PIN_SIZE),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        });
    }
    let Some(slot) = drag.0 else { return };
    let layout = preview.layout.as_ref();
    let fits = |ng: &NewGame, pos: Vec2| -> Option<Option<TileId>> {
        let uv = on_picture(pos)?;
        let tile = layout?.tile_at(uv)?;
        Some(start_fits(ng, layout?, &rules.0, slot, tile).then_some(tile))
    };
    for e in drags.read() {
        let pos = e.pointer.position;
        for (mut node, mut image) in &mut ghost {
            let (left, top) = ghost_at(pos);
            node.left = left;
            node.top = top;
            // Faded where it can't go.
            let alpha = if matches!(fits(&ng, pos), Some(None)) { 0.4 } else { 1.0 };
            image.color.set_alpha(alpha);
        }
    }
    for e in ends.read() {
        if !primary(e.button) {
            continue;
        }
        drag.0 = None;
        for g in &ghosts {
            commands.entity(g).despawn();
        }
        for (_, mut vis) in &mut pins {
            *vis = Visibility::Inherited;
        }
        match fits(&ng, e.pointer.position) {
            Some(Some(tile)) => ng.set_start(slot, Some(tile)),
            // Dropped off the map: back to a random start.
            None => {
                if ng.start(slot).is_some() {
                    ng.set_start(slot, None);
                }
            }
            Some(None) => {}
        }
    }
}

// --- Map type picker ---------------------------------------------------------

/// Whether the map type picker is open over the setup screen.
#[derive(Resource, Default)]
struct TypePicker(bool);

/// The picker starts closed, except that FOURX_MENU=maptypes opens it the
/// first time.
fn open_picker_at_start(mut picker: ResMut<TypePicker>, mut first: Local<bool>) {
    picker.0 = !*first && std::env::var("FOURX_MENU").as_deref() == Ok("maptypes");
    *first = true;
}

/// Each map type's picture, drawn in the background once the setup screen opens.
#[derive(Resource, Default)]
struct TypeThumbs {
    started: bool,
    jobs: Vec<(MapType, PreviewJob)>,
    images: HashMap<MapType, Handle<Image>>,
}

/// Where a map type's picture goes in the picker.
#[derive(Component)]
struct TypeThumb(MapType);

fn update_thumbs(
    mut thumbs: ResMut<TypeThumbs>,
    rules: Res<RulesRes>,
    state: Res<State<AppState>>,
    mut images: ResMut<Assets<Image>>,
    mut shown: Query<(&TypeThumb, &mut ImageNode, &mut Node)>,
) {
    if *state.get() != AppState::Setup {
        return;
    }
    if !thumbs.started {
        thumbs.started = true;
        let (width, height) = MapSize::Standard.dims();
        for kind in MapType::ALL {
            let settings = MapSettings { width, height, seed: showcase_seed(kind), kind, ..default() };
            thumbs.jobs.push((kind, PreviewJob::start(rules.0.clone(), settings, 360)));
        }
    }
    let mut i = 0;
    while i < thumbs.jobs.len() {
        if let Some(image) = thumbs.jobs[i].1.poll(&mut images) {
            let (kind, _) = thumbs.jobs.swap_remove(i);
            thumbs.images.insert(kind, image);
        } else {
            i += 1;
        }
    }
    for (thumb, mut node, mut layout) in &mut shown {
        if let Some(image) = thumbs.images.get(&thumb.0) {
            if node.image != *image {
                node.image = image.clone();
                layout.display = Display::Flex;
            }
        }
    }
}

/// The options row showing the map type: the name sits where the steppers
/// show their values, on a button as wide as their arrows and value, which
/// opens the picker.
fn map_type_row(parent: &mut ChildSpawnerCommands, kind: MapType) {
    let (name, blurb) = map_type_text(kind);
    parent
        .spawn(Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(2), ..default() })
        .with_children(|block| {
            block.spawn(Node { width: percent(100), align_items: AlignItems::Center, column_gap: px(8), ..default() }).with_children(|row| {
                row.spawn((Node { flex_grow: 1.0, ..default() }, children![ui::label("Map type")]));
                row.spawn((
                    // Two arrows (36), the value (170) and the gaps between (8).
                    ui::base_button(px(36.0 * 2.0 + 170.0 + 16.0), px(30)),
                    Tooltip::new("Map type", "Choose the shape of the world."),
                    children![(ui::value_text(name), Pickable::IGNORE)],
                    observe(|_: On<Activate>, mut picker: ResMut<TypePicker>| picker.0 = true),
                ));
            });
            block.spawn(ui::text(blurb, 12.5, palette::TEXT_DIM));
        });
}

/// The picker: a card per map type with a picture of a typical world.
fn spawn_type_picker(root: &mut ChildSpawnerCommands, current: MapType, thumbs: &TypeThumbs) {
    root.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        GlobalZIndex(5),
        // Clicks on the dimmed backdrop close the picker.
        Button,
        OwnStyle,
        observe(|_: On<Activate>, mut picker: ResMut<TypePicker>| picker.0 = false),
    ))
    .with_children(|dim| {
        dim.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(16),
                padding: UiRect::all(px(24)),
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::all(px(8)),
                max_width: percent(94),
                max_height: percent(94),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            ui::frame(),
            // Clicks inside the panel stay here.
            Button,
            OwnStyle,
        ))
        .with_children(|panel| {
            panel.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|top| {
                top.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() }).with_children(|t| {
                    t.spawn((ui::heading("Map Type", 28.0, palette::ACCENT), TextShadow::default()));
                    t.spawn(ui::text("The shape of the world: how land and sea are laid out. Each picture is a typical world of its kind.", 14.0, palette::TEXT_DIM));
                });
                top.spawn((sized_button("Close", 120.0), observe(|_: On<Activate>, mut picker: ResMut<TypePicker>| picker.0 = false)));
            });
            panel
                .spawn(Node {
                    display: Display::Grid,
                    grid_template_columns: RepeatedGridTrack::px(3, 340.0),
                    column_gap: px(14),
                    row_gap: px(14),
                    ..default()
                })
                .with_children(|grid| {
                    for kind in MapType::ALL {
                        grid.spawn(type_card(kind, kind == current, thumbs.images.get(&kind).cloned()));
                    }
                });
        });
    });
}

fn type_card(kind: MapType, selected: bool, image: Option<Handle<Image>>) -> impl Bundle {
    let (name, blurb) = map_type_text(kind);
    let (w, h) = MapSize::Standard.dims();
    (
        Button,
        Hovered::default(),
        OwnStyle,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::all(px(10)),
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(palette::BUTTON),
        BorderColor::all(palette::BUTTON_BORDER),
        ButtonState { active: selected, ..default() },
        StripeColor(if selected { palette::ACCENT } else { palette::BUTTON_BORDER }),
        observe(move |_: On<Activate>, mut ng: ResMut<NewGame>, mut picker: ResMut<TypePicker>| {
            ng.map_type = kind;
            picker.0 = false;
        }),
        children![
            (
                Node {
                    width: percent(100),
                    aspect_ratio: Some(w as f32 * 1.732 / (h as f32 * 1.5 + 0.5)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.07, 0.1, 0.13)),
                BorderColor::all(palette::PANEL_BORDER.with_alpha(0.4)),
                Pickable::IGNORE,
                children![(
                    TypeThumb(kind),
                    ImageNode::new(image.clone().unwrap_or_default()),
                    Node { width: percent(100), height: percent(100), display: if image.is_some() { Display::Flex } else { Display::None }, ..default() },
                    Pickable::IGNORE,
                )],
            ),
            (ui::heading(name, 19.0, if selected { palette::ACCENT } else { palette::TEXT }), Pickable::IGNORE),
            (ui::text(blurb, 13.0, palette::TEXT_DIM), Pickable::IGNORE),
        ],
    )
}

/// A player in the setup list: badge, civ and leader. Controls go in as children.
fn player_row(icons: &Icons, civ: Option<&CivDef>, who: &str, you: bool, slot: usize, placed: bool, homes: bool) -> impl Bundle {
    let (name, leader) = match civ {
        Some(c) => (c.name.clone(), c.leader.clone()),
        None => ("Random".to_string(), "Any civilization".to_string()),
    };
    let start = if placed {
        "Start chosen on the map"
    } else if homes && civ.is_none_or(|c| c.home.is_some()) {
        "Starts in its homeland"
    } else {
        "Random start"
    };
    let stripe = civ.map_or(palette::TEXT_DIM, |c| rgb(c.colors.primary));
    (
        Node {
            width: percent(100),
            align_items: AlignItems::Center,
            column_gap: px(10),
            padding: UiRect::axes(px(10), px(6)),
            border: UiRect::new(px(3), px(1), px(1), px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(if you { palette::BUTTON_ACTIVE } else { palette::PANEL_LIGHT }),
        BorderColor { left: stripe, ..BorderColor::all(palette::PANEL_BORDER.with_alpha(0.45)) },
        children![
            (
                civ_badge(icons, civ, 40.0),
                StartHandle(slot),
                Tooltip::new("Choose a start", "Drag this shield onto the map to choose where they start."),
            ),
            (
                Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, ..default() },
                children![
                    ui::bold(who.to_uppercase(), 10.5, palette::TEXT_DIM),
                    ui::heading(name, 16.0, palette::TEXT),
                    ui::text(leader, 12.5, palette::TEXT_DIM),
                    ui::text(start, 11.5, if placed { palette::ACCENT } else { palette::TEXT_DIM }),
                ],
            ),
        ],
    )
}
