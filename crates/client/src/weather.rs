//! Weather by climate, visual only (the sim never sees it): fronts of cloud
//! drift across the map with the wind, it rains where it's temperate and
//! snows where it's cold, and deserts stay mostly clear. The thickest fronts
//! hold storms: thunderstorms with lightning where it rains, blizzards where
//! it snows (gales of snow, a white-out haze, snow piling up twice as fast).
//!
//! A low-resolution weather texture over the map (the same area as the fog of
//! war) is updated on the CPU a few times a second:
//!  - R: cloud cover up to 0.5, storm above (a storm is always overcast),
//!  - G: what falls (0.5 nothing, above that rain, below it snow),
//!  - B: snow lying on the ground (builds up while it snows, melts when warm),
//!  - A: wet ground (soaks quickly in rain, dries slowly).
//!
//! The terrain, water and vegetation shaders sample it (their shared weather
//! block) for darker ground under storms, wet ground and settled snow. Rain and
//! snow fall near the camera only: one mesh of quads that the `precip` shader
//! moves, wraps around a box at the camera and thins out by the weather, so
//! there is no work per particle on the CPU. Zoomed out they fade away.
//!
//! `FOURX_WEATHER=rain|snow|storm|blizzard|clear` forces the weather around
//! the camera (`snow:0.3` or `blizzard:0.3` with that depth of snow lying);
//! `FOURX_WEATHER_SHIFT=x` centres it x units east of where the camera looks,
//! to see its edge.

use crate::camera::CameraRig;
use crate::layout::{hex_at, map_bounds};
use crate::loading::{AppState, InGameSet};
use crate::settings::Settings;
use crate::wrapnoise::Cylinder;
use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use noise::{Fbm, MultiFractal, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;

/// Weather texture samples per world unit (fronts are much bigger than tiles).
const RESOLUTION: f32 = 1.0;
/// The wind in world units per second; the shaders' `WEATHER_WIND` matches.
pub const WIND: Vec2 = Vec2::new(0.35, 0.12);
const UPDATE_EVERY: f32 = 0.25;
/// Weather simulated when a game starts, so snow and wet ground already lie
/// where the weather has been.
const SPIN_UP: f32 = 150.0;
/// Seconds of full snowfall to cover the ground, and of rain to soak it.
const SNOW_BUILD: f32 = 40.0;
const SOAK: f32 = 8.0;
/// Seconds for wet ground to dry under a clear sky.
const DRY: f32 = 70.0;
/// Particles of each kind (rain and snow); ordinary weather uses about half,
/// storms all of them.
const PARTICLES: usize = 12000;
/// Chances per second of lightning somewhere in view (each strikes only if
/// the place it picks is in a thunderstorm).
const LIGHTNING_TRIES: f32 = 0.8;
/// Camera distances over which rain and snow fade out.
const PRECIP_FADE: (f32, f32) = (30.0, 45.0);

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/precip.wesl");
        app.add_plugins(MaterialPlugin::<PrecipMaterial>::default())
            .init_resource::<LocalWeather>()
            .add_systems(Startup, create)
            .add_systems(OnEnter(AppState::InGame), start)
            .add_systems(Update, (simulate, lightning, follow_camera).chain().in_set(InGameSet));
    }
}

/// The weather texture (shared by materials; updated in place).
#[derive(Resource)]
pub struct WeatherMap {
    pub image: Handle<Image>,
}

/// The weather where the camera looks, eased over a few seconds: the sun dims
/// under an overcast sky, storms darken it and blizzards close in the haze
/// (see daylight.rs and settings.rs).
#[derive(Resource, Default)]
pub struct LocalWeather {
    pub cover: f32,
    pub storm: f32,
    /// How much of that storm is a blizzard (0..1).
    pub blizzard: f32,
    /// Lightning lighting up the sky now (0..1, flickering).
    pub flash: f32,
    /// Rain and snow falling where the camera looks (0..1).
    pub rain: f32,
    pub snow: f32,
    /// Strikes so far, and where and how strong the latest was (for thunder).
    pub strikes: u32,
    pub strike: (Vec2, f32),
}

