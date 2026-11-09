// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Steam for the host: connects when `project.json` has `steam.appId` (the game then shows as
//! running in the player's Steam, and the script `Steam` namespace works), pumps Steam's callbacks
//! each frame and disconnects at exit. Everything else leaves Steam off and the game runs the
//! same: no `steam` setting, a build without the `steam` feature, Steam's DLL missing, or a Steam
//! client that isn't running (a warning, not an error).

use platypus_core::project::SteamConfig;

/// Windows delay-loads `steam_api64.dll` (build.rs), so a missing one must be found before the
/// first Steam call, which would otherwise end the program.
#[cfg(all(feature = "steam", windows))]
fn dll_present() -> bool {
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut core::ffi::c_void;
    }
    let name: Vec<u16> = "steam_api64.dll\0".encode_utf16().collect();
    // Loading it keeps it loaded, which is what the delay-load would do on the first call anyway.
    !unsafe { LoadLibraryW(name.as_ptr()) }.is_null()
}

#[cfg(all(feature = "steam", not(windows)))]
fn dll_present() -> bool {
    true
}

/// Connects as the game's app id; a project without `steam.appId` doesn't touch Steam.
#[cfg(feature = "steam")]
pub fn start(config: Option<SteamConfig>) {
    let Some(config) = config else { return };
    if !dll_present() {
        tracing::warn!(
            "Steam: steam_api64.dll not found next to the program; the Steam API stays off"
        );
        return;
    }
    match platypus_steam::install(config.app_id) {
        Ok(id) => tracing::info!("Steam: connected as app {id}"),
        Err(e) => tracing::warn!("Steam: not connected ({e}); the Steam API stays off"),
    }
}

#[cfg(not(feature = "steam"))]
pub fn start(config: Option<SteamConfig>) {
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
