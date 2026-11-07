// Water surface. Extends Bevy's StandardMaterial: colour, transparency and foam are
// computed per pixel from the depth of the terrain below (sampled from a height
// texture), then lit by the regular PBR pipeline so shadows, fog and the sun
// glint on the wave normal map all still apply.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};
import bevy_pbr::render::mesh_view_bindings::{globals, view, lights};

struct WaterParams {
    // World XZ of the height texture's first texel, and the world size it covers.
    origin: vec2<f32>,
    size: vec2<f32>,
    shallow: vec4<f32>,
    mid: vec4<f32>,
    deep: vec4<f32>,
    foam: vec4<f32>,
    // World units per repeat of the two wave layers (whole fractions of the map
    // width, so waves wrap seamlessly).
    wave_tiles: vec2<f32>,
    // Fog of war texture area (see apply_fog).
    fog: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> params: WaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var height_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var height_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var wave_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var wave_sampler: sampler;

@group(constants::MATERIAL_BIND_GROUP) @binding(105) var fog_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var fog_sampler: sampler;
// R: cloud cover and storm, G: falling rain/snow, B: snow lying, A: wet ground (weather.rs).
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var weather_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var weather_sampler: sampler;

// --- Fog of war -------------------------------------------------------------
// Identical in terrain.wesl, water.wesl and vegetation.wesl; keep them in sync
// (a client test checks). The fog texture holds 0 (never explored), 0.5
// (explored, not visible) or 1 (visible), blended softly between tiles and
// eased over time where it changes.
//
// Unexplored land lies under a bank of cloud: two layers at different heights
// (so they shift against each other as the camera moves), each a slowly
// churning, domain-warped noise field, lit from the sun's side.

// Clouds are lit like the land: by the sun (or the moon at night) and the sky
// (see daylight.rs). Their colours below were chosen for the noon light, so
// both are relative to it: 1 at noon.
const FOG_NOON_SUN: vec3<f32> = vec3(11000.0, 9790.0, 7817.0);
const FOG_NOON_SKY: vec3<f32> = vec3(470.3, 622.9, 900.0);

struct FogSky {
    // Towards the sun on the ground plane.
    sun_dir: vec2<f32>,
    sun: vec3<f32>,
    sky: vec3<f32>,
}

fn fog_sky() -> FogSky {
    let light = lights.directional_lights[0];
    let d = light.direction_to_light.xz;
    return FogSky(
        d / max(length(d), 1e-3),
        light.color.rgb / FOG_NOON_SUN,
        lights.ambient_color.rgb / FOG_NOON_SKY,
    );
}
// Heights of the two cloud layers.
const FOG_HIGH: f32 = 3.4;
const FOG_LOW: f32 = 1.2;

fn fog_hash(p: vec2<f32>) -> f32 {
    // Integer hash (no sin(): its precision breaks down at large coordinates).
    var h = (bitcast<u32>(i32(p.x)) * 1597334677u) ^ (bitcast<u32>(i32(p.y)) * 3812015801u);
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h) / 4294967295.0;
}

fn fog_gradient(i: vec2<f32>) -> vec2<f32> {
    let a = fog_hash(i) * 6.2831853;
    return vec2(cos(a), sin(a));
}

/// Gradient noise (0..1): smooth, without the blocky cells of value noise.
fn fog_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = dot(fog_gradient(i), f);
    let b = dot(fog_gradient(i + vec2(1.0, 0.0)), f - vec2(1.0, 0.0));
    let c = dot(fog_gradient(i + vec2(0.0, 1.0)), f - vec2(0.0, 1.0));
    let d = dot(fog_gradient(i + vec2(1.0, 1.0)), f - vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 0.75 + 0.5;
}

