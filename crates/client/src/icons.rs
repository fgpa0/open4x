//! Procedural UI icons: small vector drawings rendered at startup with
//! tiny-skia (no third-party art). Each icon gets a shared dark outline, a soft
//! drop shadow and top-lit gradient fills, in the spirit of strategy-game icons.
//! A white variant of every icon can be tinted (unit flags use player colours).

use crate::image_util::rgba_image;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use std::collections::HashMap;
use tiny_skia::{
    Color as SkColor, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, RadialGradient, Path, PathBuilder, Pixmap,
    PixmapPaint, Point, Rect, Shader, SpreadMode, Stroke, Transform,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    // Yields and stats.
    Food,
    Production,
    Gold,
    Culture,
    Science,
    Population,
    Strength,
    Defense,
    Moves,
    Sight,
    Health,
    Turns,
    /// The time of day in the top bar.
    Daytime,
    Night,
    // Units.
    Settler,
    Worker,
    Scout,
    Warrior,
    Archer,
    Spearman,
    Catapult,
    Galley,
    Quadrireme,
    Horseman,
    Swordsman,
    ChariotArcher,
    CompositeBowman,
    Pikeman,
    Crossbowman,
    Knight,
    Trebuchet,
    Longswordsman,
    Musketman,
    Cannon,
    Caravel,
    Frigate,
    Slinger,
    Courser,
    Cuirassier,
    Explorer,
    Galleon,
    Rifleman,
    Cavalry,
    Artillery,
    Ironclad,
    ShipOfTheLine,
    Infantry,
    MachineGun,
    Tank,
    Destroyer,
    Battleship,
    Merchant,
    General,
    Admiral,
    /// An army (its standard).
    Army,
    /// A veteran's chevrons (experience and rank).
    Rank,
    // Improvements.
    Farm,
    Mine,
    Pasture,
    Camp,
    Plantation,
    Quarry,
    LumberMill,
    FishingBoats,
    OilWell,
    Road,
    Railroad,
    Tunnel,
    Canal,
    Bridge,
    // Resources.
    Wheat,
    Cattle,
    Sheep,
    Deer,
    Bananas,
    Stone,
    Fish,
    Wine,
    Silk,
    Spices,
    Incense,
    Furs,
    Ivory,
    Gems,
    GoldOre,
    Salt,
    Marble,
    Pearls,
    Horses,
    Iron,
    Niter,
    Coal,
    Oil,
    // Buildings.
    Monument,
    Granary,
    Workshop,
    Market,
    Walls,
    Library,
    Amphitheater,
    Temple,
    Harbor,
    Aqueduct,
    University,
    Castle,
    Observatory,
    Bank,
    StarFort,
    Shrine,
    Barracks,
    Lighthouse,
    Stable,
    Armory,
    Caravanserai,
    Shipyard,
    OperaHouse,
    Factory,
    StockExchange,
    PublicSchool,
    Museum,
    MilitaryAcademy,
    Arsenal,
    Hospital,
    Supermarket,
    PowerPlant,
    MilitaryBase,
    BroadcastTower,
    ResearchLab,
    // Orders and actions.
    Explore,
    Sleep,
    Skip,
    FoundCity,
    EndTurn,
    Balanced,
    Auto,
    Upgrade,
    /// Trade routes.
    Trade,
    /// Keep doing the same (a merchant's route).
    Repeat,
    /// Move to another of our cities (a merchant's transfer).
    MoveToCity,
    // Interface.
    Close,
    Menu,
    Warning,
    Check,
    Log,
    Map,
    River,
    // Civilization emblems (drawn to be tinted with the civ's colours).
    Banner,
    Windmill,
    Laurel,
    Ankh,
    Bow,
    Sun,
    Longship,
    Taegeuk,
    Sunstone,
    WingedDisc,
    Chakana,
    Random,
}

impl Icon {
    pub const ALL: [Icon; 158] = [
        Icon::Food,
        Icon::Production,
        Icon::Gold,
        Icon::Culture,
        Icon::Science,
        Icon::Population,
        Icon::Strength,
        Icon::Defense,
        Icon::Moves,
        Icon::Sight,
        Icon::Health,
        Icon::Turns,
        Icon::Daytime,
        Icon::Night,
        Icon::Settler,
        Icon::Worker,
        Icon::Scout,
        Icon::Warrior,
        Icon::Archer,
        Icon::Spearman,
        Icon::Catapult,
        Icon::Galley,
        Icon::Quadrireme,
        Icon::Horseman,
        Icon::Swordsman,
        Icon::ChariotArcher,
        Icon::CompositeBowman,
        Icon::Pikeman,
        Icon::Crossbowman,
        Icon::Knight,
        Icon::Trebuchet,
        Icon::Longswordsman,
        Icon::Musketman,
        Icon::Cannon,
        Icon::Caravel,
        Icon::Frigate,
        Icon::Slinger,
        Icon::Courser,
        Icon::Cuirassier,
        Icon::Explorer,
        Icon::Galleon,
        Icon::Rifleman,
        Icon::Cavalry,
        Icon::Artillery,
        Icon::Ironclad,
        Icon::ShipOfTheLine,
        Icon::Infantry,
        Icon::MachineGun,
        Icon::Tank,
        Icon::Destroyer,
        Icon::Battleship,
        Icon::Merchant,
        Icon::General,
        Icon::Admiral,
        Icon::Army,
        Icon::Rank,
        Icon::Farm,
        Icon::Mine,
        Icon::Pasture,
        Icon::Camp,
        Icon::Plantation,
        Icon::Quarry,
        Icon::LumberMill,
        Icon::FishingBoats,
        Icon::OilWell,
        Icon::Road,
        Icon::Railroad,
        Icon::Tunnel,
        Icon::Canal,
        Icon::Bridge,
        Icon::Wheat,
        Icon::Cattle,
        Icon::Sheep,
        Icon::Deer,
        Icon::Bananas,
        Icon::Stone,
        Icon::Fish,
        Icon::Wine,
        Icon::Silk,
        Icon::Spices,
        Icon::Incense,
        Icon::Furs,
        Icon::Ivory,
        Icon::Gems,
        Icon::GoldOre,
        Icon::Salt,
        Icon::Marble,
        Icon::Pearls,
        Icon::Horses,
        Icon::Iron,
        Icon::Niter,
        Icon::Coal,
        Icon::Oil,
        Icon::Monument,
        Icon::Granary,
        Icon::Workshop,
        Icon::Market,
        Icon::Walls,
        Icon::Library,
        Icon::Amphitheater,
        Icon::Temple,
        Icon::Harbor,
        Icon::Aqueduct,
        Icon::University,
        Icon::Castle,
        Icon::Observatory,
        Icon::Bank,
        Icon::StarFort,
        Icon::Shrine,
        Icon::Barracks,
        Icon::Lighthouse,
        Icon::Stable,
        Icon::Armory,
        Icon::Caravanserai,
        Icon::Shipyard,
        Icon::OperaHouse,
        Icon::Factory,
        Icon::StockExchange,
        Icon::PublicSchool,
        Icon::Museum,
        Icon::MilitaryAcademy,
        Icon::Arsenal,
        Icon::Hospital,
        Icon::Supermarket,
        Icon::PowerPlant,
        Icon::MilitaryBase,
        Icon::BroadcastTower,
        Icon::ResearchLab,
        Icon::Explore,
        Icon::Sleep,
        Icon::Skip,
        Icon::FoundCity,
        Icon::EndTurn,
        Icon::Balanced,
        Icon::Auto,
        Icon::Upgrade,
        Icon::Trade,
        Icon::Repeat,
        Icon::MoveToCity,
        Icon::Close,
        Icon::Menu,
        Icon::Warning,
        Icon::Check,
        Icon::Log,
        Icon::Map,
        Icon::River,
        Icon::Banner,
        Icon::Windmill,
        Icon::Laurel,
        Icon::Ankh,
        Icon::Bow,
        Icon::Sun,
        Icon::Longship,
        Icon::Taegeuk,
        Icon::Sunstone,
        Icon::WingedDisc,
        Icon::Chakana,
        Icon::Random,
    ];

    /// The icon for a unit's `art` name from the rules.
    pub fn unit(art: &str) -> Icon {
        match art {
            "settler" => Icon::Settler,
            "worker" => Icon::Worker,
            "scout" => Icon::Scout,
            "warrior" => Icon::Warrior,
            "archer" => Icon::Archer,
            "spearman" => Icon::Spearman,
            "catapult" => Icon::Catapult,
            "galley" => Icon::Galley,
            "quadrireme" => Icon::Quadrireme,
            "horseman" => Icon::Horseman,
            "swordsman" => Icon::Swordsman,
            "chariot_archer" => Icon::ChariotArcher,
            "composite_bowman" => Icon::CompositeBowman,
            "pikeman" => Icon::Pikeman,
            "crossbowman" => Icon::Crossbowman,
            "knight" => Icon::Knight,
            "trebuchet" => Icon::Trebuchet,
            "longswordsman" => Icon::Longswordsman,
            "musketman" => Icon::Musketman,
            "cannon" => Icon::Cannon,
            "caravel" => Icon::Caravel,
            "frigate" => Icon::Frigate,
            "slinger" => Icon::Slinger,
            "courser" => Icon::Courser,
            "cuirassier" => Icon::Cuirassier,
            "explorer" => Icon::Explorer,
            "galleon" => Icon::Galleon,
            "rifleman" => Icon::Rifleman,
            "cavalry" => Icon::Cavalry,
            "artillery" => Icon::Artillery,
            "ironclad" => Icon::Ironclad,
            "ship_of_the_line" => Icon::ShipOfTheLine,
            "infantry" => Icon::Infantry,
            "machine_gun" => Icon::MachineGun,
            "tank" => Icon::Tank,
            "destroyer" => Icon::Destroyer,
            "battleship" => Icon::Battleship,
            "merchant" => Icon::Merchant,
            "general" => Icon::General,
            "admiral" => Icon::Admiral,
            _ => Icon::Strength,
        }
    }

    /// The icon for an improvement's `art` name from the rules.
    pub fn improvement(art: &str) -> Icon {
        match art {
            "farm" => Icon::Farm,
            "mine" => Icon::Mine,
            "pasture" => Icon::Pasture,
            "camp" => Icon::Camp,
            "plantation" => Icon::Plantation,
            "quarry" => Icon::Quarry,
            "lumber_mill" => Icon::LumberMill,
            "fishing_boats" => Icon::FishingBoats,
            "oil_well" => Icon::OilWell,
            _ => Icon::Worker,
        }
    }

    /// The icon for a resource id from the rules.
    pub fn resource(id: &str) -> Icon {
        match id {
            "wheat" => Icon::Wheat,
            "cattle" => Icon::Cattle,
            "sheep" => Icon::Sheep,
            "deer" => Icon::Deer,
            "bananas" => Icon::Bananas,
            "stone" => Icon::Stone,
            "fish" => Icon::Fish,
            "wine" => Icon::Wine,
            "silk" => Icon::Silk,
            "spices" => Icon::Spices,
            "incense" => Icon::Incense,
            "furs" => Icon::Furs,
            "ivory" => Icon::Ivory,
            "gems" => Icon::Gems,
            "gold" => Icon::GoldOre,
            "salt" => Icon::Salt,
            "marble" => Icon::Marble,
            "pearls" => Icon::Pearls,
            "horses" => Icon::Horses,
            "iron" => Icon::Iron,
            "niter" => Icon::Niter,
            "coal" => Icon::Coal,
            "oil" => Icon::Oil,
            _ => Icon::Random,
        }
    }

    /// The emblem for a civ's `emblem` name from the rules.
    pub fn emblem(name: &str) -> Icon {
        match name {
            "windmill" => Icon::Windmill,
            "laurel" => Icon::Laurel,
            "ankh" => Icon::Ankh,
            "bow" => Icon::Bow,
            "sun" => Icon::Sun,
            "longship" => Icon::Longship,
            "taegeuk" => Icon::Taegeuk,
            "sunstone" => Icon::Sunstone,
            "winged_disc" => Icon::WingedDisc,
            "chakana" => Icon::Chakana,
            _ => Icon::Random,
        }
    }

    /// The icon for a building id from the rules.
    pub fn building(id: &str) -> Icon {
        match id {
            "monument" => Icon::Monument,
            "granary" => Icon::Granary,
            "workshop" => Icon::Workshop,
            "market" => Icon::Market,
            "walls" => Icon::Walls,
            "library" => Icon::Library,
            "amphitheater" => Icon::Amphitheater,
            "temple" => Icon::Temple,
            "harbor" => Icon::Harbor,
            "aqueduct" => Icon::Aqueduct,
            "university" => Icon::University,
            "castle" => Icon::Castle,
            "observatory" => Icon::Observatory,
            "bank" => Icon::Bank,
            "star_fort" => Icon::StarFort,
            "shrine" => Icon::Shrine,
            "barracks" => Icon::Barracks,
            "lighthouse" => Icon::Lighthouse,
            "stable" => Icon::Stable,
            "armory" => Icon::Armory,
            "caravanserai" => Icon::Caravanserai,
            "shipyard" => Icon::Shipyard,
            "opera_house" => Icon::OperaHouse,
            "factory" => Icon::Factory,
            "stock_exchange" => Icon::StockExchange,
            "public_school" => Icon::PublicSchool,
            "museum" => Icon::Museum,
            "military_academy" => Icon::MilitaryAcademy,
            "arsenal" => Icon::Arsenal,
            "hospital" => Icon::Hospital,
            "supermarket" => Icon::Supermarket,
            "power_plant" => Icon::PowerPlant,
            "military_base" => Icon::MilitaryBase,
            "broadcast_tower" => Icon::BroadcastTower,
            "research_lab" => Icon::ResearchLab,
            // Unique buildings look like what they replace.
            "waag" => Icon::Market,
            "forum" => Icon::Monument,
            "mortuary_temple" | "pairidaeza" => Icon::Temple,
            "ordu" | "hwaseong" => Icon::Walls,
            "sankore" => Icon::Library,
            "longhouse" => Icon::Workshop,
            "tlachtli" => Icon::Amphitheater,
            "qullqa" => Icon::Granary,
            _ => Icon::FoundCity,
        }
    }
}

/// Rendered icons: full colour and white (for tinting).
#[derive(Resource)]
pub struct Icons {
    color: HashMap<Icon, Handle<Image>>,
    white: HashMap<Icon, Handle<Image>>,
}

impl Icons {
    pub fn get(&self, icon: Icon) -> Handle<Image> {
        self.color[&icon].clone()
    }

    pub fn white(&self, icon: Icon) -> Handle<Image> {
        self.white[&icon].clone()
    }
}

/// Pixel size icons are rendered at (mipmaps cover smaller sizes).
const SIZE: u32 = 128;

pub struct IconsPlugin;

impl Plugin for IconsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, (create, create_atlas));
    }
}

fn create(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut color = HashMap::new();
    let mut white = HashMap::new();
    for icon in Icon::ALL {
        for (white_variant, map) in [(false, &mut color), (true, &mut white)] {
            let pixmap = render(icon, SIZE, white_variant);
            let mut image = rgba_image(straight_rgba(&pixmap), SIZE, SIZE, true, false, false);
            // UI layout reads image sizes in the main world.
            image.asset_usage = RenderAssetUsages::default();
            map.insert(icon, images.add(image));
        }
    }
    // Dev aid: FOURX_ICON_SHEET=path.png writes every icon to one image.
    if let Ok(path) = std::env::var("FOURX_ICON_SHEET") {
        write_sheet(&path);
    }
    commands.insert_resource(Icons { color, white });
}

/// Something drawn in a [`GlyphAtlas`]: an icon, a digit, or the soft shadow
/// map icons sit on so they stand out from the ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Icon(Icon),
    Digit(u8),
    Disc,
}

/// Icons and digits in one texture, for map overlays drawn as textured quads.
#[derive(Resource)]
pub struct GlyphAtlas {
    pub image: Handle<Image>,
    /// Each glyph's place in the texture (UV min and max).
    cells: HashMap<Glyph, (Vec2, Vec2)>,
}

impl GlyphAtlas {
    pub fn uv(&self, g: Glyph) -> (Vec2, Vec2) {
        self.cells.get(&g).copied().unwrap_or((Vec2::ZERO, Vec2::ZERO))
    }
}

/// Pixels per atlas cell.
const CELL: u32 = 96;

