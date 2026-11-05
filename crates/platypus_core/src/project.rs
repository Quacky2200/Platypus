// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `project.json`: a game's project and product metadata (name, title, company, identifier, icon,
//! start scene, layers, quality, ...), shared by `platypus-build` and `platypus_app`.
//!
//! Strings may hold **template variables** so a fresh project takes its defaults from the person
//! building it (`"title": "${firstName}'s Game"`): `${username}`, `${firstName}`, `${lastName}`,
//! `${fullName}` (from [`UserInfo`]), `${year}`, `${date}`, the project's own `${name}`,
//! `${title}`, `${company}`, `${version}`, `${identifier}`, `${description}`, `${copyright}`,
//! `${exe}`, and `${env:NAME}` / `${env:NAME:-fallback}`. `$${` writes a literal `${`. An unknown
//! variable is an error that names the file and line. Platform sections (`"windows": { .. }`)
//! override fields for that platform. `name` is the only required field; an omitted field takes a
//! default (`"title": "${firstName}'s Game"`, `"company": "${firstName}'s Company"`, license
//! `private`, ...) and an explicitly empty string stays empty ([`Project::empty_fields`] lists
//! them so the builder can warn). **Deprecated:** without a `project.json` the `platypus` section of
//! `package.json` (and its `name`, `version`, `author`, `description`) is used, without templates.
//!
//! A *resolved* project (`"resolved": true`, what [`Project::to_json`] writes and the packaged exe
//! carries) is read as it is: no templates, no platform sections.

use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// The file a project is described in.
pub const PROJECT_FILE: &str = "project.json";

/// The platforms a project can name in a section of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Macos,
    Linux,
    Android,
    Ios,
}

impl Platform {
    pub const ALL: [Platform; 5] = [
        Platform::Windows,
        Platform::Macos,
        Platform::Linux,
        Platform::Android,
        Platform::Ios,
    ];

    /// The platform this program was built for.
    pub fn current() -> Platform {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "android") {
            Platform::Android
        } else if cfg!(target_os = "ios") {
            Platform::Ios
        } else {
            Platform::Linux
        }
    }

    /// The key of the section in `project.json`.
    pub fn name(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Macos => "macos",
            Platform::Linux => "linux",
            Platform::Android => "android",
            Platform::Ios => "ios",
        }
    }

    pub fn parse(s: &str) -> Option<Platform> {
        Platform::ALL.into_iter().find(|p| p.name() == s)
    }
}

/// What the templates know about the person and the machine. Tests inject their own.
pub trait UserInfo {
    /// The account name (`quack`).
    fn username(&self) -> String;
    /// The full or display name (`Matthew Smith`); empty when the system has none.
    fn full_name(&self) -> String;
    /// Today as year, month, day.
    fn today(&self) -> (i32, u32, u32) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        civil_from_days(secs.div_euclid(86_400))
    }
    /// An environment variable (`${env:NAME}`).
    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
    (y, m, d)
}

/// The person running this program, read from the operating system.
pub struct SystemUser;

impl UserInfo for SystemUser {
    fn username(&self) -> String {
        ["USERNAME", "USER", "LOGNAME"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "user".into())
    }

    fn full_name(&self) -> String {
        system_full_name(&self.username())
    }
}

/// A user given by hand (tests, `platypus-build` flags).
#[derive(Clone, Debug, Default)]
pub struct FixedUser {
    pub username: String,
    pub full_name: String,
    pub date: (i32, u32, u32),
    pub env: HashMap<String, String>,
}

impl UserInfo for FixedUser {
    fn username(&self) -> String {
        self.username.clone()
    }
    fn full_name(&self) -> String {
        self.full_name.clone()
    }
    fn today(&self) -> (i32, u32, u32) {
        self.date
    }
    fn env(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }
}

#[cfg(windows)]
fn system_full_name(_username: &str) -> String {
    #[link(name = "secur32")]
    unsafe extern "system" {
        fn GetUserNameExW(format: i32, buffer: *mut u16, size: *mut u32) -> u8;
    }
    const NAME_DISPLAY: i32 = 3;
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` UTF-16 units; the function writes at most that many and sets `len`.
    let ok = unsafe { GetUserNameExW(NAME_DISPLAY, buf.as_mut_ptr(), &mut len) };
    if ok != 0 && len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
            .trim()
            .to_string()
    } else {
        String::new()
    }
}

#[cfg(not(windows))]
fn system_full_name(username: &str) -> String {
    use std::process::Command;
    let run = |cmd: &mut Command| {
        cmd.output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    if cfg!(target_os = "macos") {
        let n = run(Command::new("id").arg("-F"));
        if !n.is_empty() {
            return n;
        }
        let out = run(Command::new("dscl").args([
            ".",
            "-read",
            &format!("/Users/{username}"),
            "RealName",
        ]));
        // "RealName:\n Matthew Smith" or "RealName: Matthew Smith"
        return out
            .split_once(':')
            .map(|(_, n)| n.trim().to_string())
            .unwrap_or_default();
    }
    gecos_name(&run(Command::new("getent").args(["passwd", username])))
}

/// The first comma part of the GECOS field (5th) of a `passwd` line.
pub fn gecos_name(passwd_line: &str) -> String {
    passwd_line
        .lines()
        .next()
        .and_then(|l| l.split(':').nth(4))
        .and_then(|g| g.split(',').next())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// First and last word of a full name; a single word has no last name.
pub fn split_name(full: &str) -> (String, String) {
    let words: Vec<&str> = full.split_whitespace().collect();
    match words.as_slice() {
        [] => (String::new(), String::new()),
        [one] => ((*one).to_string(), String::new()),
        [first, .., last] => ((*first).to_string(), (*last).to_string()),
    }
}

/// A `project.json` problem, with the place in the file when there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectError {
    pub file: String,
    /// 1-based; 0 when the place is unknown.
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(
                f,
                "{}:{}:{}: {}",
                self.file, self.line, self.col, self.message
            )
        } else {
            write!(f, "{}: {}", self.file, self.message)
        }
    }
}

impl std::error::Error for ProjectError {}

/// Where the values came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `project.json`.
    ProjectJson,
    /// The `platypus` section of `package.json`.
    PackageJson,
    /// Neither file exists: only defaults.
    Defaults,
}

/// Which renderer `platypus-app` draws a project with (`renderer` in `project.json`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Renderer {
    /// Left out: chosen from what the compiled program uses (the bundle's tier): GLES for a
    /// program that is only UI, wgpu with the world renderer otherwise.
    #[default]
    Auto,
    /// wgpu with the world renderer, even for a program that is only UI (Vulkan by default on
    /// Windows and Linux, DX12 through `WGPU_BACKEND`, Metal on macOS).
    Vulkan,
    /// The light UI-only path: GLES through EGL (ANGLE linked in on Windows), else wgpu with
    /// small GPU allocations. No 3D world: "3D Not Supported" when the game asks for it.
    Gles,
    /// The UI drawn on the CPU (`platypus_render_cpu`, softbuffer), no GPU API. UI only, as `gles`.
    Cpu,
}

impl Renderer {
    /// A value of `renderer` (`auto` is the default, also accepted on the command line).
    pub fn parse(s: &str) -> Option<Renderer> {
        match s {
            "auto" => Some(Renderer::Auto),
            "vulkan" => Some(Renderer::Vulkan),
            "gles" => Some(Renderer::Gles),
            "cpu" => Some(Renderer::Cpu),
            _ => None,
        }
    }

    /// The value written in `project.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Renderer::Auto => "auto",
            Renderer::Vulkan => "vulkan",
            Renderer::Gles => "gles",
            Renderer::Cpu => "cpu",
        }
    }

    /// Whether this renderer draws only the UI (no world renderer).
    pub fn ui_only(self) -> bool {
        matches!(self, Renderer::Gles | Renderer::Cpu)
    }

    /// The first renderer that is set (not `Auto`), in order of precedence.
    pub fn first_set(choices: impl IntoIterator<Item = Renderer>) -> Renderer {
        choices
            .into_iter()
            .find(|r| *r != Renderer::Auto)
            .unwrap_or_default()
    }
}

