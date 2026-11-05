// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use crate::WindowSurface;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no suitable GPU adapter (hardware or software) was found: {0}")]
    NoAdapter(#[from] wgpu::RequestAdapterError),
    #[error("could not create GPU device: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("could not create window surface: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    #[error("could not read back GPU buffer: {0}")]
    Readback(String),
    /// The GPU in use cannot present to this window (it is on a monitor another adapter drives).
    #[error("{0}")]
    Unsupported(String),
}

static LOW_MEMORY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Asks the GPU allocator for small blocks (`MemoryHints::MemoryUsage`) instead of fast ones,
/// for apps that are only UI. Call before the first [`Gpu`] is made.
pub fn set_low_memory(on: bool) {
    LOW_MEMORY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The GPU device and queue every renderer shares.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// A GPU with no window, for tests and tools. Falls back to a software adapter
    /// (WARP / llvmpipe) when no hardware GPU is available.
    pub fn headless() -> Result<Self, RenderError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = request_adapter(&instance, None)?;
        Self::from_adapter(instance, adapter)
    }

    /// A GPU plus a surface for `target` (e.g. an `Arc<winit::window::Window>`).
    pub fn with_surface(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<(Self, WindowSurface), RenderError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(target)?;
        let adapter = request_adapter(&instance, Some(&surface))?;
        let gpu = Self::from_adapter(instance, adapter)?;
        let surface = WindowSurface::new(&gpu, surface, width, height);
        Ok((gpu, surface))
    }

    /// A surface for another window on this GPU (same instance, adapter and device), for games
    /// with more than one window. The renderers (`UiRenderer`, `WorldRenderer`) are shared by
    /// every window: draw into this surface with [`WindowSurface::present_with`].
    ///
    /// The surface is made with vsync off and secondary windows keep it off (do not call
    /// `set_vsync(true)`): with several FIFO swapchains presented in one thread some drivers wait
    /// a vblank per window and the frame rate divides by the window count. Only the main
    /// window follows `QualitySettings.vSyncCount`.
    ///
    /// `target` holds a clone of the window (an `Arc<winit::window::Window>`): **drop the
    /// returned surface before the OS window is destroyed** (`PlatformApp::window_closed`).
    ///
    /// Not covered by a test: wgpu cannot create a surface without a real window, so this is
    /// exercised by the app's multi-window path only.
    pub fn create_window_surface(
        &self,
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<WindowSurface, RenderError> {
        let surface = self.instance.create_surface(target)?;
        if !self.adapter.is_surface_supported(&surface) {
            return Err(RenderError::Unsupported(format!(
                "the GPU {} cannot present to this window",
                self.adapter_name()
            )));
        }
        Ok(WindowSurface::new(self, surface, width, height))
    }

    pub fn adapter_name(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
    }

    fn from_adapter(instance: wgpu::Instance, adapter: wgpu::Adapter) -> Result<Self, RenderError> {
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("platypus device"),
                // Lets MSAA use every sample count the GPU has (WebGPU only promises 1 and
                // 4), where it's offered.
                // Block-compressed (BC) textures when the GPU has them (desktop GPUs do):
                // cooked games ship them.
                required_features: adapter.features()
                    & (wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
                        | wgpu::Features::TEXTURE_COMPRESSION_BC),
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                // The default (Performance) reserves 32-64 MB blocks, ~240 MB for a UI-only
                // app; `platypus-app` asks for small ones ([`set_low_memory`]).
                memory_hints: if LOW_MEMORY.load(std::sync::atomic::Ordering::Relaxed) {
                    wgpu::MemoryHints::MemoryUsage
                } else {
                    wgpu::MemoryHints::default()
                },
                ..Default::default()
            }))?;
        tracing::info!(adapter = %adapter.get_info().name, "GPU ready");
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }
}

fn request_adapter(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, RenderError> {
    let request = |force_fallback_adapter| {
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter,
            compatible_surface: surface,
            ..Default::default()
        }))
    };
    match request(false) {
        Ok(adapter) => Ok(adapter),
        Err(e) => {
            tracing::warn!("no hardware adapter ({e}); trying software fallback");
            Ok(request(true)?)
        }
    }
}