/// Builds the atlas with every icon and the digits 0 to 9.
fn create_atlas(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let glyphs: Vec<Glyph> = Icon::ALL.into_iter().map(Glyph::Icon).chain((0..10).map(Glyph::Digit)).chain([Glyph::Disc]).collect();
    let cols = 12u32;
    let rows = (glyphs.len() as u32).div_ceil(cols);
    let mut sheet = Pixmap::new(CELL * cols, CELL * rows).unwrap();
    let mut cells = HashMap::new();
    for (i, &g) in glyphs.iter().enumerate() {
        let (x, y) = ((i as u32 % cols) * CELL, (i as u32 / cols) * CELL);
        let p = match g {
            Glyph::Icon(icon) => render(icon, CELL, false),
            Glyph::Digit(d) => digit(d, CELL),
            Glyph::Disc => disc(CELL),
        };
        sheet.draw_pixmap(x as i32, y as i32, p.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        // A pixel of margin keeps neighbours from bleeding in.
        let (w, h) = ((CELL * cols) as f32, (CELL * rows) as f32);
        cells.insert(g, (Vec2::new((x as f32 + 1.0) / w, (y as f32 + 1.0) / h), Vec2::new((x + CELL) as f32 / w - 1.0 / w, (y + CELL) as f32 / h - 1.0 / h)));
    }
    let image = rgba_image(straight_rgba(&sheet), CELL * cols, CELL * rows, true, false, false);
    commands.insert_resource(GlyphAtlas { image: images.add(image), cells });
}

/// A soft dark shadow, faint at the middle and fading to nothing at the edge,
/// so map icons read on bright ground without a hard badge around them.
fn disc(size: u32) -> Pixmap {
    let mut out = Pixmap::new(size, size).unwrap();
    let ts = Transform::from_scale(size as f32 / 100.0, size as f32 / 100.0);
    let shadow = (20, 18, 16);
    let stops = vec![
        GradientStop::new(0.0, sk(shadow, 0.34)),
        GradientStop::new(0.55, sk(shadow, 0.26)),
        GradientStop::new(1.0, sk(shadow, 0.0)),
    ];
    let shader = RadialGradient::new(Point::from_xy(50.0, 50.0), Point::from_xy(50.0, 50.0), 48.0, stops, SpreadMode::Pad, Transform::identity());
    let paint = Paint { anti_alias: true, shader: shader.unwrap(), ..Paint::default() };
    out.fill_path(&circle(50.0, 50.0, 48.0), &paint, FillRule::Winding, ts, None);
    out
}

/// A bold digit, light with a dark outline, filling most of a cell.
fn digit(d: u8, size: u32) -> Pixmap {
    // Strokes in a 100x100 box.
    let paths: Vec<Path> = match d {
        0 => vec![oval(50.0, 50.0, 22.0, 34.0, 0.0)],
        1 => vec![polyline(&[(36.0, 30.0), (54.0, 16.0), (54.0, 84.0)]), polyline(&[(36.0, 84.0), (70.0, 84.0)])],
        2 => {
            let mut pb = PathBuilder::new();
            pb.move_to(30.0, 32.0);
            pb.quad_to(34.0, 14.0, 52.0, 14.0);
            pb.quad_to(72.0, 14.0, 72.0, 34.0);
            pb.quad_to(72.0, 50.0, 30.0, 84.0);
            pb.line_to(74.0, 84.0);
            vec![pb.finish().unwrap()]
        }
        3 => vec![arc(50.0, 32.0, 19.0, 200.0, 450.0), arc(50.0, 66.0, 20.0, 270.0, 520.0)],
        4 => vec![polyline(&[(62.0, 84.0), (62.0, 16.0), (26.0, 64.0), (76.0, 64.0)])],
        5 => {
            let mut pb = PathBuilder::new();
            pb.move_to(70.0, 16.0);
            pb.line_to(36.0, 16.0);
            pb.line_to(32.0, 46.0);
            pb.quad_to(44.0, 40.0, 52.0, 40.0);
            pb.quad_to(72.0, 42.0, 72.0, 62.0);
            pb.quad_to(70.0, 84.0, 50.0, 84.0);
            pb.quad_to(34.0, 84.0, 28.0, 74.0);
            vec![pb.finish().unwrap()]
        }
        6 => {
            let mut pb = PathBuilder::new();
            pb.move_to(66.0, 18.0);
            pb.quad_to(30.0, 24.0, 30.0, 62.0);
            vec![pb.finish().unwrap(), circle(51.0, 64.0, 20.0)]
        }
        7 => vec![polyline(&[(28.0, 16.0), (72.0, 16.0), (44.0, 84.0)])],
        8 => vec![circle(50.0, 32.0, 17.0), circle(50.0, 66.0, 19.0)],
        _ => {
            let mut pb = PathBuilder::new();
            pb.move_to(34.0, 82.0);
            pb.quad_to(70.0, 76.0, 70.0, 38.0);
            vec![pb.finish().unwrap(), circle(49.0, 36.0, 20.0)]
        }
    };
    let scale = size as f32 / 100.0;
    let ts = Transform::from_scale(scale, scale);
    let mut out = Pixmap::new(size, size).unwrap();
    let stroke = |w: f32| Stroke { width: w, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    let paint = |c: SkColor| {
        let mut p = Paint::default();
        p.set_color(c);
        p.anti_alias = true;
        p
    };
    for path in &paths {
        out.stroke_path(path, &paint(sk(OUTLINE, 1.0)), &stroke(26.0), ts, None);
    }
    for path in &paths {
        out.stroke_path(path, &paint(sk((250, 246, 236), 1.0)), &stroke(13.0), ts, None);
    }
    out
}

fn straight_rgba(pixmap: &Pixmap) -> Vec<u8> {
    pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

fn write_sheet(path: &str) {
    let cell = 96;
    let cols = 9;
    let rows = Icon::ALL.len().div_ceil(cols) as u32 * 2;
    let mut sheet = Pixmap::new(cell * cols as u32, cell * rows).unwrap();
    sheet.fill(SkColor::from_rgba8(58, 52, 44, 255));
    for (i, icon) in Icon::ALL.into_iter().enumerate() {
        for (k, white) in [false, true].into_iter().enumerate() {
            let p = render(icon, cell, white);
            let row = (i / cols) as u32 * 2 + k as u32;
            let x = (i % cols) as i32 * cell as i32;
            sheet.draw_pixmap(x, (row * cell) as i32, p.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }
    let _ = sheet.save_png(path);
}

// --- Drawing ---------------------------------------------------------------

type Rgb = (u8, u8, u8);

enum Kind {
    /// Part of the silhouette: outlined, shadowed, gradient filled.
    Body(Rgb),
    /// Drawn on top of the bodies without an outline.
    Detail(Rgb, f32),
    /// A stroked line on top of the bodies.
    Line(Rgb, f32, f32),
}

struct Part {
    path: Path,
    kind: Kind,
    even_odd: bool,
}

/// An icon being described, in a 100x100 space.
#[derive(Default)]
struct Sketch {
    parts: Vec<Part>,
}

impl Sketch {
    fn body(&mut self, path: Path, c: Rgb) -> &mut Self {
        self.parts.push(Part { path, kind: Kind::Body(c), even_odd: false });
        self
    }

    /// A body with holes (inner paths cut out).
    fn body_eo(&mut self, path: Path, c: Rgb) -> &mut Self {
        self.parts.push(Part { path, kind: Kind::Body(c), even_odd: true });
        self
    }

    fn detail(&mut self, path: Path, c: Rgb) -> &mut Self {
        self.parts.push(Part { path, kind: Kind::Detail(c, 1.0), even_odd: false });
        self
    }

    fn detail_alpha(&mut self, path: Path, c: Rgb, alpha: f32) -> &mut Self {
        self.parts.push(Part { path, kind: Kind::Detail(c, alpha), even_odd: false });
        self
    }

    fn line(&mut self, path: Path, c: Rgb, width: f32) -> &mut Self {
        self.parts.push(Part { path, kind: Kind::Line(c, width, 1.0), even_odd: false });
        self
    }

    fn line_alpha(&mut self, path: Path, c: Rgb, width: f32, alpha: f32) -> &mut Self {
        self.parts.push(Part { path, kind: Kind::Line(c, width, alpha), even_odd: false });
        self
    }

    /// Rotates every part so far around the centre.
    fn rotate(&mut self, degrees: f32) -> &mut Self {
        let t = Transform::from_rotate_at(degrees, 50.0, 50.0);
        for p in &mut self.parts {
            p.path = p.path.clone().transform(t).unwrap();
        }
        self
    }
}

const OUTLINE: Rgb = (28, 21, 15);

fn sk(c: Rgb, alpha: f32) -> SkColor {
    SkColor::from_rgba8(c.0, c.1, c.2, (alpha.clamp(0.0, 1.0) * 255.0) as u8)
}

fn shade(c: Rgb, f: f32, add: f32) -> Rgb {
    let ch = |v: u8| ((v as f32 * f + add * 255.0).clamp(0.0, 255.0)) as u8;
    (ch(c.0), ch(c.1), ch(c.2))
}

fn grey(c: Rgb) -> Rgb {
    // White variant: keep a little of the value structure so details read.
    let l = (0.3 * c.0 as f32 + 0.59 * c.1 as f32 + 0.11 * c.2 as f32) / 255.0;
    let v = (200.0 + 55.0 * l) as u8;
    (v, v, v)
}

fn render(icon: Icon, size: u32, white: bool) -> Pixmap {
    let mut s = Sketch::default();
    draw(icon, &mut s);
    let scale = size as f32 / 100.0;
    let ts = Transform::from_scale(scale, scale);
    let mut out = Pixmap::new(size, size).unwrap();
    let round = |w: f32| Stroke { width: w, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    let rule = |p: &Part| if p.even_odd { FillRule::EvenOdd } else { FillRule::Winding };
    let solid = |c: SkColor| {
        let mut paint = Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        paint
    };

    // Drop shadow: the silhouette, offset down, composited at partial opacity.
    let mut shadow = Pixmap::new(size, size).unwrap();
    let black = solid(SkColor::BLACK);
    let down = ts.pre_translate(0.0, 3.5);
    for p in s.parts.iter().filter(|p| matches!(p.kind, Kind::Body(_))) {
        shadow.fill_path(&p.path, &black, rule(p), down, None);
        shadow.stroke_path(&p.path, &black, &round(10.0), down, None);
    }
    out.draw_pixmap(0, 0, shadow.as_ref(), &PixmapPaint { opacity: 0.45, ..Default::default() }, Transform::identity(), None);

    // Shared outline under all bodies.
    let outline = solid(sk(OUTLINE, 1.0));
    for p in s.parts.iter().filter(|p| matches!(p.kind, Kind::Body(_))) {
        out.stroke_path(&p.path, &outline, &round(7.0), ts, None);
        out.fill_path(&p.path, &outline, rule(p), ts, None);
    }

    for p in &s.parts {
        match p.kind {
            Kind::Body(c) => {
                let c = if white { grey(c) } else { c };
                let b = p.path.bounds();
                let shader = LinearGradient::new(
                    Point::from_xy(0.0, b.top()),
                    Point::from_xy(0.0, b.bottom()),
                    vec![
                        GradientStop::new(0.0, sk(shade(c, 1.12, 0.1), 1.0)),
                        GradientStop::new(0.55, sk(c, 1.0)),
                        GradientStop::new(1.0, sk(shade(c, 0.72, 0.0), 1.0)),
                    ],
                    SpreadMode::Pad,
                    Transform::identity(),
                )
                .unwrap_or(Shader::SolidColor(sk(c, 1.0)));
                let paint = Paint { shader, anti_alias: true, ..Default::default() };
                out.fill_path(&p.path, &paint, rule(p), ts, None);
            }
            Kind::Detail(c, a) => {
                let c = if white { shade(grey(c), 0.6, 0.0) } else { c };
                out.fill_path(&p.path, &solid(sk(c, a)), rule(p), ts, None);
            }
            Kind::Line(c, w, a) => {
                let c = if white { shade(grey(c), 0.6, 0.0) } else { c };
                out.stroke_path(&p.path, &solid(sk(c, a)), &round(w), ts, None);
            }
        }
    }
    out
}

// --- Shape helpers (100x100 space, y down) ----------------------------------

fn circle(x: f32, y: f32, r: f32) -> Path {
    PathBuilder::from_circle(x, y, r).unwrap()
}

fn oval(x: f32, y: f32, rx: f32, ry: f32, degrees: f32) -> Path {
    let p = PathBuilder::from_oval(Rect::from_xywh(x - rx, y - ry, rx * 2.0, ry * 2.0).unwrap()).unwrap();
    p.transform(Transform::from_rotate_at(degrees, x, y)).unwrap()
}

fn poly(points: &[(f32, f32)]) -> Path {
    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish().unwrap()
}

fn polyline(points: &[(f32, f32)]) -> Path {
    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        pb.line_to(x, y);
    }
    pb.finish().unwrap()
}

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish().unwrap()
}

/// An open arc (degrees, clockwise from +x since y points down).
fn arc(cx: f32, cy: f32, r: f32, from: f32, to: f32) -> Path {
    let n = 24;
    let pts: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / n as f32).to_radians();
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    polyline(&pts)
}

/// A line turned into a fillable shape (so it gets an outline).
fn thick(path: Path, width: f32) -> Path {
    let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    path.stroke(&stroke, 4.0).unwrap()
}

fn combine(paths: &[Path]) -> Path {
    let mut pb = PathBuilder::new();
    for p in paths {
        pb.push_path(p);
    }
    pb.finish().unwrap()
}

/// A ship's hull seen from the side, from `x0` to `x1`, its deck at `top`
/// and keel at `bottom`; the bow (right) and stern (left) curve up.
fn galley_hull(x0: f32, x1: f32, top: f32, bottom: f32) -> Path {
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + 6.0, top - 4.0);
    pb.line_to(x1 - 4.0, top);
    pb.quad_to(x1 - 10.0, bottom, x1 - 26.0, bottom);
    pb.line_to(x0 + 22.0, bottom);
    pb.quad_to(x0 + 6.0, bottom - 2.0, x0 + 6.0, top - 4.0);
    pb.close();
    pb.finish().unwrap()
}

fn rotated(path: Path, degrees: f32, cx: f32, cy: f32) -> Path {
    path.transform(Transform::from_rotate_at(degrees, cx, cy)).unwrap()
}

fn letter_z(x: f32, y: f32, s: f32) -> Path {
    let pts = [
        (0.0, 0.0),
        (42.0, 0.0),
        (42.0, 10.0),
        (16.0, 40.0),
        (42.0, 40.0),
        (42.0, 50.0),
        (0.0, 50.0),
        (0.0, 40.0),
        (26.0, 10.0),
        (0.0, 10.0),
    ];
    poly(&pts.map(|(px, py)| (x + px * s, y + py * s)))
}

// --- The icons ---------------------------------------------------------------

const WOOD: Rgb = (150, 98, 55);
const DARK_WOOD: Rgb = (105, 70, 42);
const STEEL: Rgb = (188, 194, 206);
const BRASS: Rgb = (214, 172, 78);
const STONE: Rgb = (190, 182, 166);
const GOLD: Rgb = (244, 196, 62);
const CREAM: Rgb = (238, 228, 204);

fn draw(icon: Icon, s: &mut Sketch) {
    match icon {
        Icon::Food => {
            // A green apple with a leaf: food reads green, apart from gold.
            let skin = (112, 196, 70);
            s.body(circle(38.0, 58.0, 30.0), skin);
            s.body(circle(62.0, 58.0, 30.0), skin);
            s.detail(oval(50.0, 34.0, 10.0, 5.0, 0.0), (74, 140, 46));
            s.line_alpha(polyline(&[(36.0, 46.0), (30.0, 62.0)]), (220, 250, 190), 5.0, 0.7);
            s.body(thick(polyline(&[(50.0, 34.0), (53.0, 14.0)]), 5.0), WOOD);
            s.body(oval(66.0, 18.0, 14.0, 7.0, -25.0), (64, 150, 54));
        }
        Icon::Wheat => {
            let grain = (236, 196, 84);
            s.body(rrect(47.0, 30.0, 6.0, 64.0, 3.0), (170, 150, 70));
            s.body(oval(50.0, 21.0, 8.0, 14.0, 0.0), grain);
            for y in [40.0, 58.0, 76.0] {
                s.body(oval(39.0, y, 8.0, 14.0, -38.0), grain);
                s.body(oval(61.0, y, 8.0, 14.0, 38.0), grain);
            }
            s.line_alpha(polyline(&[(50.0, 12.0), (50.0, 28.0)]), (255, 245, 200), 2.5, 0.7);
        }
        Icon::Production => {
            s.body(rrect(44.0, 32.0, 12.0, 62.0, 4.0), WOOD);
            s.body(rrect(22.0, 10.0, 56.0, 26.0, 5.0), (222, 128, 48));
            s.detail(rrect(22.0, 30.0, 56.0, 6.0, 2.0), (160, 82, 30));
            s.line_alpha(polyline(&[(28.0, 16.0), (72.0, 16.0)]), (255, 220, 170), 3.0, 0.8);
            s.rotate(-40.0);
        }
        Icon::Gold => {
            s.body(circle(50.0, 50.0, 38.0), GOLD);
            s.line(circle(50.0, 50.0, 27.0), (186, 132, 26), 4.0);
            s.detail(poly(&[(50.0, 32.0), (60.0, 50.0), (50.0, 68.0), (40.0, 50.0)]), (196, 142, 32));
            s.line_alpha(arc(50.0, 50.0, 32.0, 200.0, 260.0), (255, 250, 220), 4.0, 0.9);
        }
        Icon::Culture => {
            let c = (184, 118, 226);
            s.body(oval(32.0, 74.0, 13.0, 10.0, -20.0), c);
            s.body(oval(70.0, 66.0, 13.0, 10.0, -20.0), c);
            s.body(rrect(38.0, 24.0, 8.0, 50.0, 2.0), c);
            s.body(rrect(76.0, 16.0, 8.0, 50.0, 2.0), c);
            s.body(poly(&[(38.0, 22.0), (84.0, 12.0), (84.0, 26.0), (38.0, 36.0)]), c);
        }
        Icon::Science => {
            s.body(poly(&[(40.0, 14.0), (60.0, 14.0), (60.0, 38.0), (86.0, 80.0), (80.0, 90.0), (20.0, 90.0), (14.0, 80.0), (40.0, 38.0)]), (205, 228, 245));
            s.body(rrect(33.0, 8.0, 34.0, 10.0, 4.0), (205, 228, 245));
            s.detail(poly(&[(30.0, 62.0), (70.0, 62.0), (82.0, 81.0), (78.0, 86.0), (22.0, 86.0), (18.0, 81.0)]), (70, 160, 235));
            s.detail_alpha(circle(42.0, 74.0, 4.0), (220, 240, 255), 0.9);
            s.detail_alpha(circle(56.0, 70.0, 3.0), (220, 240, 255), 0.9);
        }
        Icon::Population => {
            let skin = (238, 214, 176);
            s.body(circle(50.0, 30.0, 17.0), skin);
            let mut pb = PathBuilder::new();
            pb.move_to(14.0, 92.0);
            pb.quad_to(14.0, 52.0, 50.0, 52.0);
            pb.quad_to(86.0, 52.0, 86.0, 92.0);
            pb.close();
            s.body(pb.finish().unwrap(), (226, 200, 150));
        }
        Icon::Strength => {
            s.body(poly(&[(43.0, 16.0), (50.0, 2.0), (57.0, 16.0), (57.0, 62.0), (43.0, 62.0)]), STEEL);
            s.line_alpha(polyline(&[(50.0, 10.0), (50.0, 58.0)]), (120, 126, 140), 2.5, 0.8);
            s.body(rrect(26.0, 60.0, 48.0, 10.0, 4.0), BRASS);
            s.body(rrect(44.5, 69.0, 11.0, 19.0, 2.0), DARK_WOOD);
            s.body(circle(50.0, 92.0, 7.0), BRASS);
            s.rotate(40.0);
        }
        Icon::Defense => {
            s.body(shield_path(1.0), (96, 134, 196));
            s.line_alpha(rotated(shield_path(0.72), 0.0, 50.0, 50.0), (200, 220, 248), 3.5, 0.9);
            s.detail(rrect(46.0, 26.0, 8.0, 50.0, 2.0), (200, 220, 248));
            s.detail(rrect(30.0, 40.0, 40.0, 8.0, 2.0), (200, 220, 248));
        }
        Icon::Moves => {
            let leather = (176, 120, 68);
            s.body(poly(&[(30.0, 8.0), (62.0, 8.0), (62.0, 54.0), (84.0, 62.0), (93.0, 74.0), (93.0, 90.0), (25.0, 90.0), (28.0, 56.0)]), leather);
            s.detail(rrect(27.0, 8.0, 36.0, 10.0, 2.0), (130, 84, 44));
            s.detail(rrect(25.0, 83.0, 68.0, 7.0, 2.0), (84, 56, 32));
            s.line_alpha(polyline(&[(62.0, 54.0), (52.0, 62.0)]), (130, 84, 44), 3.0, 0.9);
        }
        Icon::Sight => {
            let mut pb = PathBuilder::new();
            pb.move_to(6.0, 50.0);
            pb.quad_to(50.0, 6.0, 94.0, 50.0);
            pb.quad_to(50.0, 94.0, 6.0, 50.0);
            pb.close();
            s.body(pb.finish().unwrap(), (244, 242, 236));
            s.detail(circle(50.0, 50.0, 17.0), (78, 146, 210));
            s.detail(circle(50.0, 50.0, 8.0), (20, 24, 32));
            s.detail(circle(56.0, 44.0, 4.0), (255, 255, 255));
        }
        Icon::Health => {
            let mut pb = PathBuilder::new();
            pb.move_to(50.0, 90.0);
            pb.cubic_to(18.0, 66.0, 6.0, 46.0, 10.0, 30.0);
            pb.cubic_to(14.0, 12.0, 40.0, 6.0, 50.0, 26.0);
            pb.cubic_to(60.0, 6.0, 86.0, 12.0, 90.0, 30.0);
            pb.cubic_to(94.0, 46.0, 82.0, 66.0, 50.0, 90.0);
            pb.close();
            s.body(pb.finish().unwrap(), (222, 64, 64));
            s.detail_alpha(oval(30.0, 30.0, 8.0, 5.0, -35.0), (255, 220, 220), 0.8);
        }
        Icon::Turns => {
            s.body(poly(&[(28.0, 16.0), (72.0, 16.0), (54.0, 50.0), (72.0, 84.0), (28.0, 84.0), (46.0, 50.0)]), (218, 230, 240));
            s.body(rrect(20.0, 6.0, 60.0, 12.0, 4.0), WOOD);
            s.body(rrect(20.0, 82.0, 60.0, 12.0, 4.0), WOOD);
            s.detail(poly(&[(36.0, 26.0), (64.0, 26.0), (50.0, 44.0)]), (232, 186, 92));
            s.detail(poly(&[(50.0, 60.0), (66.0, 80.0), (34.0, 80.0)]), (232, 186, 92));
        }
        Icon::Daytime => {
            for i in 0..12 {
                let ray = poly(&[(45.0, 24.0), (50.0, 6.0), (55.0, 24.0)]);
                s.body(rotated(ray, i as f32 * 30.0, 50.0, 50.0), (246, 196, 82));
            }
            s.body(circle(50.0, 50.0, 22.0), (252, 214, 104));
            s.detail(circle(46.0, 46.0, 13.0), (255, 232, 150));
        }
        Icon::Night => {
            // A crescent moon: an outer curve bulging left, back along an inner one.
            let mut moon = PathBuilder::new();
            moon.move_to(64.0, 10.0);
            moon.cubic_to(14.0, 12.0, 14.0, 88.0, 64.0, 90.0);
            moon.cubic_to(54.0, 72.0, 54.0, 28.0, 64.0, 10.0);
            moon.close();
            s.body(moon.finish().unwrap(), (222, 230, 246));
            s.detail(circle(32.0, 58.0, 4.5), (190, 200, 222));
            s.detail(circle(36.0, 36.0, 3.0), (190, 200, 222));
        }
        Icon::Settler => {
            let mut pb = PathBuilder::new();
            pb.move_to(16.0, 58.0);
            pb.line_to(16.0, 40.0);
            pb.quad_to(16.0, 14.0, 50.0, 14.0);
            pb.quad_to(84.0, 14.0, 84.0, 40.0);
            pb.line_to(84.0, 58.0);
            pb.close();
            s.body(pb.finish().unwrap(), CREAM);
            s.body(rrect(8.0, 52.0, 84.0, 16.0, 3.0), WOOD);
            s.body(circle(27.0, 76.0, 13.0), DARK_WOOD);
            s.body(circle(73.0, 76.0, 13.0), DARK_WOOD);
            s.detail(circle(27.0, 76.0, 4.0), BRASS);
            s.detail(circle(73.0, 76.0, 4.0), BRASS);
            s.line(polyline(&[(39.0, 18.0), (39.0, 52.0)]), (176, 160, 128), 3.0);
            s.line(polyline(&[(61.0, 18.0), (61.0, 52.0)]), (176, 160, 128), 3.0);
        }
        Icon::Worker => {
            s.body(rrect(45.0, 24.0, 10.0, 70.0, 4.0), WOOD);
            let mut pb = PathBuilder::new();
            pb.move_to(8.0, 36.0);
            pb.quad_to(50.0, 2.0, 92.0, 36.0);
            pb.line_to(88.0, 42.0);
            pb.quad_to(50.0, 20.0, 12.0, 42.0);
            pb.close();
            s.body(pb.finish().unwrap(), STEEL);
            s.detail(rrect(43.0, 18.0, 14.0, 14.0, 3.0), (110, 116, 128));
            s.rotate(25.0);
        }
        Icon::Scout => {
            s.body(rrect(8.0, 42.0, 30.0, 16.0, 3.0), (120, 80, 50));
            s.body(rrect(35.0, 39.0, 28.0, 22.0, 3.0), BRASS);
            s.body(rrect(60.0, 35.0, 32.0, 30.0, 4.0), BRASS);
            s.detail(rrect(82.0, 35.0, 6.0, 30.0, 1.0), (160, 120, 40));
            s.detail(rrect(52.0, 39.0, 5.0, 22.0, 1.0), (160, 120, 40));
            s.rotate(-30.0);
        }
        Icon::General => {
            // A commander's star over a red sash.
            s.body(rrect(14.0, 70.0, 72.0, 14.0, 5.0), (176, 52, 44));
            s.line_alpha(polyline(&[(18.0, 77.0), (82.0, 77.0)]), (230, 120, 100), 2.0, 0.7);
            let star: Vec<(f32, f32)> = (0..10)
                .map(|i| {
                    let r = if i % 2 == 0 { 40.0 } else { 16.5 };
                    let a = (-90.0 + 36.0 * i as f32).to_radians();
                    (50.0 + r * a.cos(), 42.0 + r * a.sin())
                })
                .collect();
            s.body(poly(&star), BRASS);
            s.detail(circle(50.0, 43.0, 7.0), (250, 226, 150));
        }
        Icon::Admiral => {
            // An anchor under a commander's star.
            s.body(rrect(46.0, 30.0, 8.0, 52.0, 3.0), STEEL);
            s.body(rrect(32.0, 36.0, 36.0, 7.0, 3.0), STEEL);
            s.line(polyline(&[(18.0, 64.0), (24.0, 78.0), (38.0, 86.0), (50.0, 88.0), (62.0, 86.0), (76.0, 78.0), (82.0, 64.0)]), STEEL, 7.0);
            s.body(poly(&[(12.0, 60.0), (24.0, 58.0), (18.0, 70.0)]), STEEL);
            s.body(poly(&[(88.0, 60.0), (76.0, 58.0), (82.0, 70.0)]), STEEL);
            s.line_alpha(polyline(&[(49.0, 34.0), (49.0, 78.0)]), (220, 228, 236), 2.0, 0.6);
            let star: Vec<(f32, f32)> = (0..10)
                .map(|i| {
                    let r = if i % 2 == 0 { 17.0 } else { 7.0 };
                    let a = (-90.0 + 36.0 * i as f32).to_radians();
                    (50.0 + r * a.cos(), 18.0 + r * a.sin())
                })
                .collect();
            s.body(poly(&star), BRASS);
            s.detail(circle(50.0, 18.0, 3.0), (250, 226, 150));
        }
        Icon::Rank => {
            // Two gold chevrons, one above the other.
            for y in [44.0, 74.0] {
                let chevron = poly(&[(12.0, y), (50.0, y - 26.0), (88.0, y), (88.0, y + 16.0), (50.0, y - 10.0), (12.0, y + 16.0)]);
                s.body(chevron, BRASS);
            }
            s.line_alpha(polyline(&[(18.0, 42.0), (50.0, 20.0), (82.0, 42.0)]), (250, 226, 150), 2.5, 0.8);
        }
        Icon::Army => {
            // A standard on its pole, with two spears crossed behind it.
            s.body(rotated(rrect(47.0, 10.0, 6.0, 84.0, 2.0), 32.0, 50.0, 52.0), WOOD);
            s.body(rotated(rrect(47.0, 10.0, 6.0, 84.0, 2.0), -32.0, 50.0, 52.0), WOOD);
            for a in [32.0, -32.0] {
                s.body(rotated(poly(&[(50.0, 2.0), (56.0, 14.0), (44.0, 14.0)]), a, 50.0, 52.0), STEEL);
            }
            s.body(rrect(46.0, 8.0, 8.0, 86.0, 3.0), DARK_WOOD);
            s.body(poly(&[(54.0, 12.0), (90.0, 12.0), (80.0, 26.0), (90.0, 40.0), (54.0, 40.0)]), (190, 56, 46));
            s.line_alpha(polyline(&[(58.0, 18.0), (82.0, 18.0)]), (240, 150, 120), 2.5, 0.6);
            s.body(circle(50.0, 8.0, 6.0), BRASS);
        }
        Icon::Warrior => {
            s.body(rrect(44.0, 8.0, 11.0, 86.0, 4.0), WOOD);
            let mut pb = PathBuilder::new();
            pb.move_to(52.0, 20.0);
            pb.line_to(70.0, 14.0);
            pb.quad_to(80.0, 8.0, 88.0, 6.0);
            pb.quad_to(98.0, 32.0, 88.0, 58.0);
            pb.quad_to(80.0, 54.0, 70.0, 48.0);
            pb.line_to(52.0, 42.0);
            pb.close();
            s.body(pb.finish().unwrap(), STEEL);
            s.line_alpha(arc(52.0, 32.0, 38.0, -38.0, 36.0), (244, 248, 255), 3.0, 0.85);
            s.detail(rrect(44.0, 18.0, 11.0, 26.0, 2.0), (110, 116, 128));
            s.rotate(-20.0);
        }
        Icon::Archer => {
            let mut pb = PathBuilder::new();
            pb.move_to(30.0, 6.0);
            pb.quad_to(86.0, 50.0, 30.0, 94.0);
            pb.quad_to(70.0, 50.0, 30.0, 6.0);
            pb.close();
            s.body(pb.finish().unwrap(), WOOD);
            s.body(rrect(12.0, 47.5, 70.0, 5.0, 2.0), (200, 170, 120));
            s.body(poly(&[(76.0, 40.0), (95.0, 50.0), (76.0, 60.0)]), STEEL);
            s.body(poly(&[(6.0, 40.0), (20.0, 47.0), (20.0, 53.0), (6.0, 60.0), (12.0, 50.0)]), (204, 72, 56));
            s.line(polyline(&[(30.0, 8.0), (30.0, 92.0)]), (238, 230, 210), 2.0);
        }
        Icon::Spearman => {
            s.body(rrect(46.0, 6.0, 8.0, 88.0, 3.0), WOOD);
            s.body(poly(&[(50.0, 0.0), (60.0, 18.0), (50.0, 26.0), (40.0, 18.0)]), STEEL);
            s.body(circle(30.0, 62.0, 22.0), (186, 72, 56));
            s.line(circle(30.0, 62.0, 15.0), (240, 210, 150), 3.0);
            s.detail(circle(30.0, 62.0, 5.0), BRASS);
            s.rotate(20.0);
        }
        Icon::Catapult => {
            // Throwing arm with a loaded stone, on a wheeled frame.
            s.body(thick(polyline(&[(70.0, 60.0), (24.0, 20.0)]), 8.0), WOOD);
            s.body(circle(20.0, 17.0, 10.0), DARK_WOOD);
            s.body(circle(19.0, 12.0, 7.0), STONE);
            s.body(poly(&[(38.0, 70.0), (52.0, 36.0), (66.0, 70.0), (58.0, 70.0), (52.0, 52.0), (46.0, 70.0)]), DARK_WOOD);
            s.detail(circle(52.0, 44.0, 4.0), BRASS);
            s.body(rrect(8.0, 64.0, 84.0, 11.0, 3.0), WOOD);
            for x in [24.0, 76.0] {
                s.body(circle(x, 80.0, 13.0), DARK_WOOD);
                s.line(circle(x, 80.0, 8.0), (176, 128, 80), 2.5);
                s.detail(circle(x, 80.0, 3.5), STEEL);
            }
        }
        Icon::Galley => {
            // Side view: a square sail, a long hull with a bronze ram and a row of oars.
            s.body(rrect(48.0, 8.0, 4.5, 58.0, 2.0), DARK_WOOD);
            s.body(poly(&[(24.0, 14.0), (76.0, 14.0), (80.0, 50.0), (20.0, 50.0)]), CREAM);
            s.detail(poly(&[(22.5, 30.0), (77.5, 30.0), (78.5, 38.0), (21.5, 38.0)]), (176, 64, 52));
            for x in [22.0, 34.0, 46.0, 58.0, 70.0] {
                s.body(thick(polyline(&[(x + 8.0, 66.0), (x - 2.0, 90.0)]), 3.5), WOOD);
            }
            s.body(galley_hull(4.0, 96.0, 56.0, 72.0), WOOD);
            s.detail(poly(&[(14.0, 60.0), (88.0, 60.0), (87.0, 64.0), (15.0, 64.0)]), DARK_WOOD);
            s.body(poly(&[(92.0, 64.0), (99.0, 69.0), (92.0, 72.0)]), BRASS);
            s.body(thick(arc(10.0, 52.0, 8.0, 90.0, 300.0), 4.0), WOOD);
        }
        Icon::Quadrireme => {
            // A heavier warship with two banks of oars and a catapult on deck.
            s.body(rrect(30.0, 10.0, 4.0, 46.0, 2.0), DARK_WOOD);
            s.body(poly(&[(14.0, 16.0), (50.0, 16.0), (53.0, 42.0), (11.0, 42.0)]), CREAM);
            s.body(thick(polyline(&[(74.0, 48.0), (54.0, 22.0)]), 5.0), WOOD);
            s.body(circle(52.0, 20.0, 6.0), STONE);
            s.body(poly(&[(64.0, 52.0), (72.0, 36.0), (80.0, 52.0)]), DARK_WOOD);
            for (row, dy) in [(0.0, 0.0), (1.0, 6.0)] {
                for x in [20.0, 32.0, 44.0, 56.0, 68.0] {
                    let x = x + row * 5.0;
                    s.body(thick(polyline(&[(x + 7.0, 66.0 + dy), (x - 3.0, 88.0 + dy)]), 3.0), if row == 0.0 { WOOD } else { DARK_WOOD });
                }
            }
            s.body(galley_hull(4.0, 96.0, 52.0, 74.0), WOOD);
            for y in [58.0, 65.0] {
                s.detail(poly(&[(12.0, y), (90.0, y), (89.0, y + 3.0), (13.0, y + 3.0)]), DARK_WOOD);
            }
            s.body(poly(&[(92.0, 66.0), (99.0, 71.0), (92.0, 74.0)]), BRASS);
            s.body(thick(arc(10.0, 48.0, 8.0, 90.0, 300.0), 4.0), WOOD);
        }
        Icon::Library => {
            let books = [((176, 64, 52), 14.0, 30.0), ((66, 108, 176), 30.0, 22.0), ((74, 140, 84), 46.0, 34.0), ((196, 150, 60), 62.0, 26.0)];
            for (c, x, top) in books {
                s.body(rrect(x, top, 15.0, 90.0 - top, 2.0), c);
                s.line_alpha(polyline(&[(x + 3.0, top + 10.0), (x + 12.0, top + 10.0)]), (250, 240, 210), 2.0, 0.8);
            }
            s.body(rotated(rrect(76.0, 34.0, 15.0, 56.0, 2.0), 14.0, 83.0, 90.0), (120, 70, 140));
            s.body(rrect(8.0, 88.0, 86.0, 6.0, 2.0), DARK_WOOD);
        }
        Icon::Amphitheater => {
            s.body(rrect(8.0, 40.0, 84.0, 52.0, 3.0), STONE);
            for x in [14.0, 38.0, 62.0] {
                let mut pb = PathBuilder::new();
                pb.move_to(x, 92.0);
                pb.line_to(x, 64.0);
                pb.quad_to(x + 12.0, 48.0, x + 24.0, 64.0);
                pb.line_to(x + 24.0, 92.0);
                pb.close();
                s.detail(pb.finish().unwrap(), (88, 74, 60));
            }
            s.line_alpha(polyline(&[(10.0, 50.0), (90.0, 50.0)]), (130, 122, 108), 2.5, 0.9);
            s.body(poly(&[(8.0, 40.0), (50.0, 14.0), (92.0, 40.0)]), (206, 196, 176));
        }
        Icon::Temple => {
            s.body(poly(&[(6.0, 34.0), (50.0, 8.0), (94.0, 34.0)]), (226, 214, 188));
            s.body(rrect(10.0, 32.0, 80.0, 8.0, 1.0), (206, 196, 176));
            for x in [16.0, 34.0, 52.0, 70.0] {
                s.body(rrect(x, 42.0, 12.0, 40.0, 2.0), (226, 214, 188));
            }
            s.body(rrect(6.0, 82.0, 88.0, 10.0, 2.0), (190, 180, 160));
            s.detail(circle(50.0, 24.0, 4.0), BRASS);
        }
        Icon::Monument => {
            s.body(poly(&[(40.0, 24.0), (50.0, 6.0), (60.0, 24.0), (64.0, 80.0), (36.0, 80.0)]), STONE);
            s.body(rrect(22.0, 78.0, 56.0, 14.0, 3.0), (160, 150, 134));
            s.line_alpha(polyline(&[(50.0, 14.0), (50.0, 74.0)]), (255, 250, 236), 2.5, 0.6);
            s.detail(poly(&[(46.0, 34.0), (54.0, 34.0), (50.0, 44.0)]), (214, 172, 78));
        }
        Icon::Granary => {
            s.body(rrect(22.0, 46.0, 56.0, 46.0, 4.0), (204, 162, 104));
            s.body(poly(&[(10.0, 52.0), (50.0, 10.0), (90.0, 52.0)]), (222, 186, 92));
            s.detail(rrect(41.0, 62.0, 18.0, 30.0, 8.0), (96, 62, 36));
            s.line_alpha(polyline(&[(30.0, 40.0), (50.0, 20.0), (70.0, 40.0)]), (255, 236, 170), 2.5, 0.8);
        }
        Icon::Workshop => {
            let mut pts = Vec::new();
            for k in 0..8 {
                let base = k as f32 * 45.0;
                for (r, a) in [(33.0, base - 15.0), (44.0, base - 9.0), (44.0, base + 9.0), (33.0, base + 15.0)] {
                    let a = f32::to_radians(a);
                    pts.push((50.0 + r * a.cos(), 50.0 + r * a.sin()));
                }
            }
            s.body_eo(combine(&[poly(&pts), circle(50.0, 50.0, 13.0)]), (176, 182, 196));
            s.line_alpha(circle(50.0, 50.0, 22.0), (120, 126, 140), 3.0, 0.8);
        }
        Icon::Market => {
            let tan = (196, 150, 92);
            let mut pb = PathBuilder::new();
            pb.move_to(32.0, 34.0);
            pb.quad_to(8.0, 60.0, 20.0, 84.0);
            pb.quad_to(50.0, 98.0, 80.0, 84.0);
            pb.quad_to(92.0, 60.0, 68.0, 34.0);
            pb.close();
            s.body(pb.finish().unwrap(), tan);
            s.body(poly(&[(34.0, 12.0), (66.0, 12.0), (58.0, 34.0), (42.0, 34.0)]), tan);
            s.body(rrect(34.0, 30.0, 32.0, 8.0, 3.0), (130, 88, 48));
            s.detail(circle(50.0, 66.0, 13.0), GOLD);
            s.line(circle(50.0, 66.0, 8.5), (186, 132, 26), 2.5);
        }
        Icon::Walls => {
            s.body(rrect(10.0, 38.0, 80.0, 54.0, 2.0), STONE);
            for x in [10.0, 31.0, 52.0, 73.0] {
                s.body(rrect(x, 24.0, 17.0, 18.0, 2.0), STONE);
            }
            for y in [54.0, 70.0] {
                s.line_alpha(polyline(&[(12.0, y), (88.0, y)]), (130, 122, 108), 2.5, 0.9);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(40.0, 92.0);
            pb.line_to(40.0, 72.0);
            pb.quad_to(50.0, 58.0, 60.0, 72.0);
            pb.line_to(60.0, 92.0);
            pb.close();
            s.detail(pb.finish().unwrap(), (70, 56, 44));
        }
        Icon::Explore => {
            s.body(circle(50.0, 50.0, 41.0), (232, 222, 196));
            s.line(circle(50.0, 50.0, 34.0), (150, 118, 76), 3.0);
            s.detail(poly(&[(50.0, 14.0), (60.0, 50.0), (40.0, 50.0)]), (214, 64, 52));
            s.detail(poly(&[(50.0, 86.0), (60.0, 50.0), (40.0, 50.0)]), (84, 88, 100));
            s.detail(circle(50.0, 50.0, 5.0), BRASS);
            s.rotate(30.0);
        }
        Icon::Sleep => {
            let c = (176, 204, 246);
            s.body(letter_z(16.0, 38.0, 1.05), c);
            s.body(letter_z(64.0, 12.0, 0.6), c);
        }
        Icon::Skip => {
            let c = (230, 210, 150);
            s.body(poly(&[(12.0, 22.0), (50.0, 50.0), (12.0, 78.0)]), c);
            s.body(poly(&[(48.0, 22.0), (86.0, 50.0), (48.0, 78.0)]), c);
        }
        Icon::FoundCity => {
            s.body(rrect(28.0, 42.0, 44.0, 50.0, 2.0), STONE);
            for x in [24.0, 43.0, 62.0] {
                s.body(rrect(x, 32.0, 14.0, 14.0, 2.0), STONE);
            }
            s.body(rrect(48.0, 6.0, 4.0, 28.0, 1.0), DARK_WOOD);
            s.body(poly(&[(52.0, 7.0), (76.0, 13.0), (52.0, 20.0)]), (214, 64, 52));
            let mut pb = PathBuilder::new();
            pb.move_to(42.0, 92.0);
            pb.line_to(42.0, 74.0);
            pb.quad_to(50.0, 62.0, 58.0, 74.0);
            pb.line_to(58.0, 92.0);
            pb.close();
            s.detail(pb.finish().unwrap(), (70, 56, 44));
            s.line_alpha(polyline(&[(30.0, 58.0), (70.0, 58.0)]), (130, 122, 108), 2.5, 0.9);
        }
        Icon::EndTurn => {
            s.body(poly(&[(12.0, 37.0), (50.0, 37.0), (50.0, 15.0), (90.0, 50.0), (50.0, 85.0), (50.0, 63.0), (12.0, 63.0)]), (242, 208, 112));
            s.line_alpha(polyline(&[(16.0, 42.0), (54.0, 42.0), (54.0, 26.0)]), (255, 246, 210), 3.0, 0.8);
        }
        Icon::Balanced => {
            s.body(rrect(46.0, 18.0, 8.0, 64.0, 2.0), BRASS);
            s.body(poly(&[(26.0, 92.0), (74.0, 92.0), (62.0, 80.0), (38.0, 80.0)]), BRASS);
            s.body(rrect(12.0, 18.0, 76.0, 8.0, 3.0), BRASS);
            for x in [22.0, 78.0] {
                let mut pb = PathBuilder::new();
                pb.move_to(x - 15.0, 56.0);
                pb.quad_to(x, 76.0, x + 15.0, 56.0);
                pb.close();
                s.body(pb.finish().unwrap(), BRASS);
                s.line(polyline(&[(x - 13.0, 56.0), (x, 24.0), (x + 13.0, 56.0)]), (120, 92, 40), 2.0);
            }
            s.detail(circle(50.0, 14.0, 6.0), BRASS);
        }
        Icon::Auto => {
            let c = (120, 200, 196);
            s.body(thick(arc(50.0, 50.0, 30.0, 200.0, 335.0), 12.0), c);
            s.body(poly(&[(68.0, 30.0), (92.0, 36.0), (78.0, 56.0)]), c);
            s.body(thick(arc(50.0, 50.0, 30.0, 20.0, 155.0), 12.0), c);
            s.body(poly(&[(32.0, 70.0), (8.0, 64.0), (22.0, 44.0)]), c);
        }
        Icon::Repeat => {
            let c = (232, 198, 112);
            s.body(thick(polyline(&[(18.0, 60.0), (18.0, 30.0), (70.0, 30.0)]), 11.0), c);
            s.body(poly(&[(66.0, 14.0), (90.0, 30.0), (66.0, 46.0)]), c);
            s.body(thick(polyline(&[(82.0, 40.0), (82.0, 70.0), (30.0, 70.0)]), 11.0), c);
            s.body(poly(&[(34.0, 54.0), (10.0, 70.0), (34.0, 86.0)]), c);
        }
        Icon::MoveToCity => {
            // An arrow over into a town's tower.
            s.body(rrect(46.0, 46.0, 42.0, 46.0, 2.0), STONE);
            for x in [42.0, 58.0, 74.0] {
                s.body(rrect(x, 36.0, 14.0, 13.0, 2.0), STONE);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(59.0, 92.0);
            pb.line_to(59.0, 76.0);
            pb.quad_to(67.0, 64.0, 75.0, 76.0);
            pb.line_to(75.0, 92.0);
            pb.close();
            s.detail(pb.finish().unwrap(), (70, 56, 44));
            let c = (232, 198, 112);
            s.body(thick(polyline(&[(16.0, 90.0), (16.0, 20.0), (50.0, 20.0)]), 11.0), c);
            s.body(poly(&[(46.0, 4.0), (72.0, 20.0), (46.0, 36.0)]), c);
        }
        Icon::Close => {
            let c = (222, 96, 76);
            s.body(rotated(rrect(43.0, 10.0, 14.0, 80.0, 5.0), 45.0, 50.0, 50.0), c);
            s.body(rotated(rrect(43.0, 10.0, 14.0, 80.0, 5.0), -45.0, 50.0, 50.0), c);
        }
        Icon::Menu => {
            for y in [20.0, 44.0, 68.0] {
                s.body(rrect(14.0, y, 72.0, 13.0, 5.0), CREAM);
            }
        }
        Icon::Warning => {
            let mut pb = PathBuilder::new();
            pb.move_to(50.0, 8.0);
            pb.line_to(94.0, 88.0);
            pb.line_to(6.0, 88.0);
            pb.close();
            s.body(pb.finish().unwrap(), (246, 196, 64));
            s.detail(rrect(45.0, 34.0, 10.0, 32.0, 4.0), (48, 36, 24));
            s.detail(circle(50.0, 76.0, 6.0), (48, 36, 24));
        }
        Icon::Check => {
            s.body(thick(polyline(&[(16.0, 52.0), (40.0, 76.0), (84.0, 24.0)]), 15.0), (120, 204, 104));
        }
        Icon::Log => {
            // A parchment scroll with lines of writing.
            let paper = (232, 214, 172);
            s.body(rrect(20.0, 16.0, 60.0, 70.0, 3.0), paper);
            s.body(rrect(12.0, 8.0, 76.0, 14.0, 7.0), (204, 180, 132));
            s.body(rrect(12.0, 80.0, 76.0, 14.0, 7.0), (204, 180, 132));
            for (y, w) in [(34.0, 44.0), (46.0, 36.0), (58.0, 42.0), (70.0, 28.0)] {
                s.line_alpha(polyline(&[(28.0, y), (28.0 + w, y)]), (96, 72, 48), 4.0, 0.9);
            }
        }
        Icon::Map => {
            // A folded map with a route.
            let panels = [(8.0, (214, 196, 150)), (36.0, (196, 178, 132)), (64.0, (214, 196, 150))];
            for (x, c) in panels {
                s.body(poly(&[(x, 18.0 + (x - 8.0) / 28.0 % 2.0 * 6.0), (x + 28.0, 12.0 + (x - 8.0) / 28.0 % 2.0 * 6.0), (x + 28.0, 84.0), (x, 90.0)]), c);
            }
            s.detail(poly(&[(14.0, 40.0), (30.0, 34.0), (30.0, 60.0), (14.0, 66.0)]), (120, 160, 96));
            s.line_alpha(polyline(&[(18.0, 74.0), (40.0, 58.0), (56.0, 66.0), (80.0, 36.0)]), (176, 60, 48), 4.0, 0.95);
            s.detail(circle(80.0, 34.0, 6.0), (176, 60, 48));
        }
        Icon::River => {
            let c = (96, 170, 232);
            for y in [28.0, 50.0, 72.0] {
                let mut pb = PathBuilder::new();
                pb.move_to(12.0, y);
                pb.quad_to(24.0, y - 12.0, 37.0, y);
                pb.quad_to(50.0, y + 12.0, 63.0, y);
                pb.quad_to(76.0, y - 12.0, 88.0, y);
                s.body(thick(pb.finish().unwrap(), 9.0), c);
            }
        }
        Icon::Banner => {
            s.body(shield_path(1.0), CREAM);
            s.line_alpha(shield_path(0.86), (255, 255, 255), 2.5, 0.35);
        }
        Icon::Windmill => {
            s.body(poly(&[(43.0, 46.0), (57.0, 46.0), (64.0, 90.0), (36.0, 90.0)]), CREAM);
            s.detail(rrect(46.0, 76.0, 8.0, 14.0, 3.0), (110, 90, 70));
            for a in [45.0, 135.0, 225.0, 315.0] {
                let blade = combine(&[rrect(47.0, 6.0, 6.0, 34.0, 2.0), rrect(53.0, 8.0, 9.0, 28.0, 1.5)]);
                s.body(rotated(blade, a, 50.0, 42.0), CREAM);
            }
            for a in [45.0, 135.0, 225.0, 315.0] {
                for k in [14.0, 22.0, 30.0] {
                    s.line_alpha(rotated(polyline(&[(53.0, 42.0 - k), (62.0, 42.0 - k)]), a, 50.0, 42.0), (120, 100, 80), 1.5, 0.8);
                }
            }
            s.body(circle(50.0, 42.0, 6.0), CREAM);
        }
        Icon::Laurel => {
            for side in [-1.0f32, 1.0] {
                let stem = arc(50.0, 50.0, 34.0, if side < 0.0 { 105.0 } else { 75.0 }, if side < 0.0 { 215.0 } else { -35.0 });
                s.body(thick(stem, 4.0), CREAM);
                for i in 0..7 {
                    let t = i as f32 / 6.0;
                    let a: f32 = if side < 0.0 { 110.0 + t * 100.0 } else { 70.0 - t * 100.0 };
                    let (x, y) = (50.0 + 34.0 * a.to_radians().cos(), 50.0 + 34.0 * a.to_radians().sin());
                    // Leaves point along the branch, outwards and inwards in turn.
                    let out = if i % 2 == 0 { 9.0 } else { -7.0 };
                    let (lx, ly) = (x + out * a.to_radians().cos(), y + out * a.to_radians().sin());
                    let tilt = a + 90.0 * side + if i % 2 == 0 { 25.0 } else { -25.0 } * side;
                    s.body(oval(lx, ly, 9.0, 4.5, tilt), CREAM);
                }
            }
            s.body(thick(polyline(&[(42.0, 88.0), (58.0, 82.0)]), 4.0), CREAM);
            s.body(thick(polyline(&[(42.0, 82.0), (58.0, 88.0)]), 4.0), CREAM);
        }
        Icon::Ankh => {
            let ring = combine(&[oval(50.0, 28.0, 15.0, 19.0, 0.0), oval(50.0, 28.0, 7.0, 11.0, 0.0)]);
            s.body_eo(ring, CREAM);
            s.body(rrect(22.0, 45.0, 56.0, 11.0, 3.0), CREAM);
            s.body(poly(&[(45.0, 50.0), (55.0, 50.0), (60.0, 92.0), (40.0, 92.0)]), CREAM);
        }
        Icon::Bow => {
            s.body(thick(arc(10.0, 50.0, 46.0, -62.0, 62.0), 6.5), CREAM);
            s.line(polyline(&[(31.6, 9.4), (31.6, 90.6)]), CREAM, 2.0);
            s.body(thick(polyline(&[(18.0, 50.0), (78.0, 50.0)]), 4.0), CREAM);
            s.body(poly(&[(92.0, 50.0), (76.0, 41.0), (80.0, 50.0), (76.0, 59.0)]), CREAM);
            s.body(poly(&[(18.0, 50.0), (10.0, 41.0), (6.0, 41.0), (13.0, 50.0), (6.0, 59.0), (10.0, 59.0)]), CREAM);
            s.rotate(-35.0);
        }
        Icon::Sun => {
            for i in 0..16 {
                let long = i % 2 == 0;
                let ray = poly(&[(46.0, 30.0), (50.0, if long { 4.0 } else { 14.0 }), (54.0, 30.0)]);
                s.body(rotated(ray, i as f32 * 22.5, 50.0, 50.0), CREAM);
            }
            s.body(circle(50.0, 50.0, 22.0), CREAM);
            s.line_alpha(circle(50.0, 50.0, 15.0), (150, 120, 70), 2.5, 0.7);
        }
        Icon::Longship => {
            let mut hull = PathBuilder::new();
            hull.move_to(6.0, 50.0);
            hull.quad_to(14.0, 74.0, 34.0, 76.0);
            hull.line_to(66.0, 76.0);
            hull.quad_to(86.0, 74.0, 94.0, 50.0);
            hull.quad_to(86.0, 62.0, 66.0, 62.0);
            hull.line_to(34.0, 62.0);
            hull.quad_to(14.0, 62.0, 6.0, 50.0);
            hull.close();
            s.body(rrect(47.5, 12.0, 5.0, 52.0, 2.0), CREAM);
            s.body(poly(&[(26.0, 16.0), (74.0, 16.0), (78.0, 50.0), (22.0, 50.0)]), CREAM);
            for x in [36.0, 50.0, 64.0] {
                s.detail(poly(&[(x - 4.0, 16.5), (x + 4.0, 16.5), (x + 5.5, 49.5), (x - 5.5, 49.5)]), (150, 120, 90));
            }
            s.body(hull.finish().unwrap(), CREAM);
            s.body(thick(arc(90.0, 42.0, 7.0, 90.0, -160.0), 4.0), CREAM);
            s.body(thick(arc(10.0, 42.0, 7.0, 90.0, 340.0), 4.0), CREAM);
            for x in [30.0, 42.0, 54.0, 66.0] {
                s.detail(circle(x + 2.0, 66.0, 3.5), (150, 120, 90));
            }
        }
        Icon::Taegeuk => {
            s.body(circle(50.0, 50.0, 38.0), CREAM);
            let mut pb = PathBuilder::new();
            pb.move_to(88.0, 50.0);
            for (cx, r, from, to) in [(50.0, 38.0, 0.0f32, 180.0f32), (31.0, 19.0, 180.0, 360.0), (69.0, 19.0, 180.0, 0.0)] {
                for i in 1..=24 {
                    let a = (from + (to - from) * i as f32 / 24.0).to_radians();
                    pb.line_to(cx + r * a.cos(), 50.0 + r * a.sin());
                }
            }
            pb.close();
            s.detail(pb.finish().unwrap(), (120, 110, 100));
        }
        Icon::Sunstone => {
            s.body(circle(50.0, 50.0, 40.0), CREAM);
            s.line_alpha(circle(50.0, 50.0, 33.0), (120, 100, 80), 2.5, 0.9);
            for i in 0..8 {
                let ray = poly(&[(45.0, 33.0), (50.0, 18.0), (55.0, 33.0)]);
                s.detail(rotated(ray, i as f32 * 45.0 + 22.5, 50.0, 50.0), (120, 100, 80));
            }
            s.line_alpha(circle(50.0, 50.0, 17.0), (120, 100, 80), 2.5, 0.9);
            for i in 0..4 {
                let square = rrect(46.0, 26.0, 8.0, 8.0, 1.0);
                s.detail(rotated(square, i as f32 * 90.0 + 45.0, 50.0, 50.0), (120, 100, 80));
            }
            s.detail(circle(50.0, 50.0, 7.0), (120, 100, 80));
        }
        Icon::WingedDisc => {
            for side in [-1.0f32, 1.0] {
                for (i, len) in [40.0f32, 33.0, 25.0].into_iter().enumerate() {
                    let y = 38.0 + i as f32 * 9.0;
                    let x0 = 50.0 + side * 10.0;
                    let x1 = 50.0 + side * (10.0 + len);
                    let feather = poly(&[(x0, y - 4.0), (x1, y - 7.0 + i as f32), (x1 - side * 4.0, y + 3.0), (x0, y + 4.0)]);
                    s.body(feather, CREAM);
                }
            }
            for (dx, len) in [(-7.0f32, 22.0f32), (0.0, 28.0), (7.0, 22.0)] {
                s.body(poly(&[(50.0 + dx * 0.4 - 3.5, 60.0), (50.0 + dx * 0.4 + 3.5, 60.0), (50.0 + dx * 1.6 + 4.0, 60.0 + len), (50.0 + dx * 1.6 - 4.0, 60.0 + len)]), CREAM);
            }
            let disc = combine(&[circle(50.0, 44.0, 13.0), circle(50.0, 44.0, 6.5)]);
            s.body_eo(disc, CREAM);
        }
        Icon::Chakana => {
            let cross = poly(&[
                (40.0, 10.0), (60.0, 10.0), (60.0, 24.0), (76.0, 24.0), (76.0, 40.0), (90.0, 40.0),
                (90.0, 60.0), (76.0, 60.0), (76.0, 76.0), (60.0, 76.0), (60.0, 90.0), (40.0, 90.0),
                (40.0, 76.0), (24.0, 76.0), (24.0, 60.0), (10.0, 60.0), (10.0, 40.0), (24.0, 40.0),
                (24.0, 24.0), (40.0, 24.0),
            ]);
            s.body_eo(combine(&[cross, circle(50.0, 50.0, 10.0)]), CREAM);
            s.line_alpha(polyline(&[(40.0, 18.0), (60.0, 18.0)]), (150, 120, 90), 2.0, 0.7);
            s.line_alpha(polyline(&[(40.0, 82.0), (60.0, 82.0)]), (150, 120, 90), 2.0, 0.7);
        }
        Icon::Horseman => {
            let coat = (150, 96, 56);
            // A horse's head and neck in profile, with a mane and bridle.
            let mut pb = PathBuilder::new();
            pb.move_to(30.0, 92.0);
            pb.quad_to(26.0, 58.0, 42.0, 36.0);
            pb.line_to(50.0, 14.0);
            pb.line_to(58.0, 24.0);
            pb.quad_to(76.0, 30.0, 88.0, 52.0);
            pb.quad_to(90.0, 62.0, 80.0, 64.0);
            pb.quad_to(70.0, 60.0, 62.0, 56.0);
            pb.quad_to(60.0, 74.0, 70.0, 92.0);
            pb.close();
            s.body(pb.finish().unwrap(), coat);
            s.detail(poly(&[(42.0, 36.0), (50.0, 14.0), (46.0, 40.0), (34.0, 62.0), (30.0, 88.0), (28.0, 60.0)]), (70, 46, 30));
            s.detail(circle(64.0, 38.0, 3.5), (30, 22, 16));
            s.line_alpha(polyline(&[(60.0, 54.0), (74.0, 46.0), (84.0, 56.0)]), (220, 200, 160), 2.5, 0.9);
        }
        Icon::Swordsman => {
            s.body(poly(&[(46.0, 22.0), (50.0, 4.0), (54.0, 22.0), (54.0, 64.0), (46.0, 64.0)]), STEEL);
            s.line_alpha(polyline(&[(50.0, 12.0), (50.0, 60.0)]), (120, 126, 140), 2.0, 0.8);
            s.body(rrect(30.0, 62.0, 40.0, 9.0, 3.0), BRASS);
            s.body(rrect(45.5, 70.0, 9.0, 16.0, 2.0), DARK_WOOD);
            s.body(circle(50.0, 90.0, 6.0), BRASS);
            s.rotate(-35.0);
            s.body(rrect(52.0, 40.0, 34.0, 46.0, 6.0), (186, 72, 56));
            s.detail(rrect(66.0, 44.0, 6.0, 38.0, 2.0), BRASS);
        }
        Icon::Farm => {
            // Fields in rows under a rising sun.
            s.body(poly(&[(4.0, 60.0), (96.0, 60.0), (96.0, 92.0), (4.0, 92.0)]), (198, 168, 76));
            for (i, x) in [10.0f32, 26.0, 42.0, 58.0, 74.0, 90.0].iter().enumerate() {
                let c = if i % 2 == 0 { (238, 206, 98) } else { (150, 170, 70) };
                s.detail(poly(&[(x - 6.0, 62.0), (x + 2.0, 62.0), (x + 6.0, 90.0), (x - 10.0, 90.0)]), c);
            }
            s.body(arc(50.0, 56.0, 24.0, 180.0, 360.0), (244, 176, 62));
            s.body(poly(&[(26.0, 56.0), (74.0, 56.0), (74.0, 58.0), (26.0, 58.0)]), (244, 176, 62));
            let mut pb = PathBuilder::new();
            pb.move_to(26.0, 56.0);
            pb.quad_to(26.0, 32.0, 50.0, 32.0);
            pb.quad_to(74.0, 32.0, 74.0, 56.0);
            pb.close();
            s.body(pb.finish().unwrap(), (246, 186, 70));
        }
        Icon::Mine => {
            s.body(poly(&[(4.0, 92.0), (36.0, 22.0), (52.0, 40.0), (66.0, 26.0), (96.0, 92.0)]), (150, 132, 116));
            s.detail(poly(&[(36.0, 92.0), (36.0, 64.0), (64.0, 64.0), (64.0, 92.0)]), (34, 28, 24));
            s.body(rrect(30.0, 58.0, 8.0, 34.0, 1.0), WOOD);
            s.body(rrect(62.0, 58.0, 8.0, 34.0, 1.0), WOOD);
            s.body(rrect(26.0, 54.0, 48.0, 9.0, 2.0), WOOD);
            s.detail(circle(46.0, 84.0, 4.0), (200, 160, 80));
        }
        Icon::Pasture => {
            for x in [12.0f32, 38.0, 64.0, 88.0] {
                s.body(rrect(x - 4.0, 40.0, 8.0, 52.0, 2.0), WOOD);
            }
            s.body(rrect(6.0, 50.0, 88.0, 8.0, 2.0), (178, 126, 76));
            s.body(rrect(6.0, 70.0, 88.0, 8.0, 2.0), (178, 126, 76));
        }
        Icon::Camp => {
            s.body(poly(&[(12.0, 88.0), (44.0, 18.0), (76.0, 88.0)]), (196, 160, 112));
            s.detail(poly(&[(36.0, 88.0), (44.0, 56.0), (52.0, 88.0)]), (70, 48, 30));
            s.line(polyline(&[(40.0, 18.0), (34.0, 8.0)]), DARK_WOOD, 4.0);
            s.line(polyline(&[(48.0, 18.0), (54.0, 8.0)]), DARK_WOOD, 4.0);
            s.body(poly(&[(70.0, 90.0), (82.0, 66.0), (94.0, 90.0)]), (236, 128, 40));
            s.detail(poly(&[(76.0, 90.0), (82.0, 76.0), (88.0, 90.0)]), (252, 212, 90));
        }
        Icon::Plantation => {
            let leaf = (84, 150, 64);
            for (x, y) in [(22.0f32, 42.0f32), (50.0, 36.0), (78.0, 42.0)] {
                s.body(rrect(x - 3.0, y + 10.0, 6.0, 40.0, 2.0), DARK_WOOD);
                s.body(circle(x, y, 16.0), leaf);
                s.detail(circle(x - 5.0, y + 2.0, 3.5), (200, 60, 70));
                s.detail(circle(x + 5.0, y - 3.0, 3.5), (200, 60, 70));
            }
        }
        Icon::Quarry => {
            s.body(rrect(8.0, 60.0, 40.0, 30.0, 3.0), STONE);
            s.body(rrect(52.0, 60.0, 40.0, 30.0, 3.0), (176, 168, 152));
            s.body(rrect(30.0, 28.0, 40.0, 30.0, 3.0), (204, 196, 180));
            s.line_alpha(polyline(&[(14.0, 66.0), (40.0, 66.0)]), (250, 246, 236), 2.5, 0.7);
            s.line_alpha(polyline(&[(36.0, 34.0), (62.0, 34.0)]), (250, 246, 236), 2.5, 0.7);
        }
        Icon::Road => {
            let mut pb = PathBuilder::new();
            pb.move_to(36.0, 6.0);
            pb.line_to(64.0, 6.0);
            pb.line_to(94.0, 94.0);
            pb.line_to(6.0, 94.0);
            pb.close();
            s.body(pb.finish().unwrap(), (176, 146, 106));
            for (y0, y1) in [(12.0f32, 24.0f32), (36.0, 52.0), (64.0, 86.0)] {
                let w = |y: f32| 1.0 + y / 40.0;
                s.detail(poly(&[(50.0 - w(y0), y0), (50.0 + w(y0), y0), (50.0 + w(y1), y1), (50.0 - w(y1), y1)]), (238, 222, 186));
            }
        }
        Icon::Railroad => {
            // A track running away into the distance: sleepers and two rails.
            for k in 0..6 {
                let y = 14.0 + k as f32 * 15.0;
                let w = 14.0 + y * 0.36;
                s.body(rrect(50.0 - w, y, w * 2.0, 6.0 + k as f32 * 0.8, 1.5), DARK_WOOD);
            }
            for side in [-1.0f32, 1.0] {
                s.body(thick(polyline(&[(50.0 + side * 8.0, 6.0), (50.0 + side * 30.0, 96.0)]), 5.0), STEEL);
            }
        }
        Icon::Tunnel => {
            // A mountain with a stone arch at its foot and a road going in.
            s.body(poly(&[(2.0, 92.0), (40.0, 12.0), (56.0, 34.0), (68.0, 22.0), (98.0, 92.0)]), (150, 132, 116));
            s.detail(poly(&[(40.0, 12.0), (48.0, 24.0), (42.0, 22.0), (36.0, 28.0)]), (236, 236, 240));
            let mut pb = PathBuilder::new();
            pb.move_to(26.0, 92.0);
            pb.line_to(26.0, 68.0);
            pb.quad_to(50.0, 38.0, 74.0, 68.0);
            pb.line_to(74.0, 92.0);
            pb.close();
            s.body(pb.finish().unwrap(), STONE);
            let mut pb = PathBuilder::new();
            pb.move_to(36.0, 92.0);
            pb.line_to(36.0, 72.0);
            pb.quad_to(50.0, 52.0, 64.0, 72.0);
            pb.line_to(64.0, 92.0);
            pb.close();
            s.detail(pb.finish().unwrap(), (28, 22, 18));
            s.detail(poly(&[(44.0, 92.0), (56.0, 92.0), (53.0, 80.0), (47.0, 80.0)]), (176, 146, 106));
        }
        Icon::Canal => {
            // Water between two stone banks, with a boat on it.
            s.body(rrect(4.0, 24.0, 92.0, 56.0, 4.0), STONE);
            s.detail(rrect(4.0, 34.0, 92.0, 36.0, 2.0), (70, 140, 200));
            for x in [14.0f32, 38.0, 62.0, 86.0] {
                s.line_alpha(polyline(&[(x - 6.0, 46.0), (x + 4.0, 46.0)]), (200, 230, 250), 2.5, 0.8);
            }
            s.body(poly(&[(30.0, 56.0), (70.0, 56.0), (62.0, 66.0), (38.0, 66.0)]), WOOD);
            s.line(polyline(&[(50.0, 56.0), (50.0, 30.0)]), DARK_WOOD, 3.0);
            s.body(poly(&[(52.0, 30.0), (66.0, 52.0), (52.0, 52.0)]), CREAM);
        }
        Icon::Bridge => {
            // A stone bridge of three arches, a high one in the middle, over water.
            s.body(rrect(0.0, 62.0, 100.0, 30.0, 4.0), (70, 140, 200));
            for (x, y) in [(10.0f32, 80.0), (44.0, 86.0), (78.0, 78.0)] {
                s.line_alpha(polyline(&[(x - 6.0, y), (x + 6.0, y)]), (200, 230, 250), 2.5, 0.8);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(2.0, 30.0);
            pb.line_to(98.0, 30.0);
            pb.line_to(98.0, 76.0);
            pb.line_to(88.0, 76.0);
            pb.quad_to(77.0, 42.0, 66.0, 76.0);
            pb.line_to(62.0, 76.0);
            pb.quad_to(50.0, 22.0, 38.0, 76.0);
            pb.line_to(34.0, 76.0);
            pb.quad_to(23.0, 42.0, 12.0, 76.0);
            pb.line_to(2.0, 76.0);
            pb.close();
            s.body(pb.finish().unwrap(), STONE);
            // The parapet.
            s.detail(rrect(2.0, 24.0, 96.0, 8.0, 2.0), (150, 140, 124));
        }
        Icon::Cattle => {
            let hide = (122, 82, 52);
            s.body(rrect(20.0, 30.0, 60.0, 52.0, 22.0), hide);
            s.body(poly(&[(22.0, 36.0), (4.0, 22.0), (8.0, 16.0), (28.0, 30.0)]), CREAM);
            s.body(poly(&[(78.0, 36.0), (96.0, 22.0), (92.0, 16.0), (72.0, 30.0)]), CREAM);
            s.body(rrect(32.0, 62.0, 36.0, 26.0, 12.0), (226, 180, 156));
            s.detail(circle(42.0, 75.0, 3.5), (90, 56, 46));
            s.detail(circle(58.0, 75.0, 3.5), (90, 56, 46));
            s.detail(circle(36.0, 48.0, 4.0), (20, 16, 12));
            s.detail(circle(64.0, 48.0, 4.0), (20, 16, 12));
        }
        Icon::Sheep => {
            let wool = (240, 236, 224);
            for (x, y, r) in [(30.0, 44.0, 16.0), (50.0, 38.0, 17.0), (70.0, 44.0, 16.0), (38.0, 60.0, 16.0), (62.0, 60.0, 16.0)] {
                s.body(circle(x, y, r), wool);
            }
            s.body(oval(50.0, 70.0, 13.0, 16.0, 0.0), (70, 60, 54));
            s.detail(circle(44.0, 66.0, 2.5), (240, 236, 224));
            s.detail(circle(56.0, 66.0, 2.5), (240, 236, 224));
            s.line(polyline(&[(42.0, 86.0), (42.0, 94.0)]), (70, 60, 54), 4.0);
            s.line(polyline(&[(58.0, 86.0), (58.0, 94.0)]), (70, 60, 54), 4.0);
        }
        Icon::Deer => {
            let fur = (176, 118, 66);
            s.body(oval(50.0, 66.0, 15.0, 22.0, 0.0), fur);
            s.body(oval(50.0, 84.0, 9.0, 8.0, 0.0), (214, 170, 120));
            for side in [-1.0f32, 1.0] {
                let x = |v: f32| 50.0 + side * v;
                s.body(thick(polyline(&[(x(8.0), 48.0), (x(22.0), 26.0), (x(30.0), 8.0)]), 5.0), CREAM);
                s.body(thick(polyline(&[(x(18.0), 32.0), (x(36.0), 28.0)]), 4.0), CREAM);
                s.body(thick(polyline(&[(x(25.0), 18.0), (x(40.0), 12.0)]), 4.0), CREAM);
                s.body(oval(x(20.0), 52.0, 8.0, 4.5, side * 30.0), fur);
            }
            s.detail(circle(43.0, 62.0, 3.0), (20, 16, 12));
            s.detail(circle(57.0, 62.0, 3.0), (20, 16, 12));
        }
        Icon::Bananas => {
            let yellow = (246, 214, 74);
            for (i, a) in [-28.0f32, -8.0, 12.0].iter().enumerate() {
                let mut pb = PathBuilder::new();
                pb.move_to(44.0, 22.0);
                pb.quad_to(10.0, 50.0, 40.0, 88.0);
                pb.quad_to(26.0, 50.0, 54.0, 26.0);
                pb.close();
                let p = rotated(pb.finish().unwrap(), *a, 50.0, 22.0);
                s.body(p, shade(yellow, 1.0 - i as f32 * 0.06, 0.0));
            }
            s.body(rrect(42.0, 8.0, 12.0, 18.0, 3.0), (110, 140, 60));
        }
        Icon::Stone => {
            s.body(poly(&[(10.0, 86.0), (18.0, 56.0), (40.0, 46.0), (52.0, 62.0), (48.0, 86.0)]), STONE);
            s.body(poly(&[(44.0, 86.0), (52.0, 50.0), (72.0, 30.0), (90.0, 48.0), (92.0, 86.0)]), (166, 158, 144));
            s.line_alpha(polyline(&[(58.0, 54.0), (74.0, 38.0)]), (240, 236, 226), 2.5, 0.7);
        }
        Icon::Fish => {
            let scale = (96, 150, 196);
            s.body(oval(46.0, 50.0, 32.0, 18.0, 0.0), scale);
            s.body(poly(&[(74.0, 50.0), (94.0, 30.0), (94.0, 70.0)]), scale);
            s.detail(circle(26.0, 46.0, 4.0), (20, 24, 32));
            s.line_alpha(arc(38.0, 50.0, 14.0, 300.0, 420.0), (220, 236, 248), 2.5, 0.8);
            s.line_alpha(arc(52.0, 50.0, 14.0, 300.0, 420.0), (220, 236, 248), 2.5, 0.8);
        }
        Icon::Wine => {
            let grape = (122, 54, 120);
            for (x, y) in [(36.0, 40.0), (52.0, 40.0), (68.0, 40.0), (44.0, 54.0), (60.0, 54.0), (52.0, 68.0), (36.0, 26.0)] {
                if (x, y) == (36.0, 26.0) {
                    continue;
                }
                s.body(circle(x, y, 9.0), grape);
            }
            s.body(oval(64.0, 20.0, 16.0, 9.0, -20.0), (98, 156, 70));
            s.line(polyline(&[(52.0, 30.0), (54.0, 16.0)]), DARK_WOOD, 4.0);
            s.detail(circle(49.0, 37.0, 2.5), (220, 180, 220));
        }
        Icon::Silk => {
            let cloth = (226, 96, 120);
            let mut pb = PathBuilder::new();
            pb.move_to(10.0, 30.0);
            pb.quad_to(50.0, 10.0, 90.0, 30.0);
            pb.quad_to(70.0, 50.0, 90.0, 74.0);
            pb.quad_to(50.0, 92.0, 10.0, 74.0);
            pb.quad_to(30.0, 50.0, 10.0, 30.0);
            pb.close();
            s.body(pb.finish().unwrap(), cloth);
            s.line_alpha(arc(50.0, 120.0, 80.0, 240.0, 300.0), (255, 210, 220), 3.0, 0.8);
            s.line_alpha(arc(50.0, 140.0, 80.0, 245.0, 295.0), (255, 210, 220), 3.0, 0.6);
        }
        Icon::Spices => {
            s.body(oval(50.0, 72.0, 36.0, 18.0, 0.0), (150, 98, 58));
            let mut pb = PathBuilder::new();
            pb.move_to(18.0, 70.0);
            pb.quad_to(50.0, 10.0, 82.0, 70.0);
            pb.close();
            s.body(pb.finish().unwrap(), (210, 84, 40));
            s.detail(circle(40.0, 50.0, 3.0), (250, 180, 70));
            s.detail(circle(58.0, 44.0, 3.0), (250, 180, 70));
            s.detail(circle(52.0, 60.0, 3.0), (250, 180, 70));
        }
        Icon::Incense => {
            s.body(poly(&[(26.0, 70.0), (74.0, 70.0), (64.0, 92.0), (36.0, 92.0)]), BRASS);
            s.body(rrect(20.0, 64.0, 60.0, 9.0, 3.0), (190, 148, 62));
            for (x, a) in [(40.0f32, 0.0f32), (56.0, 8.0)] {
                let mut pb = PathBuilder::new();
                pb.move_to(x, 60.0);
                pb.cubic_to(x - 14.0, 46.0, x + 14.0, 36.0, x + a - 4.0, 12.0);
                s.line_alpha(pb.finish().unwrap(), (228, 230, 236), 6.0, 0.9);
            }
        }
        Icon::Furs => {
            let fur = (160, 110, 70);
            let mut pb = PathBuilder::new();
            pb.move_to(30.0, 14.0);
            pb.quad_to(50.0, 22.0, 70.0, 14.0);
            pb.line_to(76.0, 34.0);
            pb.quad_to(94.0, 40.0, 92.0, 56.0);
            pb.quad_to(78.0, 54.0, 72.0, 62.0);
            pb.quad_to(76.0, 80.0, 66.0, 92.0);
            pb.quad_to(50.0, 84.0, 34.0, 92.0);
            pb.quad_to(24.0, 80.0, 28.0, 62.0);
            pb.quad_to(22.0, 54.0, 8.0, 56.0);
            pb.quad_to(6.0, 40.0, 24.0, 34.0);
            pb.close();
            s.body(pb.finish().unwrap(), fur);
            s.detail(oval(50.0, 56.0, 12.0, 24.0, 0.0), (196, 150, 104));
        }
        Icon::Ivory => {
            let mut pb = PathBuilder::new();
            pb.move_to(20.0, 90.0);
            pb.quad_to(24.0, 30.0, 82.0, 10.0);
            pb.quad_to(52.0, 40.0, 38.0, 92.0);
            pb.close();
            s.body(pb.finish().unwrap(), (240, 230, 204));
            s.line_alpha(polyline(&[(26.0, 70.0), (38.0, 76.0)]), (190, 172, 140), 2.5, 0.8);
            s.line_alpha(polyline(&[(30.0, 54.0), (42.0, 60.0)]), (190, 172, 140), 2.5, 0.8);
        }
        Icon::Gems => {
            let ruby = (64, 176, 196);
            s.body(poly(&[(28.0, 18.0), (72.0, 18.0), (90.0, 40.0), (50.0, 90.0), (10.0, 40.0)]), ruby);
            s.detail(poly(&[(28.0, 18.0), (40.0, 40.0), (10.0, 40.0)]), (150, 226, 238));
            s.detail(poly(&[(40.0, 40.0), (60.0, 40.0), (50.0, 90.0)]), (40, 140, 166));
            s.line_alpha(polyline(&[(10.0, 40.0), (90.0, 40.0)]), (210, 246, 250), 2.0, 0.8);
        }
        Icon::GoldOre => {
            s.body(poly(&[(10.0, 80.0), (22.0, 46.0), (50.0, 30.0), (80.0, 40.0), (92.0, 76.0), (60.0, 90.0)]), (120, 108, 96));
            for (x, y, r) in [(36.0, 56.0, 9.0), (60.0, 50.0, 7.0), (54.0, 72.0, 8.0), (76.0, 66.0, 5.0)] {
                s.detail(circle(x, y, r), GOLD);
                s.detail_alpha(circle(x - r * 0.3, y - r * 0.3, r * 0.35), (255, 246, 200), 0.9);
            }
        }
        Icon::Salt => {
            s.body(poly(&[(10.0, 88.0), (50.0, 22.0), (90.0, 88.0)]), (238, 240, 244));
            s.detail(poly(&[(50.0, 22.0), (66.0, 88.0), (90.0, 88.0)]), (204, 210, 222));
            for (x, y) in [(40.0, 70.0), (56.0, 56.0), (48.0, 80.0)] {
                s.detail(rrect(x - 3.0, y - 3.0, 6.0, 6.0, 1.0), (255, 255, 255));
            }
        }
        Icon::Marble => {
            s.body(rrect(14.0, 30.0, 72.0, 52.0, 4.0), (236, 232, 226));
            s.line_alpha(polyline(&[(20.0, 44.0), (40.0, 52.0), (54.0, 46.0), (80.0, 62.0)]), (170, 170, 186), 2.5, 0.9);
            s.line_alpha(polyline(&[(26.0, 72.0), (46.0, 64.0), (70.0, 74.0)]), (170, 170, 186), 2.0, 0.8);
            s.detail(poly(&[(14.0, 30.0), (86.0, 30.0), (78.0, 20.0), (22.0, 20.0)]), (250, 248, 244));
        }
        Icon::Pearls => {
            let mut pb = PathBuilder::new();
            pb.move_to(8.0, 64.0);
            pb.quad_to(50.0, 18.0, 92.0, 64.0);
            pb.quad_to(50.0, 96.0, 8.0, 64.0);
            pb.close();
            s.body(pb.finish().unwrap(), (166, 140, 120));
            s.detail(oval(50.0, 66.0, 34.0, 16.0, 0.0), (220, 196, 180));
            s.body(circle(50.0, 58.0, 13.0), (246, 244, 240));
            s.detail_alpha(circle(46.0, 54.0, 4.0), (255, 255, 255), 0.95);
        }
        Icon::Horses => {
            let coat = (126, 82, 50);
            let mut pb = PathBuilder::new();
            pb.move_to(24.0, 92.0);
            pb.quad_to(22.0, 58.0, 38.0, 38.0);
            pb.line_to(44.0, 14.0);
            pb.line_to(54.0, 26.0);
            pb.quad_to(74.0, 30.0, 88.0, 54.0);
            pb.quad_to(88.0, 64.0, 78.0, 66.0);
            pb.quad_to(68.0, 62.0, 58.0, 58.0);
            pb.quad_to(56.0, 76.0, 66.0, 92.0);
            pb.close();
            s.body(pb.finish().unwrap(), coat);
            s.detail(poly(&[(38.0, 38.0), (44.0, 14.0), (42.0, 40.0), (30.0, 62.0), (26.0, 88.0), (24.0, 60.0)]), (52, 36, 24));
            s.detail(circle(60.0, 38.0, 3.5), (24, 18, 12));
        }
        Icon::Iron => {
            let iron = (126, 132, 146);
            for (y, x) in [(64.0f32, 10.0f32), (64.0, 50.0), (34.0, 30.0)] {
                s.body(poly(&[(x, y + 24.0), (x + 6.0, y), (x + 40.0, y), (x + 46.0, y + 24.0)]), iron);
                s.line_alpha(polyline(&[(x + 9.0, y + 5.0), (x + 37.0, y + 5.0)]), (220, 226, 236), 2.5, 0.8);
            }
        }
        Icon::CompositeBowman => {
            // A recurve bow of horn and sinew, drawn, with the arrow nocked.
            let mut pb = PathBuilder::new();
            pb.move_to(22.0, 6.0);
            pb.quad_to(38.0, 4.0, 38.0, 16.0);
            pb.quad_to(66.0, 50.0, 38.0, 84.0);
            pb.quad_to(38.0, 96.0, 22.0, 94.0);
            s.body(thick(pb.finish().unwrap(), 7.0), (120, 74, 44));
            s.line_alpha(polyline(&[(36.0, 30.0), (46.0, 50.0), (36.0, 70.0)]), (226, 196, 140), 3.0, 0.9);
            s.line(polyline(&[(24.0, 8.0), (12.0, 50.0), (24.0, 92.0)]), (238, 230, 210), 2.0);
            s.body(rrect(12.0, 47.5, 72.0, 5.0, 2.0), (200, 170, 120));
            s.body(poly(&[(80.0, 41.0), (97.0, 50.0), (80.0, 59.0)]), STEEL);
            s.body(poly(&[(8.0, 42.0), (20.0, 47.0), (20.0, 53.0), (8.0, 58.0), (13.0, 50.0)]), (66, 108, 176));
        }
        Icon::ChariotArcher => {
            // A light two-wheeled chariot with a bow above it.
            s.body(poly(&[(30.0, 44.0), (80.0, 44.0), (86.0, 66.0), (34.0, 66.0)]), (176, 64, 52));
            s.detail(poly(&[(36.0, 50.0), (78.0, 50.0), (81.0, 60.0), (39.0, 60.0)]), (130, 44, 38));
            s.body(thick(polyline(&[(32.0, 58.0), (6.0, 50.0)]), 4.0), DARK_WOOD);
            s.body(circle(56.0, 72.0, 20.0), DARK_WOOD);
            for a in [0.0f32, 60.0, 120.0] {
                let (c, sn) = (a.to_radians().cos() * 14.0, a.to_radians().sin() * 14.0);
                s.line(polyline(&[(56.0 - c, 72.0 - sn), (56.0 + c, 72.0 + sn)]), (176, 128, 80), 2.5);
            }
            s.detail(circle(56.0, 72.0, 4.0), BRASS);
            let mut pb = PathBuilder::new();
            pb.move_to(30.0, 30.0);
            pb.quad_to(58.0, 0.0, 86.0, 30.0);
            s.body(thick(pb.finish().unwrap(), 6.0), WOOD);
            s.line(polyline(&[(31.0, 30.0), (85.0, 30.0)]), (238, 230, 210), 2.0);
        }
        Icon::Pikeman => {
            // Two long pikes crossed, with a pennant.
            for (deg, c) in [(-22.0f32, WOOD), (22.0, DARK_WOOD)] {
                s.body(rotated(rrect(47.0, 10.0, 6.0, 88.0, 3.0), deg, 50.0, 54.0), c);
                s.body(rotated(poly(&[(50.0, -2.0), (56.0, 12.0), (50.0, 18.0), (44.0, 12.0)]), deg, 50.0, 54.0), STEEL);
            }
            s.body(rotated(poly(&[(53.0, 18.0), (74.0, 22.0), (53.0, 30.0)]), 22.0, 50.0, 54.0), (186, 72, 56));
            s.detail(circle(50.0, 54.0, 5.0), BRASS);
        }
        Icon::Crossbowman => {
            // A crossbow seen from above: stock, steel prod, string and bolt.
            s.body(rrect(45.0, 30.0, 10.0, 64.0, 3.0), WOOD);
            let mut pb = PathBuilder::new();
            pb.move_to(10.0, 42.0);
            pb.quad_to(50.0, 14.0, 90.0, 42.0);
            s.body(thick(pb.finish().unwrap(), 7.0), STEEL);
            s.line(polyline(&[(12.0, 43.0), (50.0, 62.0), (88.0, 43.0)]), (238, 230, 210), 2.0);
            s.body(rrect(47.5, 12.0, 5.0, 52.0, 2.0), DARK_WOOD);
            s.body(poly(&[(50.0, 2.0), (56.0, 16.0), (44.0, 16.0)]), STEEL);
            s.detail(rrect(46.0, 70.0, 8.0, 10.0, 2.0), BRASS);
            s.rotate(30.0);
        }
        Icon::Knight => {
            // A lance behind a heater shield with a gold chevron.
            s.body(rotated(poly(&[(48.0, 2.0), (54.0, 2.0), (58.0, 96.0), (44.0, 96.0)]), 35.0, 50.0, 50.0), (200, 184, 150));
            s.body(rotated(poly(&[(40.0, 66.0), (62.0, 66.0), (64.0, 74.0), (38.0, 74.0)]), 35.0, 50.0, 50.0), STEEL);
            s.body(shield_path(0.72).transform(Transform::from_translate(-10.0, 6.0)).unwrap(), (54, 92, 166));
            s.detail(poly(&[(18.0, 64.0), (40.0, 40.0), (62.0, 64.0), (62.0, 74.0), (40.0, 50.0), (18.0, 74.0)]), GOLD);
        }
        Icon::Trebuchet => {
            // Counterweight engine: A-frame, long arm, weight box and sling.
            s.body(poly(&[(30.0, 88.0), (50.0, 34.0), (70.0, 88.0), (62.0, 88.0), (50.0, 52.0), (38.0, 88.0)]), DARK_WOOD);
            s.body(rrect(8.0, 84.0, 84.0, 9.0, 3.0), WOOD);
            s.body(thick(polyline(&[(14.0, 12.0), (68.0, 54.0)]), 6.0), WOOD);
            s.body(rrect(60.0, 52.0, 20.0, 18.0, 2.0), STONE);
            s.line(polyline(&[(14.0, 12.0), (16.0, 34.0)]), (226, 210, 170), 2.0);
            s.body(circle(16.0, 38.0, 6.0), STONE);
            s.detail(circle(50.0, 40.0, 4.0), BRASS);
        }
        Icon::Longswordsman => {
            // A long two-handed sword behind a great helm.
            s.body(poly(&[(46.0, 16.0), (50.0, 0.0), (54.0, 16.0), (54.0, 66.0), (46.0, 66.0)]), STEEL);
            s.body(rrect(30.0, 64.0, 40.0, 7.0, 3.0), BRASS);
            s.body(rrect(46.0, 70.0, 8.0, 22.0, 2.0), DARK_WOOD);
            s.body(circle(50.0, 95.0, 5.0), BRASS);
            s.rotate(-40.0);
            s.body(rrect(36.0, 34.0, 40.0, 48.0, 9.0), (150, 158, 172));
            s.detail(rrect(40.0, 50.0, 32.0, 5.0, 2.0), (34, 30, 30));
            s.detail(rrect(53.0, 55.0, 6.0, 20.0, 2.0), (120, 126, 140));
            s.line_alpha(polyline(&[(42.0, 40.0), (70.0, 40.0)]), (230, 236, 246), 2.5, 0.8);
        }
        Icon::Musketman => {
            // A musket with a bayonet, and a powder horn.
            s.body(poly(&[(10.0, 92.0), (30.0, 70.0), (40.0, 62.0), (46.0, 70.0), (22.0, 96.0)]), WOOD);
            s.body(thick(polyline(&[(36.0, 66.0), (86.0, 16.0)]), 6.0), (88, 92, 104));
            s.body(thick(polyline(&[(84.0, 18.0), (96.0, 4.0)]), 2.5), STEEL);
            s.detail(circle(40.0, 64.0, 3.0), BRASS);
            let mut pb = PathBuilder::new();
            pb.move_to(54.0, 82.0);
            pb.quad_to(76.0, 86.0, 88.0, 62.0);
            pb.line_to(94.0, 66.0);
            pb.quad_to(82.0, 96.0, 52.0, 92.0);
            pb.close();
            s.body(pb.finish().unwrap(), (226, 206, 160));
            s.detail(rrect(50.0, 80.0, 6.0, 14.0, 2.0), BRASS);
        }
        Icon::Cannon => {
            // A bronze barrel on a two-wheeled carriage, cannonballs beside it.
            s.body(rotated(poly(&[(14.0, 38.0), (78.0, 42.0), (78.0, 56.0), (14.0, 60.0)]), -18.0, 46.0, 50.0), BRASS);
            s.body(rotated(rrect(74.0, 38.0, 8.0, 22.0, 2.0), -18.0, 46.0, 50.0), (184, 140, 58));
            s.body(rotated(circle(12.0, 49.0, 7.0), -18.0, 46.0, 50.0), (184, 140, 58));
            s.body(poly(&[(20.0, 62.0), (60.0, 58.0), (92.0, 88.0), (84.0, 92.0)]), DARK_WOOD);
            s.body(circle(40.0, 72.0, 18.0), WOOD);
            for a in [0.0f32, 45.0, 90.0, 135.0] {
                let (c, sn) = (a.to_radians().cos() * 13.0, a.to_radians().sin() * 13.0);
                s.line(polyline(&[(40.0 - c, 72.0 - sn), (40.0 + c, 72.0 + sn)]), DARK_WOOD, 2.5);
            }
            s.detail(circle(40.0, 72.0, 4.0), STEEL);
            for (x, y) in [(8.0, 90.0), (20.0, 90.0), (14.0, 81.0)] {
                s.body(circle(x, y, 5.5), (60, 60, 66));
            }
        }
        Icon::Caravel => {
            // Two lateen sails on a high-sterned hull.
            s.body(rrect(56.0, 8.0, 4.0, 56.0, 2.0), DARK_WOOD);
            s.body(rrect(30.0, 20.0, 4.0, 44.0, 2.0), DARK_WOOD);
            s.body(poly(&[(58.0, 6.0), (86.0, 56.0), (40.0, 56.0)]), CREAM);
            s.body(poly(&[(32.0, 18.0), (52.0, 56.0), (16.0, 56.0)]), CREAM);
            s.detail(poly(&[(60.0, 30.0), (64.0, 30.0), (64.0, 52.0), (60.0, 52.0)]), (176, 64, 52));
            s.detail(poly(&[(52.0, 38.0), (72.0, 38.0), (72.0, 42.0), (52.0, 42.0)]), (176, 64, 52));
            let mut pb = PathBuilder::new();
            pb.move_to(4.0, 50.0);
            pb.line_to(22.0, 54.0);
            pb.line_to(96.0, 58.0);
            pb.quad_to(88.0, 80.0, 70.0, 82.0);
            pb.line_to(24.0, 82.0);
            pb.quad_to(8.0, 76.0, 4.0, 50.0);
            pb.close();
            s.body(pb.finish().unwrap(), WOOD);
            s.detail(poly(&[(12.0, 64.0), (90.0, 66.0), (88.0, 70.0), (13.0, 68.0)]), DARK_WOOD);
        }
        Icon::Frigate => {
            // Three masts of square sails over a dark hull with a row of gunports.
            for (x, h) in [(24.0f32, 40.0f32), (50.0, 50.0), (76.0, 42.0)] {
                s.body(rrect(x - 2.0, 60.0 - h - 6.0, 4.0, h + 6.0, 2.0), DARK_WOOD);
                s.body(poly(&[(x - 12.0, 60.0 - h), (x + 12.0, 60.0 - h), (x + 14.0, 60.0 - h / 2.0 - 2.0), (x - 14.0, 60.0 - h / 2.0 - 2.0)]), CREAM);
                s.body(poly(&[(x - 14.0, 60.0 - h / 2.0 + 2.0), (x + 14.0, 60.0 - h / 2.0 + 2.0), (x + 15.0, 56.0), (x - 15.0, 56.0)]), CREAM);
            }
            s.body(galley_hull(2.0, 98.0, 62.0, 86.0), (70, 52, 40));
            s.detail(poly(&[(12.0, 66.0), (90.0, 66.0), (89.0, 70.0), (13.0, 70.0)]), (214, 172, 78));
            for x in [22.0, 34.0, 46.0, 58.0, 70.0, 82.0] {
                s.detail(rrect(x - 3.0, 73.0, 6.0, 5.0, 1.0), (24, 18, 14));
            }
        }
        Icon::LumberMill => {
            // A circular saw blade cutting into a log.
            s.body(rrect(6.0, 62.0, 88.0, 26.0, 12.0), (150, 98, 55));
            s.body(circle(82.0, 75.0, 13.0), (214, 176, 120));
            s.line(circle(82.0, 75.0, 7.0), (160, 112, 66), 2.0);
            let mut pts = Vec::new();
            for k in 0..16 {
                let a = (k as f32 * 22.5).to_radians();
                let b = (k as f32 * 22.5 + 14.0).to_radians();
                pts.push((40.0 + 34.0 * a.cos(), 40.0 + 34.0 * a.sin()));
                pts.push((40.0 + 26.0 * b.cos(), 40.0 + 26.0 * b.sin()));
            }
            s.body_eo(combine(&[poly(&pts), circle(40.0, 40.0, 7.0)]), STEEL);
            s.line_alpha(circle(40.0, 40.0, 17.0), (130, 136, 150), 2.5, 0.8);
        }
        Icon::Niter => {
            // Pale saltpetre crystals on dark rock.
            s.body(poly(&[(6.0, 92.0), (16.0, 70.0), (84.0, 66.0), (96.0, 92.0)]), (96, 86, 78));
            let crystal = (236, 232, 214);
            for (x, w, h, deg) in [(30.0f32, 12.0f32, 46.0f32, -14.0f32), (50.0, 14.0, 60.0, 4.0), (70.0, 12.0, 40.0, 18.0)] {
                let p = poly(&[(x - w / 2.0, 80.0), (x - w / 2.0, 80.0 - h + 8.0), (x, 80.0 - h), (x + w / 2.0, 80.0 - h + 8.0), (x + w / 2.0, 80.0)]);
                s.body(rotated(p, deg, x, 80.0), crystal);
                s.line_alpha(rotated(polyline(&[(x - 1.0, 78.0), (x - 1.0, 84.0 - h)]), deg, x, 80.0), (255, 255, 255), 2.0, 0.8);
            }
        }
        Icon::Harbor => {
            // An anchor.
            let iron = (96, 110, 134);
            s.body_eo(combine(&[circle(50.0, 16.0, 10.0), circle(50.0, 16.0, 5.0)]), iron);
            s.body(rrect(46.0, 24.0, 8.0, 62.0, 3.0), iron);
            s.body(rrect(30.0, 30.0, 40.0, 8.0, 3.0), iron);
            s.body(thick(arc(50.0, 52.0, 34.0, 20.0, 160.0), 8.0), iron);
            s.body(poly(&[(84.0, 52.0), (92.0, 66.0), (78.0, 66.0)]), iron);
            s.body(poly(&[(16.0, 52.0), (22.0, 66.0), (8.0, 66.0)]), iron);
            s.line_alpha(polyline(&[(50.0, 28.0), (50.0, 82.0)]), (190, 204, 226), 2.0, 0.8);
        }
        Icon::Aqueduct => {
            // Two tiers of stone arches carrying a channel of water.
            s.body(rrect(6.0, 20.0, 88.0, 72.0, 2.0), STONE);
            s.detail(rrect(6.0, 14.0, 88.0, 8.0, 2.0), (86, 150, 214));
            for x in [10.0f32, 30.0, 50.0, 70.0] {
                let mut pb = PathBuilder::new();
                pb.move_to(x + 2.0, 50.0);
                pb.line_to(x + 2.0, 36.0);
                pb.quad_to(x + 10.0, 26.0, x + 18.0, 36.0);
                pb.line_to(x + 18.0, 50.0);
                pb.close();
                s.detail(pb.finish().unwrap(), (88, 74, 60));
            }
            for x in [10.0f32, 50.0] {
                let mut pb = PathBuilder::new();
                pb.move_to(x + 4.0, 92.0);
                pb.line_to(x + 4.0, 70.0);
                pb.quad_to(x + 20.0, 52.0, x + 36.0, 70.0);
                pb.line_to(x + 36.0, 92.0);
                pb.close();
                s.detail(pb.finish().unwrap(), (88, 74, 60));
            }
            s.line_alpha(polyline(&[(8.0, 54.0), (92.0, 54.0)]), (130, 122, 108), 2.5, 0.9);
        }
        Icon::University => {
            // A domed hall with columns.
            let marble = (226, 214, 188);
            let mut pb = PathBuilder::new();
            pb.move_to(24.0, 40.0);
            pb.quad_to(24.0, 12.0, 50.0, 12.0);
            pb.quad_to(76.0, 12.0, 76.0, 40.0);
            pb.close();
            s.body(pb.finish().unwrap(), (110, 150, 176));
            s.body(rrect(46.0, 2.0, 8.0, 12.0, 2.0), BRASS);
            s.body(rrect(14.0, 38.0, 72.0, 8.0, 1.0), (206, 196, 176));
            for x in [18.0, 34.0, 50.0, 66.0] {
                s.body(rrect(x, 48.0, 10.0, 34.0, 2.0), marble);
            }
            s.body(rrect(8.0, 82.0, 84.0, 10.0, 2.0), (190, 180, 160));
            s.line_alpha(arc(50.0, 40.0, 18.0, 200.0, 260.0), (230, 240, 250), 2.5, 0.7);
        }
        Icon::Castle => {
            // A keep between two towers, all crenellated, with a pennant.
            let stone = (176, 168, 152);
            for x in [8.0f32, 70.0] {
                s.body(rrect(x, 30.0, 22.0, 62.0, 2.0), stone);
                for k in 0..3 {
                    s.body(rrect(x + k as f32 * 8.0, 22.0, 6.0, 10.0, 1.0), stone);
                }
            }
            s.body(rrect(28.0, 18.0, 44.0, 74.0, 2.0), (196, 188, 170));
            for k in 0..4 {
                s.body(rrect(28.0 + k as f32 * 12.0, 10.0, 8.0, 10.0, 1.0), (196, 188, 170));
            }
            let mut pb = PathBuilder::new();
            pb.move_to(40.0, 92.0);
            pb.line_to(40.0, 70.0);
            pb.quad_to(50.0, 56.0, 60.0, 70.0);
            pb.line_to(60.0, 92.0);
            pb.close();
            s.detail(pb.finish().unwrap(), (70, 56, 44));
            s.detail(rrect(46.0, 34.0, 8.0, 14.0, 3.0), (70, 56, 44));
            s.line(polyline(&[(50.0, 12.0), (50.0, 3.0)]), DARK_WOOD, 2.5);
            s.body(poly(&[(51.0, 3.0), (64.0, 6.5), (51.0, 10.0)]), (186, 72, 56));
        }
        Icon::Observatory => {
            // A slit dome with a telescope.
            s.body(rrect(14.0, 60.0, 72.0, 32.0, 2.0), STONE);
            s.body(thick(polyline(&[(46.0, 44.0), (86.0, 10.0)]), 9.0), BRASS);
            let mut pb = PathBuilder::new();
            pb.move_to(16.0, 62.0);
            pb.quad_to(16.0, 24.0, 50.0, 24.0);
            pb.quad_to(84.0, 24.0, 84.0, 62.0);
            pb.close();
            s.body(pb.finish().unwrap(), (198, 204, 214));
            s.detail(poly(&[(46.0, 26.0), (56.0, 26.0), (58.0, 60.0), (44.0, 60.0)]), (36, 40, 56));
            s.detail(rrect(40.0, 74.0, 20.0, 18.0, 6.0), (88, 74, 60));
            s.detail(circle(82.0, 12.0, 3.0), (255, 236, 160));
        }
        Icon::Bank => {
            // Stacks of gold coins.
            for (x, n) in [(22.0f32, 4), (50.0, 6), (78.0, 3)] {
                for k in 0..n {
                    let y = 86.0 - k as f32 * 10.0;
                    s.body(oval(x, y, 15.0, 6.0, 0.0), GOLD);
                }
                let top = 86.0 - (n - 1) as f32 * 10.0;
                s.line_alpha(oval(x, top, 9.0, 3.0, 0.0), (186, 132, 26), 2.0, 0.9);
            }
        }
        Icon::StarFort => {
            // A star-shaped fortress seen from above.
            let mut pts = Vec::new();
            for k in 0..10 {
                let a = (k as f32 * 36.0 - 90.0).to_radians();
                let r = if k % 2 == 0 { 46.0 } else { 27.0 };
                pts.push((50.0 + r * a.cos(), 52.0 + r * a.sin()));
            }
            s.body(poly(&pts), STONE);
            let inner: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (50.0 + (x - 50.0) * 0.62, 52.0 + (y - 52.0) * 0.62)).collect();
            s.detail(poly(&inner), (110, 150, 76));
            s.detail(rrect(42.0, 44.0, 16.0, 16.0, 2.0), (176, 168, 152));
            s.line_alpha(poly(&pts), (130, 122, 108), 2.0, 0.6);
        }
        Icon::Slinger => {
            // A leather sling whirled round, its stone flying off.
            let leather = (160, 110, 64);
            s.body(thick(arc(46.0, 50.0, 30.0, 120.0, 400.0), 4.0), (214, 190, 146));
            s.body(oval(70.0, 30.0, 12.0, 8.0, 35.0), leather);
            s.body(circle(72.0, 28.0, 7.0), STONE);
            s.body(rrect(36.0, 70.0, 14.0, 22.0, 5.0), leather);
            s.line_alpha(polyline(&[(82.0, 18.0), (94.0, 8.0)]), (240, 236, 220), 2.5, 0.8);
            s.line_alpha(polyline(&[(86.0, 26.0), (96.0, 22.0)]), (240, 236, 220), 2.5, 0.8);
        }
        Icon::Courser => {
            // A light horse's head at the gallop, with a pennant on a lance.
            let coat = (196, 150, 96);
            s.body(rotated(poly(&[(46.0, 2.0), (51.0, 2.0), (53.0, 70.0), (45.0, 70.0)]), -28.0, 50.0, 50.0), (200, 184, 150));
            s.body(poly(&[(38.0, 4.0), (60.0, 12.0), (40.0, 20.0)]), (190, 60, 48));
            let mut pb = PathBuilder::new();
            pb.move_to(24.0, 94.0);
            pb.quad_to(24.0, 62.0, 40.0, 42.0);
            pb.line_to(48.0, 24.0);
            pb.line_to(56.0, 34.0);
            pb.quad_to(76.0, 40.0, 90.0, 62.0);
            pb.quad_to(90.0, 72.0, 80.0, 72.0);
            pb.quad_to(70.0, 68.0, 60.0, 64.0);
            pb.quad_to(58.0, 80.0, 66.0, 94.0);
            pb.close();
            s.body(pb.finish().unwrap(), coat);
            s.detail(poly(&[(40.0, 42.0), (48.0, 24.0), (44.0, 46.0), (32.0, 66.0), (26.0, 90.0), (26.0, 64.0)]), (90, 60, 36));
            s.detail(circle(62.0, 48.0, 3.5), (30, 22, 16));
        }
        Icon::Cuirassier => {
            // A steel breastplate with a sabre across it.
            let mut pb = PathBuilder::new();
            pb.move_to(22.0, 22.0);
            pb.quad_to(50.0, 14.0, 78.0, 22.0);
            pb.line_to(80.0, 52.0);
            pb.quad_to(76.0, 82.0, 50.0, 92.0);
            pb.quad_to(24.0, 82.0, 20.0, 52.0);
            pb.close();
            s.body(pb.finish().unwrap(), STEEL);
            s.detail(poly(&[(36.0, 18.0), (50.0, 34.0), (64.0, 18.0)]), (120, 126, 140));
            s.line_alpha(polyline(&[(50.0, 34.0), (50.0, 86.0)]), (120, 126, 140), 2.5, 0.8);
            s.line_alpha(arc(50.0, 56.0, 22.0, 200.0, 250.0), (240, 244, 250), 3.0, 0.8);
            for (x, y) in [(28.0, 30.0), (72.0, 30.0)] {
                s.detail(circle(x, y, 3.0), BRASS);
            }
            s.body(thick(arc(104.0, 104.0, 92.0, 196.0, 246.0), 5.0), (214, 220, 230));
            s.body(rotated(rrect(4.0, 74.0, 22.0, 6.0, 2.0), -20.0, 15.0, 77.0), BRASS);
            s.body(circle(9.0, 86.0, 5.0), BRASS);
        }
        Icon::Explorer => {
            // A compass rose on a rolled map.
            s.body(rrect(8.0, 60.0, 84.0, 30.0, 4.0), (226, 206, 160));
            s.body(circle(12.0, 75.0, 9.0), (196, 172, 124));
            s.body(circle(88.0, 75.0, 9.0), (196, 172, 124));
            s.line_alpha(polyline(&[(24.0, 70.0), (44.0, 78.0), (60.0, 68.0), (76.0, 80.0)]), (176, 64, 52), 2.5, 0.8);
            s.body(circle(50.0, 36.0, 28.0), BRASS);
            s.detail(circle(50.0, 36.0, 22.0), CREAM);
            s.detail(poly(&[(50.0, 16.0), (56.0, 36.0), (50.0, 56.0), (44.0, 36.0)]), (176, 64, 52));
            s.detail(poly(&[(30.0, 36.0), (50.0, 31.0), (70.0, 36.0), (50.0, 41.0)]), (60, 60, 70));
            s.detail(circle(50.0, 36.0, 3.0), BRASS);
        }
        Icon::Galleon => {
            // A tall ship: square sails on two masts, a high castle aft.
            for (x, h) in [(36.0f32, 50.0f32), (64.0, 44.0)] {
                s.body(rrect(x - 2.0, 58.0 - h - 6.0, 4.0, h + 6.0, 2.0), DARK_WOOD);
                s.body(poly(&[(x - 14.0, 58.0 - h), (x + 14.0, 58.0 - h), (x + 16.0, 58.0 - h / 2.0 - 2.0), (x - 16.0, 58.0 - h / 2.0 - 2.0)]), CREAM);
                s.body(poly(&[(x - 16.0, 58.0 - h / 2.0 + 2.0), (x + 16.0, 58.0 - h / 2.0 + 2.0), (x + 17.0, 54.0), (x - 17.0, 54.0)]), CREAM);
                s.detail(rrect(x - 2.0, 58.0 - h + 6.0, 4.0, h / 2.0 - 10.0, 1.0), (176, 64, 52));
            }
            let mut pb = PathBuilder::new();
            pb.move_to(4.0, 40.0);
            pb.line_to(22.0, 42.0);
            pb.line_to(22.0, 56.0);
            pb.line_to(96.0, 58.0);
            pb.quad_to(88.0, 82.0, 70.0, 84.0);
            pb.line_to(24.0, 84.0);
            pb.quad_to(8.0, 76.0, 4.0, 40.0);
            pb.close();
            s.body(pb.finish().unwrap(), WOOD);
            s.detail(poly(&[(10.0, 64.0), (90.0, 66.0), (88.0, 70.0), (11.0, 68.0)]), BRASS);
        }
        Icon::FishingBoats => {
            // A small boat with a net hanging over its side.
            s.body(rrect(46.0, 14.0, 4.0, 52.0, 2.0), DARK_WOOD);
            s.body(poly(&[(50.0, 14.0), (76.0, 58.0), (50.0, 58.0)]), CREAM);
            let mut pb = PathBuilder::new();
            pb.move_to(8.0, 60.0);
            pb.line_to(92.0, 60.0);
            pb.quad_to(84.0, 80.0, 66.0, 82.0);
            pb.line_to(32.0, 82.0);
            pb.quad_to(14.0, 78.0, 8.0, 60.0);
            pb.close();
            s.body(pb.finish().unwrap(), WOOD);
            s.body(poly(&[(14.0, 62.0), (34.0, 62.0), (30.0, 94.0), (6.0, 90.0)]), (214, 200, 160));
            for k in 0..4 {
                let y = 68.0 + k as f32 * 7.0;
                s.line_alpha(polyline(&[(12.0, y), (32.0, y + 1.0)]), (130, 110, 80), 1.5, 0.9);
            }
            s.line_alpha(polyline(&[(4.0, 92.0), (96.0, 92.0)]), (96, 150, 196), 3.0, 0.9);
        }
        Icon::Shrine => {
            // A small open-sided shrine with a flame inside.
            let wood = (176, 82, 56);
            s.body(poly(&[(10.0, 38.0), (50.0, 12.0), (90.0, 38.0)]), wood);
            s.body(rrect(6.0, 34.0, 88.0, 8.0, 3.0), (150, 64, 44));
            s.body(rrect(20.0, 42.0, 9.0, 44.0, 2.0), wood);
            s.body(rrect(71.0, 42.0, 9.0, 44.0, 2.0), wood);
            s.body(rrect(12.0, 84.0, 76.0, 9.0, 2.0), STONE);
            s.body(rrect(40.0, 70.0, 20.0, 14.0, 2.0), STONE);
            let mut pb = PathBuilder::new();
            pb.move_to(50.0, 46.0);
            pb.quad_to(62.0, 58.0, 56.0, 68.0);
            pb.line_to(44.0, 68.0);
            pb.quad_to(38.0, 58.0, 50.0, 46.0);
            pb.close();
            s.body(pb.finish().unwrap(), (246, 170, 60));
            s.detail(oval(50.0, 62.0, 4.0, 6.0, 0.0), (255, 230, 140));
        }
        Icon::Barracks => {
            // Crossed swords over a tent.
            let mut pb = PathBuilder::new();
            pb.move_to(8.0, 92.0);
            pb.line_to(50.0, 40.0);
            pb.line_to(92.0, 92.0);
            pb.close();
            s.body(pb.finish().unwrap(), (186, 168, 122));
            s.detail(poly(&[(42.0, 92.0), (50.0, 66.0), (58.0, 92.0)]), (96, 72, 44));
            for deg in [-40.0f32, 40.0] {
                let blade = poly(&[(47.0, 8.0), (50.0, 0.0), (53.0, 8.0), (53.0, 46.0), (47.0, 46.0)]);
                s.body(rotated(blade, deg, 50.0, 40.0), STEEL);
                s.body(rotated(rrect(38.0, 44.0, 24.0, 6.0, 2.0), deg, 50.0, 40.0), BRASS);
                s.body(rotated(rrect(47.0, 49.0, 6.0, 12.0, 2.0), deg, 50.0, 40.0), DARK_WOOD);
            }
        }
        Icon::Lighthouse => {
            // A tapering tower with a light at the top, beams to each side.
            s.body(poly(&[(36.0, 92.0), (42.0, 34.0), (58.0, 34.0), (64.0, 92.0)]), (236, 230, 220));
            s.detail(poly(&[(39.0, 72.0), (61.0, 72.0), (62.0, 82.0), (38.0, 82.0)]), (190, 64, 52));
            s.detail(poly(&[(41.0, 48.0), (59.0, 48.0), (60.0, 58.0), (40.0, 58.0)]), (190, 64, 52));
            s.body(rrect(38.0, 28.0, 24.0, 8.0, 2.0), DARK_WOOD);
            s.body(rrect(42.0, 14.0, 16.0, 15.0, 2.0), (252, 214, 104));
            s.body(poly(&[(40.0, 14.0), (50.0, 4.0), (60.0, 14.0)]), DARK_WOOD);
            s.detail_alpha(poly(&[(42.0, 18.0), (4.0, 8.0), (4.0, 32.0), (42.0, 24.0)]), (255, 236, 150), 0.6);
            s.detail_alpha(poly(&[(58.0, 18.0), (96.0, 8.0), (96.0, 32.0), (58.0, 24.0)]), (255, 236, 150), 0.6);
            s.body(rrect(20.0, 88.0, 60.0, 8.0, 3.0), STONE);
        }
        Icon::Stable => {
            // A horseshoe.
            let iron = (130, 136, 150);
            s.body(thick(arc(50.0, 46.0, 30.0, 150.0, 390.0), 14.0), iron);
            for a in [170.0f32, 210.0, 250.0, 290.0, 330.0, 370.0] {
                let r = a.to_radians();
                s.detail(circle(50.0 + 30.0 * r.cos(), 46.0 + 30.0 * r.sin(), 2.5), (40, 40, 46));
            }
            s.line_alpha(arc(50.0, 46.0, 34.0, 200.0, 260.0), (230, 236, 246), 2.5, 0.8);
        }
        Icon::Armory => {
            // An anvil with a hammer.
            let iron = (96, 100, 112);
            s.body(poly(&[(10.0, 46.0), (80.0, 46.0), (92.0, 38.0), (90.0, 52.0), (72.0, 60.0), (64.0, 60.0), (64.0, 76.0), (36.0, 76.0), (36.0, 60.0), (24.0, 60.0)]), iron);
            s.body(rrect(26.0, 76.0, 48.0, 14.0, 3.0), (80, 84, 96));
            s.line_alpha(polyline(&[(16.0, 49.0), (78.0, 49.0)]), (190, 196, 210), 2.5, 0.8);
            s.body(rotated(rrect(46.0, 4.0, 8.0, 40.0, 3.0), -30.0, 50.0, 30.0), WOOD);
            s.body(rotated(rrect(36.0, 0.0, 28.0, 14.0, 3.0), -30.0, 50.0, 30.0), STEEL);
        }
        Icon::Caravanserai => {
            // A walled courtyard inn under a pointed gate arch.
            let clay = (214, 178, 124);
            s.body(rrect(6.0, 40.0, 88.0, 52.0, 2.0), clay);
            s.body(rrect(30.0, 24.0, 40.0, 20.0, 2.0), clay);
            for x in [6.0, 80.0] {
                s.body(rrect(x, 30.0, 14.0, 14.0, 2.0), clay);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(38.0, 92.0);
            pb.line_to(38.0, 56.0);
            pb.quad_to(38.0, 38.0, 50.0, 32.0);
            pb.quad_to(62.0, 38.0, 62.0, 56.0);
            pb.line_to(62.0, 92.0);
            pb.close();
            s.detail(pb.finish().unwrap(), (96, 62, 36));
            for x in [16.0, 76.0] {
                s.detail(rrect(x, 56.0, 8.0, 12.0, 4.0), (96, 62, 36));
            }
            s.line_alpha(polyline(&[(8.0, 46.0), (92.0, 46.0)]), (240, 214, 170), 2.0, 0.8);
        }
        Icon::Shipyard => {
            // A ship's ribs on a slipway, a crane above.
            s.body(poly(&[(4.0, 92.0), (96.0, 80.0), (96.0, 88.0), (4.0, 98.0)]), STONE);
            let mut pb = PathBuilder::new();
            pb.move_to(14.0, 60.0);
            pb.line_to(86.0, 56.0);
            pb.quad_to(80.0, 80.0, 62.0, 82.0);
            pb.line_to(30.0, 84.0);
            pb.quad_to(16.0, 80.0, 14.0, 60.0);
            pb.close();
            s.body(pb.finish().unwrap(), WOOD);
            for x in [26.0f32, 38.0, 50.0, 62.0, 74.0] {
                s.detail(rrect(x - 2.0, 60.0, 4.0, 20.0, 1.0), DARK_WOOD);
            }
            s.body(rrect(84.0, 10.0, 6.0, 48.0, 2.0), DARK_WOOD);
            s.body(thick(polyline(&[(87.0, 12.0), (30.0, 22.0)]), 5.0), DARK_WOOD);
            s.line(polyline(&[(36.0, 22.0), (36.0, 44.0)]), (226, 210, 170), 2.0);
            s.body(rrect(30.0, 44.0, 12.0, 8.0, 2.0), STEEL);
        }
        Icon::OperaHouse => {
            // Comedy and tragedy masks over a curtain.
            s.body(rrect(6.0, 8.0, 88.0, 84.0, 4.0), (164, 40, 52));
            for x in [22.0, 40.0, 60.0, 78.0] {
                s.line_alpha(polyline(&[(x, 14.0), (x, 88.0)]), (110, 22, 34), 3.0, 0.8);
            }
            s.body(rrect(6.0, 6.0, 88.0, 12.0, 3.0), GOLD);
            let mask = |cx: f32, cy: f32| {
                let mut pb = PathBuilder::new();
                pb.move_to(cx - 16.0, cy - 16.0);
                pb.quad_to(cx, cy - 22.0, cx + 16.0, cy - 16.0);
                pb.quad_to(cx + 18.0, cy + 12.0, cx, cy + 20.0);
                pb.quad_to(cx - 18.0, cy + 12.0, cx - 16.0, cy - 16.0);
                pb.close();
                pb.finish().unwrap()
            };
            s.body(mask(36.0, 54.0), CREAM);
            s.body(mask(64.0, 50.0), GOLD);
            s.detail(oval(30.0, 48.0, 4.0, 3.0, 0.0), (40, 30, 30));
            s.detail(oval(42.0, 48.0, 4.0, 3.0, 0.0), (40, 30, 30));
            s.line(arc(36.0, 58.0, 8.0, 20.0, 160.0), (40, 30, 30), 2.5);
            s.detail(oval(58.0, 44.0, 4.0, 3.0, 0.0), (40, 30, 30));
            s.detail(oval(70.0, 44.0, 4.0, 3.0, 0.0), (40, 30, 30));
            s.line(arc(64.0, 64.0, 8.0, 200.0, 340.0), (40, 30, 30), 2.5);
        }
        Icon::Rifleman => {
            // A long rifle with a bayonet, and a kepi.
            s.body(poly(&[(8.0, 94.0), (26.0, 74.0), (36.0, 66.0), (42.0, 74.0), (20.0, 98.0)]), WOOD);
            s.body(thick(polyline(&[(32.0, 70.0), (88.0, 14.0)]), 5.0), (70, 74, 84));
            s.body(thick(polyline(&[(86.0, 16.0), (98.0, 2.0)]), 2.5), STEEL);
            s.detail(rrect(44.0, 54.0, 8.0, 5.0, 1.0), BRASS);
            s.body(poly(&[(50.0, 86.0), (54.0, 66.0), (82.0, 62.0), (90.0, 82.0)]), (64, 74, 120));
            s.body(rrect(46.0, 82.0, 48.0, 8.0, 3.0), (40, 40, 44));
            s.detail(rrect(56.0, 72.0, 28.0, 4.0, 1.0), BRASS);
        }
        Icon::Cavalry => {
            // A curved sabre over a horse's head.
            let coat = (60, 44, 34);
            let mut pb = PathBuilder::new();
            pb.move_to(28.0, 94.0);
            pb.quad_to(24.0, 60.0, 40.0, 40.0);
            pb.line_to(48.0, 20.0);
            pb.line_to(56.0, 30.0);
            pb.quad_to(76.0, 36.0, 88.0, 58.0);
            pb.quad_to(90.0, 68.0, 80.0, 70.0);
            pb.quad_to(70.0, 66.0, 60.0, 62.0);
            pb.quad_to(58.0, 80.0, 68.0, 94.0);
            pb.close();
            s.body(pb.finish().unwrap(), coat);
            s.detail(circle(62.0, 44.0, 3.5), (230, 220, 200));
            s.line_alpha(polyline(&[(58.0, 60.0), (72.0, 52.0), (84.0, 62.0)]), (200, 60, 50), 2.5, 0.9);
            s.body(thick(arc(110.0, 100.0, 96.0, 196.0, 246.0), 5.0), (214, 220, 230));
            s.body(rotated(rrect(8.0, 66.0, 22.0, 6.0, 2.0), -24.0, 19.0, 69.0), BRASS);
        }
        Icon::Artillery => {
            // A long steel barrel on a carriage with spoked wheels.
            s.body(rotated(poly(&[(10.0, 42.0), (90.0, 44.0), (90.0, 52.0), (10.0, 56.0)]), -16.0, 50.0, 50.0), (88, 96, 108));
            s.body(rotated(rrect(10.0, 40.0, 16.0, 18.0, 3.0), -16.0, 50.0, 50.0), (70, 76, 88));
            s.body(poly(&[(22.0, 60.0), (54.0, 56.0), (90.0, 90.0), (82.0, 94.0)]), (90, 104, 72));
            s.body(circle(40.0, 72.0, 20.0), (90, 104, 72));
            s.detail(circle(40.0, 72.0, 14.0), (60, 70, 50));
            for a in [0.0f32, 30.0, 60.0, 90.0, 120.0, 150.0] {
                let (c, sn) = (a.to_radians().cos() * 15.0, a.to_radians().sin() * 15.0);
                s.line(polyline(&[(40.0 - c, 72.0 - sn), (40.0 + c, 72.0 + sn)]), (130, 140, 110), 2.0);
            }
            s.detail(circle(40.0, 72.0, 4.0), STEEL);
        }
        Icon::Ironclad => {
            // A low iron hull with a funnel and a gun turret.
            s.body(rrect(46.0, 26.0, 14.0, 30.0, 2.0), (60, 60, 66));
            s.detail(rrect(46.0, 26.0, 14.0, 5.0, 1.0), (176, 64, 52));
            s.detail_alpha(circle(56.0, 18.0, 8.0), (180, 180, 186), 0.7);
            s.detail_alpha(circle(64.0, 10.0, 6.0), (180, 180, 186), 0.5);
            s.body(rrect(18.0, 46.0, 24.0, 12.0, 5.0), (96, 100, 110));
            s.body(rrect(4.0, 49.0, 18.0, 4.0, 2.0), (70, 74, 84));
            let mut pb = PathBuilder::new();
            pb.move_to(4.0, 56.0);
            pb.line_to(96.0, 56.0);
            pb.line_to(86.0, 78.0);
            pb.line_to(14.0, 78.0);
            pb.close();
            s.body(pb.finish().unwrap(), (84, 88, 98));
            s.detail(poly(&[(8.0, 62.0), (92.0, 62.0), (90.0, 66.0), (10.0, 66.0)]), (176, 64, 52));
            s.line_alpha(polyline(&[(4.0, 86.0), (96.0, 86.0)]), (96, 150, 196), 3.0, 0.9);
        }
        Icon::ShipOfTheLine => {
            // A tall ship with two rows of gunports.
            for (x, h) in [(26.0f32, 36.0f32), (52.0, 44.0), (76.0, 36.0)] {
                s.body(rrect(x - 2.0, 52.0 - h - 6.0, 4.0, h + 6.0, 2.0), DARK_WOOD);
                s.body(poly(&[(x - 12.0, 52.0 - h), (x + 12.0, 52.0 - h), (x + 14.0, 52.0 - h / 2.0 - 2.0), (x - 14.0, 52.0 - h / 2.0 - 2.0)]), CREAM);
                s.body(poly(&[(x - 14.0, 52.0 - h / 2.0 + 2.0), (x + 14.0, 52.0 - h / 2.0 + 2.0), (x + 15.0, 48.0), (x - 15.0, 48.0)]), CREAM);
            }
            s.body(galley_hull(2.0, 98.0, 54.0, 90.0), (50, 40, 34));
            for (y, c) in [(58.0f32, (214, 172, 78)), (72.0, (214, 172, 78))] {
                s.detail(poly(&[(12.0, y), (90.0, y), (89.0, y + 4.0), (13.0, y + 4.0)]), c);
            }
            for y in [63.0f32, 77.0] {
                for x in [22.0, 34.0, 46.0, 58.0, 70.0, 82.0] {
                    s.detail(rrect(x - 3.0, y, 6.0, 5.0, 1.0), (24, 18, 14));
                }
            }
        }
        Icon::Coal => {
            // Black lumps with a glint.
            for (x, y, r) in [(34.0f32, 64.0f32, 22.0f32), (66.0, 60.0, 24.0), (50.0, 38.0, 20.0)] {
                let pts: Vec<(f32, f32)> = (0..7)
                    .map(|k| {
                        let a = (k as f32 * 51.4 + x).to_radians();
                        let rr = r * (0.82 + 0.18 * ((k * 7 + x as usize) % 3) as f32 / 2.0);
                        (x + rr * a.cos(), y + rr * a.sin())
                    })
                    .collect();
                s.body(poly(&pts), (46, 46, 52));
                s.detail_alpha(poly(&[(x - 6.0, y - 10.0), (x + 2.0, y - 12.0), (x - 2.0, y - 4.0)]), (150, 156, 170), 0.8);
            }
        }
        Icon::Factory => {
            // A saw-tooth roofed mill with two smoking chimneys.
            let brick = (170, 88, 60);
            s.body(rrect(64.0, 14.0, 10.0, 46.0, 1.0), brick);
            s.body(rrect(80.0, 22.0, 9.0, 38.0, 1.0), brick);
            s.detail_alpha(circle(69.0, 8.0, 7.0), (190, 190, 196), 0.7);
            s.detail_alpha(circle(86.0, 14.0, 6.0), (190, 190, 196), 0.6);
            let mut pts = vec![(6.0, 92.0), (6.0, 58.0)];
            for k in 0..4 {
                let x = 6.0 + k as f32 * 22.0;
                pts.push((x + 22.0, 44.0));
                pts.push((x + 22.0, 58.0));
            }
            pts.push((94.0, 92.0));
            s.body(poly(&pts), brick);
            for k in 0..4 {
                let x = 6.0 + k as f32 * 22.0;
                s.detail(poly(&[(x + 20.0, 48.0), (x + 20.0, 57.0), (x + 12.0, 57.0)]), (150, 196, 220));
            }
            for x in [14.0, 36.0, 58.0, 80.0] {
                s.detail(rrect(x, 66.0, 10.0, 12.0, 1.0), (60, 40, 32));
            }
        }
        Icon::StockExchange => {
            // A rising line of prices over a column of coins.
            s.body(rrect(8.0, 10.0, 84.0, 82.0, 6.0), (226, 220, 204));
            for y in [30.0, 50.0, 70.0] {
                s.line_alpha(polyline(&[(14.0, y), (86.0, y)]), (190, 180, 160), 1.5, 0.8);
            }
            s.line(polyline(&[(14.0, 78.0), (32.0, 60.0), (46.0, 68.0), (62.0, 40.0), (74.0, 46.0), (86.0, 20.0)]), (52, 150, 70), 5.0);
            s.body(poly(&[(78.0, 16.0), (90.0, 14.0), (88.0, 26.0)]), (52, 150, 70));
            for k in 0..3 {
                s.body(oval(28.0, 84.0 - k as f32 * 8.0, 14.0, 5.0, 0.0), GOLD);
            }
        }
        Icon::PublicSchool => {
            // A slate with chalk sums and an apple.
            s.body(rrect(6.0, 14.0, 70.0, 56.0, 4.0), WOOD);
            s.detail(rrect(12.0, 20.0, 58.0, 44.0, 2.0), (52, 72, 64));
            s.line_alpha(polyline(&[(18.0, 32.0), (30.0, 32.0)]), (236, 236, 230), 2.5, 0.9);
            s.line_alpha(polyline(&[(24.0, 26.0), (24.0, 38.0)]), (236, 236, 230), 2.5, 0.9);
            s.line_alpha(polyline(&[(36.0, 30.0), (46.0, 30.0)]), (236, 236, 230), 2.5, 0.9);
            s.line_alpha(polyline(&[(36.0, 34.0), (46.0, 34.0)]), (236, 236, 230), 2.5, 0.9);
            s.line_alpha(polyline(&[(18.0, 50.0), (60.0, 50.0)]), (236, 236, 230), 2.5, 0.7);
            s.body(circle(72.0, 74.0, 16.0), (204, 56, 48));
            s.body(circle(84.0, 74.0, 14.0), (204, 56, 48));
            s.body(oval(82.0, 54.0, 8.0, 4.0, -25.0), (74, 150, 56));
            s.detail_alpha(oval(70.0, 68.0, 4.0, 3.0, -30.0), (255, 210, 200), 0.8);
        }
        Icon::Museum => {
            // A classical hall with a statue on a plinth before it.
            let marble = (226, 214, 188);
            s.body(poly(&[(6.0, 30.0), (50.0, 8.0), (94.0, 30.0)]), marble);
            s.body(rrect(10.0, 28.0, 80.0, 6.0, 1.0), (206, 196, 176));
            for x in [14.0, 30.0, 62.0, 78.0] {
                s.body(rrect(x, 36.0, 9.0, 40.0, 2.0), marble);
            }
            s.body(rrect(6.0, 76.0, 88.0, 8.0, 2.0), (190, 180, 160));
            s.body(rrect(38.0, 66.0, 24.0, 12.0, 2.0), STONE);
            s.body(circle(50.0, 40.0, 6.0), (196, 170, 120));
            s.body(poly(&[(44.0, 48.0), (56.0, 48.0), (58.0, 66.0), (42.0, 66.0)]), (196, 170, 120));
            s.body(rrect(2.0, 86.0, 96.0, 8.0, 2.0), (170, 160, 142));
        }
        Icon::MilitaryAcademy => {
            // A shako with a plume over crossed swords.
            for deg in [-40.0f32, 40.0] {
                let blade = poly(&[(47.0, 30.0), (50.0, 22.0), (53.0, 30.0), (53.0, 82.0), (47.0, 82.0)]);
                s.body(rotated(blade, deg, 50.0, 66.0), STEEL);
                s.body(rotated(rrect(38.0, 80.0, 24.0, 5.0, 2.0), deg, 50.0, 66.0), BRASS);
            }
            s.body(poly(&[(30.0, 24.0), (70.0, 24.0), (66.0, 64.0), (34.0, 64.0)]), (40, 46, 70));
            s.body(rrect(28.0, 60.0, 44.0, 8.0, 2.0), (24, 24, 28));
            s.detail(circle(50.0, 42.0, 8.0), GOLD);
            s.body(oval(50.0, 14.0, 6.0, 12.0, 0.0), (200, 48, 44));
        }
        Icon::Arsenal => {
            // Cannonballs stacked before a bombproof vault.
            s.body(rrect(8.0, 26.0, 84.0, 50.0, 3.0), (150, 136, 112));
            let mut pb = PathBuilder::new();
            pb.move_to(8.0, 30.0);
            pb.quad_to(50.0, 4.0, 92.0, 30.0);
            pb.close();
            s.body(pb.finish().unwrap(), (110, 140, 76));
            s.detail(rrect(38.0, 44.0, 24.0, 32.0, 10.0), (60, 50, 40));
            for (x, y) in [(22.0f32, 86.0f32), (36.0, 86.0), (50.0, 86.0), (64.0, 86.0), (78.0, 86.0), (29.0, 75.0), (43.0, 75.0), (57.0, 75.0), (71.0, 75.0)] {
                if (x - 50.0).abs() < 10.0 && y < 80.0 {
                    continue;
                }
                s.body(circle(x, y, 7.0), (60, 60, 66));
            }
        }
        Icon::Infantry => {
            // A bolt-action rifle behind a brimmed steel helmet.
            s.body(poly(&[(6.0, 92.0), (22.0, 76.0), (32.0, 68.0), (38.0, 76.0), (18.0, 96.0)]), WOOD);
            s.body(thick(polyline(&[(28.0, 72.0), (86.0, 14.0)]), 5.0), (70, 74, 84));
            s.body(thick(polyline(&[(84.0, 16.0), (94.0, 6.0)]), 2.5), STEEL);
            s.line(polyline(&[(40.0, 58.0), (48.0, 64.0)]), STEEL, 3.0);
            let olive = (102, 108, 72);
            s.body(oval(60.0, 74.0, 34.0, 9.0, 0.0), olive);
            let mut pb = PathBuilder::new();
            pb.move_to(36.0, 74.0);
            pb.quad_to(38.0, 44.0, 60.0, 44.0);
            pb.quad_to(82.0, 44.0, 84.0, 74.0);
            pb.close();
            s.body(pb.finish().unwrap(), olive);
            s.detail(poly(&[(38.0, 66.0), (82.0, 66.0), (83.0, 71.0), (37.0, 71.0)]), (176, 64, 52));
            s.detail_alpha(oval(52.0, 54.0, 8.0, 4.0, -20.0), (200, 206, 170), 0.6);
        }
        Icon::MachineGun => {
            // A water-cooled gun on a tripod, its belt of rounds hanging down.
            let gun = (60, 64, 70);
            s.body(thick(polyline(&[(30.0, 66.0), (14.0, 92.0)]), 4.0), gun);
            s.body(thick(polyline(&[(36.0, 66.0), (56.0, 92.0)]), 4.0), gun);
            s.body(thick(polyline(&[(34.0, 64.0), (34.0, 92.0)]), 4.0), gun);
            s.body(rrect(10.0, 42.0, 34.0, 20.0, 4.0), gun);
            s.body(rrect(40.0, 44.0, 46.0, 16.0, 6.0), (92, 98, 76));
            for x in [50.0f32, 62.0, 74.0] {
                s.detail(rrect(x, 44.0, 3.0, 16.0, 1.0), (60, 66, 50));
            }
            s.body(rrect(84.0, 49.0, 12.0, 6.0, 2.0), gun);
            s.body(thick(polyline(&[(10.0, 50.0), (2.0, 46.0)]), 4.0), WOOD);
            for k in 0..6 {
                let (x, y) = (20.0 - k as f32 * 2.0, 64.0 + k as f32 * 5.0);
                s.detail(rrect(x, y, 10.0, 3.5, 1.0), BRASS);
            }
        }
        Icon::Tank => {
            // A tank side on: tracks with road wheels, the hull, a turret and a long gun.
            let olive = (98, 106, 70);
            s.body(rrect(6.0, 66.0, 88.0, 22.0, 11.0), (54, 54, 56));
            for x in [18.0f32, 34.0, 50.0, 66.0, 82.0] {
                s.detail(circle(x, 77.0, 7.0), (120, 124, 110));
                s.detail(circle(x, 77.0, 2.5), (54, 54, 56));
            }
            s.body(poly(&[(8.0, 66.0), (16.0, 50.0), (86.0, 50.0), (96.0, 62.0), (94.0, 66.0)]), olive);
            s.body(poly(&[(28.0, 50.0), (34.0, 32.0), (66.0, 30.0), (74.0, 50.0)]), olive);
            s.body(rrect(66.0, 36.0, 32.0, 5.0, 2.0), (70, 76, 60));
            s.detail(poly(&[(30.0, 44.0), (72.0, 44.0), (73.0, 48.0), (29.0, 48.0)]), (176, 64, 52));
            s.line(polyline(&[(40.0, 30.0), (40.0, 10.0)]), (60, 60, 60), 2.0);
        }
        Icon::Destroyer => {
            // A long, low grey hull with two raked funnels and a bridge.
            let grey = (120, 128, 138);
            s.body(rrect(30.0, 38.0, 18.0, 20.0, 2.0), grey);
            s.detail(rrect(31.0, 41.0, 16.0, 4.0, 1.0), (40, 50, 62));
            for x in [52.0f32, 66.0] {
                s.body(poly(&[(x, 58.0), (x + 4.0, 34.0), (x + 12.0, 34.0), (x + 10.0, 58.0)]), grey);
                s.detail(poly(&[(x + 3.4, 38.0), (x + 12.0, 38.0), (x + 11.5, 42.0), (x + 2.8, 42.0)]), (176, 64, 52));
            }
            s.line(polyline(&[(40.0, 38.0), (42.0, 14.0)]), (70, 74, 84), 2.5);
            let mut pb = PathBuilder::new();
            pb.move_to(2.0, 56.0);
            pb.line_to(98.0, 54.0);
            pb.line_to(86.0, 70.0);
            pb.line_to(10.0, 70.0);
            pb.close();
            s.body(pb.finish().unwrap(), grey);
            s.body(rrect(10.0, 50.0, 12.0, 6.0, 2.0), grey);
            s.body(rrect(0.0, 51.0, 12.0, 3.0, 1.0), (70, 74, 84));
            s.line_alpha(polyline(&[(4.0, 80.0), (96.0, 80.0)]), (96, 150, 196), 3.0, 0.9);
        }
        Icon::Battleship => {
            // A broad hull with big gun turrets, a tower bridge and a funnel.
            let grey = (112, 120, 130);
            for (x, h) in [(36.0f32, 18.0f32), (40.0, 30.0), (43.0, 42.0)] {
                s.body(rrect(x - (48.0 - h) * 0.3, 54.0 - h, 18.0 - (h - 18.0) * 0.25, h, 2.0), grey);
            }
            s.line(polyline(&[(48.0, 12.0), (48.0, 2.0)]), (70, 74, 84), 2.5);
            s.body(rrect(58.0, 30.0, 14.0, 24.0, 4.0), grey);
            s.detail(rrect(58.0, 34.0, 14.0, 5.0, 1.0), (176, 64, 52));
            for (x, dir) in [(16.0f32, -1.0f32), (82.0, 1.0)] {
                s.body(rrect(x - 8.0, 44.0, 16.0, 10.0, 4.0), grey);
                for k in 0..2 {
                    let y = 46.0 + k as f32 * 4.0;
                    let x1 = x + dir * 24.0;
                    s.body(thick(polyline(&[(x, y), (x1, y - 2.0)]), 2.5), (70, 74, 84));
                }
            }
            s.body(galley_hull(2.0, 98.0, 56.0, 78.0), grey);
            s.detail(poly(&[(10.0, 62.0), (92.0, 62.0), (90.0, 66.0), (12.0, 66.0)]), (176, 64, 52));
            s.line_alpha(polyline(&[(4.0, 88.0), (96.0, 88.0)]), (96, 150, 196), 3.0, 0.9);
        }
        Icon::Oil => {
            // A drop of crude with a sheen, over a dark pool.
            s.body(oval(50.0, 86.0, 34.0, 8.0, 0.0), (30, 28, 32));
            let mut pb = PathBuilder::new();
            pb.move_to(50.0, 8.0);
            pb.quad_to(78.0, 46.0, 76.0, 60.0);
            pb.quad_to(74.0, 82.0, 50.0, 82.0);
            pb.quad_to(26.0, 82.0, 24.0, 60.0);
            pb.quad_to(22.0, 46.0, 50.0, 8.0);
            pb.close();
            s.body(pb.finish().unwrap(), (34, 32, 38));
            s.detail_alpha(oval(40.0, 56.0, 6.0, 12.0, 20.0), (130, 110, 170), 0.8);
            s.detail_alpha(oval(62.0, 70.0, 5.0, 3.0, -20.0), (100, 160, 150), 0.6);
        }
        Icon::OilWell => {
            // A timber derrick, oil gushing from its top.
            s.body(poly(&[(20.0, 94.0), (44.0, 18.0), (56.0, 18.0), (80.0, 94.0), (70.0, 94.0), (52.0, 30.0), (48.0, 30.0), (30.0, 94.0)]), WOOD);
            for (y, w) in [(44.0f32, 13.0f32), (62.0, 19.0), (80.0, 25.0)] {
                s.body(rrect(50.0 - w, y, w * 2.0, 4.0, 1.0), DARK_WOOD);
            }
            s.line(polyline(&[(36.0, 62.0), (60.0, 80.0)]), DARK_WOOD, 2.5);
            s.line(polyline(&[(64.0, 62.0), (40.0, 80.0)]), DARK_WOOD, 2.5);
            s.body(rrect(40.0, 12.0, 20.0, 7.0, 2.0), DARK_WOOD);
            s.body(oval(50.0, 6.0, 14.0, 6.0, 0.0), (34, 32, 38));
            s.body(oval(36.0, 14.0, 6.0, 4.0, 30.0), (34, 32, 38));
            s.body(oval(66.0, 14.0, 6.0, 4.0, -30.0), (34, 32, 38));
        }
        Icon::Hospital => {
            // A white ward building with a green cross and a pulse line.
            s.body(rrect(10.0, 30.0, 80.0, 62.0, 4.0), (236, 236, 232));
            s.body(rrect(30.0, 12.0, 40.0, 22.0, 3.0), (236, 236, 232));
            s.detail(poly(&[(44.0, 14.0), (56.0, 14.0), (56.0, 20.0), (62.0, 20.0), (62.0, 28.0), (56.0, 28.0), (56.0, 34.0), (44.0, 34.0), (44.0, 28.0), (38.0, 28.0), (38.0, 20.0), (44.0, 20.0)]), (52, 160, 90));
            for x in [18.0f32, 34.0, 58.0, 74.0] {
                s.detail(rrect(x, 42.0, 8.0, 10.0, 1.0), (120, 170, 200));
            }
            s.line(polyline(&[(16.0, 70.0), (34.0, 70.0), (40.0, 58.0), (48.0, 82.0), (54.0, 64.0), (60.0, 70.0), (84.0, 70.0)]), (52, 160, 90), 3.5);
            s.detail(rrect(42.0, 84.0, 16.0, 8.0, 1.0), (90, 110, 130));
        }
        Icon::Supermarket => {
            // A shopping trolley full of groceries.
            let wire = (150, 160, 172);
            s.body(circle(78.0, 20.0, 8.0), (204, 56, 48));
            s.body(oval(46.0, 22.0, 10.0, 14.0, -10.0), (232, 196, 70));
            s.body(rrect(56.0, 12.0, 12.0, 22.0, 3.0), (90, 150, 210));
            s.body(circle(30.0, 26.0, 9.0), (90, 160, 60));
            s.body(poly(&[(14.0, 30.0), (88.0, 30.0), (80.0, 66.0), (22.0, 66.0)]), wire);
            for x in [28.0f32, 40.0, 52.0, 64.0, 76.0] {
                s.line_alpha(polyline(&[(x - 4.0, 34.0), (x - 2.0, 62.0)]), (90, 100, 112), 2.0, 0.8);
            }
            s.line_alpha(polyline(&[(18.0, 46.0), (84.0, 46.0)]), (90, 100, 112), 2.0, 0.8);
            s.body(thick(polyline(&[(88.0, 30.0), (96.0, 14.0)]), 4.0), wire);
            s.body(thick(polyline(&[(22.0, 66.0), (20.0, 78.0), (84.0, 78.0)]), 4.0), wire);
            s.body(circle(28.0, 88.0, 6.0), (50, 50, 56));
            s.body(circle(76.0, 88.0, 6.0), (50, 50, 56));
        }
        Icon::PowerPlant => {
            // A turbine hall with tall chimneys and a lightning bolt.
            let brick = (176, 168, 150);
            s.body(rrect(56.0, 6.0, 10.0, 54.0, 1.0), brick);
            s.body(rrect(74.0, 6.0, 10.0, 54.0, 1.0), brick);
            for x in [56.0f32, 74.0] {
                s.detail(rrect(x, 12.0, 10.0, 4.0, 1.0), (176, 64, 52));
            }
            s.body(rrect(8.0, 46.0, 86.0, 46.0, 3.0), brick);
            for x in [16.0f32, 30.0, 44.0, 58.0, 72.0] {
                s.detail(rrect(x, 56.0, 8.0, 18.0, 1.0), (120, 170, 200));
            }
            s.body(poly(&[(30.0, 6.0), (14.0, 34.0), (26.0, 34.0), (18.0, 56.0), (42.0, 22.0), (30.0, 22.0), (38.0, 6.0)]), (250, 210, 60));
        }
        Icon::MilitaryBase => {
            // A concrete bunker with a gun slit behind barbed wire.
            let concrete = (170, 166, 154);
            let mut pb = PathBuilder::new();
            pb.move_to(10.0, 74.0);
            pb.line_to(16.0, 34.0);
            pb.quad_to(50.0, 18.0, 84.0, 34.0);
            pb.line_to(90.0, 74.0);
            pb.close();
            s.body(pb.finish().unwrap(), concrete);
            s.detail(rrect(26.0, 44.0, 48.0, 8.0, 2.0), (34, 34, 38));
            s.detail_alpha(poly(&[(20.0, 36.0), (50.0, 26.0), (50.0, 30.0), (22.0, 40.0)]), (210, 206, 196), 0.7);
            for x in [10.0f32, 50.0, 90.0] {
                s.body(rrect(x - 2.0, 72.0, 4.0, 22.0, 1.0), DARK_WOOD);
            }
            for k in 0..8 {
                let x = 8.0 + k as f32 * 11.0;
                s.line(polyline(&[(x, 78.0), (x + 11.0, 88.0)]), (90, 90, 96), 2.0);
                s.line(polyline(&[(x, 88.0), (x + 11.0, 78.0)]), (90, 90, 96), 2.0);
            }
            s.line(polyline(&[(8.0, 83.0), (92.0, 83.0)]), (90, 90, 96), 2.0);
        }
        Icon::BroadcastTower => {
            // A lattice mast sending out radio waves.
            let steel = (176, 64, 52);
            s.body(poly(&[(30.0, 94.0), (47.0, 20.0), (53.0, 20.0), (70.0, 94.0), (62.0, 94.0), (50.0, 36.0), (38.0, 94.0)]), steel);
            for (y, w) in [(48.0f32, 9.0f32), (66.0, 13.0), (84.0, 17.0)] {
                s.body(rrect(50.0 - w, y, w * 2.0, 3.0, 1.0), (220, 220, 220));
            }
            s.body(circle(50.0, 16.0, 6.0), (250, 210, 60));
            for r in [16.0f32, 28.0] {
                s.line(arc(50.0, 16.0, r, -40.0, 40.0), (96, 150, 210), 4.0);
                s.line(arc(50.0, 16.0, r, 140.0, 220.0), (96, 150, 210), 4.0);
            }
        }
        Icon::ResearchLab => {
            // A conical flask with a bubbling liquid, and a test tube.
            let glass = (214, 232, 240);
            s.body(poly(&[(36.0, 10.0), (56.0, 10.0), (56.0, 38.0), (80.0, 86.0), (12.0, 86.0), (36.0, 38.0)]), glass);
            s.detail(poly(&[(25.0, 60.0), (67.0, 60.0), (78.0, 84.0), (14.0, 84.0)]), (70, 190, 120));
            s.detail_alpha(circle(40.0, 50.0, 4.0), (70, 190, 120), 0.8);
            s.detail_alpha(circle(50.0, 40.0, 3.0), (70, 190, 120), 0.7);
            s.detail_alpha(circle(44.0, 28.0, 2.5), (70, 190, 120), 0.6);
            s.body(rrect(32.0, 6.0, 28.0, 6.0, 2.0), glass);
            s.body(rotated(rrect(76.0, 20.0, 12.0, 54.0, 6.0), 14.0, 82.0, 46.0), glass);
            s.detail(rotated(rrect(77.0, 48.0, 10.0, 24.0, 5.0), 14.0, 82.0, 46.0), (110, 120, 220));
        }
        Icon::Trade => {
            // Two arrows chasing each other round a gold coin.
            let blue = (96, 150, 210);
            s.body(thick(arc(50.0, 50.0, 36.0, 200.0, 330.0), 9.0), blue);
            s.body(poly(&[(84.0, 32.0), (96.0, 56.0), (72.0, 52.0)]), blue);
            s.body(thick(arc(50.0, 50.0, 36.0, 20.0, 150.0), 9.0), blue);
            s.body(poly(&[(16.0, 68.0), (4.0, 44.0), (28.0, 48.0)]), blue);
            s.body(circle(50.0, 50.0, 18.0), GOLD);
            s.line(circle(50.0, 50.0, 12.0), (186, 132, 26), 2.5);
        }
        Icon::Merchant => {
            // A pack mule laden with bundles, side on.
            let coat = (128, 104, 84);
            s.body(rrect(22.0, 42.0, 54.0, 26.0, 12.0), coat);
            for x in [26.0f32, 38.0, 60.0, 70.0] {
                s.body(rrect(x, 60.0, 7.0, 32.0, 3.0), coat);
            }
            s.body(poly(&[(70.0, 48.0), (82.0, 24.0), (96.0, 34.0), (92.0, 42.0), (80.0, 56.0)]), coat);
            s.body(poly(&[(82.0, 26.0), (80.0, 12.0), (88.0, 22.0)]), coat);
            s.detail(circle(87.0, 32.0, 2.5), (30, 22, 16));
            s.body(rrect(24.0, 22.0, 24.0, 26.0, 5.0), (186, 74, 52));
            s.body(rrect(46.0, 26.0, 22.0, 22.0, 5.0), (214, 170, 82));
            s.detail(rrect(34.0, 14.0, 22.0, 12.0, 4.0), CREAM);
            s.line_alpha(polyline(&[(36.0, 22.0), (36.0, 48.0)]), (90, 60, 40), 2.5, 0.9);
            s.line_alpha(polyline(&[(57.0, 26.0), (57.0, 48.0)]), (90, 60, 40), 2.5, 0.9);
            s.line(polyline(&[(22.0, 48.0), (12.0, 70.0)]), DARK_WOOD, 3.0);
        }
        Icon::Upgrade => {
            // Two gold chevrons pointing up.
            for y in [12.0f32, 44.0] {
                s.body(poly(&[(50.0, y), (88.0, y + 34.0), (74.0, y + 46.0), (50.0, y + 24.0), (26.0, y + 46.0), (12.0, y + 34.0)]), GOLD);
            }
        }
        Icon::Random => {
            s.body(rrect(22.0, 22.0, 56.0, 56.0, 10.0), CREAM);
            for (x, y) in [(36.0, 36.0), (64.0, 36.0), (50.0, 50.0), (36.0, 64.0), (64.0, 64.0)] {
                s.detail(circle(x, y, 5.0), (90, 76, 60));
            }
            s.rotate(15.0);
        }
    }
}

fn shield_path(scale: f32) -> Path {
    let mut pb = PathBuilder::new();
    pb.move_to(50.0, 8.0);
    pb.quad_to(68.0, 18.0, 88.0, 16.0);
    pb.line_to(88.0, 46.0);
    pb.quad_to(86.0, 78.0, 50.0, 94.0);
    pb.quad_to(14.0, 78.0, 12.0, 46.0);
    pb.line_to(12.0, 16.0);
    pb.quad_to(32.0, 18.0, 50.0, 8.0);
    pb.close();
    let p = pb.finish().unwrap();
    let t = Transform::from_translate(50.0, 51.0).pre_scale(scale, scale).pre_translate(-50.0, -51.0);
    p.transform(t).unwrap()
}
