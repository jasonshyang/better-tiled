// Layer blend modes. A layer with a blend mode is drawn alone, then combined with what is below
// it (the backdrop) by `fs_composite`; `fs_blit` draws the finished view.
//
// Colours are premultiplied. The formulas are the W3C compositing ones (source-over with a
// separable blend function), which Qt's composition modes in Tiled implement; `Add` is Qt's
// `Plus`, a clamped sum.

@group(0) @binding(0) var backdrop: texture_2d<f32>;
@group(0) @binding(1) var source: texture_2d<f32>;

struct FullOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) mode: u32,
};

// One triangle covering the target; the blend mode comes in as the instance index.
@vertex
fn vs_full(@builtin(vertex_index) vi: u32, @builtin(instance_index) mode: u32) -> FullOut {
    let c = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var out: FullOut;
    out.position = vec4<f32>(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0, 0.0, 1.0);
    out.uv = c;
    out.mode = mode;
    return out;
}

fn hard_light(cb: f32, cs: f32) -> f32 {
    if (cs <= 0.5) {
        return cb * 2.0 * cs;
    }
    let s = 2.0 * cs - 1.0;
    return cb + s - cb * s;
}

fn soft_light(cb: f32, cs: f32) -> f32 {
    if (cs <= 0.5) {
        return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
    }
    var d: f32;
    if (cb <= 0.25) {
        d = ((16.0 * cb - 12.0) * cb + 4.0) * cb;
    } else {
        d = sqrt(cb);
    }
    return cb + (2.0 * cs - 1.0) * (d - cb);
}

// The blend function B(backdrop, source) of one straight channel; the mode numbers are
// `compositor::mode_index`'s.
fn blend(mode: u32, cb: f32, cs: f32) -> f32 {
    switch mode {
        case 2u: { return cb * cs; }
        case 3u: { return cb + cs - cb * cs; }
        case 4u: { return hard_light(cs, cb); }
        case 5u: { return min(cb, cs); }
        case 6u: { return max(cb, cs); }
        case 7u: {
            if (cb <= 0.0) {
                return 0.0;
            }
            if (cs >= 1.0) {
                return 1.0;
            }
            return min(1.0, cb / (1.0 - cs));
        }
        case 8u: {
            if (cb >= 1.0) {
                return 1.0;
            }
            if (cs <= 0.0) {
                return 0.0;
            }
            return 1.0 - min(1.0, (1.0 - cb) / cs);
        }
        case 9u: { return hard_light(cb, cs); }
        case 10u: { return soft_light(cb, cs); }
        case 11u: { return abs(cb - cs); }
        case 12u: { return cb + cs - 2.0 * cb * cs; }
        default: { return cs; }
    }
}

fn unpremultiply(c: vec4<f32>) -> vec3<f32> {
    if (c.a <= 0.0) {
        return vec3<f32>(0.0);
    }
    return clamp(c.rgb / c.a, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_composite(in: FullOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.position.xy);
    let d = textureLoad(backdrop, p, 0);
    let s = textureLoad(source, p, 0);
    if (in.mode == 1u) {
        return min(s + d, vec4<f32>(1.0));
    }
    let cb = unpremultiply(d);
    let cs = unpremultiply(s);
    let b = vec3<f32>(blend(in.mode, cb.r, cs.r), blend(in.mode, cb.g, cs.g), blend(in.mode, cb.b, cs.b));
    let rgb = s.rgb * (1.0 - d.a) + d.rgb * (1.0 - s.a) + s.a * d.a * b;
    return vec4<f32>(rgb, s.a + d.a - s.a * d.a);
}

@fragment
fn fs_blit(in: FullOut) -> @location(0) vec4<f32> {
    let size = textureDimensions(backdrop);
    let p = min(vec2<u32>(in.uv * vec2<f32>(size)), size - vec2<u32>(1u, 1u));
    return textureLoad(backdrop, p, 0);
}
