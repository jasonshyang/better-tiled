// Sprites: tiles, tile objects, image layers, glyphs, impostors and solid quads.
//
// Every image is a region of a 2D-array "page" texture with a half-size second mip level.
// Instances name a sprite; `remap` redirects animated tiles to their current frame. Sampling is
// `textureLoad` clamped to the sprite's own rectangle, so a neighbour never bleeds in.

struct Globals {
    // The world point at the viewport's top-left corner.
    origin: vec2<f32>,
    // Device pixels per world pixel.
    zoom: f32,
    _pad0: f32,
    // The viewport in device pixels.
    viewport: vec2<f32>,
    _pad1: vec2<f32>,
};

struct LayerParams {
    // World pixels added to every position (offsets and parallax).
    offset: vec2<f32>,
    opacity: f32,
    // Non-zero: apply `tint` with Tiled's formula.
    tinted: u32,
    // Straight-alpha tint.
    tint: vec4<f32>,
};

struct Sprite {
    // x, y, width, height in texels at mip level 0.
    rect: vec4<u32>,
    page: u32,
    // 0 pending, 1 ready, 2 missing, 3 impostor.
    state: u32,
    _pad: vec2<u32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var pages: texture_2d_array<f32>;
@group(0) @binding(2) var<storage, read> sprites: array<Sprite>;
@group(0) @binding(3) var<storage, read> remap: array<u32>;
@group(0) @binding(4) var impostors: texture_2d_array<f32>;
@group(1) @binding(0) var<uniform> layer: LayerParams;

const FLIP_H: u32 = 1u;
const FLIP_V: u32 = 2u;
const TRANSPOSE: u32 = 4u;
const SOLID: u32 = 8u;
const MARKER: u32 = 16u;

struct InstanceIn {
    @location(0) dst: vec4<f32>,
    @location(1) pivot_rot: vec4<f32>,
    @location(2) sprite: u32,
    @location(3) flags: u32,
    @location(4) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) sprite: u32,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) color: vec4<f32>,
};

// Turns `p` clockwise on screen (y points down) by `angle` radians about `pivot`.
fn rotate_about(p: vec2<f32>, pivot: vec2<f32>, angle: f32) -> vec2<f32> {
    let d = p - pivot;
    let s = sin(angle);
    let c = cos(angle);
    return pivot + vec2<f32>(d.x * c - d.y * s, d.x * s + d.y * c);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: InstanceIn) -> VsOut {
    // Two triangles: (0,0) (1,0) (0,1) / (0,1) (1,0) (1,1), as bit masks.
    let corner = vec2<f32>(f32((50u >> vi) & 1u), f32((44u >> vi) & 1u));
    var p = inst.dst.xy + corner * inst.dst.zw;
    let spin = inst.pivot_rot.w;
    if (spin != 0.0) {
        p = rotate_about(p, inst.dst.xy + 0.5 * inst.dst.zw, spin);
    }
    let rotation = inst.pivot_rot.z;
    if (rotation != 0.0) {
        p = rotate_about(p, inst.pivot_rot.xy, rotation);
    }
    let screen = (p + layer.offset - globals.origin) * globals.zoom;
    var out: VsOut;
    out.position = vec4<f32>(
        screen.x / globals.viewport.x * 2.0 - 1.0,
        1.0 - screen.y / globals.viewport.y * 2.0,
        0.0,
        1.0,
    );
    out.local = corner;
    out.sprite = inst.sprite;
    out.flags = inst.flags;
    out.color = inst.color;
    return out;
}

// A premultiplied texel of mip level `level`, clamped to `rect` (in that level's texels).
fn texel(page: i32, rect: vec4<i32>, p: vec2<i32>, level: i32) -> vec4<f32> {
    let q = clamp(p, vec2<i32>(0, 0), max(rect.zw - vec2<i32>(1, 1), vec2<i32>(0, 0)));
    let c = textureLoad(pages, rect.xy + q, page, level);
    return vec4<f32>(c.rgb * c.a, c.a);
}