#[derive(Clone, Copy, PartialEq)]
enum Forced {
    Rain,
    /// With the depth of snow lying (0..1).
    Snow(f32),
    Storm,
    Blizzard(f32),
    Clear,
}

/// The weather simulation: climate per texel and the ground's memory.
#[derive(Resource)]
struct Sky {
    width: usize,
    height: usize,
    min: Vec2,
    texel: Vec2,
    /// Mean sea-level temperature and seasonal swing (°C) and moisture
    /// (0..1) per texel, smoothed.
    temperature: Vec<f32>,
    swing: Vec<f32>,
    moisture: Vec<f32>,
    /// The season now (see `Season::now`).
    season: f32,
    snow: Vec<f32>,
    wet: Vec<f32>,
    /// The texture's texels as last stepped.
    texels: Vec<[f32; 4]>,
    /// Seconds of weather so far (starts at a seed-dependent point).
    time: f32,
    since_update: f32,
    noise: Fbm<Perlin>,
    cylinder: Cylinder,
    forced: Option<(Forced, Vec2)>,
}

impl Sky {
    fn new(game: &sim::game::Game, forced: Option<(Forced, Vec2)>) -> Self {
        let grid = &game.map.grid;
        let (min, max) = map_bounds(grid);
        let width = ((max.x - min.x) * RESOLUTION).round() as usize;
        let height = ((max.y - min.y) * RESOLUTION).round() as usize;
        let texel = (max - min) / Vec2::new(width as f32, height as f32);
        let mut temperature = vec![0.0; width * height];
        let mut land = vec![0.0; width * height];
        let mut moisture = vec![0.0; width * height];
        for y in 0..height {
            for x in 0..width {
                let p = min + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * texel;
                let i = y * width + x;
                if let Some(t) = grid.tile(hex_at(p)) {
                    let tile = game.map.tile(t);
                    let water = game.map.is_water(&game.rules, t);
                    temperature[i] = crate::season::sea_level_temperature(tile);
                    land[i] = if water { 0.0 } else { 1.0 };
                    // The sea is as moist as a temperate forest.
                    moisture[i] = if water { 0.6 } else { tile.moisture };
                }
            }
        }
        let temperature = blur(&temperature, width, height, 3);
        let land = blur(&land, width, height, 3);
        let swing = (0..width * height)
            .map(|i| {
                let z = min.y + ((i / width) as f32 + 0.5) * texel.y;
                crate::season::swing(crate::season::latitude(z, grid.height), land[i])
            })
            .collect();
        let moisture = blur(&moisture, width, height, 3);
        let seed = game.map.seed;
        Sky {
            width,
            height,
            min,
            texel,
            temperature,
            swing,
            moisture,
            season: 0.0,
            snow: vec![0.0; width * height],
            wet: vec![0.0; width * height],
            texels: Vec::new(),
            time: (seed % 997) as f32 * 41.0,
            since_update: UPDATE_EVERY,
            noise: Fbm::<Perlin>::new(seed as u32 ^ 0x5eed).set_octaves(4),
            cylinder: Cylinder::new(max.x - min.x),
            forced,
        }
    }

