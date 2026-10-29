// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A GLES 3 executor for the UI frame plan (`platypus_render::plan`), for `platypus-app`: the same
//! draw list and plan as the wgpu renderer, drawn with GLES through EGL (ANGLE on Windows).
//!
//! [`GlesRenderer`] plans a draw list with `plan::plan`, then runs the plan: the instances go
//! in a vertex buffer, the gradient and clip tables in float data textures (GLES 3.0 has no
//! storage buffers), glyph pixels from the shared `GlyphCache` into two page textures, and the
//! `Clear` and `Draw` steps draw into canvases like wgpu's: plain `RGBA8` ones holding sRGB values
//! (`Blending::Srgb`, the default, blending as browsers do), or `SRGB8_ALPHA8` ones blending in
//! linear light (`Blending::Linear`). Images come from the shared `ImageCache` (decoded and rasterized without a
//! GPU), as `SRGB8_ALPHA8` textures. Layers run as the plan says: `Copy` (a framebuffer blit),
//! `Blur` (two passes of `effects.rs`' separable Gaussian) and `Composite` (opacity, clip,
//! `filter` matrix, drop-shadow tint and shift, mask, and the blend modes), which carry opacity
//! groups, blur, filters, drop shadows, masks and backdrop filters.

mod effects;
pub mod egl;
mod shaders;

/// Whether ANGLE is linked into this program (the `static-angle` feature, Windows) rather than
/// loaded from `libEGL.dll` at run time.
pub const STATIC_ANGLE: bool = cfg!(all(windows, feature = "static-angle"));

use std::collections::HashMap;
use std::path::PathBuf;

use glow::HasContext;
pub use platypus_render::Blending;
use platypus_render::glyphs::{COLOR_SIZE, GlyphCache, GlyphEntry, MASK_SIZE};
use platypus_render::image_cache::{ImageCache, ImageUpload};
use platypus_render::plan::{
    self, FramePlan, ImageInfo, Instance, PlanParams, PlanResources, PxRect, Step,
};
use platypus_render::{Color, RenderStats, UiBackend};
use platypus_ui::DrawCmd;

/// Floats per gradient table entry (`GpuGradient`: 12 `vec4`s) and per clip table entry
/// (`GpuClip`: 5), as texels of the data textures.
const GRADIENT_TEXELS: i32 = 12;
const CLIP_TEXELS: i32 = 5;
const INSTANCE_BYTES: i32 = std::mem::size_of::<Instance>() as i32;
/// Largest blur kernel (taps either side) a single pass uses, as in the wgpu executor.
const MAX_BLUR_RADIUS: f32 = 128.0;

/// Where a frame is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    /// The framebuffer (`None`: the window's default one).
    pub framebuffer: Option<glow::Framebuffer>,
    /// Physical pixels.
    pub size: (u32, u32),
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// Whether the framebuffer encodes sRGB on write (an `SRGB8_ALPHA8` attachment does; a
    /// window's default framebuffer usually doesn't).
    pub srgb: bool,
}

/// An offscreen texture of the frame's size a step draws into.
struct Canvas {
    texture: glow::Texture,
    framebuffer: glow::Framebuffer,
}

/// The renderer's GL programs. Each is compiled the first time a frame needs it: through ANGLE
/// every one is HLSL compiled by `d3dcompiler` and the driver, and a page of text or boxes needs
/// only `Quad` and `Blit` (the others cost memory the driver keeps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramKind {
    Quad,
    Image,
    Blur,
    Composite,
    CompositeBlend,
    Blit,
}

impl ProgramKind {
    pub const ALL: [ProgramKind; 6] = [
        ProgramKind::Quad,
        ProgramKind::Image,
        ProgramKind::Blur,
        ProgramKind::Composite,
        ProgramKind::CompositeBlend,
        ProgramKind::Blit,
    ];

