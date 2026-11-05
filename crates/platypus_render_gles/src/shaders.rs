// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! GLSL ES 3.00 ports of the UI shaders (`web_ui_render/src/ui/quad.wgsl`, `effects.wgsl`).
//!
//! GLES 3.0 has no storage buffers, so the gradient and clip tables the wgpu shaders read as
//! arrays are float data textures here: one table entry per row, `u_grads` 12 texels wide and
//! `u_clips` 5 (the `GpuGradient` and `GpuClip` fields in order, as `vec4`s). Geometry is in
//! logical pixels with y down, as in wgpu: the vertex shader maps it to clip space itself, and
//! the fragment shader turns `gl_FragCoord` (y up) back into logical pixels.

/// What both quad stages read: uniforms, the table accessors and the clip transforms.
pub(crate) const QUAD_COMMON: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2D;

uniform vec4 u_globals;     // x, y: viewport (physical px), z: scale, w: 1 when the canvases
                            // hold sRGB values (`Blending::Srgb`)
uniform sampler2D u_clips;  // 5 texels per entry: rect, rx, ry, bounds, info
uniform sampler2D u_grads;  // 12 texels per entry: info, info2, colors[8], stops[2]
uniform sampler2D u_mask;   // glyph coverage page (R8)
uniform sampler2D u_color;  // glyph colour page (sRGB)

bool srgb_canvas() {
    return u_globals.w > 0.5;
}

vec3 linear_to_srgb(vec3 c) {
    vec3 low = c * 12.92;
    vec3 high = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return mix(high, low, vec3(lessThanEqual(c, vec3(0.0031308))));
}

// A colour sampled from an sRGB texture (decoded to linear), in the canvas's space.
vec3 sampled(vec3 c) {
    return srgb_canvas() ? linear_to_srgb(c) : c;
}

const uint KIND_QUAD = 0u;
const uint KIND_OUTER_SHADOW = 1u;
const uint KIND_INSET_SHADOW = 2u;
const uint KIND_GLYPH = 4u;
const uint KIND_BORDER = 5u;
const uint KIND_GRADIENT_PIECE = 6u;
const uint KIND_WAVY = 7u;

vec4 clip_texel(uint entry, uint k) {
    return texelFetch(u_clips, ivec2(int(k), int(entry)), 0);
}

// Transforms (`PushTransform`) live in the clip table: entry i - 1 holds the homography
// from the element's plane to the window and its inverse.
vec2 xf_apply(uint i, vec2 p) {
    vec4 e0 = clip_texel(i - 1u, 0u);
    vec4 e1 = clip_texel(i - 1u, 1u);
    vec4 e2 = clip_texel(i - 1u, 2u);
    float x = e0.x * p.x + e0.y * p.y + e0.z;
    float y = e0.w * p.x + e1.x * p.y + e1.y;
    float w = e1.z * p.x + e1.w * p.y + e2.x;
    return vec2(x, y) / max(w, 1e-6);
}

vec2 xf_unapply(uint i, vec2 p) {
    vec4 e2 = clip_texel(i - 1u, 2u);
    vec4 e3 = clip_texel(i - 1u, 3u);
    vec4 e4 = clip_texel(i - 1u, 4u);
    float x = e2.y * p.x + e2.z * p.y + e2.w;
    float y = e3.x * p.x + e3.y * p.y + e3.z;
    float w = e3.w * p.x + e4.x * p.y + e4.y;
    return vec2(x, y) / max(w, 1e-6);
}
"#;

