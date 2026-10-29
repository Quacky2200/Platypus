// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The main window drawn by the GLES executor (`platypus_render_gles`) through EGL: ANGLE on
//! Windows (linked in with the `static-angle` feature, else `libEGL.dll`), Mesa and friends
//! elsewhere. This is the light path for programs that are only UI (`host::Choice::Gles`), so
//! they load no Vulkan or wgpu. There is no 3D world, world panels or secondary windows on this
//! path; if EGL (or the windowing system) is unavailable the host falls back to wgpu.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use platypus_platform::Window;
use platypus_render::UiBackend as _;
use platypus_render_gles::egl::EglContext;
use platypus_render_gles::{GlesRenderer, Target};

use super::App;

/// The frame time when the monitor's refresh rate isn't known: 60 Hz.
pub(crate) const FRAME_CAP: Duration = Duration::from_micros(16_667);

/// The shortest frame time that lets the loop run at the window's monitor's refresh rate (a
/// frame that changed nothing is not swapped, so nothing waits for the display). A 120 Hz
/// monitor gets 120 frames a second; an unknown rate gets [`FRAME_CAP`].
pub(crate) fn frame_cap(window: &Window) -> Duration {
    frame_cap_for_millihertz(
        window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz()),
    )
}

fn frame_cap_for_millihertz(millihertz: Option<u32>) -> Duration {
    match millihertz.filter(|&mhz| mhz > 0) {
        Some(mhz) => Duration::from_secs_f64(1000.0 / f64::from(mhz)),
        None => FRAME_CAP,
    }
}

pub(crate) struct GlesGfx {
    pub(crate) window: Arc<Window>,
    // Declared before `context`: the renderer's GL objects go first.
    renderer: GlesRenderer,
    context: EglContext,
    /// Frames and render time since the last report.
    report: (u32, Duration, Instant),
    /// What the window shows now, so an unchanged frame isn't redrawn.
    shown: Option<Shown>,
    /// When the first frame was shown, and whether the driver was trimmed since (`trim_due`).
    first_frame: Option<Instant>,
    trimmed: bool,
}

/// How long after the first frame the Direct3D driver under ANGLE is trimmed: its shader
/// compiles for the first draws are done by then.
const TRIM_AFTER: Duration = Duration::from_secs(1);

/// Is the one trim of the driver's start-up memory due? `env` is `PLATYPUS_GLES_TRIM` (`0`
/// turns it off).
pub(crate) fn trim_due(
    trimmed: bool,
    since_first_frame: Option<Duration>,
    env: Option<&str>,
) -> bool {
    !trimmed && env != Some("0") && since_first_frame.is_some_and(|d| d >= TRIM_AFTER)
}

/// What a light path's window shows (size, scale, draw lists), to skip unchanged frames.
pub(crate) type Shown = (
    (u32, u32),
    u32,
    Vec<platypus_ui::DrawCmd>,
    Option<Vec<platypus_ui::DrawCmd>>,
);

/// Where ANGLE's DLLs are looked for first (`PLATYPUS_ANGLE_DIR`; ignored when ANGLE is linked in).
fn angle_dir() -> Option<PathBuf> {
    std::env::var_os("PLATYPUS_ANGLE_DIR").map(PathBuf::from)
}

/// How the EGL library is found, for the log.
const EGL_SOURCE: &str = if platypus_render_gles::STATIC_ANGLE {
    "ANGLE linked in"
} else {
    "EGL loaded at run time"
};

impl GlesGfx {
    /// A context and renderer on `window`, or why there isn't one.
    fn new(
        window: Arc<Window>,
        asset_root: &Path,
        blending: platypus_render_gles::Blending,
    ) -> Result<Self, String> {
        let dir = angle_dir();
        super::mem_trace::mark("window");
        let context = EglContext::window(dir.as_deref(), &*window)?;
        context.set_vsync(true);
        super::mem_trace::mark("EGL context (ANGLE: Direct3D 11 device, swap chain)");
        let mut renderer = GlesRenderer::new(context.gl())?;
        renderer.set_asset_root(asset_root.to_path_buf());
        renderer.set_blending(blending);
        super::mem_trace::mark("GLES renderer (programs, tables)");
        Ok(Self {
            window,
            renderer,
            context,
            report: (0, Duration::ZERO, Instant::now()),
            shown: None,
            first_frame: None,
            trimmed: false,
        })
    }

    /// Trims the driver once, a second after the first frame (`trim_due`).
    fn trim_if_due(&mut self) {
        let env = std::env::var("PLATYPUS_GLES_TRIM").ok();
        if trim_due(
            self.trimmed,
            self.first_frame.map(|t| t.elapsed()),
            env.as_deref(),
        ) {
            self.trimmed = true;
            let ok = self.context.trim_device();
            tracing::debug!("GLES: trimmed the driver's start-up memory ({ok})");
            super::mem_trace::mark("the driver trimmed");
        }
    }

