// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Steam for the host: connects when `project.json` has `steam` (the game then shows as running in
//! the player's Steam, and the script `Steam` namespace works), pumps Steam's callbacks each frame
//! and disconnects at exit. A Steam that isn't running is a warning, not an error: the API stays off.

use platypus_core::project::SteamConfig;

/// Connects as the game's app id; a project without `steam.appId` doesn't touch Steam.
pub fn start(config: Option<SteamConfig>) {
    let Some(config) = config else { return };
    match platypus_steam::install(config.app_id) {
        Ok(id) => tracing::info!("Steam: connected as app {id}"),
        Err(e) => tracing::warn!("Steam: not connected ({e}); the Steam API stays off"),
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
