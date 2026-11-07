// Ambient particles (see ambience/puffs.rs): smoke, mist, dust, leaves,
// butterflies, fireflies, lamps and spray. Every particle is a quad whose
// four corners carry its centre; this shader turns it to face the camera and
// draws its shape. Particles fade out where they meet the ground (no hard
// edge where a quad cuts into a hill) and over unexplored land, and take the
// scene's distance haze.

import bevy_pbr::render::mesh_view_bindings::{globals, view, fog};

struct PuffParams {
    // Light on the particles relative to noon (sun and sky, as for rain).
    light: vec4<f32>,
    // The fog of war texture's area: uv = (xz - area.xy) / area.zw.
    fog_area: vec4<f32>,
    // The heights texture's area, likewise.
    ground_area: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: PuffParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var fog_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var fog_sampler: sampler;
// R: the terrain's height (water.rs).
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var ground_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var ground_sampler: sampler;

// Shapes (puffs::Shape).
const SMOKE: u32 = 0u;
const LEAF: u32 = 1u;
const GLOW: u32 = 2u;
const BUTTERFLY: u32 = 3u;
const DROP: u32 = 4u;
const MIST: u32 = 5u;

struct Vertex {
    @location(0) centre: vec3<f32>,
    // x: size, y: rotation, z: shape.
    @location(1) extra: vec3<f32>,
    @location(2) corner: vec2<f32>,
    // x: width over height, y: random.
    @location(3) more: vec2<f32>,
    @location(5) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
    @location(3) @interpolate(flat) shape: u32,
    @location(4) seed: f32,
    // How high above the ground the particle's centre is, over its size.
    @location(5) lift: f32,
};

@vertex
fn vertex(in: Vertex) -> VertexOut {
    var out: VertexOut;
    let c = in.corner * 2.0 - 1.0;
    let size = in.extra.x;
    let angle = in.extra.y;
    let shape = u32(in.extra.z + 0.5);
    let r = vec2(c.x * cos(angle) - c.y * sin(angle), c.x * sin(angle) + c.y * cos(angle));
    let right = normalize(view.world_from_view[0].xyz);
    var up = normalize(view.world_from_view[1].xyz);
    if shape == MIST {
        // Mist lies low: its quads lean back towards the ground.
        up = normalize(mix(up, vec3(0.0, 0.0, -1.0), 0.6));
    }
    let world = in.centre + (right * r.x * in.more.x + up * r.y) * size;
    out.position = view.clip_from_world * vec4(world, 1.0);
    out.corner = c;
    out.color = in.color;
    out.world = world;
    out.shape = shape;
    out.seed = in.more.y;
    out.lift = size;
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

@fragment
fn fragment(in: VertexOut) -> @location(0) vec4<f32> {
    let c = in.corner;
    let d = length(c);
    var color = in.color.rgb;
    var alpha = in.color.a;
    var lit = true;
    var soft = 0.03;
    switch in.shape {
        case SMOKE: {
            // A lumpy, soft round puff, lighter on top.
            let n = noise(c * 2.2 + vec2(in.seed * 17.0, in.seed * 5.0)) * 0.45 + noise(c * 4.5 + in.seed * 31.0) * 0.25;
            alpha *= (1.0 - smoothstep(0.25, 1.0, d + (0.35 - n) * 0.6));
            color *= 0.85 + 0.25 * c.y;
            soft = 0.06;
        }
        case MIST: {
            let n = noise(c * 1.6 + vec2(in.seed * 13.0, globals.time * 0.03 + in.seed * 7.0));
            alpha *= (1.0 - smoothstep(0.1, 1.0, d)) * (0.55 + 0.6 * n);
            soft = 0.04;
        }
        case LEAF: {
            // A pointed oval with a midrib.
            let q = vec2(c.x * 1.8, c.y);
            let edge = length(q) + abs(c.y) * 0.25;
            alpha *= 1.0 - smoothstep(0.8, 0.95, edge);
            color *= 1.0 - 0.3 * (1.0 - smoothstep(0.0, 0.12, abs(c.x)));
        }
        case BUTTERFLY: {
            // Two pairs of wings either side of a dark body; the wings beat
            // by narrowing the shape.
            let beat = abs(sin(globals.time * 14.0 + in.seed * 40.0));
            let x = c.x / max(beat, 0.15);
            let upper = length(vec2(abs(x) - 0.45, (c.y - 0.2) * 1.3));
            let lower = length(vec2(abs(x) - 0.35, (c.y + 0.35) * 1.6));
            let wing = 1.0 - smoothstep(0.45, 0.55, min(upper, lower * 1.2));
            let body = (1.0 - smoothstep(0.06, 0.1, abs(c.x))) * (1.0 - smoothstep(0.6, 0.7, abs(c.y)));
            alpha *= max(wing * step(abs(x), 1.0), body);
            color = mix(color, vec3(0.05), body);
        }
        case GLOW: {
            // A bright core and a halo (fireflies blink, lamps flicker).
            let core = 1.0 - smoothstep(0.0, 0.25, d);
            let halo = (1.0 - smoothstep(0.0, 1.0, d));
            alpha *= core + halo * halo * 0.35;
            color *= 1.0 + core * 2.0;
            lit = false;
            soft = 0.0;
        }
        default: {
            // Drops of spray and flakes of snow: small and round.
            alpha *= 1.0 - smoothstep(0.4, 1.0, d);
            soft = 0.0;
        }
    }
    if lit {
        color *= params.light.rgb;
    }
    // Fade where the quad meets the ground, and over unexplored land.
    let ground = textureSampleLevel(ground_texture, ground_sampler, (in.world.xz - params.ground_area.xy) / params.ground_area.zw, 0.0).r;
    if soft > 0.0 {
        alpha *= smoothstep(0.0, soft, in.world.y - max(ground, 0.0));
    }
    let seen = textureSampleLevel(fog_texture, fog_sampler, (in.world.xz - params.fog_area.xy) / params.fog_area.zw, 0.0).r;
    alpha *= smoothstep(0.3, 0.6, seen);
    if alpha < 0.004 {
        discard;
    }
    // The scene's distance haze (linear, see settings.rs).
    if fog.mode == 1u {
        let distance = length(in.world - view.world_position);
        let f = fog.base_color.a * (1.0 - clamp((fog.be.y - distance) / (fog.be.y - fog.be.x), 0.0, 1.0));
        color = mix(color, fog.base_color.rgb, f);
    }
    return vec4(color, alpha);
}
