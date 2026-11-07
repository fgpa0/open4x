//! Ambient particles: one mesh of camera-facing quads rebuilt every frame
//! from a list of puffs (smoke, mist, dust, leaves, butterflies, fireflies,
//! lamps, spray), drawn by `shaders/puffs.wesl`. Emitters elsewhere push
//! puffs; this moves them, ages them and drops the dead.

use crate::loading::{AppState, InGameSet};
use crate::wrap::{nearest_shift, WrapPeriod};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

/// More than this many puffs at once and new ones are refused.
const MAX: usize = 6000;

/// How a puff is drawn (the shader's shapes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Smoke = 0,
    Leaf = 1,
    /// Lit from within: fireflies and lamps (not shaded by the sun).
    Glow = 2,
    Butterfly = 3,
    /// Spray and snow: small and round.
    Drop = 4,
    /// Low, wide and very soft.
    Mist = 5,
}

#[derive(Clone, Copy, Debug)]
pub struct Puff {
    pub pos: Vec3,
    pub vel: Vec3,
    /// Added to the velocity every second (buoyancy, gravity).
    pub accel: Vec3,
    /// Fraction of its velocity a puff keeps each second.
    pub drag: f32,
    pub age: f32,
    pub life: f32,
    /// Size at birth and at death (half the quad's height).
    pub size: (f32, f32),
    /// Colour and peak opacity.
    pub color: Vec4,
    pub shape: Shape,
    /// Width over height.
    pub aspect: f32,
    pub angle: f32,
    /// Turning speed (radians a second).
    pub spin: f32,
    /// Random number for the shader (noise, flapping, blinking).
    pub seed: f32,
    /// Fractions of its life spent fading in and out.
    pub fade: (f32, f32),
    /// Opacity that blinks on and off (fireflies) at this rate, if set.
    pub blink: f32,
    /// Wanders sideways by up to this much a second (butterflies, flies).
    pub flutter: f32,
}

impl Puff {
    pub fn new(shape: Shape, pos: Vec3, life: f32, size: f32, color: Vec4) -> Self {
        Puff {
            pos,
            vel: Vec3::ZERO,
            accel: Vec3::ZERO,
            drag: 1.0,
            age: 0.0,
            life,
            size: (size, size),
            color,
            shape,
            aspect: 1.0,
            angle: 0.0,
            spin: 0.0,
            seed: (pos.x * 12.9898 + pos.z * 78.233 + life * 3.17).sin().abs().fract(),
            fade: (0.15, 0.4),
            blink: 0.0,
            flutter: 0.0,
        }
    }
    pub fn vel(mut self, v: Vec3) -> Self {
        self.vel = v;
        self
    }
    pub fn accel(mut self, a: Vec3) -> Self {
        self.accel = a;
        self
    }
    pub fn drag(mut self, d: f32) -> Self {
        self.drag = d;
        self
    }
    pub fn grow(mut self, to: f32) -> Self {
        self.size.1 = to;
        self
    }
    pub fn aspect(mut self, a: f32) -> Self {
        self.aspect = a;
        self
    }
    pub fn spin(mut self, angle: f32, spin: f32) -> Self {
        self.angle = angle;
        self.spin = spin;
        self
    }
    pub fn fade(mut self, fade_in: f32, fade_out: f32) -> Self {
        self.fade = (fade_in, fade_out);
        self
    }
    pub fn blink(mut self, rate: f32) -> Self {
        self.blink = rate;
        self
    }
    pub fn flutter(mut self, f: f32) -> Self {
        self.flutter = f;
        self
    }
    pub fn seed(mut self, s: f32) -> Self {
        self.seed = s;
        self
    }

    /// Opacity now: faded in and out over its life, and blinking.
    fn alpha(&self, time: f32) -> f32 {
        let u = (self.age / self.life).clamp(0.0, 1.0);
        let fade_in = if self.fade.0 > 0.0 { (u / self.fade.0).min(1.0) } else { 1.0 };
        let fade_out = if self.fade.1 > 0.0 { ((1.0 - u) / self.fade.1).min(1.0) } else { 1.0 };
        let blink = if self.blink > 0.0 {
            let w = (time * self.blink + self.seed * 37.0).sin();
            ((w - 0.2) * 2.0).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.color.w * fade_in * fade_out * blink
    }
}

/// The puffs in the air, and lights that stay put (lamps and torches, set
/// by `towns.rs`) shown with `lamps` opacity.
#[derive(Resource, Default)]
pub struct Puffs {
    pub list: Vec<Puff>,
    pub lamps: Vec<Puff>,
    pub lamp_level: f32,
}

impl Puffs {
    pub fn add(&mut self, p: Puff) {
        if self.list.len() < MAX {
            self.list.push(p);
        }
    }