// Sharp bilinear: texel edges anti-aliased over one device pixel.
fn magnified(page: i32, rect: vec4<i32>, t: vec2<f32>, fw: vec2<f32>) -> vec4<f32> {
    let tc = t - 0.5;
    let i = floor(tc);
    let f = clamp((tc - i - 0.5) / fw + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
    let ii = vec2<i32>(i);
    let a = texel(page, rect, ii, 0);
    let b = texel(page, rect, ii + vec2<i32>(1, 0), 0);
    let c = texel(page, rect, ii + vec2<i32>(0, 1), 0);
    let d = texel(page, rect, ii + vec2<i32>(1, 1), 0);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// A box filter over the pixel's footprint, up to 4×4 taps of the given level.
fn minified(page: i32, rect: vec4<i32>, t: vec2<f32>, fw: vec2<f32>, level: i32) -> vec4<f32> {
    let n = vec2<i32>(clamp(ceil(fw), vec2<f32>(1.0), vec2<f32>(4.0)));
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < n.y; y++) {
        for (var x = 0; x < n.x; x++) {
            let o = (vec2<f32>(f32(x), f32(y)) + 0.5) / vec2<f32>(n) - 0.5;
            sum += texel(page, rect, vec2<i32>(floor(t + o * fw)), level);
        }
    }
    return sum / f32(n.x * n.y);
}

// An impostor texel, already premultiplied, clamped to the impostor's rectangle.
fn impostor_texel(page: i32, rect: vec4<i32>, p: vec2<i32>) -> vec4<f32> {
    let q = clamp(p, vec2<i32>(0, 0), max(rect.zw - vec2<i32>(1, 1), vec2<i32>(0, 0)));
    return textureLoad(impostors, rect.xy + q, page, 0);
}

// An impostor is made at about the screen's resolution: bilinear when magnified, a box of up to
// 2×2 texels when minified.
fn impostor(page: i32, rect: vec4<i32>, t: vec2<f32>, fw: vec2<f32>) -> vec4<f32> {
    if (max(fw.x, fw.y) <= 1.0) {
        let tc = t - 0.5;
        let i = floor(tc);
        let f = tc - i;
        let ii = vec2<i32>(i);
        let a = impostor_texel(page, rect, ii);
        let b = impostor_texel(page, rect, ii + vec2<i32>(1, 0));
        let c = impostor_texel(page, rect, ii + vec2<i32>(0, 1));
        let d = impostor_texel(page, rect, ii + vec2<i32>(1, 1));
        return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
    }
    let n = vec2<i32>(clamp(ceil(fw), vec2<f32>(1.0), vec2<f32>(2.0)));
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < n.y; y++) {
        for (var x = 0; x < n.x; x++) {
            let o = (vec2<f32>(f32(x), f32(y)) + 0.5) / vec2<f32>(n) - 0.5;
            sum += impostor_texel(page, rect, vec2<i32>(floor(t + o * fw)));
        }
    }
    return sum / f32(n.x * n.y);
}

// Tiled's marker for what cannot be drawn: a half-transparent dark quad with a red border and
// a red cross, its lines one world pixel wide (at least one device pixel).
fn marker(local: vec2<f32>, per_px: vec2<f32>) -> vec4<f32> {
    let width = max(globals.zoom, 1.0);
    let size = 1.0 / max(per_px, vec2<f32>(1e-6));
    let p = local * size;
    let edge = min(p, size - p);
    let diagonal = length(size);
    let d1 = abs(p.x * size.y - p.y * size.x) / diagonal;
    let d2 = abs(p.x * size.y + p.y * size.x - size.x * size.y) / diagonal;
    if (min(edge.x, edge.y) < width || min(d1, d2) < width * 0.5) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }
    return vec4<f32>(0.0, 0.0, 0.0, 0.5);
}

// Tiled's tint of a premultiplied texel: rgb = T ⊙ (c·a + 1 − a) for straight c, alpha kept.
fn tinted(p: vec4<f32>) -> vec4<f32> {
    if (layer.tinted == 0u) {
        return p;
    }
    return vec4<f32>(layer.tint.rgb * (p.rgb + (1.0 - p.a)) * p.a, p.a);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let s = sprites[remap[in.sprite]];
    // Derivatives before any non-uniform control flow.
    let per_px = fwidth(in.local);
    var uv = in.local;
    if ((in.flags & FLIP_V) != 0u) {
        uv.y = 1.0 - uv.y;
    }
    if ((in.flags & FLIP_H) != 0u) {
        uv.x = 1.0 - uv.x;
    }
    if ((in.flags & TRANSPOSE) != 0u) {
        uv = uv.yx;
    }
    let size = vec2<f32>(f32(s.rect.z), f32(s.rect.w));
    let t = uv * size;
    let fw = max(fwidth(t), vec2<f32>(1e-5));
    let alpha = in.color.a * layer.opacity * select(1.0, layer.tint.a, layer.tinted != 0u);

    if ((in.flags & SOLID) != 0u) {
        return vec4<f32>(in.color.rgb, 1.0) * alpha;
    }
    if ((in.flags & MARKER) != 0u || s.state == 2u) {
        return marker(in.local, per_px) * alpha;
    }
    if (s.state == 0u || s.rect.z == 0u || s.rect.w == 0u) {
        discard;
    }
    let rect = vec4<i32>(s.rect);
    let page = i32(s.page);
    var color: vec4<f32>;
    if (s.state == 3u) {
        color = tinted(impostor(page, rect, t, fw));
    } else if (max(fw.x, fw.y) <= 1.0) {
        color = tinted(magnified(page, rect, t, fw));
    } else if (max(fw.x, fw.y) <= 3.0) {
        color = tinted(minified(page, rect, t, fw, 0));
    } else {
        // Half size: the rectangle covers the texels the full one touches.
        let lo = rect.xy / 2;
        let hi = (rect.xy + rect.zw + vec2<i32>(1, 1)) / 2;
        let half = vec4<i32>(lo, hi - lo);
        let shift = vec2<f32>(rect.xy) * 0.5 - vec2<f32>(lo);
        color = tinted(minified(page, half, t * 0.5 + shift, fw * 0.5, 1));
    }
    return vec4<f32>(color.rgb * in.color.rgb, color.a) * alpha;
}
