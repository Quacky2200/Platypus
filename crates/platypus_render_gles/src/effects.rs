// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! GLSL ES 3.00 ports of `effects.wgsl`: the separable blur and the layer composite (opacity,
//! a clip from the clip table, a `filter` colour matrix, a drop-shadow tint and shift, a mask
//! texture, and the W3C blend modes). All draw one full-screen triangle (`shaders::BLIT_VERTEX`).
//!
//! Textures are the frame's canvases, sampled in the framebuffer's own orientation: integer
//! fetches use `gl_FragCoord` directly, and the one filtered sample (a shifted drop shadow)
//! flips y to get the plan's top-left pixel coordinates back and forth.

use crate::shaders::{GEOMETRY, QUAD_COMMON};

/// One pass of the separable Gaussian: `u_blur` is (dir.x, dir.y, sigma in px, taps either
/// side). Reads stay inside `u_bounds` (first x, first y, last x, last y, in the canvas's own
/// orientation): the whole canvas, or a `backdrop-filter`'s box. With `u_mirror` on, reads
/// past the bounds reflect back in (as Chrome does for backdrop-filter); otherwise they clamp.
pub const BLUR_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2D;
uniform sampler2D u_src;
uniform vec4 u_blur;
uniform vec4 u_bounds;
uniform float u_mirror;
layout(location = 0) out vec4 o_color;

void main() {
    ivec2 low = ivec2(u_bounds.xy);
    ivec2 high = ivec2(u_bounds.zw);
    ivec2 at = ivec2(gl_FragCoord.xy);
    int r = int(u_blur.w);
    vec4 sum = vec4(0.0);
    float weights = 0.0;
    for (int i = -r; i <= r; i++) {
        float x = float(i);
        float w = exp(-(x * x) / (2.0 * u_blur.z * u_blur.z));
        ivec2 q = at + ivec2(u_blur.xy * x);
        if (u_mirror > 0.5) {
            if (q.x < low.x) q.x = 2 * low.x - 1 - q.x;
            if (q.x > high.x) q.x = 2 * high.x + 1 - q.x;
            if (q.y < low.y) q.y = 2 * low.y - 1 - q.y;
            if (q.y > high.y) q.y = 2 * high.y + 1 - q.y;
        }
        q = clamp(q, low, high);
        sum += texelFetch(u_src, q, 0) * w;
        weights += w;
    }
    o_color = sum / weights;
}
"#;

const COMPOSITE_COMMON: &str = r#"
uniform sampler2D u_src;       // the layer
uniform sampler2D u_backdrop;  // what is beneath it (blend modes)
uniform sampler2D u_mask_tex;  // the layer's mask
uniform vec4 u_comp;           // x: opacity, y: scale, z: clip index (1-based, 0 = none), w: blend mode
uniform mat4 u_matrix;         // `filter` colour matrix (sRGB, straight alpha), as columns
uniform vec4 u_offset;         // its offset column
uniform vec4 u_tint;           // drop shadows: the source's alpha in this colour
uniform vec4 u_shift;          // xy: source shift (physical px), z: matrix on, w: tint on
uniform vec4 u_maskmode;       // x: mask mode (0 none, 1 alpha, 2 luminance)
layout(location = 0) out vec4 o_color;

vec3 to_srgb(vec3 c) {
    vec3 low = c * 12.92;
    vec3 high = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return mix(high, low, vec3(lessThanEqual(c, vec3(0.0031308))));
}

vec3 from_srgb(vec3 c) {
    vec3 low = c / 12.92;
    vec3 high = pow((c + 0.055) / 1.055, vec3(2.4));
    return mix(high, low, vec3(lessThanEqual(c, vec3(0.04045))));
}

// A canvas colour (straight alpha) as sRGB, and back.
vec3 canvas_to_srgb(vec3 c) {
    return srgb_canvas() ? c : to_srgb(c);
}

vec3 canvas_from_srgb(vec3 c) {
    return srgb_canvas() ? c : from_srgb(c);
}

// The fragment in the plan's pixels (y down from the top left).
vec2 frag_pos() {
    return vec2(gl_FragCoord.x, u_globals.y - gl_FragCoord.y);
}

// How much of the layer shows here (`mask-image`).
float mask_at() {
    if (u_maskmode.x < 0.5) {
        return 1.0;
    }
    vec4 m = texelFetch(u_mask_tex, ivec2(gl_FragCoord.xy), 0);
    if (u_maskmode.x < 1.5) {
        return m.a;
    }
    // Luminance of the (premultiplied) mask colour, in sRGB like CSS.
    vec3 straight = m.a > 0.0 ? m.rgb / m.a : vec3(0.0);
    return dot(canvas_to_srgb(straight), vec3(0.2126, 0.7152, 0.0722)) * m.a;
}

// The layer's pixel here, after its drop-shadow tint and `filter` colour matrix.
vec4 filtered_source() {
    vec2 q = frag_pos() - u_shift.xy;
    vec4 c = texture(u_src, vec2(q.x, u_globals.y - q.y) / u_globals.xy);
    if (u_shift.w > 0.5) {
        c = u_tint * c.a;
    }
    if (u_shift.z > 0.5) {
        // CSS filter functions work on straight-alpha sRGB, like browsers.
        vec3 straight = c.a > 0.0 ? c.rgb / c.a : vec3(0.0);
        vec4 f = clamp(u_matrix * vec4(canvas_to_srgb(straight), c.a) + u_offset, vec4(0.0), vec4(1.0));
        c = vec4(canvas_from_srgb(f.rgb) * f.a, f.a);
    }
    return c;
}
"#;

