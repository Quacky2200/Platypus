// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `platypus-app <game> --bench`: a windowless, uncapped frame loop that times every frame and writes
//! the result as JSON (`docs/platypus-build.md`, "Benchmarks").
//!
//! No OS window is made: the main window is an offscreen texture, the platform is the fake one
//! (`platypus_platform::FakePlatform`, secondary windows virtual as in screenshot runs), and input
//! comes from a script of segments (`--bench-input spec.json`) sent the way the platform sends
//! the OS's. Each frame waits for the GPU work of the one before (a swapchain queueing one frame),
//! so GPU time counts too.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use platypus_platform::{
    FakePlatform, GamepadEvent, InputEvent, Modifiers, MouseButton, WheelDelta,
};
use platypus_platform::{WindowKey, WindowSpec};
use serde_json::{Value, json};

use super::App;
use super::frame_target::FrameTarget;

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Marks the start of the process (for the start-up time); call first thing in `main`.
pub(crate) fn mark_start() {
    PROCESS_START.get_or_init(Instant::now);
}

fn since_start() -> Duration {
    PROCESS_START.get().map_or(Duration::ZERO, Instant::elapsed)
}

/// The command line of a bench run.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchArgs {
    /// Segments and input (`--bench-input`); without one, one idle segment of `frames`.
    pub input: Option<PathBuf>,
    /// Where the JSON goes (`--bench-out`); stdout without one.
    pub out: Option<PathBuf>,
    /// Frames run before measuring (`--warmup`), and measured (`--frames`).
    pub warmup: u32,
    pub frames: u32,
    /// `--fixed-dt seconds`: every frame advances the game's clock by exactly this.
    pub fixed_dt: Option<f64>,
    /// `--size WxH`: the target's size in pixels (default: the game's window size).
    pub size: Option<(u32, u32)>,
    /// `--bench-shots dir`: a PNG of the last frame of each segment, to check the input did
    /// what it should (drawn after the segment's timing).
    pub shots: Option<PathBuf>,
}

impl Default for BenchArgs {
    fn default() -> Self {
        BenchArgs {
            input: None,
            out: None,
            warmup: 120,
            frames: 600,
            fixed_dt: None,
            size: None,
            shots: None,
        }
    }
}

/// Flags that belong to a bench run (any of them turns it on).
pub(crate) fn is_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--bench"
            | "--bench-input"
            | "--bench-out"
            | "--bench-shots"
            | "--warmup"
            | "--fixed-dt"
            | "--size"
    )
}

impl BenchArgs {
    /// Reads one flag (and its value from `rest`).
    pub(crate) fn flag(
        &mut self,
        flag: &str,
        rest: &mut impl Iterator<Item = std::ffi::OsString>,
    ) -> Result<()> {
        let mut value = || -> Result<String> {
            rest.next()
                .map(|v| v.to_string_lossy().into_owned())
                .with_context(|| format!("{flag} needs a value"))
        };
        match flag {
            "--bench" => {}
            "--bench-input" => self.input = Some(value()?.into()),
            "--bench-out" => self.out = Some(value()?.into()),
            "--bench-shots" => self.shots = Some(value()?.into()),
            "--warmup" => self.warmup = value()?.parse().context("--warmup needs a number")?,
            "--fixed-dt" => {
                let dt: f64 = value()?.parse().context("--fixed-dt needs seconds")?;
                if !(f64::EPSILON..1.0).contains(&dt) {
                    bail!("--fixed-dt must be between 0 and 1 second");
                }
                self.fixed_dt = Some(dt);
            }
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').context("--size needs WxH")?;
                self.size = Some((w.parse()?, h.parse()?));
            }
            _ => bail!("unknown bench flag {flag}"),
        }
        Ok(())
    }
}

/// A run's plan: frames before the first segment, then each segment.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Spec {
    pub warmup: u32,
    pub fixed_dt: Option<f64>,
    pub segments: Vec<Segment>,
}

