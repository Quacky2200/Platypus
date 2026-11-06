// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! How the wgpu path composites a frame's screen UI: over the world, and with the FPS counter.
//! Both follow from blending (docs/rendering.md "Blending"): the hardware blends onto an
//! sRGB-encoding target in linear light, so sRGB blending needs the UI on one canvas.

use platypus_render::{Blending, wgpu};
use web_ui_dom::DrawCmd;
use web_ui_dom::transform::Homography;

/// Whether screen UI goes over a copy of the world (`render_over`) rather than being blended
/// onto it (`render_overlay`): when it reads the world (backdrop filters, blend modes), and with
/// sRGB blending onto a target that encodes on write, whose hardware blend is in linear light.
pub(crate) fn over_a_copy(
    blending: Blending,
    format: wgpu::TextureFormat,
    cmds: &[DrawCmd],
) -> bool {
    platypus_render::needs_backdrop(cmds)
        || (blending == Blending::Srgb && format.is_srgb() && !cmds.is_empty())
}

/// Puts the FPS counter's commands, laid out at `ratio` times the UI's scale, after the UI's, so
/// the counter is composited with the frame (one canvas, one blending).
pub(crate) fn append_scaled(cmds: &mut Vec<DrawCmd>, extra: Vec<DrawCmd>, ratio: f32) {
    if (ratio - 1.0).abs() < 1e-6 {
        cmds.extend(extra);
        return;
    }
    cmds.push(DrawCmd::PushTransform(Homography([
        [ratio, 0.0, 0.0],
        [0.0, ratio, 0.0],
        [0.0, 0.0, 1.0],
    ])));
    cmds.extend(extra);
    cmds.push(DrawCmd::PopTransform);
}

#[cfg(test)]
mod tests {
    use super::*;

    // M2-BLEND-10
    #[test]
    fn srgb_ui_goes_over_a_copy_of_the_world_and_the_fps_counter_joins_it() {
        let srgb = wgpu::TextureFormat::Bgra8UnormSrgb;
        let plain = wgpu::TextureFormat::Bgra8Unorm;
        let quad = DrawCmd::PopClip;
        // sRGB blending onto an encoding target needs the copy; linear or a plain target don't,
        // and nothing to draw needs nothing.
        assert!(over_a_copy(
            Blending::Srgb,
            srgb,
            std::slice::from_ref(&quad)
        ));
        assert!(!over_a_copy(
            Blending::Linear,
            srgb,
            std::slice::from_ref(&quad)
        ));
        assert!(!over_a_copy(
            Blending::Srgb,
            plain,
            std::slice::from_ref(&quad)
        ));
        assert!(!over_a_copy(Blending::Srgb, srgb, &[]));
        // The counter at the UI's scale is appended as is, at another under a scale transform.
        let mut cmds = vec![DrawCmd::PopClip];
        append_scaled(&mut cmds, vec![DrawCmd::PopClip], 1.0);
        assert_eq!(cmds.len(), 2);
        append_scaled(&mut cmds, vec![DrawCmd::PopClip], 2.0);
        assert_eq!(cmds.len(), 5);
        assert!(
            matches!(&cmds[2], DrawCmd::PushTransform(h) if h.0[0][0] == 2.0 && h.0[1][1] == 2.0)
        );
        assert!(matches!(cmds[4], DrawCmd::PopTransform));
    }
}
