// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `platypus-build`: builds a game written in TypeScript into `build/` for the engine host (`platypus-app`), with no bun and no
//! `node_modules` (see `docs/platypus-build.md`).
//!
//! A build reads `project.json` (deprecated fallback: the `platypus` section of `package.json`) and `tsconfig.json`, resolves the module graph from the
//! entry file (`platypus_ts::modules`), runs `tsc --noEmit -p .` when `tsc` can be found, compiles the
//! bundle with all engine manifests (so errors are found now, not when the game starts), and writes
//! `build/game.pbundle` (the resolved source, the CSS and asset lists, and what the program needs
//! from the renderer: `platypus_ts::renderer`), `build/game.css`, and the assets the program names.

pub mod assets;
pub mod bench;
pub mod css;
pub mod dev;
pub mod icon;
pub mod include;
pub mod init;
pub mod package;
pub mod packs;
pub mod pe;
pub mod tsconfig;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What `project.json` (or `package.json`) and `tsconfig.json` say about a game.
#[derive(Debug, Clone)]
pub struct GameConfig {
    pub root: PathBuf,
    pub name: String,
    pub title: String,
    pub company: String,
    pub identifier: String,
    /// The entry file, relative to the game folder.
    pub entry: String,
    /// `tsconfig.json` `include` (default `["src"]`).
    pub include: Vec<String>,
    pub jsx: Option<String>,
    pub has_tsconfig: bool,
    /// `project.json` `include` / `exclude` (files, folders, globs) and `startScene`.
    pub pack_include: Option<Vec<String>>,
    pub pack_exclude: Option<Vec<String>>,
    pub start_scene: Option<String>,
    /// `project.json` `prebuild`: a command run in the game folder before compiling.
    pub prebuild: Option<String>,
    /// One warning per metadata field `project.json` leaves empty on purpose (`build` and
    /// `package` print them, `check` does not).
    pub empty_field_warnings: Vec<String>,
    /// `project.json` `renderer` (`Auto` when left out).
    pub renderer: platypus_core::project::Renderer,
    /// Warnings about `project.json` fields that are no longer used or have an unknown value.
    pub project_warnings: Vec<String>,
}

/// Runs `project.json`'s `prebuild` command in the game folder (through `cmd /C` on Windows,
/// `sh -c` elsewhere); a failure stops the build with the command's output.
pub fn run_prebuild(root: &Path, cmd: &str) -> Result<(), String> {
    let mut c;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c = std::process::Command::new("cmd");
        c.args(["/S", "/C"]).raw_arg(format!("\"{cmd}\""));
    }
    #[cfg(not(windows))]
    {
        c = std::process::Command::new("sh");
        c.args(["-c", cmd]);
    }
    let out = c
        .current_dir(root)
        .output()
        .map_err(|e| format!("prebuild `{cmd}`: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "prebuild `{cmd}` failed ({}):\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// A folder is a game when it has a `project.json`. The deprecated fallback: a `package.json`
/// with a `platypus` section.
pub fn is_game(dir: &Path) -> bool {
    dir.join(platypus_core::project::PROJECT_FILE).is_file()
        || std::fs::read_to_string(dir.join("package.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .is_some_and(|v| platypus_core::project::package_section(&v).is_some())
}

/// The games directly inside `dir` (see [`is_game`]), sorted by path.
pub fn find_games(dir: &Path) -> Vec<PathBuf> {
    let mut games: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| is_game(p))
                .collect()
        })
        .unwrap_or_default();
    games.sort();
    games
}

impl GameConfig {
    pub fn load(root: &Path) -> Result<GameConfig, String> {
        let user = platypus_core::project::SystemUser;
        let project = platypus_core::project::Project::load(
            root,
            &platypus_core::project::LoadOptions::new(
                platypus_core::project::Platform::current(),
                &user,
            ),
        )
        .map_err(|e| e.to_string())?;
        let name = project.name.clone();
        let empty_field_warnings = project.empty_field_warnings();
        let ts_path = root.join("tsconfig.json");
        let ts: serde_json::Value = std::fs::read_to_string(&ts_path)
            .ok()
            .and_then(|t| serde_json::from_str(&strip_json_comments(&t)).ok())
            .unwrap_or(serde_json::Value::Null);
        let include = ts["include"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["src".to_string()]);
        let entry = match project.main.clone() {
            Some(m) => m,
            None => [
                "src/main.tsx",
                "src/main.ts",
                "src/index.tsx",
                "src/index.ts",
                "main.tsx",
                "main.ts",
            ]
            .iter()
            .find(|c| root.join(c).exists())
            .map(|c| c.to_string())
            .ok_or_else(|| {
                format!(
                    "{}: no entry file (src/main.tsx, src/main.ts, ...)",
                    root.display()
                )
            })?,
        };
        Ok(GameConfig {
            root: root.to_path_buf(),
            title: project.title,
            company: project.company,
            identifier: project.identifier,
            name,
            entry,
            include,
            jsx: ts["compilerOptions"]["jsx"].as_str().map(String::from),
            has_tsconfig: ts_path.exists(),
            pack_include: project.include.clone(),
            pack_exclude: project.exclude.clone(),
            start_scene: project.start_scene.clone(),
            prebuild: project.prebuild.clone(),
            empty_field_warnings,
            renderer: project.renderer,
            project_warnings: project.warnings.clone(),
        })
    }
}

/// Removes `//` and `/* */` comments (outside strings) and trailing commas, so JSONC parses.
pub fn strip_json_comments(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else {
            out.push(c);
        }
        i += 1;
    }
    // Trailing commas before } or ].
    let chars: Vec<char> = out.chars().collect();
    let mut res = String::new();
    let mut in_str = false;
    for (i, &c) in chars.iter().enumerate() {
        if c == '"' && (i == 0 || chars[i - 1] != '\\') {
            in_str = !in_str;
        }
        if !in_str && c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        res.push(c);
    }
    res
}