/// Frames measured together, with the input sent during them. Frame numbers count from the
/// segment's start, its own warm-up included.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Segment {
    pub name: String,
    pub warmup: u32,
    pub frames: u32,
    pub input: Vec<(u32, Step)>,
}

/// One thing sent to the main window at a frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    Event(InputEvent),
    /// The pointer moves to the middle of the first screen-UI text that reads this (found
    /// when the frame comes, so a script needs no coordinates for buttons and links).
    MoveToText(String),
}

impl Spec {
    /// One idle segment, from the command line.
    fn idle(args: &BenchArgs) -> Spec {
        Spec {
            warmup: args.warmup,
            fixed_dt: args.fixed_dt,
            segments: vec![Segment {
                name: "idle".into(),
                warmup: 0,
                frames: args.frames,
                input: Vec::new(),
            }],
        }
    }

    /// Reads a bench file; the command line's `--fixed-dt` wins over the file's.
    pub(crate) fn parse(text: &str, args: &BenchArgs) -> Result<Spec> {
        let v: Value = serde_json::from_str(text).context("bench file is not JSON")?;
        let num = |v: &Value, key: &str, default: u32| -> u32 {
            v[key].as_u64().map_or(default, |n| n as u32)
        };
        let mut segments = Vec::new();
        for (i, s) in v["segments"].as_array().into_iter().flatten().enumerate() {
            let mut input = Vec::new();
            for e in s["input"].as_array().into_iter().flatten() {
                let once = events(e).with_context(|| format!("segment {i}: {e}"))?;
                // `"count": n, "every": frames` repeats an input.
                let every = num(e, "every", 1).max(1);
                for k in 0..num(e, "count", 1) {
                    input.extend(once.iter().map(|(f, s)| (f + k * every, s.clone())));
                }
            }
            input.sort_by_key(|(f, _)| *f);
            let input = with_modifiers(input);
            segments.push(Segment {
                name: s["name"]
                    .as_str()
                    .map_or_else(|| format!("segment {}", i + 1), str::to_owned),
                warmup: num(s, "warmup", 0),
                frames: num(s, "frames", args.frames),
                input,
            });
        }
        if segments.is_empty() {
            segments = Spec::idle(args).segments;
            segments[0].frames = num(&v, "frames", args.frames);
        }
        Ok(Spec {
            warmup: num(&v, "warmup", args.warmup),
            fixed_dt: args.fixed_dt.or(v["fixedDt"].as_f64()),
            segments,
        })
    }
}

/// The DOM `key` of a physical key code, for scripts that read `key` (`KeyA` -> `a`).
fn key_of(code: &str) -> String {
    if let Some(c) = code.strip_prefix("Key").filter(|c| c.len() == 1) {
        return c.to_lowercase();
    }
    if let Some(d) = code.strip_prefix("Digit") {
        return d.to_owned();
    }
    match code {
        "Space" => " ".into(),
        "Minus" => "-".into(),
        "Equal" => "=".into(),
        _ => code.to_owned(),
    }
}

fn button_of(name: Option<&str>) -> MouseButton {
    match name {
        Some("right") => MouseButton::Right,
        Some("middle") => MouseButton::Middle,
        _ => MouseButton::Left,
    }
}

/// A gamepad axis or button name as the platform's (static) one.
fn pad_name(name: &str) -> &'static str {
    const NAMES: &[&str] = &[
        "LeftStickX",
        "LeftStickY",
        "RightStickX",
        "RightStickY",
        "LeftZ",
        "RightZ",
        "DPadX",
        "DPadY",
        "South",
        "East",
        "West",
        "North",
        "LeftBumper",
        "RightBumper",
        "LeftTrigger",
        "RightTrigger",
        "Select",
        "Start",
        "Mode",
        "LeftStick",
        "RightStick",
        "DPadUp",
        "DPadDown",
        "DPadLeft",
        "DPadRight",
    ];
    NAMES
        .iter()
        .find(|n| **n == name)
        .copied()
        .unwrap_or("Unknown")
}