/// Layered noise (0..1); each octave rotated so no grid direction lines up.
fn fog_clouds(p: vec2<f32>, octaves: i32) -> f32 {
    let r = mat2x2<f32>(0.8, -0.6, 0.6, 0.8);
    var q = p;
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    for (var i = 0; i < octaves; i++) {
        sum += fog_noise(q) * amp;
        norm += amp;
        q = r * q * 2.03 + vec2(17.0, 9.0);
        amp *= 0.5;
    }
    return sum / norm;
}

/// Noise-space point `p` bent by a slow warp field, so billows roll and
/// change shape instead of just sliding by.
fn fog_warp(p: vec2<f32>, t: f32) -> vec2<f32> {
    let warp = vec2(
        fog_noise(p * 0.4 + vec2(t * 0.021, -t * 0.008)),
        fog_noise(p * 0.4 + vec2(5.2, 1.3) + vec2(-t * 0.012, t * 0.017)),
    );
    return p + (warp - 0.5) * 0.9 + vec2(t * 0.018, t * 0.007);
}

/// Cloud density (0..1) at a warped point, its contrast stretched: rounded
/// heads of cloud with clear valleys between.
fn fog_density(q: vec2<f32>, octaves: i32) -> f32 {
    return smoothstep(0.28, 0.72, fog_clouds(q, octaves));
}

/// Where the view ray to `world` crosses the horizontal plane at `height`, so
/// every surface under a pixel shows the same cloud whatever its own height
/// (no seams between land, water and trees).
fn fog_layer(world: vec3<f32>, height: f32) -> vec2<f32> {
    let eye = view.world_position;
    let k = (height - eye.y) / min(world.y - eye.y, -1e-3);
    return (eye + (world - eye) * k).xz;
}

struct Fog {
    // 0 unexplored, 0.5 explored, 1 visible (blended between tiles).
    v: f32,
    // Haze noise over remembered land, and the cloud colour (only computed
    // where needed).
    n: f32,
    mist: vec3<f32>,
    // How much of the real surface shows through: 0 = pure mist.
    explored: f32,
    // Light reaching the ground: the cloud bank shades land beside it.
    shade: f32,
    // Sky light relative to noon, for the haze.
    sky: vec3<f32>,
}

fn fog_sample(uv_of: vec2<f32>, area: vec4<f32>) -> f32 {
    return textureSampleLevel(fog_texture, fog_sampler, (uv_of - area.xy) / area.zw, 0.0).r;
}

/// Fog of war at a world position. `area` maps world XZ to fog texture
/// coordinates: uv = (xz - area.xy) / area.zw. Cheap where fully visible.
fn fog_at(world: vec3<f32>, area: vec4<f32>, t: f32) -> Fog {
    let v = fog_sample(world.xz, area);
    let sky = fog_sky();
    // The cloud bank's shadow falls away from the sun onto land beside it.
    let towards_sun = fog_sample(world.xz + sky.sun_dir * 1.3, area);
    // Fully visible and out of the shadow (most of the screen): skip the clouds.
    if v > 0.995 && towards_sun > 0.995 {
        return Fog(v, 0.5, vec3(0.0), 1.0, 1.0, sky.sky);
    }
    let shade = mix(0.62, 1.0, smoothstep(0.02, 0.45, towards_sun));
    // Explored land (seen or remembered) shows no cloud, only the haze.
    let haze = fog_clouds(fog_layer(world, FOG_HIGH) * 0.085 + vec2(t * 0.018, t * 0.007), 3);
    if v > 0.49 {
        return Fog(v, haze, vec3(0.0), 1.0, shade, sky.sky);
    }

    // Upper layer: big lit billows. Lighting compares the density here with a
    // step towards the sun: thinning towards the sun means a lit flank,
    // thickening means the side in shadow.
    let high = fog_warp(fog_layer(world, FOG_HIGH) * 0.085, t);
    let d = fog_density(high, 5);
    let toward = fog_density(high + sky.sun_dir * 0.05, 4);
    let lit = clamp(0.5 + (d - toward) * 3.5, 0.0, 1.0);
    // Lower layer: darker, slower, seen through gaps in the upper one.
    let low = fog_warp(fog_layer(world, FOG_LOW) * 0.13 + vec2(31.0, 7.0), t * 0.6);
    let deep = fog_density(low, 4);

    let shadow_col = vec3(0.3, 0.35, 0.45) * sky.sky;
    let body_col = vec3(0.6, 0.63, 0.69) * mix(sky.sky, sky.sun, 0.5);
    let lit_col = vec3(1.0, 0.97, 0.9) * sky.sun;
    let under = mix(shadow_col * 0.8, body_col * 0.8, deep);
    // Thick cloud is bright; its flank away from the sun falls into shadow.
    var top = mix(body_col * 0.85, lit_col, smoothstep(0.35, 1.0, d));
    top *= mix(0.62, 1.12, lit);
    let cover = smoothstep(0.15, 0.55, d);
    let mist = mix(under, top, cover);

    // Unexplored: thick cloud, fraying into wisps over explored ground. Thin
    // spots of the upper layer let the edge reach further in or fall back.
    // Remembered land (v = 0.5) is always clear, so it can skip all this.
    let n = d * 0.65 + deep * 0.35;
    let explored = smoothstep(0.05, 0.4, v * 1.25 + (n - 0.5) * 0.4);
    return Fog(v, haze, mist, explored, shade, sky.sky);
}

