// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `platypus-build package <game>`: one executable for a game. A copy of the engine host
//! (`platypus-app`) gets the game's icon and version info (Windows PE resources, `pe.rs`) and the
//! game's code (bundle, CSS, metadata, icon) as a payload at its end (`platypus_core::payload`). The
//! assets are copied to a `data/` folder beside it, never embedded, and the third-party notices
//! (`THIRD-PARTY-NOTICES.md`, built into this program; `--no-notices` leaves them out) are
//! written beside it. See `docs/platypus-build.md`.

use crate::{Options, build, icon, include, packs, pe};
use platypus_core::payload::{self, Archive};
use platypus_core::project::{
    FixedUser, LoadOptions, Platform, Project, Renderer, SystemUser, UserInfo,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// What the executable says about itself. Resolved from the project (`project.json`, else
/// `package.json`) and the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    /// The file name without `.exe`.
    pub exe: String,
    pub company: String,
    pub identifier: String,
    pub version: String,
    pub copyright: String,
    pub description: String,
    /// The icon file (`.png` or `.ico`), relative to the game folder, or as given on the command line.
    pub icon: Option<String>,
    /// `private`, `custom` or an SPDX identifier.
    pub license: String,
}

impl Metadata {
    /// The Windows file name (`OriginalFilename`).
    pub fn file_name(&self) -> String {
        format!("{}.exe", self.exe)
    }
}

/// Command-line overrides of [`Metadata`].
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub title: Option<String>,
    pub exe: Option<String>,
    pub company: Option<String>,
    pub version: Option<String>,
    pub copyright: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
}

/// The command-line overrides as the loader takes them (literal values, field names).
fn override_pairs(ov: &Overrides) -> Vec<(String, String)> {
    [
        ("title", &ov.title),
        ("exe", &ov.exe),
        ("company", &ov.company),
        ("version", &ov.version),
        ("copyright", &ov.copyright),
        ("description", &ov.description),
        ("icon", &ov.icon),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.as_ref().map(|v| (k.to_owned(), v.clone())))
    .collect()
}

impl Metadata {
    pub fn from_project(p: &Project) -> Metadata {
        Metadata {
            title: p.title.clone(),
            exe: p.exe.clone(),
            company: p.company.clone(),
            identifier: p.identifier.clone(),
            version: p.version.clone(),
            copyright: p.copyright.clone(),
            description: p.description.clone(),
            icon: p.icon.clone(),
            license: p.license.clone(),
        }
    }
}

/// The minimal `package.json` of the payload: name, version, description and the npm spelling of
/// the license (`UNLICENSED` for `private`), for tools that look for one.
pub fn payload_npm_json(project: &Project) -> Value {
    serde_json::json!({
        "name": project.name,
        "version": project.version,
        "description": project.description,
        "license": platypus_core::project::npm_license(&project.license, project.license_file.as_deref()),
    })
}

const LICENSE_NAMES: &[&str] = &[
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    "COPYING",
    "UNLICENSE",
];

/// The license text to ship: `licenseFile`, else the first of LICENSE, LICENSE.md, ... in the
/// game folder. None for `private` and for an SPDX license without a file (the identifier alone
/// goes into the metadata); an error for `custom` without a file or a missing `licenseFile`.
pub fn find_license_file(game: &Path, project: &Project) -> Result<Option<PathBuf>, String> {
    if project.license == "private" || project.license.is_empty() {
        return Ok(None);
    }
    if let Some(rel) = &project.license_file {
        if !payload::safe_path(rel) {
            return Err(format!("licenseFile: unsafe path {rel:?}"));
        }
        let p = game.join(rel);
        return if p.is_file() {
            Ok(Some(p))
        } else {
            Err(format!("licenseFile {} does not exist", p.display()))
        };
    }
    let found = LICENSE_NAMES
        .iter()
        .map(|n| game.join(n))
        .find(|p| p.is_file());
    if found.is_none() && project.license == "custom" {
        return Err("license is `custom` but there is no licenseFile and no LICENSE file".into());
    }
    Ok(found)
}

/// Resolves the project of `game` for `platform` (`platypus_core::project`: `project.json` with its
/// templates and platform section, else the `platypus` section of `package.json`), with the command
/// line on top, and the metadata of the executable.
pub fn resolve(
    game: &Path,
    ov: &Overrides,
    platform: Platform,
    user: &dyn UserInfo,
) -> Result<(Metadata, Project), String> {
    let mut opts = LoadOptions::new(platform, user);
    opts.overrides = override_pairs(ov);
    let project = Project::load(game, &opts).map_err(|e| e.to_string())?;
    Ok((Metadata::from_project(&project), project))
}

/// The `project.json` that goes into the payload: the resolved project (no templates, no platform
/// sections) with the icon's name inside the payload and where the exe looks for its data, so the
/// running game (`Application`, the window title and icon, the settings folder) sees the same
/// values as the version info.
pub fn payload_project_json(project: &Project, icon_entry: Option<&str>, data: &DataInfo) -> Value {
    let mut p = project.clone();
    p.icon = icon_entry.map(str::to_owned);
    p.data_dir = Some(data.dir.clone());
    p.data_required = Some(data.required);
    p.to_json()
}

fn io(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

/// Every file under `dir` as `/`-separated paths relative to `base`, skipping dot names.
fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| io(dir, e))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        if path.is_dir() {
            walk(base, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(base) {
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push(rel);
        }
    }
    Ok(())
}

/// The name of the data folder and whether the game needs it to run (`platypus.dataDir`,
/// `platypus.dataRequired` of the payload's `package.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataInfo {
    pub dir: String,
    pub required: bool,
}

/// The default name of the folder beside the exe that holds the assets.
pub const DEFAULT_DATA_DIR: &str = "data";

/// The most the embedded essentials may add up to: past this, none of them are embedded.
pub const ESSENTIALS_LIMIT: u64 = 256 * 1024;

/// An asset file that goes to the data folder (not read into memory here: games have gigabytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFile {
    /// Relative to the game folder; the same path under `data/`.
    pub rel: String,
    pub src: PathBuf,
    pub size: u64,
    /// Why the file is taken (a pack build places files by it).
    pub reason: include::Reason,
}

/// What a game is made of: the small payload and the files for the data folder.
#[derive(Debug)]
pub struct Collected {
    /// The payload entries (path relative to the game folder).
    pub payload: Vec<(String, Vec<u8>)>,
    /// Everything for `data/`, including the files also embedded as essentials.
    pub data: Vec<DataFile>,
    pub info: DataInfo,
    /// Problems with what was selected (an `include` that matches nothing).
    pub warnings: Vec<String>,
}

/// `dir` is one plain folder name (no separators, not hidden).
pub fn valid_data_dir(dir: &str) -> bool {
    !dir.is_empty()
        && !dir.starts_with('.')
        && !dir.contains(['/', '\\', ':'])
        && payload::safe_path(dir)
}