/// The steps with a `Modifiers` event after each press or release of Ctrl, Shift, Alt or the
/// Windows key, as the window sends: the UI reads modifiers from those (Ctrl+Z in a text field).
fn with_modifiers(input: Vec<(u32, Step)>) -> Vec<(u32, Step)> {
    let mut held = Modifiers::default();
    let mut out = Vec::with_capacity(input.len());
    for (frame, step) in input {
        let change = match &step {
            Step::Event(InputEvent::KeyDown { code, .. }) => Some((code.clone(), true)),
            Step::Event(InputEvent::KeyUp { code, .. }) => Some((code.clone(), false)),
            _ => None,
        };
        out.push((frame, step));
        let Some((code, down)) = change else {
            continue;
        };
        let flag = match code.as_str() {
            "ControlLeft" | "ControlRight" => &mut held.ctrl,
            "ShiftLeft" | "ShiftRight" => &mut held.shift,
            "AltLeft" | "AltRight" => &mut held.alt,
            "MetaLeft" | "MetaRight" | "SuperLeft" | "SuperRight" => &mut held.meta,
            _ => continue,
        };
        *flag = down;
        out.push((frame, Step::Event(InputEvent::Modifiers(held))));
    }
    out
}

/// One input entry as the steps it stands for, each at its frame.
fn events(e: &Value) -> Result<Vec<(u32, Step)>> {
    let frame = e["frame"].as_u64().context("an input needs a frame")? as u32;
    let hold = e["hold"].as_u64().map_or(1, |h| h.max(1) as u32);
    let button = button_of(e["button"].as_str());
    let text = || -> Result<String> {
        e["text"]
            .as_str()
            .map(str::to_owned)
            .context("a text input needs a text")
    };
    match e["kind"].as_str().unwrap_or("") {
        "clickText" => {
            return Ok(vec![
                (frame, Step::MoveToText(text()?)),
                (frame, Step::Event(InputEvent::MouseDown { button })),
                (frame + hold, Step::Event(InputEvent::MouseUp { button })),
            ]);
        }
        "moveToText" => return Ok(vec![(frame, Step::MoveToText(text()?))]),
        _ => {}
    }
    Ok(platform_events(e, frame, hold, button)?
        .into_iter()
        .map(|(f, e)| (f, Step::Event(e)))
        .collect())
}