/// Applies fog of war to a lit colour.
fn apply_fog(color: vec4<f32>, fog: Fog) -> vec4<f32> {
    if fog.v > 0.995 {
        return vec4(color.rgb * fog.shade, color.a);
    }
    // Remembered ground: desaturated and dimmed under a thin, drifting haze.
    let lum = dot(color.rgb, vec3(0.299, 0.587, 0.114));
    let haze = 0.15 + smoothstep(0.3, 0.8, fog.n) * 0.2;
    let remembered = mix(mix(vec3(lum), color.rgb, 0.45) * 0.78, vec3(0.36, 0.4, 0.47) * fog.sky, haze);
    let seen = smoothstep(0.55, 0.95, fog.v + (fog.n - 0.5) * 0.15);
    var c = mix(remembered, color.rgb, seen) * fog.shade;
    c = mix(fog.mist, c, fog.explored);
    return vec4(c, mix(1.0, color.a, fog.explored));
}

// --- Weather ----------------------------------------------------------------
// Identical in terrain.wesl, water.wesl and vegetation.wesl; keep them in sync
// (a client test checks). The weather texture covers the same area as the fog
// of war (see weather.rs): R cloud cover up to 0.5 and storm above, G what
// falls (0.5 nothing, above rain, below snow), B snow lying on the ground, A
// wet ground.

// The wind (weather::WIND).
const WEATHER_WIND: vec2<f32> = vec2(0.35, 0.12);

struct Weather {
    // Light let through by the clouds overhead (1 under a clear sky).
    light: f32,
    rain: f32,
    snow_cover: f32,
    wet: f32,
    storm: f32,
}

fn weather_sample(xz: vec2<f32>, area: vec4<f32>) -> vec4<f32> {
    return textureSampleLevel(weather_texture, weather_sampler, (xz - area.xy) / area.zw, 0.0);
}

// Degrees colder per unit of height (mountain tiles are 13 °C colder than
// the land around them and stand about 0.9 units higher).
const SNOW_LAPSE: f32 = 15.0;

/// The temperature (°C) at height `y`, from the terrain's climate texture (R
/// mean sea-level temperature, B broad noise, A seasonal swing; see
/// season.rs) and the season (`season`: the swing's sine, lagged so snow
/// lingers into spring).
fn climate_temperature(climate: vec4<f32>, y: f32, season: f32) -> f32 {
    let mean = climate.r * 80.0 - 40.0 + (climate.b - 0.5) * 5.0;
    // The seasons swing less up high.
    let swing = (climate.a * 60.0 - 30.0) * clamp(1.0 - 0.3 * y, 0.4, 1.0);
    return mean - SNOW_LAPSE * max(y, 0.0) + swing * season;
}

