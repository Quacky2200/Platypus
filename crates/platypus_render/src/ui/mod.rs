// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Draws `platypus_ui` draw lists: rounded boxes, borders, gradients, box shadows,
//! blur filters, group opacity and backdrop blur (DESIGN.md §5.2).
//!
//! A frame is drawn into an internal canvas texture, then blitted to the target.
//! Plain boxes and shadows are batched into one instanced draw; each layer or
//! backdrop blur adds a few full-screen passes limited by scissor rects.
//!
//! Clips (`overflow`, `clipPath`) don't break batches: they go into a clip table
//! and every instance carries the index of the clip active when it was drawn. A
//! layer is clipped when it is composited, so a blur inside a clip is cut off by it.

mod atlas;
pub mod backend;
pub mod glyph_clocks;
pub mod glyphs;
mod gradient;
pub mod image_cache;
mod images;
mod pipelines;
pub mod plan;

use std::collections::HashMap;
use std::path::PathBuf;

use platypus_ui::DrawCmd;
use wgpu::util::DeviceExt;

use crate::{Color, Gpu};
use atlas::GlyphAtlas;
use glyphs::GlyphEntry;
use gradient::GpuGradient;
use image_cache::ImageCache;
pub use plan::{Blending, needs_backdrop};
use plan::{GpuClip, Instance, Step, blur_reach};

/// Format of the canvas and layer textures with `Blending::Linear`: sRGB storage, linear
/// blending (see [`canvas_format`]).
pub const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Largest blur kernel (taps either side) a single pass will use.
const MAX_BLUR_RADIUS: f32 = 128.0;

/// Where a frame is drawn.
pub struct RenderTarget<'a> {
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    /// Physical pixels.
    pub size: (u32, u32),
    /// Physical pixels per logical pixel (the window's scale factor).
    pub scale: f32,
}

/// Counts for the most recent frame (used by tests and future profiling).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Instanced draws of boxes and shadows.
    pub draw_calls: u32,
    /// Render passes of any kind (including blur, composite and blit).
    pub passes: u32,
    pub layers: u32,
    pub instances: u32,
    /// Clip table entries (`overflow`, `clipPath`, backdrop blurs).
    pub clips: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewport: [f32; 2],
    scale: f32,
    /// 1 when the canvases hold sRGB values (`Blending::Srgb`).
    srgb: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurParams {
    dir: [f32; 2],
    sigma: f32,
    radius: f32,
    /// Reads stay in these pixels: first x, first y, last x, last y.
    bounds: [f32; 4],
    /// 1 to reflect reads past the bounds back in (`backdrop-filter`); 0 to clamp them.
    mirror: f32,
    _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeParams {
    opacity: f32,
    scale: f32,
    /// Clip table index (1-based, 0 = none).
    clip: f32,
    /// Blend mode (`BlendMode::index`); 0 composites normally.
    blend: f32,
    /// `filter` colour matrix, as WGSL columns, and its offset column.
    matrix: [[f32; 4]; 4],
    offset: [f32; 4],
    /// Drop shadows draw the source's alpha in this colour (linear, premultiplied).
    tint: [f32; 4],
    /// xy: shift of the source (physical px); z: 1 to apply the matrix; w: 1 to tint.
    shift: [f32; 4],
    /// x: mask mode (0 none, 1 alpha, 2 luminance); y: 1 when the canvases hold sRGB values.
    mask: [f32; 4],
}

struct PooledTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    format: wgpu::TextureFormat,
}

/// The format of the canvas and layer textures for `blending`: plain 8-bit values blended as
/// they are (sRGB), or sRGB storage blended in linear light.
pub fn canvas_format(blending: Blending) -> wgpu::TextureFormat {
    match blending {
        Blending::Srgb => wgpu::TextureFormat::Rgba8Unorm,
        Blending::Linear => CANVAS_FORMAT,
    }
}