    pub fn room(&self) -> usize {
        MAX - self.list.len().min(MAX)
    }
}

#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct PuffParams {
    light: Vec4,
    fog_area: Vec4,
    ground_area: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct PuffMaterial {
    #[uniform(0)]
    params: PuffParams,
    #[texture(1)]
    #[sampler(2)]
    fog: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    ground: Handle<Image>,
}

impl Material for PuffMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://client/shaders/puffs.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://client/shaders/puffs.wesl".into()
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

#[derive(Component)]
struct PuffMesh;

pub fn plugin(app: &mut App) {
    crate::embed_puff_shader(app);
    app.add_plugins(MaterialPlugin::<PuffMaterial>::default())
        .init_resource::<Puffs>()
        .add_systems(OnEnter(AppState::InGame), spawn.after(crate::play::start_game))
        .add_systems(Update, (step, draw).chain().in_set(InGameSet).in_set(super::AmbienceSet::Draw));
    if std::env::var("FOURX_AMBIENCE_LOG").is_ok() {
        app.add_systems(Update, log.in_set(InGameSet));
    }
}

/// `FOURX_AMBIENCE_LOG=1`: logs the puffs of each kind every two seconds.
fn log(time: Res<Time>, puffs: Res<Puffs>, stage: Res<super::Stage>, mut last: Local<f32>) {
    if time.elapsed_secs() - *last < 2.0 {
        return;
    }
    *last = time.elapsed_secs();
    let mut counts = [0; 6];
    for p in &puffs.list {
        counts[p.shape as usize] += 1;
    }
    info!("ambience: smoke {} leaf {} glow {} butterfly {} drop {} mist {}, lamps {} at {:.2}; day {:.2} night {:.2} distance {:.0}", counts[0], counts[1], counts[2], counts[3], counts[4], counts[5], puffs.lamps.len(), puffs.lamp_level, stage.day, stage.night, stage.distance);
}

fn spawn(
    mut commands: Commands,
    mut puffs: ResMut<Puffs>,
    fog: Res<crate::fog::FogOfWar>,
    ice: Res<crate::water::IceCover>,
    terrain: Res<crate::terrain::Terrain>,
    game: Res<crate::GameMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PuffMaterial>>,
) {
    *puffs = Puffs::default();
    let hf = &terrain.heights;
    let material = materials.add(PuffMaterial {
        params: PuffParams {
            light: Vec4::ONE,
            fog_area: crate::fog::params(&game.map.grid),
            ground_area: Vec4::new(hf.origin.x, hf.origin.y, hf.cols as f32 * hf.step, hf.rows as f32 * hf.step),
        },
        fog: fog.image.clone(),
        ground: ice.image(),
    });
    commands.spawn((
        PuffMesh,
        Mesh3d(meshes.add(empty_mesh())),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Visibility::Hidden,
        NoFrustumCulling,
        NotShadowCaster,
    ));
}

/// One invisible quad of no size: Bevy warns about meshes with no vertices.
fn empty_mesh() -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32; 3]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0f32; 2]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 4])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// Moves and ages the puffs; the wind carries them all a little.
fn step(time: Res<Time>, mut puffs: ResMut<Puffs>) {
    let dt = time.delta_secs().min(0.1);
    let t = time.elapsed_secs();
    puffs.list.retain_mut(|p| {
        p.age += dt;
        if p.age >= p.life {
            return false;
        }
        p.vel += p.accel * dt;
        if p.drag < 1.0 {
            p.vel *= p.drag.powf(dt);
        }
        if p.flutter > 0.0 {
            let s = p.seed * 50.0;
            p.vel += Vec3::new((t * 2.3 + s).sin(), (t * 3.1 + s * 1.3).sin() * 0.5, (t * 1.9 + s * 0.7).cos()) * p.flutter * dt;
        }
        p.pos += p.vel * dt;
        p.angle += p.spin * dt;
        true
    });
}

/// Rebuilds the particle mesh around the camera, lit like the scene.
fn draw(
    time: Res<Time>,
    puffs: Res<Puffs>,
    period: Res<WrapPeriod>,
    camera: Single<&GlobalTransform, With<crate::camera::MainCamera>>,
    sun: Query<&DirectionalLight>,
    ambient: Res<GlobalAmbientLight>,
    mut view: Query<(&Mesh3d, &MeshMaterial3d<PuffMaterial>, &mut Visibility), With<PuffMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PuffMaterial>>,
) {
    let Ok((mesh, material, mut visibility)) = view.single_mut() else { return };
    let t = time.elapsed_secs();
    let cam_x = camera.translation().x;
    let lamps = puffs.lamps.iter().filter(|_| puffs.lamp_level > 0.01).map(|p| (p, puffs.lamp_level));
    let all = puffs.list.iter().map(|p| (p, 1.0)).chain(lamps);
    let n = puffs.list.len() + if puffs.lamp_level > 0.01 { puffs.lamps.len() } else { 0 };
    let (mut pos, mut extra, mut corner, mut more, mut color, mut idx) =
        (Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 6));
    for (p, level) in all {
        let a = p.alpha(t) * level;
        if a < 0.004 {
            continue;
        }
        let u = (p.age / p.life).clamp(0.0, 1.0);
        let size = p.size.0 + (p.size.1 - p.size.0) * u;
        let centre = p.pos + Vec3::X * nearest_shift(p.pos.x, cam_x, period.0);
        let base = pos.len() as u32;
        for c in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            pos.push(centre.to_array());
            extra.push([size, p.angle, p.shape as u8 as f32]);
            corner.push(c);
            more.push([p.aspect, p.seed]);
            color.push([p.color.x, p.color.y, p.color.z, a]);
        }
        idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    // Nothing to draw: hide the last mesh rather than upload an empty one.
    visibility.set_if_neq(if idx.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
    if idx.is_empty() {
        return;
    }
    if let Some(mut m) = meshes.get_mut(&mesh.0) {
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, extra);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, corner);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_1, more);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, color);
        m.insert_indices(Indices::U32(idx));
    }
    // Lit by the sun and the sky relative to noon (like rain and snow).
    let light = sun.iter().next().map_or(Vec3::ONE, |l| l.color.to_linear().to_vec3() * l.illuminance / 11_000.0);
    let sky = ambient.color.to_linear().to_vec3() * ambient.brightness / Vec3::new(470.3, 622.9, 900.0);
    if let Some(mut m) = materials.get_mut(&material.0) {
        m.params.light = (light * 0.6 + sky * 0.4).extend(1.0);
    }
}
