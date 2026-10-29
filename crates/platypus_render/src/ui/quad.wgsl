// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Platypus UI quads: rounded boxes with borders and fills (solid or gradient),
// and analytic outer / inset box shadows. One instance per primitive.
//
// Geometry is in logical pixels; `globals.scale` converts to physical pixels.
// Colours arrive premultiplied in the canvas's space (sRGB values when `globals.srgb`
// is 1, else linear light), except gradient stops (sRGB, straight alpha), which are
// interpolated in premultiplied sRGB like CSS.

struct Globals {
    viewport: vec2<f32>, // physical pixels
    scale: f32,
    srgb: f32, // 1: the canvas holds and blends sRGB values (`Blending::Srgb`)
};

fn srgb_canvas() -> bool {
    return globals.srgb > 0.5;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3(0.0031308));
}

// A colour sampled from an sRGB texture (decoded to linear), in the canvas's space.
fn sampled(c: vec3<f32>) -> vec3<f32> {
    return select(c, linear_to_srgb(c), srgb_canvas());
}

struct Gradient {
    // x: kind (0 linear, 1 radial, 2 conic), y: stop count,
    // zw: linear direction (unit vector), radial radii, or conic start angle (z, radians).
    info: vec4<f32>,
    // x: linear gradient line length, yz: conic centre from the box's top-left,
    // w: 1 for repeating gradients.
    info2: vec4<f32>,
    colors: array<vec4<f32>, 8>,
    stops: array<vec4<f32>, 2>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<storage, read> gradients: array<Gradient>;
// Glyph atlas pages, read texel-exact with textureLoad.
@group(0) @binding(2) var glyph_mask: texture_2d<f32>;
@group(0) @binding(3) var glyph_color: texture_2d<f32>;
@group(0) @binding(4) var<storage, read> clips: array<Clip>;

const KIND_QUAD: u32 = 0u;
const KIND_OUTER_SHADOW: u32 = 1u;
const KIND_INSET_SHADOW: u32 = 2u;
const KIND_GLYPH: u32 = 4u;
const KIND_BORDER: u32 = 5u;
const KIND_GRADIENT_PIECE: u32 = 6u;
const KIND_WAVY: u32 = 7u;

struct Instance {
    @location(0) rect: vec4<f32>,   // the shape: x, y, w, h
    @location(1) radii: vec4<f32>,  // top-left, top-right, bottom-right, bottom-left
    @location(2) p0: vec4<f32>,     // quad: border widths (t, r, b, l); shadows: second shape rect
    @location(3) p1: vec4<f32>,     // shadows: second shape radii
    @location(4) color0: vec4<f32>, // fill, or shadow colour
    @location(5) color1: vec4<f32>, // border colour
    @location(6) params: vec4<f32>, // x: kind, y: sigma (logical px), z: gradient index or -1,
                                    // w: clip index (1-based, 0 = none)
    @location(7) radii_y: vec4<f32>, // vertical corner radii (elliptical corners)
    @location(8) xf: vec4<f32>,      // x: transform index in the clip table (0 = none)
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) rect: vec4<f32>,
    @location(1) @interpolate(flat) radii: vec4<f32>,
    @location(2) @interpolate(flat) p0: vec4<f32>,
    @location(3) @interpolate(flat) p1: vec4<f32>,
    @location(4) @interpolate(flat) color0: vec4<f32>,
    @location(5) @interpolate(flat) color1: vec4<f32>,
    @location(6) @interpolate(flat) params: vec4<f32>,
    @location(7) @interpolate(flat) radii_y: vec4<f32>,
    @location(8) @interpolate(flat) xf: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let kind = u32(inst.params.x);
    // Grow the drawn area by one physical pixel for anti-aliasing, and by the
    // blur extent for outer shadows.
    var grow = 1.0 / globals.scale;
    if kind == KIND_OUTER_SHADOW {
        grow += 3.0 * inst.params.y;
    }
    if kind == KIND_GLYPH {
        // Glyph quads are pixel-aligned atlas images: no anti-aliasing margin.
        grow = 0.0;
    }
    var area = vec4<f32>(inst.rect.xy - vec2(grow), inst.rect.zw + vec2(2.0 * grow));
    // Don't rasterize what the clip removes anyway (fragments only use `rect`).
    let clip = u32(inst.params.w);
    let xf = u32(inst.xf.x);
    // (The clip bounds are in window space, so this only works untransformed.)
    if clip > 0u && xf == 0u {
        let b = clips[clip - 1u].bounds;
        let lo = max(area.xy, b.xy - vec2(grow));
        let hi = max(lo, min(area.xy + area.zw, b.xy + b.zw + vec2(grow)));
        area = vec4(lo, hi - lo);
    }

    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    var logical = area.xy + corners[vi] * area.zw;
    if xf > 0u {
        // Into the window through the element's plane (straight edges stay straight).
        logical = xf_apply(xf, logical);
    }
    let physical = logical * globals.scale;
    let ndc = vec2(physical.x / globals.viewport.x * 2.0 - 1.0, 1.0 - physical.y / globals.viewport.y * 2.0);

