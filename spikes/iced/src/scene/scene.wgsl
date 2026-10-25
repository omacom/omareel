// The whole composition in one instanced draw: every layer of Composition.qml is one `Item`, a
// quad in its own local space mapped to the canvas by an affine transform (the zoom, the camera's
// scale, the cursor's rotation). The fragment shader picks what the item is by `k.x`.

struct Globals {
    // The canvas in composition units (output pixels at the output size).
    canvas: vec2<f32>,
    // 1 when the target is an sRGB format: the scene blends in sRGB space, as Qt Quick does, so
    // the result is decoded before the hardware encodes it again.
    srgb_target: u32,
    _pad: u32,
};

struct Item {
    // canvas = (m.x * x + m.z * y + t.x, m.y * x + m.w * y + t.y)
    m: vec4<f32>,
    t: vec4<f32>,
    // The local rectangle the quad covers (x0, y0, x1, y1).
    quad: vec4<f32>,
    // The rounded rectangle (or ring box) the item is: x0, y0, x1, y1.
    shape: vec4<f32>,
    // Fill colour and border colour, straight alpha.
    color: vec4<f32>,
    color2: vec4<f32>,
    // Texture coordinates: uv = uv.xy + t * uv.zw, t being the position in `shape` (0..1).
    uv: vec4<f32>,
    // x: corner radius (ring: centre-line radius); y: border width, shadow sigma or stroke
    // width; z: opacity; w: saturation (1 unchanged).
    p: vec4<f32>,
    // x: kind; y: texture; z: quarter turns of the content; w: horizontal flip.
    k: vec4<u32>,
};

const KIND_RECT: u32 = 0u;
const KIND_SHADOW: u32 = 1u;
const KIND_IMAGE: u32 = 2u;
const KIND_TEXT: u32 = 3u;
const KIND_RING: u32 = 4u;

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<storage, read> items: array<Item>;
@group(0) @binding(2) var linear_sampler: sampler;
@group(0) @binding(3) var tex_wallpaper: texture_2d<f32>;
@group(0) @binding(4) var tex_screen: texture_2d<f32>;
@group(0) @binding(5) var tex_camera: texture_2d<f32>;
@group(0) @binding(6) var tex_cursor: texture_2d<f32>;
@group(0) @binding(7) var tex_text: texture_2d<f32>;
@group(0) @binding(8) var tex_backdrop: texture_2d<f32>;

// The item's fields travel as flat varyings: reading the 144-byte item from the storage buffer
// in every fragment cost the Iris Xe about 3 ms a frame.
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) shape: vec4<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) color2: vec4<f32>,
    @location(4) @interpolate(flat) uv: vec4<f32>,
    @location(5) @interpolate(flat) p: vec4<f32>,
    @location(6) @interpolate(flat) k: vec4<u32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VertexOut {
    let item = items[instance];
    let corner = vec2<f32>(f32(vertex & 1u), f32((vertex >> 1u) & 1u));
    let local = mix(item.quad.xy, item.quad.zw, corner);
    let canvas = vec2<f32>(
        item.m.x * local.x + item.m.z * local.y + item.t.x,
        item.m.y * local.x + item.m.w * local.y + item.t.y,
    );
    var out: VertexOut;
    out.position = vec4<f32>(canvas.x / globals.canvas.x * 2.0 - 1.0, 1.0 - canvas.y / globals.canvas.y * 2.0, 0.0, 1.0);
    out.local = local;
    out.shape = item.shape;
    out.color = item.color;
    out.color2 = item.color2;
    out.uv = item.uv;
    out.p = item.p;
    out.k = item.k;
    return out;
}

fn sd_round_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half.x, half.y));
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn coverage(distance: f32, pixel: f32) -> f32 {
    return clamp(0.5 - distance / pixel, 0.0, 1.0);
}

// A Gaussian-blurred rounded rectangle in closed form along x and four samples along y
// (Evan Wallace, "Fast Rounded Rectangle Shadows").
fn gaussian(x: f32, sigma: f32) -> f32 {
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (2.5066283 * sigma);
}

fn erf2(x: vec2<f32>) -> vec2<f32> {
    let s = sign(x);
    let a = abs(x);
    var r = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    r = r * r;
    return s - s / (r * r);
}