/// How translucent UI colours combine (`blending`): the UI renderers' setting, from web-ui.
pub use web_ui_core::Blending;

/// Which theme the game's UI matches with `prefers-color-scheme` (`theme` in `project.json`):
/// the system's light/dark preference, or one forced (docs/platform.md "System theme").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Theme {
    /// Follow the system (live, as it changes).
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    /// A value of `theme`.
    pub fn parse(s: &str) -> Option<Theme> {
        match s {
            "system" => Some(Theme::System),
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            _ => None,
        }
    }

    /// The value written in `project.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// Whether the UI is dark: forced, or asked of the system only when following it.
    pub fn is_dark(self, system_dark: impl FnOnce() -> bool) -> bool {
        match self {
            Theme::System => system_dark(),
            Theme::Light => false,
            Theme::Dark => true,
        }
    }
}

/// The note for a `project.json` that still has the old `type`.
pub const TYPE_REPLACED: &str =
    "`type` is replaced by `renderer`; \"app\" = \"gles\", \"game\" = \"vulkan\"";

/// A project with every template resolved and the platform section merged.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub name: String,
    /// `renderer`: `vulkan`, `gles` or `cpu`; left out (or unknown), chosen from what the program uses.
    pub renderer: Renderer,
    /// `blending`: `srgb` (the default) or `linear`.
    pub blending: Blending,
    /// `theme`: `system` (the default), `light` or `dark`.
    pub theme: Theme,
    pub title: String,
    /// `subtitle`: an optional second line under the title (a series' episode, say), shown by the
    /// video-settings prompt and `Application.subtitle`; `None` when unset or empty.
    pub subtitle: Option<String>,
    pub company: String,
    pub identifier: String,
    pub version: String,
    pub description: String,
    pub copyright: String,
    /// The executable's file name, no extension.
    pub exe: String,
    pub icon: Option<String>,
    /// `private` (all rights reserved), `custom` or an SPDX identifier.
    pub license: String,
    /// The license text file (`licenseFile`), relative to the project.
    pub license_file: Option<String>,
    pub start_scene: Option<String>,
    /// The entry file (`main`), relative to the project.
    pub main: Option<String>,
    /// The data folder beside a packaged exe (`dataDir`).
    pub data_dir: Option<String>,
    /// A command `platypus-build` runs in the game folder before it compiles (`prebuild`, as written:
    /// no `${..}` expansion, so a shell's own variables pass through), for sources a game generates
    /// (the md-reader's bundled docs).
    pub prebuild: Option<String>,
    /// Written by the packager: false when the exe runs without a data folder.
    pub data_required: Option<bool>,
    /// Files the packager takes: paths, folders or globs (`*`, `**`); default `["assets/**"]`.
    pub include: Option<Vec<String>>,
    /// Globs removed from what `include` and the inferred patterns matched.
    pub exclude: Option<Vec<String>>,
    pub fps: Option<bool>,
    pub run_in_background: bool,
    /// `videoSettingsPrompt`: ask the player for the display mode, resolution, vsync and quality
    /// once, before the game's window opens (`platypus-build package --video-settings-prompt`
    /// writes it into a packaged game's copy).
    pub video_settings_prompt: Option<bool>,
    pub layers: Value,
    pub quality: Value,
    pub display: Value,
    /// Keys this version does not know, kept as they are.
    pub extra: Map<String, Value>,
    /// Metadata fields (`title`, `company`, ...) the file sets to an empty string on purpose.
    pub empty_fields: Vec<String>,
    /// Warnings about fields that are no longer used or have an unknown value (`type`,
    /// `renderer`); `platypus-build` prints them, the host logs them.
    pub warnings: Vec<String>,
    /// Fields the file leaves out that took their default (`project.json` only).
    pub defaulted: Vec<String>,
    pub source: Source,
    /// The file the values came from.
    pub file: Option<PathBuf>,
}

/// How to resolve a project.
pub struct LoadOptions<'a> {
    pub platform: Platform,
    pub user: &'a dyn UserInfo,
    /// Literal values (no templates) that replace fields: command-line flags. Keys are field
    /// names (`title`, `company`, `exe`, ...).
    pub overrides: Vec<(String, String)>,
}

impl<'a> LoadOptions<'a> {
    pub fn new(platform: Platform, user: &'a dyn UserInfo) -> Self {
        Self {
            platform,
            user,
            overrides: Vec::new(),
        }
    }
}

/// Keys that are strings with templates, resolved on demand.
const TEXT_KEYS: &[&str] = &[
    "name",
    "title",
    "subtitle",
    "company",
    "identifier",
    "version",
    "description",
    "copyright",
    "exe",
    "icon",
    "startScene",
    "main",
    "dataDir",
    "license",
    "licenseFile",
];

/// Keys the loader gives a meaning; everything else goes to [`Project::extra`].
const KNOWN_KEYS: &[&str] = &[
    "name",
    "type",
    "renderer",
    "blending",
    "theme",
    "title",
    "subtitle",
    "company",
    "identifier",
    "version",
    "description",
    "copyright",
    "exe",
    "icon",
    "startScene",
    "main",
    "dataDir",
    "license",
    "licenseFile",
    "dataRequired",
    "prebuild",
    "include",
    "exclude",
    "fps",
    "runInBackground",
    "videoSettingsPrompt",
    "layers",
    "quality",
    "display",
    "resolved",
];

/// The accepted `license` values besides `private` and `custom` (SPDX identifiers).
pub const SPDX_LICENSES: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "GPL-2.0-only",
    "GPL-2.0-or-later",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "LGPL-2.1-only",
    "LGPL-3.0-only",
    "AGPL-3.0-only",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "MPL-2.0",
    "Unlicense",
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "Zlib",
    "BSL-1.0",
    "0BSD",
];

/// The canonical spelling of a license value (`private`, `custom` or an entry of
/// [`SPDX_LICENSES`]), matched without regard to case; `UNLICENSED` means `private`.
pub fn canonical_license(s: &str) -> Option<String> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("private") || t.eq_ignore_ascii_case("unlicensed") {
        return Some("private".into());
    }
    if t.eq_ignore_ascii_case("custom") {
        return Some("custom".into());
    }
    SPDX_LICENSES
        .iter()
        .find(|l| l.eq_ignore_ascii_case(t))
        .map(|l| (*l).to_owned())
}

/// The `license` of an npm `package.json` for a project license: `private` is `UNLICENSED`,
/// `custom` is `SEE LICENSE IN <file>`, an SPDX identifier stays.
pub fn npm_license(license: &str, license_file: Option<&str>) -> String {
    match license {
        "private" => "UNLICENSED".into(),
        "custom" => format!("SEE LICENSE IN {}", license_file.unwrap_or("LICENSE")),
        other => other.to_owned(),
    }
}