    /// Cloud cover (0..1) and what falls (+rain, -snow) at texel `i` now,
    /// the temperature that decides rain or snow, how much forced weather
    /// there is (0..1) and how stormy it is (0..1).
    fn sky_at(&self, i: usize) -> (f32, f32, f32, f32, f32) {
        let (x, y) = (i % self.width, i / self.width);
        let p = self.min + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * self.texel;
        let mut temperature = self.temperature[i] + self.swing[i] * self.season;
        let moisture = self.moisture[i];
        // Fronts: big noise blobs drifting with the wind, slowly changing shape.
        let q = p - WIND * self.time;
        let warp = self.cylinder.vec2(&self.noise, q, 0.015) * 14.0;
        let n = self.cylinder.get(&self.noise, q + warp, 0.03, self.time as f64 * 0.003) * 0.5 + 0.5;
        // Moist regions cloud over more easily; deserts almost never.
        let threshold = 0.62 - 0.2 * moisture;
        let mut cover = smooth(threshold, threshold + 0.1, n);
        let mut amount = smooth(threshold + 0.06, threshold + 0.16, n) * smooth(0.12, 0.45, moisture);
        // Storms: smaller cells inside the thickest fronts, quicker to change,
        // in moist air only.
        let core = smooth(threshold + 0.08, threshold + 0.16, n) * smooth(0.2, 0.45, moisture);
        let mut storm = if core > 0.0 {
            let cells = self.cylinder.get(&self.noise, q + Vec2::new(311.0, -97.0), 0.06, self.time as f64 * 0.012 + 50.0) * 0.5 + 0.5;
            core * smooth(0.46, 0.56, cells)
        } else {
            0.0
        };
        let mut near = 0.0;
        if let Some((kind, centre)) = self.forced {
            let d = (p - centre).abs();
            let d = Vec2::new(d.x.min(self.cylinder.period as f32 - d.x), d.y).length();
            near = 1.0 - smooth(14.0, 22.0, d);
            match kind {
                Forced::Clear => (cover, amount) = (0.0, 0.0),
                Forced::Rain => {
                    (cover, amount) = (cover.max(near), amount.max(near));
                    temperature = temperature.max(12.0 * near);
                }
                Forced::Snow(_) => {
                    (cover, amount) = (cover.max(near), amount.max(near));
                    temperature = temperature.min(10.0 - 16.0 * near);
                }
                Forced::Storm => {
                    (cover, amount, storm) = (cover.max(near), amount.max(near), storm.max(near));
                    temperature = temperature.max(16.0 * near);
                }
                Forced::Blizzard(_) => {
                    (cover, amount, storm) = (cover.max(near), amount.max(near), storm.max(near));
                    temperature = temperature.min(10.0 - 20.0 * near);
                }
            }
        }
        let snow = 1.0 - smooth(-2.0, 3.0, temperature);
        (cover, amount.max(storm) * (1.0 - 2.0 * snow), temperature, near, storm)
    }

    /// Advances the weather by `dt` and updates the texture's texels.
    fn step(&mut self, dt: f32) {
        self.time += dt;
        let kind = self.forced.map(|(kind, _)| kind);
        let sky: Vec<_> = (0..self.width * self.height).into_par_iter().map(|i| self.sky_at(i)).collect();
        self.texels = self
            .snow
            .par_iter_mut()
            .zip(self.wet.par_iter_mut())
            .zip(sky.par_iter())
            .map(|((snow, wet), &(cover, fall, temperature, forced, storm))| {
                let (rain, falling_snow) = (fall.max(0.0), (-fall).max(0.0));
                // Snow builds up while it snows (twice as fast in a blizzard)
                // and melts faster the warmer it is (and in rain); in the cold
                // it barely melts at all.
                let melt = ((temperature + 1.0) / 6.0).clamp(0.02, 1.0) / 60.0 + rain / 20.0;
                let build = falling_snow * (1.0 + storm) * dt / SNOW_BUILD;
                *snow = (*snow + build - melt * (1.0 - falling_snow) * dt).clamp(0.0, 1.0);
                *wet = (*wet + rain * (1.0 + storm) * dt / SOAK - (1.0 - rain) * (1.0 - cover * 0.6) * dt / DRY).clamp(0.0, 1.0);
                // Forced weather shows its ground at once.
                match kind {
                    Some(Forced::Rain | Forced::Storm) => *wet = wet.max(forced),
                    Some(Forced::Snow(depth) | Forced::Blizzard(depth)) => *snow += (depth - *snow) * forced,
                    _ => {}
                }
                let sky = if storm > 0.0 { 0.5 + 0.5 * storm } else { 0.5 * cover };
                [sky, 0.5 + fall * 0.5, *snow, *wet]
            })
            .collect();
    }

    /// The texel under a world point.
    fn texel_at(&self, p: Vec2) -> Option<[f32; 4]> {
        let c = ((p - self.min) / self.texel).floor();
        let y = c.y as i32;
        if self.texels.is_empty() || y < 0 || y >= self.height as i32 {
            return None;
        }
        let x = (c.x as i32).rem_euclid(self.width as i32) as usize;
        Some(self.texels[y as usize * self.width + x])
    }