fn platform_events(
    e: &Value,
    frame: u32,
    hold: u32,
    button: MouseButton,
) -> Result<Vec<(u32, InputEvent)>> {
    let f = |k: &str| e[k].as_f64();
    let code = || -> Result<String> {
        e["code"]
            .as_str()
            .map(str::to_owned)
            .context("a key input needs a code")
    };
    let key = |code: &str| {
        e["key"]
            .as_str()
            .map_or_else(|| key_of(code), str::to_owned)
    };
    let down = |code: String| {
        let key = key(&code);
        let text = (key.chars().count() == 1).then(|| key.clone());
        InputEvent::KeyDown {
            code,
            key,
            text,
            repeat: false,
        }
    };
    let up = |code: String| InputEvent::KeyUp {
        key: key(&code),
        code,
    };
    let at = |x: f64, y: f64| InputEvent::MouseMove { x, y };
    let pad = e["pad"].as_u64().unwrap_or(0) as u32;
    Ok(match e["kind"].as_str().unwrap_or("") {
        "key" => vec![(frame, down(code()?)), (frame + hold, up(code()?))],
        "keyDown" => vec![(frame, down(code()?))],
        "keyUp" => vec![(frame, up(code()?))],
        "mouseMove" => vec![(
            frame,
            at(
                f("x").context("mouseMove needs x")?,
                f("y").context("mouseMove needs y")?,
            ),
        )],
        "click" => {
            let (x, y) = (
                f("x").context("click needs x")?,
                f("y").context("click needs y")?,
            );
            vec![
                (frame, at(x, y)),
                (frame, InputEvent::MouseDown { button }),
                (frame + hold, InputEvent::MouseUp { button }),
            ]
        }
        "mouseDown" => vec![(frame, InputEvent::MouseDown { button })],
        "mouseUp" => vec![(frame, InputEvent::MouseUp { button })],
        "wheel" => vec![(
            frame,
            InputEvent::MouseWheel(WheelDelta::Lines {
                x: f("dx").unwrap_or(0.0) as f32,
                y: f("dy").unwrap_or(0.0) as f32,
            }),
        )],
        "axis" => {
            let name = pad_name(e["axis"].as_str().context("axis needs a name")?);
            let value = f("value").unwrap_or(0.0) as f32;
            vec![(
                frame,
                InputEvent::Gamepad {
                    id: pad,
                    event: GamepadEvent::Axis(name, value),
                },
            )]
        }
        "button" => {
            let name = pad_name(e["name"].as_str().context("button needs a name")?);
            vec![
                (
                    frame,
                    InputEvent::Gamepad {
                        id: pad,
                        event: GamepadEvent::ButtonDown(name),
                    },
                ),
                (
                    frame + hold,
                    InputEvent::Gamepad {
                        id: pad,
                        event: GamepadEvent::ButtonUp(name),
                    },
                ),
            ]
        }
        other => bail!("unknown input kind {other:?}"),
    })
}

/// One frame's times, in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct FrameTimes {
    pub total: f64,
    /// Game code (hooks, timers, the UI it renders).
    pub script: f64,
    /// A compiled game's registry sync into the renderer.
    pub sync: f64,
    /// Drawing: the UI's layout and draw list, the world, the GPU wait.
    pub render: f64,
}

/// Summary of a run of frames.
pub(crate) fn stats(frames: &[FrameTimes]) -> Value {
    if frames.is_empty() {
        return json!({ "frames": 0 });
    }
    let n = frames.len() as f64;
    let mut sorted: Vec<f64> = frames.iter().map(|f| f.total).collect();
    sorted.sort_by(f64::total_cmp);
    let pct = |p: f64| sorted[(((sorted.len() - 1) as f64) * p).round() as usize];
    let sum: f64 = sorted.iter().sum();
    let mean = |g: fn(&FrameTimes) -> f64| frames.iter().map(g).sum::<f64>() / n;
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    let script = mean(|f| f.script);
    let sync = mean(|f| f.sync);
    let render = mean(|f| f.render);
    json!({
        "frames": frames.len(),
        "fps": round(if sum > 0.0 { n * 1000.0 / sum } else { 0.0 }),
        "meanMs": round(sum / n),
        "medianMs": round(pct(0.5)),
        "p95Ms": round(pct(0.95)),
        "p99Ms": round(pct(0.99)),
        "worstMs": round(sorted[sorted.len() - 1]),
        "scriptMs": round(script),
        "syncMs": round(sync),
        "renderMs": round(render),
        "otherMs": round((sum / n - script - sync - render).max(0.0)),
    })
}

/// The most memory the process held at once, in MB (Windows: peak working set).
fn peak_memory_mb() -> Option<f64> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        // SAFETY: the counters are a plain struct of the size given; the handle is the
        // pseudo-handle of this process.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        (ok != 0).then(|| (c.PeakWorkingSetSize as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The middle of the first shown screen-UI text block reading `text` (else the first containing
