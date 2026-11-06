// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The main window drawn on the CPU (`web_ui_render_cpu`: `vello_cpu` into a pixel buffer,
//! shown through `softbuffer`), no GPU API at all. Opt-in only (`renderer: "cpu"` or
//! `--renderer cpu`), never chosen automatically; UI only, like the GLES path: no 3D world,
//! world panels or secondary windows ("3D Not Supported" when the game asks for the world). If
//! softbuffer can't make a surface, the host falls back to wgpu.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use platypus_platform::Window;
use web_ui_render_cpu::{CpuRenderer, Presenter};

use super::App;
use super::gles_gfx::{Shown, unchanged};

pub(crate) struct CpuGfx {
    pub(crate) window: Arc<Window>,
    renderer: CpuRenderer,
    presenter: Presenter<Arc<Window>>,
    /// Frames and render time since the last report.
    report: (u32, Duration, Instant),
    /// What the window shows now, so an unchanged frame isn't redrawn.
    shown: Option<Shown>,
}

/// The background, as the other renderers clear to.
fn clear() -> web_ui_dom::Rgba {
    let c = super::CLEAR;
    web_ui_dom::Rgba {
        r: c.r as f32,
        g: c.g as f32,
        b: c.b as f32,
        a: 1.0,
    }
}

impl App {
    /// Starts drawing `window` on the CPU. `false`: it can't (the caller falls back).
    pub(crate) fn install_cpu(&mut self, window: Arc<Window>) -> bool {
        let presenter = match Presenter::new(window.clone()) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("CPU renderer unavailable (softbuffer): {e}");
                return false;
            }
        };
        let mut renderer = CpuRenderer::new();
        renderer.set_asset_root(self.asset_root.clone());
        if self.blending == platypus_core::project::Blending::Linear {
            renderer.ask_linear_blending();
        }
        tracing::info!("renderer ready (CPU: vello_cpu + softbuffer)");
        self.light_system_info(&window, "CPU (vello_cpu)", "Cpu");
        self.cpu = Some(CpuGfx {
            window,
            renderer,
            presenter,
            report: (0, Duration::ZERO, Instant::now()),
            shown: None,
        });
        true
    }

    /// Tells the UI the size of every image it asked for (layout needs it).
    fn load_cpu_images(&mut self) {
        let Some(cpu) = &mut self.cpu else {
            return;
        };
        for src in self.ui.take_image_requests() {
            let size = cpu.renderer.image_size(&src);
            self.ui.set_image_size(&src, size);
        }
    }

    /// This frame's size, scale and draw lists.
    fn cpu_frame(&mut self) -> Option<CpuFrame> {
        let (dpi, size) = self.main_metrics()?;
        let scale = super::ui_factor_for(dpi, size);
        let window = self.cpu.as_ref()?.window.clone();
        self.light_media_env(&window, scale);
        self.load_cpu_images();
        let cmds = self.light_ui_commands(size, scale);
        self.scene.scene.update_world();
        let fps_cmds = self.light_fps_commands(size, dpi);
        Some(CpuFrame {
            size,
            scale,
            dpi,
            cmds,
            fps_cmds,
        })
    }

    /// `--screenshot`: draws the UI once and writes the pixels the CPU made as a PNG.
    pub(crate) fn save_screenshot_cpu(&mut self, path: &Path) -> Result<()> {
        let f = self.cpu_frame().context("no window")?;
        let cpu = self.cpu.as_mut().context("no renderer")?;
        let pixmap = f.draw(&mut cpu.renderer);
        // Opaque (drawn over an opaque clear), so premultiplied is the same as straight RGBA.
        let pixels: Vec<u8> = pixmap
            .data()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();
        let (w, h) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        image::save_buffer(path, &pixels, w, h, image::ColorType::Rgba8)?;
        tracing::info!("saved screenshot to {}", path.display());
        Ok(())
    }

    /// Draws one frame of the screen UI and shows it.
    pub(crate) fn render_cpu(&mut self, started: Instant) {
        let Some(f) = self.cpu_frame() else {
            return;
        };
        let Some(cpu) = &mut self.cpu else {
            return;
        };
        // Nothing changed since the last frame: nothing to draw or present.
        if unchanged(
            cpu.shown.as_ref(),
            f.size,
            f.scale,
            &f.cmds,
            f.fps_cmds.as_deref(),
        ) {
            self.tick_fps();
            return;
        }
        let pixmap = f.draw(&mut cpu.renderer);
        if let Err(e) = cpu.presenter.present(pixmap) {
            tracing::error!("CPU present: {e}");
        }
        cpu.shown = Some((f.size, f.scale.to_bits(), f.cmds, f.fps_cmds));
        self.render_time += started.elapsed();
        cpu.report.0 += 1;
        cpu.report.1 += started.elapsed();
        let secs = cpu.report.2.elapsed().as_secs_f64();
        if secs >= 1.0 {
            tracing::debug!(
                "CPU renderer: {:.1} fps, {:.2} ms per frame (draw + present)",
                f64::from(cpu.report.0) / secs,
                cpu.report.1.as_secs_f64() * 1000.0 / f64::from(cpu.report.0)
            );
            cpu.report = (0, Duration::ZERO, Instant::now());
        }
        self.tick_fps();
    }
}

struct CpuFrame {
    size: (u32, u32),
    scale: f32,
    dpi: f64,
    cmds: Vec<web_ui_dom::DrawCmd>,
    fps_cmds: Option<Vec<web_ui_dom::DrawCmd>>,
}

impl CpuFrame {
    /// The UI, and the FPS counter over it at the window's own scale.
    fn draw<'r>(&self, renderer: &'r mut CpuRenderer) -> &'r web_ui_render_cpu::Pixmap {
        let mut layers: Vec<(f32, &[web_ui_dom::DrawCmd])> = vec![(self.scale, &self.cmds)];
        if let Some(fps) = &self.fps_cmds {
            layers.push((self.dpi as f32, fps));
        }
        renderer.render(self.size, clear(), &layers)
    }
}