/// Snow lying (0..1) because of the cold: a dusting just below freezing,
/// fully white in deep cold.
fn climate_snow(climate: vec4<f32>, y: f32, season: f32) -> f32 {
    return smoothstep(1.0, -7.0, climate_temperature(climate, y, season));
}

/// The weather at a world position.
fn weather_at(world: vec3<f32>, area: vec4<f32>, t: f32) -> Weather {
    let here = weather_sample(world.xz, area);
    let cover = min(here.r * 2.0, 1.0);
    let storm = max(here.r * 2.0 - 1.0, 0.0);
    // Rain or snow falling (the weather texture is smooth, so the area it
    // falls on has soft edges).
    let falling = smoothstep(0.05, 0.5, abs(here.g * 2.0 - 1.0));
    // Overcast dims the light a little, falling rain or snow more, so you can
    // see on the ground where the weather is. Less so by moonlight. (Not in
    // proportion to the sun: an overcast sky at the camera already dims it.)
    let day = smoothstep(0.0, 0.3, dot(fog_sky().sun, vec3(0.3, 0.5, 0.2)));
    let light = (1.0 - (0.08 * cover + 0.4 * falling) * mix(0.4, 1.0, day)) * (1.0 - 0.4 * storm);
    return Weather(light, max(here.g * 2.0 - 1.0, 0.0), here.b, here.a, storm);
}