    /// The shaders' sources: (vertex, fragment).
    fn sources(self) -> (String, String) {
        match self {
            ProgramKind::Quad => (shaders::quad_vertex(), shaders::quad_fragment()),
            ProgramKind::Image => (shaders::quad_vertex(), shaders::image_fragment()),
            ProgramKind::Blur => (shaders::BLIT_VERTEX.into(), effects::BLUR_FRAGMENT.into()),
            ProgramKind::Composite => (shaders::BLIT_VERTEX.into(), effects::composite_fragment()),
            ProgramKind::CompositeBlend => (
                shaders::BLIT_VERTEX.into(),
                effects::composite_blend_fragment(),
            ),
            ProgramKind::Blit => (shaders::BLIT_VERTEX.into(), shaders::BLIT_FRAGMENT.into()),
        }
    }
}

/// The programs compiled so far, by kind (compiled once, on first use).
#[derive(Default)]
struct Programs {
    slots: [Option<glow::Program>; ProgramKind::ALL.len()],
    /// Kinds whose compile failed (logged once; their steps are skipped).
    failed: [bool; ProgramKind::ALL.len()],
}

impl Programs {
    fn get(&mut self, gl: &glow::Context, kind: ProgramKind) -> Option<glow::Program> {
        let i = kind as usize;
        if self.slots[i].is_none() && !self.failed[i] {
            let (v, f) = kind.sources();
            match compile(gl, &v, &f) {
                Ok(p) => self.slots[i] = Some(p),
                Err(e) => {
                    tracing::error!("GLES renderer: the {kind:?} program: {e}");
                    self.failed[i] = true;
                }
            }
        }
        self.slots[i]
    }

    fn compiled(&self) -> Vec<ProgramKind> {
        ProgramKind::ALL
            .into_iter()
            .filter(|k| self.slots[*k as usize].is_some())
            .collect()
    }
}

pub struct GlesRenderer {
    gl: glow::Context,
    programs: Programs,
    vao: glow::VertexArray,
    instances: glow::Buffer,
    clip_table: glow::Texture,
    gradient_table: glow::Texture,
    glyph_mask: glow::Texture,
    /// The colour glyph page (emoji): 1x1 until the first colour glyph arrives.
    glyph_color: glow::Texture,
    glyph_color_sized: bool,
    canvases: Vec<Canvas>,
    canvas_size: (u32, u32),
    /// What the canvases hold (their format follows it).
    canvas_blending: Blending,
    /// How translucent colours combine (docs/rendering.md "Blending").
    blending: Blending,
    glyphs: GlyphCache,
    images: ImageCache,
    /// Image textures by the cache's key.
    image_textures: HashMap<String, glow::Texture>,
}

/// What planning asks of this renderer: glyphs and images from the shared caches (their pixels
/// reach the textures in `execute`).
struct Resources<'a> {
    glyphs: &'a mut GlyphCache,
    images: &'a mut ImageCache,
}

impl PlanResources for Resources<'_> {
    fn glyph(
        &mut self,
        run: &platypus_ui::GlyphRun,
        glyph: u32,
        size_px: f32,
        subpixel: u8,
        stroke_px: f32,
    ) -> Option<GlyphEntry> {
        self.glyphs.glyph(run, glyph, size_px, subpixel, stroke_px)
    }

    fn image(&mut self, url: &str) -> Option<ImageInfo> {
        self.images
            .image(url)
            .map(|(key, size)| ImageInfo { size, key })
    }

    fn svg_at(&mut self, url: &str, px: (f32, f32)) -> Option<(String, (u32, u32))> {
        self.images.svg_at(url, false, px)
    }
}

fn compile(gl: &glow::Context, vertex: &str, fragment: &str) -> Result<glow::Program, String> {
    unsafe {
        let program = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, source) in [
            (glow::VERTEX_SHADER, vertex),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let log = gl.get_shader_info_log(shader);
                return Err(format!("GLSL compile error: {log}"));
            }
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(format!(
                "GLSL link error: {}",
                gl.get_program_info_log(program)
            ));
        }
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        Ok(program)
    }
}

