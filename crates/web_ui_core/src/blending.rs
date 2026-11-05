// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

/// How translucent UI colours combine (`blending` in `project.json`): one setting for a whole
/// frame of every UI renderer (docs/rendering.md "Blending").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Blending {
    /// `src * a + dst * (1 - a)` on sRGB values, as browsers.
    #[default]
    Srgb,
    /// The same sum on linear-light values (Platypus before 2026-10-30).
    Linear,
}

impl Blending {
    /// A value of `blending`.
    pub fn parse(s: &str) -> Option<Blending> {
        match s {
            "srgb" => Some(Blending::Srgb),
            "linear" => Some(Blending::Linear),
            _ => None,
        }
    }

    /// The value written in `project.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Blending::Srgb => "srgb",
            Blending::Linear => "linear",
        }
    }
}