pub struct UiRenderer {
    /// How translucent colours combine (docs/rendering.md "Blending").
    blending: Blending,
    sources: pipelines::Sources,
    /// The pipelines drawing into canvases, per blending (made when first used).
    canvas_pipelines: HashMap<Blending, pipelines::CanvasPipelines>,
    image_samplers: images::ImageSamplers,
    images: ImageCache,
    image_textures: images::ImageTextures,
    atlas: GlyphAtlas,
    /// When each glyph of animated text was first shown, and the clock for `text-effect`s.
    glyph_clocks: glyph_clocks::GlyphClocks,
    blit_pipeline_layout: wgpu::PipelineLayout,
    /// By target format, whether the canvas blends over the target (overlay), and the
    /// conversion on the way.
    blit_pipelines: HashMap<(wgpu::TextureFormat, bool, pipelines::Blit), wgpu::RenderPipeline>,
    sampler: wgpu::Sampler,
    globals: wgpu::Buffer,
    instances: wgpu::Buffer,
    gradients: wgpu::Buffer,
    clips: wgpu::Buffer,
    pool: Vec<PooledTexture>,
    /// The texture the world is drawn into when the UI needs to read it.
    backdrop: Option<(wgpu::TextureView, (u32, u32), wgpu::TextureFormat)>,
}

