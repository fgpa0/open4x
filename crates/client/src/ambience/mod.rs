//! Life and atmosphere around the camera, visual only (nothing
//! here touches the game): chimney smoke and lamps in towns, mist at dawn,
//! leaves in autumn, drifting snow, butterflies and fireflies, dust devils,
//! leaping fish and whales, fishing boats, ox carts on the roads, grazing
//! animals and rainbows after rain. Particles are `puffs`; the rest are
//! models moved here. Setting "Wildlife and ambience" turns it off.

pub mod puffs;
mod herds;
mod nature;
mod rainbow;
mod roads;
mod sea;
mod towns;

pub use herds::{herd_model, HerdAnimal};

use crate::camera::CameraRig;
use crate::layout::hex_at;
use crate::loading::InGameSet;
use crate::season::Season;
use crate::terrain::{height::WATER_LEVEL, Terrain};
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sim::game::Game;
use sim::hex::TileId;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmbienceSet {
    /// Works out what the camera sees.
    Stage,
    /// Emitters and models.
    Emit,
    /// The particle mesh.
    Draw,
}

pub struct AmbiencePlugin;

impl Plugin for AmbiencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stage>()
            .insert_resource(Dice(ChaCha8Rng::seed_from_u64(seed())))
            .configure_sets(Update, (AmbienceSet::Stage, AmbienceSet::Emit, AmbienceSet::Draw).chain().after(crate::play::PlaySet))
            .add_systems(Update, stage.in_set(InGameSet).in_set(AmbienceSet::Stage));
        puffs::plugin(app);
        towns::plugin(app);
        nature::plugin(app);
        sea::plugin(app);
        roads::plugin(app);
        herds::plugin(app);
        rainbow::plugin(app);
    }
}

fn seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64)
}

/// The random numbers for everything ambient (not the game's).
#[derive(Resource)]
pub struct Dice(pub ChaCha8Rng);

/// What the camera looks at, for emitters.
#[derive(Resource, Default, Clone, Copy)]
pub struct Stage {
    /// Ambient sights are on (the setting, and not in the strategic view).
    pub on: bool,
    pub focus: Vec2,
    pub distance: f32,
    /// The ground the camera sees (roughly).
    pub min: Vec2,
    pub max: Vec2,
    /// Time of day (0..1, see `daylight`), and how dark it is (0..1).
    pub day: f32,
    pub night: f32,
    pub dt: f32,
    /// Weather where the camera looks.
    pub cover: f32,
    pub storm: f32,
}

impl Stage {
    /// A random point on the ground in view.
    pub fn point(&self, rng: &mut impl Rng) -> Vec2 {
        Vec2::new(rng.gen_range(self.min.x..self.max.x), rng.gen_range(self.min.y..self.max.y))
    }

    pub fn sees(&self, p: Vec2, margin: f32) -> bool {
        p.x > self.min.x - margin && p.x < self.max.x + margin && p.y > self.min.y - margin && p.y < self.max.y + margin
    }

    pub fn area(&self) -> f32 {
        (self.max - self.min).x * (self.max - self.min).y
    }

    /// How many to start this frame at `rate` a second.
    pub fn spawns(&self, rate: f32, rng: &mut impl Rng) -> usize {
        let n = rate * self.dt;
        n.floor() as usize + usize::from(rng.gen_bool(n.fract().clamp(0.0, 1.0) as f64))
    }

    /// 1 up close fading to 0 between `near` and `far` camera distances.
    pub fn close(&self, near: f32, far: f32) -> f32 {
        1.0 - smooth((self.distance - near) / (far - near))
    }

    /// How much bigger small things are drawn when zoomed out, so they
    /// stay visible.
    pub fn grow(&self) -> f32 {
        (self.distance / 14.0).max(1.0).sqrt()
    }
}

fn stage(
    time: Res<Time>,
    rig: Res<CameraRig>,
    clock: Res<crate::daylight::DayClock>,
    weather: Res<crate::weather::LocalWeather>,
    settings: Res<crate::settings::Settings>,
    view: Res<crate::strategic::StrategicView>,
    mut stage: ResMut<Stage>,
) {
    let d = rig.current_distance;
    let focus = rig.focus;
    // The view's footprint on the ground for the camera's pitch and field of
    // view (wider at the top of the screen, which is further away).
    *stage = Stage {
        on: settings.ambience && !view.0,
        focus,
        distance: d,
        min: focus + Vec2::new(-0.62 * d, -0.42 * d),
        max: focus + Vec2::new(0.62 * d, 0.3 * d),
        day: clock.t,
        night: crate::daylight::night(clock.t),
        dt: time.delta_secs().min(0.1),
        cover: weather.cover,
        storm: weather.storm,
    };
}

/// What is at a point on the map, as far as the player knows.
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub tile: TileId,
    pub p: Vec2,
    pub ground: f32,
    pub water: bool,
    pub deep: bool,
    pub frozen: bool,
    pub river: bool,
    /// The terrain's and feature's art names.
    pub terrain: &'static str,
    pub feature: Option<&'static str>,
    pub moisture: f32,
    /// Temperature now (°C) and the yearly mean.
    pub temp: f32,
    pub mean: f32,
    pub lat: f32,
    /// In sight of the player now.
    pub visible: bool,
}

impl Spot {
    pub fn forest(&self) -> bool {
        matches!(self.feature, Some("forest" | "jungle"))
    }

    /// Open land where flowers grow.
    pub fn meadow(&self) -> bool {
        !self.water && matches!(self.terrain, "grassland" | "plains") && !self.forest()
    }

    /// Autumn where the spot is (the north cools while the south warms).
    pub fn autumn(&self, season: &Season) -> bool {
        let cooling = season.trend() * self.lat.signum() < 0.0;
        cooling && self.temp > 0.0 && self.temp < 13.0
    }
}

/// What is at `p`, if the player has explored it.
pub fn spot(game: &Game, season: &Season, terrain: &Terrain, me: sim::game::PlayerId, p: Vec2) -> Option<Spot> {
    let tile = game.map.grid.tile(hex_at(p))?;
    let player = &game.players[me as usize];
    if !player.explored[tile as usize] {
        return None;
    }
    let t = game.map.tile(tile);
    let def = game.rules.terrain(t.terrain);
    let (temp, mean, lat) = crate::season::temperature_now(game, tile, season);
    // Strings in the rules live as long as the game's rules; art names are
    // compared, never kept, so leaking them once per name is fine.
    Some(Spot {
        tile,
        p,
        ground: terrain.heights.sample(p).max(WATER_LEVEL),
        water: def.water,
        deep: def.deep,
        frozen: def.water && game.sea_ice(tile) != sim::climate::SeaIce::Open,
        river: t.rivers != 0,
        terrain: art(&def.art),
        feature: t.feature.map(|f| art(&game.rules.feature(f).art)),
        moisture: t.moisture,
        temp,
        mean,
        lat,
        visible: player.sees(tile),
    })
}

/// Interns an art name (there are a dozen or so).
fn art(name: &str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let mut names = NAMES.get_or_init(Default::default).lock().unwrap();
    if let Some(n) = names.iter().find(|n| **n == name) {
        return n;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.push(leaked);
    leaked
}

pub fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A prop from the Blender libraries as a mesh for the plants' material
/// (`vegetation::Scenery`), marked as a bare surface for snow.
pub fn scenery_prop(name: &str) -> Mesh {
    let mut m = crate::models::prop(name).clone();
    let bare = crate::vegetation::snow_tag(crate::vegetation::Surface::Bare, 0.5);
    m.map_alpha_from(0, |_| bare);
    m.into_mesh()
}
