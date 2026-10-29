// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Turning `platypus_ui` gradients into the GPU representation the quad shader reads.

pub use platypus_ui::resolve_stops;
use platypus_ui::{BackgroundImage, GradientDirection, RadialShape, Rect};

const KIND_LINEAR: f32 = 0.0;
const KIND_RADIAL: f32 = 1.0;
const KIND_CONIC: f32 = 2.0;

/// Most stops a gradient can have on the GPU; extra stops are dropped with a warning.
pub const MAX_STOPS: usize = 8;

/// Matches `struct Gradient` in `quad.wgsl` (192 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuGradient {
    pub info: [f32; 4],
    pub info2: [f32; 4],
    pub colors: [[f32; 4]; MAX_STOPS],
    pub stops: [[f32; 4]; 2],
}

/// Builds the GPU gradient for `image` painted over `rect`, or `None` if it isn't
/// a gradient (images are drawn separately).
pub fn build(image: &BackgroundImage, rect: Rect) -> Option<GpuGradient> {
    let (w, h) = (rect.width.max(0.0), rect.height.max(0.0));
    // The conic centre, relative to the box's top-left.
    let mut center = (w / 2.0, h / 2.0);
    let (kind, a, b, line_length, stops, repeating) = match image {
        BackgroundImage::Url(_) => return None,
        BackgroundImage::Linear {
            direction,
            stops,
            repeating,
        } => {
            let dir = match *direction {
                GradientDirection::Angle(deg) => {
                    let t = deg.to_radians();
                    // 0deg points up; y grows downwards.
                    (t.sin(), -t.cos())
                }
                GradientDirection::Corner { right, bottom } => {
                    // CSS "magic corners": perpendicular to the diagonal joining the
                    // two neighbouring corners, pointing at the named corner.
                    let sx = if right { 1.0 } else { -1.0 };
                    let sy = if bottom { 1.0 } else { -1.0 };
                    let (x, y) = (h * sx, w * sy);
                    let len = (x * x + y * y).sqrt().max(f32::EPSILON);
                    (x / len, y / len)
                }
            };
            let length = (w * dir.0).abs() + (h * dir.1).abs();
            (
                KIND_LINEAR,
                dir.0,
                dir.1,
                length.max(f32::EPSILON),
                stops,
                *repeating,
            )
        }
        BackgroundImage::Radial {
            shape,
            at,
            stops,
            repeating,
        } => {
            // Sized to the farthest corner from the centre (the CSS default).
            let c = (at.x.resolve(w), at.y.resolve(h));
            center = c;
            let (dx, dy) = ((c.0.max(w - c.0)), (c.1.max(h - c.1)));
            let (rx, ry) = match shape {
                RadialShape::Circle => {
                    let r = (dx * dx + dy * dy).sqrt();
                    (r, r)
                }
                RadialShape::Ellipse => {
                    (dx * std::f32::consts::SQRT_2, dy * std::f32::consts::SQRT_2)
                }
            };
            (
                KIND_RADIAL,
                rx.max(f32::EPSILON),
                ry.max(f32::EPSILON),
                rx.max(f32::EPSILON),
                stops,
                *repeating,
            )
        }
        BackgroundImage::Conic {
            from,
            at,
            stops,
            repeating,
        } => {
            center = (at.x.resolve(w), at.y.resolve(h));
            // Positions are fractions of a turn; a length can't place a conic stop.
            (KIND_CONIC, from.to_radians(), 0.0, 1.0, stops, *repeating)
        }
    };

    let mut g = GpuGradient {
        info: [kind, 0.0, a, b],
        info2: [
            line_length,
            center.0,
            center.1,
            f32::from(u8::from(repeating)),
        ],
        ..Default::default()
    };
    let resolved = resolve_stops(stops, line_length);
    if resolved.len() > MAX_STOPS {
        tracing::warn!(
            "gradient has {} stops; only the first {MAX_STOPS} are drawn",
            resolved.len()
        );
    }
    let resolved = &resolved[..resolved.len().min(MAX_STOPS)];
    g.info[1] = resolved.len() as f32;
    for (i, (color, pos)) in resolved.iter().enumerate() {
        g.colors[i] = *color;
        g.stops[i / 4][i % 4] = *pos;
    }
    Some(g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_ui::{ColorStop, Len, Rgba};

    fn stop(p: Option<Len>) -> ColorStop {
        ColorStop {
            color: Rgba::new(1.0, 0.0, 0.0, 1.0),
            position: p,
        }
    }

    fn positions(stops: &[ColorStop], len: f32) -> Vec<f32> {
        resolve_stops(stops, len)
            .into_iter()
            .map(|(_, p)| p)
            .collect()
    }

    #[test]
    fn missing_positions_follow_css_fixup() {
        assert_eq!(positions(&[stop(None), stop(None)], 100.0), [0.0, 1.0]);
        assert_eq!(
            positions(&[stop(None), stop(None), stop(None)], 100.0),
            [0.0, 0.5, 1.0]
        );
        assert_eq!(
            positions(
                &[
                    stop(None),
                    stop(Some(Len::Percent(0.2))),
                    stop(None),
                    stop(None)
                ],
                100.0
            ),
            [0.0, 0.2, 0.6, 1.0]
        );
        // Positions never go backwards, and px resolve against the line length.
        assert_eq!(
            positions(
                &[stop(Some(Len::Percent(0.5))), stop(Some(Len::Px(20.0)))],
                100.0
            ),
            [0.5, 0.5]
        );
    }

    #[test]
    fn linear_directions_and_lengths() {
        let rect = Rect::new(0.0, 0.0, 200.0, 100.0);
        let lin = |direction| BackgroundImage::Linear {
            direction,
            stops: vec![stop(None), stop(None)],
            repeating: false,
        };
        // "to right": along +x, the line is the box width.
        let g = build(&lin(GradientDirection::Angle(90.0)), rect).unwrap();
        assert!((g.info[2] - 1.0).abs() < 1e-6 && g.info[3].abs() < 1e-6);
        assert!((g.info2[0] - 200.0).abs() < 1e-3);
        // Default "to bottom": along +y (screen down), the line is the height.
        let g = build(&lin(GradientDirection::Angle(180.0)), rect).unwrap();
        assert!((g.info[3] - 1.0).abs() < 1e-6);
        assert!((g.info2[0] - 100.0).abs() < 1e-3);
        // "to bottom right" on a 200x100 box points along (100, 200) normalised.
        let g = build(
            &lin(GradientDirection::Corner {
                right: true,
                bottom: true,
            }),
            rect,
        )
        .unwrap();
        let n = (100f32.powi(2) + 200f32.powi(2)).sqrt();
        assert!((g.info[2] - 100.0 / n).abs() < 1e-5 && (g.info[3] - 200.0 / n).abs() < 1e-5);
        assert_eq!(g.info[1], 2.0);
        assert_eq!(build(&BackgroundImage::Url("x.png".into()), rect), None);
    }

    #[test]
    fn radial_sizes_reach_the_farthest_corner() {
        let rect = Rect::new(0.0, 0.0, 60.0, 80.0);
        let centre = platypus_ui::BackgroundPosition {
            x: Len::Percent(0.5),
            y: Len::Percent(0.5),
        };
        let circle = BackgroundImage::Radial {
            shape: RadialShape::Circle,
            at: centre,
            stops: vec![stop(None), stop(None)],
            repeating: true,
        };
        let g = build(&circle, rect).unwrap();
        assert!((g.info[2] - 50.0).abs() < 1e-4 && (g.info[3] - 50.0).abs() < 1e-4);
        assert_eq!(g.info2[3], 1.0, "repeating flag");
    }

    // `circle at 100% 0` (M3d): centred on the top-right corner, sized to the bottom-left one.
    #[test]
    fn radial_centre_moves_and_the_size_follows_the_far_corner() {
        let rect = Rect::new(0.0, 0.0, 60.0, 80.0);
        let corner = BackgroundImage::Radial {
            shape: RadialShape::Circle,
            at: platypus_ui::BackgroundPosition {
                x: Len::Percent(1.0),
                y: Len::Px(0.0),
            },
            stops: vec![stop(None), stop(None)],
            repeating: false,
        };
        let g = build(&corner, rect).unwrap();
        assert_eq!((g.info2[1], g.info2[2]), (60.0, 0.0), "centre");
        assert!(
            (g.info[2] - 100.0).abs() < 1e-4,
            "radius to (0, 80): {}",
            g.info[2]
        );
    }

    #[test]
    fn conic_centre_and_start_angle() {
        let rect = Rect::new(10.0, 10.0, 60.0, 80.0);
        let conic = BackgroundImage::Conic {
            from: 90.0,
            at: platypus_ui::BackgroundPosition {
                x: Len::Percent(0.25),
                y: Len::Px(5.0),
            },
            stops: vec![stop(None), stop(None)],
            repeating: false,
        };
        let g = build(&conic, rect).unwrap();
        assert_eq!(g.info[0], KIND_CONIC);
        assert!((g.info[2] - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!((g.info2[1], g.info2[2]), (15.0, 5.0), "relative to the box");
        assert_eq!(g.info2[3], 0.0);
    }
}
