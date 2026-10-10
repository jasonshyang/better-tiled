// Vector content: strokes and fills. Positions are world pixels; the extrusion is in device
// pixels, so cosmetic widths, dashes and offsets need nothing from the CPU when the zoom
// changes. Strokes are anti-aliased across their width.

struct Globals {
    origin: vec2<f32>,
    zoom: f32,
    _pad0: f32,
    viewport: vec2<f32>,
    _pad1: vec2<f32>,
};

struct LayerParams {
    offset: vec2<f32>,
    opacity: f32,
    tinted: u32,
    tint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> layer: LayerParams;

struct VertexIn {
    @location(0) pos: vec2<f32>,
    @location(1) extrude: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) edge: f32,
    @location(4) half_width: f32,
    @location(5) along: f32,
    @location(6) dash: vec3<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) edge: f32,
    @location(2) @interpolate(flat) half_width: f32,
    @location(3) along: f32,
    @location(4) @interpolate(flat) dash: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VsOut {
    let screen = (v.pos + layer.offset - globals.origin) * globals.zoom + v.extrude;
    var out: VsOut;
    out.position = vec4<f32>(
        screen.x / globals.viewport.x * 2.0 - 1.0,
        1.0 - screen.y / globals.viewport.y * 2.0,
        0.0,
        1.0,
    );
    out.color = v.color;
    out.edge = v.edge;
    out.half_width = v.half_width;
    out.along = v.along;
    out.dash = v.dash;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var coverage = 1.0;
    if (in.half_width >= 0.0) {
        let reach = in.half_width + 0.5;
        coverage = clamp(reach - abs(in.edge) * reach, 0.0, 1.0);
        if (in.dash.x > 0.0) {
            let period = in.dash.x + in.dash.y;
            let d = in.along * globals.zoom + in.dash.z;
            let m = d - floor(d / period) * period;
            if (m >= in.dash.x) {
                discard;
            }
        }
    }
    let a = in.color.a * coverage * layer.opacity;
    return vec4<f32>(in.color.rgb * a, a);
}
