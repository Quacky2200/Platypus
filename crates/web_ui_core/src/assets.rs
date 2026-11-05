// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Resolving asset paths (`url(...)` images, fonts) that scripts give the engine.

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Random access to one asset without reading it all (streamed sounds): packs decode the
/// chunks a read covers, folders seek.
pub trait RangeRead: Send + Sync {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Reads up to `buf.len()` bytes at `offset`; 0 at or past the end.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize>;
}

/// The files a game can load: the app installs one ([`set_asset_fs`]) over packs and/or the game
/// folder, and every asset read ([`read_asset`]) goes through it. Paths are game paths.
pub trait AssetFs: Send + Sync {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>>;
    /// A handle for reading the asset in pieces.
    fn open_range(&self, path: &str) -> std::io::Result<Arc<dyn RangeRead>>;
    fn exists(&self, path: &str) -> bool;
    /// Every visible asset path, sorted.
    fn list(&self) -> Vec<String>;
    /// The pack that holds `path` (`assets`, `scene-level1`), `""` for a loose file or a miss.
    fn pack_of(&self, path: &str) -> String;
}

static FS: RwLock<Option<Arc<dyn AssetFs>>> = RwLock::new(None);

/// Installs the file system (or removes it with `None`: reads go to the folder `root` they are
/// given, as before).
pub fn set_asset_fs(fs: Option<Arc<dyn AssetFs>>) {
    *FS.write().expect("not poisoned") = fs;
}

thread_local! {
    static LOCAL: std::cell::RefCell<Option<Arc<dyn AssetFs>>> = const { std::cell::RefCell::new(None) };
}

/// The installed file system: this thread's [`with_asset_fs`] one, else the app's.
pub fn asset_fs() -> Option<Arc<dyn AssetFs>> {
    LOCAL
        .with(|l| l.borrow().clone())
        .or_else(|| FS.read().expect("not poisoned").clone())
}

/// Runs `f` with `fs` as this thread's file system only (tests: other threads and the app's own
/// are unaffected).
pub fn with_asset_fs<R>(fs: Arc<dyn AssetFs>, f: impl FnOnce() -> R) -> R {
    let previous = LOCAL.with(|l| l.replace(Some(fs)));
    let result = f();
    LOCAL.with(|l| *l.borrow_mut() = previous);
    result
}

/// The bytes of an asset: a `data:` URL decoded, else the installed file system's answer, else
/// `root/<path>` (checked by [`resolve_asset`]).
pub fn read_asset(root: &Path, path: &str) -> std::io::Result<Vec<u8>> {
    if path.trim_start().starts_with("data:") {
        return decode_data_url(path)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad data URL"));
    }
    if let Some(fs) = asset_fs() {
        return fs.read(path);
    }
    let file = resolve_asset(root, path).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("`{path}` is outside the game folder (asset paths are relative to it)"),
        )
    })?;
    std::fs::read(&file)
        .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", file.display())))
}

/// [`read_asset`] as text.
pub fn read_asset_string(root: &Path, path: &str) -> std::io::Result<String> {
    String::from_utf8(read_asset(root, path)?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Decodes a `data:` URL (`data:[<type>][;base64],<data>`), as Bun emits for small
/// assets referenced from CSS. Returns `None` if `url` isn't a valid data URL.
pub fn decode_data_url(url: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let rest = url.trim().strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if meta.ends_with(";base64") {
        let cleaned: String = data.chars().filter(|c| !c.is_whitespace()).collect();
        base64::engine::general_purpose::STANDARD
            .decode(cleaned)
            .ok()
    } else {
        Some(percent_decode(data))
    }
}

fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..(i + 3).min(s.len())], 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

static PLAN: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
static WARNED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Dev loop: the game paths a packaged build would hold (`build/assets.plan`, written by
/// `platypus-build build`). Afterwards [`resolve_asset`] warns once for a file that exists but is not
/// in it, since a packaged game would not find it.
pub fn set_pack_plan(keys: impl IntoIterator<Item = String>) {
    let _ = PLAN.set(keys.into_iter().collect());
}

/// The warning for a loaded `key` that a packaged build would not include, if any.
pub fn plan_warning(plan: &std::collections::HashSet<String>, key: &str) -> Option<String> {
    let skip = key.ends_with(".meta") || key.starts_with("build/") || key.starts_with(".platypus/");
    (!skip && !plan.contains(key)).then(|| {
        format!(
            "asset \"{key}\" loaded, but a packaged build would not include it (add it to \"include\" in project.json)"
        )
    })
}

fn check_plan(url: &str, path: &Path) {
    let Some(plan) = PLAN.get() else { return };
    let key = url
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != ".")
        .collect::<Vec<_>>()
        .join("/");
    if let Some(w) = plan_warning(plan, &key)
        && path.is_file()
        && let Ok(mut seen) = WARNED.lock()
        && !seen.contains(&key)
    {
        seen.push(key);
        tracing::warn!("{w}");
    }
}

/// Resolves `url` against the game folder `root`, refusing anything that could
/// leave it: absolute paths, drive letters, URL schemes and `..` components.
/// Game scripts can't use this to read arbitrary files.
pub fn resolve_asset(root: &Path, url: &str) -> Option<PathBuf> {
    let path = resolve_asset_quiet(root, url)?;
    check_plan(url.trim(), &path);
    Some(path)
}

/// [`resolve_asset`] without the dev-loop plan warning: for looking a file up (listing, `exists`)
/// rather than loading it.
pub fn resolve_asset_quiet(root: &Path, url: &str) -> Option<PathBuf> {
    let url = url.trim();
    if url.is_empty() || url.contains(':') || url.starts_with(['/', '\\']) {
        return None;
    }
    let relative = Path::new(url);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    Some(root.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_data_urls() {
        assert_eq!(
            decode_data_url("data:text/plain;base64,aGk=").as_deref(),
            Some(&b"hi"[..])
        );
        assert_eq!(decode_data_url("data:,a%20b").as_deref(), Some(&b"a b"[..]));
        assert_eq!(decode_data_url("data:;base64,!!!"), None);
        assert_eq!(decode_data_url("assets/x.png"), None);
    }

    #[test]
    fn urls_cannot_escape_the_game_folder() {
        let root = Path::new("game");
        assert_eq!(
            resolve_asset(root, "ui/panel.png"),
            Some(root.join("ui/panel.png"))
        );
        assert_eq!(resolve_asset(root, "./a.png"), Some(root.join("./a.png")));
        for bad in [
            "",
            "../secret.png",
            "ui/../../x.png",
            "/etc/passwd",
            "\\\\server\\x.png",
            "C:/x.png",
            "https://example.com/x.png",
        ] {
            assert_eq!(resolve_asset(root, bad), None, "{bad:?}");
        }
    }

    // M9-API-1143
    #[test]
    fn a_loaded_file_outside_the_plan_gets_one_warning_text() {
        let plan: std::collections::HashSet<String> =
            ["assets/a.png".to_owned()].into_iter().collect();
        assert_eq!(plan_warning(&plan, "assets/a.png"), None);
        assert_eq!(plan_warning(&plan, "assets/a.png.meta"), None);
        assert_eq!(plan_warning(&plan, "build/big-1.ttf"), None);
        let w = plan_warning(&plan, "assets/b.wav").unwrap();
        assert!(w.contains("assets/b.wav") && w.contains("include"), "{w}");
    }
}
