// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Editor`: what the editor (`editor/`, a game like any other) asks of the host. The log for its
//! console (`logSince`), opening a file in the user's code editor (`openFile`) and file operations
//! on the project folder (`listDir`, `copyIn`, `move`) that answer at once, with `lastError` saying
//! why one failed.
//!
//! It works only when the host was started with a project (`platypus-app editor --project <folder>`,
//! [`enable`]); in any other game the calls do nothing and fail, so games can't use them. The host
//! feeds the log ([`push_log`] from its tracing layer while [`capturing`]) and says which outside
//! paths were dropped on the window ([`allow_sources`]), the only ones `copyIn` copies.

pub mod files;
mod open;
pub mod project;

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;
use ts_api::ts_api;

/// The most log records kept; older ones drop off.
pub const LOG_LIMIT: usize = 5000;

/// One log record as the editor's console shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct LogRecord {
    /// Counts up from 1, so readers can ask for what they haven't seen.
    pub seq: f64,
    /// 0 trace, 1 debug, 2 info, 3 warn, 4 error.
    pub level: f64,
    pub target: String,
    pub message: String,
    /// Milliseconds since the log started.
    pub time_ms: f64,
}

/// A key of the project's `project.json` as Project Settings shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectField {
    pub key: String,
    /// `string`, `bool`, `enum` or `path`.
    pub ty: String,
    pub group: String,
    /// An enum's choices, `|` between them ("" is "left out").
    pub options: String,
    pub doc: String,
    /// The value as written (a string's text with its `${templates}`; `true` / `false`); "" when unset.
    pub value: String,
    /// Whether the file has the key.
    pub set: bool,
}

/// Checks a value for a settings field; the value to write (`None`: remove the key).
fn project_value(key: &str, json: &str) -> Result<Option<serde_json::Value>, String> {
    use serde_json::Value;
    let f = platypus_core::project_edit::FIELDS
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| format!("{key} isn't a project setting"))?;
    let v: Value = serde_json::from_str(json).map_err(|e| format!("{key}: {e}"))?;
    match (f.ty, &v) {
        ("bool", Value::Bool(_)) => Ok(Some(v)),
        ("enum", Value::String(s)) if s.is_empty() => Ok(None),
        ("enum", Value::String(s)) if f.options.contains(&s.as_str()) => Ok(Some(v)),
        ("enum", _) => Err(format!("{key} is one of {}", f.options[1..].join(", "))),
        ("string" | "path", Value::String(s)) if key == "name" && s.trim().is_empty() => {
            Err("a project needs a name".into())
        }
        // An empty layer name or size removes the key (unnamed, or the default).
        ("string" | "number", Value::String(s)) if s.is_empty() && key.contains('.') => Ok(None),
        ("string" | "path", Value::String(_)) => Ok(Some(v)),
        ("number", Value::Number(_)) => Ok(Some(v)),
        _ => Err(format!("{key} takes a {}", f.ty)),
    }
}

/// What a project path is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileStat {
    pub exists: bool,
    pub dir: bool,
    pub size: f64,
    /// Last modified, milliseconds since 1970 (0 when unknown).
    pub modified_ms: f64,
}

/// One entry of a folder listing.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectEntry {
    pub name: String,
    pub dir: bool,
    /// Bytes (0 for folders).
    pub size: f64,
}

struct State {
    /// The project folder (`--project`): set, the editor calls work.
    project: Option<PathBuf>,
    /// The editor's own folder, for relative paths in `openFile`.
    ui_root: PathBuf,
    /// Files and folders dropped on the window: the only outside paths `copyIn` copies.
    dropped: BTreeSet<PathBuf>,
    log: VecDeque<LogRecord>,
    next_seq: u64,
    started: Option<Instant>,
    /// Why the last file operation failed ("" when it worked).
    error: String,
}

// A static, not per thread: the host's tracing layer pushes records from any thread.
static STATE: Mutex<State> = Mutex::new(State {
    project: None,
    ui_root: PathBuf::new(),
    dropped: BTreeSet::new(),
    log: VecDeque::new(),
    next_seq: 1,
    started: None,
    error: String::new(),
});

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Turns the editor calls on for a project folder (made absolute); `ui_root` is the editor's own
/// folder. The log is kept from now on.
pub fn enable(project: &Path, ui_root: &Path) {
    let project = std::path::absolute(project).unwrap_or_else(|_| project.to_path_buf());
    st(|s| {
        s.project = Some(project);
        s.ui_root = ui_root.to_path_buf();
        s.started.get_or_insert_with(Instant::now);
    });
}