fn shadow_x(x: f32, y: f32, sigma: f32, corner: f32, half: vec2<f32>) -> f32 {
    let delta = min(half.y - corner - abs(y), 0.0);
    let curved = half.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    let integral = 0.5 + 0.5 * erf2((x + vec2<f32>(-curved, curved)) * (0.70710678 / sigma));
    return integral.y - integral.x;
}

fn rounded_box_shadow(lower: vec2<f32>, upper: vec2<f32>, point: vec2<f32>, sigma: f32, radius: f32) -> f32 {
    let center = (lower + upper) * 0.5;
    let half = (upper - lower) * 0.5;
    let corner = min(radius, min(half.x, half.y));
    let p = point - center;
    let low = p.y - half.y;
    let high = p.y + half.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) / 4.0;
    var y = start + step * 0.5;
    var value = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        value = value + shadow_x(p.x, p.y - y, sigma, corner, half) * gaussian(y, sigma) * step;
        y = y + step;
    }
    return value;
}

// Where in the unturned content a position `t` of the turned, flipped item shows.
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

// Textures hold premultiplied colour (video and wallpapers are opaque; resvg premultiplies).
fn sample_texture(index: u32, uv: vec2<f32>) -> vec4<f32> {
    switch index {
        case 0u: { return textureSampleLevel(tex_wallpaper, linear_sampler, uv, 0.0); }
        case 1u: { return textureSampleLevel(tex_screen, linear_sampler, uv, 0.0); }
        case 2u: { return textureSampleLevel(tex_camera, linear_sampler, uv, 0.0); }
        case 3u: { return textureSampleLevel(tex_cursor, linear_sampler, uv, 0.0); }
        case 5u: { return textureSampleLevel(tex_backdrop, linear_sampler, uv, 0.0); }
        default: { return vec4<f32>(0.0); }
    }
}

fn premultiplied(color: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(color.rgb * color.a, color.a);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let item = in;
    // Derivatives are only defined in uniform control flow, so take them before the switch.
    let fw = fwidth(in.local);
    let pixel = max(max(fw.x, fw.y), 1e-4);
    let center = (item.shape.xy + item.shape.zw) * 0.5;
    let half = (item.shape.zw - item.shape.xy) * 0.5;
    let p = in.local - center;
    let t = (in.local - item.shape.xy) / max(item.shape.zw - item.shape.xy, vec2<f32>(1e-4));

    var out = vec4<f32>(0.0);
    switch item.k.x {
        case KIND_RECT: {
            // A QML Rectangle: fill, and a border of width p.y inside the edge.
            let d = sd_round_box(p, half, item.p.x);
            let outer = coverage(d, pixel);
            let inner = select(outer, coverage(d + item.p.y, pixel), item.p.y > 0.0);
            out = premultiplied(item.color) * inner + premultiplied(item.color2) * (outer - inner);
        }
        case KIND_SHADOW: {
            let sigma = max(item.p.y, 0.01);
            // Well inside the rectangle the blur changes nothing: skip the integral there.
            var a = 1.0;
            if sd_round_box(p, half, item.p.x) > -3.0 * sigma {
                a = rounded_box_shadow(item.shape.xy, item.shape.zw, in.local, sigma, item.p.x);
            }
            out = premultiplied(item.color) * a;
        }
        case KIND_IMAGE: {
            let d = sd_round_box(p, half, item.p.x);
            let c = coverage(d, pixel);
            let u = orient(clamp(t, vec2<f32>(0.0), vec2<f32>(1.0)), item.k.z, item.k.w);
            var color = sample_texture(item.k.y, item.uv.xy + u * item.uv.zw);
            if item.p.w != 1.0 {
                let gray = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
                color = vec4<f32>(clamp(mix(vec3<f32>(gray), color.rgb, item.p.w), vec3<f32>(0.0), vec3<f32>(color.a)), color.a);
            }
            out = color * c;
        }
        case KIND_TEXT: {
            let a = textureSampleLevel(tex_text, linear_sampler, item.uv.xy + t * item.uv.zw, 0.0).r;
            out = premultiplied(item.color) * a;
        }
        case KIND_RING: {
            let d = abs(length(p) - item.p.x) - item.p.y * 0.5;
            out = premultiplied(item.color) * coverage(d, pixel);
        }
        default: {}
    }
    out = out * item.p.z;
    if globals.srgb_target == 1u && out.a > 0.0 {
        out = vec4<f32>(srgb_to_linear(out.rgb / out.a) * out.a, out.a);
    }
    return out;
}