pub fn quad_vertex() -> String {
    format!(
        "{QUAD_COMMON}{}",
        r#"
layout(location = 0) in vec4 a_rect;
layout(location = 1) in vec4 a_radii;
layout(location = 2) in vec4 a_p0;
layout(location = 3) in vec4 a_p1;
layout(location = 4) in vec4 a_color0;
layout(location = 5) in vec4 a_color1;
layout(location = 6) in vec4 a_params;
layout(location = 7) in vec4 a_radii_y;
layout(location = 8) in vec4 a_xf;

flat out vec4 v_rect;
flat out vec4 v_radii;
flat out vec4 v_p0;
flat out vec4 v_p1;
flat out vec4 v_color0;
flat out vec4 v_color1;
flat out vec4 v_params;
flat out vec4 v_radii_y;
flat out vec4 v_xf;

void main() {
    uint kind = uint(a_params.x);
    // Grow the drawn area by one physical pixel for anti-aliasing, and by the blur extent
    // for outer shadows. Glyph quads are pixel-aligned atlas images: no margin.
    float grow = 1.0 / u_globals.z;
    if (kind == KIND_OUTER_SHADOW) {
        grow += 3.0 * a_params.y;
    }
    if (kind == KIND_GLYPH) {
        grow = 0.0;
    }
    vec4 area = vec4(a_rect.xy - vec2(grow), a_rect.zw + vec2(2.0 * grow));
    // Don't rasterize what the clip removes anyway (fragments only use `rect`). The clip
    // bounds are in window space, so this only works untransformed.
    uint clip = uint(a_params.w);
    uint xf = uint(a_xf.x);
    if (clip > 0u && xf == 0u) {
        vec4 b = clip_texel(clip - 1u, 3u);
        vec2 lo = max(area.xy, b.xy - vec2(grow));
        vec2 hi = max(lo, min(area.xy + area.zw, b.xy + b.zw + vec2(grow)));
        area = vec4(lo, hi - lo);
    }
    vec2 corners[6] = vec2[6](
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0)
    );
    vec2 logical = area.xy + corners[gl_VertexID] * area.zw;
    if (xf > 0u) {
        // Into the window through the element's plane (straight edges stay straight).
        logical = xf_apply(xf, logical);
    }
    vec2 physical = logical * u_globals.z;
    vec2 ndc = vec2(physical.x / u_globals.x * 2.0 - 1.0, 1.0 - physical.y / u_globals.y * 2.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_rect = a_rect;
    v_radii = a_radii;
    v_p0 = a_p0;
    v_p1 = a_p1;
    v_color0 = a_color0;
    v_color1 = a_color1;
    v_params = a_params;
    v_radii_y = a_radii_y;
    v_xf = a_xf;
}
"#
    )
}

/// Pure helpers (no instance varyings) the quad and effect fragment programs share: shape
/// coverage and the clip table's coverage.
pub(crate) const GEOMETRY: &str = r#"// Picks the corner radius for the quadrant `q` (relative to the centre, y down).
float corner_radius(vec2 q, vec4 radii) {
    float top = q.x > 0.0 ? radii.y : radii.x;
    float bottom = q.x > 0.0 ? radii.z : radii.w;
    return q.y > 0.0 ? bottom : top;
}

// Signed distance (logical px) to a rounded rectangle with per-corner radii. Elliptical
// corners (`rx`, `ry`); for circular ones this is the exact distance.
float sd_rounded_box(vec2 p, vec4 rect, vec4 rx, vec4 ry) {
    vec2 half_size = rect.zw * 0.5;
    vec2 q0 = p - (rect.xy + half_size);
    vec2 edge = abs(q0) - half_size;
    vec2 r = vec2(min(corner_radius(q0, rx), half_size.x), min(corner_radius(q0, ry), half_size.y));
    if (r.x > 0.0 && r.y > 0.0) {
        vec2 q = edge + r; // relative to the corner's centre
        if (q.x > 0.0 && q.y > 0.0) {
            return (length(q / r) - 1.0) * min(r.x, r.y);
        }
    }
    return min(max(edge.x, edge.y), 0.0) + length(max(edge, vec2(0.0)));
}

// Anti-aliased coverage from a signed distance: one physical pixel of feathering.
float coverage(float d) {
    return clamp(0.5 - d * u_globals.z, 0.0, 1.0);
}

float shape_coverage(vec2 p, vec4 rect, vec4 rx, vec4 ry) {
    if (rect.z <= 0.0 || rect.w <= 0.0) {
        return 0.0;
    }
    return coverage(sd_rounded_box(p, rect, rx, ry));
}

// --- Clips (overflow, clip-path) -----------------------------------------------------
// A table of rounded rectangles with elliptical corners, or polygons. Entries chain to
// their parent (1-based index, 0 = none); coverage is the product along the chain.

struct Clip {
    vec4 rect;   // logical px
    vec4 rx;     // corner radii x: top-left, top-right, bottom-right, bottom-left
    vec4 ry;     // corner radii y
    vec4 bounds; // the whole chain's bounding box (x, y, w, h)
    vec4 info;   // x: parent; y: kind (0 box, 1 nonzero polygon, 2 evenodd) + 4 x transform; z: first point; w: point count
};