/// Whether the host runs the editor ([`enable`] was called).
pub fn enabled() -> bool {
    st(|s| s.project.is_some())
}

/// Whether log records are kept (the host's tracing layer asks before formatting one).
pub fn capturing() -> bool {
    enabled()
}

/// Paths dropped on the window from the system, which `copyIn` may copy.
pub fn allow_sources(files: &[String]) {
    st(|s| s.dropped.extend(files.iter().map(PathBuf::from)));
}

/// Keeps one log record (while [`capturing`]).
pub fn push_log(level: u8, target: &str, message: String) {
    st(|s| {
        if s.project.is_none() {
            return;
        }
        let started = *s.started.get_or_insert_with(Instant::now);
        let seq = s.next_seq;
        s.next_seq += 1;
        if s.log.len() >= LOG_LIMIT {
            s.log.pop_front();
        }
        s.log.push_back(LogRecord {
            seq: seq as f64,
            level: f64::from(level),
            target: target.to_string(),
            message,
            time_ms: started.elapsed().as_secs_f64() * 1000.0,
        });
    });
}

/// For tests: forgets the project, the log and the dropped paths.
pub fn reset() {
    st(|s| {
        s.project = None;
        s.dropped.clear();
        s.log.clear();
        s.next_seq = 1;
        s.started = None;
        s.error.clear();
    });
}

/// Runs an editor operation on the project folder (other crates' editor calls too), keeping its
/// error for `Editor.lastError` and warning with it; `T::default()` when it failed.
pub fn project_op<T: Default>(name: &str, f: impl FnOnce(&Path) -> Result<T, String>) -> T {
    file_op(name, |root, _| f(root))
}

/// The project folder, when the editor runs.
pub fn project_root() -> Option<PathBuf> {
    st(|s| s.project.clone())
}

/// Runs a file operation on the project folder, keeping its error for `lastError`.
fn file_op<T: Default>(
    name: &str,
    f: impl FnOnce(&Path, &BTreeSet<PathBuf>) -> Result<T, String>,
) -> T {
    let (root, dropped) = st(|s| (s.project.clone(), s.dropped.clone()));
    let result = match root {
        Some(root) => f(&root, &dropped),
        None => Err("no project folder (start the editor with --project <folder>)".into()),
    };
    match result {
        Ok(v) => {
            st(|s| s.error.clear());
            v
        }
        Err(e) => {
            warn(&format!("Editor.{name}: {e}"));
            st(|s| s.error = e);
            T::default()
        }
    }
}

/// A warning in the log (through the log sink: this crate has no tracing of its own).
fn warn(text: &str) {
    crate::log::emit(crate::log::Level::Warn, text);
}

// The shim of a call returning one record (`stat`) writes through the caller's pointer.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[ts_api(
    namespace = "Editor",
    records(
        /// One record of the engine's log: engine lines and the scripts' `console.*` (target `script`) in order.
        LogRecord = LogRecord [seq: f64, level: f64, target: string, message: string, time_ms: f64],
        /// A file or folder of the project.
        ProjectEntry = ProjectEntry [name: string, dir: bool, size: f64],
        /// What a project path is (`Editor.stat`).
        FileStat = FileStat [exists: bool, dir: bool, size: f64, modified_ms: f64],
        /// A key of `project.json` as Project Settings shows it.
        ProjectField = ProjectField [key: string, ty: string, group: string, options: string, doc: string, value: string, set: bool]
    )
)]
pub mod editor_api {
    use super::*;

    /// Whether the host runs the editor on a project (`platypus-app editor --project <folder>`).
    #[property]
    pub fn is_available() -> bool {
        enabled()
    }

