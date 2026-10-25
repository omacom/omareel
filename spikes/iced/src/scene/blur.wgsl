// BlurredBackdrop.qml's camera copy: the turned, cover-cropped camera captured into a tiny
// texture, then blurred with a separable Gaussian into a slightly larger one.

struct Params {
    // Cover crop of the camera frame: x, y, w, h in texture coordinates.
    crop: vec4<f32>,
    // The source texel step along the blur direction (zero for the capture).
    direction: vec2<f32>,
    sigma: f32,
    // Quarter turns and flip of the camera content.
    quarter_turns: u32,
    flip: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var source: texture_2d<f32>;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> VertexOut {
    // One triangle over the whole target.
    let uv = vec2<f32>(f32((vertex << 1u) & 2u), f32(vertex & 2u));
    var out: VertexOut;
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn orient(t: vec2<f32>, quarter_turns: u32, flip: u32) -> vec2<f32> {
    var u = t;
    switch quarter_turns {
        case 1u: { u = vec2<f32>(t.y, 1.0 - t.x); }
        case 2u: { u = vec2<f32>(1.0 - t.x, 1.0 - t.y); }
        case 3u: { u = vec2<f32>(1.0 - t.y, t.x); }
        default: {}
    }
    if flip == 1u {
        u.x = 1.0 - u.x;
    }
    return u;
}

// Each target texel averages an 8x8 grid over its footprint, so the 24x24 capture of a 640x480
// camera neither aliases nor flickers.
@fragment
fn fs_capture(in: VertexOut) -> @location(0) vec4<f32> {
    let target_texel = 1.0 / 24.0;
    let origin = floor(in.uv * 24.0) * target_texel;
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < 8; y = y + 1) {
        for (var x = 0; x < 8; x = x + 1) {
            let t = origin + (vec2<f32>(f32(x), f32(y)) + 0.5) / 8.0 * target_texel;
            let u = orient(t, params.quarter_turns, params.flip);
            sum = sum + textureSampleLevel(source, linear_sampler, params.crop.xy + u * params.crop.zw, 0.0);
        }
    }
    return sum / 64.0;
}

@fragment
fn fs_blur(in: VertexOut) -> @location(0) vec4<f32> {
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var i = -6; i <= 6; i = i + 1) {
        let x = f32(i);
        let w = exp(-(x * x) / (2.0 * params.sigma * params.sigma));
        sum = sum + textureSampleLevel(source, linear_sampler, in.uv + params.direction * x, 0.0) * w;
        weight = weight + w;
    }
    return sum / weight;
}