/// An absolute, `.`/`..`-free form of `p` for comparing (no file system access).
fn abs(p: &Path) -> PathBuf {
    let p = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Copies the data files under `dir` (same relative paths). Returns the bytes written.
pub fn copy_data(files: &[DataFile], dir: &Path) -> Result<u64, String> {
    let mut total = 0;
    for f in files {
        let to = dir.join(&f.rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        total += std::fs::copy(&f.src, &to).map_err(|e| io(&to, e))?;
    }
    Ok(total)
}

/// The files of the game.
///
/// The **payload** holds only `build/game.pbundle`, `build/game.css`, the rewritten `package.json`
/// (`payload_package_json`), the icon as `platypus-icon.<ext>` and the *essentials*: the assets the
/// bundle itself names (`assets` in `game.pbundle`, no `.meta`), but only when together they are
/// at most [`ESSENTIALS_LIMIT`] (256 KiB); past it none is embedded, a large file never is.
///
/// The **data** files are everything the game may read: every asset the bundle names, the scene
/// `platypus.startScene`, everything `include` selects (paths, folders, globs; default `["assets/**"]`, minus `exclude`) and the matches of the dynamic path patterns the build found, each
/// with its `.meta`, and the other files of `build/` (what the CSS references). Not `game.o`,
/// `game_bindings.rs`, `*.exe`, `game.js`, source maps, dot files, copies of assets in `build/` or
/// anything under `exclude` (the output's own data folder).
///
/// `info.required` is false when the game has no data files, or when the essentials are all of
/// them (apart from `.meta`): such a game starts without a data folder.
pub fn collect_files(
    game: &Path,
    project: &Project,
    icon_bytes: Option<(&str, Vec<u8>)>,
    data_dir: &str,
    exclude: &[PathBuf],
) -> Result<Collected, String> {
    let bundle_path = game.join("build/game.pbundle");
    let bundle: Value = serde_json::from_str(
        &std::fs::read_to_string(&bundle_path).map_err(|e| io(&bundle_path, e))?,
    )
    .map_err(|e| format!("{}: {e}", bundle_path.display()))?;

    // What to take from the game folder itself.
    let mut named: Vec<String> = Vec::new();
    for a in bundle["assets"].as_array().into_iter().flatten() {
        named.extend(a.as_str().map(str::to_owned));
    }
    let mut wanted = named.clone();
    wanted.extend(project.start_scene.clone());
    // Why each file is taken, strongest first: named by the code, reached by a dynamic path, `include`.
    let mut reasons: std::collections::HashMap<String, include::Reason> =
        std::collections::HashMap::new();
    for f in &wanted {
        reasons.entry(f.clone()).or_insert(include::Reason::Code);
    }
    // The build finds assets by the string literals of the program; a path built at run time
    // (`"assets/" + name`) is a pattern (`assets/*.png`, from the bundle's `patterns`), and
    // CSS text is not seen at all, so the `assets` folder goes in whole unless `include` says
    // what to take instead. `include` and `exclude` hold paths, folders and globs.
    let mut warnings = Vec::new();
    let all = include::game_files(game);
    for inc in project.include.iter().flatten() {
        let rel = inc.trim_start_matches("./").trim_end_matches('/');
        if !payload::safe_path(rel) {
            return Err(format!("include: unsafe path {rel:?}"));
        }
    }
    let selected = include::select(&all, project.include.as_deref(), project.exclude.as_deref());
    for u in &selected.unmatched {
        warnings.push(format!("include {u:?} matches no file"));
    }
    let included = selected.files;
    let excluded = |f: &str| {
        project
            .exclude
            .iter()
            .flatten()
            .any(|x| platypus_core::glob::include_match(x, f))
    };
    for p in bundle["patterns"].as_array().into_iter().flatten() {
        if let Some(glob) = p["glob"].as_str() {
            for f in include::match_pattern(&all, glob)
                .into_iter()
                .filter(|f| !excluded(f))
            {
                reasons.entry(f.clone()).or_insert(include::Reason::Pattern);
                wanted.push(f);
            }
        }
    }
    for f in included {
        reasons.entry(f.clone()).or_insert(include::Reason::Include);
        wanted.push(f);
    }
    let mut paths: Vec<String> = Vec::new();
    for rel in wanted {
        if !payload::safe_path(&rel) {
            return Err(format!("unsafe asset path {rel:?}"));
        }
        for r in [rel.clone(), format!("{rel}.meta")] {
            if !paths.contains(&r) && game.join(&r).is_file() {
                paths.push(r);
            }
        }
    }
    // build/: all but the copies of assets already taken, build products and executables.
    let mut built = Vec::new();
    if game.join("build").is_dir() {
        walk(game, &game.join("build"), &mut built)?;
    }
    for rel in built {
        let inner = rel.strip_prefix("build/").unwrap_or(&rel);
        let skip = matches!(
            inner,
            "game.pbundle"
                | "game.css"
                | "game.o"
                | "game_bindings.rs"
                | "game.js"
                | "vendor.js"
                | "assets.plan"
        ) || inner.ends_with(".map")
            || inner.ends_with(".exe")
            || paths.iter().any(|p| p == inner)
            || exclude.iter().any(|x| abs(&game.join(&rel)).starts_with(x));
        if !skip && !paths.contains(&rel) {
            paths.push(rel);
        }
    }

    let mut data = Vec::new();
    for rel in paths {
        let src = game.join(&rel);
        let size = std::fs::metadata(&src).map_err(|e| io(&src, e))?.len();
        let base = rel.strip_suffix(".meta").unwrap_or(&rel);
        let reason = reasons.get(base).copied().unwrap_or(include::Reason::Code);
        data.push(DataFile {
            rel,
            src,
            size,
            reason,
        });
    }

    let mut files = Vec::new();
    for rel in ["build/game.pbundle", "build/game.css"] {
        let p = game.join(rel);
        match std::fs::read(&p) {
            Ok(bytes) => files.push((rel.to_string(), bytes)),
            Err(e) if rel == "build/game.pbundle" => return Err(io(&p, e)),
            Err(_) => {}
        }
    }
    // The essentials: the assets the bundle names, if all of them are tiny together.
    let essential: Vec<&DataFile> = data
        .iter()
        .filter(|f| !f.rel.ends_with(".meta") && named.contains(&f.rel))
        .collect();
    let embed = essential.iter().map(|f| f.size).sum::<u64>() <= ESSENTIALS_LIMIT;
    let embedded: Vec<&str> = if embed {
        essential.iter().map(|f| f.rel.as_str()).collect()
    } else {
        Vec::new()
    };
    for f in &data {
        if embedded.contains(&f.rel.as_str()) {
            let bytes = std::fs::read(&f.src).map_err(|e| io(&f.src, e))?;
            files.push((f.rel.clone(), bytes));
        }
    }
    let info = DataInfo {
        dir: data_dir.to_string(),
        required: data
            .iter()
            .any(|f| !f.rel.ends_with(".meta") && !embedded.contains(&f.rel.as_str())),
    };

    let icon_entry = icon_bytes.as_ref().map(|(name, _)| name.to_string());
    files.push((
        "package.json".into(),
        serde_json::to_vec_pretty(&payload_npm_json(project)).map_err(|e| e.to_string())?,
    ));
    let json = payload_project_json(project, icon_entry.as_deref(), &info);
    files.push((
        "project.json".into(),
        serde_json::to_vec_pretty(&json).map_err(|e| e.to_string())?,
    ));
    if let Some((name, bytes)) = icon_bytes {
        files.push((name.to_string(), bytes));
    }
    Ok(Collected {
        payload: files,
        data,
        info,
        warnings,
    })
}

/// How to package.
#[derive(Debug, Clone, Default)]
pub struct PackageOptions {
    /// The engine host to copy (default: `find_host`).
    pub host: Option<PathBuf>,
    /// Where to write (default `<game>/build/<exe>.exe`).
    pub out: Option<PathBuf>,
    pub overrides: Overrides,
    /// Do not build first: use the `build/` that is there.
    pub no_build: bool,
    pub no_tsc: bool,
    /// A command to sign the finished file; `{exe}` is replaced by its (quoted) path.
    pub sign_cmd: Option<String>,
    /// The folder beside the exe that gets the assets (default `data`).
    pub data_dir: Option<String>,
    /// Do not copy the assets (the exe still looks for the data folder).
    pub no_data: bool,
    /// The person the templates of `project.json` see (default: the system's user).
    pub user: Option<FixedUser>,
    /// Write the data as packs (`data/assets.dat`, `data/scene-<slug>.dat`, `data/index.json`)
    /// instead of loose files. Opt-in: the hosts do not read packs yet.
    pub packs: bool,
    /// The codec of the packs (`--codec`, default LZ4).
    pub codec: Option<platypus_pack::Codec>,
    /// List every file of every pack in the report.
    pub report: bool,
    /// `--video-settings-prompt` / `--no-video-settings-prompt`: the exe's `videoSettingsPrompt`,
    /// over the project's.
    pub video_settings_prompt: Option<bool>,
    /// `--renderer vulkan|gles|cpu|auto`: the exe's `renderer`, over the project's.
    pub renderer: Option<Renderer>,
    /// `--no-notices`: do not write `THIRD-PARTY-NOTICES.md` beside the exe (the exe still
    /// prints them with `--licenses`).
    pub no_notices: bool,
    /// `--debug-info`: write `<exe>.pdebug` beside the exe, so a crash report can name the
    /// TypeScript file and line. Left out by default: a shipped game carries no source map.
    pub debug_info: bool,
    /// `--steam-library <file>`: the Steam library to copy beside the exe of a game with
    /// `steam.appId`, instead of looking for it (`steam::find_library`).
    pub steam_library: Option<PathBuf>,
}

/// The notices of everything the host is built from (generated by `tools/platypus_notices` from
/// Cargo.lock; the same text the host prints with `--licenses`), written beside every packaged
/// exe. Built in, so a game folder outside the repository gets them too.
pub const NOTICES: &str = include_str!("../../../THIRD-PARTY-NOTICES.md");
/// Their file name beside the exe.
pub const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.md";

#[derive(Debug)]
pub struct Packaged {
    pub path: PathBuf,
    /// The engine host the exe was made from.
    pub host: PathBuf,
    pub size: u64,
    pub payload_size: u64,
    pub files: usize,
    /// The data folder, when assets were copied.
    pub data_path: Option<PathBuf>,
    pub data_files: usize,
    pub data_bytes: u64,
    pub info: DataInfo,
    /// The packs written instead of loose data files (`--packs`).
    pub pack_report: Option<packs::PackReport>,
    /// The names of the embedded files.
    pub embedded: Vec<String>,
    pub meta: Metadata,
    /// License files copied next to the exe (and into the data folder).
    pub license_files: Vec<PathBuf>,
    /// `THIRD-PARTY-NOTICES.md` beside the exe (`None` with `--no-notices`).
    pub notices: Option<PathBuf>,
    /// `<exe>.pdebug` beside the exe (`--debug-info`).
    pub debug_info: Option<PathBuf>,
    /// The Steam library copied beside the exe (a game with `steam.appId`).
    pub steam_library: Option<PathBuf>,
    /// Windows resources were written (the host is a PE file).
    pub resources: bool,
    /// Warnings of the build, empty metadata fields and fields still at their defaults.
    pub warnings: Vec<String>,
}

fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// The engine host's name, without extension.
pub const HOST: &str = "platypus-app";

/// Where cargo puts the root workspace's output (`[build] target-dir` in `.cargo/config.toml`).
pub const CARGO_OUT: &str = "build";

/// The `platypus-app` cargo builds in the checkout `repo` for `profile` (`release`, `debug`):
/// under `$CARGO_TARGET_DIR` when it is set, else `<repo>/build/<profile>/`.
pub fn built_host(repo: &Path, profile: &str) -> PathBuf {
    let out = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join(CARGO_OUT));
    out.join(profile).join(exe_name(HOST))
}

/// The engine host (`platypus-app`): `--host`, `$PLATYPUS_HOST`, the host next to this program,
/// or the newest `build/{release,debug}/platypus-app` (cargo's output, [`built_host`]) found going
/// up from the game folder and the current folder.
pub fn find_host(game: &Path, explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return if p.is_file() {
            Ok(p.to_path_buf())
        } else {
            Err(format!("host {} not found", p.display()))
        };
    }
    if let Some(p) = std::env::var_os("PLATYPUS_HOST").map(PathBuf::from) {
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!("$PLATYPUS_HOST {} not found", p.display()))
        };
    }
    let mut found: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        found.push(dir.join(exe_name(HOST)));
    }
    let mut starts = vec![game.to_path_buf()];
    starts.extend(std::env::current_dir().ok());
    for s in starts {
        let s = s.canonicalize().unwrap_or(s);
        for a in s.ancestors() {
            for profile in ["release", "debug"] {
                found.push(built_host(a, profile));
            }
        }
    }
    newest_host(found).ok_or_else(|| {
        format!(
            "no {HOST} host found (cargo build --release -p platypus_app, or pass --host <path>)"
        )
    })
}