    fn target(&self, size: (u32, u32), scale: f32) -> Target {
        Target {
            framebuffer: None,
            size,
            scale,
            srgb: false,
        }
    }
}

/// Whether `shown` is exactly this frame.
pub(crate) fn unchanged(
    shown: Option<&Shown>,
    size: (u32, u32),
    scale: f32,
    cmds: &[platypus_ui::DrawCmd],
    fps_cmds: Option<&[platypus_ui::DrawCmd]>,
) -> bool {
    shown.is_some_and(|(sz, sc, c, f)| {
        *sz == size && *sc == scale.to_bits() && c.as_slice() == cmds && f.as_deref() == fps_cmds
    })
}

impl App {
    /// Starts drawing `window` with the GLES executor. `false`: it can't (the caller falls back).
    pub(crate) fn install_gles(&mut self, window: Arc<Window>) -> bool {
        let gles = match GlesGfx::new(window.clone(), &self.asset_root, self.blending) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("GLES renderer unavailable ({EGL_SOURCE}): {e}");
                return false;
            }
        };
        tracing::info!("renderer ready (GLES through EGL, {EGL_SOURCE})");
        self.light_system_info(&window, "GLES (EGL)", "Gles");
        self.gles = Some(gles);
        true
    }

    /// `SystemInfo` and `Window.main` for a light (UI-only) renderer on `window`.
    pub(crate) fn light_system_info(&mut self, window: &Window, device: &str, kind: &str) {
        let size = window.inner_size();
        let dpi = window.scale_factor();
        self.info.set_system(serde_json::json!({
            "graphicsDeviceName": device,
            "graphicsDeviceType": kind,
            "graphicsDeviceKind": kind,
            "screenWidth": size.width,
            "screenHeight": size.height,
            "screenRefreshRate": 0.0,
            "dpi": dpi * 96.0,
        }));
        self.info.set(
            "display",
            super::display::state(Some(window), &self.display),
        );
        let title = platypus_engine_api::window::state(platypus_engine_api::window::MAIN)
            .map_or_else(String::new, |s| s.title);
        platypus_engine_api::window::set_main(
            &title,
            super::windows::to_ui((size.width, size.height), dpi),
            dpi,
        );
        if title != window.title() {
            window.set_title(&title);
        }
    }

    /// Tells the UI the size of every image it asked for (layout needs it).
    fn load_gles_images(&mut self) {
        let Some(gles) = &mut self.gles else {
            return;
        };
        for src in self.ui.take_image_requests() {
            let size = gles.renderer.image_size(&(), &src);
            self.ui.set_image_size(&src, size);
        }
    }

    /// The FPS counter's commands: at window resolution and scale, over the UI.
    pub(crate) fn light_fps_commands(
        &mut self,
        size: (u32, u32),
        dpi: f64,
    ) -> Option<Vec<platypus_ui::DrawCmd>> {
        self.fps_on.then(|| {
            let text = self.fps.text(self.started.elapsed().as_secs_f64());
            self.fps_overlay
                .draw_list(&text, size.0, size.1, dpi as f32)
        })
    }

    /// The UI's draw list, or the "3D Not Supported" screen when the game uses the world, which
    /// the light renderers don't draw.
    pub(crate) fn light_ui_commands(
        &mut self,
        size: (u32, u32),
        scale: f32,
    ) -> Vec<platypus_ui::DrawCmd> {
        if self.world_view.needs_world() {
            return self
                .notice
                .get_or_insert_with(|| super::notice::Notice::new("3D Not Supported"))
                .draw_list(size.0, size.1, scale);
        }
        self.ui.draw_list()
    }

    /// Sets the UI's media environment (resolution, the system's or the game's theme) before a
    /// light frame.
    pub(crate) fn light_media_env(&mut self, window: &Window, scale: f32) {
        let dark = self.manifest.dark(Some(window));
        self.ui.set_media_env(platypus_ui::media::MediaEnv {
            resolution: scale,
            dark,
            ..self.ui.media_env()
        });
    }

    /// Draws the UI (and the FPS counter over it) into the window's back buffer.
    fn draw_gles(
        &mut self,
        size: (u32, u32),
        scale: f32,
        dpi: f64,
        cmds: &[platypus_ui::DrawCmd],
        fps_cmds: Option<&[platypus_ui::DrawCmd]>,
    ) -> Result<()> {
        let gles = self.gles.as_mut().context("no renderer")?;
        let target = gles.target(size, scale);
        gles.renderer.render(&target, super::CLEAR, cmds);
        if let Some(fps) = fps_cmds {
            let target = gles.target(size, dpi as f32);
            gles.renderer.render_overlay(&target, fps);
        }
        Ok(())
    }

    /// `--screenshot`: draws the UI once and writes it as a PNG.
    pub(crate) fn save_screenshot_gles(&mut self, path: &Path) -> Result<()> {
        let (dpi, size) = self.main_metrics().context("no window")?;
        let scale = super::ui_factor_for(dpi, size);
        self.load_gles_images();
        let cmds = self.light_ui_commands(size, scale);
        let fps_cmds = self.light_fps_commands(size, dpi);
        self.draw_gles(size, scale, dpi, &cmds, fps_cmds.as_deref())?;
        let gles = self.gles.as_ref().context("no renderer")?;
        let (w, h) = size;
        let pixels = gles.renderer.read_back(None, size);
        image::save_buffer(path, &pixels, w, h, image::ColorType::Rgba8)?;
        tracing::info!("saved screenshot to {}", path.display());
        Ok(())
    }

    /// Draws one frame of the screen UI and shows it.
    pub(crate) fn render_gles(&mut self, started: Instant) {
        let Some((dpi, size)) = self.main_metrics() else {
            return;
        };
        let scale = super::ui_factor_for(dpi, size);
        let Some(window) = self.gles.as_ref().map(|g| g.window.clone()) else {
            return;
        };
        self.light_media_env(&window, scale);
        self.load_gles_images();
        let cmds = self.light_ui_commands(size, scale);
        self.scene.scene.update_world();
        let fps_cmds = self.light_fps_commands(size, dpi);
        let Some(gles) = &mut self.gles else {
            return;
        };
        gles.trim_if_due();
        // Nothing changed since the last frame: nothing to draw or swap.
        if unchanged(gles.shown.as_ref(), size, scale, &cmds, fps_cmds.as_deref()) {
            // The counter still counts the frame: it shows the loop's pace, not the redraws.
            self.tick_fps();
            return;
        }
        if let Err(e) = self.draw_gles(size, scale, dpi, &cmds, fps_cmds.as_deref()) {
            tracing::error!("GLES draw: {e}");
            return;
        }
        let Some(gles) = &mut self.gles else {
            return;
        };
        if let Err(e) = gles.context.swap_buffers() {
            tracing::error!("{e}");
        }
        gles.shown = Some((size, scale.to_bits(), cmds, fps_cmds));
        gles.first_frame.get_or_insert_with(Instant::now);
        self.render_time += started.elapsed();
        gles.report.0 += 1;
        gles.report.1 += started.elapsed();
        let secs = gles.report.2.elapsed().as_secs_f64();
        if self.frames < 3 {
            super::mem_trace::mark("a frame drawn");
        }
        if secs >= 1.0 {
            super::mem_trace::mark("a second of frames");
            super::mem_trace::regions_once();
            tracing::debug!(
                "GLES renderer: {:.1} fps, {:.2} ms per frame (draw + swap)",
                f64::from(gles.report.0) / secs,
                gles.report.1.as_secs_f64() * 1000.0 / f64::from(gles.report.0)
            );
            gles.report = (0, Duration::ZERO, Instant::now());
        }
        self.tick_fps();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // M2-GLES-15
    #[test]
    fn the_driver_is_trimmed_once_a_second_after_the_first_frame() {
        let s = |ms| Some(Duration::from_millis(ms));
        // No frame shown yet, or too soon after it.
        assert!(!trim_due(false, None, None));
        assert!(!trim_due(false, s(500), None));
        assert!(trim_due(false, s(1000), None));
        // Once only, and `PLATYPUS_GLES_TRIM=0` turns it off.
        assert!(!trim_due(true, s(5000), None));
        assert!(!trim_due(false, s(5000), Some("0")));
        assert!(trim_due(false, s(5000), Some("1")));
    }

    // Frame caps follow the monitor's refresh rate; 60 Hz when it doesn't say.
    #[test]
    fn the_cap_follows_the_refresh_rate() {
        let at_60 = frame_cap_for_millihertz(Some(60_000));
        assert!((at_60.as_secs_f64() - FRAME_CAP.as_secs_f64()).abs() < 1e-6);
        let at_120 = frame_cap_for_millihertz(Some(120_000));
        assert!((at_120.as_secs_f64() - 1.0 / 120.0).abs() < 1e-6);
        assert_eq!(frame_cap_for_millihertz(None), FRAME_CAP);
        assert_eq!(frame_cap_for_millihertz(Some(0)), FRAME_CAP);
    }
}
