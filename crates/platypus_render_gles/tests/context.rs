// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The EGL context (M2-GLES-01). Needs an EGL driver: ANGLE's folder in `PLATYPUS_ANGLE_DIR` on
//! Windows, Mesa's EGL elsewhere; the test passes without saying anything when there is none.

use glow::HasContext;

use crate::common::headless as context;

// M2-GLES-01
#[test]
fn a_gles3_context_clears_a_framebuffer_and_reads_it_back() {
    let Some(cx) = context() else { return };
    let gl = cx.gl();
    unsafe {
        let version = gl.get_parameter_string(glow::VERSION);
        assert!(version.contains("OpenGL ES 3"), "{version}");
        // A driver that can't make one (Mesa's llvmpipe on some machines): skipped with a note.
        let texture = match gl.create_texture() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("skipped: {e}");
                return;
            }
        };
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::SRGB8_ALPHA8 as i32,
            4,
            4,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        let fbo = gl.create_framebuffer().unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        assert_eq!(
            gl.check_framebuffer_status(glow::FRAMEBUFFER),
            glow::FRAMEBUFFER_COMPLETE
        );
        gl.clear_color(1.0, 0.0, 0.0, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
        let mut pixels = [0u8; 4 * 4 * 4];
        gl.read_pixels(
            0,
            0,
            4,
            4,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixels)),
        );
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    }
}