const COMPOSITE_PLAIN_MAIN: &str = r#"
void main() {
    vec4 color = filtered_source() * u_comp.x * mask_at();
    o_color = color * clip_coverage_with(frag_pos() / u_globals.z, uint(u_comp.z));
}
"#;

/// W3C compositing: the layer (source) blends with a copy of what's beneath it (backdrop), in
/// sRGB like browsers. The pass replaces the destination pixels.
const BLEND: &str = r#"
vec3 hard_light(vec3 cb, vec3 cs) {
    vec3 multiply = cb * 2.0 * cs;
    vec3 s = 2.0 * cs - 1.0;
    vec3 screen = cb + s - cb * s;
    return mix(screen, multiply, vec3(lessThanEqual(cs, vec3(0.5))));
}

vec3 soft_light(vec3 cb, vec3 cs) {
    vec3 d = mix(sqrt(cb), ((16.0 * cb - 12.0) * cb + 4.0) * cb, vec3(lessThanEqual(cb, vec3(0.25))));
    vec3 dark = cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
    vec3 light = cb + (2.0 * cs - 1.0) * (d - cb);
    return mix(light, dark, vec3(lessThanEqual(cs, vec3(0.5))));
}

vec3 color_dodge(vec3 cb, vec3 cs) {
    vec3 r = min(vec3(1.0), cb / max(1.0 - cs, vec3(1e-6)));
    return mix(mix(r, vec3(1.0), vec3(greaterThanEqual(cs, vec3(1.0)))), vec3(0.0), vec3(lessThanEqual(cb, vec3(0.0))));
}

vec3 color_burn(vec3 cb, vec3 cs) {
    vec3 r = 1.0 - min(vec3(1.0), (1.0 - cb) / max(cs, vec3(1e-6)));
    return mix(mix(r, vec3(0.0), vec3(lessThanEqual(cs, vec3(0.0)))), vec3(1.0), vec3(greaterThanEqual(cb, vec3(1.0))));
}

float lum(vec3 c) {
    return dot(c, vec3(0.3, 0.59, 0.11));
}

vec3 clip_color(vec3 c) {
    float l = lum(c);
    float n = min(c.r, min(c.g, c.b));
    float x = max(c.r, max(c.g, c.b));
    vec3 result = c;
    if (n < 0.0) {
        result = l + (result - l) * l / max(l - n, 1e-6);
    }
    if (x > 1.0) {
        result = l + (result - l) * (1.0 - l) / max(x - l, 1e-6);
    }
    return result;
}

vec3 set_lum(vec3 c, float l) {
    return clip_color(c + (l - lum(c)));
}

float sat(vec3 c) {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

// The spec's SetSat: the largest channel becomes `s`, the smallest 0, the middle one scales
// between them.
vec3 set_sat(vec3 c, float s) {
    float n = min(c.r, min(c.g, c.b));
    float range = sat(c);
    if (range <= 0.0) {
        return vec3(0.0);
    }
    return (c - n) * s / range;
}

vec3 blend(uint mode, vec3 cb, vec3 cs) {
    switch (mode) {
        case 1u: return cb * cs;
        case 2u: return cb + cs - cb * cs;
        case 3u: return hard_light(cs, cb);
        case 4u: return min(cb, cs);
        case 5u: return max(cb, cs);
        case 6u: return color_dodge(cb, cs);
        case 7u: return color_burn(cb, cs);
        case 8u: return hard_light(cb, cs);
        case 9u: return soft_light(cb, cs);
        case 10u: return abs(cb - cs);
        case 11u: return cb + cs - 2.0 * cb * cs;
        case 12u: return set_lum(set_sat(cs, sat(cb)), lum(cb));
        case 13u: return set_lum(set_sat(cb, sat(cs)), lum(cb));
        case 14u: return set_lum(cs, lum(cb));
        case 15u: return set_lum(cb, lum(cs));
        default: return cs;
    }
}
"#;

const COMPOSITE_BLEND_MAIN: &str = r#"
void main() {
    vec4 s = filtered_source() * u_comp.x * mask_at();
    vec4 b = texelFetch(u_backdrop, ivec2(gl_FragCoord.xy), 0);
    vec3 cs = canvas_to_srgb(s.rgb / max(s.a, 1e-6));
    vec3 cb = canvas_to_srgb(b.rgb / max(b.a, 1e-6));
    vec3 mixed = clamp(blend(uint(u_comp.w), cb, cs), vec3(0.0), vec3(1.0));
    // Source-over with the blended colour where both are present.
    float a = s.a + b.a * (1.0 - s.a);
    vec3 c = s.a * (1.0 - b.a) * cs + b.a * (1.0 - s.a) * cb + s.a * b.a * mixed;
    vec3 straight = a > 0.0 ? c / a : vec3(0.0);
    vec4 result = vec4(canvas_from_srgb(straight) * a, a);
    float cover = clip_coverage_with(frag_pos() / u_globals.z, uint(u_comp.z));
    o_color = mix(b, result, cover);
}
"#;

/// Composites a layer over its parent with opacity, clip, filter and mask (the blend pass is
/// [`composite_blend_fragment`]).
pub fn composite_fragment() -> String {
    format!("{QUAD_COMMON}{GEOMETRY}{COMPOSITE_COMMON}{COMPOSITE_PLAIN_MAIN}")
}

/// The same with a blend mode: mixes the layer with a copy of its parent.
pub fn composite_blend_fragment() -> String {
    format!("{QUAD_COMMON}{GEOMETRY}{COMPOSITE_COMMON}{BLEND}{COMPOSITE_BLEND_MAIN}")
}