/// it), in window pixels.
fn text_centre(app: &mut App, text: &str) -> Option<(f64, f64)> {
    app.ui.layout_if_needed();
    let ui = &app.ui;
    let mut blocks = Vec::new();
    let mut stack = vec![web_ui_dom::ROOT];
    while let Some(id) = stack.pop() {
        if !ui.is_displayed(id) {
            continue;
        }
        if ui.is_text_root(id) {
            blocks.push((id, ui.text_content(id)));
            continue;
        }
        stack.extend(ui.children(id).iter().rev());
    }
    let found = blocks
        .iter()
        .find(|(_, t)| t.trim() == text)
        .or_else(|| blocks.iter().find(|(_, t)| t.contains(text)))?;
    let r = ui.rect(found.0)?;
    // Through transforms (a modal dialog is centred by `translate: -50% -50%`).
    let (x, y) = ui.to_window(found.0, r.x + r.width / 2.0, r.y + r.height / 2.0)?;
    let scale = f64::from(app.scale());
    Some((f64::from(x) * scale, f64::from(y) * scale))
}

/// Runs the game without a window as `args` says and writes the JSON report.
pub(crate) fn run(mut app: App, args: &BenchArgs, size: (u32, u32), title: &str) -> Result<()> {
    let spec = match &args.input {
        Some(path) => Spec::parse(
            &std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?,
            args,
        )?,
        None => Spec::idle(args),
    };
    let mut lap = web_ui_ts::timing::Lap::start();
    let gpu = platypus_render::Gpu::headless().map_err(|e| anyhow::anyhow!("{e}"))?;
    lap.mark("GPU device (headless)");
    let target = FrameTarget::offscreen(&gpu, size.0, size.1);
    let adapter = gpu.adapter_name();
    app.install_gfx(gpu, target, None);
    lap.mark("renderer set-up");
    let game = app
        .code_root
        .canonicalize()
        .unwrap_or_else(|_| app.code_root.clone());
    let game = game
        .file_name()
        .map_or_else(|| title.to_owned(), |n| n.to_string_lossy().into_owned());
    let started = app.started;
    let main = WindowSpec::main(title, f64::from(size.0), f64::from(size.1));
    let mut p = FakePlatform::with_app_main(app, main, 1.0);
    lap.mark("platform");
    let load_ms = since_start().as_secs_f64() * 1000.0;

    let mut frame_no: u64 = 0;
    let mut startup_ms = None;
    let mut startup_parts = Vec::new();
    let mut pads = std::collections::HashSet::new();
    crate::sync_profile::enable();
    let mut run_frames = |p: &mut FakePlatform<App>, count: u32, input: &[(u32, Step)]| {
        let mut times = Vec::with_capacity(count as usize);
        let mut parts = Vec::with_capacity(count as usize);
        let mut next = 0;
        for f in 0..count {
            if p.exited() {
                break;
            }
            // The frame's clock, then its input (handled as part of the frame), then the frame.
            frame_no += 1;
            if let Some(dt) = spec.fixed_dt {
                let at = Duration::from_nanos((frame_no as f64 * dt * 1e9).ceil() as u64);
                p.app().fake_now = Some(started + at);
            }
            let app = p.app();
            app.script_time = Duration::ZERO;
            app.sync_time = Duration::ZERO;
            app.render_time = Duration::ZERO;
            let t = Instant::now();
            while let Some((_, step)) = input.get(next).filter(|(at, _)| *at <= f) {
                next += 1;
                let event = match step {
                    Step::Event(e) => e.clone(),
                    Step::MoveToText(text) => match text_centre(p.app(), text) {
                        Some((x, y)) => InputEvent::MouseMove { x, y },
                        None => {
                            tracing::warn!("bench: no text {text:?} on screen");
                            continue;
                        }
                    },
                };
                if let InputEvent::Gamepad { id, .. } = &event
                    && pads.insert(*id)
                {
                    p.send(
                        WindowKey::MAIN,
                        InputEvent::Gamepad {
                            id: *id,
                            event: GamepadEvent::Connected {
                                name: "bench".into(),
                            },
                        },
                    );
                }
                p.send(WindowKey::MAIN, event);
            }
            p.step();
            let total = t.elapsed();
            if startup_ms.is_none() {
                startup_ms = Some(since_start().as_secs_f64() * 1000.0);
                web_ui_ts::timing::add("first frame", total.as_secs_f64() * 1000.0);
                startup_parts = web_ui_ts::timing::take();
            }
            let app = p.app();
            times.push(FrameTimes {
                total: total.as_secs_f64() * 1000.0,
                script: app.script_time.as_secs_f64() * 1000.0,
                sync: app.sync_time.as_secs_f64() * 1000.0,
                render: app.render_time.as_secs_f64() * 1000.0,
            });
            parts.push(crate::sync_profile::take());
        }
        (times, parts)
    };

    run_frames(&mut p, spec.warmup, &[]);
    let mut all = Vec::new();
    let mut all_parts = Vec::new();
    let mut segments = Vec::new();
    for (i, s) in spec.segments.iter().enumerate() {
        let (times, parts) = run_frames(&mut p, s.warmup + s.frames, &s.input);
        let measured = times.get(s.warmup as usize..).unwrap_or(&[]);
        let measured_parts = parts.get(s.warmup as usize..).unwrap_or(&[]);
        all.extend_from_slice(measured);
        all_parts.extend_from_slice(measured_parts);
        let mut v = stats(measured);
        v["syncParts"] = part_means(measured_parts);
        v["name"] = s.name.clone().into();
        v["warmup"] = s.warmup.into();
        segments.push(v);
        if let Some(dir) = &args.shots {
            std::fs::create_dir_all(dir)?;
            let slug: String = s
                .name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            p.app()
                .save_screenshot(&dir.join(format!("{:02}-{slug}.png", i + 1)))?;
        }
    }
    if let Some(gfx) = p.app().gfx.as_mut() {
        gfx.surface.finish(&gfx.gpu);
    }
    let report = json!({
        "game": game,
        "backend": "native",
        "adapter": adapter,
        "size": [size.0, size.1],
        "fixedDt": spec.fixed_dt,
        "warmupFrames": spec.warmup,
        "exitedEarly": p.exited(),
        "loadMs": (load_ms * 10.0).round() / 10.0,
        "startupMs": startup_ms.map(|m| (m * 10.0).round() / 10.0),
        "startupParts": part_means(&[startup_parts]),
        "peakMemoryMb": peak_memory_mb(),
        "total": stats(&all),
        "syncParts": part_means(&all_parts),
        "segments": segments,
    });
    write_report(args.out.as_deref(), &report)
}