/// Every engine manifest the compiler checks calls against, plus the `Ui` namespace: the same lists
/// as the host's `native::manifests()`.
pub fn manifests() -> Vec<String> {
    let mut all = platypus_engine_api::manifests();
    all.push(platypus_scene::api::scene_api::MANIFEST.to_json());
    all.extend(platypus_scenes::manifests());
    all.extend(platypus_physics_api::manifests());
    all.extend(platypus_render_api::manifests());
    all.push(platypus_ts_ui::manifest_json());
    all
}

/// The text of `types/platypus.d.ts` (the shipped ambient and TypeScript-side declarations included).
pub fn types_dts() -> Result<String, String> {
    let ms: Vec<String> = manifests()
        .into_iter()
        .filter(|m| !m.contains("\"namespace\":\"Ui\""))
        .collect();
    platypus_ts::types_dts::generate(&ms)
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Skip the `tsc` check.
    pub no_tsc: bool,
    /// Also write the AOT object `build/game.o` (release builds).
    pub aot: bool,
    /// Run `platypus-app <game> --cook` afterwards (needs the `platypus-app` binary; see `find_platypus`).
    pub cook: bool,
    /// Skip compiling the bundle (module graph, CSS and assets only).
    pub no_compile: bool,
    /// Skip `project.json`'s `prebuild` command (its output is already there).
    pub no_prebuild: bool,
    /// Also lower the functions nothing calls (they are skipped otherwise), so their errors show.
    pub check_unused: bool,
    /// Build with the UI inspector (`--add-ui-inspector`): `game.pbundle` says `"uiInspector": true`.
    /// Off, the bundle has no trace of it and the host takes the fast path
    /// (`docs/research/platypus-ui-inspector.md`).
    pub ui_inspector: bool,
}

/// The UI inspector's overlay (TSX, `inspector/overlay.tsx`): appended to the program, which
/// mounts it in its own root (`createRoot(9000001)`, the host's inspector target).
const INSPECTOR_OVERLAY: &str = include_str!("../inspector/overlay.tsx");
/// What the overlay and the editor's "UI" panel share (`inspector/shared.tsx`). A program that
/// imports it itself (the editor does) already has it, so it is not appended twice.
const INSPECTOR_SHARED: &str = include_str!("../inspector/shared.tsx");
/// The value logic the shared parts use (`inspector/values.ts`).
const INSPECTOR_VALUES: &str = include_str!("../inspector/values.ts");
/// The inspector's stylesheet: added to `game.css` by the build, not to the program.
const INSPECTOR_CSS: &str = include_str!("../inspector/inspector.css");
const INSPECTOR_SHARED_MARKER: &str = "function UiInspField(";

/// `text` as part of the program: no `import` statements (the parts are appended in order) and no
/// `export`s (the program's own exports stay its exports).
fn inspector_part(text: &str) -> String {
    let mut out = text.to_owned();
    while let Some(at) = out.find("import ") {
        // Only a statement that starts a line.
        if at > 0 && !out[..at].ends_with('\n') {
            break;
        }
        let Some(end) = out[at..].find("\";") else {
            break;
        };
        out.replace_range(at..at + end + 2, "");
    }
    out.replace("\nexport ", "\n")
}

/// Adds the inspector to a bundled program (`--add-ui-inspector`).
fn add_ui_inspector(bundle: &mut platypus_ts::modules::Bundle) {
    let mut parts = vec![("<ui-inspector>/overlay.tsx", INSPECTOR_OVERLAY)];
    // A program that imports the shared parts (the editor does) has them, and the logic below them.
    if !bundle.source.contains(INSPECTOR_SHARED_MARKER) {
        parts.insert(0, ("<ui-inspector>/shared.tsx", INSPECTOR_SHARED));
        parts.insert(0, ("<ui-inspector>/values.ts", INSPECTOR_VALUES));
    }
    for (name, text) in parts {
        if !bundle.source.ends_with('\n') {
            bundle.source.push('\n');
        }
        // Lines of the appended text are told apart from the game's in error messages.
        let start = bundle.source.matches('\n').count();
        bundle.source.push_str(&inspector_part(text));
        bundle.starts.push((start, name.to_string()));
    }
}

