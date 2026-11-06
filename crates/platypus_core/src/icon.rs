// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The Platypus icon (`resources/icon/icon.png`), used by every game that has none of its own,
//! and Platypus's product name. The `.ico` and `.res` writers are web-ui's (`web_ui_core::icon`),
//! re-exported here.

/// The Platypus icon: a 1254 x 1254 PNG with transparent corners.
pub const DEFAULT_PNG: &[u8] = include_bytes!("../../../resources/icon/icon.png");

pub use web_ui_core::icon::*;

/// The product name the host and packaged games without their own metadata carry.
pub const PRODUCT_NAME: &str = "Platypus";

#[cfg(test)]
mod tests {
    use super::*;

    // M9-ICO-01
    #[test]
    fn the_default_icon_is_a_square_png() {
        assert_eq!(&DEFAULT_PNG[..8], b"\x89PNG\r\n\x1a\n");
        let w = u32::from_be_bytes(DEFAULT_PNG[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(DEFAULT_PNG[20..24].try_into().unwrap());
        assert_eq!((w, h), (1254, 1254));
    }
}