    var out: VsOut;
    out.pos = vec4(ndc, 0.0, 1.0);
    out.rect = inst.rect;
    out.radii = inst.radii;
    out.p0 = inst.p0;
    out.p1 = inst.p1;
    out.color0 = inst.color0;
    out.color1 = inst.color1;
    out.params = inst.params;
    out.radii_y = inst.radii_y;
    out.xf = inst.xf;
    return out;
}

// Picks the corner radius for the quadrant `q` (relative to the centre, y down).
fn corner_radius(q: vec2<f32>, radii: vec4<f32>) -> f32 {
    let top = select(radii.x, radii.y, q.x > 0.0);
    let bottom = select(radii.w, radii.z, q.x > 0.0);
    return select(top, bottom, q.y > 0.0);
}

// Signed distance (logical px) to a rounded rectangle with per-corner radii.
// Elliptical corners (`rx`, `ry`); for circular ones this is the exact distance.
fn sd_rounded_box(p: vec2<f32>, rect: vec4<f32>, rx: vec4<f32>, ry: vec4<f32>) -> f32 {
    let half = rect.zw * 0.5;
    let q0 = p - (rect.xy + half);
    let edge = abs(q0) - half;
    let r = vec2(min(corner_radius(q0, rx), half.x), min(corner_radius(q0, ry), half.y));
    if r.x > 0.0 && r.y > 0.0 {
        let q = edge + r; // relative to the corner's centre
        if q.x > 0.0 && q.y > 0.0 {
            return (length(q / r) - 1.0) * min(r.x, r.y);
        }
    }
    return min(max(edge.x, edge.y), 0.0) + length(max(edge, vec2(0.0)));
}

// Anti-aliased coverage from a signed distance: one physical pixel of feathering.
fn coverage(d: f32) -> f32 {
    return clamp(0.5 - d * globals.scale, 0.0, 1.0);
}

fn shape_coverage(p: vec2<f32>, rect: vec4<f32>, rx: vec4<f32>, ry: vec4<f32>) -> f32 {
    if rect.z <= 0.0 || rect.w <= 0.0 {
        return 0.0;
    }
    return coverage(sd_rounded_box(p, rect, rx, ry));
}

// Inner edge radii, as CSS: horizontal radii lose the left/right width, vertical
// ones the top/bottom (`b` is top, right, bottom, left).
fn inner_rx(rx: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return max(rx - vec4(b.w, b.y, b.y, b.w), vec4(0.0));
}
fn inner_ry(ry: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return max(ry - vec4(b.x, b.x, b.z, b.z), vec4(0.0));
}

// --- Blurred rounded rectangles (Evan Wallace's analytic box shadow) -------

fn erf2(x: vec2<f32>) -> vec2<f32> {
    let s = sign(x);
    let a = abs(x);
    var r = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    r = r * r;
    return s - s / (r * r);
}

fn gaussian(x: f32, sigma: f32) -> f32 {
    let pi = 3.141592653589793;
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (sqrt(2.0 * pi) * sigma);
}

fn shadow_x(x: f32, y: f32, sigma: f32, corner: f32, half: vec2<f32>) -> f32 {
    let delta = min(half.y - corner - abs(y), 0.0);
    let curved = half.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    let integral = 0.5 + 0.5 * erf2((x + vec2(-curved, curved)) * (sqrt(0.5) / sigma));
    return integral.y - integral.x;
}