Clip get_clip(uint i) {
    return Clip(clip_texel(i, 0u), clip_texel(i, 1u), clip_texel(i, 2u), clip_texel(i, 3u), clip_texel(i, 4u));
}

// Approximate signed distance (logical px) to a box with elliptical corners.
float sd_clip(vec2 p, Clip c) {
    return sd_rounded_box(p, c.rect, c.rx, c.ry);
}

// Polygon clips: the points follow the entry in the table, one vec4 each (x, y, and their
// contour's point range), read as a flat array of vec4s.
vec4 clip_word(uint i) {
    return clip_texel(i / 5u, i % 5u);
}

// Signed distance to the polygon's edges: negative inside, by its fill rule.
float sd_polygon(vec2 p, Clip c, bool even_odd) {
    uint first = uint(c.info.z);
    uint count = uint(c.info.w);
    float d = 1e9;
    int winding = 0;
    uint crossings = 0u;
    for (uint k = 0u; k < count; k++) {
        vec4 a4 = clip_word(first + k);
        uint next = (k + 1u < uint(a4.w)) ? k + 1u : uint(a4.z);
        vec2 a = a4.xy;
        vec2 b = clip_word(first + next).xy;
        vec2 e = b - a;
        vec2 w = p - a;
        float t = clamp(dot(w, e) / max(dot(e, e), 1e-8), 0.0, 1.0);
        d = min(d, length(w - e * t));
        float left = e.x * w.y - e.y * w.x;
        if (a.y <= p.y && b.y > p.y && left > 0.0) {
            winding += 1;
        } else if (a.y > p.y && b.y <= p.y && left < 0.0) {
            winding -= 1;
        }
        if ((a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * e.x / e.y) {
            crossings += 1u;
        }
    }
    bool inside = even_odd ? ((crossings & 1u) == 1u) : (winding != 0);
    return inside ? -d : d;
}

float clip_coverage_with(vec2 p, uint index) {
    uint i = index;
    float cover = 1.0;
    for (int n = 0; n < 32 && i > 0u && cover > 0.0; n++) {
        Clip c = get_clip(i - 1u);
        // info.y: the shape kind (0 box, 1 nonzero polygon, 2 evenodd) + 4 x the transform
        // its shape is in.
        uint kind = uint(c.info.y) % 4u;
        uint t = uint(c.info.y) / 4u;
        vec2 q = p;
        if (t > 0u) {
            q = xf_unapply(t, p);
        }
        float d;
        if (kind > 0u) {
            d = sd_polygon(q, c, kind == 2u);
        } else {
            d = sd_clip(q, c);
        }
        cover *= clamp(0.5 - d * u_globals.z, 0.0, 1.0);
        i = uint(c.info.x);
    }
    return cover;
}

"#;

/// The fragment helpers both quad programs share: shapes, clips, gradients, glyphs, `shade`.
const FRAGMENT_BODY: &str = r#"
flat in vec4 v_rect;
flat in vec4 v_radii;
flat in vec4 v_p0;
flat in vec4 v_p1;
flat in vec4 v_color0;
flat in vec4 v_color1;
flat in vec4 v_params;
flat in vec4 v_radii_y;
flat in vec4 v_xf;

layout(location = 0) out vec4 o_color;

// Inner edge radii, as CSS: horizontal radii lose the left/right width, vertical ones the
// top/bottom (`b` is top, right, bottom, left).
vec4 inner_rx(vec4 rx, vec4 b) {
    return max(rx - vec4(b.w, b.y, b.y, b.w), vec4(0.0));
}
vec4 inner_ry(vec4 ry, vec4 b) {
    return max(ry - vec4(b.x, b.x, b.z, b.z), vec4(0.0));
}

// --- Blurred rounded rectangles (Evan Wallace's analytic box shadow) ---------

vec2 erf2(vec2 x) {
    vec2 s = sign(x);
    vec2 a = abs(x);
    vec2 r = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    r = r * r;
    return s - s / (r * r);
}

float gaussian(float x, float sigma) {
    float pi = 3.141592653589793;
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (sqrt(2.0 * pi) * sigma);
}

float shadow_x(float x, float y, float sigma, float corner, vec2 half_size) {
    float delta = min(half_size.y - corner - abs(y), 0.0);
    float curved = half_size.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    vec2 integral = 0.5 + 0.5 * erf2((x + vec2(-curved, curved)) * (sqrt(0.5) / sigma));
    return integral.y - integral.x;
}

// Coverage of a rounded rectangle convolved with a Gaussian of `sigma`.
float blurred_box(vec2 p, vec4 rect, vec4 radii, float sigma) {
    if (rect.z <= 0.0 || rect.w <= 0.0) {
        return 0.0;
    }
    if (sigma < 0.01) {
        return shape_coverage(p, rect, radii, radii);
    }
    vec2 half_size = rect.zw * 0.5;
    vec2 point = p - (rect.xy + half_size);
    float corner = min(corner_radius(point, radii), min(half_size.x, half_size.y));
    float low = point.y - half_size.y;
    float high = point.y + half_size.y;
    float start = clamp(-3.0 * sigma, low, high);
    float end = clamp(3.0 * sigma, low, high);
    float step_y = (end - start) / 4.0;
    float y = start + step_y * 0.5;
    float value = 0.0;
    for (int i = 0; i < 4; i++) {
        value += shadow_x(point.x, point.y - y, sigma, corner, half_size) * gaussian(y, sigma) * step_y;
        y += step_y;
    }
    return clamp(value, 0.0, 1.0);
}

// --- Gradients ---------------------------------------------------------------

vec3 srgb_to_linear(vec3 c) {
    vec3 low = c / 12.92;
    vec3 high = pow((c + 0.055) / 1.055, vec3(2.4));
    return mix(high, low, vec3(lessThanEqual(c, vec3(0.04045))));
}

vec4 grad_texel(uint g, uint k) {
    return texelFetch(u_grads, ivec2(int(k), int(g)), 0);
}

float stop_position(uint g, uint i) {
    return grad_texel(g, 10u + i / 4u)[i % 4u];
}

vec4 premultiply(vec4 c) {
    return vec4(c.rgb * c.a, c.a);
}

// Gradient colour at `p` for a box `rect`, premultiplied in the canvas's space.
vec4 gradient_color(uint index, vec2 p, vec4 rect) {
    vec4 info = grad_texel(index, 0u);
    vec4 info2 = grad_texel(index, 1u);
    vec2 center = rect.xy + rect.zw * 0.5;
    float t;
    if (info.x < 0.5) {
        t = dot(p - center, info.zw) / info2.x + 0.5;
    } else if (info.x < 1.5) {
        // Radial: about its own centre (`info2.yz`, relative to the box), as the wgpu shader.
        t = length((p - (rect.xy + info2.yz)) / info.zw);
    } else {
        // Clockwise from up, starting at the `from` angle, as a fraction of a turn.
        vec2 d = p - (rect.xy + info2.yz);
        float turn = 6.283185307;
        t = fract((atan(d.x, -d.y) - info.z) / turn);
    }
    uint count = uint(info.y);
    if (info2.w > 0.5) {
        // Repeating: wrap t into the span between the first and last stops.
        float first = stop_position(index, 0u);
        float span = stop_position(index, count - 1u) - first;
        if (span > 1e-6) {
            t = first + fract((t - first) / span) * span;
        }
    }
    vec4 c = grad_texel(index, 2u);
    if (t >= stop_position(index, count - 1u)) {
        c = grad_texel(index, 2u + count - 1u);
    } else if (t > stop_position(index, 0u)) {
        for (uint i = 0u; i + 1u < count; i++) {
            float a = stop_position(index, i);
            float b = stop_position(index, i + 1u);
            if (t >= a && t < b) {
                float f = b > a ? (t - a) / (b - a) : 0.0;
                // Interpolate in premultiplied sRGB, like CSS.
                vec4 m = mix(premultiply(grad_texel(index, 2u + i)), premultiply(grad_texel(index, 3u + i)), f);
                c = vec4(m.a > 0.0 ? m.rgb / m.a : vec3(0.0), m.a);
                break;
            }
        }
    }
    if (srgb_canvas()) {
        return vec4(c.rgb * c.a, c.a);
    }
    return vec4(srgb_to_linear(c.rgb) * c.a, c.a);
}

// --- Styled borders ------------------------------------------------------------
// `p0` holds the widths (top, right, bottom, left); the gradient table entry `params.z`
// holds the side colours (linear, premultiplied) in `colors[0..4]` and the side styles in
// `info` (BorderStyle::index: 0 none, 1 hidden, 2 solid, 3 dashed, 4 dotted, 5 double,
// 6 groove, 7 ridge, 8 inset, 9 outset).

vec4 styled_border(vec2 p) {
    vec4 r = v_rect;
    vec4 b = v_p0;
    float outer = shape_coverage(p, r, v_radii, v_radii_y);
    vec4 inner_rect = vec4(r.x + b.w, r.y + b.x, r.z - b.w - b.y, r.w - b.x - b.z);
    float inner = shape_coverage(p, inner_rect, inner_rx(v_radii, b), inner_ry(v_radii_y, b));
    float ring = outer - min(inner, outer);
    if (ring <= 0.0) {
        return vec4(0.0);
    }
    uint entry = uint(v_params.z);
    // Which side: the nearest outer edge relative to its width, so corners split along the
    // line from the outer to the inner corner, like CSS.
    vec4 d = vec4(p.y - r.y, r.x + r.z - p.x, r.y + r.w - p.y, p.x - r.x);
    vec4 bm = max(b, vec4(1e-4));
    vec4 n = vec4(b.x > 0.0 ? d.x / bm.x : 1e9, b.y > 0.0 ? d.y / bm.y : 1e9,
                  b.z > 0.0 ? d.z / bm.z : 1e9, b.w > 0.0 ? d.w / bm.w : 1e9);
    uint side = 0u;
    float best = n.x;
    if (n.y < best) { side = 1u; best = n.y; }
    if (n.z < best) { side = 2u; best = n.z; }
    if (n.w < best) { side = 3u; }
    float w = b[side];
    float t = d[side]; // depth into the border from its outer edge
    // Distance along the side, clockwise, and the side's length.
    float s;
    float len;
    switch (side) {
        case 0u: s = p.x - r.x; len = r.z; break;
        case 1u: s = p.y - r.y; len = r.w; break;
        case 2u: s = r.x + r.z - p.x; len = r.z; break;
        default: s = r.y + r.w - p.y; len = r.w; break;
    }
    uint style = uint(grad_texel(entry, 0u)[side]);
    vec4 color = grad_texel(entry, 2u + side);
    // Half as bright in sRGB (0.22 in linear light).
    vec4 dark = vec4(color.rgb * (srgb_canvas() ? 0.5 : 0.22), color.a);
    bool top_left = side == 0u || side == 3u;
    bool outer_half = t < w * 0.5;
    float m = 1.0;
    switch (style) {
        case 0u:
        case 1u:
            m = 0.0;
            break;
        case 3u: {
            // Dashes twice as long as the gaps, a whole number per side, centred on the
            // corners so they join there.
            float unit = len / max(1.0, round(len / (3.0 * w)));
            m = fract(s / unit + 1.0 / 3.0) < 2.0 / 3.0 ? 1.0 : 0.0;
            break;
        }
        case 4u: {
            // Round dots one width across, a whole number per side, one per corner.
            float unit = len / max(1.0, round(len / (2.0 * w)));
            float along = (fract(s / unit + 0.5) - 0.5) * unit;
            float dist = length(vec2(along, t - w * 0.5));
            m = clamp((w * 0.5 - dist) * u_globals.z + 0.5, 0.0, 1.0);
            break;
        }
        case 5u:
            m = (t < w / 3.0 || t > w * 2.0 / 3.0) ? 1.0 : 0.0;
            break;
        case 6u:
        case 7u: {
            // groove: the outer half dark on the top/left sides, the inner half on the
            // bottom/right; ridge the other way round.
            bool dark_here = (outer_half == top_left) == (style == 6u);
            color = dark_here ? dark : color;
            break;
        }
        case 8u:
            color = top_left ? dark : color;
            break;
        case 9u:
            color = top_left ? color : dark;
            break;
        default:
            break;
    }
    return color * ring * m;
}

// --- Glyphs ----------------------------------------------------------------------
// `rect` is the destination (logical px, pixel-aligned in physical px), `p0` the image in
// the atlas (px), `params.y` 1 for colour glyphs, `color0` the text colour.

// Coverage adjustment for text. Blending in linear light makes dark text on light
// backgrounds look thin; boost coverage for dark text there. Blending in sRGB needs none.
float text_coverage(float a, vec4 color) {
    if (srgb_canvas()) {
        return a;
    }
    float luma = dot(color.rgb / max(color.a, 1e-4), vec3(0.2126, 0.7152, 0.0722));
    float dark = 1.0 - smoothstep(0.1, 0.6, luma);
    return mix(a, pow(a, 1.0 / 1.6), dark);
}

// `p` is the fragment in the glyph's plane (logical px).
vec4 glyph(vec2 p) {
    vec2 origin = round(v_rect.xy * u_globals.z);
    vec2 local = floor(p * u_globals.z) - origin;
    vec2 size = v_p0.zw;
    if (local.x < 0.0 || local.y < 0.0 || local.x >= size.x || local.y >= size.y) {
        discard;
    }
    ivec2 texel = ivec2(v_p0.xy + local);
    if (v_params.y > 0.5 && v_params.y < 1.5) {
        // Colour glyph (emoji): sRGB texture, decoded to linear on load.
        vec4 c = texelFetch(u_color, texel, 0);
        return vec4(sampled(c.rgb) * c.a, c.a) * v_color0.a;
    }
    float a = texelFetch(u_mask, texel, 0).r;
    if (v_params.y > 1.5) {
        // `font-smooth: never`: hard edges, no coverage boost.
        float crisp = a >= 0.5 ? 1.0 : 0.0;
        return v_color0 * crisp;
    }
    if (v_params.z >= 0.0) {
        // `background-clip: text`: the gradient over the element's box (p1).
        vec4 c = gradient_color(uint(v_params.z), p, v_p1);
        return c * text_coverage(a, c);
    }
    return v_color0 * text_coverage(a, v_color0);
}

float clip_coverage(vec2 p, vec4 params) {
    return clip_coverage_with(p, uint(params.w));
}

// The fragment in the instance's own plane.
vec2 local_point(vec2 world) {
    uint xf = uint(v_xf.x);
    if (xf > 0u) {
        return xf_unapply(xf, world);
    }
    return world;
}

vec4 shade(vec2 p) {
    uint kind = uint(v_params.x);
    float sigma = v_params.y;

    if (kind == KIND_OUTER_SHADOW) {
        // CSS clips outer shadows to outside the box that casts them.
        float shadow = blurred_box(p, v_rect, v_radii, sigma);
        return v_color0 * shadow * (1.0 - shape_coverage(p, v_p0, v_p1, v_p1));
    }
    if (kind == KIND_GLYPH) {
        return glyph(p);
    }
    if (kind == KIND_BORDER) {
        return styled_border(p);
    }
    if (kind == KIND_WAVY) {
        // A wavy text decoration: a sine wave `params.y` thick swinging across `rect`.
        float t = max(v_params.y, 1.0 / u_globals.z);
        float amp = max(v_rect.w - t, 0.0) * 0.5;
        float k = 6.2831853 / max(4.0 * t, 4.0);
        float x = p.x - v_rect.x;
        float y = v_rect.y + v_rect.w * 0.5 + amp * sin(x * k);
        float slope = amp * k * cos(x * k);
        float d = abs(p.y - y) / sqrt(1.0 + slope * slope) - t * 0.5;
        float inside = (x >= 0.0 && x <= v_rect.z) ? 1.0 : 0.0;
        return v_color0 * coverage(d) * inside;
    }
    if (kind == KIND_GRADIENT_PIECE) {
        // A border-image piece cut from a gradient: `p0` is the source part (absolute px),
        // stretched over `rect`; `p1` the area the gradient spans.
        float inside = shape_coverage(p, v_rect, vec4(0.0), vec4(0.0));
        vec2 src = v_p0.xy + (p - v_rect.xy) * v_p0.zw / max(v_rect.zw, vec2(1e-4));
        return gradient_color(uint(v_params.z), src, v_p1) * inside;
    }
    if (kind == KIND_INSET_SHADOW) {
        // Drawn inside the padding box, everywhere except the (blurred) hole.
        float inside = shape_coverage(p, v_rect, v_radii, v_radii_y);
        return v_color0 * inside * (1.0 - blurred_box(p, v_p0, v_p1, sigma));
    }

    // A box: fill (solid colour with an optional gradient over it), then border.
    float outer = shape_coverage(p, v_rect, v_radii, v_radii_y);
    if (outer <= 0.0) {
        return vec4(0.0);
    }
    vec4 b = v_p0; // border widths: top, right, bottom, left
    vec4 inner_rect = vec4(v_rect.x + b.w, v_rect.y + b.x, v_rect.z - b.w - b.y, v_rect.w - b.x - b.z);
    float inner = min(shape_coverage(p, inner_rect, inner_rx(v_radii, b), inner_ry(v_radii_y, b)), outer);

    vec4 fill = v_color0;
    float gradient = v_params.z;
    if (gradient >= 0.0) {
        // A background gradient layer: `p1` is one tile, repeating along x when `params.y`
        // has bit 1 and along y with bit 2.
        vec2 at = p;
        vec4 tile = v_rect;
        bool inside = true;
        if (v_p1.z > 0.0 && v_p1.w > 0.0) {
            tile = v_p1;
            uint bits = uint(v_params.y);
            vec2 local = p - tile.xy;
            vec2 wrapped = local - floor(local / tile.zw) * tile.zw;
            bool rx = (bits & 1u) != 0u;
            bool ry = (bits & 2u) != 0u;
            at = tile.xy + vec2(rx ? wrapped.x : local.x, ry ? wrapped.y : local.y);
            inside = (rx || (local.x >= 0.0 && local.x <= tile.z))
                && (ry || (local.y >= 0.0 && local.y <= tile.w));
        }
        if (inside) {
            vec4 g = gradient_color(uint(gradient), at, tile);
            fill = g + fill * (1.0 - g.a);
        }
    }
    // CSS backgrounds extend under the border (background-clip: border-box).
    vec4 base = fill * outer;
    vec4 border = v_color1 * (outer - inner);
    return border + base * (1.0 - border.a);
}

"#;

const QUAD_MAIN: &str = r#"void main() {
    // gl_FragCoord is y up; the plan's pixels are y down from the top left.
    vec2 world = vec2(gl_FragCoord.x, u_globals.y - gl_FragCoord.y) / u_globals.z;
    float clip = clip_coverage(world, v_params);
    if (clip <= 0.0) {
        discard;
    }
    o_color = shade(local_point(world)) * clip;
}
"#;