impl UiRenderer {
    /// Pins the clock `text-effect`s read (seconds), for repeatable renders; `None` goes back
    /// to real time.
    pub fn set_effect_time(&mut self, time: Option<f32>) {
        self.glyph_clocks.set_time(time);
    }

    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let quad_module = device.create_shader_module(wgpu::include_wgsl!("quad.wgsl"));
        let effects_module = device.create_shader_module(wgpu::include_wgsl!("effects.wgsl"));

        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let storage = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };

        let quad_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui quad"),
            entries: &[
                uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    ..texture
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    ..texture
                },
                storage(4, wgpu::ShaderStages::VERTEX_FRAGMENT),
            ],
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui blur"),
            entries: &[
                texture,
                sampler_entry,
                uniform(2, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui composite"),
            entries: &[
                texture,
                sampler_entry,
                uniform(3, wgpu::ShaderStages::FRAGMENT),
                storage(4, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    ..texture
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    ..texture
                },
            ],
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui blit"),
            entries: &[texture, sampler_entry],
        });

        let image_sampler = |label, filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        let image_samplers = images::ImageSamplers {
            smooth: image_sampler("ui images", wgpu::FilterMode::Linear),
            nearest: image_sampler("ui images (pixelated)", wgpu::FilterMode::Nearest),
        };
        let sources = pipelines::Sources {
            quad_module,
            effects_module,
            quad_layout,
            blur_layout,
            composite_layout,
            blit_layout,
        };
        let blit_pipeline_layout = sources.pipeline_layout(device, &[&sources.blit_layout]);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instances = Self::buffer(
            gpu,
            "ui instances",
            1024 * std::mem::size_of::<Instance>(),
            wgpu::BufferUsages::VERTEX,
        );
        let gradients = Self::buffer(
            gpu,
            "ui gradients",
            16 * std::mem::size_of::<GpuGradient>(),
            wgpu::BufferUsages::STORAGE,
        );
        let clips = Self::buffer(
            gpu,
            "ui clips",
            16 * std::mem::size_of::<GpuClip>(),
            wgpu::BufferUsages::STORAGE,
        );

        Self {
            blending: Blending::default(),
            sources,
            canvas_pipelines: HashMap::new(),
            image_samplers,
            images: ImageCache::default(),
            image_textures: images::ImageTextures::default(),
            atlas: GlyphAtlas::new(gpu),
            glyph_clocks: glyph_clocks::GlyphClocks::default(),
            blit_pipeline_layout,
            blit_pipelines: HashMap::new(),
            sampler,
            globals,
            instances,
            gradients,
            clips,
            pool: Vec::new(),
            backdrop: None,
        }
    }

    fn buffer(gpu: &Gpu, label: &str, size: usize, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size as u64,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// How translucent colours combine from the next frame on (`Blending::Srgb` by default, as
    /// browsers; docs/rendering.md "Blending").
    pub fn set_blending(&mut self, blending: Blending) {
        self.blending = blending;
    }

    pub fn blending(&self) -> Blending {
        self.blending
    }

    /// Folder that `url(...)` background images are loaded from (the game folder).
    /// URLs must be relative paths inside it.
    pub fn set_asset_root(&mut self, root: impl Into<PathBuf>) {
        self.images.set_root(root.into());
    }

    /// Loads the image at `url` (as `image` elements and `url(...)` use it) and returns
    /// its natural size in pixels, or `None` if it can't be loaded.
    pub fn image_size(&mut self, gpu: &Gpu, url: &str) -> Option<(u32, u32)> {
        let size = self.images.image(url).map(|(_, size)| size);
        self.image_textures.apply(
            gpu,
            &self.sources.blit_layout,
            &self.image_samplers,
            &mut self.images,
        );
        size
    }

    /// Draws `cmds` over `clear` into `target`.
    pub fn render(
        &mut self,
        gpu: &Gpu,
        target: &RenderTarget<'_>,
        clear: Color,
        cmds: &[DrawCmd],
    ) -> RenderStats {
        self.render_with(gpu, target, clear, cmds, false, None)
    }

    /// Draws `cmds` over what `target` already holds (screen UI over the 3D world):
    /// the canvas starts transparent and is blended onto the target. Backdrop filters
    /// and blend modes only see other UI this way; [`UiRenderer::render_over`] gives
    /// them the world. The blend onto a target that encodes sRGB on write happens in
    /// linear light whatever the blending (the hardware's); `render_over` is exact.
    pub fn render_overlay(
        &mut self,
        gpu: &Gpu,
        target: &RenderTarget<'_>,
        cmds: &[DrawCmd],
    ) -> RenderStats {
        let clear = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        self.render_with(gpu, target, clear, cmds, true, None)
    }

    /// Draws `cmds` over `backdrop` (the 3D world, drawn into a texture from
    /// [`UiRenderer::backdrop_target`]) and puts the result in `target`. The canvas
    /// starts as a copy of the world, so `backdrop-filter` and blend modes work on it.
    pub fn render_over(
        &mut self,
        gpu: &Gpu,
        target: &RenderTarget<'_>,
        backdrop: &wgpu::TextureView,
        cmds: &[DrawCmd],
    ) -> RenderStats {
        let clear = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        self.render_with(gpu, target, clear, cmds, false, Some(backdrop))
    }

    /// A texture to draw the world into (of the target's size and format) when the UI
    /// over it needs to read it (see [`needs_backdrop`]); kept between frames.
    pub fn backdrop_target(
        &mut self,
        gpu: &Gpu,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> wgpu::TextureView {
        if let Some((view, s, f)) = &self.backdrop
            && *s == size
            && *f == format
        {
            return view.clone();
        }
        let view = gpu
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("ui backdrop"),
                size: wgpu::Extent3d {
                    width: size.0.max(1),
                    height: size.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        self.backdrop = Some((view.clone(), size, format));
        view
    }

    fn render_with(
        &mut self,
        gpu: &Gpu,
        target: &RenderTarget<'_>,
        clear: Color,
        cmds: &[DrawCmd],
        overlay: bool,
        backdrop: Option<&wgpu::TextureView>,
    ) -> RenderStats {
        let (size, scale) = (target.size, target.scale);
        let mut stats = RenderStats::default();
        if size.0 == 0 || size.1 == 0 {
            return stats;
        }
        self.images.begin_frame();

        self.atlas.begin_frame();
        self.glyph_clocks.begin_frame();

        // --- Plan: the backend-neutral half (plan.rs). ---
        let plan = plan::plan(
            cmds,
            &plan::PlanParams {
                size,
                scale,
                clear,
                seeded: backdrop.is_some(),
                blending: self.blending,
            },
            &mut WgpuResources {
                atlas: &mut self.atlas,
                images: &mut self.images,
                clocks: &mut self.glyph_clocks,
            },
        );
        let plan::FramePlan {
            instances,
            mut gradients,
            clips: mut clip_table,
            steps,
            textures,
            layers,
            blending,
        } = plan;
        let srgb = blending == Blending::Srgb;
        let format = canvas_format(blending);
        if !self.canvas_pipelines.contains_key(&blending) {
            let made = self.sources.canvas(&gpu.device, format);
            self.canvas_pipelines.insert(blending, made);
        }
        stats.layers = layers;
        stats.clips = clip_table.len() as u32;
        // The glyphs and images the plan loaded reach their textures before the frame's draws.
        self.atlas.flush(gpu);
        self.image_textures.apply(
            gpu,
            &self.sources.blit_layout,
            &self.image_samplers,
            &mut self.images,
        );
        let frame_textures: Vec<PooledTexture> = (0..textures)
            .map(|_| self.take_texture(gpu, size, format))
            .collect();

        // --- Upload. ---
        gpu.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals {
                viewport: [size.0 as f32, size.1 as f32],
                scale,
                srgb: f32::from(u8::from(srgb)),
            }),
        );
        if !instances.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&instances);
            if self.instances.size() < bytes.len() as u64 {
                self.instances = Self::buffer(
                    gpu,
                    "ui instances",
                    bytes.len().next_power_of_two(),
                    wgpu::BufferUsages::VERTEX,
                );
            }
            gpu.queue.write_buffer(&self.instances, 0, bytes);
        }
        if gradients.is_empty() {
            gradients.push(GpuGradient::default()); // storage bindings can't be empty
        }
        let gradient_bytes: &[u8] = bytemuck::cast_slice(&gradients);
        if self.gradients.size() < gradient_bytes.len() as u64 {
            self.gradients = Self::buffer(
                gpu,
                "ui gradients",
                gradient_bytes.len().next_power_of_two(),
                wgpu::BufferUsages::STORAGE,
            );
        }
        gpu.queue.write_buffer(&self.gradients, 0, gradient_bytes);
        if clip_table.is_empty() {
            clip_table.push(GpuClip::default()); // storage bindings can't be empty
        }
        let clip_bytes: &[u8] = bytemuck::cast_slice(&clip_table);
        if self.clips.size() < clip_bytes.len() as u64 {
            self.clips = Self::buffer(
                gpu,
                "ui clips",
                clip_bytes.len().next_power_of_two(),
                wgpu::BufferUsages::STORAGE,
            );
        }
        gpu.queue.write_buffer(&self.clips, 0, clip_bytes);
        stats.instances = instances.len() as u32;

        // --- Record. ---
        let device = &gpu.device;
        let quad_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui quad"),
            layout: &self.sources.quad_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.gradients.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(self.atlas.mask_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(self.atlas.color_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.clips.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ui frame"),
        });
        let mut pending_clear: HashMap<usize, wgpu::Color> = HashMap::new();
        // Over the world: the canvas starts as a copy of it (instead of its clear).
        if let Some(world) = backdrop {
            // An sRGB canvas gets the world's encoded values (it is read decoded).
            let conversion = if srgb {
                pipelines::Blit::Encode
            } else {
                pipelines::Blit::Copy
            };
            let seed = self.blit_pipeline(gpu, format, false, conversion);
            let bind_group = self.texture_bind_group(gpu, &self.sources.blit_layout, world, None);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui backdrop seed"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_textures[0].view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&seed);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
            stats.passes += 1;
        }
        let load_for =
            |pending_clear: &mut HashMap<usize, wgpu::Color>, target: usize| match pending_clear
                .remove(&target)
            {
                Some(c) => wgpu::LoadOp::Clear(c),
                None => wgpu::LoadOp::Load,
            };
        let attachment = |view, load| wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        };
        // Applies any clear still pending on `target` before it's read.
        let settle = |encoder: &mut wgpu::CommandEncoder,
                      pending_clear: &mut HashMap<usize, wgpu::Color>,
                      stats: &mut RenderStats,
                      target: usize| {
            if let Some(c) = pending_clear.remove(&target) {
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ui clear"),
                    color_attachments: &[Some(attachment(
                        &frame_textures[target].view,
                        wgpu::LoadOp::Clear(c),
                    ))],
                    ..Default::default()
                });
                stats.passes += 1;
            }
        };

        let pipes = &self.canvas_pipelines[&blending];
        for step in &steps {
            match step {
                Step::Clear { target, color } => {
                    pending_clear.insert(*target, (*color).into());
                }
                Step::Draw {
                    target,
                    range,
                    image,
                } => {
                    let image_bind_group = match image {
                        Some((key, pixelated)) => match self.image_textures.get(key) {
                            Some(loaded) if *pixelated => Some(&loaded.bind_group_nearest),
                            Some(loaded) => Some(&loaded.bind_group),
                            None => continue,
                        },
                        None => None,
                    };
                    let load = load_for(&mut pending_clear, *target);
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("ui quads"),
                        color_attachments: &[Some(attachment(&frame_textures[*target].view, load))],
                        ..Default::default()
                    });
                    match image_bind_group {
                        Some(bind_group) => {
                            pass.set_pipeline(&pipes.image);
                            pass.set_bind_group(1, bind_group, &[]);
                        }
                        None => pass.set_pipeline(&pipes.quad),
                    }
                    pass.set_bind_group(0, &quad_bind_group, &[]);
                    pass.set_vertex_buffer(0, self.instances.slice(..));
                    pass.draw(0..6, range.clone());
                    stats.draw_calls += 1;
                    stats.passes += 1;
                }
                Step::Copy { src, dst } => {
                    settle(&mut encoder, &mut pending_clear, &mut stats, *src);
                    pending_clear.remove(dst);
                    encoder.copy_texture_to_texture(
                        frame_textures[*src].texture.as_image_copy(),
                        frame_textures[*dst].texture.as_image_copy(),
                        frame_textures[*src].texture.size(),
                    );
                }
                Step::Blur {
                    tex,
                    temp,
                    area,
                    sigma,
                    contained,
                } => {
                    settle(&mut encoder, &mut pending_clear, &mut stats, *tex);
                    let radius = (3.0 * sigma).ceil().min(MAX_BLUR_RADIUS);
                    let (tall, bounds) = blur_reach(area, radius, *contained, size);
                    for (src, dst, dir, scissor, load) in [
                        (
                            *tex,
                            *temp,
                            [1.0, 0.0],
                            tall,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        ),
                        (*temp, *tex, [0.0, 1.0], *area, wgpu::LoadOp::Load),
                    ] {
                        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("ui blur params"),
                            contents: bytemuck::bytes_of(&BlurParams {
                                dir,
                                sigma: sigma.max(0.01),
                                radius,
                                bounds,
                                mirror: f32::from(*contained),
                                _pad: [0.0; 3],
                            }),
                            usage: wgpu::BufferUsages::UNIFORM,
                        });
                        let bind_group = self.texture_bind_group(
                            gpu,
                            &self.sources.blur_layout,
                            &frame_textures[src].view,
                            Some((2, &params)),
                        );
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("ui blur"),
                            color_attachments: &[Some(attachment(&frame_textures[dst].view, load))],
                            ..Default::default()
                        });
                        pass.set_pipeline(&pipes.blur);
                        pass.set_bind_group(0, &bind_group, &[]);
                        pass.set_scissor_rect(scissor.x, scissor.y, scissor.w, scissor.h);
                        pass.draw(0..3, 0..1);
                        stats.passes += 1;
                    }
                }
                Step::Composite {
                    src,
                    dst,
                    area,
                    opacity,
                    clip,
                    blend,
                    matrix,
                    tint,
                    shift,
                    mask,
                } => {
                    settle(&mut encoder, &mut pending_clear, &mut stats, *src);
                    if let Some((m, _)) = mask {
                        settle(&mut encoder, &mut pending_clear, &mut stats, *m);
                    }
                    if let Some((_, backdrop)) = blend {
                        settle(&mut encoder, &mut pending_clear, &mut stats, *backdrop);
                    }
                    let backdrop = blend.map_or(*src, |(_, b)| b);
                    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("ui composite params"),
                        contents: bytemuck::bytes_of(&CompositeParams {
                            opacity: *opacity,
                            scale,
                            clip: *clip as f32,
                            blend: blend.map_or(0.0, |(mode, _)| mode as f32),
                            matrix: {
                                let m = matrix.unwrap_or_default();
                                // Row-major 4×5 to WGSL columns.
                                [0, 1, 2, 3].map(|col| [0, 1, 2, 3].map(|row| m[row * 5 + col]))
                            },
                            offset: {
                                let m = matrix.unwrap_or_default();
                                [m[4], m[9], m[14], m[19]]
                            },
                            tint: tint.unwrap_or_default(),
                            shift: [
                                shift[0],
                                shift[1],
                                f32::from(u8::from(matrix.is_some())),
                                f32::from(u8::from(tint.is_some())),
                            ],
                            mask: [
                                match mask {
                                    None => 0.0,
                                    Some((_, platypus_ui::MaskMode::Alpha)) => 1.0,
                                    Some((_, platypus_ui::MaskMode::Luminance)) => 2.0,
                                },
                                f32::from(u8::from(srgb)),
                                0.0,
                                0.0,
                            ],
                        }),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("ui composite"),
                        layout: &self.sources.composite_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(
                                    &frame_textures[*src].view,
                                ),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: params.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 4,
                                resource: self.clips.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 5,
                                resource: wgpu::BindingResource::TextureView(
                                    &frame_textures[backdrop].view,
                                ),
                            },
                            wgpu::BindGroupEntry {
                                binding: 6,
                                resource: wgpu::BindingResource::TextureView(
                                    &frame_textures[mask.map_or(*src, |(m, _)| m)].view,
                                ),
                            },
                        ],
                    });
                    let load = load_for(&mut pending_clear, *dst);
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("ui composite"),
                        color_attachments: &[Some(attachment(&frame_textures[*dst].view, load))],
                        ..Default::default()
                    });
                    pass.set_pipeline(if blend.is_some() {
                        &pipes.composite_blend
                    } else {
                        &pipes.composite
                    });
                    pass.set_bind_group(0, &bind_group, &[]);
                    pass.set_scissor_rect(area.x, area.y, area.w, area.h);
                    pass.draw(0..3, 0..1);
                    stats.passes += 1;
                }
            }
        }
        settle(&mut encoder, &mut pending_clear, &mut stats, 0);

        // --- Blit the canvas to the target. ---
        // An sRGB canvas is decoded for a target that encodes on write.
        let conversion = if srgb && target.format.is_srgb() {
            pipelines::Blit::Decode
        } else {
            pipelines::Blit::Copy
        };
        let blit = self.blit_pipeline(gpu, target.format, overlay, conversion);
        let bind_group = self.texture_bind_group(
            gpu,
            &self.sources.blit_layout,
            &frame_textures[0].view,
            None,
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui blit"),
                color_attachments: &[Some(attachment(
                    target.view,
                    if overlay {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                    },
                ))],
                ..Default::default()
            });
            pass.set_pipeline(&blit);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
            stats.passes += 1;
        }
        gpu.queue.submit([encoder.finish()]);

        // Commands are submitted in order, so textures can be reused next frame.
        self.pool.extend(frame_textures);
        stats
    }

    fn texture_bind_group(
        &self,
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        view: &wgpu::TextureView,
        uniform: Option<(u32, &wgpu::Buffer)>,
    ) -> wgpu::BindGroup {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
        ];
        if let Some((binding, buffer)) = uniform {
            entries.push(wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            });
        }
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &entries,
        })
    }

    fn blit_pipeline(
        &mut self,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        overlay: bool,
        conversion: pipelines::Blit,
    ) -> wgpu::RenderPipeline {
        self.blit_pipelines
            .entry((format, overlay, conversion))
            .or_insert_with(|| {
                gpu.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("ui blit"),
                        layout: Some(&self.blit_pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &self.sources.effects_module,
                            entry_point: Some("vs_fullscreen"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        fragment: Some(wgpu::FragmentState {
                            module: &self.sources.effects_module,
                            entry_point: Some(conversion.entry()),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                // The canvas is premultiplied.
                                blend: overlay
                                    .then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        multiview_mask: None,
                        cache: None,
                    })
            })
            .clone()
    }

    /// Takes a canvas texture of `size` and `format` from the pool, or creates one.
    fn take_texture(
        &mut self,
        gpu: &Gpu,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> PooledTexture {
        if let Some(i) = self
            .pool
            .iter()
            .position(|t| t.size == size && t.format == format)
        {
            return self.pool.swap_remove(i);
        }
        // Drop textures of other sizes or formats (the window was resized, the blending changed).
        self.pool.retain(|t| t.size == size && t.format == format);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui canvas"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        PooledTexture {
            texture,
            view,
            size,
            format,
        }
    }
}

/// The wgpu renderer's glyph atlas and image cache, for planning a frame (their pixels
/// reach the GPU after planning).
struct WgpuResources<'a> {
    atlas: &'a mut GlyphAtlas,
    images: &'a mut ImageCache,
    clocks: &'a mut glyph_clocks::GlyphClocks,
}