// Coverage of a rounded rectangle convolved with a Gaussian of `sigma`.
fn blurred_box(p: vec2<f32>, rect: vec4<f32>, radii: vec4<f32>, sigma: f32) -> f32 {
    if rect.z <= 0.0 || rect.w <= 0.0 {
        return 0.0;
    }
    if sigma < 0.01 {
        return shape_coverage(p, rect, radii, radii);
    }
    let half = rect.zw * 0.5;
    let point = p - (rect.xy + half);
    let corner = min(corner_radius(point, radii), min(half.x, half.y));
    let low = point.y - half.y;
    let high = point.y + half.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) / 4.0;
    var y = start + step * 0.5;
    var value = 0.0;
    for (var i = 0; i < 4; i++) {
        value += shadow_x(point.x, point.y - y, sigma, corner, half) * gaussian(y, sigma) * step;
        y += step;
    }
    return clamp(value, 0.0, 1.0);
}

// --- Gradients --------------------------------------------------------------

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(high, low, c <= vec3(0.04045));
}

fn stop_position(g: Gradient, i: u32) -> f32 {
    return g.stops[i / 4u][i % 4u];
}

fn premultiply(c: vec4<f32>) -> vec4<f32> {
    return vec4(c.rgb * c.a, c.a);
}

// Gradient colour at `p` for a box `rect`, premultiplied in the canvas's space.
fn gradient_color(index: u32, p: vec2<f32>, rect: vec4<f32>) -> vec4<f32> {
    let g = gradients[index];
    let center = rect.xy + rect.zw * 0.5;
    var t: f32;
    if g.info.x < 0.5 {
        t = dot(p - center, g.info.zw) / g.info2.x + 0.5;
    } else if g.info.x < 1.5 {
        // Radial: about its own centre (`info2.yz`, relative to the box).
        t = length((p - (rect.xy + g.info2.yz)) / g.info.zw);
    } else {
        // Clockwise from up, starting at the `from` angle, as a fraction of a turn.
        let d = p - (rect.xy + g.info2.yz);
        let turn = 6.283185307;
        t = fract((atan2(d.x, -d.y) - g.info.z) / turn);
    }
    let count = u32(g.info.y);
    if g.info2.w > 0.5 {
        // Repeating: wrap t into the span between the first and last stops.
        let first = stop_position(g, 0u);
        let span = stop_position(g, count - 1u) - first;
        if span > 1e-6 {
            t = first + fract((t - first) / span) * span;
        }
    }
    var c = g.colors[0];
    if t >= stop_position(g, count - 1u) {
        c = g.colors[count - 1u];
    } else if t > stop_position(g, 0u) {
        for (var i = 0u; i + 1u < count; i++) {
            let a = stop_position(g, i);
            let b = stop_position(g, i + 1u);
            if t >= a && t < b {
                let f = select(0.0, (t - a) / (b - a), b > a);
                // Interpolate in premultiplied sRGB, like CSS.
                let m = mix(premultiply(g.colors[i]), premultiply(g.colors[i + 1u]), f);
                c = vec4(select(vec3(0.0), m.rgb / m.a, m.a > 0.0), m.a);
                break;
            }
        }
    }
    if srgb_canvas() {
        return vec4(c.rgb * c.a, c.a);
    }
    return vec4(srgb_to_linear(c.rgb) * c.a, c.a);
}

// --- Styled borders -----------------------------------------------------------
// Borders that aren't one solid colour. `p0` holds the widths (top, right, bottom,
// left); the gradient table entry `params.z` holds the side colours (linear,
// premultiplied) in `colors[0..4]` and the side styles in `info`
// (BorderStyle::index: 0 none, 1 hidden, 2 solid, 3 dashed, 4 dotted, 5 double,
// 6 groove, 7 ridge, 8 inset, 9 outset).

