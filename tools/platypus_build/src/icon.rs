// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Icons of packaged games: web-ui's conversion (`web_ui_build::icon`), and the Platypus icon
//! for games without one.

use platypus_core::icon::DEFAULT_PNG;
pub use web_ui_build::icon::*;

/// The Platypus icon as an `.ico`, for games that have none.
pub fn default_ico() -> Vec<u8> {
    to_ico(DEFAULT_PNG).expect("the Platypus icon converts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageFormat;

    // M9-ICO-05
    #[test]
    fn the_default_icon_decodes_and_converts_to_every_size() {
        let img = image::load_from_memory_with_format(DEFAULT_PNG, ImageFormat::Png).unwrap();
        assert_eq!((img.width(), img.height()), (1254, 1254));
        let ico = default_ico();
        assert_eq!(
            ico_sizes(&ico),
            [16, 24, 32, 48, 64, 128, 256].map(|s| (s, s))
        );
        let decoded = image::load_from_memory_with_format(&ico, ImageFormat::Ico).unwrap();
        assert_eq!(decoded.width(), 256);
    }
}
