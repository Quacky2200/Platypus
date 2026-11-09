// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The Steam library of a packaged game. `platypus-app` loads Steam's own library at run time
//! (`platypus_steam`), so a game whose `project.json` has `steam.appId` needs that library beside
//! its exe. It comes from Valve's Steamworks SDK (`redistributable_bin`), under Valve's terms, so
//! packaging never fetches it: it copies the one the developer put in a known place.

use platypus_core::project::Platform;
use std::path::{Path, PathBuf};

/// The library's file name on `platform`; `None` where Platypus has no Steam support.
pub fn library_name(platform: Platform) -> Option<&'static str> {
    match platform {
        Platform::Windows => Some("steam_api64.dll"),
        Platform::Linux => Some("libsteam_api.so"),
        Platform::Macos => Some("libsteam_api.dylib"),
        Platform::Android | Platform::Ios => None,
    }
}

/// Where packaging looks, in order: `--steam-library <file>`, then `steam/` and the game folder,
/// then the folder of the host program. An explicit file that doesn't exist is an error; otherwise
/// `None` means nothing was found.
pub fn find_library(
    game: &Path,
    host_dir: Option<&Path>,
    name: &str,
    explicit: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    if let Some(file) = explicit {
        return if file.is_file() {
            Ok(Some(file.to_path_buf()))
        } else {
            Err(format!("--steam-library {}: no such file", file.display()))
        };
    }
    let mut places = vec![game.join("steam"), game.to_path_buf()];
    places.extend(host_dir.map(Path::to_path_buf));
    Ok(places
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("platypus_steam_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // M8-STM-13
    #[test]
    fn the_library_has_a_name_per_desktop_platform() {
        assert_eq!(library_name(Platform::Windows), Some("steam_api64.dll"));
        assert_eq!(library_name(Platform::Linux), Some("libsteam_api.so"));
        assert_eq!(library_name(Platform::Macos), Some("libsteam_api.dylib"));
        assert_eq!(library_name(Platform::Android), None);
    }

    // M8-STM-13
    #[test]
    fn the_library_is_found_in_steam_then_the_game_then_beside_the_host() {
        let game = folder("find_game");
        let host = folder("find_host");
        assert_eq!(find_library(&game, Some(&host), "lib.x", None), Ok(None));

        std::fs::write(host.join("lib.x"), "host").unwrap();
        let found = find_library(&game, Some(&host), "lib.x", None).unwrap();
        assert_eq!(found, Some(host.join("lib.x")));

        std::fs::write(game.join("lib.x"), "game").unwrap();
        let found = find_library(&game, Some(&host), "lib.x", None).unwrap();
        assert_eq!(found, Some(game.join("lib.x")));

        std::fs::create_dir_all(game.join("steam")).unwrap();
        std::fs::write(game.join("steam/lib.x"), "steam").unwrap();
        let found = find_library(&game, Some(&host), "lib.x", None).unwrap();
        assert_eq!(found, Some(game.join("steam/lib.x")));

        let _ = std::fs::remove_dir_all(&game);
        let _ = std::fs::remove_dir_all(&host);
    }

    // M8-STM-13
    #[test]
    fn an_explicit_library_wins_and_a_missing_one_is_an_error() {
        let game = folder("explicit");
        std::fs::write(game.join("lib.x"), "game").unwrap();
        let mine = game.join("mine.bin");
        std::fs::write(&mine, "mine").unwrap();
        assert_eq!(
            find_library(&game, None, "lib.x", Some(&mine)),
            Ok(Some(mine.clone()))
        );
        let nowhere = game.join("nowhere.bin");
        let e = find_library(&game, None, "lib.x", Some(&nowhere)).unwrap_err();
        assert!(e.contains("no such file"), "{e}");
        let _ = std::fs::remove_dir_all(&game);
    }
}