fn styled_border(in: VsOut, p: vec2<f32>) -> vec4<f32> {
    let r = in.rect;
    let b = in.p0;
    let outer = shape_coverage(p, r, in.radii, in.radii_y);
    let inner_rect = vec4(r.x + b.w, r.y + b.x, r.z - b.w - b.y, r.w - b.x - b.z);
    let inner = shape_coverage(p, inner_rect, inner_rx(in.radii, b), inner_ry(in.radii_y, b));
    let ring = outer - min(inner, outer);
    if ring <= 0.0 {
        return vec4(0.0);
    }
    let entry = gradients[u32(in.params.z)];
    // Which side: the nearest outer edge relative to its width, so corners split
    // along the line from the outer to the inner corner, like CSS.
    let d = vec4(p.y - r.y, r.x + r.z - p.x, r.y + r.w - p.y, p.x - r.x);
    let n = select(vec4(1e9), d / max(b, vec4(1e-4)), b > vec4(0.0));
    var side = 0u;
    var best = n.x;
    if n.y < best { side = 1u; best = n.y; }
    if n.z < best { side = 2u; best = n.z; }
    if n.w < best { side = 3u; }
    let w = b[side];
    let t = d[side]; // depth into the border from its outer edge
    // Distance along the side, clockwise, and the side's length.
    var s: f32;
    var len: f32;
    switch side {
        case 0u: { s = p.x - r.x; len = r.z; }
        case 1u: { s = p.y - r.y; len = r.w; }
        case 2u: { s = r.x + r.z - p.x; len = r.z; }
        default: { s = r.y + r.w - p.y; len = r.w; }
    }
    let style = u32(entry.info[side]);
    var color = entry.colors[side];
    // Half as bright in sRGB (0.22 in linear light).
    let dark = vec4(color.rgb * select(0.22, 0.5, srgb_canvas()), color.a);
    let top_left = side == 0u || side == 3u;
    let outer_half = t < w * 0.5;
    var m = 1.0;
    switch style {
        case 0u, 1u: { m = 0.0; }
        case 3u: {
            // Dashes twice as long as the gaps, a whole number per side, centred on
            // the corners so they join there.
            let unit = len / max(1.0, round(len / (3.0 * w)));
            m = select(0.0, 1.0, fract(s / unit + 1.0 / 3.0) < 2.0 / 3.0);
        }
        case 4u: {
            // Round dots one width across, a whole number per side, one per corner.
            let unit = len / max(1.0, round(len / (2.0 * w)));
            let along = (fract(s / unit + 0.5) - 0.5) * unit;
            let dist = length(vec2(along, t - w * 0.5));
            m = clamp((w * 0.5 - dist) * globals.scale + 0.5, 0.0, 1.0);
        }
        case 5u: {
            m = select(0.0, 1.0, t < w / 3.0 || t > w * 2.0 / 3.0);
        }
        case 6u, 7u: {
            // groove: the outer half dark on the top/left sides, the inner half on
            // the bottom/right; ridge the other way round.
            let dark_here = (outer_half == top_left) == (style == 6u);
            color = select(color, dark, dark_here);
        }
        case 8u: { color = select(color, dark, top_left); }
        case 9u: { color = select(dark, color, top_left); }
        default: {}
    }
    return color * ring * m;
}

// --- Glyphs -------------------------------------------------------------------
// `rect` is the destination (logical px, pixel-aligned in physical px), `p0` the
// image in the atlas (px), `params.y` 1 for colour glyphs, `color0` the text colour.

/// Coverage adjustment for text. Blending in linear light makes dark text on
/// light backgrounds look thin; boost coverage for dark text there (a simplified
/// version of what browsers do). Blending in sRGB needs none.
fn text_coverage(a: f32, color: vec4<f32>) -> f32 {
    if srgb_canvas() {
        return a;
    }
    let luma = dot(color.rgb / max(color.a, 1e-4), vec3(0.2126, 0.7152, 0.0722));
    let dark = 1.0 - smoothstep(0.1, 0.6, luma);
    return mix(a, pow(a, 1.0 / 1.6), dark);
}

// `p` is the fragment in the glyph's plane (logical px).
fn glyph(in: VsOut, p: vec2<f32>) -> vec4<f32> {
    let origin = round(in.rect.xy * globals.scale);
    let local = floor(p * globals.scale) - origin;
    let size = in.p0.zw;
    if local.x < 0.0 || local.y < 0.0 || local.x >= size.x || local.y >= size.y {
        discard;
    }
    let texel = vec2<i32>(in.p0.xy + local);
    if in.params.y > 0.5 && in.params.y < 1.5 {
        // Colour glyph (emoji): sRGB texture, decoded to linear on load.
        let c = textureLoad(glyph_color, texel, 0);
        return vec4(sampled(c.rgb) * c.a, c.a) * in.color0.a;
    }
    var a = textureLoad(glyph_mask, texel, 0).r;
    if in.params.y > 1.5 {
        // `font-smooth: never`: hard edges, no coverage boost.
        let crisp = select(0.0, 1.0, a >= 0.5);
        return in.color0 * crisp;
    }
    if in.params.z >= 0.0 {
        // `background-clip: text`: the gradient over the element's box (p1).
        let c = gradient_color(u32(in.params.z), p, in.p1);
        return c * text_coverage(a, c);
    }
    return in.color0 * text_coverage(a, in.color0);
}