/// Background and border images (`fs_image`): one texture per draw; `rect` and `radii` are the
/// box (the clip shape), `p0` one tile of the image in logical px, `params.yz` say whether it
/// repeats along x and y. The texture wraps by repeating; the shader clamps on the axes that
/// don't repeat.
const IMAGE_MAIN: &str = r#"
uniform sampler2D u_image;

// `background-image-tint` / `border-image-tint` (premultiplied); all zeros means no tint.
vec4 image_tint(vec4 t) {
    return all(equal(t, vec4(0.0))) ? vec4(1.0) : t;
}

void main() {
    vec2 world = vec2(gl_FragCoord.x, u_globals.y - gl_FragCoord.y) / u_globals.z;
    vec2 p = local_point(world);
    float clip = shape_coverage(p, v_rect, v_radii, v_radii_y) * clip_coverage(world, v_params);
    if (clip <= 0.0) {
        discard;
    }
    vec4 tile = v_p0;
    vec2 uv = (p - tile.xy) / tile.zw;
    bool repeat_x = v_params.y > 0.5;
    bool repeat_y = v_params.z > 0.5;
    // `background-repeat: space`: tiles are `p1.xy` apart; nothing shows in the gaps.
    vec2 gap = v_p1.xy;
    vec2 period = tile.zw + gap;
    vec2 local = p - tile.xy;
    vec2 wrapped = local - floor(local / period) * period;
    bool gap_x = repeat_x && gap.x > 0.0;
    bool gap_y = repeat_y && gap.y > 0.0;
    if ((gap_x && wrapped.x > tile.z) || (gap_y && wrapped.y > tile.w)) {
        discard;
    }
    uv = vec2(gap_x ? wrapped.x / tile.z : uv.x, gap_y ? wrapped.y / tile.w : uv.y);
    if ((!repeat_x && (uv.x < 0.0 || uv.x > 1.0)) || (!repeat_y && (uv.y < 0.0 || uv.y > 1.0))) {
        discard;
    }
    vec2 half_texel = 0.5 / vec2(textureSize(u_image, 0));
    if (v_color0.z > 0.0) {
        // A border-image piece: `color0` is its part of the texture (uv). Wrap within the
        // piece and keep filtering inside it.
        vec4 sub = v_color0;
        vec2 t = vec2(repeat_x ? fract(uv.x) : uv.x, repeat_y ? fract(uv.y) : uv.y);
        vec2 at = clamp(sub.xy + t * sub.zw, sub.xy + half_texel, sub.xy + sub.zw - half_texel);
        vec4 c = textureLod(u_image, at, 0.0);
        o_color = vec4(sampled(c.rgb) * c.a, c.a) * image_tint(v_color1) * clip;
        return;
    }
    // The sampler repeats. On axes that don't repeat, keep bilinear filtering from wrapping
    // around to the opposite edge. sRGB textures sample as linear.
    vec2 clamped = clamp(uv, half_texel, vec2(1.0) - half_texel);
    vec2 sample_uv = vec2(repeat_x ? uv.x : clamped.x, repeat_y ? uv.y : clamped.y);
    vec4 c = textureLod(u_image, sample_uv, 0.0);
    o_color = vec4(sampled(c.rgb) * c.a, c.a) * image_tint(v_color1) * clip;
}
"#;