/// A page-sized texture the glyph atlas fills.
unsafe fn page(gl: &glow::Context, size: u32, internal: u32) -> Result<glow::Texture, String> {
    unsafe {
        let texture = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_storage_2d(glow::TEXTURE_2D, 1, internal, size as i32, size as i32);
        for (p, v) in [
            (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
            (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
            (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
        }
        Ok(texture)
    }
}

/// An empty float data texture (nearest, clamped), to be sized by the frame.
unsafe fn table(gl: &glow::Context) -> Result<glow::Texture, String> {
    unsafe {
        let texture = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        for (p, v) in [
            (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
            (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
            (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
        }
        Ok(texture)
    }
}

impl GlesRenderer {
    /// A renderer on the current GL context (`gl`, from [`egl::EglContext::gl`]).
    pub fn new(gl: glow::Context) -> Result<Self, String> {
        unsafe {
            // The two programs every frame needs, now: a context that can't build them is no
            // renderer (the host falls back). The others wait for a frame that uses them.
            let mut programs = Programs::default();
            for kind in [ProgramKind::Quad, ProgramKind::Blit] {
                let (v, f) = kind.sources();
                programs.slots[kind as usize] = Some(compile(&gl, &v, &f)?);
            }
            let vao = gl.create_vertex_array()?;
            let instances = gl.create_buffer()?;
            let clip_table = table(&gl)?;
            let gradient_table = table(&gl)?;
            let glyph_mask = page(&gl, MASK_SIZE, glow::R8)?;
            let glyph_color = page(&gl, 1, glow::SRGB8_ALPHA8)?;
            Ok(Self {
                gl,
                programs,
                vao,
                instances,
                clip_table,
                gradient_table,
                glyph_mask,
                glyph_color,
                glyph_color_sized: false,
                canvases: Vec::new(),
                canvas_size: (0, 0),
                canvas_blending: Blending::default(),
                blending: Blending::default(),
                glyphs: GlyphCache::new(),
                images: ImageCache::default(),
                image_textures: HashMap::new(),
            })
        }
    }

    /// How translucent colours combine from the next frame on (`Blending::Srgb` by default, as
    /// browsers; docs/rendering.md "Blending").
    pub fn set_blending(&mut self, blending: Blending) {
        self.blending = blending;
    }

    pub fn blending(&self) -> Blending {
        self.blending
    }

    /// The programs compiled so far (they compile on first use).
    pub fn compiled_programs(&self) -> Vec<ProgramKind> {
        self.programs.compiled()
    }

    /// Whether the colour glyph page has its full size (it does once a colour glyph was drawn).
    pub fn has_color_page(&self) -> bool {
        self.glyph_color_sized
    }

    /// Draws `cmds` over `clear` into `target`.
    pub fn render(&mut self, target: &Target, clear: Color, cmds: &[DrawCmd]) -> RenderStats {
        self.draw(target, clear, cmds, false)
    }

    /// Draws `cmds` over what `target` already holds (the canvas starts transparent and is
    /// blended on, premultiplied).
    pub fn render_overlay(&mut self, target: &Target, cmds: &[DrawCmd]) -> RenderStats {
        self.draw(target, Color::TRANSPARENT, cmds, true)
    }

    fn draw(
        &mut self,
        target: &Target,
        clear: Color,
        cmds: &[DrawCmd],
        overlay: bool,
    ) -> RenderStats {
        if target.size.0 == 0 || target.size.1 == 0 {
            return RenderStats::default();
        }
        self.glyphs.begin_frame();
        self.images.begin_frame();
        let mut resources = Resources {
            glyphs: &mut self.glyphs,
            images: &mut self.images,
        };
        let frame = plan::plan(
            cmds,
            &PlanParams {
                size: target.size,
                scale: target.scale,
                clear,
                seeded: false,
                blending: self.blending,
            },
            &mut resources,
        );
        self.run(target, &frame, overlay)
    }

    /// The pixels of `framebuffer` (`None`: the window's back buffer), RGBA, top row first.
    pub fn read_back(&self, framebuffer: Option<glow::Framebuffer>, size: (u32, u32)) -> Vec<u8> {
        let gl = &self.gl;
        let mut bottom_up = vec![0u8; (size.0 * size.1 * 4) as usize];
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, framebuffer);
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                0,
                0,
                size.0 as i32,
                size.1 as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut bottom_up)),
            );
        }
        let row = (size.0 * 4) as usize;
        bottom_up
            .chunks_exact(row)
            .rev()
            .flatten()
            .copied()
            .collect()
    }

    /// (Re)creates `count` canvases of `size` holding `blending`'s values, keeping the ones that fit.
    fn canvases(
        &mut self,
        count: usize,
        size: (u32, u32),
        blending: Blending,
    ) -> Result<(), String> {
        if self.canvas_size != size || self.canvas_blending != blending {
            unsafe {
                for c in self.canvases.drain(..) {
                    self.gl.delete_framebuffer(c.framebuffer);
                    self.gl.delete_texture(c.texture);
                }
            }
            self.canvas_size = size;
            self.canvas_blending = blending;
        }
        let format = match blending {
            Blending::Srgb => glow::RGBA8,
            Blending::Linear => glow::SRGB8_ALPHA8,
        };
        while self.canvases.len() < count {
            unsafe {
                let gl = &self.gl;
                let texture = gl.create_texture()?;
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.tex_storage_2d(glow::TEXTURE_2D, 1, format, size.0 as i32, size.1 as i32);
                for (p, v) in [
                    (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                    (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                    (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                    (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
                ] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
                }
                let framebuffer = gl.create_framebuffer()?;
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                    return Err("the canvas framebuffer is incomplete".into());
                }
                self.canvases.push(Canvas {
                    texture,
                    framebuffer,
                });
            }
        }
        Ok(())
    }

    /// Writes the glyph pixels placed since the last frame into the page textures.
    fn upload_glyphs(&mut self) {
        let uploads = self.glyphs.take_uploads();
        if !self.glyph_color_sized && uploads.iter().any(|u| u.color) {
            // The first colour glyph: the page gets its real size.
            match unsafe { page(&self.gl, COLOR_SIZE, glow::SRGB8_ALPHA8) } {
                Ok(t) => {
                    unsafe { self.gl.delete_texture(self.glyph_color) };
                    self.glyph_color = t;
                    self.glyph_color_sized = true;
                }
                Err(e) => tracing::error!("GLES renderer: the colour glyph page: {e}"),
            }
        }
        let gl = &self.gl;
        unsafe {
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            for u in uploads {
                if u.color && !self.glyph_color_sized {
                    continue;
                }
                let (texture, format) = if u.color {
                    (self.glyph_color, glow::RGBA)
                } else {
                    (self.glyph_mask, glow::RED)
                };
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    u.x as i32,
                    u.y as i32,
                    u.width as i32,
                    u.height as i32,
                    format,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&u.data)),
                );
            }
        }
    }

    /// Creates the textures of images loaded since the last frame and drops the ones the cache
    /// let go of.
    fn upload_images(&mut self) {
        let gl = &self.gl;
        for key in self.images.take_dropped() {
            if let Some(texture) = self.image_textures.remove(&key) {
                unsafe { gl.delete_texture(texture) };
            }
        }
        for ImageUpload {
            key,
            width,
            height,
            pixels,
        } in self.images.take_uploads()
        {
            unsafe {
                let Ok(texture) = gl.create_texture() else {
                    continue;
                };
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::SRGB8_ALPHA8 as i32,
                    width as i32,
                    height as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&pixels)),
                );
                // Images repeat (the shader clamps where they shouldn't); the filter follows
                // `image-rendering` at each draw.
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
                if let Some(old) = self.image_textures.insert(key, texture) {
                    gl.delete_texture(old);
                }
            }
        }
    }

    /// Points a quad-family program at the frame's globals and tables.
    /// `srgb`: the canvases hold sRGB values.
    unsafe fn bind_quad_uniforms(
        &self,
        program: glow::Program,
        size: (u32, u32),
        scale: f32,
        srgb: bool,
    ) {
        let gl = &self.gl;
        unsafe {
            gl.use_program(Some(program));
            gl.uniform_4_f32(
                gl.get_uniform_location(program, "u_globals").as_ref(),
                size.0 as f32,
                size.1 as f32,
                scale,
                f32::from(u8::from(srgb)),
            );
            for (name, unit, texture) in [
                ("u_clips", 0, self.clip_table),
                ("u_grads", 1, self.gradient_table),
                ("u_mask", 2, self.glyph_mask),
                ("u_color", 3, self.glyph_color),
            ] {
                gl.active_texture(glow::TEXTURE0 + unit);
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.uniform_1_i32(gl.get_uniform_location(program, name).as_ref(), unit as i32);
            }
            gl.uniform_1_i32(gl.get_uniform_location(program, "u_image").as_ref(), 4);
        }
    }

    /// Applies a clear still pending on `canvas`, before something reads it.
    unsafe fn settle(&self, pending: &mut [Option<Color>], canvas: usize, stats: &mut RenderStats) {
        let Some(c) = pending[canvas].take() else {
            return;
        };
        let gl = &self.gl;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvases[canvas].framebuffer));
            gl.disable(glow::SCISSOR_TEST);
            gl.clear_color(c.r as f32, c.g as f32, c.b as f32, c.a as f32);
            gl.clear(glow::COLOR_BUFFER_BIT);
        }
        stats.passes += 1;
    }

    /// Starts a full-screen pass into canvas `dst`, limited to `area`. `clear` wipes the whole
    /// canvas first (a wgpu load op does not stop at the scissor); `blend` is premultiplied
    /// source-over, else the pass replaces what it covers.
    unsafe fn begin_pass(
        &self,
        program: glow::Program,
        dst: usize,
        area: &PxRect,
        clear: Option<Color>,
        blend: bool,
        size: (u32, u32),
    ) {
        let gl = &self.gl;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvases[dst].framebuffer));
            gl.viewport(0, 0, size.0 as i32, size.1 as i32);
            gl.disable(glow::SCISSOR_TEST);
            if let Some(c) = clear {
                gl.clear_color(c.r as f32, c.g as f32, c.b as f32, c.a as f32);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }
            if blend {
                gl.enable(glow::BLEND);
                gl.blend_func_separate(
                    glow::ONE,
                    glow::ONE_MINUS_SRC_ALPHA,
                    glow::ONE,
                    glow::ONE_MINUS_SRC_ALPHA,
                );
            } else {
                gl.disable(glow::BLEND);
            }
            // The plan's rectangles are y down; GL's scissor starts at the bottom left.
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(
                area.x as i32,
                (size.1 - area.y - area.h) as i32,
                area.w as i32,
                area.h as i32,
            );
            gl.use_program(Some(program));
        }
    }

    /// Binds canvas `canvas` to texture `unit` and points the sampler `name` at it.
    unsafe fn bind_canvas(&self, program: glow::Program, name: &str, unit: u32, canvas: usize) {
        let gl = &self.gl;
        unsafe {
            gl.active_texture(glow::TEXTURE0 + unit);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.canvases[canvas].texture));
            gl.uniform_1_i32(gl.get_uniform_location(program, name).as_ref(), unit as i32);
        }
    }

    /// Runs a plan: uploads its tables, draws its steps into the canvases and copies canvas 0
    /// to the target.
    pub fn execute(&mut self, target: &Target, frame: &FramePlan) -> RenderStats {
        self.run(target, frame, false)
    }

    fn run(&mut self, target: &Target, frame: &FramePlan, overlay: bool) -> RenderStats {
        let mut stats = RenderStats {
            layers: frame.layers,
            instances: frame.instances.len() as u32,
            clips: frame.clips.len() as u32,
            ..RenderStats::default()
        };
        if let Err(e) = self.canvases(frame.textures, target.size, frame.blending) {
            tracing::error!("GLES renderer: {e}");
            return stats;
        }
        self.upload_glyphs();
        self.upload_images();
        let size = target.size;
        let gl = &self.gl;
        unsafe {
            // The instance buffer and the two tables.
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instances));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&frame.instances),
                glow::DYNAMIC_DRAW,
            );
            let default_gradient = [platypus_render::plan::GpuGradient::default()];
            let gradients: &[_] = if frame.gradients.is_empty() {
                &default_gradient
            } else {
                &frame.gradients
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(self.gradient_table));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA32F as i32,
                GRADIENT_TEXELS,
                gradients.len() as i32,
                0,
                glow::RGBA,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(bytemuck::cast_slice(gradients))),
            );
            let default_clip = [platypus_render::plan::GpuClip::default()];
            let clips: &[_] = if frame.clips.is_empty() {
                &default_clip
            } else {
                &frame.clips
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(self.clip_table));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA32F as i32,
                CLIP_TEXELS,
                clips.len() as i32,
                0,
                glow::RGBA,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(bytemuck::cast_slice(clips))),
            );

            // State: premultiplied alpha blending, nothing else.
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::SCISSOR_TEST);
            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instances));
        }

        let scale = target.scale;
        let srgb = frame.blending == Blending::Srgb;
        let mut pending_clear: Vec<Option<Color>> = vec![None; self.canvases.len()];
        for step in &frame.steps {
            match step {
                Step::Clear { target, color } => pending_clear[*target] = Some(*color),
                Step::Draw {
                    target,
                    range,
                    image,
                } => {
                    let (kind, texture, pixelated) = match image {
                        Some((key, pixelated)) => match self.image_textures.get(key) {
                            Some(t) => (ProgramKind::Image, Some(*t), *pixelated),
                            None => continue,
                        },
                        None => (ProgramKind::Quad, None, false),
                    };
                    let Some(program) = self.programs.get(&self.gl, kind) else {
                        continue;
                    };
                    let gl = &self.gl;
                    unsafe {
                        gl.disable(glow::SCISSOR_TEST);
                        gl.enable(glow::BLEND);
                        gl.blend_func_separate(
                            glow::ONE,
                            glow::ONE_MINUS_SRC_ALPHA,
                            glow::ONE,
                            glow::ONE_MINUS_SRC_ALPHA,
                        );
                        self.bind_quad_uniforms(program, size, scale, srgb);
                        if let Some(t) = texture {
                            gl.active_texture(glow::TEXTURE4);
                            gl.bind_texture(glow::TEXTURE_2D, Some(t));
                            let filter = if pixelated {
                                glow::NEAREST
                            } else {
                                glow::LINEAR
                            };
                            gl.tex_parameter_i32(
                                glow::TEXTURE_2D,
                                glow::TEXTURE_MIN_FILTER,
                                filter as i32,
                            );
                            gl.tex_parameter_i32(
                                glow::TEXTURE_2D,
                                glow::TEXTURE_MAG_FILTER,
                                filter as i32,
                            );
                        }
                        gl.bind_framebuffer(
                            glow::FRAMEBUFFER,
                            Some(self.canvases[*target].framebuffer),
                        );
                        gl.viewport(0, 0, size.0 as i32, size.1 as i32);
                        if let Some(c) = pending_clear[*target].take() {
                            gl.clear_color(c.r as f32, c.g as f32, c.b as f32, c.a as f32);
                            gl.clear(glow::COLOR_BUFFER_BIT);
                            stats.passes += 1;
                        }
                        // One vec4 attribute per instance field, read from this range.
                        for location in 0..9u32 {
                            gl.enable_vertex_attrib_array(location);
                            gl.vertex_attrib_pointer_f32(
                                location,
                                4,
                                glow::FLOAT,
                                false,
                                INSTANCE_BYTES,
                                range.start as i32 * INSTANCE_BYTES + location as i32 * 16,
                            );
                            gl.vertex_attrib_divisor(location, 1);
                        }
                        gl.draw_arrays_instanced(
                            glow::TRIANGLES,
                            0,
                            6,
                            (range.end - range.start) as i32,
                        );
                    }
                    stats.draw_calls += 1;
                    stats.passes += 1;
                }
                Step::Copy { src, dst } => unsafe {
                    self.settle(&mut pending_clear, *src, &mut stats);
                    pending_clear[*dst] = None;
                    let gl = &self.gl;
                    let (w, h) = (size.0 as i32, size.1 as i32);
                    gl.disable(glow::SCISSOR_TEST);
                    gl.bind_framebuffer(
                        glow::READ_FRAMEBUFFER,
                        Some(self.canvases[*src].framebuffer),
                    );
                    gl.bind_framebuffer(
                        glow::DRAW_FRAMEBUFFER,
                        Some(self.canvases[*dst].framebuffer),
                    );
                    // Same format both sides: the pixels are copied as they are.
                    gl.blit_framebuffer(
                        0,
                        0,
                        w,
                        h,
                        0,
                        0,
                        w,
                        h,
                        glow::COLOR_BUFFER_BIT,
                        glow::NEAREST,
                    );
                },
                Step::Blur {
                    tex,
                    temp,
                    area,
                    sigma,
                    contained,
                } => unsafe {
                    let Some(program) = self.programs.get(&self.gl, ProgramKind::Blur) else {
                        continue;
                    };
                    self.settle(&mut pending_clear, *tex, &mut stats);
                    pending_clear[*temp] = None;
                    let radius = (3.0 * sigma).ceil().min(MAX_BLUR_RADIUS);
                    // Horizontal into `temp`, then vertical back into `tex`.
                    let (tall, b) = plan::blur_reach(area, radius, *contained, size);
                    // The bounds are y down; the canvases are read bottom up.
                    let bottom = size.1 as f32 - 1.0;
                    let bounds = [b[0], bottom - b[3], b[2], bottom - b[1]];
                    for (from, to, dir, rect, clear) in [
                        (*tex, *temp, [1.0, 0.0], tall, Some(Color::TRANSPARENT)),
                        (*temp, *tex, [0.0, 1.0], *area, None),
                    ] {
                        self.begin_pass(program, to, &rect, clear, false, size);
                        self.bind_canvas(program, "u_src", 5, from);
                        let gl = &self.gl;
                        gl.uniform_4_f32(
                            gl.get_uniform_location(program, "u_blur").as_ref(),
                            dir[0],
                            dir[1],
                            sigma.max(0.01),
                            radius,
                        );
                        gl.uniform_4_f32_slice(
                            gl.get_uniform_location(program, "u_bounds").as_ref(),
                            &bounds,
                        );
                        gl.uniform_1_f32(
                            gl.get_uniform_location(program, "u_mirror").as_ref(),
                            f32::from(*contained),
                        );
                        gl.draw_arrays(glow::TRIANGLES, 0, 3);
                        stats.passes += 1;
                    }
                },
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
                } => unsafe {
                    let kind = if blend.is_some() {
                        ProgramKind::CompositeBlend
                    } else {
                        ProgramKind::Composite
                    };
                    let Some(program) = self.programs.get(&self.gl, kind) else {
                        continue;
                    };
                    self.settle(&mut pending_clear, *src, &mut stats);
                    if let Some((m, _)) = mask {
                        self.settle(&mut pending_clear, *m, &mut stats);
                    }
                    if let Some((_, backdrop)) = blend {
                        self.settle(&mut pending_clear, *backdrop, &mut stats);
                    }
                    let backdrop = blend.map_or(*src, |(_, b)| b);
                    let clear = pending_clear[*dst].take();
                    self.begin_pass(program, *dst, area, clear, blend.is_none(), size);
                    self.bind_quad_uniforms(program, size, scale, srgb);
                    self.bind_canvas(program, "u_src", 5, *src);
                    self.bind_canvas(program, "u_backdrop", 6, backdrop);
                    self.bind_canvas(program, "u_mask_tex", 7, mask.map_or(*src, |(m, _)| m));
                    let gl = &self.gl;
                    let loc = |name: &str| gl.get_uniform_location(program, name);
                    gl.uniform_4_f32(
                        loc("u_comp").as_ref(),
                        *opacity,
                        scale,
                        *clip as f32,
                        blend.map_or(0.0, |(mode, _)| mode as f32),
                    );
                    let m = matrix.unwrap_or_default();
                    // Row-major 4x5 to the columns of a mat4 and its offset column.
                    let columns: [f32; 16] = std::array::from_fn(|i| m[(i % 4) * 5 + i / 4]);
                    gl.uniform_matrix_4_f32_slice(loc("u_matrix").as_ref(), false, &columns);
                    gl.uniform_4_f32(loc("u_offset").as_ref(), m[4], m[9], m[14], m[19]);
                    let t = tint.unwrap_or_default();
                    gl.uniform_4_f32(loc("u_tint").as_ref(), t[0], t[1], t[2], t[3]);
                    gl.uniform_4_f32(
                        loc("u_shift").as_ref(),
                        shift[0],
                        shift[1],
                        f32::from(u8::from(matrix.is_some())),
                        f32::from(u8::from(tint.is_some())),
                    );
                    let mode = match mask {
                        None => 0.0,
                        Some((_, platypus_ui::MaskMode::Alpha)) => 1.0,
                        Some((_, platypus_ui::MaskMode::Luminance)) => 2.0,
                    };
                    gl.uniform_4_f32(loc("u_maskmode").as_ref(), mode, 0.0, 0.0, 0.0);
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                    stats.passes += 1;
                },
            }
        }
        let Some(blit) = self.programs.get(&self.gl, ProgramKind::Blit) else {
            return stats;
        };
        let gl = &self.gl;
        unsafe {
            // A canvas nothing drew into still shows its clear.
            self.settle(&mut pending_clear, 0, &mut stats);
            gl.disable(glow::SCISSOR_TEST);
            // The canvas, pixel for pixel, onto the target.
            gl.bind_framebuffer(glow::FRAMEBUFFER, target.framebuffer);
            gl.viewport(0, 0, size.0 as i32, size.1 as i32);
            if overlay {
                gl.enable(glow::BLEND);
                gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            } else {
                gl.disable(glow::BLEND);
            }
            gl.use_program(Some(blit));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.canvases[0].texture));
            gl.uniform_1_i32(gl.get_uniform_location(blit, "u_src").as_ref(), 0);
            // Encode a linear canvas for a target that doesn't on write; decode an sRGB one for
            // a target that does.
            let mode = match (srgb, target.srgb) {
                (false, false) => 1.0,
                (true, true) => 2.0,
                _ => 0.0,
            };
            gl.uniform_1_f32(gl.get_uniform_location(blit, "u_mode").as_ref(), mode);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            stats.passes += 1;
        }
        stats
    }
}

impl UiBackend for GlesRenderer {
    type Context = ();
    type Target<'a> = Target;

    fn set_asset_root(&mut self, root: PathBuf) {
        self.images.set_root(root);
    }

    fn set_blending(&mut self, blending: Blending) {
        GlesRenderer::set_blending(self, blending);
    }

    fn image_size(&mut self, _: &(), url: &str) -> Option<(u32, u32)> {
        self.images.image(url).map(|(_, size)| size)
    }

    fn render(&mut self, _: &(), target: &Target, clear: Color, cmds: &[DrawCmd]) -> RenderStats {
        GlesRenderer::render(self, target, clear, cmds)
    }

    fn render_overlay(&mut self, _: &(), target: &Target, cmds: &[DrawCmd]) -> RenderStats {
        GlesRenderer::render_overlay(self, target, cmds)
    }
}