// --- Clips (overflow, clip-path) ---------------------------------------------
// A table of rounded rectangles with elliptical corners, or polygons. Entries chain
// to their parent (1-based index, 0 = none); coverage is the product along the chain.

struct Clip {
    rect: vec4<f32>,   // logical px
    rx: vec4<f32>,     // corner radii x: top-left, top-right, bottom-right, bottom-left
    ry: vec4<f32>,     // corner radii y
    bounds: vec4<f32>, // the whole chain's bounding box (x, y, w, h)
    info: vec4<f32>,   // x: parent index (1-based, 0 = none); y: kind (0 box, 1 nonzero
                       // polygon, 2 evenodd) + 4 × transform index; z: first point (vec4
                       // index); w: point count
};

fn clip_corner(q: vec2<f32>, radii: vec4<f32>) -> f32 {
    let top = select(radii.x, radii.y, q.x > 0.0);
    let bottom = select(radii.w, radii.z, q.x > 0.0);
    return select(top, bottom, q.y > 0.0);
}

// Approximate signed distance (logical px) to a box with elliptical corners.
fn sd_clip(p: vec2<f32>, c: Clip) -> f32 {
    let half = c.rect.zw * 0.5;
    let q0 = p - (c.rect.xy + half);
    let edge = abs(q0) - half;
    let r = vec2(min(clip_corner(q0, c.rx), half.x), min(clip_corner(q0, c.ry), half.y));
    if r.x > 0.0 && r.y > 0.0 {
        let q = edge + r; // relative to the corner's centre
        if q.x > 0.0 && q.y > 0.0 {
            return (length(q / r) - 1.0) * min(r.x, r.y);
        }
    }
    return min(max(edge.x, edge.y), 0.0) + length(max(edge, vec2(0.0)));
}

// Transforms (`PushTransform`) are stored in the clip table too: entry i - 1 holds
// the homography from the element's plane to the window and its inverse.
fn xf_apply(i: u32, p: vec2<f32>) -> vec2<f32> {
    let e = clips[i - 1u];
    let x = e.rect.x * p.x + e.rect.y * p.y + e.rect.z;
    let y = e.rect.w * p.x + e.rx.x * p.y + e.rx.y;
    let w = e.rx.z * p.x + e.rx.w * p.y + e.ry.x;
    return vec2(x, y) / max(w, 1e-6);
}

fn xf_unapply(i: u32, p: vec2<f32>) -> vec2<f32> {
    let e = clips[i - 1u];
    let x = e.ry.y * p.x + e.ry.z * p.y + e.ry.w;
    let y = e.bounds.x * p.x + e.bounds.y * p.y + e.bounds.z;
    let w = e.bounds.w * p.x + e.info.x * p.y + e.info.y;
    return vec2(x, y) / max(w, 1e-6);
}

// Polygon clips (`clip-path: polygon()` / `path()`): the points follow the entry
// in the table, one vec4 each (x, y, and their contour's point range), read as a
// flat array of vec4s.
fn clip_word(i: u32) -> vec4<f32> {
    let e = clips[i / 5u];
    switch i % 5u {
        case 0u: { return e.rect; }
        case 1u: { return e.rx; }
        case 2u: { return e.ry; }
        case 3u: { return e.bounds; }
        default: { return e.info; }
    }
}

// Signed distance to the polygon's edges: negative inside, by its fill rule.
fn sd_polygon(p: vec2<f32>, c: Clip, even_odd: bool) -> f32 {
    let first = u32(c.info.z);
    let count = u32(c.info.w);
    var d = 1e9;
    var winding = 0;
    var crossings = 0u;
    for (var k = 0u; k < count; k++) {
        let a4 = clip_word(first + k);
        let next = select(u32(a4.z), k + 1u, k + 1u < u32(a4.w));
        let a = a4.xy;
        let b = clip_word(first + next).xy;
        let e = b - a;
        let w = p - a;
        let t = clamp(dot(w, e) / max(dot(e, e), 1e-8), 0.0, 1.0);
        d = min(d, length(w - e * t));
        let left = e.x * w.y - e.y * w.x;
        if a.y <= p.y && b.y > p.y && left > 0.0 {
            winding += 1;
        } else if a.y > p.y && b.y <= p.y && left < 0.0 {
            winding -= 1;
        }
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * e.x / e.y {
            crossings += 1u;
        }
    }
    let inside = select(winding != 0, (crossings & 1u) == 1u, even_odd);
    return select(d, -d, inside);
}