/// The most recently built of the candidate hosts. Not "release first": a stale release build
/// from before the payload support would win over a fresh debug one and die with
/// `.\build\game.js not found`. Equal times go to the earlier candidate.
pub fn newest_host(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for p in candidates {
        let Ok(t) = std::fs::metadata(&p).and_then(|m| m.modified()) else {
            continue;
        };
        if !p.is_file() {
            continue;
        }
        if best.as_ref().is_none_or(|(bt, _)| t > *bt) {
            best = Some((t, p));
        }
    }
    best.map(|(_, p)| p)
}

fn run_sign(cmd: &str, exe: &Path) -> Result<(), String> {
    let line = cmd.replace("{exe}", &format!("\"{}\"", exe.display()));
    let mut c;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c = std::process::Command::new("cmd");
        c.args(["/S", "/C"]).raw_arg(format!("\"{line}\""));
    }
    #[cfg(not(windows))]
    {
        c = std::process::Command::new("sh");
        c.args(["-c", &line]);
    }
    let status = c.status().map_err(|e| format!("sign command: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("sign command failed ({status}): {line}"))
    }
}

/// Packages the game in `game`.
pub fn package(game: &Path, opts: &PackageOptions) -> Result<Packaged, String> {
    let mut warnings = Vec::new();
    if !opts.no_build {
        let report = build(
            game,
            &Options {
                no_tsc: opts.no_tsc,
                ..Default::default()
            },
        )?;
        warnings = report.warnings;
    }
    // The host without any payload of its own (read first: its kind picks the platform section).
    let host_path = find_host(game, opts.host.as_deref())?;
    let host_len = payload::host_len(&host_path)?;
    let mut host = std::fs::read(&host_path).map_err(|e| io(&host_path, e))?;
    host.truncate(host_len as usize);
    let is_pe = host.starts_with(b"MZ");
    let platform = if is_pe {
        Platform::Windows
    } else if host.starts_with(b"\x7fELF") {
        Platform::Linux
    } else if host.starts_with(&[0xCF, 0xFA, 0xED, 0xFE])
        || host.starts_with(&[0xCA, 0xFE, 0xBA, 0xBE])
    {
        Platform::Macos
    } else {
        Platform::current()
    };
    let user: &dyn UserInfo = match &opts.user {
        Some(u) => u,
        None => &SystemUser,
    };
    let (meta, mut project) = resolve(game, &opts.overrides, platform, user)?;
    // The build argument goes into the exe's own `project.json`, so it wins over the game's.
    if let Some(on) = opts.video_settings_prompt {
        project.video_settings_prompt = Some(on);
    }
    if let Some(r) = opts.renderer {
        project.renderer = r;
        // The build warned for the project's renderer; this one is the exe's.
        warnings.retain(|w| !w.starts_with(crate::RENDERER_WARNING));
        if let Some(w) = crate::renderer_warning(r, &crate::read_usage(game)) {
            warnings.push(w);
        }
    }
    // Never blocks: the exe is made, the person is told what its version info lacks.
    for w in project
        .empty_field_warnings()
        .into_iter()
        .chain(project.default_field_warnings())
    {
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    }

    // The icon: relative to the game folder (from package.json) or a path given on the command line.
    let icon_path = meta.icon.as_deref().map(|i| {
        let p = PathBuf::from(i);
        if opts.overrides.icon.is_some() || p.is_absolute() {
            p
        } else {
            game.join(i)
        }
    });
    let icon = match &icon_path {
        Some(p) => {
            let data = std::fs::read(p).map_err(|e| io(p, e))?;
            // Fail now, not on the player's machine, if it is not an image.
            icon::to_ico(&data).map_err(|e| format!("{}: {e}", p.display()))?;
            let ext = if icon::is_ico(&data) { "ico" } else { "png" };
            Some((format!("platypus-icon.{ext}"), data))
        }
        None => None,
    };

    let data_dir = opts
        .data_dir
        .clone()
        .unwrap_or_else(|| DEFAULT_DATA_DIR.to_string());
    if !valid_data_dir(&data_dir) {
        return Err(format!(
            "--data-dir {data_dir:?} is not a plain folder name"
        ));
    }

    let out = opts.out.clone().unwrap_or_else(|| {
        let name = if is_pe {
            meta.file_name()
        } else {
            meta.exe.clone()
        };
        game.join("build").join(name)
    });
    let data_path = out
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join(&data_dir);
    let collected = collect_files(
        game,
        &project,
        icon.as_ref().map(|(n, d)| (n.as_str(), d.clone())),
        &data_dir,
        &[abs(&data_path)],
    )?;
    for w in &collected.warnings {
        if !warnings.contains(w) {
            warnings.push(w.clone());
        }
    }
    if is_pe {
        // The game's own icon, else Platypus's (whatever icon the host was built with).
        let ico = match &icon {
            Some((_, data)) => icon::to_ico(data)?,
            None => icon::default_ico(),
        };
        host = pe::set_resources(host, &meta, Some(&ico))?;
    }
    let payload_size = payload::write_payload(&mut host, &collected.payload)?;

    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, &host).map_err(|e| io(&tmp, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&tmp, &out).map_err(|e| io(&out, e))?;
    // The assets stay files beside the exe.
    let mut pack_report = None;
    let data_bytes = if opts.no_data || collected.data.is_empty() {
        0
    } else if opts.packs {
        let plan = packs::plan(game, &collected.data)?;
        let report = packs::write(
            &plan,
            &data_path,
            opts.codec.unwrap_or(platypus_pack::Codec::Lz4),
        )?;
        let bytes = report.bytes();
        pack_report = Some(report);
        bytes
    } else {
        copy_data(&collected.data, &data_path)?
    };
    let copied = !(opts.no_data || collected.data.is_empty());
    // The license text goes beside the exe (and into the data folder when it was named).
    let mut license_files = Vec::new();
    if let Some(src) = find_license_file(game, &project)? {
        let file = src
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| "LICENSE".into());
        let mut dests = vec![
            out.parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
                .join(&file),
        ];
        if copied && project.license_file.is_some() {
            dests.push(data_path.join(&file));
        }
        for d in dests {
            std::fs::copy(&src, &d).map_err(|e| io(&d, e))?;
            license_files.push(d);
        }
    }
    let notices = if opts.no_notices {
        None
    } else {
        let path = out
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
            .join(NOTICES_FILE);
        // `\n` line ends whatever the checkout has, as the host prints them.
        let text = NOTICES.replace("\r\n", "\n");
        std::fs::write(&path, text).map_err(|e| io(&path, e))?;
        Some(path)
    };
    let debug_info = if opts.debug_info {
        let from = game.join("build").join(crate::DEBUG_FILE);
        let to = out.with_extension("pdebug");
        std::fs::copy(&from, &to).map_err(|e| io(&from, e))?;
        Some(to)
    } else {
        None
    };
    let steam_library = match (&project.steam, crate::steam::library_name(platform)) {
        (Some(_), Some(name)) => {
            let host_dir = host_path.parent();
            match crate::steam::find_library(game, host_dir, name, opts.steam_library.as_deref())? {
                Some(src) => {
                    let dest = out
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_default()
                        .join(name);
                    std::fs::copy(&src, &dest).map_err(|e| io(&dest, e))?;
                    Some(dest)
                }
                None => {
                    warnings.push(format!(
                        "steam.appId is set but {name} was not found (put it in the game's steam/ folder or pass --steam-library; it comes from the Steamworks SDK, redistributable_bin): the game runs with Steam off until it is beside the exe"
                    ));
                    None
                }
            }
        }
        (None, _) if opts.steam_library.is_some() => {
            warnings.push(
                "--steam-library given but project.json has no steam.appId: not copied".to_string(),
            );
            None
        }
        _ => None,
    };
    if let Some(cmd) = &opts.sign_cmd {
        run_sign(cmd, &out)?;
    }
    // Read it back: a packaged file that cannot be opened is an error now.
    let check = Archive::open(&out)?.ok_or("the payload was not found in the output")?;
    let size = std::fs::metadata(&out).map_err(|e| io(&out, e))?.len();
    Ok(Packaged {
        path: out,
        host: host_path,
        size,
        payload_size,
        files: check.entries.len(),
        data_path: copied.then_some(data_path),
        data_files: match (&pack_report, copied) {
            (Some(r), _) => r.entries(),
            (None, true) => collected.data.len(),
            (None, false) => 0,
        },
        data_bytes,
        info: collected.info,
        pack_report,
        embedded: check.entries.iter().map(|e| e.path.clone()).collect(),
        meta,
        license_files,
        notices,
        debug_info,
        steam_library,
        resources: is_pe,
        warnings,
    })
}