    /// The project folder's full path ("" without one).
    #[property]
    pub fn project() -> String {
        st(|s| {
            s.project
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
    }

    /// The kept log records with `seq` above `after`, oldest first (the last 5000 at most).
    pub fn log_since(after: f64) -> Vec<LogRecord> {
        st(|s| s.log.iter().filter(|r| r.seq > after).cloned().collect())
    }

    /// Opens a file at a line and column in the user's code editor: `PLATYPUS_EDITOR_OPEN` (with
    /// `{file}`, `{line}`, `{col}`), else VS Code's `code -g` when it is on PATH. A relative path is
    /// looked up in the current folder, the editor's folder and the engine's source tree.
    #[optional(line = 1.0, col = 1.0)]
    pub fn open_file(path: &str, line: f64, col: f64) {
        let Some(ui_root) = st(|s| s.project.is_some().then(|| s.ui_root.clone())) else {
            warn("Editor.openFile: only the editor opens files");
            return;
        };
        if let Err(e) = open::open_file(path, line.max(1.0) as u32, col.max(1.0) as u32, &ui_root) {
            warn(&e);
        }
    }

    /// A project folder's entries ("" is the root): folders first, by name, without dot files.
    pub fn list_dir(path: &str) -> Vec<ProjectEntry> {
        file_op("listDir", |root, _| {
            Ok(project::list(root, path)?
                .into_iter()
                .map(|e| ProjectEntry {
                    name: e.name,
                    dir: e.dir,
                    size: e.size as f64,
                })
                .collect())
        })
    }

    /// Copies files or folders dropped on the window into a project folder, renaming instead of
    /// overwriting (`a (2).png`). Returns the new project paths.
    pub fn copy_in(sources: &[String], dir: &str) -> Vec<String> {
        file_op("copyIn", |root, dropped| {
            let sources: Vec<PathBuf> = sources.iter().map(PathBuf::from).collect();
            if let Some(bad) = sources.iter().find(|s| !dropped.contains(*s)) {
                return Err(format!("{} wasn't dropped on the editor", bad.display()));
            }
            project::copy_in(root, &sources, dir)
        })
    }

    /// Moves a project file or folder into a project folder; returns its new path. Refuses
    /// overwriting and moving a folder into itself.
    #[js_name = "move"]
    pub fn move_into(path: &str, dir: &str) -> String {
        file_op("move", |root, _| project::move_into(root, path, dir))
    }

    /// A project file's text ("" when it can't be read: see `lastError`).
    pub fn read_text(path: &str) -> String {
        file_op("readText", |root, _| files::read_text(root, path))
    }

    /// Writes a project file, making its folders; the old file is replaced in one step (the text
    /// goes to a temporary file first). False when it failed.
    pub fn write_text(path: &str, text: &str) -> bool {
        file_op("writeText", |root, _| {
            files::write_text(root, path, text).map(|()| true)
        })
    }

    /// Whether a project path exists.
    pub fn exists(path: &str) -> bool {
        project_root().is_some_and(|r| files::exists(&r, path))
    }

    /// Whether a project path exists, is a folder, its size and when it last changed.
    pub fn stat(path: &str) -> FileStat {
        file_op("stat", |root, _| {
            let s = files::stat(root, path)?;
            Ok(FileStat {
                exists: s.exists,
                dir: s.dir,
                size: s.size as f64,
                modified_ms: s.modified_ms,
            })
        })
    }

    /// A path in project folder `dir` that isn't taken: `dir/name`, else `dir/name (2).ext`, ...
    pub fn free_name(dir: &str, name: &str) -> String {
        file_op("freeName", |root, _| files::free_name(root, dir, name))
    }

    /// Makes a project folder (and those above it); returns its path ("" if it exists or failed).
    pub fn create_dir(path: &str) -> String {
        file_op("createDir", |root, _| files::create_dir(root, path))
    }

    /// Renames a project file or folder in its folder (its `.meta` too); returns the new path.
    pub fn rename(path: &str, name: &str) -> String {
        file_op("rename", |root, _| files::rename(root, path, name))
    }

    /// Moves a project file or folder (and its `.meta`) into the project's trash
    /// (`.platypus/trash`); returns the id `restore` puts it back with.
    pub fn trash(path: &str) -> String {
        file_op("trash", |root, _| files::trash(root, path))
    }

    /// Puts an item back from the trash where it was; returns its path.
    pub fn restore(id: &str) -> String {
        file_op("restore", |root, _| files::restore(root, id))
    }

    /// The project's start scene (`startScene` in its `project.json`; "" for none).
    #[property]
    pub fn start_scene() -> String {
        project_root()
            .and_then(|r| std::fs::read_to_string(r.join("project.json")).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["startScene"].as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// The project's 32 layer names (its `project.json` `layers` over the built-ins; "" for a
    /// layer without a name), for the Inspector's layer choice.
    pub fn layer_names() -> Vec<String> {
        let layers = project_root()
            .and_then(|r| std::fs::read_to_string(r.join("project.json")).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .map(|v| v["layers"].clone())
            .unwrap_or_default();
        crate::layers::names_for(&layers)
    }

    /// The settings of the project's `project.json`, as written (templates unexpanded).
    pub fn project_fields() -> Vec<ProjectField> {
        file_op("projectFields", |root, _| {
            let text = files::read_text(root, "project.json")?;
            platypus_core::project_edit::FIELDS
                .iter()
                .map(|f| {
                    let raw = platypus_core::project_edit::get_key(&text, f.key)?;
                    let value = match raw
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                    {
                        Some(Ok(serde_json::Value::String(s))) => s,
                        Some(Ok(v)) => v.to_string(),
                        _ => String::new(),
                    };
                    Ok(ProjectField {
                        key: f.key.into(),
                        ty: f.ty.into(),
                        group: f.group.into(),
                        options: f.options.join("|"),
                        doc: f.doc.into(),
                        value,
                        set: raw.is_some(),
                    })
                })
                .collect()
        })
    }

    /// Sets a project setting to a value (JSON text), changing only that key's text in
    /// `project.json` (an empty choice removes the key). False with a reason in `lastError`.
    pub fn set_project_field(key: &str, json: &str) -> bool {
        file_op("setProjectField", |root, _| {
            let text = files::read_text(root, "project.json")?;
            let out = match project_value(key, json)? {
                Some(v) => platypus_core::project_edit::set_key(&text, key, &v)?,
                None => platypus_core::project_edit::remove_key(&text, key)?,
            };
            serde_json::from_str::<serde_json::Value>(&out)
                .map_err(|e| format!("the edit would break project.json: {e}"))?;
            files::write_text(root, "project.json", &out).map(|()| true)
        })
    }

    /// Removes a project setting from `project.json` (its default applies); not `name`.
    pub fn remove_project_field(key: &str) -> bool {
        file_op("removeProjectField", |root, _| {
            if key == "name" {
                return Err("a project needs a name".into());
            }
            let text = files::read_text(root, "project.json")?;
            let out = platypus_core::project_edit::remove_key(&text, key)?;
            files::write_text(root, "project.json", &out).map(|()| true)
        })
    }

    /// Gives a project asset a `.meta` file (its GUID and importer settings) if it has none;
    /// returns the GUID ("" for a file that isn't an asset).
    pub fn ensure_meta(path: &str) -> String {
        file_op("ensureMeta", |root, _| {
            let file = project::resolve(root, path)?;
            platypus_core::meta::ensure(&file).map_err(|e| format!("{path}: {e}"))
        })
    }

    /// Why the last file operation failed ("" when it worked).
    #[property]
    pub fn last_error() -> String {
        st(|s| s.error.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The editor's state is one for the process: these tests take turns.
    static ONE: Mutex<()> = Mutex::new(());

    // M5-EDT-48
    #[test]
    fn layer_names_are_the_projects_over_the_built_ins() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let dir =
            std::env::temp_dir().join(format!("platypus-editor-layers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("project.json"),
            r#"{ "name": "x", "layers": { "8": "Player", "4": "Lava", "40": "No", "9": "" } }"#,
        )
        .unwrap();
        enable(&dir, &dir);
        let names = editor_api::layer_names();
        assert_eq!(names.len(), 32);
        assert_eq!(
            (
                names[0].as_str(),
                names[4].as_str(),
                names[8].as_str(),
                names[9].as_str()
            ),
            ("Default", "Lava", "Player", "")
        );
        reset();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // M5-PRJ-06
    #[test]
    fn display_quality_and_layer_settings_edit_their_sections() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let dir = std::env::temp_dir().join(format!("platypus-editor-prj6-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("project.json"),
            "{\n  \"name\": \"x\",\n  \"layers\": { \"8\": \"A\" }\n}\n",
        )
        .unwrap();
        enable(&dir, &dir);
        let fields = editor_api::project_fields();
        let get = |k: &str| fields.iter().find(|f| f.key == k).unwrap().clone();
        assert_eq!(
            (
                get("layers.8").value.as_str(),
                get("layers.8").group.as_str()
            ),
            ("A", "Layers")
        );
        assert_eq!(get("display.width").ty, "number");
        assert!(editor_api::set_project_field("display.width", "1600"));
        assert!(
            !editor_api::set_project_field("display.width", "\"wide\""),
            "a number"
        );
        assert!(editor_api::set_project_field("quality.default", "\"High\""));
        assert!(
            editor_api::set_project_field("layers.8", "\"\""),
            "empty removes"
        );
        let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["display"]["width"], 1600);
        assert_eq!(v["quality"]["default"], "High");
        assert!(v.get("layers").is_none(), "{text}");
        reset();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // M5-PRJ-04
    #[test]
    fn project_settings_edit_only_their_keys() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let dir = std::env::temp_dir().join(format!("platypus-editor-prj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file =
            "{\n  \"name\": \"${firstName}'s\",\n  \"custom\": [1],\n  \"renderer\": \"gles\"\n}\n";
        std::fs::write(dir.join("project.json"), file).unwrap();
        enable(&dir, &dir);
        let fields = editor_api::project_fields();
        let get = |k: &str| fields.iter().find(|f| f.key == k).unwrap().clone();
        assert_eq!(
            (get("name").value.as_str(), get("name").set),
            ("${firstName}'s", true)
        );
        assert!(!get("title").set);
        assert_eq!(get("renderer").options, "|vulkan|gles|cpu");

        assert!(editor_api::set_project_field("title", "\"Hello\""));
        assert!(editor_api::set_project_field("fps", "false"));
        // An empty choice removes the key.
        assert!(editor_api::set_project_field("renderer", "\"\""));
        let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
        assert_eq!(
            text,
            "{\n  \"name\": \"${firstName}'s\",\n  \"custom\": [1],\n  \"title\": \"Hello\",\n  \"fps\": false\n}\n"
        );
        // Refused, with a reason; the file is unchanged.
        assert!(!editor_api::set_project_field("renderer", "\"metal\""));
        assert!(editor_api::last_error().contains("renderer"));
        assert!(!editor_api::set_project_field("name", "\"  \""));
        assert!(!editor_api::set_project_field("fps", "\"yes\""));
        assert!(
            !editor_api::set_project_field("custom", "1"),
            "not a setting"
        );
        assert!(!editor_api::remove_project_field("name"));
        assert!(editor_api::remove_project_field("title"));
        assert_eq!(
            std::fs::read_to_string(dir.join("project.json")).unwrap(),
            "{\n  \"name\": \"${firstName}'s\",\n  \"custom\": [1],\n  \"fps\": false\n}\n"
        );
        reset();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // M5-EDT-27
    #[test]
    fn editor_calls_fail_without_a_project_and_work_with_one() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert!(!editor_api::is_available());
        push_log(2, "script", "not kept".into());
        assert!(editor_api::log_since(0.0).is_empty());
        assert!(editor_api::list_dir("").is_empty());
        assert!(editor_api::last_error().contains("--project"));

        let dir = std::env::temp_dir().join(format!("platypus-editor-api-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        enable(&dir, &dir);
        assert!(editor_api::is_available());
        push_log(3, "script", "careful".into());
        push_log(2, "platypus_app", "loaded".into());
        let got: Vec<_> = editor_api::log_since(0.0)
            .into_iter()
            .map(|r| (r.seq, r.level, r.target, r.message))
            .collect();
        assert_eq!(
            got,
            [
                (1.0, 3.0, "script".to_string(), "careful".to_string()),
                (2.0, 2.0, "platypus_app".to_string(), "loaded".to_string())
            ]
        );
        assert_eq!(editor_api::log_since(1.0).len(), 1);
        let names: Vec<_> = editor_api::list_dir("")
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["assets"]);
        assert_eq!(editor_api::last_error(), "");
        // Only what was dropped on the window comes in from outside.
        assert!(editor_api::copy_in(&["C:/elsewhere/x.png".into()], "assets").is_empty());
        assert!(editor_api::last_error().contains("wasn't dropped"));
        reset();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