fn clip_coverage_with(p: vec2<f32>, index: u32, scale: f32) -> f32 {
    var i = index;
    var cover = 1.0;
    for (var n = 0; n < 32 && i > 0u && cover > 0.0; n++) {
        let c = clips[i - 1u];
        // info.y: the shape kind (0 box, 1 nonzero polygon, 2 evenodd) + 4 × the
        // transform its shape is in.
        let kind = u32(c.info.y) % 4u;
        let t = u32(c.info.y) / 4u;
        var q = p;
        if t > 0u {
            q = xf_unapply(t, p);
        }
        var d: f32;
        if kind > 0u {
            d = sd_polygon(q, c, kind == 2u);
        } else {
            d = sd_clip(q, c);
        }
        cover *= clamp(0.5 - d * scale, 0.0, 1.0);
        i = u32(c.info.x);
    }
    return cover;
}

fn clip_coverage(p: vec2<f32>, params: vec4<f32>) -> f32 {
    return clip_coverage_with(p, u32(params.w), globals.scale);
}

// --- Fragment ---------------------------------------------------------------

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let world = in.pos.xy / globals.scale;
    let clip = clip_coverage(world, in.params);
    if clip <= 0.0 {
        discard;
    }
    return shade(in, local_point(in, world)) * clip;
}

// The fragment in the instance's own plane.
fn local_point(in: VsOut, world: vec2<f32>) -> vec2<f32> {
    let xf = u32(in.xf.x);
    if xf > 0u {
        return xf_unapply(xf, world);
    }
    return world;
}

fn shade(in: VsOut, p: vec2<f32>) -> vec4<f32> {
    let kind = u32(in.params.x);
    let sigma = in.params.y;

    if kind == KIND_OUTER_SHADOW {
        // CSS clips outer shadows to outside the box that casts them.
        let shadow = blurred_box(p, in.rect, in.radii, sigma);
        return in.color0 * shadow * (1.0 - shape_coverage(p, in.p0, in.p1, in.p1));
    }
    if kind == KIND_GLYPH {
        return glyph(in, p);
    }
    if kind == KIND_BORDER {
        return styled_border(in, p);
    }
    if kind == KIND_WAVY {
        // A wavy text decoration: a sine wave `params.y` thick swinging across `rect`.
        let t = max(in.params.y, 1.0 / globals.scale);
        let amp = max(in.rect.w - t, 0.0) * 0.5;
        let k = 6.2831853 / max(4.0 * t, 4.0);
        let x = p.x - in.rect.x;
        let y = in.rect.y + in.rect.w * 0.5 + amp * sin(x * k);
        let slope = amp * k * cos(x * k);
        let d = abs(p.y - y) / sqrt(1.0 + slope * slope) - t * 0.5;
        let inside = select(0.0, 1.0, x >= 0.0 && x <= in.rect.z);
        return in.color0 * coverage(d) * inside;
    }
    if kind == KIND_GRADIENT_PIECE {
        // A border-image piece cut from a gradient: `p0` is the source part (absolute
        // px), stretched over `rect`; `p1` the area the gradient spans.
        let inside = shape_coverage(p, in.rect, vec4(0.0), vec4(0.0));
        let src = in.p0.xy + (p - in.rect.xy) * in.p0.zw / max(in.rect.zw, vec2(1e-4));
        return gradient_color(u32(in.params.z), src, in.p1) * inside;
    }
    if kind == KIND_INSET_SHADOW {
        // Drawn inside the padding box, everywhere except the (blurred) hole.
        let inside = shape_coverage(p, in.rect, in.radii, in.radii_y);
        return in.color0 * inside * (1.0 - blurred_box(p, in.p0, in.p1, sigma));
    }

    // A box: fill (solid colour with an optional gradient over it), then border.
    let outer = shape_coverage(p, in.rect, in.radii, in.radii_y);
    if outer <= 0.0 {
        return vec4(0.0);
    }
    let b = in.p0; // border widths: top, right, bottom, left
    let inner_rect = vec4(
        in.rect.x + b.w,
        in.rect.y + b.x,
        in.rect.z - b.w - b.y,
        in.rect.w - b.x - b.z,
    );
    let inner = min(
        shape_coverage(p, inner_rect, inner_rx(in.radii, b), inner_ry(in.radii_y, b)),
        outer,
    );

    var fill = in.color0;
    let gradient = in.params.z;
    if gradient >= 0.0 {
        // A background gradient layer: `p1` is one tile, repeating along x when
        // `params.y` has bit 1 and along y with bit 2.
        var at = p;
        var tile = in.rect;
        var inside = true;
        if in.p1.z > 0.0 && in.p1.w > 0.0 {
            tile = in.p1;
            let bits = u32(in.params.y);
            let local = p - tile.xy;
            let wrapped = local - floor(local / tile.zw) * tile.zw;
            let rx = (bits & 1u) != 0u;
            let ry = (bits & 2u) != 0u;
            at = tile.xy + vec2(select(local.x, wrapped.x, rx), select(local.y, wrapped.y, ry));
            inside = (rx || (local.x >= 0.0 && local.x <= tile.z))
                && (ry || (local.y >= 0.0 && local.y <= tile.w));
        }
        if inside {
            let g = gradient_color(u32(gradient), at, tile);
            fill = g + fill * (1.0 - g.a);
        }
    }
    // CSS backgrounds extend under the border (background-clip: border-box).
    let base = fill * outer;
    let border = in.color1 * (outer - inner);
    return border + base * (1.0 - border.a);
}

