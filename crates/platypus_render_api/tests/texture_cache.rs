// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Texture.load` shares textures (RDY-TEX-02).
use platypus_render_api::{TextureRef, load_texture};

fn load(src: &str, filter: &str) -> TextureRef {
    load_texture(src, filter, "", true, f64::NAN, false)
}

// RDY-TEX-02
#[test]
fn the_same_file_with_the_same_settings_is_one_texture() {
    platypus_render_api::reset();
    let a = load("art/a.png", "");
    assert_eq!(load("art/a.png", ""), a, "a scene loaded again shares it");
    assert_ne!(load("art/b.png", ""), a, "another file");
    assert_ne!(load("art/a.png", "point"), a, "other settings");
    // One load command per texture, not per call.
    let made = platypus_render_api::take_commands().len();
    assert_eq!(made, 3);
}