const USAGE: &str = "usage: platypus-build package <game> [--host platypus-app.exe] [--out file] [--no-build] [--no-tsc]
       [--exe name] [--title t] [--company c] [--version v] [--copyright c] [--description d] [--icon file.png|ico]
       [--sign-cmd \"signtool sign ... {exe}\"] [--data-dir name] [--no-data] [--packs [--codec lz4|zstd] [--report]]
       [--video-settings-prompt | --no-video-settings-prompt] [--renderer vulkan|gles|cpu|auto] [--no-notices] [--debug-info]
       [--steam-library steam_api64.dll|libsteam_api.so|libsteam_api.dylib]";

/// The `package` command: `args` are the ones after the word `package`. Returns the exit code.
pub fn run_cli(args: &[String]) -> i32 {
    let mut opts = PackageOptions::default();
    let mut game = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = |it: &mut std::slice::Iter<'_, String>| it.next().cloned();
        match a.as_str() {
            "--no-build" => opts.no_build = true,
            "--no-tsc" => opts.no_tsc = true,
            "--host" => opts.host = value(&mut it).map(PathBuf::from),
            "--out" => opts.out = value(&mut it).map(PathBuf::from),
            "--exe" => opts.overrides.exe = value(&mut it),
            "--title" => opts.overrides.title = value(&mut it),
            "--company" => opts.overrides.company = value(&mut it),
            "--version" => opts.overrides.version = value(&mut it),
            "--copyright" => opts.overrides.copyright = value(&mut it),
            "--description" => opts.overrides.description = value(&mut it),
            "--icon" => opts.overrides.icon = value(&mut it),
            "--sign-cmd" => opts.sign_cmd = value(&mut it),
            "--data-dir" => opts.data_dir = value(&mut it),
            "--no-data" => opts.no_data = true,
            "--no-notices" => opts.no_notices = true,
            "--debug-info" => opts.debug_info = true,
            "--steam-library" => opts.steam_library = value(&mut it).map(PathBuf::from),
            "--packs" => opts.packs = true,
            "--codec" => match value(&mut it).map(|c| packs::parse_codec(&c)) {
                Some(Ok(c)) => opts.codec = Some(c),
                Some(Err(e)) => {
                    eprintln!("error: {e}");
                    return 2;
                }
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
            "--report" => opts.report = true,
            "--video-settings-prompt" => opts.video_settings_prompt = Some(true),
            "--no-video-settings-prompt" => opts.video_settings_prompt = Some(false),
            "--renderer" => match value(&mut it).as_deref().and_then(Renderer::parse) {
                Some(r) => opts.renderer = Some(r),
                None => {
                    eprintln!("error: --renderer takes vulkan, gles, cpu or auto\n{USAGE}");
                    return 2;
                }
            },
            s if !s.starts_with("--") && game.is_none() => game = Some(PathBuf::from(s)),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let game = game.unwrap_or_else(|| PathBuf::from("."));
    match package(&game, &opts) {
        Ok(p) => {
            println!("host: {}", p.host.display());
            println!(
                "{}: {} file(s) embedded ({} bytes), {} bytes total{}",
                p.path.display(),
                p.files,
                p.payload_size,
                p.size,
                if p.resources {
                    ", icon and version info set"
                } else {
                    ""
                }
            );
            if let Some(r) = &p.pack_report {
                print!("{}", r.listing(opts.report, 40));
                for n in &r.notes {
                    println!("note: {n}");
                }
            }
            match &p.data_path {
                Some(d) => println!(
                    "{}: {} asset file(s), {} bytes {}{}",
                    d.display(),
                    p.data_files,
                    p.data_bytes,
                    if p.pack_report.is_some() {
                        "in packs"
                    } else {
                        "copied"
                    },
                    if p.info.required {
                        " (the exe needs this folder beside it)"
                    } else {
                        ""
                    }
                ),
                None if p.info.required => println!(
                    "assets not copied (--no-data): the exe needs a `{}` folder beside it, --data <dir> or $PLATYPUS_DATA",
                    p.info.dir
                ),
                None => {}
            }
            match &p.notices {
                Some(n) => println!(
                    "{}: third-party notices (ship it with the exe)",
                    n.display()
                ),
                None => println!(
                    "notices not written (--no-notices): the exe prints them with --licenses"
                ),
            }
            if let Some(s) = &p.steam_library {
                println!(
                    "{}: Steam library (Valve's, from the Steamworks SDK: ship it beside the exe, check its terms)",
                    s.display()
                );
            }
            if let Some(d) = &p.debug_info {
                println!(
                    "{}: debug info (crash reports name TypeScript lines; ship it beside the exe only if you want that)",
                    d.display()
                );
            }
            for w in &p.warnings {
                eprintln!("warning: {w}");
            }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn proj(pkg: &Value, folder: &str, ov: &Overrides) -> Project {
        let user = FixedUser::default();
        let mut o = LoadOptions::new(Platform::Windows, &user);
        o.overrides = override_pairs(ov);
        Project::from_package_json_with(pkg, folder, &o)
    }

    fn resolve_json(pkg: &Value, folder: &str, ov: &Overrides) -> Metadata {
        Metadata::from_project(&proj(pkg, folder, ov))
    }

    fn pkg() -> Value {
        json!({
            "name": "my-game", "version": "1.2.3", "author": "Ann", "description": "Pkg description",
            "platypus": { "title": "My Game", "company": "Acme", "identifier": "com.acme.mygame", "icon": "assets/icon.png" }
        })
    }

    // M9-TS-2400
    #[test]
    fn metadata_defaults_come_from_package_json() {
        let m = resolve_json(&pkg(), "folder", &Overrides::default());
        assert_eq!(m.title, "My Game");
        assert_eq!(m.exe, "My Game");
        assert_eq!(m.company, "Acme");
        assert_eq!(m.version, "1.2.3");
        assert_eq!(m.copyright, "Copyright (c) Acme");
        assert_eq!(m.description, "Pkg description");
        assert_eq!(m.identifier, "com.acme.mygame");
        assert_eq!(m.icon.as_deref(), Some("assets/icon.png"));
        // Nothing at all: the folder names it.
        let m = resolve_json(&json!({}), "cool", &Overrides::default());
        assert_eq!(
            (m.title.as_str(), m.exe.as_str(), m.version.as_str()),
            ("cool", "cool", "0.0.0")
        );
        assert_eq!(m.copyright, "");
        assert_eq!(m.description, "cool");
        assert_eq!(m.identifier, "dev.platypus.cool");
    }

    // M9-TS-2401
    #[test]
    fn platypus_fields_and_flags_override_in_order() {
        let mut p = pkg();
        p["platypus"]["version"] = json!("2.0.0");
        p["platypus"]["exe"] = json!("mygame.exe");
        p["platypus"]["copyright"] = json!("(c) 2026 Acme Inc.");
        let m = resolve_json(&p, "f", &Overrides::default());
        assert_eq!((m.version.as_str(), m.exe.as_str()), ("2.0.0", "mygame"));
        assert_eq!(m.copyright, "(c) 2026 Acme Inc.");
        let ov = Overrides {
            version: Some("3.1".into()),
            exe: Some("bad/na:me".into()),
            icon: Some("x.ico".into()),
            description: Some("d".into()),
            ..Default::default()
        };
        let m = resolve_json(&p, "f", &ov);
        assert_eq!((m.version.as_str(), m.exe.as_str()), ("3.1", "bad_na_me"));
        assert_eq!(
            (m.icon.as_deref(), m.description.as_str()),
            (Some("x.ico"), "d")
        );
    }

    // M9-TS-2402
    #[test]
    fn the_payload_project_json_carries_the_resolved_metadata() {
        let project = proj(
            &pkg(),
            "f",
            &Overrides {
                version: Some("9.9.9".into()),
                ..Default::default()
            },
        );
        let data = DataInfo {
            dir: "data".into(),
            required: true,
        };
        let j = payload_project_json(&project, Some("platypus-icon.png"), &data);
        assert_eq!(j["resolved"], true);
        assert_eq!(j["version"], "9.9.9");
        assert_eq!(j["icon"], "platypus-icon.png");
        assert_eq!(j["title"], "My Game");
        assert_eq!(j["dataDir"], "data");
        assert_eq!(j["dataRequired"], true);
        assert_eq!(j["name"], "my-game");
        assert!(
            payload_project_json(&project, None, &data)
                .get("icon")
                .is_none()
        );
    }

    fn game_folder(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("platypus_pkg_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (p, c) in [
            (
                "build/game.pbundle",
                r#"{"format":1,"assets":["assets/a.png","scenes/s.scene.json","../evil"]}"#,
            ),
            ("build/game.css", "body{}"),
            ("build/font-1.ttf", "FONT"),
            ("build/assets/a.png", "COPY"),
            ("build/game.o", "OBJ"),
            ("build/old.exe", "EXE"),
            ("build/.platypus-reload", "{}"),
            ("assets/a.png", "PNG"),
            ("assets/a.png.meta", "META"),
            ("assets/unused.png", "UNUSED"),
            ("scenes/s.scene.json", "{}"),
            ("extra/x.txt", "X"),
            ("extra/sub/y.txt", "Y"),
        ] {
            let f = d.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, c).unwrap();
        }
        d
    }

    // M9-TS-2410
    #[test]
    fn collect_takes_build_the_named_assets_and_includes_but_not_the_rest() {
        let d = game_folder("collect");
        // The unsafe `../evil` in the asset list is refused.
        let mut p = pkg();
        assert!(
            collect_files(&d, &proj(&p, "f", &Overrides::default()), None, "data", &[])
                .unwrap_err()
                .contains("unsafe")
        );
        std::fs::write(
            d.join("build/game.pbundle"),
            r#"{"format":1,"assets":["assets/a.png","scenes/s.scene.json"]}"#,
        )
        .unwrap();
        p["platypus"]["include"] = json!(["extra"]);
        let c = collect_files(
            &d,
            &proj(&p, "f", &Overrides::default()),
            Some(("platypus-icon.png", vec![1])),
            "data",
            &[],
        )
        .unwrap();
        let data: Vec<&str> = c.data.iter().map(|f| f.rel.as_str()).collect();
        for want in [
            "build/font-1.ttf",
            "assets/a.png",
            "assets/a.png.meta",
            "scenes/s.scene.json",
            "extra/x.txt",
            "extra/sub/y.txt",
        ] {
            assert!(data.contains(&want), "{want} missing from {data:?}");
        }
        for not in [
            "build/game.pbundle",
            "build/game.css",
            "build/game.o",
            "build/old.exe",
            "build/.platypus-reload",
            "build/assets/a.png",
            "assets/unused.png",
        ] {
            assert!(!data.contains(&not), "{not} should be left out");
        }
        // The game folder's copy of an asset is the one copied.
        let a = c.data.iter().find(|f| f.rel == "assets/a.png").unwrap();
        assert_eq!(std::fs::read(&a.src).unwrap(), b"PNG");
        // Without `platypus.include` the whole assets folder goes to data.
        p["platypus"].as_object_mut().unwrap().remove("include");
        let c =
            collect_files(&d, &proj(&p, "f", &Overrides::default()), None, "data", &[]).unwrap();
        assert!(c.data.iter().any(|f| f.rel == "assets/unused.png"));
        assert!(!c.data.iter().any(|f| f.rel == "extra/x.txt"));
        // Anything under `exclude` (the output's own data folder) is not collected.
        let c = collect_files(
            &d,
            &proj(&p, "f", &Overrides::default()),
            None,
            "data",
            &[d.join("build")],
        )
        .unwrap();
        assert!(!c.data.iter().any(|f| f.rel.starts_with("build/")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2552
    #[test]
    fn globs_in_include_work_patterns_add_their_files_and_nothing_drops_silently() {
        let d = game_folder("globs");
        for f in [
            "assets/sfx/a.wav",
            "assets/sfx/b.wav",
            "assets/sfx/c.mp3",
            "assets/wip/w.psd",
            "assets/wip/v.png",
        ] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), f).unwrap();
        }
        std::fs::write(
            d.join("build/game.pbundle"),
            r#"{"format":1,"assets":["assets/a.png"],"patterns":[{"glob":"assets/sfx/*.*"}]}"#,
        )
        .unwrap();
        let mut p = pkg();
        p["platypus"]["include"] = json!(["extra/*.txt", "assets/nope/*.png"]);
        p["platypus"]["exclude"] = json!(["assets/sfx/b.wav"]);
        let c =
            collect_files(&d, &proj(&p, "f", &Overrides::default()), None, "data", &[]).unwrap();
        let data: Vec<&str> = c.data.iter().map(|f| f.rel.as_str()).collect();
        // The glob, the pattern's matches (minus exclude) and the named asset are taken.
        for want in [
            "extra/x.txt",
            "assets/sfx/a.wav",
            "assets/sfx/c.mp3",
            "assets/a.png",
        ] {
            assert!(data.contains(&want), "{want} missing from {data:?}");
        }
        for not in [
            "extra/sub/y.txt",
            "assets/sfx/b.wav",
            "assets/wip/v.png",
            "assets/unused.png",
        ] {
            assert!(!data.contains(&not), "{not} should be left out");
        }
        // A pattern that reaches nothing is said, not dropped.
        assert!(
            c.warnings.iter().any(|w| w.contains("assets/nope/*.png")),
            "{:?}",
            c.warnings
        );
        // `exclude` also trims the default include.
        p["platypus"].as_object_mut().unwrap().remove("include");
        p["platypus"]["exclude"] = json!(["**/*.psd", "assets/wip"]);
        let c =
            collect_files(&d, &proj(&p, "f", &Overrides::default()), None, "data", &[]).unwrap();
        let data: Vec<&str> = c.data.iter().map(|f| f.rel.as_str()).collect();
        assert!(data.contains(&"assets/sfx/b.wav") && data.contains(&"assets/unused.png"));
        assert!(!data.iter().any(|f| f.starts_with("assets/wip")));
        assert!(c.warnings.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2600
    #[test]
    fn the_payload_holds_only_code_and_tiny_essentials() {
        let d = game_folder("payload");
        std::fs::write(
            d.join("build/game.pbundle"),
            r#"{"format":1,"assets":["assets/a.png","scenes/s.scene.json"]}"#,
        )
        .unwrap();
        let p = pkg();
        let c = collect_files(
            &d,
            &proj(&p, "f", &Overrides::default()),
            Some(("platypus-icon.png", vec![1])),
            "data",
            &[],
        )
        .unwrap();
        let names: Vec<&str> = c.payload.iter().map(|(n, _)| n.as_str()).collect();
        // Small named assets are essentials: embedded, with the code.
        for want in [
            "build/game.pbundle",
            "build/game.css",
            "project.json",
            "platypus-icon.png",
            "assets/a.png",
            "scenes/s.scene.json",
        ] {
            assert!(names.contains(&want), "{want} missing from {names:?}");
        }
        // Not the other assets, their `.meta`, the fonts or anything else.
        for not in ["assets/a.png.meta", "assets/unused.png", "build/font-1.ttf"] {
            assert!(!names.contains(&not), "{not} must not be embedded");
        }
        // The unused asset and the font still need the data folder.
        assert!(c.info.required);
        let j: Value = serde_json::from_slice(
            &c.payload
                .iter()
                .find(|(n, _)| n == "project.json")
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(j["dataDir"], "data");
        assert_eq!(j["dataRequired"], true);

        // Exactly at the limit (the scene is 2 bytes): embedded. One byte over: nothing is.
        std::fs::write(d.join("scenes/s.scene.json"), b"{}").unwrap();
        std::fs::write(
            d.join("assets/a.png"),
            vec![7u8; ESSENTIALS_LIMIT as usize - 2],
        )
        .unwrap();
        let c =
            collect_files(&d, &proj(&p, "f", &Overrides::default()), None, "data", &[]).unwrap();
        assert!(c.payload.iter().any(|(n, _)| n == "assets/a.png"));
        std::fs::write(
            d.join("assets/a.png"),
            vec![7u8; ESSENTIALS_LIMIT as usize - 1],
        )
        .unwrap();
        let c =
            collect_files(&d, &proj(&p, "f", &Overrides::default()), None, "data", &[]).unwrap();
        let names: Vec<&str> = c.payload.iter().map(|(n, _)| n.as_str()).collect();
        assert!(!names.contains(&"assets/a.png") && !names.contains(&"scenes/s.scene.json"));
        assert_eq!(names.len(), 4, "{names:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2601
    #[test]
    fn a_game_whose_assets_are_all_essentials_starts_without_a_data_folder() {
        let d = std::env::temp_dir().join(format!("platypus_pkg_small_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("build")).unwrap();
        std::fs::write(d.join("build/game.pbundle"), r#"{"format":1,"assets":[]}"#).unwrap();
        let p = json!({});
        // No assets at all (hello): nothing to need.
        let c =
            collect_files(&d, &proj(&p, "f", &Overrides::default()), None, "data", &[]).unwrap();
        assert!(c.data.is_empty() && !c.info.required);
        // Tiny named assets only (the `.meta` is not needed): complete.
        std::fs::create_dir_all(d.join("assets")).unwrap();
        std::fs::write(d.join("assets/t.png"), b"T").unwrap();
        std::fs::write(d.join("assets/t.png.meta"), b"M").unwrap();
        std::fs::write(
            d.join("build/game.pbundle"),
            r#"{"format":1,"assets":["assets/t.png"]}"#,
        )
        .unwrap();
        let c =
            collect_files(&d, &proj(&p, "f", &Overrides::default()), None, "data", &[]).unwrap();
        assert!(!c.info.required);
        assert!(c.payload.iter().any(|(n, _)| n == "assets/t.png"));
        // A file the bundle does not name (the whole folder is included): required.
        std::fs::write(d.join("assets/other.bin"), b"O").unwrap();
        let c =
            collect_files(&d, &proj(&p, "f", &Overrides::default()), None, "data", &[]).unwrap();
        assert!(c.info.required);
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2602
    #[test]
    fn the_data_folder_name_must_be_plain() {
        for ok in ["data", "assets-pack", "Game Data"] {
            assert!(valid_data_dir(ok), "{ok}");
        }
        for bad in ["", ".", "..", "a/b", "a\\b", "C:", "..\\x", ".hidden"] {
            assert!(!valid_data_dir(bad), "{bad}");
        }
    }

    // M9-TS-2411
    #[test]
    fn version_numbers_become_four_parts() {
        assert_eq!(pe::version_quad("1.2.3"), [1, 2, 3, 0]);
        assert_eq!(pe::version_quad("2.0.0-beta.1+x"), [2, 0, 0, 0]);
        assert_eq!(pe::version_quad("1.2.3.4"), [1, 2, 3, 4]);
        assert_eq!(pe::version_quad("junk"), [0, 0, 0, 0]);
    }

    fn png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(32, 32, image::Rgba([1, 2, 3, 255]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// The first image of the exe's main icon group.
    fn pe_icon_bytes(pe: &[u8]) -> Vec<u8> {
        let image = editpe::Image::parse(pe).unwrap();
        let res = image.resource_directory().unwrap();
        res.get_main_icon().unwrap().unwrap().to_vec()
    }

    /// A game with no icon and empty title and description, packaged with this test program
    /// as the host; the exe's bytes.
    fn packaged_without_icon(tag: &str) -> Vec<u8> {
        let d = game_folder(tag);
        std::fs::write(d.join("build/game.pbundle"), r#"{"format":1,"assets":[]}"#).unwrap();
        std::fs::write(
            d.join(platypus_core::project::PROJECT_FILE),
            r#"{ "name": "tiny", "title": "", "description": "" }"#,
        )
        .unwrap();
        let opts = PackageOptions {
            host: Some(std::env::current_exe().unwrap()),
            out: Some(d.join("out/tiny.exe")),
            no_build: true,
            ..Default::default()
        };
        let p = package(&d, &opts).unwrap();
        let bytes = std::fs::read(p.path).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        bytes
    }

    // M9-VSP-08
    #[test]
    fn the_video_settings_prompt_argument_goes_into_the_exe_over_the_project() {
        let packaged = |tag: &str, project: &str, arg: Option<bool>| -> Value {
            let d = game_folder(tag);
            std::fs::write(d.join("build/game.pbundle"), r#"{"format":1,"assets":[]}"#).unwrap();
            std::fs::write(d.join(platypus_core::project::PROJECT_FILE), project).unwrap();
            let opts = PackageOptions {
                host: Some(std::env::current_exe().unwrap()),
                out: Some(d.join("out/vsp.exe")),
                no_build: true,
                video_settings_prompt: arg,
                ..Default::default()
            };
            let p = package(&d, &opts).unwrap();
            let mut a = Archive::open(&p.path).unwrap().unwrap();
            let j: Value = serde_json::from_slice(&a.read("project.json").unwrap()).unwrap();
            let _ = std::fs::remove_dir_all(&d);
            j
        };
        let off = r#"{ "name": "vsp", "videoSettingsPrompt": false }"#;
        assert_eq!(
            packaged("vsp_on", off, Some(true))["videoSettingsPrompt"],
            true
        );
        let on = r#"{ "name": "vsp", "videoSettingsPrompt": true }"#;
        assert_eq!(
            packaged("vsp_off", on, Some(false))["videoSettingsPrompt"],
            false
        );
        // Without the argument the project's own value is kept (or none at all).
        assert_eq!(packaged("vsp_keep", on, None)["videoSettingsPrompt"], true);
        assert!(
            packaged("vsp_none", r#"{ "name": "vsp" }"#, None)
                .get("videoSettingsPrompt")
                .is_none()
        );
    }

    // M9-HOST-14
    #[test]
    fn the_renderer_argument_goes_into_the_exe_and_warns_for_a_world_game() {
        let packaged = |tag: &str, project: &str, arg: Option<Renderer>| -> (Value, Packaged) {
            let d = game_folder(tag);
            std::fs::write(
                d.join("build/game.pbundle"),
                r#"{"format":1,"assets":[],"renderer":"world3d","uses":["Camera","Light"]}"#,
            )
            .unwrap();
            std::fs::write(d.join(platypus_core::project::PROJECT_FILE), project).unwrap();
            let opts = PackageOptions {
                host: Some(std::env::current_exe().unwrap()),
                out: Some(d.join("out/ren.exe")),
                no_build: true,
                renderer: arg,
                ..Default::default()
            };
            let p = package(&d, &opts).unwrap();
            let mut a = Archive::open(&p.path).unwrap().unwrap();
            let j: Value = serde_json::from_slice(&a.read("project.json").unwrap()).unwrap();
            assert!(
                p.notices.as_ref().is_some_and(|n| n.is_file()),
                "the notices are beside the exe"
            );
            let _ = std::fs::remove_dir_all(&d);
            (j, p)
        };
        let vulkan = r#"{ "name": "ren", "renderer": "vulkan" }"#;
        let (j, p) = packaged("ren_gles", vulkan, Some(Renderer::Gles));
        assert_eq!(j["renderer"], "gles");
        let w: Vec<&String> = p
            .warnings
            .iter()
            .filter(|w| w.starts_with(crate::RENDERER_WARNING))
            .collect();
        assert_eq!(w.len(), 1, "{:?}", p.warnings);
        assert!(w[0].contains("Camera, Light") && w[0].contains("3D Not Supported"));
        let (j, p) = packaged("ren_cpu", vulkan, Some(Renderer::Cpu));
        assert_eq!(j["renderer"], "cpu");
        assert!(p.warnings.iter().any(|w| w.starts_with("renderer is cpu")));
        // Without the argument the project's own value is kept; `auto` leaves it out.
        let (j, _) = packaged("ren_keep", vulkan, None);
        assert_eq!(j["renderer"], "vulkan");
        let (j, _) = packaged("ren_auto", vulkan, Some(Renderer::Auto));
        assert!(j.get("renderer").is_none());
    }

    // M9-LIC-16
    #[test]
    fn the_notices_are_written_beside_the_exe_unless_no_notices() {
        let run = |tag: &str, no_notices: bool| {
            let d = game_folder(tag);
            std::fs::write(d.join("build/game.pbundle"), r#"{"format":1,"assets":[]}"#).unwrap();
            let opts = PackageOptions {
                host: Some(std::env::current_exe().unwrap()),
                out: Some(d.join("out/game.exe")),
                no_build: true,
                no_notices,
                ..Default::default()
            };
            let p = package(&d, &opts).unwrap();
            let beside = d.join("out").join(NOTICES_FILE);
            let written = std::fs::read_to_string(&beside).ok();
            let _ = std::fs::remove_dir_all(&d);
            (p.notices, beside, written)
        };
        let (notices, beside, written) = run("notices_on", false);
        assert_eq!(notices.as_deref(), Some(beside.as_path()));
        let written = written.expect("THIRD-PARTY-NOTICES.md beside the exe");
        assert_eq!(written, NOTICES.replace("\r\n", "\n"));
        assert!(written.contains("## ANGLE (BSD-3-Clause)") && written.contains("\n## MIT\n"));
        let (notices, _, written) = run("notices_off", true);
        assert!(notices.is_none() && written.is_none());
    }

    // M8-STM-14
    #[test]
    fn a_steam_game_gets_the_steam_library_beside_its_exe_and_others_get_nothing() {
        let name = crate::steam::library_name(Platform::current()).expect("a desktop test host");
        let run = |tag: &str, project: &str, with_library: bool| {
            let d = game_folder(tag);
            std::fs::write(d.join("build/game.pbundle"), r#"{"format":1,"assets":[]}"#).unwrap();
            std::fs::write(d.join("project.json"), project).unwrap();
            if with_library {
                std::fs::create_dir_all(d.join("steam")).unwrap();
                std::fs::write(d.join("steam").join(name), "LIBRARY").unwrap();
            }
            let opts = PackageOptions {
                host: Some(std::env::current_exe().unwrap()),
                out: Some(d.join("out/game.exe")),
                no_build: true,
                ..Default::default()
            };
            let p = package(&d, &opts).unwrap();
            let copied = std::fs::read_to_string(d.join("out").join(name)).ok();
            let _ = std::fs::remove_dir_all(&d);
            (p, copied)
        };
        let steam = r#"{"name":"g","steam":{"appId":4242}}"#;
        let (p, copied) = run("steam_with", steam, true);
        assert_eq!(copied.as_deref(), Some("LIBRARY"));
        assert!(p.steam_library.is_some());
        assert!(!p.warnings.iter().any(|w| w.contains("steam.appId")));

        // No library to copy: packaged all the same, with a warning that says where to put it.
        let (p, copied) = run("steam_without", steam, false);
        assert!(copied.is_none() && p.steam_library.is_none());
        assert!(
            p.warnings
                .iter()
                .any(|w| w.contains("steam.appId") && w.contains(name))
        );

        // A game that doesn't use Steam never gets the library, even when one lies around.
        let (p, copied) = run("steam_off", r#"{"name":"g"}"#, true);
        assert!(copied.is_none() && p.steam_library.is_none());
    }

    // M9-ICO-06
    #[test]
    fn a_game_without_an_icon_or_names_gets_platypus_ones() {
        if !cfg!(windows) {
            return; // the test program is the host; only Windows hosts are PE files
        }
        let bytes = packaged_without_icon("no_icon");
        let r = pe::read_resources(&bytes).unwrap();
        assert_eq!(r.icon_sizes, icon::SIZES.map(|s| (s, s)));
        for key in ["ProductName", "FileDescription"] {
            assert!(
                r.strings.contains(&(key.into(), "Platypus".into())),
                "{key}: {:?}",
                r.strings
            );
        }
        // The 16 px image is the Platypus icon's.
        let ico = icon::default_ico();
        // The .ico's first directory entry: the image's length at 14, its offset at 18.
        let word = |i: usize| u32::from_le_bytes(ico[i..i + 4].try_into().unwrap()) as usize;
        let (len, at) = (word(14), word(18));
        assert_eq!(pe_icon_bytes(&bytes), &ico[at..at + len]);
    }

    // M9-TS-2412
    #[test]
    fn package_writes_one_file_with_payload_resources_and_a_working_sign_hook() {
        let d = game_folder("package");
        std::fs::write(
            d.join("build/game.pbundle"),
            r#"{"format":1,"assets":["assets/a.png"]}"#,
        )
        .unwrap();
        std::fs::write(d.join("package.json"), pkg().to_string()).unwrap();
        std::fs::write(d.join("assets/icon.png"), png_bytes()).unwrap();
        // This test program stands in for the host (a PE file on Windows).
        let host = std::env::current_exe().unwrap();
        let out = d.join("out/my game.exe");
        let sign_marker = d.join("signed.txt");
        let opts = PackageOptions {
            host: Some(host),
            out: Some(out.clone()),
            no_build: true,
            overrides: Overrides {
                version: Some("4.5.6".into()),
                ..Default::default()
            },
            sign_cmd: Some(format!(
                "echo signed > \"{}\" & rem {{exe}}",
                sign_marker.display()
            )),
            ..Default::default()
        };
        let p = package(&d, &opts).unwrap();
        assert_eq!(p.path, out);
        assert!(p.files >= 5);
        let mut a = Archive::open(&out).unwrap().unwrap();
        let j: Value = serde_json::from_slice(&a.read("project.json").unwrap()).unwrap();
        assert_eq!(j["version"], "4.5.6");
        assert_eq!(j["icon"], "platypus-icon.png");
        assert_eq!(a.read("platypus-icon.png").unwrap(), png_bytes());
        assert_eq!(a.read("assets/a.png").unwrap(), b"PNG");
        if cfg!(windows) {
            assert!(sign_marker.exists(), "the sign command did not run");
            let bytes = std::fs::read(&out).unwrap();
            let r = pe::read_resources(&bytes).unwrap();
            assert!(
                r.strings
                    .contains(&("ProductVersion".into(), "4.5.6".into()))
            );
            assert_eq!(r.icon_sizes.len(), icon::SIZES.len());
            // The game's own icon, not Platypus's (a flat colour compresses far smaller).
            let own = pe_icon_bytes(&bytes);
            assert_ne!(own, pe_icon_bytes(&packaged_without_icon("own_vs_default")));
        }
        // A second package on top of a packaged file replaces its payload.
        let again = package(
            &d,
            &PackageOptions {
                host: Some(out.clone()),
                out: Some(d.join("out/b.exe")),
                no_build: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(again.files, p.files);
        // Nothing of `out/data` (the first run's assets) is collected into the second.
        assert!(d.join("out/data/assets/a.png").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2710
    #[test]
    fn templates_resolve_at_package_time_into_the_payload_and_the_version_info() {
        let d = game_folder("templates");
        std::fs::write(d.join("build/game.pbundle"), r#"{"format":1,"assets":[]}"#).unwrap();
        std::fs::write(
            d.join("project.json"),
            r#"{ "name": "fox", "title": "${firstName}'s Game", "company": "${fullName}",
                 "identifier": "com.${username}.${name}", "version": "1.0.0",
                 "copyright": "Copyright (c) ${year} ${company}",
                 "startScene": "scenes/s.scene.json", "layers": { "8": "Detail" },
                 "windows": { "description": "Windows build of ${title}" },
                 "linux": { "description": "Linux build" } }"#,
        )
        .unwrap();
        // Different from the package.json the other tests use: it must not be read.
        std::fs::write(d.join("package.json"), pkg().to_string()).unwrap();
        let out = d.join("out/g.exe");
        let p = package(
            &d,
            &PackageOptions {
                host: Some(std::env::current_exe().unwrap()),
                out: Some(out.clone()),
                no_build: true,
                user: Some(FixedUser {
                    username: "mattb".into(),
                    full_name: "Matthew Brown".into(),
                    date: (2026, 10, 21),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.meta.title, "Matthew's Game");
        assert_eq!(p.meta.exe, "Matthews Game");
        let mut a = Archive::open(&out).unwrap().unwrap();
        let j: Value = serde_json::from_slice(&a.read("project.json").unwrap()).unwrap();
        let npm: Value = serde_json::from_slice(&a.read("package.json").unwrap()).unwrap();
        assert_eq!(
            (npm["name"].as_str(), npm["license"].as_str()),
            (Some("fox"), Some("UNLICENSED"))
        );
        assert_eq!(j["license"], "private");
        assert!(p.license_files.is_empty());
        assert_eq!(j["resolved"], true);
        assert_eq!(j["title"], "Matthew's Game");
        assert_eq!(j["company"], "Matthew Brown");
        assert_eq!(j["identifier"], "com.mattb.fox");
        assert_eq!(j["copyright"], "Copyright (c) 2026 Matthew Brown");
        assert_eq!(j["startScene"], "scenes/s.scene.json");
        assert_eq!(j["layers"]["8"], "Detail");
        assert!(j.get("windows").is_none() && j.get("linux").is_none());
        if cfg!(windows) {
            // The test program is a PE file: the Windows section applies, and the version info matches.
            assert_eq!(j["description"], "Windows build of Matthew's Game");
            let r = pe::read_resources(&std::fs::read(&out).unwrap()).unwrap();
            for (k, v) in [
                ("ProductName", "Matthew's Game"),
                ("CompanyName", "Matthew Brown"),
                ("LegalCopyright", "Copyright (c) 2026 Matthew Brown"),
                ("FileDescription", "Windows build of Matthew's Game"),
            ] {
                assert!(
                    r.strings.contains(&(k.into(), v.into())),
                    "{k}: {:?}",
                    r.strings
                );
            }
        }
        // A reader of the payload (the app) sees the same values without the user's information.
        let dir = d.join("extracted");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("project.json"), a.read("project.json").unwrap()).unwrap();
        let other = FixedUser::default();
        let back = Project::load(&dir, &LoadOptions::new(Platform::Linux, &other)).unwrap();
        assert_eq!(
            (back.title.as_str(), back.identifier.as_str()),
            ("Matthew's Game", "com.mattb.fox")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2711
    #[test]
    fn a_license_ships_beside_the_exe_and_in_the_metadata() {
        let d = game_folder("license");
        std::fs::write(d.join("build/game.pbundle"), r#"{"format":1,"assets":[]}"#).unwrap();
        std::fs::write(d.join("LICENSE"), "MIT text").unwrap();
        std::fs::write(d.join("EULA.txt"), "custom text").unwrap();
        let run = |project: &str, out: &str| {
            std::fs::write(d.join("project.json"), project).unwrap();
            package(
                &d,
                &PackageOptions {
                    host: Some(std::env::current_exe().unwrap()),
                    out: Some(d.join(out)),
                    no_build: true,
                    ..Default::default()
                },
            )
        };
        let p = run(r#"{ "name": "g", "license": "MIT" }"#, "mit/g.exe").unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("mit/LICENSE")).unwrap(),
            "MIT text"
        );
        assert_eq!(p.license_files.len(), 1);
        let mut a = Archive::open(&d.join("mit/g.exe")).unwrap().unwrap();
        let npm: Value = serde_json::from_slice(&a.read("package.json").unwrap()).unwrap();
        assert_eq!(npm["license"], "MIT");
        if cfg!(windows) {
            let r = pe::read_resources(&std::fs::read(d.join("mit/g.exe")).unwrap()).unwrap();
            assert!(
                r.strings
                    .contains(&("Comments".into(), "License: MIT".into())),
                "{:?}",
                r.strings
            );
        }
        // A named file also goes into the data folder.
        let p = run(
            r#"{ "name": "g", "license": "custom", "licenseFile": "EULA.txt" }"#,
            "cu/g.exe",
        )
        .unwrap();
        assert!(d.join("cu/EULA.txt").is_file() && d.join("cu/data/EULA.txt").is_file());
        assert_eq!(p.license_files.len(), 2);
        let mut a = Archive::open(&d.join("cu/g.exe")).unwrap().unwrap();
        let npm: Value = serde_json::from_slice(&a.read("package.json").unwrap()).unwrap();
        assert_eq!(npm["license"], "SEE LICENSE IN EULA.txt");
        // Errors: custom without a file, a missing licenseFile; private copies nothing.
        std::fs::remove_file(d.join("LICENSE")).unwrap();
        assert!(run(r#"{ "name": "g", "license": "custom" }"#, "e1/g.exe").is_err());
        assert!(
            run(
                r#"{ "name": "g", "license": "MIT", "licenseFile": "nope" }"#,
                "e2/g.exe"
            )
            .is_err()
        );
        assert!(
            run(r#"{ "name": "g", "license": "MIT" }"#, "e3/g.exe")
                .unwrap()
                .license_files
                .is_empty()
        );
        run(r#"{ "name": "g" }"#, "pr/g.exe").unwrap();
        assert!(!d.join("pr/LICENSE").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    fn package_with(d: &Path, out: &str, data_dir: Option<&str>, no_data: bool) -> Packaged {
        package(
            d,
            &PackageOptions {
                host: Some(std::env::current_exe().unwrap()),
                out: Some(d.join(out)),
                no_build: true,
                data_dir: data_dir.map(str::to_owned),
                no_data,
                ..Default::default()
            },
        )
        .unwrap()
    }

    // M9-TS-2603
    #[test]
    fn assets_are_copied_to_a_data_folder_beside_the_exe_and_not_embedded() {
        let d = game_folder("data");
        std::fs::write(
            d.join("build/game.pbundle"),
            r#"{"format":1,"assets":["assets/a.png"]}"#,
        )
        .unwrap();
        std::fs::write(d.join("package.json"), pkg().to_string()).unwrap();
        std::fs::write(d.join("assets/icon.png"), png_bytes()).unwrap();
        // A big asset: not embedded, copied byte for byte.
        let big = vec![9u8; 300 * 1024];
        std::fs::write(d.join("assets/big.bin"), &big).unwrap();
        std::fs::write(
            d.join("build/game.pbundle"),
            r#"{"format":1,"assets":["assets/a.png","assets/big.bin"]}"#,
        )
        .unwrap();

        let p = package_with(&d, "out/g.exe", None, false);
        let data = d.join("out/data");
        assert_eq!(p.data_path.as_deref(), Some(data.as_path()));
        assert_eq!(std::fs::read(data.join("assets/big.bin")).unwrap(), big);
        for rel in [
            "assets/a.png",
            "assets/a.png.meta",
            "assets/unused.png",
            "build/font-1.ttf",
        ] {
            assert!(data.join(rel).is_file(), "{rel} not in data/");
        }
        assert!(!data.join("build/game.pbundle").exists());
        assert!(p.data_files >= 6);
        assert!(p.data_bytes >= big.len() as u64);
        // The exe's table of contents has the code and no asset beyond the tiny essentials.
        let a = Archive::open(&p.path).unwrap().unwrap();
        let names: Vec<&str> = a.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(names.contains(&"build/game.pbundle") && names.contains(&"project.json"));
        for not in [
            "assets/big.bin",
            "assets/unused.png",
            "assets/a.png.meta",
            "build/font-1.ttf",
        ] {
            assert!(!names.contains(&not), "{not} embedded in {names:?}");
        }
        assert!(p.info.required);

        // --data-dir renames the folder (and the exe is told).
        let p = package_with(&d, "out2/g.exe", Some("game-data"), false);
        assert!(d.join("out2/game-data/assets/big.bin").is_file());
        assert!(!d.join("out2/data").exists());
        let mut a = Archive::open(&p.path).unwrap().unwrap();
        let j: Value = serde_json::from_slice(&a.read("project.json").unwrap()).unwrap();
        assert_eq!(j["dataDir"], "game-data");

        // --no-data writes no folder, but the exe still looks for it.
        let p = package_with(&d, "out3/g.exe", None, true);
        assert!(!d.join("out3/data").exists());
        assert_eq!(p.data_path, None);
        assert!(p.info.required);
        let err = package(
            &d,
            &PackageOptions {
                host: Some(std::env::current_exe().unwrap()),
                out: Some(d.join("out4/g.exe")),
                no_build: true,
                data_dir: Some("../x".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("--data-dir"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2723
    #[test]
    fn package_warns_about_empty_and_default_metadata_and_still_packages() {
        let d = game_folder("warns");
        std::fs::write(d.join("build/game.pbundle"), r#"{"format":1,"assets":[]}"#).unwrap();
        std::fs::write(
            d.join("project.json"),
            r#"{ "name": "g", "company": "", "description": "" }"#,
        )
        .unwrap();
        let p = package_with(&d, "out/g.exe", None, false);
        assert!(p.path.is_file());
        let has = |s: &str| p.warnings.iter().any(|w| w.contains(s));
        assert!(
            has("project.json: \"company\" is empty"),
            "{:?}",
            p.warnings
        );
        assert!(
            has("project.json: \"description\" is empty"),
            "{:?}",
            p.warnings
        );
        // company is empty on purpose, so not "still the default"; copyright and license are.
        assert!(!has("\"company\" is not set"), "{:?}", p.warnings);
        assert!(has("\"copyright\" is not set"), "{:?}", p.warnings);
        assert!(has("\"license\" is not set"), "{:?}", p.warnings);
        // Everything given: nothing to say.
        std::fs::write(
            d.join("project.json"),
            r#"{ "name": "g", "company": "Acme", "copyright": "(c) Acme", "license": "MIT" }"#,
        )
        .unwrap();
        let p = package_with(&d, "out/g2.exe", None, false);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2440: needs the engine host and a GPU, so it runs on request:
    // `cargo test -p platypus_build -- --ignored standalone` (PLATYPUS_HOST or a built build/debug/platypus-app).
    #[test]
    #[ignore = "needs platypus_app built and a GPU"]
    fn standalone_exe_runs_from_another_folder_without_the_game_folder() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
        let d = std::env::temp_dir().join(format!("platypus_pkg_run_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let game = d.join("game");
        for f in ["project.json", "tsconfig.json", "src/main.ts"] {
            let to = game.join(f);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(src.join(f), to).unwrap();
        }
        let opts = PackageOptions {
            no_tsc: true,
            out: Some(d.join("bin/hello.exe")),
            ..Default::default()
        };
        let p = package(&game, &opts).unwrap();
        // The game folder is gone; the program runs from yet another folder.
        std::fs::remove_dir_all(&game).unwrap();
        let cwd = d.join("elsewhere");
        std::fs::create_dir_all(&cwd).unwrap();
        let shot = cwd.join("out.png");
        let status = std::process::Command::new(&p.path)
            .args(["--screenshot", "out.png", "--frames", "5"])
            .current_dir(&cwd)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(std::fs::metadata(&shot).unwrap().len() > 100);
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2610: `cargo test -p platypus_build -- --ignored standalone_data`. A game with an
    // asset bigger than the essentials limit runs from `data/` beside the exe, with the game
    // folder gone, and says where it looked when `data/` is gone too.
    #[test]
    #[ignore = "needs platypus_app built and a GPU"]
    fn standalone_data_folder_is_read_and_its_absence_is_a_clear_error() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/boxes");
        let d = std::env::temp_dir().join(format!("platypus_pkg_data_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let game = d.join("game");
        for f in [
            "project.json",
            "tsconfig.json",
            "src/main.tsx",
            "assets/stripes.png",
        ] {
            let (from, to) = (src.join(f), game.join(f));
            if from.is_file() {
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::copy(from, to).unwrap();
            }
        }
        // Pushes the game past the 256 KiB essentials limit.
        std::fs::write(game.join("assets/padding.bin"), vec![0u8; 300 * 1024]).unwrap();
        let opts = PackageOptions {
            no_tsc: true,
            out: Some(d.join("bin/boxes.exe")),
            ..Default::default()
        };
        let p = package(&game, &opts).unwrap();
        assert!(!p.embedded.iter().any(|n| n.starts_with("assets/")));
        assert!(d.join("bin/data/assets/stripes.png").is_file());
        std::fs::remove_dir_all(&game).unwrap();
        let cwd = d.join("elsewhere");
        std::fs::create_dir_all(&cwd).unwrap();
        let run = |name: &str| {
            std::process::Command::new(&p.path)
                .args(["--screenshot", name, "--frames", "5"])
                .current_dir(&cwd)
                .output()
                .unwrap()
        };
        let ok = run("out.png");
        assert!(
            ok.status.success(),
            "{}",
            String::from_utf8_lossy(&ok.stderr)
        );
        assert!(std::fs::metadata(cwd.join("out.png")).unwrap().len() > 100);
        std::fs::remove_dir_all(d.join("bin/data")).unwrap();
        let bad = run("none.png");
        assert!(!bad.status.success());
        let err = String::from_utf8_lossy(&bad.stderr);
        assert!(err.contains("data folder was not found"), "{err}");
        assert!(!cwd.join("none.png").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2559
    #[test]
    fn the_newest_host_wins_not_the_release_one() {
        let d = std::env::temp_dir().join(format!("platypus_build_host_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (release, debug, gone) = (
            d.join("release.exe"),
            d.join("debug.exe"),
            d.join("gone.exe"),
        );
        std::fs::write(&release, b"old").unwrap();
        std::fs::write(&debug, b"new").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&release)
            .unwrap()
            .set_modified(old)
            .unwrap();
        // The stale release build is listed first, as `find_host` lists it.
        let list = vec![release.clone(), debug.clone(), gone];
        assert_eq!(newest_host(list), Some(debug.clone()));
        assert_eq!(newest_host(vec![d.join("none.exe")]), None);
        // Equal times: the first candidate.
        let t = std::fs::metadata(&debug).unwrap().modified().unwrap();
        std::fs::File::options()
            .write(true)
            .open(&release)
            .unwrap()
            .set_modified(t)
            .unwrap();
        assert_eq!(newest_host(vec![release.clone(), debug]), Some(release));
        let _ = std::fs::remove_dir_all(&d);
    }
    // M9-TS-2600
    #[test]
    fn there_is_one_host_and_a_missing_explicit_one_is_an_error() {
        let d = std::env::temp_dir().join(format!("platypus_build_type_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // An explicit host that is missing is an error (`$PLATYPUS_HOST` is not consulted then).
        let missing = d.join("none");
        let e = find_host(&missing, Some(&missing)).unwrap_err();
        assert!(e.contains("not found"), "{e}");
        assert_eq!(HOST, "platypus-app");
        let _ = std::fs::remove_dir_all(&d);
    }
}