// --- Background images ---------------------------------------------------------
// Drawn by a separate pipeline (one texture per draw). The instance's `rect` and
// `radii` are the box (the clip shape), `p0` is one tile of the image in logical
// px, and `params.yz` say whether it repeats along x and y.

@group(1) @binding(0) var image_tex: texture_2d<f32>;
@group(1) @binding(1) var image_samp: sampler;

@fragment
fn fs_image(in: VsOut) -> @location(0) vec4<f32> {
    let world = in.pos.xy / globals.scale;
    let p = local_point(in, world);
    let clip = shape_coverage(p, in.rect, in.radii, in.radii_y) * clip_coverage(world, in.params);
    if clip <= 0.0 {
        discard;
    }
    let tile = in.p0;
    var uv = (p - tile.xy) / tile.zw;
    let repeat_x = in.params.y > 0.5;
    let repeat_y = in.params.z > 0.5;
    // `background-repeat: space`: tiles are `p1.xy` apart; nothing shows in the gaps.
    let gap = in.p1.xy;
    let period = tile.zw + gap;
    let local = p - tile.xy;
    let wrapped = local - floor(local / period) * period;
    if (repeat_x && gap.x > 0.0 && wrapped.x > tile.z) || (repeat_y && gap.y > 0.0 && wrapped.y > tile.w) {
        discard;
    }
    uv = select(uv, wrapped / tile.zw, vec2(repeat_x && gap.x > 0.0, repeat_y && gap.y > 0.0));
    if (!repeat_x && (uv.x < 0.0 || uv.x > 1.0)) || (!repeat_y && (uv.y < 0.0 || uv.y > 1.0)) {
        discard;
    }
    let half_texel = 0.5 / vec2<f32>(textureDimensions(image_tex));
    if in.color0.z > 0.0 {
        // A border-image piece: `color0` is its part of the texture (uv). Wrap within
        // the piece and keep filtering inside it.
        let sub = in.color0;
        let t = select(uv, fract(uv), vec2(repeat_x, repeat_y));
        let at = clamp(sub.xy + t * sub.zw, sub.xy + half_texel, sub.xy + sub.zw - half_texel);
        let c = textureSampleLevel(image_tex, image_samp, at, 0.0);
        return vec4(sampled(c.rgb) * c.a, c.a) * image_tint(in.color1) * clip;
    }
    // The sampler repeats. On axes that don't repeat, keep bilinear filtering
    // from wrapping around to the opposite edge. sRGB textures sample as linear.
    let clamped = clamp(uv, half_texel, vec2(1.0) - half_texel);
    let sample_uv = vec2(select(clamped.x, uv.x, repeat_x), select(clamped.y, uv.y, repeat_y));
    let c = textureSampleLevel(image_tex, image_samp, sample_uv, 0.0);
    return vec4(sampled(c.rgb) * c.a, c.a) * image_tint(in.color1) * clip;
}

// `background-image-tint` / `border-image-tint` (linear, premultiplied); all zeros
// means no tint.
fn image_tint(t: vec4<f32>) -> vec4<f32> {
    return select(t, vec4(1.0), all(t == vec4(0.0)));
}
