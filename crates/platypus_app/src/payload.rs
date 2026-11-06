// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A single-file game: when this executable carries a payload (`platypus-build package`, format in
//! `web_ui_core::payload`), the game's *code* (bundle, CSS, `project.json`, icon, and a few tiny
//! essentials) is extracted once to a per-user cache folder and the game runs from there.
//! The assets are not in the payload: they are the `data/` folder beside the exe
//! (`asset_root.rs` finds it).
//!
//! The unpacking is web-ui's (`web_ui_core::unpack`, re-exported as `platypus_core::unpack`).

use std::path::PathBuf;

use platypus_core::unpack::{cache_dir, extract_cached};

/// The folder of the embedded game, extracting it first if needed; `None` when this
/// executable has no payload (it is the plain engine host).
pub fn embedded_game() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    match extract_cached(&exe, &cache_dir("Platypus", "games")?) {
        Ok(dir) => dir,
        Err(e) => {
            // No window yet and no log: stderr is all there is.
            eprintln!("platypus: the embedded game could not be unpacked: {e}");
            None
        }
    }
}