/// A name that is safe as a file name on every system.
pub fn file_safe(s: &str) -> String {
    let t: String = s
        .chars()
        .filter(|c| !"'\u{2019}\"".contains(*c))
        .map(|c| {
            if c.is_alphanumeric() || "-_ .".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let t = t.trim().trim_matches('.').to_string();
    if t.is_empty() { "game".into() } else { t }
}

/// A reverse-DNS identifier made of safe parts: each dot-separated part keeps letters, digits,
/// `-` and `_`; empty parts go; a part may not start with a digit.
pub fn sanitize_identifier(s: &str) -> String {
    let parts: Vec<String> = s
        .split('.')
        .map(|p| {
            let p: String = p
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if p.starts_with(|c: char| c.is_ascii_digit()) {
                format!("_{p}")
            } else {
                p
            }
        })
        .filter(|p| !p.is_empty())
        .collect();
    parts.join(".")
}

/// An error while resolving: where it happened (the field) and the text to look for in the file.
struct RErr {
    field: Option<String>,
    needle: Option<String>,
    message: String,
}

enum Raw {
    Template(String),
    Literal(String),
}

struct Resolver<'a> {
    raw: HashMap<String, Raw>,
    memo: HashMap<String, String>,
    stack: Vec<String>,
    user: &'a dyn UserInfo,
    pkg: PackageDefaults,
    folder: String,
    /// The `package.json` fallback (or no file): the old defaults, no required `name`.
    legacy: bool,
    defaulted: Vec<String>,
}

#[derive(Default)]
struct PackageDefaults {
    name: Option<String>,
    version: Option<String>,
    author: Option<String>,
    description: Option<String>,
}

/// The engine's section of a `package.json` (the deprecated fallback): `platypus`, else `iron`,
/// its name from before the rename to Platypus.
pub fn package_section(pkg: &Value) -> Option<&Map<String, Value>> {
    pkg.get("platypus")
        .or_else(|| pkg.get("iron"))
        .and_then(Value::as_object)
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Like [`text`], but an empty string is a value (a `project.json` field left empty on purpose).
fn text_or_empty(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        other => text(other),
    }
}

impl PackageDefaults {
    fn from(pkg: &Value) -> Self {
        Self {
            name: text(&pkg["name"]),
            version: text(&pkg["version"]),
            author: pkg["author"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            description: text(&pkg["description"]),
        }
    }
}

impl Resolver<'_> {
    fn value(&mut self, key: &str) -> Result<String, RErr> {
        if let Some(v) = self.memo.get(key) {
            return Ok(v.clone());
        }
        if self.stack.iter().any(|k| k == key) {
            let mut chain = self.stack.clone();
            chain.push(key.to_owned());
            return Err(RErr {
                field: Some(key.to_owned()),
                needle: Some(format!("${{{key}}}")),
                message: format!("circular reference: {}", chain.join(" -> ")),
            });
        }
        self.stack.push(key.to_owned());
        let r = self.compute(key);
        self.stack.pop();
        let mut v = r?;
        match key {
            "identifier" => v = sanitize_identifier(&v),
            "exe" => v = file_safe(v.trim_end_matches(".exe")),
            _ => {}
        }
        self.memo.insert(key.to_owned(), v.clone());
        Ok(v)
    }

    fn compute(&mut self, key: &str) -> Result<String, RErr> {
        match self.raw.get(key) {
            Some(Raw::Literal(s)) => return Ok(s.clone()),
            Some(Raw::Template(s)) => {
                let s = s.clone();
                return self.expand(&s).map_err(|mut e| {
                    e.field.get_or_insert_with(|| key.to_owned());
                    e
                });
            }
            None => {}
        }
        if !self.legacy {
            self.defaulted.push(key.to_owned());
            match key {
                "title" => return self.expand("${firstName}'s Game"),
                "company" => return self.expand("${firstName}'s Company"),
                "version" => return Ok("0.1.0".into()),
                _ => {}
            }
        }
        Ok(match key {
            "name" => self.pkg.name.clone().unwrap_or_else(|| self.folder.clone()),
            "title" => self.value("name")?,
            "license" => "private".to_owned(),
            "company" => self.pkg.author.clone().unwrap_or_default(),
            "version" => self.pkg.version.clone().unwrap_or_else(|| "0.0.0".into()),
            "identifier" => {
                let n = self.value("name")?;
                format!("dev.platypus.{}", sanitize_identifier(&n))
            }
            "description" => match self.pkg.description.clone() {
                Some(d) => d,
                None => self.value("title")?,
            },
            "copyright" => {
                let c = self.value("company")?;
                if c.is_empty() {
                    String::new()
                } else {
                    format!("Copyright (c) {c}")
                }
            }
            "exe" => self.value("title")?,
            _ => String::new(),
        })
    }

    fn var(&mut self, name: &str) -> Result<String, RErr> {
        let (first, last) = split_name(&self.full_name());
        Ok(match name {
            "username" => self.user.username(),
            "fullName" => self.full_name(),
            "firstName" => first,
            "lastName" => last,
            "year" => self.user.today().0.to_string(),
            "date" => {
                let (y, m, d) = self.user.today();
                format!("{y:04}-{m:02}-{d:02}")
            }
            "name" | "title" | "company" | "version" | "identifier" | "description"
            | "copyright" | "exe" => self.value(name)?,
            n if n.starts_with("env:") => {
                let spec = &n[4..];
                let (var, fallback) = match spec.split_once(":-") {
                    Some((v, f)) => (v, Some(f)),
                    None => (spec, None),
                };
                match self.user.env(var.trim()) {
                    Some(v) => v,
                    None => match fallback {
                        Some(f) => f.to_owned(),
                        None => {
                            return Err(RErr {
                                field: None,
                                needle: Some(format!("${{{n}}}")),
                                message: format!(
                                    "environment variable `{}` is not set (use `${{env:{}:-default}}` for a default)",
                                    var.trim(),
                                    var.trim()
                                ),
                            });
                        }
                    },
                }
            }
            _ => {
                return Err(RErr {
                    field: None,
                    needle: Some(format!("${{{name}}}")),
                    message: format!(
                        "unknown variable `${{{name}}}` (known: username, firstName, lastName, fullName, year, date, name, title, company, version, identifier, description, copyright, exe, env:NAME)"
                    ),
                });
            }
        })
    }

    fn full_name(&self) -> String {
        let full = self.user.full_name();
        let full = full.trim();
        if full.is_empty() {
            self.user.username()
        } else {
            full.to_owned()
        }
    }

    /// Replaces `${..}` in `s`; `$${` is a literal `${`.
    fn expand(&mut self, s: &str) -> Result<String, RErr> {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(i) = rest.find('$') {
            out.push_str(&rest[..i]);
            rest = &rest[i..];
            if let Some(after) = rest.strip_prefix("$${") {
                out.push_str("${");
                rest = after;
            } else if let Some(after) = rest.strip_prefix("${") {
                let Some(end) = after.find('}') else {
                    return Err(RErr {
                        field: None,
                        needle: Some(rest.chars().take(24).collect()),
                        message: "`${` without a closing `}` (write `$${` for a literal)".into(),
                    });
                };
                let name = after[..end].trim();
                if name.is_empty() {
                    return Err(RErr {
                        field: None,
                        needle: Some("${}".into()),
                        message: "empty variable `${}`".into(),
                    });
                }
                out.push_str(&self.var(name)?);
                rest = &after[end + 1..];
            } else {
                out.push('$');
                rest = &rest[1..];
            }
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Expands every string inside a JSON value.
    fn expand_value(&mut self, v: &Value, templates: bool) -> Result<Value, RErr> {
        Ok(match v {
            Value::String(s) if templates => Value::String(self.expand(s)?),
            Value::Array(a) => Value::Array(
                a.iter()
                    .map(|x| self.expand_value(x, templates))
                    .collect::<Result<_, _>>()?,
            ),
            Value::Object(o) => {
                let mut m = Map::new();
                for (k, x) in o {
                    m.insert(k.clone(), self.expand_value(x, templates)?);
                }
                Value::Object(m)
            }
            other => other.clone(),
        })
    }
}

/// Removes leading `//` comment lines (their lines stay, empty, so positions hold).
fn strip_leading_comments(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut in_header = true;
    for line in t.split_inclusive('\n') {
        if in_header && (line.trim_start().starts_with("//") || line.trim().is_empty()) {
            out.push_str(if line.ends_with('\n') { "\n" } else { "" });
        } else {
            in_header = false;
            out.push_str(line);
        }
    }
    out
}

/// 1-based line and column of the first `needle` in `text`.
fn locate(text: &str, needle: &str) -> (usize, usize) {
    match text.find(needle) {
        Some(i) => {
            let before = &text[..i];
            let line = before.matches('\n').count() + 1;
            let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            (line, col)
        }
        None => (0, 0),
    }
}

impl Project {
    /// The project of a folder with no files: all defaults.
    pub fn defaults(folder: &str) -> Project {
        Self::from_package_json(&Value::Null, folder)
    }

    /// A project from a `package.json` value (its `platypus` section and top-level fields), no templates.
    pub fn from_package_json(pkg: &Value, folder: &str) -> Project {
        let user = FixedUser::default();
        Self::from_package_json_with(pkg, folder, &LoadOptions::new(Platform::current(), &user))
    }

    /// [`from_package_json`](Self::from_package_json) with the overrides of `opts`.
    pub fn from_package_json_with(pkg: &Value, folder: &str, opts: &LoadOptions<'_>) -> Project {
        let doc = package_section(pkg).cloned().unwrap_or_default();
        let source = if pkg.is_object() {
            Source::PackageJson
        } else {
            Source::Defaults
        };
        // Without templates nothing can fail.
        build(doc, pkg, folder, false, opts, source, None, "").unwrap_or_else(|e| panic!("{e}"))
    }

    /// Reads the project in `dir`: `project.json`, else `package.json`, else defaults.
    pub fn load(dir: &Path, opts: &LoadOptions<'_>) -> Result<Project, ProjectError> {
        let absolute = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let folder = absolute
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "game".into());
        let path = dir.join(PROJECT_FILE);
        let file = path.display().to_string();
        let Ok(text) = std::fs::read_to_string(&path) else {
            // Deprecated fallback: the `platypus` section of `package.json`.
            let pkg_path = dir.join("package.json");
            let pkg_text = std::fs::read_to_string(&pkg_path).ok();
            let pkg: Value = match &pkg_text {
                Some(t) => serde_json::from_str(t).map_err(|e| ProjectError {
                    file: pkg_path.display().to_string(),
                    line: e.line(),
                    col: e.column(),
                    message: e.to_string(),
                })?,
                None => Value::Null,
            };
            let (doc, source, f) = match &pkg_text {
                Some(_) => (
                    package_section(&pkg).cloned().unwrap_or_default(),
                    Source::PackageJson,
                    Some(pkg_path),
                ),
                None => (Map::new(), Source::Defaults, None),
            };
            return build(doc, &pkg, &folder, false, opts, source, f, "");
        };
        let value: Value =
            serde_json::from_str(&strip_leading_comments(&text)).map_err(|e| ProjectError {
                file: file.clone(),
                line: e.line(),
                col: e.column(),
                message: e.to_string(),
            })?;
        let Value::Object(doc) = value else {
            return Err(ProjectError {
                file,
                line: 1,
                col: 1,
                message: "a project is a JSON object".into(),
            });
        };
        let resolved = doc.get("resolved") == Some(&Value::Bool(true));
        build(
            doc,
            &Value::Null,
            &folder,
            !resolved,
            opts,
            Source::ProjectJson,
            Some(path),
            &text,
        )
    }

    fn file_label(&self) -> String {
        self.file
            .as_ref()
            .map_or_else(|| PROJECT_FILE.to_owned(), |f| f.display().to_string())
    }

    /// One warning per metadata field the file sets to an empty string on purpose
    /// (`project.json: "company" is empty (...)`). `build` and `package` print them, `check` does not.
    pub fn empty_field_warnings(&self) -> Vec<String> {
        let file = self.file_label();
        self.empty_fields
            .iter()
            .map(|k| {
                let effect = match k.as_str() {
                    "title" => "the window title will be empty",
                    "identifier" => "the app identifier will be empty",
                    "company" => "the exe's version info will have no company",
                    "version" => "the exe's version info will have no version",
                    "description" => "the exe's version info will have no description",
                    "copyright" => "the exe's version info will have no copyright",
                    _ => "the exe's version info will have no license",
                };
                format!("{file}: \"{k}\" is empty ({effect})")
            })
            .collect()
    }

    /// One warning for each of `company`, `copyright` and `license` the file leaves out, so they
    /// still hold their defaults: a game about to be released should say who made it.
    pub fn default_field_warnings(&self) -> Vec<String> {
        let file = self.file_label();
        ["company", "copyright", "license"]
            .into_iter()
            .filter(|k| self.defaulted.iter().any(|d| d == k))
            .map(|k| {
                let v = match k {
                    "company" => &self.company,
                    "copyright" => &self.copyright,
                    _ => &self.license,
                };
                format!("{file}: \"{k}\" is not set (using the default \"{v}\")")
            })
            .collect()
    }

    /// The resolved project as a `project.json` (marked `"resolved": true`: read back as it is).
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("resolved".into(), true.into());
        for (k, v) in [
            ("name", &self.name),
            ("title", &self.title),
            ("company", &self.company),
            ("identifier", &self.identifier),
            ("version", &self.version),
            ("description", &self.description),
            ("copyright", &self.copyright),
            ("exe", &self.exe),
            ("license", &self.license),
        ] {
            m.insert(k.into(), v.clone().into());
        }
        for (k, v) in [
            ("subtitle", &self.subtitle),
            ("icon", &self.icon),
            ("licenseFile", &self.license_file),
            ("startScene", &self.start_scene),
            ("main", &self.main),
            ("dataDir", &self.data_dir),
            ("prebuild", &self.prebuild),
        ] {
            if let Some(v) = v {
                m.insert(k.into(), v.clone().into());
            }
        }
        if self.renderer != Renderer::Auto {
            m.insert("renderer".into(), self.renderer.as_str().into());
        }
        if self.blending != Blending::Srgb {
            m.insert("blending".into(), self.blending.as_str().into());
        }
        if self.theme != Theme::System {
            m.insert("theme".into(), self.theme.as_str().into());
        }
        if let Some(r) = self.data_required {
            m.insert("dataRequired".into(), r.into());
        }
        if let Some(i) = &self.include {
            m.insert("include".into(), i.clone().into());
        }
        if let Some(i) = &self.exclude {
            m.insert("exclude".into(), i.clone().into());
        }
        if let Some(f) = self.fps {
            m.insert("fps".into(), f.into());
        }
        if self.run_in_background {
            m.insert("runInBackground".into(), true.into());
        }
        if let Some(v) = self.video_settings_prompt {
            m.insert("videoSettingsPrompt".into(), v.into());
        }
        for (k, v) in [
            ("layers", &self.layers),
            ("quality", &self.quality),
            ("display", &self.display),
        ] {
            if !v.is_null() {
                m.insert(k.into(), v.clone());
            }
        }
        for (k, v) in &self.extra {
            m.insert(k.clone(), v.clone());
        }
        Value::Object(m)
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut doc: Map<String, Value>,
    pkg: &Value,
    folder: &str,
    templates: bool,
    opts: &LoadOptions<'_>,
    source: Source,
    file: Option<PathBuf>,
    file_text: &str,
) -> Result<Project, ProjectError> {
    // Platform sections: the one for this platform overrides, all of them leave the document.
    let mut section = None;
    for p in Platform::ALL {
        if let Some(Value::Object(o)) = doc.remove(p.name())
            && p == opts.platform
            && templates
        {
            section = Some(o);
        }
    }
    if let Some(o) = section {
        for (k, v) in o {
            doc.insert(k, v);
        }
    }

    let mut r = Resolver {
        raw: HashMap::new(),
        memo: HashMap::new(),
        stack: Vec::new(),
        user: opts.user,
        pkg: PackageDefaults::from(pkg),
        folder: folder.to_owned(),
        legacy: source != Source::ProjectJson,
        defaulted: Vec::new(),
    };
    let from_file = source == Source::ProjectJson;
    for k in TEXT_KEYS {
        let v = doc
            .get(*k)
            .and_then(|v| if from_file { text_or_empty(v) } else { text(v) });
        if let Some(s) = v {
            r.raw.insert(
                (*k).to_owned(),
                if templates {
                    Raw::Template(s)
                } else {
                    Raw::Literal(s)
                },
            );
        }
    }
    for (k, v) in &opts.overrides {
        r.raw.insert(k.clone(), Raw::Literal(v.clone()));
    }

    let name = file
        .as_ref()
        .map(|f| f.display().to_string())
        .unwrap_or_else(|| PROJECT_FILE.to_owned());
    let fail = |e: RErr| {
        let (line, col) = e.needle.as_deref().map_or((0, 0), |n| locate(file_text, n));
        ProjectError {
            file: name.clone(),
            line,
            col,
            message: match e.field {
                Some(f) => format!("`{f}`: {}", e.message),
                None => e.message,
            },
        }
    };

    if from_file
        && r.raw.get("name").is_none_or(|n| match n {
            Raw::Template(s) | Raw::Literal(s) => s.trim().is_empty(),
        })
    {
        return Err(ProjectError {
            file: name.clone(),
            line: 0,
            col: 0,
            message: "`name` is required (the only required field), e.g. { \"name\": \"mygame\" }"
                .into(),
        });
    }
    let get = |r: &mut Resolver<'_>, k: &str| r.value(k).map_err(fail);
    let opt = |s: String| if s.is_empty() { None } else { Some(s) };
    let project_name = get(&mut r, "name")?;
    let title = get(&mut r, "title")?;
    let company = get(&mut r, "company")?;
    let identifier = get(&mut r, "identifier")?;
    let version = get(&mut r, "version")?;
    let description = get(&mut r, "description")?;
    let copyright = get(&mut r, "copyright")?;
    let exe = get(&mut r, "exe")?;
    let license_raw = get(&mut r, "license")?;
    let license = if license_raw.is_empty() && from_file {
        String::new()
    } else {
        canonical_license(&license_raw).ok_or_else(|| {
        let (line, col) = locate(file_text, &license_raw);
        ProjectError {
            file: name.clone(),
            line,
            col,
            message: format!(
                "`license`: unknown license `{license_raw}` (use `private`, `custom` or one of: {})",
                SPDX_LICENSES.join(", ")
            ),
        }
    })?
    };
    let empty_fields: Vec<String> = [
        ("title", &title),
        ("company", &company),
        ("identifier", &identifier),
        ("version", &version),
        ("description", &description),
        ("copyright", &copyright),
        ("license", &license),
    ]
    .into_iter()
    .filter(|(k, v)| from_file && v.is_empty() && r.raw.contains_key(*k))
    .map(|(k, _)| k.to_owned())
    .collect();
    let defaulted = r.defaulted.clone();
    let icon = if r.raw.contains_key("icon") {
        opt(get(&mut r, "icon")?)
    } else {
        None
    };
    let optional = |r: &mut Resolver<'_>, k: &str| -> Result<Option<String>, ProjectError> {
        if r.raw.contains_key(k) {
            Ok(opt(r.value(k).map_err(fail)?))
        } else {
            Ok(None)
        }
    };
    let license_file = optional(&mut r, "licenseFile")?;
    let start_scene = optional(&mut r, "startScene")?;
    let subtitle = optional(&mut r, "subtitle")?;
    let main = optional(&mut r, "main")?;
    let data_dir = optional(&mut r, "dataDir")?;

    let passthrough = |r: &mut Resolver<'_>, k: &str| -> Result<Value, ProjectError> {
        match doc.get(k) {
            Some(v) => r.expand_value(v, templates).map_err(fail),
            None => Ok(Value::Null),
        }
    };
    let layers = passthrough(&mut r, "layers")?;
    let quality = passthrough(&mut r, "quality")?;
    let display = passthrough(&mut r, "display")?;
    let strings = |v: Value| match v {
        Value::Array(a) => Some(
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect::<Vec<_>>(),
        ),
        _ => None,
    };
    let mut warnings = Vec::new();
    if doc.contains_key("type") {
        warnings.push(TYPE_REPLACED.to_string());
    }
    let renderer = match doc.get("renderer") {
        None => Renderer::Auto,
        Some(v) => match v.as_str().and_then(Renderer::parse) {
            Some(r) => r,
            None => {
                warnings.push(format!(
                    "`renderer`: expected \"vulkan\", \"gles\" or \"cpu\", not {v} (chosen automatically)"
                ));
                Renderer::Auto
            }
        },
    };
    let blending = match doc.get("blending") {
        None => Blending::Srgb,
        Some(v) => match v.as_str().and_then(Blending::parse) {
            Some(b) => b,
            None => {
                warnings.push(format!(
                    "`blending`: expected \"srgb\" or \"linear\", not {v} (srgb is used)"
                ));
                Blending::Srgb
            }
        },
    };
    let theme = match doc.get("theme") {
        None => Theme::System,
        Some(v) => match v.as_str().and_then(Theme::parse) {
            Some(t) => t,
            None => {
                warnings.push(format!(
                    "`theme`: expected \"system\", \"light\" or \"dark\", not {v} (system is used)"
                ));
                Theme::System
            }
        },
    };
    let include = strings(passthrough(&mut r, "include")?);
    let exclude = strings(passthrough(&mut r, "exclude")?);
    let mut extra = Map::new();
    for (k, v) in &doc {
        if !KNOWN_KEYS.contains(&k.as_str()) {
            extra.insert(k.clone(), r.expand_value(v, templates).map_err(fail)?);
        }
    }
    Ok(Project {
        name: project_name,
        renderer,
        blending,
        theme,
        title,
        subtitle,
        company,
        identifier,
        version,
        description,
        copyright,
        exe,
        icon,
        license,
        license_file,
        start_scene,
        main,
        data_dir,
        data_required: doc.get("dataRequired").and_then(Value::as_bool),
        prebuild: doc
            .get("prebuild")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned),
        include,
        exclude,
        fps: doc.get("fps").and_then(Value::as_bool),
        run_in_background: doc
            .get("runInBackground")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        video_settings_prompt: doc.get("videoSettingsPrompt").and_then(Value::as_bool),
        layers,
        quality,
        display,
        extra,
        empty_fields,
        warnings,
        defaulted,
        source,
        file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> FixedUser {
        FixedUser {
            username: "mattb".into(),
            full_name: "Matthew Brown".into(),
            date: (2026, 10, 21),
            env: HashMap::from([("GAME_CODE".to_owned(), "zx81".to_owned())]),
        }
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "platypus_core_project_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn load_text(
        tag: &str,
        text: &str,
        u: &FixedUser,
        platform: Platform,
    ) -> Result<Project, ProjectError> {
        let d = temp(tag);
        std::fs::write(d.join(PROJECT_FILE), text).unwrap();
        Project::load(&d, &LoadOptions::new(platform, u))
    }

    fn load_ok(tag: &str, text: &str) -> Project {
        load_text(tag, text, &user(), Platform::Windows).unwrap()
    }

    // M9-API-1300
    #[test]
    fn every_variable_resolves() {
        let p = load_ok(
            "vars",
            r#"{ "name": "fox", "version": "1.2.3", "company": "${fullName}",
                 "title": "${firstName}'s ${lastName} game ${username} ${year} ${date}",
                 "description": "${name}|${title}|${company}|${version}|${env:GAME_CODE}" }"#,
        );
        assert_eq!(p.title, "Matthew's Brown game mattb 2026 2026-10-21");
        assert_eq!(p.company, "Matthew Brown");
        assert_eq!(
            p.description,
            "fox|Matthew's Brown game mattb 2026 2026-10-21|Matthew Brown|1.2.3|zx81"
        );
    }

    // M9-VSP-13
    #[test]
    fn the_subtitle_is_optional_takes_templates_and_an_empty_one_is_none() {
        let p = load_ok(
            "subtitle",
            r#"{ "name": "g", "title": "Game Title", "subtitle": "${name}: Series 2" }"#,
        );
        assert_eq!(p.subtitle.as_deref(), Some("g: Series 2"));
        assert_eq!(p.to_json()["subtitle"], "g: Series 2");
        let none = load_ok("subtitle_none", r#"{ "name": "g" }"#);
        assert_eq!(none.subtitle, None);
        assert!(none.to_json().get("subtitle").is_none());
        assert!(!none.defaulted.contains(&"subtitle".to_string()));
        let empty = load_ok("subtitle_empty", r#"{ "name": "g", "subtitle": "" }"#);
        assert_eq!(empty.subtitle, None);
        assert!(
            empty.empty_field_warnings().is_empty(),
            "no warning: it is optional"
        );
    }

    // M9-API-1301
    #[test]
    fn dollar_dollar_brace_is_a_literal_and_a_lone_dollar_stays() {
        let p = load_ok(
            "esc",
            r#"{ "name": "g", "title": "$${name} costs $5 and $${a}" }"#,
        );
        assert_eq!(p.title, "${name} costs $5 and ${a}");
    }

    // M9-API-1302
    #[test]
    fn an_unknown_variable_is_an_error_naming_the_file_and_line() {
        let e = load_text(
            "unk",
            "{\n  \"name\": \"g\",\n  \"title\": \"${firstNam}'s Game\"\n}\n",
            &user(),
            Platform::Windows,
        )
        .unwrap_err();
        assert_eq!(e.line, 3);
        assert_eq!(e.col, 13);
        assert!(e.file.ends_with("project.json"), "{e}");
        assert!(e.message.contains("unknown variable `${firstNam}`"), "{e}");
        assert!(e.message.contains("`title`"), "{e}");
        let shown = e.to_string();
        assert!(shown.contains("project.json:3:13:"), "{shown}");
    }

    // M9-API-1303
    #[test]
    fn nested_variables_resolve_through_other_fields_and_cycles_are_errors() {
        let p = load_ok(
            "nest",
            r#"{ "name": "fox", "company": "${fullName}", "copyright": "Copyright (c) ${year} ${company}",
                 "title": "${name} by ${company}", "description": "${title}" }"#,
        );
        assert_eq!(p.copyright, "Copyright (c) 2026 Matthew Brown");
        assert_eq!(p.description, "fox by Matthew Brown");
        let e = load_text(
            "cycle",
            r#"{ "name": "g", "title": "${description}", "description": "${title}" }"#,
            &user(),
            Platform::Windows,
        )
        .unwrap_err();
        assert!(e.message.contains("circular reference"), "{e}");
    }

    // M9-API-1304
    #[test]
    fn env_variables_have_a_fallback_and_a_missing_one_is_an_error() {
        let p = load_ok(
            "env",
            r#"{ "name": "g", "title": "${env:NOPE_X:-plain}-${env:GAME_CODE}" }"#,
        );
        assert_eq!(p.title, "plain-zx81");
        let e = load_text(
            "env2",
            r#"{ "name": "g", "title": "${env:NOPE_X}" }"#,
            &user(),
            Platform::Windows,
        )
        .unwrap_err();
        assert!(e.message.contains("NOPE_X"), "{e}");
        assert_eq!(e.line, 1);
    }

    // M9-API-1305
    #[test]
    fn syntax_problems_in_a_template_are_errors() {
        for bad in [
            r#"{ "name": "g", "title": "a ${b" }"#,
            r#"{ "name": "g", "title": "a ${} b" }"#,
        ] {
            assert!(
                load_text("syn", bad, &user(), Platform::Windows).is_err(),
                "{bad}"
            );
        }
        let e = load_text("json", "{ \"title\": }", &user(), Platform::Windows).unwrap_err();
        assert_eq!(e.line, 1);
    }

    // M9-API-1306
    #[test]
    fn names_split_into_first_and_last() {
        let cases = [
            ("", ("", "")),
            ("Matthew", ("Matthew", "")),
            ("Matthew Brown", ("Matthew", "Brown")),
            ("Matthew James Brown", ("Matthew", "Brown")),
            ("  Zo\u{eb}   O'Neil-Smith ", ("Zo\u{eb}", "O'Neil-Smith")),
            (
                "\u{5c71}\u{7530} \u{592a}\u{90ce}",
                ("\u{5c71}\u{7530}", "\u{592a}\u{90ce}"),
            ),
        ];
        for (full, (f, l)) in cases {
            assert_eq!(split_name(full), (f.to_owned(), l.to_owned()), "{full:?}");
        }
    }

    // M9-API-1307
    #[test]
    fn a_missing_full_name_falls_back_to_the_account_name_and_one_word_has_no_last_name() {
        let mut u = user();
        u.full_name = String::new();
        let p = load_text(
            "noname",
            r#"{ "name": "g", "title": "${firstName}'s Game", "company": "${fullName}", "description": "[${lastName}]" }"#,
            &u,
            Platform::Linux,
        )
        .unwrap();
        assert_eq!(
            (p.title.as_str(), p.company.as_str(), p.description.as_str()),
            ("mattb's Game", "mattb", "[]")
        );
        assert_eq!(
            gecos_name("matt:x:1000:1000:Matthew Brown,Room 1,,:/home/matt:/bin/sh\n"),
            "Matthew Brown"
        );
        assert_eq!(gecos_name("matt:x:1000:1000::/home/matt:/bin/sh"), "");
        assert_eq!(gecos_name(""), "");
    }

    // M9-API-1308
    #[test]
    fn the_identifier_is_sanitised() {
        let mut u = user();
        u.username = "9 Matt B!".into();
        let p = load_text(
            "ident",
            r#"{ "name": "Fox Run", "identifier": "com.${username}.${name}" }"#,
            &u,
            Platform::Windows,
        )
        .unwrap();
        assert_eq!(p.identifier, "com._9MattB.FoxRun");
        assert_eq!(sanitize_identifier("..a..b."), "a.b");
        assert_eq!(
            load_ok("ident2", r#"{ "name": "g" }"#).identifier,
            "dev.platypus.g"
        );
    }

    // M9-API-1309
    #[test]
    fn the_platform_section_overrides_and_the_others_vanish() {
        let text = r#"{ "name": "g", "title": "T", "icon": "icon.png",
            "windows": { "icon": "icon.ico", "title": "${firstName} on Windows" },
            "linux": { "icon": "icon.svg" }, "macos": { "title": "Mac" } }"#;
        let w = load_text("pw", text, &user(), Platform::Windows).unwrap();
        assert_eq!(
            (w.title.as_str(), w.icon.as_deref()),
            ("Matthew on Windows", Some("icon.ico"))
        );
        let l = load_text("pl", text, &user(), Platform::Linux).unwrap();
        assert_eq!(
            (l.title.as_str(), l.icon.as_deref()),
            ("T", Some("icon.svg"))
        );
        let a = load_text("pa", text, &user(), Platform::Android).unwrap();
        assert_eq!(
            (a.title.as_str(), a.icon.as_deref()),
            ("T", Some("icon.png"))
        );
        assert!(!a.extra.contains_key("windows") && !a.extra.contains_key("macos"));
        assert_eq!(Platform::parse("ios"), Some(Platform::Ios));
        assert_eq!(Platform::parse("dos"), None);
    }

    // M9-API-1310
    #[test]
    fn without_a_project_file_package_json_is_used_without_templates() {
        let d = temp("legacy");
        std::fs::write(
            d.join("package.json"),
            r#"{ "name": "old", "version": "0.4.0", "author": "Acme", "description": "d",
                 "platypus": { "title": "Old ${firstName}", "startScene": "s.json", "layers": {"8": "A"},
                           "quality": {"default": "High"}, "fps": false, "runInBackground": true,
                           "videoSettingsPrompt": true } }"#,
        )
        .unwrap();
        let u = user();
        let p = Project::load(&d, &LoadOptions::new(Platform::Windows, &u)).unwrap();
        assert_eq!(p.source, Source::PackageJson);
        assert_eq!(p.title, "Old ${firstName}");
        assert_eq!(
            (
                p.name.as_str(),
                p.version.as_str(),
                p.company.as_str(),
                p.description.as_str()
            ),
            ("old", "0.4.0", "Acme", "d")
        );
        assert_eq!(p.copyright, "Copyright (c) Acme");
        assert_eq!(p.start_scene.as_deref(), Some("s.json"));
        assert_eq!(p.layers["8"], "A");
        assert_eq!(p.quality["default"], "High");
        assert_eq!((p.fps, p.run_in_background), (Some(false), true));
        assert_eq!(p.video_settings_prompt, Some(true));
        assert_eq!(p.to_json()["videoSettingsPrompt"], true);
        // No files at all: defaults named after the folder.
        let e = temp("empty");
        let p = Project::load(&e, &LoadOptions::new(Platform::Windows, &u)).unwrap();
        assert_eq!(p.source, Source::Defaults);
        assert!(p.name.starts_with("platypus_core_project_empty_") && p.title == p.name);
    }

    // M9-REN-02
    #[test]
    fn a_package_json_iron_section_from_before_the_rename_is_read() {
        let d = temp("iron_section");
        std::fs::write(
            d.join("package.json"),
            r#"{ "name": "old", "iron": { "title": "Old game" } }"#,
        )
        .unwrap();
        let u = user();
        let p = Project::load(&d, &LoadOptions::new(Platform::Windows, &u)).unwrap();
        assert_eq!(
            (p.source, p.title.as_str()),
            (Source::PackageJson, "Old game")
        );
        let both = serde_json::json!({ "iron": { "title": "a" }, "platypus": { "title": "b" } });
        assert_eq!(package_section(&both).unwrap()["title"], "b");
    }

    // M9-API-1311
    #[test]
    fn package_json_is_ignored_when_a_project_json_exists() {
        let d = temp("both");
        std::fs::write(
            d.join("package.json"),
            r#"{ "name": "npmname", "version": "3.0.0", "author": "Npm", "platypus": { "title": "FROM PACKAGE" } }"#,
        )
        .unwrap();
        std::fs::write(
            d.join(PROJECT_FILE),
            r#"{ "name": "g", "title": "${name} game" }"#,
        )
        .unwrap();
        let u = user();
        let p = Project::load(&d, &LoadOptions::new(Platform::Windows, &u)).unwrap();
        assert_eq!(
            (
                p.title.as_str(),
                p.version.as_str(),
                p.company.as_str(),
                p.source
            ),
            ("g game", "0.1.0", "Matthew's Company", Source::ProjectJson)
        );
        // A broken package.json does not matter either.
        std::fs::write(d.join("package.json"), "{ not json").unwrap();
        assert!(Project::load(&d, &LoadOptions::new(Platform::Windows, &u)).is_ok());
    }

    // M9-API-1312
    #[test]
    fn overrides_are_literal_and_other_fields_see_them() {
        let d = temp("ov");
        std::fs::write(
            d.join(PROJECT_FILE),
            r#"{ "name": "g", "title": "T", "description": "about ${title}" }"#,
        )
        .unwrap();
        let u = user();
        let mut o = LoadOptions::new(Platform::Windows, &u);
        o.overrides
            .push(("title".into(), "${not a template}".into()));
        let p = Project::load(&d, &o).unwrap();
        assert_eq!(
            (p.title.as_str(), p.description.as_str()),
            ("${not a template}", "about ${not a template}")
        );
    }

    // M9-API-1313
    #[test]
    fn a_resolved_project_round_trips_without_templates_and_keeps_unknown_keys() {
        let p = load_ok(
            "rt",
            r#"{ "// c": 1, "name": "g", "title": "$${raw} ${firstName}", "startScene": "a.scene.json",
                 "layers": {"8": "X${username}"}, "include": ["assets", "extra"], "exclude": ["**/*.psd"], "future": {"k": "${username}"},
                 "windows": {"fps": false} }"#,
        );
        assert_eq!(p.title, "${raw} Matthew");
        assert_eq!(p.fps, Some(false));
        let j = p.to_json();
        assert_eq!(j["resolved"], true);
        assert_eq!(j["future"]["k"], "mattb");
        assert!(j.get("windows").is_none());
        let d = temp("rt2");
        std::fs::write(d.join(PROJECT_FILE), j.to_string()).unwrap();
        let u = FixedUser::default();
        let q = Project::load(&d, &LoadOptions::new(Platform::Linux, &u)).unwrap();
        assert_eq!(q.title, "${raw} Matthew");
        assert_eq!(q.layers["8"], "Xmattb");
        assert_eq!(
            q.include,
            Some(vec!["assets".to_owned(), "extra".to_owned()])
        );
        assert_eq!(q.exclude, Some(vec!["**/*.psd".to_owned()]));
        assert_eq!(q.to_json(), j);
    }

    // M9-API-1314
    #[test]
    fn omitted_fields_take_their_defaults_silently() {
        let p = load_ok("def", r#"{ "name": "my game" }"#);
        assert_eq!(p.title, "Matthew's Game");
        assert_eq!(p.exe, "Matthews Game");
        assert_eq!(
            (p.version.as_str(), p.company.as_str(), p.license.as_str()),
            ("0.1.0", "Matthew's Company", "private")
        );
        assert_eq!(p.copyright, "Copyright (c) Matthew's Company");
        assert_eq!(p.identifier, "dev.platypus.mygame");
        assert_eq!(p.description, "Matthew's Game");
        assert!(p.empty_fields.is_empty() && p.empty_field_warnings().is_empty());
        assert!(p.defaulted.contains(&"company".to_owned()));
        let w = p.default_field_warnings();
        assert_eq!(w.len(), 3, "{w:?}");
        assert!(
            w[0].contains("project.json: \"company\" is not set"),
            "{w:?}"
        );
        let q = load_ok(
            "def2",
            r#"{ "name": "g", "title": "Matthew's Game", "company": "Q", "license": "MIT" }"#,
        );
        assert_eq!(q.exe, "Matthews Game");
        assert_eq!(q.copyright, "Copyright (c) Q");
        let w = q.default_field_warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("\"copyright\""), "{w:?}");
    }

    // M9-API-1321
    #[test]
    fn name_is_required_and_the_error_names_the_file() {
        for text in [
            r#"{ "title": "T" }"#,
            r#"{ "name": "", "title": "T" }"#,
            "{}",
        ] {
            let e = load_text("noname_req", text, &user(), Platform::Windows).unwrap_err();
            assert!(e.file.ends_with("project.json"), "{e}");
            assert!(e.message.contains("`name` is required"), "{e}");
        }
        // A command-line override counts.
        let d = temp("noname_ov");
        std::fs::write(d.join(PROJECT_FILE), "{}").unwrap();
        let u = user();
        let mut o = LoadOptions::new(Platform::Windows, &u);
        o.overrides.push(("name".into(), "x".into()));
        assert_eq!(Project::load(&d, &o).unwrap().name, "x");
    }

    // M9-API-1322
    #[test]
    fn an_empty_string_stays_empty_and_is_listed() {
        let p = load_ok(
            "emptyf",
            r#"{ "name": "g", "company": "", "license": "", "copyright": "", "description": "" }"#,
        );
        assert_eq!(
            (
                p.company.as_str(),
                p.license.as_str(),
                p.copyright.as_str(),
                p.description.as_str()
            ),
            ("", "", "", "")
        );
        assert_eq!(
            p.empty_fields,
            ["company", "description", "copyright", "license"]
        );
        let w = p.empty_field_warnings();
        assert!(
            w.iter().any(|l| l.ends_with(
                "project.json: \"company\" is empty (the exe's version info will have no company)"
            )),
            "{w:?}"
        );
        assert_eq!(w.len(), 4);
        // Not defaults, so no default warning for them either.
        assert!(p.default_field_warnings().is_empty());
        // The empty values round-trip through the resolved project.json.
        let d = temp("emptyf2");
        std::fs::write(d.join(PROJECT_FILE), p.to_json().to_string()).unwrap();
        let u = FixedUser::default();
        let q = Project::load(&d, &LoadOptions::new(Platform::Linux, &u)).unwrap();
        assert_eq!((q.company.as_str(), q.license.as_str()), ("", ""));
    }

    // M9-API-1315
    #[test]
    fn a_leading_comment_line_is_allowed_and_keeps_line_numbers() {
        let e = load_text(
            "cmt",
            "// my game\n// more\n{\n \"name\": \"g\", \"title\": \"${bad}\"\n}",
            &user(),
            Platform::Windows,
        )
        .unwrap_err();
        assert_eq!(e.line, 4);
        assert_eq!(
            load_ok("cmt2", "// hi\n{ \"name\": \"g\", \"title\": \"ok\" }").title,
            "ok"
        );
    }

    // M9-API-1316
    #[test]
    fn dates_are_civil() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_513), (2026, 3, 1));
        let (y, m, d) = SystemUser.today();
        assert!(y >= 2025 && (1..=12).contains(&m) && (1..=31).contains(&d));
    }

    // M9-API-1318
    #[test]
    fn license_defaults_to_private_is_validated_and_maps_to_npm() {
        assert_eq!(load_ok("lic0", r#"{ "name": "g" }"#).license, "private");
        assert_eq!(
            load_ok("lic1", r#"{ "name": "g", "license": "mit" }"#).license,
            "MIT"
        );
        assert_eq!(
            load_ok("lic2", r#"{ "name": "g", "license": "UNLICENSED" }"#).license,
            "private"
        );
        let c = load_ok(
            "lic3",
            r#"{ "name": "g", "license": "custom", "licenseFile": "EULA.txt" }"#,
        );
        assert_eq!(
            (c.license.as_str(), c.license_file.as_deref()),
            ("custom", Some("EULA.txt"))
        );
        assert_eq!(c.to_json()["licenseFile"], "EULA.txt");
        let e = load_text(
            "lic4",
            "{
 \"name\": \"g\",
 \"license\": \"WTFPL-9\"
}",
            &user(),
            Platform::Windows,
        )
        .unwrap_err();
        assert_eq!(e.line, 3);
        assert!(
            e.message.contains("unknown license `WTFPL-9`") && e.message.contains("MIT"),
            "{e}"
        );
        assert_eq!(npm_license("private", None), "UNLICENSED");
        assert_eq!(npm_license("Apache-2.0", None), "Apache-2.0");
        assert_eq!(
            npm_license("custom", Some("EULA.txt")),
            "SEE LICENSE IN EULA.txt"
        );
        assert_eq!(npm_license("custom", None), "SEE LICENSE IN LICENSE");
    }

    // M9-API-1317
    #[test]
    fn the_system_user_has_a_name() {
        assert!(!SystemUser.username().is_empty());
        // The full name may be empty (no display name); it never panics.
        let _ = SystemUser.full_name();
    }

    // M2-BLEND-01
    #[test]
    fn blending_is_srgb_by_default_linear_is_kept_and_unknown_values_warn() {
        let default = load_ok("blending-default", r#"{ "name": "x" }"#);
        assert_eq!(default.blending, Blending::Srgb);
        assert!(default.to_json().get("blending").is_none());
        let linear = load_ok(
            "blending-linear",
            r#"{ "name": "x", "blending": "linear" }"#,
        );
        assert_eq!(linear.blending, Blending::Linear);
        assert!(linear.warnings.is_empty());
        assert_eq!(linear.to_json()["blending"], "linear");
        let srgb = load_ok("blending-srgb", r#"{ "name": "x", "blending": "srgb" }"#);
        assert_eq!(srgb.blending, Blending::Srgb);
        let bad = load_ok("blending-bad", r#"{ "name": "x", "blending": "gamma" }"#);
        assert_eq!(bad.blending, Blending::Srgb);
        assert_eq!(bad.warnings.len(), 1);
        assert!(bad.warnings[0].contains("`blending`") && bad.warnings[0].contains("gamma"));
        assert!(!bad.extra.contains_key("blending"), "a known key");
    }

    // M2-THEME-03
    #[test]
    fn theme_follows_the_system_by_default_light_and_dark_force_it_and_unknown_values_warn() {
        let default = load_ok("theme-default", r#"{ "name": "x" }"#);
        assert_eq!(default.theme, Theme::System);
        assert!(default.to_json().get("theme").is_none());
        assert!(default.theme.is_dark(|| true));
        assert!(!default.theme.is_dark(|| false));
        let dark = load_ok("theme-dark", r#"{ "name": "x", "theme": "dark" }"#);
        assert_eq!(dark.theme, Theme::Dark);
        assert!(dark.warnings.is_empty());
        assert_eq!(dark.to_json()["theme"], "dark");
        assert!(dark.theme.is_dark(|| false));
        let light = load_ok("theme-light", r#"{ "name": "x", "theme": "light" }"#);
        assert_eq!(light.theme, Theme::Light);
        assert!(
            !light
                .theme
                .is_dark(|| panic!("a forced theme never asks the system"))
        );
        let system = load_ok("theme-system", r#"{ "name": "x", "theme": "system" }"#);
        assert_eq!(system.theme, Theme::System);
        let bad = load_ok("theme-bad", r#"{ "name": "x", "theme": "sepia" }"#);
        assert_eq!(bad.theme, Theme::System);
        assert_eq!(bad.warnings.len(), 1);
        assert!(bad.warnings[0].contains("`theme`") && bad.warnings[0].contains("sepia"));
        assert!(!bad.extra.contains_key("theme"), "a known key");
    }

    // M9-API-1330
    #[test]
    fn the_renderer_is_automatic_by_default_and_each_value_is_kept() {
        let auto = load_ok("renderer-default", r#"{ "name": "x" }"#);
        assert_eq!(auto.renderer, Renderer::Auto);
        assert!(auto.warnings.is_empty());
        assert!(
            auto.to_json().get("renderer").is_none(),
            "the default isn't written"
        );
        for (text, r) in [
            ("vulkan", Renderer::Vulkan),
            ("gles", Renderer::Gles),
            ("cpu", Renderer::Cpu),
        ] {
            let p = load_ok(
                "renderer-set",
                &format!(r#"{{ "name": "x", "renderer": "{text}" }}"#),
            );
            assert_eq!(p.renderer, r);
            assert_eq!(p.to_json()["renderer"], text);
            assert!(p.extra.is_empty() && p.warnings.is_empty());
        }
        assert!(Renderer::Gles.ui_only() && Renderer::Cpu.ui_only());
        assert!(!Renderer::Vulkan.ui_only() && !Renderer::Auto.ui_only());
    }

    // M9-API-1330
    #[test]
    fn an_unknown_renderer_warns_and_counts_as_automatic() {
        let p = load_ok("renderer-bad", r#"{ "name": "x", "renderer": "metal" }"#);
        assert_eq!(p.renderer, Renderer::Auto);
        assert_eq!(p.warnings.len(), 1);
        assert!(p.warnings[0].contains("`renderer`") && p.warnings[0].contains("metal"));
    }

    // M9-API-1331
    #[test]
    fn the_old_type_warns_and_is_ignored() {
        for t in ["app", "game"] {
            let p = load_ok("type-old", &format!(r#"{{ "name": "x", "type": "{t}" }}"#));
            assert_eq!(p.renderer, Renderer::Auto, "`type` is not mapped");
            assert_eq!(p.warnings, [TYPE_REPLACED]);
            assert!(p.extra.is_empty(), "`type` is not kept as an extra key");
            assert!(p.to_json().get("type").is_none());
        }
    }

    // M9-API-1332
    #[test]
    fn the_first_renderer_set_wins() {
        use Renderer::*;
        assert_eq!(Renderer::first_set([Auto, Gles, Vulkan]), Gles);
        assert_eq!(Renderer::first_set([Cpu, Gles]), Cpu);
        assert_eq!(Renderer::first_set([Auto, Auto]), Auto);
        for r in [Auto, Vulkan, Gles, Cpu] {
            assert_eq!(Renderer::parse(r.as_str()), Some(r));
        }
        assert_eq!(Renderer::parse("game"), None);
    }
}