impl plan::PlanResources for WgpuResources<'_> {
    fn glyph_clocks(&mut self) -> &mut glyph_clocks::GlyphClocks {
        self.clocks
    }

    fn glyph(
        &mut self,
        run: &platypus_ui::GlyphRun,
        glyph: u32,
        size_px: f32,
        subpixel: u8,
        stroke_px: f32,
    ) -> Option<GlyphEntry> {
        self.atlas.glyph(run, glyph, size_px, subpixel, stroke_px)
    }

    fn image(&mut self, url: &str) -> Option<plan::ImageInfo> {
        self.images
            .image(url)
            .map(|(key, size)| plan::ImageInfo { size, key })
    }

    fn svg_at(&mut self, url: &str, px: (f32, f32)) -> Option<(String, (u32, u32))> {
        self.images.svg_at(url, false, px)
    }
}

impl backend::UiBackend for UiRenderer {
    type Context = Gpu;
    type Target<'a> = RenderTarget<'a>;

    fn set_asset_root(&mut self, root: PathBuf) {
        UiRenderer::set_asset_root(self, root);
    }

    fn set_blending(&mut self, blending: Blending) {
        UiRenderer::set_blending(self, blending);
    }

    fn image_size(&mut self, cx: &Gpu, url: &str) -> Option<(u32, u32)> {
        UiRenderer::image_size(self, cx, url)
    }

    fn render(
        &mut self,
        cx: &Gpu,
        target: &RenderTarget<'_>,
        clear: Color,
        cmds: &[DrawCmd],
    ) -> RenderStats {
        UiRenderer::render(self, cx, target, clear, cmds)
    }

    fn render_overlay(
        &mut self,
        cx: &Gpu,
        target: &RenderTarget<'_>,
        cmds: &[DrawCmd],
    ) -> RenderStats {
        UiRenderer::render_overlay(self, cx, target, cmds)
    }
}