/// The mean milliseconds a frame of each part of the render sync (`sync_profile`), largest first.
fn part_means(frames: &[Vec<(&'static str, f64)>]) -> Value {
    let mut sums: Vec<(&str, f64)> = Vec::new();
    for parts in frames {
        for &(name, ms) in parts {
            match sums.iter_mut().find(|(n, _)| *n == name) {
                Some(s) => s.1 += ms,
                None => sums.push((name, ms)),
            }
        }
    }
    let n = frames.len().max(1) as f64;
    sums.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut m = serde_json::Map::new();
    for (name, sum) in sums {
        m.insert(name.into(), json!((sum / n * 10000.0).round() / 10000.0));
    }
    Value::Object(m)
}

fn write_report(out: Option<&Path>, report: &Value) -> Result<()> {
    let text = serde_json::to_string_pretty(report)? + "\n";
    match out {
        Some(path) => {
            std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
            tracing::info!("bench: wrote {}", path.display());
        }
        None => {
            use std::io::Write;
            std::io::stdout().write_all(text.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // M9-BEN-01
    #[test]
    fn a_bench_file_becomes_segments_with_timed_platform_events() {
        let spec = Spec::parse(
            r#"{ "warmup": 30, "fixedDt": 0.02, "segments": [
                { "name": "walk", "frames": 100, "warmup": 10, "input": [
                    { "frame": 5, "kind": "key", "code": "KeyD", "hold": 40 },
                    { "frame": 0, "kind": "click", "x": 10, "y": 20 } ] },
                { "frames": 50 } ] }"#,
            &BenchArgs::default(),
        )
        .unwrap();
        assert_eq!((spec.warmup, spec.fixed_dt), (30, Some(0.02)));
        assert_eq!(spec.segments.len(), 2);
        let walk = &spec.segments[0];
        assert_eq!((walk.warmup, walk.frames), (10, 100));
        let frames: Vec<u32> = walk.input.iter().map(|(f, _)| *f).collect();
        assert_eq!(frames, [0, 0, 1, 5, 45]);
        assert!(matches!(
            &walk.input[3].1,
            Step::Event(InputEvent::KeyDown { code, key, .. }) if code == "KeyD" && key == "d"
        ));
        assert!(matches!(
            &walk.input[4].1,
            Step::Event(InputEvent::KeyUp { .. })
        ));
        assert_eq!(spec.segments[1].name, "segment 2");
        let repeated = Spec::parse(
            r#"{"segments":[{"input":[{"frame":2,"kind":"clickText","text":"Go","count":3,"every":10}]}]}"#,
            &BenchArgs::default(),
        )
        .unwrap();
        let input = &repeated.segments[0].input;
        assert_eq!(input.len(), 9);
        assert_eq!(input[0], (2, Step::MoveToText("Go".into())));
        assert_eq!(input.last().unwrap().0, 23);
        assert!(
            Spec::parse(
                r#"{"segments":[{"input":[{"frame":0,"kind":"fly"}]}]}"#,
                &BenchArgs::default()
            )
            .is_err()
        );
    }

    // M9-BEN-04
    #[test]
    fn modifier_keys_send_the_modifier_state() {
        let spec = Spec::parse(
            r#"{ "segments": [ { "input": [
                { "frame": 0, "kind": "keyDown", "code": "ControlLeft" },
                { "frame": 1, "kind": "key", "code": "KeyZ" },
                { "frame": 2, "kind": "keyUp", "code": "ControlLeft" } ] } ] }"#,
            &BenchArgs::default(),
        )
        .unwrap();
        let mods: Vec<(u32, bool)> = spec.segments[0]
            .input
            .iter()
            .filter_map(|(f, s)| match s {
                Step::Event(InputEvent::Modifiers(m)) => Some((*f, m.ctrl)),
                _ => None,
            })
            .collect();
        assert_eq!(mods, [(0, true), (2, false)]);
    }

    // M9-BEN-01
    #[test]
    fn frame_stats_give_fps_and_percentiles() {
        let frames: Vec<FrameTimes> = (1..=100)
            .map(|i| FrameTimes {
                total: f64::from(i),
                script: 1.0,
                sync: 0.5,
                render: 2.0,
            })
            .collect();
        let s = stats(&frames);
        assert_eq!(s["frames"], 100);
        assert_eq!(s["medianMs"], 51.0);
        assert_eq!(s["p99Ms"], 99.0);
        assert_eq!(s["worstMs"], 100.0);
        assert_eq!(s["scriptMs"], 1.0);
        assert!((s["fps"].as_f64().unwrap() - 1000.0 / 50.5).abs() < 0.01);
    }

    #[test]
    fn bench_flags_parse() {
        let mut b = BenchArgs::default();
        let mut rest = ["0.016", "640x360"]
            .map(std::ffi::OsString::from)
            .into_iter();
        b.flag("--fixed-dt", &mut rest).unwrap();
        b.flag("--size", &mut rest).unwrap();
        assert_eq!((b.fixed_dt, b.size), (Some(0.016), Some((640, 360))));
        assert!(
            b.flag(
                "--fixed-dt",
                &mut ["2"].map(std::ffi::OsString::from).into_iter()
            )
            .is_err()
        );
    }
}