    /// Cloud cover, storm and how much of what falls is snow around a world
    /// point (a few texels averaged).
    fn weather_near(&self, p: Vec2) -> (f32, f32, f32, f32) {
        let (mut cover, mut storm, mut rain, mut snow) = (0.0, 0.0, 0.0, 0.0);
        for dy in -2..=2 {
            for dx in -2..=2 {
                let Some([sky, fall, ..]) = self.texel_at(p + Vec2::new(dx as f32, dy as f32) * self.texel) else { continue };
                cover += (sky * 2.0).min(1.0) / 25.0;
                storm += (sky * 2.0 - 1.0).max(0.0) / 25.0;
                rain += (fall * 2.0 - 1.0).max(0.0);
                snow += (1.0 - fall * 2.0).max(0.0);
            }
        }
        (cover, storm, if rain + snow > 0.01 { snow / (rain + snow) } else { 0.0 }, ((rain + snow) / 25.0).min(1.0))
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Box blur, wrapping east–west.
fn blur(v: &[f32], width: usize, height: usize, radius: i32) -> Vec<f32> {
    let mut out = vec![0.0; v.len()];
    for y in 0..height {
        for x in 0..width {
            let (mut sum, mut n) = (0.0, 0.0);
            for dy in -radius..=radius {
                let yy = y as i32 + dy;
                if yy < 0 || yy >= height as i32 {
                    continue;
                }
                for dx in -radius..=radius {
                    let xx = (x as i32 + dx).rem_euclid(width as i32) as usize;
                    sum += v[yy as usize * width + xx];
                    n += 1.0;
                }
            }
            out[y * width + x] = sum / n;
        }
    }
    out
}

fn sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..Default::default()
    })
}

fn image(width: u32, height: u32, data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = sampler();
    image
}

/// How bright lightning is `age` seconds after it strikes: a stroke, a
/// return stroke and a fading flicker.
fn flash(age: f32) -> f32 {
    let pulse = |start: f32, length: f32, strength: f32| if age >= start { strength * (-(age - start) / length).exp() } else { 0.0 };
    pulse(0.0, 0.05, 1.0) + pulse(0.11, 0.05, 0.8) + pulse(0.24, 0.1, 0.45)
}

/// Clear skies: no cloud, nothing falling, dry ground.
const CLEAR: [u8; 4] = [0, 128, 0, 0];

fn create(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(WeatherMap { image: images.add(image(1, 1, CLEAR.to_vec())) });
}

#[derive(Component)]
struct Precipitation;

/// The lightning bolt (one at a time), drawn as glowing ribbons.
#[derive(Component)]
struct Bolt;

/// Lightning: when the last bolt struck and how bright its flash is.
#[derive(Resource)]
struct Lightning {
    rng: ChaCha8Rng,
    age: f32,
    strength: f32,
    material: Handle<StandardMaterial>,
}

