// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Assets`: the asset database (each asset `.meta` file gives it a GUID and importer settings),
//! `"guid:<guid>"` sources, the index of every asset the game can load (`exists`, `list`,
//! `packOf`) and the paths of files that changed on disk (hot reload). The host fills it
//! ([`set_assets`], [`set_asset_index`], [`changed`]); scripts read it. `js/engine/src/assets.ts` had
//! `onChanged(fn)`: here `onChanged(fn)` returns a token for `offChanged(token)` and `fn` gets the
//! changed paths as one string, a path per line; `takeChanges()` polls the same list.

use std::cell::RefCell;
use ts_api::callback::{Listeners, emit};
use ts_api::ts_api;

/// One asset's database entry.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetEntry {
    /// Game-relative, forward slashes.
    pub path: String,
    pub guid: String,
    /// Its importer settings as a JSON object (`{}` when there are none).
    pub importer_json: String,
}

#[derive(Default)]
struct State {
    entries: Vec<AssetEntry>,
    /// Every loadable asset: (game path, the pack that holds it or ""), sorted by path.
    index: Vec<(String, String)>,
    changed: Vec<String>,
    listeners: Listeners<(String,)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn set_assets(entries: Vec<AssetEntry>) {
    STATE.with(|s| s.borrow_mut().entries = entries);
}

/// Every asset the game can load, as (game path, pack name or `""` for a loose file). The host
/// builds it from the file system it mounted (packs, or the game folder), so this crate needs
/// no pack code.
pub fn set_asset_index(mut index: Vec<(String, String)>) {
    index.sort();
    index.dedup_by(|a, b| a.0 == b.0);
    STATE.with(|s| s.borrow_mut().index = index);
}

/// Files that changed on disk: scripts read them with `takeChanges()`.
pub fn changed(paths: impl IntoIterator<Item = String>) {
    let paths: Vec<String> = paths.into_iter().collect();
    if paths.is_empty() {
        return;
    }
    let cbs = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.changed.extend(paths.iter().cloned());
        s.listeners.snapshot()
    });
    emit(&cbs, (paths.join("\n"),));
}

pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}

/// The index row for a path (`guid:` sources resolved, `./` and `\` cleaned).
fn indexed(path: &str) -> Option<(String, String)> {
    let path = resolve(path.trim());
    let p = clean(&path);
    STATE.with(|s| {
        let s = s.borrow();
        s.index
            .binary_search_by(|(k, _)| k.as_str().cmp(&p))
            .ok()
            .map(|i| s.index[i].clone())
    })
}

fn clean(path: &str) -> String {
    let p = path.replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_string()
}

fn guid_to_path(guid: &str) -> Option<String> {
    STATE.with(|s| {
        s.borrow()
            .entries
            .iter()
            .find(|e| e.guid == guid)
            .map(|e| e.path.clone())
    })
}

/// A path for `"guid:<guid>"` sources (others unchanged; an unknown GUID stays as given).
pub fn resolve(src: &str) -> String {
    match src.strip_prefix("guid:") {
        Some(guid) => guid_to_path(guid).unwrap_or_else(|| src.to_string()),
        None => src.to_string(),
    }
}

#[ts_api(namespace = "Assets")]
pub mod assets_api {
    use super::{STATE, clean};
    use ts_api::Callback;

    /// Calls `f(paths)` when asset files change on disk (the paths, one per line); returns a token
    /// for `offChanged`.
    #[subscription(off = "off_changed")]
    pub fn on_changed(f: Callback<(String,)>) -> u32 {
        STATE.with(|s| s.borrow_mut().listeners.add(f))
    }

    /// Stops a listener from `onChanged`; false when the token is unknown.
    pub fn off_changed(token: u32) -> bool {
        STATE.with(|s| s.borrow_mut().listeners.remove(token))
    }

    /// How many assets have a `.meta` file.
    pub fn count() -> f64 {
        STATE.with(|s| s.borrow().entries.len() as f64)
    }

    /// The path of the i-th asset ("" past the end).
    pub fn path_at(index: f64) -> String {
        STATE.with(|s| {
            s.borrow()
                .entries
                .get(index as usize)
                .map(|e| e.path.clone())
                .unwrap_or_default()
        })
    }

    /// The GUID of the i-th asset ("" past the end).
    pub fn guid_at(index: f64) -> String {
        STATE.with(|s| {
            s.borrow()
                .entries
                .get(index as usize)
                .map(|e| e.guid.clone())
                .unwrap_or_default()
        })
    }

    /// The game path of the asset with this GUID ("" when there is none).
    pub fn guid_to_path(guid: &str) -> String {
        super::guid_to_path(guid).unwrap_or_default()
    }

    /// The GUID of the asset at a game path ("" when it has no `.meta`).
    pub fn path_to_guid(path: &str) -> String {
        let p = clean(path);
        STATE.with(|s| {
            s.borrow()
                .entries
                .iter()
                .find(|e| e.path == p)
                .map(|e| e.guid.clone())
                .unwrap_or_default()
        })
    }

    /// An asset's importer settings from its `.meta`, as a JSON object (`{}` without one).
    pub fn importer(path: &str) -> String {
        let p = clean(&super::resolve(path));
        STATE.with(|s| {
            s.borrow()
                .entries
                .iter()
                .find(|e| e.path == p)
                .map_or_else(|| "{}".to_string(), |e| e.importer_json.clone())
        })
    }

    /// A path for `"guid:<guid>"` sources (others unchanged). Unknown GUIDs stay as given.
    pub fn resolve(src: &str) -> String {
        super::resolve(src)
    }

    /// Whether the game can load this path: a file of a pack or of the game folder, a `data:`
    /// URL, or a `guid:` source of a known asset.
    pub fn exists(path: &str) -> bool {
        path.trim_start().starts_with("data:") || super::indexed(path).is_some()
    }

    /// The paths of every asset matching a glob (`*` and `?` inside one folder name, `**` across
    /// folders, case-sensitive; a plain folder matches everything below it), sorted.
    pub fn list(glob: &str) -> Vec<String> {
        STATE.with(|s| {
            s.borrow()
                .index
                .iter()
                .filter(|(k, _)| platypus_core::glob::include_match(glob.trim(), k))
                .map(|(k, _)| k.clone())
                .collect()
        })
    }

    /// The pack that holds the asset at this path (`assets`, `scene-level1`), `""` for a loose
    /// file (the dev loop), a `data:` URL or a path nothing has.
    pub fn pack_of(path: &str) -> String {
        super::indexed(path)
            .map(|(_, pack)| pack)
            .unwrap_or_default()
    }

    /// The paths of asset files that changed since the last call, one per line.
    pub fn take_changes() -> String {
        STATE.with(|s| std::mem::take(&mut s.borrow_mut().changed).join("\n"))
    }
}
