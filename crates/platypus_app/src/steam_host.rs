// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Steam for the host: connects when `project.json` has `steam.appId` (the game then shows as
//! running in the player's Steam, and the script `Steam` namespace works), pumps Steam's callbacks
//! each frame and disconnects at exit. Everything else leaves Steam off and the game runs the
//! same: no `steam` setting, a build without the `steam` feature, Steam's library missing (it is
//! loaded at run time on every platform), or a Steam client that isn't running (a warning, not an
//! error).

use platypus_core::project::SteamConfig;
use std::path::Path;

/// Connects as the game's app id; a project without `steam.appId` doesn't touch Steam. The
/// library is looked for beside the program, then in the game's `steam/` folder and the game
/// folder (where `platypus-build package` finds it too), then on the system path.
#[cfg(feature = "steam")]
pub fn start(config: Option<SteamConfig>, game_dir: &Path) {
    let Some(config) = config else { return };
    let search = [game_dir.join("steam"), game_dir.to_path_buf()];
    match platypus_steam::install(config.app_id, &search) {
        Ok(id) => tracing::info!("Steam: connected as app {id}"),
        Err(e) => tracing::warn!("Steam: not connected ({e}); the Steam API stays off"),
    }
}

#[cfg(not(feature = "steam"))]
pub fn start(config: Option<SteamConfig>, _game_dir: &Path) {
    if config.is_some() {
        tracing::warn!(
            "Steam: this build has no Steam support (the `steam` feature); the Steam API stays off"
        );
    }
}

/// Runs Steam's callbacks and moves finished lobby requests into what scripts read.
pub fn pump() {
    platypus_engine_api::steam::pump();
}

/// Drops the connection (Steam then sees the game close).
pub fn shutdown() {
    platypus_engine_api::steam::install(None);
}