fn start(
    mut commands: Commands,
    session: Res<crate::play::Session>,
    rig: Res<CameraRig>,
    map: Res<WeatherMap>,
    fog: Res<crate::fog::FogOfWar>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PrecipMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut local: ResMut<LocalWeather>,
    season: Res<crate::season::Season>,
) {
    let spec = std::env::var("FOURX_WEATHER").unwrap_or_default();
    let (kind, depth) = spec.split_once(':').unwrap_or((&spec, "1"));
    let forced = match kind {
        "rain" => Some(Forced::Rain),
        "snow" => Some(Forced::Snow(depth.parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0))),
        "storm" => Some(Forced::Storm),
        "blizzard" => Some(Forced::Blizzard(depth.parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0))),
        "clear" => Some(Forced::Clear),
        _ => None,
    };
    let mut sky = Sky::new(&session.game, forced.map(|f| (f, rig.focus + forced_shift())));
    sky.season = season.now();
    for _ in 0..(SPIN_UP / 3.0) as usize {
        sky.step(3.0);
    }
    *local = LocalWeather::default();
    commands.insert_resource(sky);
    let material = standard.add(StandardMaterial {
        base_color: Color::BLACK,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(Lightning {
        rng: ChaCha8Rng::seed_from_u64(session.game.map.seed ^ 0xb017),
        age: f32::INFINITY,
        strength: 0.0,
        material: material.clone(),
    });
    commands.spawn((
        Bolt,
        Mesh3d::default(),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Visibility::Hidden,
        NotShadowCaster,
        bevy::camera::visibility::NoFrustumCulling,
    ));
    commands.spawn((
        Precipitation,
        Mesh3d(meshes.add(precip_mesh(session.game.map.seed))),
        MeshMaterial3d(materials.add(PrecipMaterial {
            params: PrecipParams { area: crate::fog::params(&session.game.map.grid), ..default() },
            weather: map.image.clone(),
            fog: fog.image.clone(),
        })),
        Transform::IDENTITY,
        NotShadowCaster,
        bevy::camera::visibility::NoFrustumCulling,
    ));
}

/// Steps the weather and uploads it.
fn simulate(
    time: Res<Time>,
    settings: Res<Settings>,
    rig: Res<CameraRig>,
    season: Res<crate::season::Season>,
    sky: Option<ResMut<Sky>>,
    map: Res<WeatherMap>,
    mut images: ResMut<Assets<Image>>,
    mut local: ResMut<LocalWeather>,
    mut near: Local<(f32, f32, f32, f32)>,
    mut was_on: Local<bool>,
) {
    let Some(mut sky) = sky else { return };
    let dt = time.delta_secs().min(0.1);
    let ease = (dt / 3.0).min(1.0);
    let (cover, storm, snow, fall) = *near;
    local.rain += (fall * (1.0 - snow) - local.rain) * ease;
    local.snow += (fall * snow - local.snow) * ease;
    local.cover += (cover - local.cover) * ease;
    local.storm += (storm - local.storm) * ease;
    local.blizzard += (storm * smooth(0.3, 0.7, snow) - local.blizzard) * ease;
    sky.since_update += dt;
    if sky.since_update < UPDATE_EVERY && *was_on == settings.weather {
        return;
    }
    let step = sky.since_update;
    sky.since_update = 0.0;
    sky.season = season.now();
    // Forced weather stays where the camera looks.
    if let Some((_, centre)) = &mut sky.forced {
        *centre = rig.focus + forced_shift();
    }
    let (width, height) = (sky.width as u32, sky.height as u32);
    let data: Vec<u8> = if settings.weather {
        sky.step(step);
        *near = sky.weather_near(rig.focus);
        sky.texels.iter().flatten().map(|v| (v * 255.0).round() as u8).collect()
    } else {
        *near = (0.0, 0.0, 0.0, 0.0);
        CLEAR.repeat((width * height) as usize)
    };
    *was_on = settings.weather;
    let Some(mut img) = images.get_mut(&map.image) else { return };
    if img.width() == width && img.height() == height {
        img.data = Some(data);
    } else {
        *img = image(width, height, data);
    }
}

/// Strikes lightning now and then in the thunderstorms in view (not in
/// blizzards, and never over unexplored land), and lets it flicker out.
fn lightning(
    time: Res<Time>,
    settings: Res<Settings>,
    rig: Res<CameraRig>,
    sky: Option<Res<Sky>>,
    lightning: Option<ResMut<Lightning>>,
    session: Res<crate::play::Session>,
    terrain: Res<crate::terrain::Terrain>,
    mut local: ResMut<LocalWeather>,
    mut bolt: Query<(&mut Mesh3d, &mut Transform, &mut Visibility), With<Bolt>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(sky), Some(mut lightning)) = (sky, lightning) else { return };
    let Ok((mut mesh, mut transform, mut visibility)) = bolt.single_mut() else { return };
    let dt = time.delta_secs().min(0.1);
    lightning.age += dt;
    let d = rig.current_distance;
    if settings.weather && lightning.age > 1.0 && lightning.rng.gen::<f32>() < LIGHTNING_TRIES * dt {
        // Somewhere in view (the camera looks north, so more lies beyond the
        // focus than before it).
        let r = &mut lightning.rng;
        let p = rig.focus + Vec2::new((r.gen::<f32>() - 0.5) * d * 1.6, (r.gen::<f32>() - 0.6) * d * 1.1);
        let game = &session.game;
        let explored = game.map.grid.tile(hex_at(p)).is_some_and(|t| game.players[session.me as usize].explored[t as usize]);
        if let Some([s, fall, ..]) = sky.texel_at(p) {
            let thunder = (s * 2.0 - 1.0).max(0.0) * smooth(0.55, 0.65, fall);
            if explored && r.gen::<f32>() < thunder {
                let ground = terrain.heights.sample(p).max(crate::terrain::height::WATER_LEVEL);
                let seed = r.gen();
                mesh.0 = meshes.add(bolt_mesh(seed, (0.24 * d).clamp(2.5, 8.0), (d / 14.0).sqrt()));
                transform.translation = Vec3::new(p.x, ground, p.y);
                lightning.age = 0.0;
                // A strike near the focus lights the whole view; far away
                // ones, seen zoomed out, light it less.
                let away = (p - rig.focus).length() / d;
                lightning.strength = thunder * (1.0 - 0.5 * smooth(0.2, 0.8, away));
                local.strikes += 1;
                local.strike = (p, lightning.strength);
            }
        }
    }
    let f = flash(lightning.age) * lightning.strength;
    local.flash = if settings.weather { f } else { 0.0 };
    let show = settings.weather && f > 0.01;
    visibility.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    if show {
        if let Some(mut m) = materials.get_mut(&lightning.material) {
            m.base_color = (LinearRgba::rgb(6.0, 7.0, 10.0) * f.min(1.0)).into();
        }
    }
}

