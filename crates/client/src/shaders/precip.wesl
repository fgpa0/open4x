// Rain and snow near the camera (see weather.rs). Every particle is a quad
// whose corners all carry its place in a box (0..1) and random numbers; this
// shader moves it, wraps it around the box that follows the camera, and keeps
// it only where the weather texture says that kind of thing is falling.

import bevy_pbr::render::mesh_view_bindings::{globals, view};

struct PrecipParams {
    // xyz: the box's corner; w: camera distance.
    box_min: vec4<f32>,
    // xyz: the box's size; w: overall strength.
    box_size: vec4<f32>,
    // The weather and fog textures' area: uv = (xz - area.xy) / area.zw.
    area: vec4<f32>,
    light: vec4<f32>,
    // x: the storm where the camera looks.
    storm: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: PrecipParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var weather_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var weather_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var fog_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var fog_sampler: sampler;

// The wind (weather::WIND).
const WEATHER_WIND: vec2<f32> = vec2(0.35, 0.12);

struct Vertex {
    // The particle's place in the box (0..1).
    @location(0) place: vec3<f32>,
    // x: 0 rain, 1 snow; y, z: random.
    @location(1) extra: vec3<f32>,
    @location(2) corner: vec2<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) alpha: f32,
    @location(2) snow: f32,
};

@vertex
fn vertex(in: Vertex) -> VertexOut {
    var out: VertexOut;
    let t = globals.time;
    let snow = in.extra.x > 0.5;
    let r = in.extra.y;
    let size = params.box_size.xyz;
    let gale = params.storm.x;

    // Rain falls fast and slants with the wind; snow drifts down and sways.
    // In a storm the gale drives both almost sideways.
    let gust = normalize(WEATHER_WIND) * gale * (0.8 + 0.4 * in.extra.z);
    var velocity = vec3(WEATHER_WIND.x * 4.0, -8.0 - r * 3.0 - gale * 3.0, WEATHER_WIND.y * 4.0) + vec3(gust.x, 0.0, gust.y) * 7.0;
    if snow {
        velocity = vec3(WEATHER_WIND.x * 1.5, -0.5 - r * 0.35 - gale * 0.6, WEATHER_WIND.y * 1.5) + vec3(gust.x, 0.0, gust.y) * 4.5;
    }
    // A fixed place in the world (as if the box covered everything), wrapped
    // into the box around the camera.
    var p = in.place * size + velocity * t;
    if snow {
        let phase = in.extra.z * 6.2831853;
        p += vec3(sin(t * 1.1 + phase), 0.0, cos(t * 0.8 + phase * 1.7)) * (0.2 + gale * 0.5);
    }
    let local = p - params.box_min.xyz;
    let wrapped = local - floor(local / size) * size;
    let world = params.box_min.xyz + wrapped;

    // Only where this kind of weather falls, and not over unexplored land.
    let uv = (world.xz - params.area.xy) / params.area.zw;
    let here = textureSampleLevel(weather_texture, weather_sampler, uv, 0.0);
    let fall = here.g * 2.0 - 1.0;
    // Sharpened, so rain stops about where the ground stops looking dim
    // (see `weather_at`) instead of thinning out over a wide margin.
    let amount = smoothstep(0.08, 0.4, select(max(fall, 0.0), max(-fall, 0.0), snow));
    // Ordinary weather uses about 60% of the particles, storms all of them.
    let storm = max(here.r * 2.0 - 1.0, 0.0);
    let density = amount * mix(0.6, 1.0, storm);
    let seen = smoothstep(0.3, 0.6, textureSampleLevel(fog_texture, fog_sampler, uv, 0.0).r);
    // Fade in and out at the box's edges so wrapping never pops.
    let e = wrapped / size;
    let edges = smoothstep(vec3(0.0), vec3(0.08), e) * (1.0 - smoothstep(vec3(0.92), vec3(1.0), e));
    let alpha = amount * seen * params.box_size.w * edges.x * edges.y * edges.z;
    // Light weather thins the particles out rather than making them faint.
    if fract(r * 7.31 + in.extra.z * 3.7) > density || alpha < 0.01 {
        out.position = vec4(0.0, 0.0, 0.0, 1.0);
        return out;
    }

    // Particles grow a little with the camera distance so they stay visible.
    let grow = sqrt(params.box_min.w / 12.0);
    let c = in.corner * 2.0 - 1.0;
    let to_eye = normalize(view.world_position - world);
    var offset: vec3<f32>;
    // Along the direction of travel, facing the camera.
    let along = normalize(velocity);
    let side = normalize(cross(along, to_eye));
    if snow {
        // Round flakes, smeared into short streaks by a gale.
        let right = normalize(view.world_from_view[0].xyz);
        let up = normalize(view.world_from_view[1].xyz);
        let flake = (right * c.x + up * c.y) * 0.028;
        let streak = side * c.x * 0.022 + along * c.y * 0.11;
        offset = mix(flake, streak, smoothstep(0.1, 0.7, gale)) * grow;
    } else {
        // A streak along the direction of fall, longer in a storm.
        offset = side * c.x * 0.007 * grow + along * c.y * (0.22 + 0.12 * gale) * grow;
    }
    out.position = view.clip_from_world * vec4(world + offset, 1.0);
    out.corner = c;
    out.alpha = alpha;
    out.snow = select(0.0, 1.0, snow);
    return out;
}

@fragment
fn fragment(in: VertexOut) -> @location(0) vec4<f32> {
    let d = length(in.corner);
    if in.snow > 0.5 {
        // A soft, bright flake.
        let a = (1.0 - smoothstep(0.35, 1.0, d)) * in.alpha * 0.9;
        return vec4(params.light.rgb * 0.95, a);
    }
    // A thin streak, fading towards its ends.
    let a = (1.0 - smoothstep(0.3, 1.0, abs(in.corner.x))) * (1.0 - smoothstep(0.4, 1.0, abs(in.corner.y)));
    return vec4(params.light.rgb * vec3(0.72, 0.78, 0.85), a * in.alpha * 0.38);
}