fn wave_normal(uv: vec2<f32>) -> vec3<f32> {
    return textureSample(wave_texture, wave_sampler, uv).rgb * 2.0 - 1.0;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

// Cellular noise: distances to the nearest and second-nearest cell points, and
// a random value for the nearest cell.
fn voronoi(p: vec2<f32>) -> vec3<f32> {
    let cell = floor(p);
    var d1 = 9.0;
    var d2 = 9.0;
    var id = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let c = cell + vec2(f32(x), f32(y));
            let point = c + vec2(hash(c), hash(c + 17.3));
            let d = distance(p, point);
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = hash(c + 3.7);
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    return vec3(d1, d2, id);
}

// Smooth value noise.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
        mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
        u.y,
    );
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Deep in unexplored mist: just the mist, no lighting or texturing at all.
    let fog = fog_at(in.world_position.xyz, params.fog, globals.time);
    if fog.explored < 0.001 {
        var mist_out: FragmentOutput;
        mist_out.color = vec4(fog.mist, 1.0);
        return mist_out;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // Screen-space AO is computed from the opaque seabed behind the water; it
    // doesn't apply to the surface itself and would only smear it.
    pbr_input.diffuse_occlusion = vec3(1.0);
    pbr_input.specular_occlusion = 1.0;

    let p = in.world_position.xz;
    let t = globals.time;
    let uv = (p - params.origin) / params.size;
    let data = textureSampleLevel(height_texture, height_sampler, uv, 0.0);
    let depth = -data.r;
    // Canals: still water, no waves or surf.
    let still = data.b;

    // Depth colour ramp.
    var colour = mix(params.shallow, params.mid, smoothstep(0.03, 0.3, depth));
    colour = mix(colour, params.deep, smoothstep(0.3, 0.8, depth));

    let wx = weather_at(in.world_position.xyz, params.fog, t);

    // Shoreline foam: a solid rim plus bands that lap towards the beach,
    // wider in a storm. Out at sea a storm whips up whitecaps that streak
    // downwind and come and go.
    let n = noise(p * 3.0 + vec2(t * 0.15, -t * 0.1));
    let rim = 1.0 - smoothstep(0.012, 0.035 + n * 0.02 + wx.storm * 0.03, depth);
    let bands = smoothstep(0.55, 0.9, sin(depth * 70.0 - t * (1.6 + wx.storm) + n * 4.0) * 0.5 + 0.5)
        * (1.0 - smoothstep(0.04, 0.11 + wx.storm * 0.08, depth)) * smoothstep(0.3, 0.7, noise(p * 6.0 - t * 0.2));
    var caps = 0.0;
    if wx.storm > 0.01 {
        let downwind = p - WEATHER_WIND * t * 3.0;
        let streak = noise(vec2(downwind.x * 2.0 + downwind.y * 0.7, downwind.y * 6.0 - downwind.x * 2.1));
        let gust = noise(p * 0.7 + vec2(t * 0.3, t * 0.2));
        caps = smoothstep(0.72 - 0.12 * wx.storm, 0.86, streak * 0.7 + gust * 0.3 + noise(p * 9.0 + t) * 0.15) * wx.storm * smoothstep(0.05, 0.2, depth);
    }
    let foam = clamp(rim + bands * 0.8 + caps * 0.85, 0.0, 1.0) * (1.0 - still);
    // Stormy water is darker and greyer.
    colour = vec4(mix(colour.rgb, vec3(dot(colour.rgb, vec3(0.3, 0.5, 0.2))) * 0.75, wx.storm * 0.6), colour.a);
    colour = mix(colour, params.foam, foam);

    // Fade out right at the waterline so the water never fights with the beach.
    colour.a *= smoothstep(0.0, 0.008, depth);

    // Sea ice: solid pack inside frozen areas, breaking into floes towards its
    // edge, with dark cracks between plates.
    let cells = voronoi(p * 1.4);
    let cover = data.g + (noise(p * 0.35) - 0.5) * 0.5 + (noise(p * 1.3) - 0.5) * 0.25;
    let floe = step(cells.z, cover * 1.6 - 0.3) * smoothstep(0.1, 0.35, cover);
    let crack = 1.0 - smoothstep(0.015, 0.06, cells.y - cells.x);
    let ice_shade = 0.88 + noise(p * 9.0) * 0.12;
    let ice_colour = mix(vec4(0.82, 0.9, 0.97, 1.0) * ice_shade, vec4(0.32, 0.5, 0.62, 1.0), crack * 0.85);
    let ice = floe * smoothstep(0.004, 0.02, depth);
    colour = mix(colour, ice_colour, ice);

    // Waves: two normal-map layers drifting in different directions, blended
    // ("whiteout" blend) and calmed in the shallows. Computed here rather than via
    // StandardMaterial because, with a normal prepass active, Bevy would otherwise
    // reuse the prepass normals, which belong to the seabed behind the water.
    let w1 = wave_normal(p / params.wave_tiles.x + vec2(t * 0.013, t * 0.008));
    let w2 = wave_normal(p / params.wave_tiles.y + vec2(-t * 0.011, t * 0.017));
    let calm = mix(0.35, 1.0, smoothstep(0.02, 0.25, depth)) * (1.0 + wx.storm * 1.8) * mix(1.0, 0.06, still);
    let ts = normalize(vec3((w1.xy + w2.xy) * calm, w1.z * w2.z));
    // Ice is flat apart from slight tilt per floe; water uses the waves.
    let floe_tilt = vec3(hash(vec2(cells.z, 1.0)) - 0.5, 8.0, hash(vec2(cells.z, 2.0)) - 0.5);
    pbr_input.N = normalize(mix(vec3(ts.x, ts.z, ts.y), normalize(floe_tilt), ice));
    pbr_input.world_normal = vec3(0.0, 1.0, 0.0);

    // Rain roughens the surface.
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.3, wx.rain * (1.0 - ice));

    pbr_input.material.base_color = colour;
    // Foam is matte, open water glossy, ice in between.
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.8, foam);
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.35, ice);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Cloud shadows dim the lit surface, not the distance haze over it.
    out.color = vec4(out.color.rgb * wx.light, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    // After distance haze, so the mist looks the same on every surface.
    out.color = apply_fog(out.color, fog);
    return out;
}