/// A forked lightning bolt `height` tall, as ribbons in the plane facing
/// the camera (which looks north): a main channel down to the ground, a few
/// fading branches and a faint glow around it. Vertex colours fade it into
/// the cloud at the top.
fn bolt_mesh(seed: u64, height: f32, width: f32) -> Mesh {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    // A strip along `points`, `w` wide, bright down the middle and fading to
    // `edge` (0..1 of that) at its sides.
    let mut ribbon = |points: &[Vec2], w: f32, brightness: f32, edge: f32| {
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let side = (b - a).perp().normalize_or_zero() * w * 0.5;
            let base = positions.len() as u32;
            for (p, y, shade) in [(a - side, a.y, edge), (a, a.y, 1.0), (a + side, a.y, edge), (b - side, b.y, edge), (b, b.y, 1.0), (b + side, b.y, edge)] {
                positions.push([p.x, p.y, 0.0]);
                // Faint at the top, where it leaves the cloud.
                let v = brightness * shade * smooth(height, height * 0.75, y);
                colors.push([v, v, v, 1.0]);
            }
            indices.extend([base, base + 1, base + 4, base, base + 4, base + 3, base + 1, base + 2, base + 5, base + 1, base + 5, base + 4]);
        }
    };
    // The main channel: a zigzag down, wandering sideways.
    let steps = 22;
    let mut main = vec![Vec2::new(0.0, height)];
    let mut x = 0.0;
    for i in 1..=steps {
        let y = height * (1.0 - i as f32 / steps as f32);
        x += (rng.gen::<f32>() - 0.5) * height * 0.07;
        x *= 0.9;
        main.push(Vec2::new(if i == steps { 0.0 } else { x }, y));
    }
    ribbon(&main, 0.035 * width, 1.0, 0.7);
    ribbon(&main, 0.3 * width, 0.14, 0.0);
    // Branches split off the upper part and die out on the way down.
    for _ in 0..rng.gen_range(2..5) {
        let start = rng.gen_range(2..steps / 2);
        let dir = if rng.gen::<bool>() { 1.0 } else { -1.0 };
        let mut branch = vec![main[start]];
        let mut p = main[start];
        for _ in 0..rng.gen_range(3..7) {
            p += Vec2::new(dir * height * (0.01 + rng.gen::<f32>() * 0.04), -height * (0.02 + rng.gen::<f32>() * 0.03));
            branch.push(p);
        }
        ribbon(&branch, 0.018 * width, 0.5, 0.6);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
}