/// How `tsc` went.
#[derive(Debug, Clone, PartialEq)]
pub enum Tsc {
    Skipped,
    Missing,
    Ok,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Report {
    pub config: GameConfig,
    pub files: Vec<String>,
    pub css_files: Vec<String>,
    pub assets: Vec<String>,
    /// The dynamic path families the code builds and what each reaches.
    pub patterns: Vec<include::PatternMatch>,
    pub tsc: Tsc,
    pub warnings: Vec<String>,
    pub object_bytes: usize,
    /// What the program needs from the renderer (`None` when lowering failed under `no_compile`:
    /// the bundle then says nothing, which the host takes as the 3D world).
    pub usage: Option<platypus_ts::renderer::Usage>,
    pub time: Duration,
}

/// How the warning about a UI-only renderer for a world game starts.
pub const RENDERER_WARNING: &str = "renderer is ";

/// The warning for `renderer` (`gles`, `cpu`) chosen for a program that uses the world.
pub fn renderer_warning(
    renderer: platypus_core::project::Renderer,
    usage: &Option<platypus_ts::renderer::Usage>,
) -> Option<String> {
    let u = usage.as_ref()?;
    if !renderer.ui_only() {
        return None;
    }
    platypus_ts::renderer::world_warning(renderer.as_str(), u.tier, &u.uses)
}

/// What `build/game.pbundle` of `game` says the program needs (`None` without one, or for a
/// bundle without the field).
pub fn read_usage(game: &Path) -> Option<platypus_ts::renderer::Usage> {
    let text = std::fs::read_to_string(game.join("build/game.pbundle")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(platypus_ts::renderer::Usage {
        tier: platypus_ts::renderer::Tier::parse(v["renderer"].as_str()?)?,
        uses: v["uses"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// What the program needs from the renderer: the engine API its compiled code calls (`externs`)
/// or uses otherwise (`uses`: components), and the scene and prefab files a packaged build would
/// hold (`plan`): objects, and their component types.
fn usage_of(
    externs: &[String],
    uses: &[String],
    manifests: &[String],
    root: &Path,
    plan: &[String],
) -> platypus_ts::renderer::Usage {
    let mut usage = platypus_ts::renderer::usage_of_symbols(externs, uses, manifests);
    for f in plan
        .iter()
        .filter(|f| f.ends_with(".scene.json") || f.ends_with(".prefab.json"))
    {
        if let Ok(text) = std::fs::read_to_string(root.join(f)) {
            let types = platypus_ts::renderer::scene_file_names(&text);
            usage.add(types.iter().map(String::as_str));
        }
    }
    usage
}

fn find_tsc(root: &Path) -> Option<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    let mut p = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    while p.pop() {
        dirs.push(p.clone());
    }
    // The checkout this tool was built from (a game outside it still finds its TypeScript).
    let mut own = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while own.pop() {
        dirs.push(own.clone());
    }
    let names: &[&str] = if cfg!(windows) {
        &["tsc.cmd", "tsc.exe"]
    } else {
        &["tsc"]
    };
    for d in dirs {
        for n in names {
            let c = d.join("node_modules/.bin").join(n);
            if c.exists() {
                return Some(c);
            }
        }
    }
    for n in names {
        if let Ok(o) = std::process::Command::new(n).arg("--version").output()
            && o.status.success()
        {
            return Some(PathBuf::from(n));
        }
    }
    None
}

/// The `tsconfig.json` text `tsc` is run with: the game's own file extended, its `include` kept (as
/// paths from `build/.tsc/`), plus the generated declarations (`platypus.d.ts` next to it). Every file is a
/// module (`moduleDetection: force`, so top-level names of a game never merge with the engine's
/// globals), and JSX is only checked (the game needs no `react/jsx-runtime`).
pub fn tsc_config(config: &GameConfig) -> String {
    let include: Vec<String> = config
        .include
        .iter()
        .map(|i| format!("../../{}", i.trim_start_matches("./")))
        .collect();

    // Bare imports are files of `modules/` next to the entry (the compiler's rule, `platypus_ts::modules`).
    let entry_dir = Path::new(&config.entry)
        .parent()
        .map(|d| d.to_string_lossy().replace("\\", "/"))
        .unwrap_or_default();
    let modules = if entry_dir.is_empty() {
        "../../modules/*".to_string()
    } else {
        format!("../../{entry_dir}/modules/*")
    };
    let mut v = serde_json::json!({
        "extends": "../../tsconfig.json",
        "compilerOptions": {
            "paths": { "*": [modules] },
            "noEmit": true,
            "types": [],
            "jsx": "preserve",
            "moduleDetection": "force",
        },
        "include": include,
        "files": ["platypus.d.ts"],
        "exclude": ["../../node_modules"],
    });
    // A game without a `tsconfig.json` is checked with the default one (`tsconfig.rs`).
    if !config.has_tsconfig {
        let mut options = tsconfig::compiler_options();
        if let (Some(o), Some(over)) = (
            options.as_object_mut(),
            v["compilerOptions"].as_object().cloned(),
        ) {
            o.extend(over);
        }
        v["compilerOptions"] = options;
        if let Some(o) = v.as_object_mut() {
            o.remove("extends");
        }
    }
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Writes `build/.tsc/{tsconfig.json,platypus.d.ts}` and returns the config path.
fn write_tsc_files(root: &Path, config: &GameConfig) -> Result<PathBuf, String> {
    let dir = root.join("build/.tsc");
    let dts = types_dts()?;
    write_atomic(&dir.join("platypus.d.ts"), dts.as_bytes()).map_err(|e| e.to_string())?;
    let cfg = dir.join("tsconfig.json");
    write_atomic(&cfg, tsc_config(config).as_bytes()).map_err(|e| e.to_string())?;
    Ok(cfg)
}

fn run_tsc(root: &Path, config: &GameConfig) -> Tsc {
    let Some(tsc) = find_tsc(root) else {
        return Tsc::Missing;
    };
    let cfg = match write_tsc_files(root, config).map(|_| "build/.tsc/tsconfig.json") {
        Ok(c) => c,
        Err(e) => return Tsc::Failed(e),
    };
    match std::process::Command::new(&tsc)
        .arg("--noEmit")
        .arg("-p")
        .arg(cfg)
        .current_dir(root)
        .output()
    {
        Ok(o) if o.status.success() => Tsc::Ok,
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            Tsc::Failed(text.trim().to_string())
        }
        Err(e) => Tsc::Failed(format!("cannot run {}: {e}", tsc.display())),
    }
}

/// String literals of the bundled source that name an existing file under the game folder.
fn referenced_assets(source: &str, root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let b = source.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let q = b[i];
        if q == b'"' || q == b'\'' || q == b'`' {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && b[j] != q && b[j] != b'\n' {
                if b[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            if j < b.len() && b[j] == q {
                let lit = &source[start..j.min(source.len())];
                let ext_ok = lit.rsplit_once('.').is_some_and(|(_, e)| {
                    assets::kind(&e.to_lowercase()).is_some() || lit.ends_with(".scene.json")
                });
                if ext_ok
                    && !lit.contains(['{', '$'])
                    && let Some(p) = platypus_core::resolve_asset(root, lit)
                    && p.is_file()
                {
                    let rel = lit.trim_start_matches("./").to_string();
                    if !found.contains(&rel) {
                        found.push(rel);
                    }
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    found.sort();
    found
}

/// Writes `path` by a temporary file and a rename, so a watcher never reads half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Builds the game in `root` into `root/build`.
pub fn build(root: &Path, opts: &Options) -> Result<Report, String> {
    let t0 = Instant::now();
    let config = GameConfig::load(root)?;
    if let Some(cmd) = config.prebuild.as_ref().filter(|_| !opts.no_prebuild) {
        run_prebuild(root, cmd).map_err(|e| format!("{}: {e}", config.name))?;
    }
    let out_dir = root.join("build");
    let entry = root.join(&config.entry);
    // The engine's global names stay the engine's: a module's own `class Random` is renamed.
    let reserved = platypus_ts::modules::global_names(&types_dts()?);
    let mut bundle = platypus_ts::modules::bundle_reserving(&entry, &reserved)?;
    if opts.ui_inspector {
        add_ui_inspector(&mut bundle);
    }
    let mut warnings = config.empty_field_warnings.clone();
    warnings.extend(config.project_warnings.iter().cloned());

    let tsc = if opts.no_tsc {
        Tsc::Skipped
    } else {
        run_tsc(root, &config)
    };
    if let Tsc::Failed(text) = &tsc {
        return Err(format!("tsc failed:\n{text}"));
    }

    if opts.check_unused {
        platypus_ts::check_unused(&bundle.source, &manifests())
            .map_err(|e| format!("{}: {}", config.name, bundle.locate_in(&e)))?;
    }
    let mut object_bytes = 0;
    let externs = if !opts.no_compile {
        let out =
            platypus_ts::compile_with_manifests(&bundle.source, "speed", "game_", 0, &manifests())
                .map_err(|e| format!("{}: {}", config.name, bundle.locate_in(&e)))?;
        object_bytes = out.object.len();
        if opts.aot {
            write_atomic(&out_dir.join("game.o"), &out.object).map_err(|e| e.to_string())?;
            write_atomic(&out_dir.join("game_bindings.rs"), out.bindings.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        Some((out.externs, out.uses))
    } else {
        // Not compiled, but the bundle still says what the program needs: lowering alone finds
        // it (a program lowering can't handle leaves it unknown, which the host takes as 3D).
        platypus_ts::extern_symbols(&bundle.source, &manifests()).ok()
    };

    // Stylesheets, in import order; the inspector's (`inspector/inspector.css`) comes with the flag.
    let mut css_paths: Vec<PathBuf> = bundle.css.iter().map(PathBuf::from).collect();
    let inspector_css = out_dir.join("ui-inspector.css");
    if opts.ui_inspector {
        write_atomic(&inspector_css, INSPECTOR_CSS.as_bytes()).map_err(|e| e.to_string())?;
        css_paths.push(inspector_css);
    } else {
        let _ = std::fs::remove_file(&inspector_css);
    }
    let mut css_name = None;
    let mut copied = Vec::new();
    if !css_paths.is_empty() {
        let out = css::bundle(&css_paths, &out_dir).map_err(|e| e.to_string())?;
        warnings.extend(out.warnings);
        copied = out.copied;
        write_atomic(&out_dir.join("game.css"), out.css.as_bytes()).map_err(|e| e.to_string())?;
        css_name = Some("game.css".to_string());
    } else {
        let _ = std::fs::remove_file(out_dir.join("game.css"));
    }

    // Assets: .meta files, then what the program names, copied into build/ with the same relative path.
    assets::update(root, false).map_err(|e| e.to_string())?;
    let names = referenced_assets(&bundle.source, root);
    for a in &names {
        for src_rel in [a.clone(), format!("{a}.meta")] {
            let src = root.join(&src_rel);
            if src.is_file() {
                let to = out_dir.join(&src_rel);
                if let Some(d) = to.parent() {
                    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                std::fs::copy(&src, &to).map_err(|e| format!("{}: {e}", src.display()))?;
            }
        }
    }

    // Paths built at run time (`assets/sfx/${n}.wav` is `assets/sfx/*.*`): which files they reach.
    let all_files = include::game_files(root);
    let found = platypus_ts::assetpaths::asset_paths(&bundle.source);
    let at = |line: usize| {
        bundle
            .locate(line)
            .map_or_else(|| format!("line {line}"), |(f, l)| format!("{f}:{l}"))
    };
    let mut patterns = Vec::new();
    for (glob, line) in &found.patterns {
        if !include::asset_looking_glob(glob) {
            continue;
        }
        let matches = include::match_pattern(&all_files, glob);
        if matches.is_empty() {
            warnings.push(format!(
                "{}: the dynamic path {glob:?} matches no asset file",
                at(*line)
            ));
        }
        patterns.push(include::PatternMatch {
            glob: glob.clone(),
            at: at(*line),
            matches,
        });
    }
    for (lit, line) in &found.literals {
        if include::asset_looking_literal(lit)
            && platypus_core::resolve_asset(root, lit).is_some_and(|p| !p.is_file())
        {
            warnings.push(format!("{}: {lit:?} names no file", at(*line)));
        }
    }
    let selected = include::select(
        &all_files,
        config.pack_include.as_deref(),
        config.pack_exclude.as_deref(),
    );
    for u in &selected.unmatched {
        warnings.push(format!("project.json include {u:?} matches no file"));
    }
    // What a packaged build would hold (the dev app warns about a loaded file outside it).
    let excluded = |f: &str| {
        config
            .pack_exclude
            .iter()
            .flatten()
            .any(|x| platypus_core::glob::include_match(x, f))
    };
    let mut plan: Vec<String> = selected.files;
    plan.extend(names.iter().cloned());
    plan.extend(config.start_scene.iter().cloned());
    for p in &patterns {
        plan.extend(p.matches.iter().filter(|f| !excluded(f)).cloned());
    }
    plan.extend(copied.iter().map(|c| format!("build/{c}")));
    plan.sort();
    plan.dedup();
    let usage = externs.map(|(e, u)| usage_of(&e, &u, &manifests(), root, &plan));
    if let Some(w) = renderer_warning(config.renderer, &usage) {
        warnings.push(w);
    }
    write_atomic(
        &out_dir.join("assets.plan"),
        plan.join(
            "
",
        )
        .as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    let mut pbundle = serde_json::json!({
        "format": 1,
        "title": config.title,
        "company": config.company,
        "identifier": config.identifier,
        "entry": config.entry,
        "source": bundle.source,
        "files": bundle.files,
        "css": css_name,
        "assets": names,
        "patterns": patterns.iter().map(|p| serde_json::json!({
            "glob": p.glob, "at": p.at, "matches": p.matches,
        })).collect::<Vec<_>>(),
    });
    // What the host chooses its renderer by (`platypus_ts::renderer`); left out when it is not
    // known, which the host takes as the 3D world.
    if let Some(u) = &usage {
        pbundle["renderer"] = u.tier.as_str().into();
        pbundle["uses"] = u.uses.clone().into();
    }
    if opts.ui_inspector {
        pbundle["uiInspector"] = true.into();
        warnings.push(
            "built with --add-ui-inspector: for development, leave it out of a release".into(),
        );
    }
    let text = serde_json::to_string_pretty(&pbundle).map_err(|e| e.to_string())?;
    write_atomic(&out_dir.join("game.pbundle"), text.as_bytes()).map_err(|e| e.to_string())?;

    if opts.cook {
        match find_platypus(root) {
            Some(platypus) => {
                let st = std::process::Command::new(&platypus)
                    .arg(root)
                    .arg("--cook")
                    .status();
                if !matches!(st, Ok(s) if s.success()) {
                    warnings.push(format!("{} --cook failed", platypus.display()));
                }
            }
            None => warnings
                .push("--cook: the `platypus-app` binary was not found (cargo build --release -p platypus_app)".into()),
        }
    }
    Ok(Report {
        config,
        files: bundle.files,
        css_files: bundle.css,
        assets: names,
        patterns,
        tsc,
        warnings,
        object_bytes,
        usage,
        time: t0.elapsed(),
    })
}

/// The `platypus-app` binary, for `--cook`: cargo's release or debug build in a folder above the game
/// (`package::built_host`).
pub fn find_platypus(root: &Path) -> Option<PathBuf> {
    let mut p = root.canonicalize().ok()?;
    while p.pop() {
        for profile in ["release", "debug"] {
            let c = package::built_host(&p, profile);
            if c.exists() {
                return Some(c);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("platypus_build_{name}_{}", assets::new_guid()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // M9-TS-1850
    #[test]
    fn jsonc_comments_and_trailing_commas_are_stripped() {
        let t = strip_json_comments("{ // c\n \"a\": \"x//y\", /* b */ \"b\": [1, 2,], }");
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["a"], "x//y");
        assert_eq!(v["b"][1], 2);
    }

    // M9-TS-1851
    #[test]
    fn config_reads_the_platypus_section_and_tsconfig() {
        let d = temp("cfg");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/main.ts"), "export const a = 1;").unwrap();
        std::fs::write(
            d.join("package.json"),
            r#"{"name":"g","platypus":{"title":"T","company":"C","identifier":"dev.x.g"}}"#,
        )
        .unwrap();
        std::fs::write(d.join("tsconfig.json"), "{\n // x\n \"compilerOptions\": {\"jsx\": \"react-jsx\"}, \"include\": [\"src\", \"types\"] }").unwrap();
        let c = GameConfig::load(&d).unwrap();
        assert_eq!(
            (c.title.as_str(), c.identifier.as_str(), c.entry.as_str()),
            ("T", "dev.x.g", "src/main.ts")
        );
        assert_eq!(c.include, ["src", "types"]);
        assert_eq!(c.jsx.as_deref(), Some("react-jsx"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2720
    #[test]
    fn config_reads_project_json_before_the_platypus_section() {
        let d = temp("projectjson");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/app.ts"), "").unwrap();
        std::fs::write(
            d.join("package.json"),
            r#"{"name":"g","platypus":{"title":"FROM PACKAGE","main":"src/other.ts"}}"#,
        )
        .unwrap();
        std::fs::write(
            d.join("project.json"),
            r#"{"name":"g","title":"T ${env:PLATYPUS_NO_SUCH_VAR_2720:-ok}","identifier":"dev.x.g","main":"src/app.ts"}"#,
        )
        .unwrap();
        let c = GameConfig::load(&d).unwrap();
        assert_eq!(
            (c.title.as_str(), c.identifier.as_str(), c.entry.as_str()),
            ("T ok", "dev.x.g", "src/app.ts")
        );
        // A typo in a template stops the build and names the file and line.
        std::fs::write(
            d.join("project.json"),
            "{\n  \"name\": \"g\",\n  \"title\": \"${titel}\"\n}",
        )
        .unwrap();
        let e = GameConfig::load(&d).unwrap_err();
        assert!(
            e.contains("project.json:3:") && e.contains("unknown variable"),
            "{e}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-1852
    #[test]
    fn a_build_writes_the_bundle_css_and_the_assets_the_code_names() {
        let d = temp("build");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("assets")).unwrap();
        std::fs::write(d.join("assets/hit.wav"), b"RIFF").unwrap();
        std::fs::write(d.join("assets/unused.png"), b"x").unwrap();
        std::fs::write(d.join("package.json"), r#"{"name":"g"}"#).unwrap();
        std::fs::write(d.join("src/ui.css"), ".a { color: red }").unwrap();
        std::fs::write(
            d.join("src/util.ts"),
            "export function twice(n: number): number { return n * 2; }",
        )
        .unwrap();
        std::fs::write(
            d.join("src/main.ts"),
            "import \"./ui.css\";\nimport { twice } from \"./util\";\nexport function f(): number { const s = \"assets/hit.wav\"; return twice(s.length); }",
        )
        .unwrap();
        let r = build(
            &d,
            &Options {
                no_tsc: true,
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(r.assets, ["assets/hit.wav"]);
        assert!(
            d.join("build/assets/hit.wav").exists() && !d.join("build/assets/unused.png").exists()
        );
        assert!(d.join("build/assets/hit.wav.meta").exists());
        assert_eq!(
            std::fs::read_to_string(d.join("build/game.css"))
                .unwrap()
                .trim(),
            ".a { color: red }"
        );
        let b: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(d.join("build/game.pbundle")).unwrap())
                .unwrap();
        assert_eq!(b["format"], 1);
        assert_eq!(b["css"], "game.css");
        assert!(b["source"].as_str().unwrap().contains("function twice"));
        assert!(r.object_bytes > 0);
        // A type error stops the build.
        std::fs::write(
            d.join("src/util.ts"),
            "export function twice(n: number): number { return n + undefinedName; }",
        )
        .unwrap();
        assert!(
            build(
                &d,
                &Options {
                    no_tsc: true,
                    ..Options::default()
                }
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    // ---- types/platypus.d.ts as globals (M9-TS-2300..) ----------------------------------------------

    fn globals_game(name: &str, main: &str) -> PathBuf {
        let d = temp(name);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("package.json"), r#"{"name":"g"}"#).unwrap();
        std::fs::write(
            d.join("tsconfig.json"),
            r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022"],"types":[],"module":"ESNext","moduleResolution":"bundler","strict":true,"noImplicitOverride":true,"verbatimModuleSyntax":true,"jsx":"react-jsx"},"include":["src"]}"#,
        )
        .unwrap();
        std::fs::write(d.join("src/main.tsx"), main).unwrap();
        d
    }

    const NO_IMPORTS: &str = "class Hello extends Component {\n  override onStart() {\n    const v = new Vector3(1, 2, 3);\n    console.log(v.x + Time.deltaTime);\n  }\n}\nregister({ Hello });\n";
    const WITH_IMPORTS: &str = "import { Component, register, Vector3, Time } from \"@platypus/engine\";\n\nclass Hello extends Component {\n  override onStart() {\n    const v = new Vector3(1, 2, 3);\n    console.log(v.x + Time.deltaTime);\n  }\n}\nregister({ Hello });\n";

    fn no_tsc() -> Options {
        Options {
            no_tsc: true,
            ..Options::default()
        }
    }

    // M9-TS-2300
    #[test]
    fn the_declarations_are_deterministic_and_name_the_engine_as_globals() {
        let a = types_dts().unwrap();
        assert_eq!(a, types_dts().unwrap());
        for want in [
            "\ndeclare class Vector3 {",
            "\ndeclare class Quaternion {",
            "\ndeclare class Color {",
            "\ndeclare class GameObject {",
            "\ndeclare abstract class Component {",
            "\ndeclare function register(",
            "\ndeclare namespace Physics {",
            "\ndeclare namespace Time {",
            "\ndeclare namespace input {",
            "\ndeclare class Rigidbody {",
            "\ndeclare class BoxCollider {",
            "\ndeclare class Camera {",
            "\ndeclare class Light {",
            "\ndeclare namespace Screen {",
            "\ndeclare namespace QualitySettings {",
            "\ndeclare namespace PlayerPrefs {",
            "\ndeclare class Prefab {",
            "\ndeclare function useState<",
            "\ndeclare function useEffect(",
            "\ndeclare function createRoot(",
            "\ndeclare function Text(",
            "\ndeclare function Dialog(",
            "\ntype ReactNode =",
            "\ninterface PlatypusStyle {",
            "\ninterface PlatypusKeyboardEvent ",
            "\ndeclare type FullScreenMode =",
            "\ndeclare interface Gamepad {",
            "\ndeclare class Collision {",
            "\ndeclare class Collision2D {",
            "\ndeclare class Collider {",
            "\ndeclare class ContactPoint {",
            "\ndeclare module \"@platypus/engine\" {",
            "  export import Vector3 = globalThis.Vector3;",
            "\ndeclare module \"@platypus/react\" {",
            "  export import createRoot = globalThis.createRoot;",
            "\ndeclare module \"react\" {",
            "  export import useState = globalThis.useState;",
        ] {
            assert!(a.contains(want), "missing {want:?}");
        }
        // Nothing is written as `export` outside the three modules.
        assert!(!a.contains("\nexport "), "a top-level export");
    }

    // M9-TS-2301
    #[test]
    fn calls_have_an_options_object_form_and_creatable_classes_a_constructor() {
        let a = types_dts().unwrap();
        // `audio.load(path, { stream: true })` and `new GameObject(name, { position })`.
        assert!(a.contains("function load(path: string, options?: { stream?: boolean }): Sound;"));
        assert!(a.contains("  constructor(name?: string, options?: GameObjectOptions);"));
        assert!(a.contains("  constructor(color?: Color, emission?: Color, smoothness?: number"));
        assert!(a.contains("  static create(options: { "));
        // A required parameter never follows an optional one.
        assert!(!a.contains("?: number, options: {"));
    }

    // M9-TS-2806
    #[test]
    fn platypus_build_types_declare_window_and_window_options() {
        let a = types_dts().unwrap();
        for want in [
            "\ndeclare class Window {",
            "\ndeclare interface WindowOptions {",
            "\ndeclare type WindowStyle =",
            "  static readonly main: Window;",
            "  static readonly current: Window;",
            "  static readonly count: number;",
            "  static readonly focused: Window | undefined;",
            "  static create(options?: WindowOptions): Window;",
            "  style: WindowStyle;",
            "  readonly parent: Window | undefined;",
            "declare function createRoot(target?: WorldPanel | Window | number)",
        ] {
            assert!(a.contains(want), "missing {want:?}");
        }
        // The positional `create` (18 optional parameters) is not declared: `#[options(.., only)]`.
        assert!(!a.contains("static create(title?: string"));
        assert!(!a.contains("static readonly focused: Window;"));
    }

    // M9-TS-2807
    #[test]
    fn tsc_checks_a_program_that_opens_windows() {
        let main = "Window.main.title = \"Game\";\nconst w = Window.create({ title: \"P\", style: \"tool\", parent: Window.main, width: 200 });\nconst f: Window | undefined = Window.focused;\nif (w.parent) console.log(w.parent.title, w.isChild, f === undefined, Window.count, Window.at(0)?.handle);\nw.onResized((width, height) => console.log(width + height));\nw.onCloseRequested(() => w.cancelClose());\nw.style = \"dialog\";\ncreateRoot(w).render(<box><text>{w.title}</text></box>);\ncreateRoot(Window.main).render(<box />);\nw.postMessage({ kind: \"select\", ids: [1, 2] });\nw.onMessage<{ kind: string }>((from, m) => console.log(from.title + m.kind));\nw.onRequest<number, string>((from, n, reply) => reply(\"x\" + n));\nWindow.onMessage<{ n: number }>((from, m) => console.log(m.n));\nWindow.broadcast({ n: 1 });\nconst reply: Promise<{ n: number }> = w.request<{ n: number }>({ kind: \"stats\" }, 500);\n";
        let d = globals_game("tscwin", main);
        let Some(_) = find_tsc(&d) else {
            eprintln!("M9-TS-2807 skipped: tsc not found (install TypeScript)");
            let _ = std::fs::remove_dir_all(&d);
            return;
        };
        let ok = |d: &Path| {
            build(
                d,
                &Options {
                    no_compile: true,
                    ..Options::default()
                },
            )
        };
        assert_eq!(ok(&d).unwrap().tsc, Tsc::Ok);
        // The option types are real.
        std::fs::write(
            d.join("src/main.tsx"),
            "Window.create({ style: \"floating\" });\n",
        )
        .unwrap();
        assert!(ok(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2302
    #[test]
    fn a_program_without_imports_compiles() {
        let d = globals_game("noimp", NO_IMPORTS);
        let r = build(&d, &no_tsc()).unwrap();
        assert!(r.object_bytes > 0 && r.files.len() == 1 && r.files[0].ends_with("src/main.tsx"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2303
    #[test]
    fn the_old_import_forms_still_compile() {
        let d = globals_game("oldimp", WITH_IMPORTS);
        let r = build(&d, &no_tsc()).unwrap();
        assert!(r.object_bytes > 0);
        let src = std::fs::read_to_string(d.join("build/game.pbundle")).unwrap();
        assert!(!src.contains("@platypus/engine\\\""), "the import was kept");
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2879
    #[test]
    fn a_module_declaring_an_engine_name_does_not_capture_the_engines() {
        let d = globals_game(
            "shadow",
            "import { Random as Dice, lerp } from \"rng\";
class Hello extends Component {
  override onStart() {
    console.log(lerp(0, 10, Random.value) + new Dice().pick());
  }
}
register({ Hello });
",
        );
        std::fs::create_dir_all(d.join("src/modules")).unwrap();
        std::fs::write(
            d.join("src/modules/rng.ts"),
            "export class Random {
  pick(): number { return 4; }
}
export function lerp(a: number, b: number, t: number): number { return a + (b - a) * t; }
",
        )
        .unwrap();
        let r = build(&d, &no_tsc()).unwrap();
        assert!(r.object_bytes > 0);
        let src = std::fs::read_to_string(d.join("build/game.pbundle")).unwrap();
        assert!(src.contains("class Random_2"), "{src}");
        assert!(src.contains("Random.value"), "{src}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2304
    #[test]
    fn the_tsc_config_extends_the_games_and_adds_the_declarations() {
        let d = globals_game("cfg2", NO_IMPORTS);
        let c = GameConfig::load(&d).unwrap();
        let v: serde_json::Value = serde_json::from_str(&tsc_config(&c)).unwrap();
        assert_eq!(v["extends"], "../../tsconfig.json");
        assert_eq!(v["include"][0], "../../src");
        assert_eq!(v["files"][0], "platypus.d.ts");
        assert_eq!(v["compilerOptions"]["moduleDetection"], "force");
        assert_eq!(v["compilerOptions"]["jsx"], "preserve");
        // M9-TS-2875: tsc resolves bare imports the way the compiler does.
        assert_eq!(v["compilerOptions"]["paths"]["*"][0], "../../src/modules/*");
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2305
    #[test]
    fn tsc_checks_a_program_without_imports_and_the_old_forms() {
        let d = globals_game("tscnoimp", NO_IMPORTS);
        let Some(_) = find_tsc(&d) else {
            eprintln!("M9-TS-2305 skipped: tsc not found (install TypeScript)");
            let _ = std::fs::remove_dir_all(&d);
            return;
        };
        let ok = |d: &Path| {
            build(
                d,
                &Options {
                    no_compile: true,
                    ..Options::default()
                },
            )
        };
        assert_eq!(ok(&d).unwrap().tsc, Tsc::Ok);
        // The declarations are real: a wrong call fails.
        std::fs::write(
            d.join("src/main.tsx"),
            "class Hello extends Component {\n  override onStart() {\n    const v = new Vector3(1, 2);\n    const n: number = \"x\";\n  }\n}\n",
        )
        .unwrap();
        let Err(e) = ok(&d) else {
            panic!("a wrong call type-checked")
        };
        assert!(e.contains("tsc failed") && e.contains("TS2554"), "{e}");
        // The old imports.
        std::fs::write(d.join("src/main.tsx"), WITH_IMPORTS).unwrap();
        assert_eq!(ok(&d).unwrap().tsc, Tsc::Ok);
        std::fs::write(
            d.join("src/main.tsx"),
            "import { useState } from \"react\";\nimport { createRoot, Text, type PlatypusStyle } from \"@platypus/react\";\nimport { type Gamepad, Color } from \"@platypus/engine\";\nconst style: PlatypusStyle = { color: \"red\" };\nfunction App() {\n  const [n, setN] = useState(0);\n  const g: Gamepad | null = null;\n  return <box style={style} onClick={() => setN(n + 1)}><Text>{n}{String(g)}{Color.white.r}</Text></box>;\n}\ncreateRoot().render(<App />);\n",
        )
        .unwrap();
        assert_eq!(ok(&d).unwrap().tsc, Tsc::Ok);
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2306
    #[test]
    fn the_generated_declarations_have_no_errors_of_their_own() {
        let d = globals_game("tscdts", NO_IMPORTS);
        let Some(tsc) = find_tsc(&d) else {
            eprintln!("M9-TS-2306 skipped: tsc not found (install TypeScript)");
            let _ = std::fs::remove_dir_all(&d);
            return;
        };
        std::fs::write(d.join("platypus.d.ts"), types_dts().unwrap()).unwrap();
        // skipLibCheck off: declaration errors are reported too.
        let o = std::process::Command::new(tsc)
            .args([
                "--noEmit",
                "--strict",
                "--skipLibCheck",
                "false",
                "--lib",
                "ES2022",
                "--target",
                "ES2022",
                "platypus.d.ts",
            ])
            .current_dir(&d)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(o.status.success(), "{text}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2721
    #[test]
    fn every_example_is_found_by_its_project_json_and_has_no_package_json() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let user = platypus_core::project::FixedUser {
            username: "tester".into(),
            full_name: "Test Person".into(),
            ..Default::default()
        };
        let mut games = 0;
        for p in find_games(&examples) {
            games += 1;
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            assert!(p.join("project.json").is_file(), "{name}");
            assert!(
                !p.join("package.json").exists(),
                "{name}: games need no package.json"
            );
            let proj = platypus_core::project::Project::load(
                &p,
                &platypus_core::project::LoadOptions::new(
                    platypus_core::project::Platform::Windows,
                    &user,
                ),
            )
            .unwrap();
            assert_eq!(
                proj.source,
                platypus_core::project::Source::ProjectJson,
                "{name}"
            );
            assert_eq!(proj.identifier, format!("dev.platypus.{name}"));
            assert_eq!(proj.company, "Test's Company");
            assert_eq!(proj.license, "private");
            assert!(!proj.title.is_empty());
        }
        assert_eq!(games, 55);
    }

    // M9-TS-2307
    #[test]
    fn all_the_examples_type_check_against_the_generated_declarations() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let Some(_) = find_tsc(&examples) else {
            eprintln!("M9-TS-2307 skipped: tsc not found (install TypeScript)");
            return;
        };
        let mut games = 0;
        for p in find_games(&examples) {
            games += 1;
            let r = build(
                &p,
                &Options {
                    no_compile: true,
                    ..Options::default()
                },
            );
            assert!(
                matches!(&r, Ok(r) if r.tsc == Tsc::Ok),
                "{}: {:?}",
                p.display(),
                r.map(|r| r.tsc)
            );
        }
        assert!(games >= 19, "{games} examples");
    }

    // M9-TS-2308
    #[test]
    fn the_examples_import_nothing_from_the_engine() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        for e in std::fs::read_dir(&examples).unwrap().flatten() {
            let src = e.path().join("src");
            let Ok(rd) = std::fs::read_dir(&src) else {
                continue;
            };
            for f in rd.flatten() {
                let Ok(t) = std::fs::read_to_string(f.path()) else {
                    continue;
                };
                for bad in [
                    "\"@platypus/engine\"",
                    "\"@platypus/react\"",
                    "from \"react\"",
                ] {
                    assert!(!t.contains(bad), "{}: {bad}", f.path().display());
                }
            }
        }
    }

    // M9-TS-2722
    #[test]
    fn empty_metadata_fields_warn_in_a_build_and_nothing_is_blocked() {
        let d = temp("emptywarn");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/main.ts"), "export const a = 1;").unwrap();
        std::fs::write(
            d.join("project.json"),
            r#"{ "name": "g", "company": "", "copyright": "" }"#,
        )
        .unwrap();
        let r = build(
            &d,
            &Options {
                no_tsc: true,
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
        assert!(
            r.warnings[0].ends_with(
                "project.json: \"company\" is empty (the exe's version info will have no company)"
            ),
            "{:?}",
            r.warnings
        );
        // Left out is not empty: no warning.
        std::fs::write(d.join("project.json"), r#"{ "name": "g" }"#).unwrap();
        let r = build(
            &d,
            &Options {
                no_tsc: true,
                ..Options::default()
            },
        )
        .unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-TS-2725
    #[test]
    fn games_are_found_by_project_json_and_the_package_json_fallback() {
        let d = temp("discover");
        for (dir, file, text) in [
            ("a", "project.json", r#"{ "name": "a" }"#),
            ("b", "package.json", r#"{ "name": "b", "platypus": {} }"#),
            ("c", "package.json", r#"{ "name": "c" }"#),
            ("d", "tsconfig.json", "{}"),
        ] {
            std::fs::create_dir_all(d.join(dir)).unwrap();
            std::fs::write(d.join(dir).join(file), text).unwrap();
        }
        let names: Vec<String> = find_games(&d)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a", "b"]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