pub fn quad_fragment() -> String {
    format!("{QUAD_COMMON}{GEOMETRY}{FRAGMENT_BODY}{QUAD_MAIN}")
}

pub fn image_fragment() -> String {
    format!("{QUAD_COMMON}{GEOMETRY}{FRAGMENT_BODY}{IMAGE_MAIN}")
}

/// A full-screen triangle.
pub const BLIT_VERTEX: &str = r#"#version 300 es
void main() {
    vec2 points[3] = vec2[3](vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    gl_Position = vec4(points[gl_VertexID], 0.0, 1.0);
}
"#;

/// Copies a target into the canvas pixel for pixel, opaque (a seeded overlay's backdrop);
/// `u_decode` undoes the sRGB transfer, for a canvas that encodes on write (linear blending) from
/// a target that doesn't decode on read.
pub const SEED_FRAGMENT: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_src;
uniform float u_decode;
layout(location = 0) out vec4 o_color;

vec3 srgb_decode(vec3 c) {
    vec3 low = c / 12.92;
    vec3 high = pow((c + 0.055) / 1.055, vec3(2.4));
    return mix(high, low, vec3(lessThanEqual(c, vec3(0.04045))));
}

void main() {
    vec4 c = texelFetch(u_src, ivec2(gl_FragCoord.xy), 0);
    o_color = vec4(u_decode > 0.5 ? srgb_decode(c.rgb) : c.rgb, 1.0);
}
"#;

/// Copies the canvas to the target pixel for pixel. `u_mode` 1 applies the sRGB transfer (a
/// linear canvas onto a target that doesn't encode on write, a window's default framebuffer),
/// 2 undoes it (an sRGB canvas onto a target that does); 0 copies as is.
pub const BLIT_FRAGMENT: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_src;
uniform float u_mode;
layout(location = 0) out vec4 o_color;

vec3 srgb_encode(vec3 c) {
    vec3 low = c * 12.92;
    vec3 high = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return mix(high, low, vec3(lessThanEqual(c, vec3(0.0031308))));
}

vec3 srgb_decode(vec3 c) {
    vec3 low = c / 12.92;
    vec3 high = pow((c + 0.055) / 1.055, vec3(2.4));
    return mix(high, low, vec3(lessThanEqual(c, vec3(0.04045))));
}

void main() {
    vec4 c = texelFetch(u_src, ivec2(gl_FragCoord.xy), 0);
    if (u_mode > 0.5) {
        // The canvas is premultiplied: convert the straight colour.
        vec3 straight = c.a > 0.0 ? c.rgb / c.a : vec3(0.0);
        c.rgb = (u_mode < 1.5 ? srgb_encode(straight) : srgb_decode(straight)) * c.a;
    }
    o_color = c;
}
"#;