/// How far east of the camera's focus forced weather is centred.
fn forced_shift() -> Vec2 {
    Vec2::X * std::env::var("FOURX_WEATHER_SHIFT").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

/// Keeps the rain and snow box around the camera, lit like the scene.
fn follow_camera(
    rig: Res<CameraRig>,
    settings: Res<Settings>,
    local: Res<LocalWeather>,
    sun: Query<&DirectionalLight>,
    ambient: Res<GlobalAmbientLight>,
    mut precip: Query<(&MeshMaterial3d<PrecipMaterial>, &mut Visibility), With<Precipitation>>,
    mut materials: ResMut<Assets<PrecipMaterial>>,
) {
    let d = rig.current_distance;
    let fade = 1.0 - smooth(PRECIP_FADE.0, PRECIP_FADE.1, d);
    for (handle, mut visibility) in &mut precip {
        let show = settings.weather && fade > 0.0;
        visibility.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
        if !show {
            continue;
        }
        let Some(mut m) = materials.get_mut(&handle.0) else { continue };
        // Sized to what the camera sees, in steps so particles don't jump about
        // with every frame of a zoom.
        let d = (d / 3.0).ceil() * 3.0;
        let size = Vec3::new(d * 1.5 + 6.0, d * 0.3 + 3.0, d * 1.3 + 6.0);
        let centre = Vec3::new(rig.focus.x, size.y * 0.5 - 0.6, rig.focus.y - d * 0.1);
        // Lit by the sun and the sky, relative to noon (like the fog clouds).
        let light = sun.iter().next().map_or(Vec3::ONE, |l| l.color.to_linear().to_vec3() * l.illuminance / 11_000.0);
        let sky = ambient.color.to_linear().to_vec3() * ambient.brightness / Vec3::new(470.3, 622.9, 900.0);
        m.params.box_min = (centre - size * 0.5).extend(d);
        m.params.box_size = size.extend(fade);
        m.params.light = (light * 0.55 + sky * 0.45).extend(1.0);
        m.params.storm = Vec4::new(local.storm, 0.0, 0.0, 0.0);
    }
}

#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct PrecipParams {
    /// The box's corner; w: camera distance (particles grow with it).
    box_min: Vec4,
    /// The box's size; w: overall strength (fades when zoomed out).
    box_size: Vec4,
    /// The weather and fog textures' area (see `fog::params`).
    area: Vec4,
    /// Light on the particles, relative to noon.
    light: Vec4,
    /// x: the storm where the camera looks (gales drive the rain and snow).
    storm: Vec4,
}

/// Rain streaks and snowflakes.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct PrecipMaterial {
    #[uniform(0)]
    params: PrecipParams,
    #[texture(1)]
    #[sampler(2)]
    weather: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    fog: Handle<Image>,
}

impl Material for PrecipMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://client/shaders/precip.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://client/shaders/precip.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// One quad per particle: every corner carries the particle's place in the
/// box (0..1, as the position) and its kind and random numbers (as the
/// normal: x 0 rain or 1 snow, y and z random); the UV says which corner.
fn precip_mesh(seed: u64) -> Mesh {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x7a1);
    let n = PARTICLES * 2;
    let (mut positions, mut normals, mut uvs, mut indices) =
        (Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 6));
    for i in 0..n {
        let place = [rng.gen::<f32>(), rng.gen::<f32>(), rng.gen::<f32>()];
        let extra = [(i >= PARTICLES) as u8 as f32, rng.gen::<f32>(), rng.gen::<f32>()];
        let base = (i * 4) as u32;
        for corner in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            positions.push(place);
            normals.push(extra);
            uvs.push(corner);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaders_know_the_wind() {
        let wind = format!("const WEATHER_WIND: vec2<f32> = vec2({:?}, {:?});", WIND.x, WIND.y);
        for shader in [
            include_str!("shaders/terrain.wesl"),
            include_str!("shaders/water.wesl"),
            include_str!("shaders/vegetation.wesl"),
            include_str!("shaders/precip.wesl"),
        ] {
            assert!(shader.contains(&wind), "missing {wind}");
        }
    }
}
