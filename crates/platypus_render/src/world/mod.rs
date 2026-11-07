// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The 3D world pass (M5): what the cameras see, drawn before the screen UI.
//!
//! The world is lit meshes ([`MeshInstance`], built-in primitives or meshes set with
//! [`WorldRenderer::set_mesh`]) and quads: coloured or textured planes, such as UI
//! panels in the world. Each camera is a [`WorldView`] with its own viewport and lights;
//! views are drawn in order, each first filling its viewport with its background (which
//! also resets depth there), so later cameras draw over earlier ones, like Unity's camera
//! depth. Opaque meshes go first, then see-through meshes and quads back to front, so
//! they blend over what's behind them. Textures are either the frame's (UI panels,
//! passed to each render) or kept by the renderer ([`WorldRenderer::set_texture`]).

pub mod bloom;
mod bone_pack;
pub mod cull;
pub mod environment;
mod fill;
mod fxaa;
pub mod mesh;
pub mod pipeline;
mod post;
mod shadows;
pub mod texture;
mod upscale;
mod volume;

use bytemuck::{Pod, Zeroable};
use platypus_core::glam::{Mat3, Mat4, Vec3, Vec4Swizzles};
use std::collections::HashMap;
use std::ops::Range;

pub use post::{POST_PARAMS, PostEffect, PostParamType, PostShader};

use crate::{Color, Gpu, RenderTarget};
pub use bloom::{
    AutoExposure, Bloom, ExposureReading, Grading, MAX_CURVE_POINTS, Metering, Tonemapping,
};
pub use environment::Sky;
pub use fill::{FillRule, tessellate_fill};
pub use mesh::{MeshData, primitive};
pub use pipeline::PipelineAsset;
use texture::SamplerKey;
pub use texture::{
    Anisotropic, CookedTexture, TexelFormat, TextureData, TextureFilter, TextureWrap, mip_chain,
};

/// A box in the world: its least and greatest corners.
type WorldBox = (Vec3, Vec3);

/// A texture a quad or mesh is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorldTexture {
    /// Index into the `textures` given to [`WorldRenderer::render_views`].
    Frame(usize),
    /// One set with [`WorldRenderer::set_texture`] (the missing checker until it exists).
    Asset(u32),
    /// A mirror's reflection, rendered by the renderer this frame (set internally).
    Reflection(usize),
}

/// A reflection probe (Unity's): a cubemap of the scene seen from `position`, which
/// meshes inside its box reflect instead of the sky.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReflectionProbe {
    /// Identifies it across frames (its cubemap is kept between renders).
    pub id: u32,
    pub position: Vec3,
    /// Half the size of its box of influence, which is centred on `position`.
    pub half_extents: Vec3,
    /// The cubemap's face size in texels (a power of two).
    pub resolution: u32,
    /// Reflections are corrected for the box's walls (rooms), not taken as infinitely far.
    pub box_projection: bool,
    /// Render it this frame (it's also rendered when first seen or resized).
    pub render: bool,
    /// The layers it captures, a bit each (Unity's `cullingMask`).
    pub culling_mask: u32,
}

/// A flat mirror: the plane a mesh reflects the scene in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mirror {
    /// The plane's normal, pointing out of the reflecting side (world space).
    pub normal: Vec3,
    /// A point on the plane.
    pub point: Vec3,
    /// The layers it reflects, a bit each.
    pub culling_mask: u32,
}

/// A culling mask with every layer.
pub const ALL_LAYERS: u32 = u32::MAX;

/// Whether `mask` has `layer`'s bit.
pub fn in_mask(mask: u32, layer: u8) -> bool {
    mask & (1u32 << (layer & 31)) != 0
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Render textures hold finished (graded) colours, like the screen.
pub const RENDER_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The open cone light beams are drawn with ([`mesh::beam`]).
pub const BEAM_MESH: u32 = PRIMITIVE_BASE + 0xFFFE;

/// How a surface covers what's behind it (Unity's Standard shader rendering modes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RenderMode {
    /// Solid. A colour alpha below 1 still draws it as [`RenderMode::Fade`] (scripts'
    /// see-through colours keep working without picking a mode).
    #[default]
    Opaque,
    /// Solid, with holes where the alpha (texture × colour) is below the cut-off: leaves,
    /// fences. Casts shadows with the same holes.
    Cutout,
    /// Everything fades with the alpha, highlights and reflections too: fading objects out.
    Fade,
    /// See-through, but highlights and reflections stay: glass, clear plastic.
    Transparent,
}

/// A material's textures besides the main one (Unity's Standard shader maps). Each is
/// sampled with the main texture's tiling, except the detail map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialMaps {
    /// A tangent-space normal map (green up), read as linear data.
    pub normal: Option<WorldTexture>,
    /// How strong the normal map's bumps are (Unity's `bumpScale`).
    pub normal_scale: f32,
    /// Metallic in red and smoothness in alpha (Unity's `metallicGlossMap`); with it the
    /// material's metallic is the map's and its smoothness scales the map's. With the
    /// specular workflow, the specular colour (rgb) and smoothness (a) instead.
    pub metallic_gloss: Option<WorldTexture>,
    /// Read `metallic_gloss` the glTF way: roughness in green, metallic in blue, both
    /// multiplying the material's values; occlusion in red.
    pub gltf_packing: bool,
    /// Ambient occlusion (green; red with glTF packing), darkening sky light and
    /// reflections only.
    pub occlusion: Option<WorldTexture>,
    pub occlusion_strength: f32,
    /// Multiplies the emission colour.
    pub emission: Option<WorldTexture>,
    /// Extra detail multiplied over the albedo (x2 in gamma space, Unity's
    /// `detailAlbedoMap`), tiled on its own.
    pub detail: Option<WorldTexture>,
    /// The detail map's tiling: scale (xy) and offset (zw).
    pub detail_tiling: [f32; 4],
    /// A mirror's reflection this frame (set by the renderer, not by callers).
    pub reflection: Option<WorldTexture>,
}

impl Default for MaterialMaps {
    fn default() -> Self {
        Self {
            normal: None,
            normal_scale: 1.0,
            metallic_gloss: None,
            gltf_packing: false,
            occlusion: None,
            occlusion_strength: 1.0,
            emission: None,
            detail: None,
            detail_tiling: [1.0, 1.0, 0.0, 0.0],
            reflection: None,
        }
    }
}

/// How a mesh adds light instead of covering what's behind it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeshGlow {
    /// Lit (or unlit) and opaque or see-through by its colour's alpha.
    #[default]
    None,
    /// Unlit, adds its colour × alpha (glowing shapes).
    Additive,
    /// Additive with soft edges and a fade towards the open end: a light beam.
    Beam,
}

/// A magenta and black checker, drawn wherever a texture is missing (never set, failed
/// to load, or removed), so missing assets stand out instead of passing as white.
pub const MISSING_TEXTURE: u32 = u32::MAX - 1;

/// A soft round spot every renderer keeps as a texture (particles, halos).
pub const DEFAULT_PARTICLE_TEXTURE: u32 = u32::MAX;

/// The built-in meshes, registered by every renderer at [`primitive_id`].
pub const PRIMITIVES: [&str; 6] = ["cube", "sphere", "capsule", "cylinder", "plane", "quad"];
/// Mesh ids from here on are the built-in primitives; scripts use ids below it.
pub const PRIMITIVE_BASE: u32 = 0xFFFF_0000;

/// The mesh id of a built-in primitive by name.
pub fn primitive_id(name: &str) -> Option<u32> {
    PRIMITIVES
        .iter()
        .position(|p| *p == name)
        .map(|i| PRIMITIVE_BASE + i as u32)
}

/// How a camera projects the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// Vertical field of view in degrees.
    Perspective { fov_y_degrees: f32 },
    /// Half the view's height in world units (Unity's `orthographicSize`): 2D games.
    Orthographic { half_height: f32 },
}

/// What a camera sees from where: `view_proj` maps world space to clip space.
#[derive(Clone, Copy, Debug)]
pub struct WorldCamera {
    pub view_proj: Mat4,
    /// The camera's position, for sorting see-through things and specular highlights.
    pub position: Vec3,
    /// Which way it looks (its -z).
    pub forward: Vec3,
    pub orthographic: bool,
    pub clear: Color,
}

impl WorldCamera {
    /// A camera at `world` (its transform: looking down its -z) for a viewport of `size`
    /// pixels. Uses the same projections as `Matrix4x4` in `@platypus/engine` (right-handed,
    /// depth 0 to 1).
    pub fn new(
        world: Mat4,
        projection: Projection,
        near: f32,
        far: f32,
        size: (u32, u32),
        clear: Color,
    ) -> Self {
        let aspect = size.0.max(1) as f32 / size.1.max(1) as f32;
        let (near, far) = (near.max(1e-4), far.max(near.max(1e-4) + 1e-3));
        let proj = match projection {
            Projection::Perspective { fov_y_degrees } => {
                platypus_core::glam::camera::rh::proj::directx::perspective(
                    fov_y_degrees.clamp(1.0, 179.0).to_radians(),
                    aspect,
                    near,
                    far,
                )
            }
            Projection::Orthographic { half_height } => {
                let h = half_height.max(1e-4);
                let w = h * aspect;
                platypus_core::glam::camera::rh::proj::directx::orthographic(
                    -w, w, -h, h, near, far,
                )
            }
        };
        Self {
            view_proj: proj * world.inverse(),
            position: world.w_axis.xyz(),
            forward: (-world.z_axis.xyz()).normalize_or(Vec3::NEG_Z),
            orthographic: matches!(projection, Projection::Orthographic { .. }),
            clear,
        }
    }

    /// A perspective camera with a vertical field of view in degrees.
    pub fn perspective(
        world: Mat4,
        fov_y_degrees: f32,
        near: f32,
        far: f32,
        size: (u32, u32),
        clear: Color,
    ) -> Self {
        Self::new(
            world,
            Projection::Perspective { fov_y_degrees },
            near,
            far,
            size,
            clear,
        )
    }
}

/// A light (Unity's `Light` types).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    /// Like the sun: parallel rays along `direction`, everywhere.
    Directional,
    /// From `position` in all directions, fading out at `range`.
    Point,
    /// A cone from `position` along `direction`, with soft edges between the inner and
    /// outer half-angles (degrees), fading out at `range`.
    Spot {
        inner_degrees: f32,
        outer_degrees: f32,
    },
    /// A spot light shaped as a pyramid (HDRP's Pyramid shape): `outer_degrees` across
    /// its height, `aspect` times as wide (in tangent), its edges softened from
    /// `inner_degrees`. `right` is its width's direction (the light's object's +x). A thin,
    /// wide one is a sheet of light: a scanner's, a laser level's, a light through a slot.
    Pyramid {
        inner_degrees: f32,
        outer_degrees: f32,
        aspect: f32,
        right: Vec3,
    },
}

impl LightKind {
    /// The half-angle (degrees) of a cone round the light: a pyramid's corners.
    pub fn bounding_degrees(self) -> f32 {
        match self {
            LightKind::Spot { outer_degrees, .. } => outer_degrees,
            LightKind::Pyramid {
                outer_degrees,
                aspect,
                ..
            } => {
                let t = (outer_degrees.to_radians() / 2.0).tan();
                2.0 * (t * (1.0 + aspect * aspect).sqrt()).atan().to_degrees()
            }
            _ => 360.0,
        }
    }
}

/// The two axes across `dir` the shaders build too (`spot_axes` in world.wgsl): a pyramid's
/// turn about its direction is measured from them.
fn spot_axes(dir: Vec3) -> (Vec3, Vec3) {
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let right = dir.cross(up).normalize_or(Vec3::X);
    (right, right.cross(dir))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub kind: LightKind,
    /// Linear colour already multiplied by the intensity.
    pub color: [f32; 3],
    pub position: Vec3,
    pub direction: Vec3,
    pub range: f32,
    /// Shadows it casts: the first directional light with them (in cascades), and spot
    /// and point lights nearest the main camera up to a budget.
    pub shadows: Option<Shadows>,
    /// The layers it lights (and whose objects cast its shadows), a bit each (Unity's
    /// `cullingMask`).
    pub culling_mask: u32,
    /// How strongly it lights fog with volumetric fog on ([`Fog::volumetric`]): 1 as
    /// bright as it lights surfaces, 0 not at all (HDRP's volumetric multiplier).
    pub volumetric: f32,
}

/// Which shadows are drawn at all (Unity's `QualitySettings.shadows`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShadowQuality {
    /// No shadows.
    Disable,
    /// Soft shadows are drawn hard (one sample).
    HardOnly,
    /// As each light asks.
    #[default]
    All,
}

/// How a light casts shadows (Unity's `LightShadows`: hard or soft) and how dark they are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadows {
    /// Filtered edges (3 × 3 PCF) instead of one sample.
    pub soft: bool,
    /// 0 (none) to 1 (fully dark where shadowed).
    pub strength: f32,
}

/// One mesh drawn in the world.
#[derive(Clone, Copy, Debug)]
pub struct MeshInstance {
    /// A built-in ([`primitive_id`]) or a mesh set with [`WorldRenderer::set_mesh`].
    pub mesh: u32,
    pub model: Mat4,
    /// Linear RGBA, straight alpha.
    pub color: [f32; 4],
    /// Linear light it gives off on its own (not lighting anything else).
    pub emission: [f32; 3],
    /// 0 (matte) to 1 (mirror): 1 - roughness, Unity's name.
    pub smoothness: f32,
    /// 0 (plastic, stone, wood) to 1 (metal: tinted reflections, no diffuse).
    pub metallic: f32,
    /// Ignores lights: just its colour (and emission).
    pub unlit: bool,
    /// Multiplies the colour; `None` is plain colour.
    pub texture: Option<WorldTexture>,
    /// Texture tiling: scale (xy) and offset (zw), Unity's `mainTextureScale` /
    /// `mainTextureOffset` (in its bottom-up UV space).
    pub tiling: [f32; 4],
    pub glow: MeshGlow,
    /// Whether it casts shadows (Unity's `shadowCastingMode` on / off).
    pub cast_shadows: bool,
    /// Reflects the scene exactly in this plane (a planar mirror) instead of the sky.
    pub mirror: Option<Mirror>,
    /// Its object's layer (0 to 31): lights, probes and mirrors with masks skip it.
    pub layer: u8,
    /// Normal, metallic, occlusion, emission and detail maps.
    pub maps: MaterialMaps,
    /// The specular workflow (Unity's "Standard (Specular setup)"): this colour is the
    /// reflectance head-on, and `metallic` is ignored.
    pub specular: Option<[f32; 3]>,
    pub mode: RenderMode,
    /// With [`RenderMode::Cutout`]: alpha below this is a hole.
    pub alpha_cutoff: f32,
    /// Draws its back faces too (leaves, cloth, open meshes), lit from that side.
    pub double_sided: bool,
    /// A skinned mesh's bones: the first of its matrices in [`WorldView::bones`] and how
    /// many. Its vertices go through them (`model` is then ignored: bones are in world
    /// space).
    pub bones: Option<(u32, u32)>,
    /// Non-zero: report whether it was seen this frame ([`WorldRenderer::seen`]), drawn by a
    /// view or into a shadow map. Animation culling asks.
    pub visibility_key: u32,
}

impl Default for MeshInstance {
    fn default() -> Self {
        Self {
            mesh: PRIMITIVE_BASE,
            model: Mat4::IDENTITY,
            color: [1.0; 4],
            emission: [0.0; 3],
            smoothness: 0.5,
            metallic: 0.0,
            unlit: false,
            texture: None,
            tiling: [1.0, 1.0, 0.0, 0.0],
            glow: MeshGlow::None,
            cast_shadows: true,
            mirror: None,
            layer: 0,
            maps: MaterialMaps::default(),
            specular: None,
            mode: RenderMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            bones: None,
            visibility_key: 0,
        }
    }
}

impl MeshInstance {
    /// Drawn in the see-through pass (blended back to front) rather than the opaque one.
    pub fn blended(&self) -> bool {
        self.glow != MeshGlow::None
            || match self.mode {
                RenderMode::Opaque => self.color[3] < 1.0,
                RenderMode::Cutout => false,
                RenderMode::Fade | RenderMode::Transparent => true,
            }
    }

    /// The textures it's drawn with, as a key for grouping and bind groups.
    fn material_key(&self) -> MaterialKey {
        MaterialKey {
            base: self.texture,
            normal: self.maps.normal,
            metallic_gloss: self.maps.metallic_gloss,
            // The specular colour map holds colours (sRGB); metallic maps hold data.
            metallic_srgb: self.specular.is_some(),
            occlusion: self.maps.occlusion,
            emission: self.maps.emission,
            detail: self.maps.detail,
            reflection: self.maps.reflection,
        }
    }
}

/// The textures a draw binds (group 1): the main texture and a material's maps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MaterialKey {
    base: Option<WorldTexture>,
    normal: Option<WorldTexture>,
    metallic_gloss: Option<WorldTexture>,
    metallic_srgb: bool,
    occlusion: Option<WorldTexture>,
    emission: Option<WorldTexture>,
    detail: Option<WorldTexture>,
    reflection: Option<WorldTexture>,
}

impl MaterialKey {
    fn texture(base: Option<WorldTexture>) -> Self {
        Self {
            base,
            ..Default::default()
        }
    }

    /// Uses the frame's textures (panels, reflections), so its bind group can't be kept.
    fn per_frame(&self) -> bool {
        [
            self.base,
            self.normal,
            self.metallic_gloss,
            self.occlusion,
            self.emission,
            self.detail,
            self.reflection,
        ]
        .iter()
        .any(|t| {
            matches!(
                t,
                Some(WorldTexture::Frame(_) | WorldTexture::Reflection(_))
            )
        })
    }
}

/// One camera's view: where on the target it draws and what it sees.
#[derive(Clone, Debug)]
pub struct WorldView {
    pub camera: WorldCamera,
    /// The part of the target it covers: x, y, width, height from the top-left, 0 to 1.
    pub viewport: [f32; 4],
    pub quads: Vec<WorldQuad>,
    pub meshes: Vec<MeshInstance>,
    pub lights: Vec<Light>,
    /// Linear light everything gets regardless of lights.
    pub ambient: [f32; 3],
    /// Fill the background with the sky ([`WorldRenderer::set_sky`]) instead of the
    /// camera's colour (Unity's skybox clear flag).
    pub skybox: bool,
    /// Draws only what's on the positive side of this plane (`[nx, ny, nz, d]`: kept where
    /// `n · p + d >= 0`), for mirror views.
    pub clip_plane: Option<[f32; 4]>,
    /// Reflection probes in this view's world (rendered for the main view).
    pub probes: Vec<ReflectionProbe>,
    /// FXAA over the whole target after it's drawn, when the main view (the biggest)
    /// asks for it (Unity's camera `antialiasing`).
    pub fxaa: bool,
    /// Post-process passes over this view's picture, in order, after tonemapping and FXAA
    /// ([`PostEffect`]).
    pub post: Vec<PostEffect>,
    /// Draws into this render texture ([`WorldRenderer::set_render_texture`]) instead
    /// of the target (Unity's `Camera.targetTexture`); its viewport is within the texture.
    pub target_texture: Option<u32>,
    /// Skinned meshes' bone matrices (each bone's world matrix times its inverse bind
    /// pose), which their [`MeshInstance::bones`] point into. Shared: views showing one
    /// world share one list, and only the runs of the skinned meshes drawn (in a view or a
    /// shadow map) are sent to the GPU, once a frame.
    pub bones: std::sync::Arc<Vec<Mat4>>,
    /// Draws the editor's ground grid on y = 0 after the opaque meshes: 1 m and 10 m
    /// lines fading with distance, the X axis red and the Z axis blue.
    pub grid: bool,
    /// Gizmo lines (the editor's wire shapes) drawn over everything, a constant width
    /// on screen, faint where something's in front of them.
    pub lines: Vec<GizmoLine>,
    /// Thick lines (`LineRenderer2D`), depth-tested and blended before the see-through
    /// things (so sprites and text show over them). A see-through stroke shows darker where
    /// its segments overlap.
    pub strokes: Vec<Stroke>,
    /// Filled shapes as a triangle list ([`tessellate_fill`]), drawn just before the
    /// strokes so outlines sit on top. Their edges are only as smooth as MSAA makes them.
    pub fills: Vec<FillVertex>,
    /// The shapes those fills and strokes belong to, in drawing order. Empty: all the
    /// fills, then all the strokes.
    pub shapes: Vec<ShapeRun>,
    /// Fog over everything this view draws except its background and the editor's grid
    /// and gizmos (`None`: clear air).
    pub fog: Option<Fog>,
    /// Boxes of fog in their own colours (mist over a pond, a cloud layer), whether or not
    /// there's `fog`; the first [`MAX_FOG_VOLUMES`] count.
    pub fog_volumes: Vec<FogVolume>,
    /// Whose exposure the view uses (its camera's id): each camera adapts on its own.
    pub exposure_key: u32,
    /// A deck of cloud drawn with the background (HDRP's cloud layer): behind everything, so
    /// distance fog doesn't hide it (`None`: none).
    pub clouds: Option<CloudLayer>,
}

/// A deck of cloud at an altitude, drawn with the background (HDRP's cloud layer): each pixel
/// of sky looks up to where its ray meets the deck, so the clouds shift as the camera moves
/// under them, and they thin out towards the horizon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudLayer {
    /// The deck's height in metres.
    pub altitude: f32,
    /// About how wide a cloud is, metres.
    pub size: f32,
    /// How much of the sky is cloud, 0 to 1.
    pub coverage: f32,
    pub opacity: f32,
    /// How soft the clouds' edges are, 0 (hard) to 1.
    pub softness: f32,
    /// The clouds' colour (linear; their thick parts are a little darker underneath).
    pub color: [f32; 3],
    /// How far the wind has carried the clouds, metres (x, z).
    pub offset: [f32; 2],
    /// Over about this distance along a ray the clouds fade into the sky, metres.
    pub fade: f32,
}

impl Default for CloudLayer {
    fn default() -> Self {
        Self {
            altitude: 1500.0,
            size: 2000.0,
            coverage: 0.5,
            opacity: 1.0,
            softness: 0.3,
            color: [0.8, 0.8, 0.82],
            offset: [0.0; 2],
            fade: 20000.0,
        }
    }
}

/// How many fog volumes a view draws.
pub const MAX_FOG_VOLUMES: usize = 8;

/// A box of fog (HDRP's local volumetric fog), lined up with the world's axes: `density`
/// per metre inside, thinning to nothing over `blend` metres in from its faces. It covers
/// what's seen through it, the background too, and light shafts light it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogVolume {
    pub centre: Vec3,
    pub half_extents: Vec3,
    pub density: f32,
    /// Linear RGB.
    pub color: [f32; 3],
    pub blend: f32,
    /// An ellipsoid inside the box rather than the box itself (soft round mist).
    pub ellipsoid: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuFogVolume {
    /// xyz: centre; w: density.
    centre: [f32; 4],
    /// xyz: half extents; w: the soft edge's depth.
    half: [f32; 4],
    /// rgb: colour; w: 1 for an ellipsoid.
    color: [f32; 4],
}

/// How fog thickens with distance (Unity's `FogMode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FogMode {
    /// None at `start`, total at `end`.
    Linear,
    /// `1 - e^(-density × distance)`.
    Exponential,
    /// `1 - e^(-(density × distance)²)`: clear nearby, then closing in (Unity's default).
    #[default]
    ExponentialSquared,
}

/// Fog (Unity's `RenderSettings.fog`): things fade towards `color` the farther they are
/// from the camera. Additive light (glows, beams) fades out instead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub mode: FogMode,
    /// Linear RGB.
    pub color: [f32; 3],
    /// The exponential modes' thickness, per metre.
    pub density: f32,
    /// Linear fog: where it begins and where it's total, in metres.
    pub start: f32,
    pub end: f32,
    /// Fog lying low as well, in the same colour.
    pub height: Option<HeightFog>,
    /// Lights shining through the fog: light shafts.
    pub volumetric: Option<VolumetricFog>,
    /// Skip meshes, quads and lights wholly beyond [`Fog::cull_distance`]: the fog hides
    /// them anyway (unless the background isn't the fog's colour: then they'd have shown
    /// as fog-coloured silhouettes).
    pub cull: bool,
}

/// Fog lets through less than this of what's behind it at its cull distance: under one
/// step of an 8-bit colour.
const FOG_OPAQUE: f32 = 1.0 / 255.0;

impl Fog {
    /// How far away the distance fog hides everything (`None`: it never does). Height fog
    /// and fog volumes only add fog, so they never bring anything back.
    pub fn cull_distance(&self) -> Option<f32> {
        let clear = -FOG_OPAQUE.ln();
        let d = match self.mode {
            FogMode::Linear => (self.end > self.start).then_some(self.end)?,
            FogMode::Exponential => clear / self.density,
            FogMode::ExponentialSquared => clear.sqrt() / self.density,
        };
        (d.is_finite() && d > 0.0).then_some(d)
    }
}

/// Light scattered by the fog towards the camera, cut by shadows: shafts where the sun
/// comes through gaps, cones under spot lights. A froxel volume over the view (screen cells
/// by depth slices out to `distance`) adds it up once per view, sampling every light's
/// (hard) shadow map `steps` times per slice; draws read it smoothly filtered. The fog
/// beyond `distance` counts as lit by the sun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumetricFog {
    /// Scales the scattered light (1: fog lit as brightly as a white surface would be).
    pub intensity: f32,
    /// -1 to 1: positive scatters forward, so looking towards a light shows more (the
    /// Henyey-Greenstein `g`).
    pub anisotropy: f32,
    /// Samples per depth slice (1 to 16): more keep thin cones sampled.
    pub steps: u32,
    /// How far the volume reaches, in metres.
    pub distance: f32,
    /// How much of each colour of light the fog scatters (white: all). It's separate from
    /// the fog's colour, which is how the fog looks in the ambient light: a night fog is
    /// dark, yet glows white under a lamp.
    pub albedo: [f32; 3],
    /// God rays: within this many metres of the camera the sun's shafts are worked out per
    /// pixel from its shadow map, so beams through bars, gaps and leaves keep sharp edges
    /// (the volume's cells would blur them). 0: off, the volume does it all.
    pub sharp_distance: f32,
    /// Samples per pixel for the sharp part (closer together nearer the camera).
    pub sharp_samples: u32,
    /// The volume's detail: 0 low, 1 medium, 2 high, 3 ultra (see `volume::SIZES`).
    pub detail: u8,
    /// How the volume's sampling noise is cleaned up (HDRP's denoising mode).
    pub denoising: Denoising,
    /// With reprojection, how much of each cell comes from earlier frames (Unreal's history
    /// weight, 0.9): higher is smoother, slower to follow a moving sun or camera.
    pub history_weight: f32,
    /// Shadow samples per cell each frame (1 to 8): more is less noise, at that many times
    /// the volume's lighting cost.
    pub samples: u32,
    /// Samples for cells whose view moved this frame (adaptive: the cost only while the
    /// camera moves, when the history counts least).
    pub motion_samples: u32,
}

/// How the light volume's sampling is cleaned up (HDRP's volumetric denoising mode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Denoising {
    /// Each frame stands alone, sampled at each cell's middle: cheapest, beams through fine
    /// patterns step and shimmer.
    None,
    /// Each cell is sampled somewhere new each frame and blended with what it held last
    /// frame (reprojected): smooth beams for one shadow lookup a cell.
    Reprojection,
    /// A blur over a cell's width as the volume is added up (four reads for one).
    Gaussian,
    /// Both: blinds and grates come out as clean haze (the default).
    #[default]
    Both,
}

impl Denoising {
    pub fn temporal(self) -> bool {
        matches!(self, Self::Reprojection | Self::Both)
    }
    pub fn spatial(self) -> bool {
        matches!(self, Self::Gaussian | Self::Both)
    }
}

impl Default for VolumetricFog {
    fn default() -> Self {
        Self {
            intensity: 1.0,
            anisotropy: 0.4,
            steps: 4,
            distance: 50.0,
            albedo: [1.0; 3],
            sharp_distance: 0.0,
            sharp_samples: 32,
            detail: 2,
            denoising: Denoising::Both,
            history_weight: 0.9,
            samples: 1,
            motion_samples: 2,
        }
    }
}

impl Default for Fog {
    /// Unity's defaults.
    fn default() -> Self {
        Self {
            mode: FogMode::ExponentialSquared,
            color: [0.5, 0.5, 0.5],
            density: 0.01,
            start: 0.0,
            end: 300.0,
            height: None,
            volumetric: None,
            cull: true,
        }
    }
}

/// Exponential height fog (Unreal's): `density` per metre at `base_height`, thinning by a
/// factor of e every `1 / falloff` metres above it and thickening below. Unlike distance
/// fog it covers the background too, so the horizon sinks into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightFog {
    pub density: f32,
    pub base_height: f32,
    /// Per metre.
    pub falloff: f32,
}

/// A line segment in the world, for [`WorldView::lines`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct GizmoLine {
    pub a: [f32; 3],
    pub b: [f32; 3],
    /// Linear RGBA, straight alpha.
    pub color: [f32; 4],
}

const LINE_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];

/// A thick line segment in the world, for [`WorldView::strokes`]: a capsule `width` world
/// units across, so a run of segments gets round joins and round caps.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Stroke {
    pub a: [f32; 3],
    pub b: [f32; 3],
    /// Full width, in world units.
    pub width: f32,
    /// Linear RGBA, straight alpha.
    pub color: [f32; 4],
}

/// A corner of a filled shape's triangle, for [`WorldView::fills`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct FillVertex {
    pub position: [f32; 3],
    /// Linear RGBA, straight alpha.
    pub color: [f32; 4],
}

const FILL_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];

/// One 2D shape's part of a view's [`WorldView::fills`] and [`WorldView::strokes`]: its fill
/// triangles' vertices and its outline's segments. Shapes draw in order, each fill then its
/// outline, so a later shape covers an earlier one whole.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShapeRun {
    pub fills: Range<u32>,
    pub strokes: Range<u32>,
}

const STROKE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32, 3 => Float32x4];

impl WorldView {
    /// A view of nothing yet (add quads, meshes and lights).
    pub fn new(camera: WorldCamera, viewport: [f32; 4]) -> Self {
        Self {
            camera,
            viewport,
            quads: Vec::new(),
            meshes: Vec::new(),
            lights: Vec::new(),
            ambient: [0.2, 0.2, 0.22],
            skybox: false,
            clip_plane: None,
            probes: Vec::new(),
            fxaa: false,
            post: Vec::new(),
            target_texture: None,
            bones: Default::default(),
            grid: false,
            lines: Vec::new(),
            strokes: Vec::new(),
            fills: Vec::new(),
            shapes: Vec::new(),
            fog: None,
            fog_volumes: Vec::new(),
            exposure_key: 0,
            clouds: None,
        }
    }
}

/// One plane in the world.
#[derive(Clone, Copy, Debug)]
pub struct WorldQuad {
    /// Places the unit quad (-0.5..0.5 in x and y, facing +z) in the world.
    pub model: Mat4,
    /// Linear RGBA, straight alpha (multiplies the texture).
    pub color: [f32; 4],
    /// Multiplies the colour; `None` is plain colour.
    pub texture: Option<WorldTexture>,
    /// The part of the texture shown: origin and size in UV space.
    pub uv: [f32; 4],
    /// Adds light instead of covering what's behind (glows, fire, sparks).
    pub additive: bool,
    /// A 2D sprite: depth-tested against solid geometry but writing no depth, so overlapping
    /// sprites are ordered by `order` (then distance) alone, whatever their tilt.
    pub flat: bool,
    /// Draw order among see-through things: higher draws later (over), whatever the
    /// distance (Unity's `sortingOrder`; meshes are 0).
    pub order: i32,
    /// Its object's layer (0 to 31), for probes' and mirrors' masks.
    pub layer: u8,
}

impl WorldQuad {
    pub fn colored(model: Mat4, color: [f32; 4]) -> Self {
        Self {
            model,
            color,
            texture: None,
            uv: [0.0, 0.0, 1.0, 1.0],
            additive: false,
            flat: false,
            order: 0,
            layer: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuQuad {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    uv: [f32; 4],
    /// x: 1 when the texture holds straight alpha (kept textures) rather than
    /// premultiplied (the frame's, such as rendered UI).
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuMesh {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    color: [f32; 4],
    emission: [f32; 4],
    params: [f32; 4],
    tiling: [f32; 4],
    /// The reflection probe it uses: centre (xyz) and mip count (w, 0: none).
    probe: [f32; 4],
    /// The probe's half extents (xyz) and 1 for box projection (w).
    probe_box: [f32; 4],
    /// x: its layer's bit, for lights' masks; y: which maps it has ([`map_flags`]);
    /// z: its [`RenderMode`] (0 opaque, 1 cutout, 2 fade, 3 transparent).
    layer: [u32; 4],
    /// rgb: the specular colour; w: 1 for the specular workflow.
    specular: [f32; 4],
    /// x: normal map strength; y: occlusion strength; z: alpha cut-off.
    maps: [f32; 4],
    /// The detail map's tiling: scale (xy), offset (zw).
    detail: [f32; 4],
}

/// Bits in `GpuMesh::layer.y` for the maps a mesh has.
mod map_flags {
    pub const NORMAL: u32 = 1;
    pub const METALLIC_GLOSS: u32 = 2;
    pub const GLTF: u32 = 4;
    pub const OCCLUSION: u32 = 8;
    pub const EMISSION: u32 = 16;
    pub const DETAIL: u32 = 32;
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuLight {
    position: [f32; 4],
    direction: [f32; 4],
    color: [f32; 4],
    spot: [f32; 4],
    /// x: the layers it lights.
    mask: [u32; 4],
    /// x: its first shadow matrix (-1: no shadows); y: strength; z: the soft filter's
    /// radius in texels (0: hard); w: a texel's size at 1 m (spot and point lights).
    shadow: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuGlobals {
    view_proj: [[f32; 4]; 4],
    background: [f32; 4],
    camera: [f32; 4],
    ambient: [f32; 4],
    /// x: the sun's shadow cascades (0: none).
    shadow: [f32; 4],
    /// Each cascade's texel size in metres (for the normal offset).
    cascade_texels: [f32; 4],
    /// Screen pixel to world direction, for the skybox.
    inv_view_proj: [[f32; 4]; 4],
    /// The view's viewport in target pixels: x, y, width, height.
    viewport: [f32; 4],
    /// x: 1 with an environment map; y: its intensity; z: its last mip; w: 1 when ambient
    /// light comes from it.
    env: [f32; 4],
    /// x: 1 to draw the sky as the background; yz: the target's size in pixels.
    flags: [f32; 4],
    /// Fragments where `dot(xyz, p) + w < 0` are dropped (zero: none).
    clip: [f32; 4],
    /// rgb: the fog's colour; w: its mode (0 none, 1 linear, 2 exponential, 3 squared).
    fog: [f32; 4],
    /// x: density; y, z: linear fog's start and end.
    fog_params: [f32; 4],
    /// xyz: the camera's forward axis; w: 1 for orthographic.
    view_dir: [f32; 4],
    /// Height fog: x density, y base height, z falloff, w 1 when on.
    fog_height: [f32; 4],
    /// Light shafts: x intensity, y anisotropy, z samples per slice, w the volume's reach
    /// (0: off).
    fog_volume: [f32; 4],
    /// rgb: the fog's albedo for light shafts.
    fog_albedo: [f32; 4],
    /// God rays: x how far the sun's shafts are worked out per pixel (0: off), y samples.
    fog_sharp: [f32; 4],
    /// x: how many fog volumes follow.
    fog_local: [f32; 4],
    /// x: pre-exposure (world.wgsl's `Globals::exposure`).
    exposure: [f32; 4],
    /// The cloud layer: x altitude, y size, z coverage, w opacity.
    cloud: [f32; 4],
    /// rgb: the clouds' colour; w: softness.
    cloud_color: [f32; 4],
    /// xy: the wind's offset; z: the fade distance; w: 1 when on.
    cloud_move: [f32; 4],
    local_fogs: [GpuFogVolume; MAX_FOG_VOLUMES],
}

impl GpuGlobals {
    fn set_clouds(&mut self, clouds: Option<&CloudLayer>) {
        let Some(c) = clouds else {
            self.cloud_move = [0.0; 4];
            return;
        };
        self.cloud = [
            c.altitude,
            c.size.max(1e-3),
            c.coverage.clamp(0.0, 1.0),
            c.opacity.clamp(0.0, 1.0),
        ];
        let [r, g, b] = c.color.map(|v| v.max(0.0));
        self.cloud_color = [r, g, b, c.softness.clamp(0.01, 1.0)];
        self.cloud_move = [c.offset[0], c.offset[1], c.fade.max(1e-3), 1.0];
    }

    fn set_fog_volumes(&mut self, volumes: &[FogVolume]) {
        let n = volumes.len().min(MAX_FOG_VOLUMES);
        for (gpu, v) in self.local_fogs.iter_mut().zip(&volumes[..n]) {
            let (c, h) = (v.centre, v.half_extents.max(Vec3::splat(1e-3)));
            *gpu = GpuFogVolume {
                centre: [c.x, c.y, c.z, v.density.max(0.0)],
                half: [h.x, h.y, h.z, v.blend.max(0.0)],
                color: [
                    v.color[0],
                    v.color[1],
                    v.color[2],
                    f32::from(u8::from(v.ellipsoid)),
                ],
            };
        }
        self.fog_local = [n as f32, 0.0, 0.0, 0.0];
    }

    fn set_fog(&mut self, fog: Option<&Fog>) {
        let Some(f) = fog else {
            self.fog = [0.0; 4];
            return;
        };
        let mode = match f.mode {
            FogMode::Linear => 1.0,
            FogMode::Exponential => 2.0,
            FogMode::ExponentialSquared => 3.0,
        };
        self.fog = [f.color[0], f.color[1], f.color[2], mode];
        self.fog_params = [f.density.max(0.0), f.start, f.end, 0.0];
        if let Some(h) = f.height {
            self.fog_height = [h.density.max(0.0), h.base_height, h.falloff.max(1e-4), 1.0];
        }
        if let Some(v) = f.volumetric {
            self.fog_volume = [
                v.intensity.max(0.0),
                v.anisotropy.clamp(-0.95, 0.95),
                v.steps.clamp(1, 16) as f32,
                v.distance.max(1e-3),
            ];
            let [r, g, b] = v.albedo.map(|c| c.max(0.0));
            self.fog_albedo = [r, g, b, 0.0];
            self.fog_sharp = [
                v.sharp_distance.max(0.0),
                v.sharp_samples.clamp(4, 128) as f32,
                f32::from(u8::from(v.denoising.spatial())),
                0.0,
            ];
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    tangent: [f32; 4],
}

/// A skinned vertex's bones and weights (a second vertex buffer).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SkinVertex {
    joints: [u32; 4],
    weights: [f32; 4],
}

const SKIN_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![4 => Uint32x4, 5 => Float32x4];

/// The mesh vertex layout every mesh pipeline uses.
const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4
];

/// A mesh's buffers on the GPU.
struct MeshBuffers {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    /// The local bounding box, for frustum culling.
    bounds: (Vec3, Vec3),
    /// Skinned meshes' bones and weights.
    skin: Option<wgpu::Buffer>,
}

/// What the last frame drew and what culling skipped (every view, mirror and probe
/// face counted), like Unity's rendering statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldStats {
    pub meshes: usize,
    pub culled_meshes: usize,
    pub quads: usize,
    pub culled_quads: usize,
    /// Of the culled meshes and quads, those the fog hid ([`Fog::cull`]).
    pub fog_culled: usize,
    pub lights: usize,
    pub culled_lights: usize,
    /// Shadow map layers drawn (once a frame, shared by every view).
    pub shadow_maps: usize,
    /// Meshes drawn into the shadow maps (once per culling mask of the casting lights):
    /// casters outside every layer are left out.
    pub shadow_casters: usize,
    /// Bone matrices sent to the GPU: only those of the skinned meshes drawn, each run once
    /// however many passes draw it.
    pub bones: usize,
}

/// Quad, background-fill, opaque-mesh and see-through-mesh pipelines for one format.
#[derive(Clone)]
struct Pipelines {
    quad: wgpu::RenderPipeline,
    /// Additive quads: they light what's behind and hide nothing, so no depth writes.
    quad_glow: wgpu::RenderPipeline,
    /// Additive meshes, seen from inside too (a beam around the camera).
    mesh_glow: wgpu::RenderPipeline,
    fill: wgpu::RenderPipeline,
    /// The editor's ground grid ([`WorldView::grid`]): depth-tested, blended, no writes.
    grid: wgpu::RenderPipeline,
    /// Gizmo lines where they're in front, and (faint) where they're hidden.
    lines: wgpu::RenderPipeline,
    lines_hidden: wgpu::RenderPipeline,
    /// Thick world lines ([`WorldView::strokes`]).
    strokes: wgpu::RenderPipeline,
    /// Filled shapes ([`WorldView::fills`]).
    fills: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    mesh_blend: wgpu::RenderPipeline,
    /// Double-sided materials: both faces drawn.
    mesh_double: wgpu::RenderPipeline,
    mesh_blend_double: wgpu::RenderPipeline,
    /// Skinned meshes: opaque, see-through, each one- or double-sided.
    skinned: [wgpu::RenderPipeline; 4],
}

/// A view's opaque runs, its blended runs (`true`: a mesh) and its lights.
/// Opaque runs, see-through runs, the view's lights in the light buffer, and which of
/// the view's lights those are (culled ones left out).
type ViewRuns = (Vec<Run>, Vec<(bool, Run)>, Range<u32>, Vec<usize>);

/// A run of instances drawn with one mesh or texture.
struct Run {
    mesh: Option<u32>,
    material: MaterialKey,
    /// Both faces drawn.
    double_sided: bool,
    /// Casts shadows with holes where its alpha is below the cut-off.
    cutout: bool,
    /// Drawn through its bones.
    skinned: bool,
    /// The reflection probe its meshes use (`None`: the sky).
    probe: Option<u32>,
    /// Additive or flat (sprite) quads: drawn without depth writes.
    glow: bool,
    range: Range<u32>,
}

pub struct WorldRenderer {
    layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    /// By colour format and sample count.
    pipelines: HashMap<(wgpu::TextureFormat, u32), Pipelines>,
    /// Multisampled colour targets the main pass resolves from, by size, format, samples.
    msaa_targets: HashMap<((u32, u32), wgpu::TextureFormat, u32), wgpu::TextureView>,
    /// The world's resolution relative to the target (0.25 to 2; Unity URP's `renderScale`):
    /// below 1 is faster and blurrier, above 1 supersamples. Screen UI isn't scaled.
    pub render_scale: f32,
    upscale: Option<upscale::Upscale>,
    /// Anti-aliasing for the main world pass: 1 (off), 2, 4 or 8 samples per pixel
    /// (Unity's `antiAliasing`), lowered to what the GPU supports. Mirrors and probes
    /// render without it.
    pub msaa: u32,
    globals_layout: wgpu::BindGroupLayout,
    shadow_layout: wgpu::BindGroupLayout,
    /// Light shafts' froxel volumes, one per view.
    volume: volume::VolumePass,
    shadow_sampler: wgpu::Sampler,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_cutout_pipeline: wgpu::RenderPipeline,
    shadow_skinned_pipeline: wgpu::RenderPipeline,
    /// Bone matrices of the frame's drawn skinned meshes, shared by every pass, and what
    /// is in it.
    bones: wgpu::Buffer,
    bone_pack: std::cell::RefCell<bone_pack::BonePack>,
    /// The sun's cascades and the spot / point lights' maps.
    sun_shadows: ShadowArray,
    local_shadows: ShadowArray,
    /// This frame's shadow casting lights and matrices.
    shadow_plan: shadows::ShadowPlan,
    /// The plan's matrices, for the shader.
    shadow_matrices: wgpu::Buffer,
    /// A `GpuGlobals` per shadow layer (its matrix as the view), and the casters.
    shadow_globals: wgpu::Buffer,
    shadow_casters: wgpu::Buffer,
    /// The sun's cascades: 1 to 4 (Unity's `shadowCascades`).
    pub shadow_cascades: u32,
    /// How far from the camera shadows reach (metres; Unity's `shadowDistance`).
    pub shadow_distance: f32,
    /// The shadow map's size in texels.
    pub shadow_resolution: u32,
    /// Which shadows are drawn.
    pub shadow_quality: ShadowQuality,
    /// Soft shadows' filter radius in texels: 1 (3 × 3 samples) or 2 (5 × 5, smoother).
    pub soft_shadow_radius: u32,
    /// Fills environment maps (made on first use).
    env_pass: Option<environment::EnvPass>,
    /// The sky's environment map; a 1-texel black cube until there's a sky.
    env_map: environment::EnvMap,
    sky: Option<Sky>,
    sky_dirty: bool,
    /// Mirrors' reflection textures, by mirror, with their sizes.
    reflection_targets: Vec<Option<(wgpu::TextureView, (u32, u32))>>,
    env_sampler: wgpu::Sampler,
    /// How bright reflections and sky light are (Unity's `reflectionIntensity`).
    pub environment_intensity: f32,
    /// Ambient light from the sky (Unity's `ambientMode` skybox) rather than each view's
    /// `ambient` colour, when there's a sky.
    pub ambient_from_sky: bool,
    texture_layout: wgpu::BindGroupLayout,
    /// One `GpuGlobals` per view, `stride` bytes apart (a dynamic offset picks one).
    globals: wgpu::Buffer,
    stride: u64,
    quads: wgpu::Buffer,
    mesh_instances: wgpu::Buffer,
    /// Every view's gizmo lines (instances of the line pipelines).
    lines: wgpu::Buffer,
    /// Every view's strokes (instances of the stroke pipeline).
    strokes: wgpu::Buffer,
    /// Every view's fill triangles.
    fills: wgpu::Buffer,
    meshes: HashMap<u32, MeshBuffers>,
    /// What the last frame drew and culled.
    stats: WorldStats,
    /// The visibility keys of the meshes the last frame saw, sorted.
    seen: Vec<u32>,
    /// Skinned meshes' world boxes this frame, by (view's bone list, first bone, mesh): the
    /// shadow pass, the sight report and each view's culling ask for the same ones.
    skin_boxes: std::cell::RefCell<HashMap<(usize, u32, u32), Option<WorldBox>>>,
    textures: HashMap<u32, AssetTexture>,
    samplers: HashMap<SamplerKey, wgpu::Sampler>,
    /// Material bind groups made from kept textures only, reused across frames (cleared
    /// when a texture or the texture quality changes).
    material_groups: HashMap<MaterialKey, wgpu::BindGroup>,
    /// Unity's `QualitySettings.anisotropicFiltering`.
    pub anisotropic: Anisotropic,
    /// Top mip levels skipped when sampling kept textures (Unity's
    /// `globalTextureMipmapLimit`: 0 full size, 1 half, 2 quarter, 3 eighth).
    pub texture_mip_limit: u32,
    /// The quality the material groups were made with.
    texture_quality: (Anisotropic, u32),
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
    /// Depth buffers by size and sample count (the screen, mirrors and probe faces).
    depth: HashMap<((u32, u32), u32), wgpu::TextureView>,
    /// Reflection probes' cubemaps, by probe id.
    probe_maps: HashMap<u32, environment::EnvMap>,
    /// 2D targets probe faces are drawn into before copying, by face size.
    probe_faces: HashMap<u32, wgpu::TextureView>,
    /// Bloom over every view (`None`: off).
    pub bloom: Option<Bloom>,
    /// Made the first time bloom (or grading) is on.
    bloom_pass: Option<bloom::BloomPass>,
    /// Exposure and tonemapping over every view.
    pub grading: Grading,
    /// Which features run (Unity's URP asset: set per quality level).
    pub pipeline: PipelineAsset,
    /// Lights in real-world units (lux, lumens, nits; a project-wide choice): the world is
    /// pre-exposed by each camera's last exposure, so sunlight stays within the HDR target.
    pub physical_units: bool,
    /// Made the first time a view asks for FXAA.
    fxaa: Option<fxaa::Fxaa>,
    /// Made the first time a view has post-process passes.
    post_chain: Option<post::PostChain>,
    /// Seconds the post-process passes have run for, and the frames (their `post_time()`).
    post_time: f32,
    post_frame: u32,
}

/// A depth texture array of shadow maps: one view of it all to sample, one per layer to
/// draw into.
struct ShadowArray {
    view: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
    res: u32,
}

impl ShadowArray {
    fn new(gpu: &Gpu, label: &str, res: u32, layers: u32) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: res,
                height: res,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            }),
            layers: (0..layers)
                .map(|l| {
                    texture.create_view(&wgpu::TextureViewDescriptor {
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        base_array_layer: l,
                        array_layer_count: Some(1),
                        ..Default::default()
                    })
                })
                .collect(),
            res,
        }
    }
}

/// A texture the renderer keeps: views reading it as colour (sRGB) and as data
/// (linear, for normal and metallic maps), and its sampling settings.
struct AssetTexture {
    srgb: wgpu::TextureView,
    linear: wgpu::TextureView,
    /// A render texture's view to draw into (its only mip).
    target: Option<wgpu::TextureView>,
    filter: TextureFilter,
    wrap: TextureWrap,
    anisotropy: u16,
    size: (u32, u32),
}

fn storage_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl WorldRenderer {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::include_wgsl!("world.wgsl"));
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // The fog volume's compute pass reads it too.
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT
                        .union(wgpu::ShaderStages::COMPUTE),
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<GpuGlobals>() as u64
                        ),
                    },
                    count: None,
                },
                storage_entry(1, wgpu::ShaderStages::VERTEX),
                storage_entry(2, wgpu::ShaderStages::VERTEX_FRAGMENT),
                storage_entry(
                    3,
                    wgpu::ShaderStages::FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
                ),
                // Skinned meshes' bone matrices.
                storage_entry(4, wgpu::ShaderStages::VERTEX),
            ],
        });
        // A draw's textures: the main one and its sampler, then the material's maps
        // (normal, metallic / specular, occlusion, emission, detail) and a mirror's
        // reflection.
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world material"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                texture_entry(5),
                texture_entry(6),
                texture_entry(7),
            ],
        });
        // Shadows and lights reach the fog volume's compute pass too.
        let lit = wgpu::ShaderStages::FRAGMENT.union(wgpu::ShaderStages::COMPUTE);
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world shadows"),
            entries: &[
                // The sun's cascades.
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: lit,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                // Spot and point lights' maps.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: lit,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                // Their matrices.
                storage_entry(5, lit),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: lit,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                // The environment cube, for reflections, sky light and the skybox.
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: lit,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: lit,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let volume = volume::VolumePass::new(device, &module, &globals_layout, &shadow_layout);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&texture_layout),
                Some(&shadow_layout),
                Some(&volume.read_layout),
            ],
            immediate_size: 0,
        });
        // Casters are drawn into shadow maps with only the globals (the layer's matrix as
        // the view, the casters as meshes).
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("world shadow casters"),
                bind_group_layouts: &[Some(&globals_layout)],
                immediate_size: 0,
            });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world shadow casters"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &VERTEX_ATTRIBUTES,
                })],
            },
            // Both sides cast: planes and open meshes have no back to cast from.
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                // Slope-scaled bias against shadow acne.
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        // Skinned casters go through their bones.
        let shadow_skinned_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world skinned shadow casters"),
                layout: Some(&shadow_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_shadow_skinned"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &VERTEX_ATTRIBUTES,
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<SkinVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &SKIN_ATTRIBUTES,
                        }),
                    ],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.0,
                        clamp: 0.0,
                    },
                }),
                multisample: Default::default(),
                fragment: None,
                multiview_mask: None,
                cache: None,
            });
        // Cut-out casters test their texture's alpha, so their holes let light through.
        let cutout_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world cut-out casters"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let shadow_cutout_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world cut-out casters"),
                layout: Some(&cutout_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_shadow_cutout"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &VERTEX_ATTRIBUTES,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.0,
                        clamp: 0.0,
                    },
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_shadow_cutout"),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                multiview_mask: None,
                cache: None,
            });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("world shadows"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let stride = (std::mem::size_of::<GpuGlobals>() as u64).div_ceil(align) * align;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("world"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("world white"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            white.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            white.size(),
        );
        let mut renderer = Self {
            layout,
            module,
            pipelines: HashMap::new(),
            globals_layout,
            shadow_layout,
            shadow_sampler,
            shadow_pipeline,
            shadow_cutout_pipeline,
            shadow_skinned_pipeline,
            bones: Self::storage::<[[f32; 4]; 4]>(gpu, "world bones", 16),
            bone_pack: Default::default(),
            sun_shadows: ShadowArray::new(gpu, "sun shadows", 1, 1),
            local_shadows: ShadowArray::new(gpu, "light shadows", 1, 1),
            shadow_plan: shadows::ShadowPlan::default(),
            shadow_matrices: Self::storage::<[[f32; 4]; 4]>(gpu, "shadow matrices", 8),
            shadow_globals: Self::buffer(
                gpu,
                "shadow globals",
                stride * 8,
                wgpu::BufferUsages::UNIFORM,
            ),
            shadow_casters: Self::storage::<GpuMesh>(gpu, "shadow casters", 64),
            shadow_cascades: 1,
            shadow_distance: 25.0,
            shadow_resolution: 2048,
            shadow_quality: ShadowQuality::All,
            soft_shadow_radius: 1,
            env_pass: None,
            env_map: environment::EnvMap::new(gpu, 1),
            sky: None,
            sky_dirty: false,
            reflection_targets: Vec::new(),
            volume,
            env_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("environment"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            environment_intensity: 1.0,
            ambient_from_sky: true,
            texture_layout,
            globals: Self::buffer(
                gpu,
                "world globals",
                stride * 4,
                wgpu::BufferUsages::UNIFORM,
            ),
            stride,
            quads: Self::storage::<GpuQuad>(gpu, "world quads", 64),
            mesh_instances: Self::storage::<GpuMesh>(gpu, "world meshes", 64),
            lines: Self::buffer(gpu, "world gizmo lines", 4096, wgpu::BufferUsages::VERTEX),
            strokes: Self::buffer(gpu, "world strokes", 4096, wgpu::BufferUsages::VERTEX),
            fills: Self::buffer(gpu, "world fills", 4096, wgpu::BufferUsages::VERTEX),
            meshes: HashMap::new(),
            stats: WorldStats::default(),
            seen: Vec::new(),
            skin_boxes: Default::default(),
            textures: HashMap::new(),
            samplers: HashMap::new(),
            material_groups: HashMap::new(),
            anisotropic: Anisotropic::Enable,
            texture_mip_limit: 0,
            texture_quality: (Anisotropic::Enable, 0),
            sampler,
            white: white.create_view(&Default::default()),
            depth: HashMap::new(),
            msaa_targets: HashMap::new(),
            msaa: 1,
            render_scale: 1.0,
            upscale: None,
            probe_maps: HashMap::new(),
            probe_faces: HashMap::new(),
            bloom: None,
            bloom_pass: None,
            grading: Grading::default(),
            pipeline: PipelineAsset::default(),
            physical_units: false,
            fxaa: None,
            post_chain: None,
            post_time: 0.0,
            post_frame: 0,
        };
        for (i, name) in PRIMITIVES.iter().enumerate() {
            let data = primitive(name).expect("built in");
            renderer.set_mesh(gpu, PRIMITIVE_BASE + i as u32, &data);
        }
        renderer.set_texture(gpu, DEFAULT_PARTICLE_TEXTURE, &texture::soft_spot(64));
        renderer.set_texture(gpu, MISSING_TEXTURE, &texture::missing());
        renderer.set_mesh(gpu, BEAM_MESH, &mesh::beam());
        renderer
    }

    fn buffer(gpu: &Gpu, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size.max(16),
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn storage<T>(gpu: &Gpu, label: &str, count: usize) -> wgpu::Buffer {
        Self::buffer(
            gpu,
            label,
            (count.max(1) * std::mem::size_of::<T>()) as u64,
            wgpu::BufferUsages::STORAGE,
        )
    }

    /// Uploads `data` to `buffer`, growing it first if it's too small.
    fn upload<T: Pod>(gpu: &Gpu, buffer: &mut wgpu::Buffer, label: &str, data: &[T]) {
        let bytes = std::mem::size_of_val(data) as u64;
        if buffer.size() < bytes {
            *buffer = Self::storage::<T>(gpu, label, data.len().next_power_of_two());
        }
        if !data.is_empty() {
            gpu.queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(data));
        }
    }

    /// Sets (or replaces) a mesh scripts can draw by `id` (below [`PRIMITIVE_BASE`]).
    /// Incomplete data gets normals and texture coordinates made up; invalid data is
    /// refused.
    pub fn set_mesh(&mut self, gpu: &Gpu, id: u32, data: &MeshData) -> bool {
        let data = data.clone().complete();
        if !data.is_valid() || data.indices.is_empty() {
            return false;
        }
        let vertices: Vec<Vertex> = (0..data.positions.len())
            .map(|i| Vertex {
                position: data.positions[i],
                normal: data.normals[i],
                uv: data.uvs[i],
                tangent: data.tangents[i],
            })
            .collect();
        let make = |label: &str, bytes: &[u8], usage: wgpu::BufferUsages| {
            let buffer = Self::buffer(gpu, label, bytes.len() as u64, usage);
            gpu.queue.write_buffer(&buffer, 0, bytes);
            buffer
        };
        self.meshes.insert(
            id,
            MeshBuffers {
                vertices: make(
                    "mesh vertices",
                    bytemuck::cast_slice(&vertices),
                    wgpu::BufferUsages::VERTEX,
                ),
                indices: make(
                    "mesh indices",
                    bytemuck::cast_slice(&data.indices),
                    wgpu::BufferUsages::INDEX,
                ),
                count: data.indices.len() as u32,
                bounds: cull::bounds(&data.positions).unwrap_or_default(),
                skin: (!data.joints.is_empty()).then(|| {
                    let skin: Vec<SkinVertex> = data
                        .joints
                        .iter()
                        .zip(&data.weights)
                        .map(|(j, w)| SkinVertex {
                            joints: j.map(u32::from),
                            weights: *w,
                        })
                        .collect();
                    make(
                        "mesh skin",
                        bytemuck::cast_slice(&skin),
                        wgpu::BufferUsages::VERTEX,
                    )
                }),
            },
        );
        true
    }

    pub fn remove_mesh(&mut self, id: u32) {
        if id < PRIMITIVE_BASE {
            self.meshes.remove(&id);
        }
    }

    /// What the last [`WorldRenderer::render_views`] drew and what culling skipped.
    pub fn stats(&self) -> WorldStats {
        self.stats
    }

    /// The [`MeshInstance::visibility_key`]s of the meshes the last frame saw (sorted, once
    /// each): in a view's frustum (and not past its fog's cull distance), or a shadow caster
    /// inside a shadow map's volume whose shadow can fall in a view (`shadow_reaches`). Skinned
    /// meshes are judged by their bind-pose box under every bone (a box that holds them, larger
    /// than they are).
    pub fn seen(&self) -> &[u32] {
        &self.seen
    }

    /// [`Self::seen`], emptied: a frame that draws no world reports nothing seen.
    pub fn take_seen(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.seen)
    }

    /// Works out [`Self::seen`] for these views, after the shadow plan is made.
    fn note_seen(&mut self, views: &[WorldView]) {
        self.seen.clear();
        if views
            .iter()
            .all(|v| v.meshes.iter().all(|m| m.visibility_key == 0))
        {
            return;
        }
        let layers = Self::shadow_layers(&self.shadow_plan);
        let mut seen = std::mem::take(&mut self.seen);
        for view in views {
            let frustum = cull::Frustum::from_view_proj(view.camera.view_proj);
            let fog_reach = view
                .fog
                .filter(|f| f.cull)
                .and_then(|f| f.cull_distance())
                .map(|d| cull::FogReach::new(&view.camera, d));
            for m in view.meshes.iter().filter(|m| m.visibility_key != 0) {
                let Some((lo, hi)) = self.world_box(view, m) else {
                    continue;
                };
                let in_view = frustum.aabb(lo, hi) && !fog_reach.is_some_and(|r| r.hides((lo, hi)));
                // A caster counts when it is in a shadow map and its shadow can fall in view.
                let casts = m.cast_shadows && !m.blended();
                let in_shadow = casts
                    && layers.iter().any(|(f, light)| {
                        in_mask(light.culling_mask, m.layer)
                            && f.aabb_past_near(lo, hi)
                            && self.shadow_reaches(light, (lo, hi), &frustum, &view.camera)
                    });
                if in_view || in_shadow {
                    seen.push(m.visibility_key);
                }
            }
        }
        seen.sort_unstable();
        seen.dedup();
        self.seen = seen;
    }

    /// Each shadow layer's volume, with the light it is of.
    fn shadow_layers(plan: &shadows::ShadowPlan) -> Vec<(cull::Frustum, &Light)> {
        let mut layers = Vec::new();
        for slot in &plan.slots {
            let count = if slot.first == 0 {
                plan.cascades
            } else {
                plan.slots
                    .iter()
                    .map(|s| s.first)
                    .filter(|f| *f > slot.first)
                    .min()
                    .unwrap_or(shadows::MAX_CASCADES + plan.local_layers)
                    - slot.first
            };
            for m in plan
                .matrices
                .iter()
                .skip(slot.first as usize)
                .take(count as usize)
            {
                layers.push((cull::Frustum::from_view_proj(*m), &slot.light));
            }
        }
        layers
    }

    /// A mesh's box in the world: its bounds moved by its model, or a skinned mesh's bind-pose
    /// bounds under every bone (kept for the frame). `None` for a mesh not uploaded yet.
    fn world_box(&self, view: &WorldView, m: &MeshInstance) -> Option<(Vec3, Vec3)> {
        let (lo, hi) = self.meshes.get(&m.mesh)?.bounds;
        let Some((start, n)) = m.bones else {
            return Some(cull::transform_aabb(lo, hi, m.model));
        };
        let key = (view.bones.as_ptr() as usize, start, m.mesh);
        if let Some(b) = self.skin_boxes.borrow().get(&key) {
            return *b;
        }
        let bones = view
            .bones
            .get(start as usize..(start + n) as usize)
            .unwrap_or(&[]);
        let b = cull::skinned_aabb(lo, hi, bones);
        self.skin_boxes.borrow_mut().insert(key, b);
        b
    }

    /// Whether a caster's shadow from `light` can reach the view: for the sun, the box swept
    /// along the light past the shadow distance meets the frustum; for a local light, the light
    /// reaches into the frustum and the box is within its range.
    fn shadow_reaches(
        &self,
        light: &Light,
        (lo, hi): (Vec3, Vec3),
        frustum: &cull::Frustum,
        camera: &WorldCamera,
    ) -> bool {
        match light.kind {
            LightKind::Directional => {
                let dir = light.direction.normalize_or_zero();
                let reach = self.shadow_distance + ((lo + hi) * 0.5).distance(camera.position);
                let step = dir * reach;
                frustum.aabb(lo.min(lo + step), hi.max(hi + step))
            }
            _ => {
                let nearest = light.position.clamp(lo, hi);
                frustum.sphere(light.position, light.range)
                    && nearest.distance(light.position) <= light.range
            }
        }
    }

    /// `fog` as the pipeline runs it: without its light shafts where they're not supported.
    fn pipeline_fog(&self, fog: Option<Fog>) -> Option<Fog> {
        let shafts = self.pipeline.volumetric_fog;
        fog.map(|f| Fog {
            volumetric: f.volumetric.filter(|_| shafts),
            ..f
        })
    }

    /// Camera `key`'s exposure as last read back from the GPU (a frame or two late; None
    /// before the post pass has run for it).
    pub fn exposure(&self, key: u32) -> Option<ExposureReading> {
        self.bloom_pass.as_ref().and_then(|b| b.exposure(key))
    }

    /// Makes camera `key`'s auto exposure jump straight to what it sees next frame,
    /// instead of easing there (after a cut or a scene load).
    pub fn reset_exposure(&mut self, key: u32) {
        if let Some(b) = self.bloom_pass.as_mut() {
            b.reset_exposure(key);
        }
    }

    pub fn has_mesh(&self, id: u32) -> bool {
        self.meshes.contains_key(&id)
    }

    /// Sets (or replaces) texture `id`, drawn with [`WorldTexture::Asset`]. Invalid
    /// data is refused.
    pub fn set_texture(&mut self, gpu: &Gpu, id: u32, data: &TextureData) -> bool {
        if !data.is_valid() {
            return false;
        }
        let max = gpu.device.limits().max_texture_dimension_2d;
        if data.width > max || data.height > max {
            tracing::warn!(
                "texture {id} is {}x{}, over the GPU's {max}",
                data.width,
                data.height
            );
            return false;
        }
        let levels: Vec<Vec<u8>> = texture::mip_chain(data)
            .into_iter()
            .map(|(px, _, _)| px)
            .collect();
        self.upload_texture(
            gpu,
            id,
            &CookedTexture {
                width: data.width,
                height: data.height,
                format: TexelFormat::Rgba8,
                srgb: data.srgb,
                mips: levels,
                filter: data.filter,
                wrap: data.wrap,
                anisotropy: data.anisotropy,
            },
        );
        true
    }

    /// Whether the GPU takes block-compressed (BC) textures.
    pub fn supports_bc(gpu: &Gpu) -> bool {
        gpu.device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
    }

    /// Sets texture `id` from a cooked texture (`platypus-app --cook`): its levels upload as they
    /// are. Refused when invalid, too big, or block-compressed on a GPU without BC.
    pub fn set_cooked_texture(&mut self, gpu: &Gpu, id: u32, data: &CookedTexture) -> bool {
        let max = gpu.device.limits().max_texture_dimension_2d;
        if !data.is_valid()
            || data.width > max
            || data.height > max
            || (data.format != TexelFormat::Rgba8 && !Self::supports_bc(gpu))
        {
            return false;
        }
        self.upload_texture(gpu, id, data);
        true
    }

    fn upload_texture(&mut self, gpu: &Gpu, id: u32, data: &CookedTexture) {
        // sRGB textures can also be read as data (a photo used as a bump map), and data
        // textures are the same either way.
        let (format, linear_format) = match data.format {
            TexelFormat::Rgba8 => (
                if data.srgb {
                    wgpu::TextureFormat::Rgba8UnormSrgb
                } else {
                    wgpu::TextureFormat::Rgba8Unorm
                },
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            TexelFormat::Bc1 => (
                if data.srgb {
                    wgpu::TextureFormat::Bc1RgbaUnormSrgb
                } else {
                    wgpu::TextureFormat::Bc1RgbaUnorm
                },
                wgpu::TextureFormat::Bc1RgbaUnorm,
            ),
            TexelFormat::Bc3 => (
                if data.srgb {
                    wgpu::TextureFormat::Bc3RgbaUnormSrgb
                } else {
                    wgpu::TextureFormat::Bc3RgbaUnorm
                },
                wgpu::TextureFormat::Bc3RgbaUnorm,
            ),
        };
        let levels = &data.mips;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("world texture"),
            size: wgpu::Extent3d {
                width: data.width,
                height: data.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[linear_format],
        });
        for (level, px) in levels.iter().enumerate() {
            // Block-compressed levels copy whole blocks: their physical size.
            let size = texture
                .size()
                .mip_level_size(level as u32, wgpu::TextureDimension::D2)
                .physical_size(format);
            let (bw, _) = format.block_dimensions();
            let row = size.width / bw * data.format.block_bytes();
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                px,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
                size,
            );
        }
        let srgb = texture.create_view(&Default::default());
        let linear = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(linear_format),
            ..Default::default()
        });
        self.textures.insert(
            id,
            AssetTexture {
                srgb,
                linear,
                target: None,
                filter: data.filter,
                wrap: data.wrap,
                anisotropy: data.anisotropy,
                size: (data.width, data.height),
            },
        );
        self.material_groups.clear();
    }

    /// Render texture `id`'s colour (sRGB) view, for the UI renderer to show it
    /// (`UiRenderer::set_external_image`); `None` for another kind of texture.
    pub fn render_texture_view(&self, id: u32) -> Option<&wgpu::TextureView> {
        let t = self.textures.get(&id)?;
        t.target.as_ref().map(|_| &t.srgb)
    }

    /// Makes (or resizes) render texture `id`: cameras draw into it
    /// ([`WorldView::target_texture`]) and materials and sprites show it like any kept
    /// texture ([`WorldTexture::Asset`]).
    pub fn set_render_texture(&mut self, gpu: &Gpu, id: u32, size: (u32, u32)) {
        let max = gpu.device.limits().max_texture_dimension_2d;
        let size = (size.0.clamp(1, max), size.1.clamp(1, max));
        if self
            .textures
            .get(&id)
            .is_some_and(|t| t.target.is_some() && t.size == size)
        {
            return;
        }
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render texture"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RENDER_TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        let srgb = texture.create_view(&Default::default());
        let linear = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        self.textures.insert(
            id,
            AssetTexture {
                target: Some(srgb.clone()),
                srgb,
                linear,
                filter: TextureFilter::Bilinear,
                wrap: TextureWrap::Clamp,
                anisotropy: 1,
                size,
            },
        );
        self.material_groups.clear();
    }

    pub fn remove_texture(&mut self, id: u32) {
        if id == MISSING_TEXTURE || id == DEFAULT_PARTICLE_TEXTURE {
            return;
        }
        self.textures.remove(&id);
        self.material_groups.clear();
    }

    /// A kept texture's size in pixels.
    pub fn texture_size(&self, id: u32) -> Option<(u32, u32)> {
        self.textures.get(&id).map(|t| t.size)
    }

    /// Sets (or clears) the sky: the environment reflections and sky light come from, and
    /// what views with `skybox` show behind everything. Redrawn on the next render.
    pub fn set_sky(&mut self, sky: Option<Sky>) {
        if self.sky != sky {
            self.sky = sky;
            self.sky_dirty = true;
        }
    }

    pub fn sky(&self) -> Option<&Sky> {
        self.sky.as_ref()
    }

    /// Draws the sky into a fresh environment map (or drops it with no sky).
    fn redraw_sky(&mut self, gpu: &Gpu) {
        let Some(sky) = self.sky else {
            self.env_map = environment::EnvMap::new(gpu, 1);
            return;
        };
        let pass = self
            .env_pass
            .get_or_insert_with(|| environment::EnvPass::new(gpu));
        let map = environment::EnvMap::new(gpu, 128);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        pass.draw_sky(gpu, &mut encoder, &map, &sky);
        gpu.queue.submit([encoder.finish()]);
        self.env_map = map;
    }

    /// Plans and draws this frame's shadow maps from `view` (the main screen view):
    /// every view drawn this frame reads them.
    fn draw_shadows(
        &mut self,
        gpu: &Gpu,
        view: Option<&WorldView>,
        textures: &[&wgpu::TextureView],
    ) {
        let plan = match view {
            Some(v) if self.shadow_quality != ShadowQuality::Disable => shadows::plan(
                &v.camera,
                &v.lights,
                shadows::PlanSettings {
                    distance: self.shadow_distance,
                    resolution: self.shadow_resolution,
                    local_resolution: (self.shadow_resolution / 2).max(256),
                    cascades: self.shadow_cascades,
                    local_lights: self.pipeline.additional_light_shadows,
                },
            ),
            _ => shadows::ShadowPlan {
                matrices: vec![Mat4::IDENTITY; shadows::MAX_CASCADES as usize],
                ..Default::default()
            },
        };
        let max = gpu.device.limits().max_texture_dimension_2d;
        let res = self.shadow_resolution.clamp(16, max);
        let local_res = (res / 2).max(16);
        if self.sun_shadows.res != res || self.sun_shadows.layers.len() < plan.cascades as usize {
            let layers = plan
                .cascades
                .max(self.sun_shadows.layers.len() as u32)
                .max(1);
            self.sun_shadows = ShadowArray::new(gpu, "sun shadows", res, layers);
        }
        if self.local_shadows.res != local_res
            || self.local_shadows.layers.len() < plan.local_layers as usize
        {
            let layers = plan
                .local_layers
                .max(self.local_shadows.layers.len() as u32)
                .max(1);
            self.local_shadows = ShadowArray::new(gpu, "light shadows", local_res, layers);
        }
        let matrices: Vec<[[f32; 4]; 4]> =
            plan.matrices.iter().map(|m| m.to_cols_array_2d()).collect();
        Self::upload(gpu, &mut self.shadow_matrices, "shadow matrices", &matrices);
        let Some(view) = view.filter(|_| !plan.slots.is_empty()) else {
            self.shadow_plan = plan;
            return;
        };
        // The casters, once per culling mask among the casting lights.
        let mut masks: Vec<u32> = Vec::new();
        for slot in &plan.slots {
            if !masks.contains(&slot.light.culling_mask) {
                masks.push(slot.light.culling_mask);
            }
        }
        struct ShadowRun {
            mesh: u32,
            material: MaterialKey,
            cutout: bool,
            skinned: bool,
            range: Range<u32>,
        }
        let mut instances: Vec<GpuMesh> = Vec::new();
        let mut runs_by_mask: Vec<(u32, Vec<ShadowRun>)> = Vec::new();
        // A caster outside every layer of its lights (what lies before a layer's near plane
        // still casts into it) leaves the maps as they are.
        let layers = Self::shadow_layers(&plan);
        for &mask in &masks {
            let mut casters: Vec<&MeshInstance> = view
                .meshes
                .iter()
                .filter(|m| self.meshes.contains_key(&m.mesh))
                .filter(|m| m.cast_shadows && !m.blended() && in_mask(mask, m.layer))
                .filter(|m| {
                    self.world_box(view, m).is_none_or(|(lo, hi)| {
                        layers.iter().any(|(f, light)| {
                            light.culling_mask == mask && f.aabb_past_near(lo, hi)
                        })
                    })
                })
                .collect();
            // Skinned cut-outs cast solid shadows.
            let cutout = |m: &MeshInstance| m.mode == RenderMode::Cutout && m.bones.is_none();
            casters.sort_by_key(|m| {
                (
                    m.mesh,
                    m.bones.is_some(),
                    cutout(m),
                    cutout(m).then(|| m.material_key()),
                )
            });
            let mut runs: Vec<ShadowRun> = Vec::new();
            for m in casters {
                let i = instances.len() as u32;
                instances.push(GpuMesh {
                    model: m.model.to_cols_array_2d(),
                    color: m.color,
                    tiling: m.tiling,
                    maps: [
                        0.0,
                        0.0,
                        m.alpha_cutoff,
                        m.bones.map_or(0.0, |run| {
                            (self.bone_pack.borrow_mut().place(&view.bones, run) + 1) as f32
                        }),
                    ],
                    ..Zeroable::zeroed()
                });
                let key = if cutout(m) {
                    m.material_key()
                } else {
                    MaterialKey::default()
                };
                match runs.last_mut() {
                    Some(r)
                        if r.mesh == m.mesh
                            && r.cutout == cutout(m)
                            && r.skinned == m.bones.is_some()
                            && r.material == key
                            && r.range.end == i =>
                    {
                        r.range.end += 1
                    }
                    _ => runs.push(ShadowRun {
                        mesh: m.mesh,
                        material: key,
                        cutout: cutout(m),
                        skinned: m.bones.is_some(),
                        range: i..i + 1,
                    }),
                }
            }
            runs_by_mask.push((mask, runs));
        }
        self.stats.shadow_casters = instances.len();
        Self::upload(gpu, &mut self.shadow_casters, "shadow casters", &instances);
        self.bone_pack.get_mut().upload(gpu, &mut self.bones);
        // Cut-out casters need their textures.
        let mut groups: HashMap<MaterialKey, wgpu::BindGroup> = HashMap::new();
        for key in runs_by_mask
            .iter()
            .flat_map(|(_, runs)| runs.iter().map(|r| r.material))
            .chain(std::iter::once(MaterialKey::default()))
        {
            if let std::collections::hash_map::Entry::Vacant(e) = groups.entry(key) {
                e.insert(self.material_group(gpu, key, textures, &[]));
            }
        }
        // One globals slot per layer, its matrix as the view.
        let layer_count = shadows::MAX_CASCADES as usize + plan.local_layers as usize;
        if self.shadow_globals.size() < self.stride * layer_count as u64 {
            self.shadow_globals = Self::buffer(
                gpu,
                "shadow globals",
                self.stride * layer_count.next_power_of_two() as u64,
                wgpu::BufferUsages::UNIFORM,
            );
        }
        for (layer, m) in plan.matrices.iter().enumerate() {
            let globals = GpuGlobals {
                view_proj: m.to_cols_array_2d(),
                ..Zeroable::zeroed()
            };
            gpu.queue.write_buffer(
                &self.shadow_globals,
                self.stride * layer as u64,
                bytemuck::bytes_of(&globals),
            );
        }
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow casters"),
            layout: &self.globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.shadow_globals,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<GpuGlobals>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.quads.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.shadow_casters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.shadow_casters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.bones.as_entire_binding(),
                },
            ],
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        for slot in &plan.slots {
            let runs = &runs_by_mask
                .iter()
                .find(|(mask, _)| *mask == slot.light.culling_mask)
                .expect("made above")
                .1;
            let layers: Vec<(u32, &wgpu::TextureView)> = if slot.first < shadows::MAX_CASCADES {
                (0..plan.cascades)
                    .map(|c| (c, &self.sun_shadows.layers[c as usize]))
                    .collect()
            } else {
                let first = slot.first - shadows::MAX_CASCADES;
                let count = if slot.light.kind == LightKind::Point {
                    6
                } else {
                    1
                };
                (first..first + count)
                    .map(|l| {
                        (
                            shadows::MAX_CASCADES + l,
                            &self.local_shadows.layers[l as usize],
                        )
                    })
                    .collect()
            };
            for (matrix, target) in layers {
                self.stats.shadow_maps += 1;
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("world shadow map"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: target,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_bind_group(0, &group, &[(self.stride * u64::from(matrix)) as u32]);
                for run in runs {
                    if run.cutout {
                        pass.set_pipeline(&self.shadow_cutout_pipeline);
                        pass.set_bind_group(1, &groups[&run.material], &[]);
                    } else if run.skinned {
                        pass.set_pipeline(&self.shadow_skinned_pipeline);
                    } else {
                        pass.set_pipeline(&self.shadow_pipeline);
                    }
                    if let Some(mesh) = self.meshes.get(&run.mesh) {
                        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                        if let (true, Some(skin)) = (run.skinned, &mesh.skin) {
                            pass.set_vertex_buffer(1, skin.slice(..));
                        }
                        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..mesh.count, 0, run.range.clone());
                    }
                }
            }
        }
        gpu.queue.submit([encoder.finish()]);
        self.shadow_plan = plan;
    }

    /// The most samples per pixel up to `wanted` that the GPU can render `format` (and
    /// depth) with.
    fn supported_samples(gpu: &Gpu, format: wgpu::TextureFormat, wanted: u32) -> u32 {
        // Without the adapter-specific feature, only WebGPU's guaranteed 1 and 4 work.
        let specific = gpu
            .device
            .features()
            .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        let color = gpu.adapter.get_texture_format_features(format).flags;
        let depth = gpu.adapter.get_texture_format_features(DEPTH_FORMAT).flags;
        [8, 4, 2]
            .into_iter()
            .filter(|&n| specific || n == 4)
            .find(|&n| {
                n <= wanted && color.sample_count_supported(n) && depth.sample_count_supported(n)
            })
            .unwrap_or(1)
    }

    /// The multisampled colour target for `size` and `format`.
    fn msaa_target(
        &mut self,
        gpu: &Gpu,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> wgpu::TextureView {
        if self.msaa_targets.len() > 4 && !self.msaa_targets.contains_key(&(size, format, samples))
        {
            self.msaa_targets.clear();
        }
        self.msaa_targets
            .entry((size, format, samples))
            .or_insert_with(|| {
                gpu.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("world msaa"),
                        size: wgpu::Extent3d {
                            width: size.0,
                            height: size.1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            })
            .clone()
    }

    fn pipelines(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, samples: u32) -> Pipelines {
        let (layout, module) = (&self.layout, &self.module);
        let blend = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX_ATTRIBUTES,
        };
        let skin_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SkinVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &SKIN_ATTRIBUTES,
        };
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GizmoLine>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &LINE_ATTRIBUTES,
        };
        let fill_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<FillVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &FILL_ATTRIBUTES,
        };
        let stroke_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Stroke>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &STROKE_ATTRIBUTES,
        };
        let make = |label: &str,
                    entry: (&str, &str),
                    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                    cull: Option<wgpu::Face>,
                    depth: (wgpu::CompareFunction, bool),
                    blend: Option<wgpu::BlendState>| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some(entry.0),
                        compilation_options: Default::default(),
                        buffers,
                    },
                    primitive: wgpu::PrimitiveState {
                        cull_mode: cull,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(depth.1),
                        depth_compare: Some(depth.0),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: samples,
                        ..Default::default()
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry.1),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        };
        let less = wgpu::CompareFunction::Less;
        // Quads at the same depth (planes) draw in their sorted order: a later one isn't hidden
        // by the depth an earlier one's soft edge wrote. Flat quads (2D sprites) and glows write no
        // depth at all (the `quad_glow` pipeline): among themselves only their sorted order
        // decides, so a spinning sprite that tilts by a rounding error isn't cut in half by
        // another at the same depth.
        let less_equal = wgpu::CompareFunction::LessEqual;
        self.pipelines
            .entry((format, samples))
            .or_insert_with(|| Pipelines {
                // Planes are seen from both sides.
                quad: make(
                    "world quads",
                    ("vs_quad", "fs_quad"),
                    &[],
                    None,
                    (less_equal, true),
                    blend,
                ),
                quad_glow: make(
                    "world glowing quads",
                    ("vs_quad", "fs_quad"),
                    &[],
                    None,
                    (less_equal, false),
                    blend,
                ),
                // The background replaces colour and resets depth in the viewport.
                fill: make(
                    "world background",
                    ("vs_fill", "fs_fill"),
                    &[],
                    None,
                    (wgpu::CompareFunction::Always, true),
                    None,
                ),
                grid: make(
                    "world grid",
                    ("vs_fill", "fs_grid"),
                    &[],
                    None,
                    (less, false),
                    blend,
                ),
                lines: make(
                    "world gizmo lines",
                    ("vs_line", "fs_line"),
                    &[Some(line_layout.clone())],
                    None,
                    (less_equal, false),
                    blend,
                ),
                lines_hidden: make(
                    "world hidden gizmo lines",
                    ("vs_line", "fs_line_hidden"),
                    &[Some(line_layout.clone())],
                    None,
                    (wgpu::CompareFunction::Greater, false),
                    blend,
                ),
                fills: make(
                    "world fills",
                    ("vs_shape", "fs_shape"),
                    &[Some(fill_layout.clone())],
                    None,
                    (less_equal, false),
                    blend,
                ),
                strokes: make(
                    "world strokes",
                    ("vs_stroke", "fs_stroke"),
                    &[Some(stroke_layout.clone())],
                    None,
                    (less_equal, false),
                    blend,
                ),
                mesh: make(
                    "world meshes",
                    ("vs_mesh", "fs_mesh"),
                    &[Some(vertex_layout.clone())],
                    Some(wgpu::Face::Back),
                    (less, true),
                    blend,
                ),
                mesh_glow: make(
                    "world glowing meshes",
                    ("vs_mesh", "fs_mesh"),
                    &[Some(vertex_layout.clone())],
                    None,
                    (less, false),
                    blend,
                ),
                // See-through meshes don't hide what's drawn after them.
                mesh_blend: make(
                    "world see-through meshes",
                    ("vs_mesh", "fs_mesh"),
                    &[Some(vertex_layout.clone())],
                    Some(wgpu::Face::Back),
                    (less, false),
                    blend,
                ),
                mesh_double: make(
                    "world double-sided meshes",
                    ("vs_mesh", "fs_mesh"),
                    &[Some(vertex_layout.clone())],
                    None,
                    (less, true),
                    blend,
                ),
                mesh_blend_double: make(
                    "world double-sided see-through meshes",
                    ("vs_mesh", "fs_mesh"),
                    &[Some(vertex_layout.clone())],
                    None,
                    (less, false),
                    blend,
                ),
                skinned: [
                    (Some(wgpu::Face::Back), true),
                    (None, true),
                    (Some(wgpu::Face::Back), false),
                    (None, false),
                ]
                .map(|(cull, writes)| {
                    make(
                        "world skinned meshes",
                        ("vs_mesh_skinned", "fs_mesh"),
                        &[Some(vertex_layout.clone()), Some(skin_layout.clone())],
                        cull,
                        (less, writes),
                        blend,
                    )
                }),
            })
            .clone()
    }

    fn depth_view(&mut self, gpu: &Gpu, size: (u32, u32), samples: u32) -> wgpu::TextureView {
        if let Some(view) = self.depth.get(&(size, samples)) {
            return view.clone();
        }
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("world depth"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            // Post-process passes that read depth bind it.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        // A few sizes at most: the screen, its mirrors, probe faces.
        if self.depth.len() > 8 {
            self.depth.clear();
        }
        self.depth.insert((size, samples), view.clone());
        view
    }

    /// Clears `target` to the camera's colour and draws `quads` into all of it.
    pub fn render(
        &mut self,
        gpu: &Gpu,
        target: &RenderTarget<'_>,
        camera: &WorldCamera,
        quads: &[WorldQuad],
        textures: &[&wgpu::TextureView],
    ) {
        let mut view = WorldView::new(*camera, [0.0, 0.0, 1.0, 1.0]);
        view.quads = quads.to_vec();
        self.render_views(gpu, target, &[view], textures);
    }

    /// Draws each camera's view into its viewport, in order. `target` is cleared to
    /// black first; parts no view covers stay black. `textures` are the textures
    /// quads and meshes refer to by index.
    pub fn render_views(
        &mut self,
        gpu: &Gpu,
        target: &RenderTarget<'_>,
        views: &[WorldView],
        textures: &[&wgpu::TextureView],
    ) {
        self.stats = WorldStats::default();
        self.post_time += self.grading.delta_time;
        self.post_frame = self.post_frame.wrapping_add(1);
        self.skin_boxes.get_mut().clear();
        self.bone_pack.get_mut().clear();
        // Shadow maps once, from the biggest screen view, for every view this frame.
        let main = views
            .iter()
            .filter(|v| v.target_texture.is_none())
            .max_by(|a, b| {
                let area = |v: &WorldView| v.viewport[2] * v.viewport[3];
                area(a).total_cmp(&area(b))
            })
            .or_else(|| views.first());
        self.draw_shadows(gpu, main, textures);
        self.note_seen(views);
        // Cameras with a target texture first, each texture's in order, so the screen's
        // cameras see this frame's render.
        let mut targets: Vec<u32> = Vec::new();
        for v in views {
            if let Some(t) = v.target_texture
                && !targets.contains(&t)
            {
                targets.push(t);
            }
        }
        for id in targets {
            self.render_into_texture(gpu, id, views, textures);
        }
        let screen: Vec<WorldView>;
        let views = if views.iter().any(|v| v.target_texture.is_some()) {
            screen = views
                .iter()
                .filter(|v| v.target_texture.is_none())
                .cloned()
                .collect();
            &screen[..]
        } else {
            views
        };
        let scale = if self.render_scale.is_finite() {
            self.render_scale.clamp(0.25, 2.0)
        } else {
            1.0
        };
        let scaled = (
            ((target.size.0 as f32 * scale).round() as u32).max(1),
            ((target.size.1 as f32 * scale).round() as u32).max(1),
        );
        if scaled == target.size {
            self.render_views_unscaled(gpu, target, views, textures);
            return;
        }
        // Drawn at the scaled size, then stretched onto the target.
        let up = self
            .upscale
            .get_or_insert_with(|| upscale::Upscale::new(gpu));
        let texture = up.target(gpu, scaled, target.format);
        let rt = RenderTarget {
            view: &texture,
            format: target.format,
            size: scaled,
            scale: target.scale * scale,
        };
        self.render_views_unscaled(gpu, &rt, views, textures);
        if let Some(up) = self.upscale.as_mut() {
            up.blit(gpu, &texture, target.view, target.format);
        }
    }

    /// Draws the views targeting render texture `id` into it. The texture can't be read
    /// while it's drawn, so in its own cameras' views things showing it show plain colour
    /// (other render textures show their latest picture).
    fn render_into_texture(
        &mut self,
        gpu: &Gpu,
        id: u32,
        views: &[WorldView],
        textures: &[&wgpu::TextureView],
    ) {
        let Some((view, size)) = self
            .textures
            .get(&id)
            .and_then(|t| Some((t.target.clone()?, t.size)))
        else {
            return;
        };
        let own = |t: &mut Option<WorldTexture>| {
            if *t == Some(WorldTexture::Asset(id)) {
                *t = None;
            }
        };
        let mine: Vec<WorldView> = views
            .iter()
            .filter(|v| v.target_texture == Some(id))
            .map(|v| {
                let mut v = v.clone();
                v.target_texture = None;
                for m in &mut v.meshes {
                    own(&mut m.texture);
                    for t in [
                        &mut m.maps.normal,
                        &mut m.maps.metallic_gloss,
                        &mut m.maps.occlusion,
                        &mut m.maps.emission,
                        &mut m.maps.detail,
                    ] {
                        own(t);
                    }
                }
                for q in &mut v.quads {
                    own(&mut q.texture);
                }
                // The screen's views refresh probes.
                for p in &mut v.probes {
                    p.render = false;
                }
                v
            })
            .collect();
        let rt = RenderTarget {
            view: &view,
            format: RENDER_TEXTURE_FORMAT,
            size,
            scale: 1.0,
        };
        self.render_views_unscaled(gpu, &rt, &mine, textures);
    }

    fn render_views_unscaled(
        &mut self,
        gpu: &Gpu,
        target: &RenderTarget<'_>,
        views: &[WorldView],
        textures: &[&wgpu::TextureView],
    ) {
        self.update_probes(gpu, views, textures);
        if !views
            .iter()
            .any(|v| v.meshes.iter().any(|m| m.mirror.is_some()))
        {
            self.render_views_with(gpu, target, views, textures, &[], true);
            return;
        }
        // Mirrors: each mirror in the main view (the biggest) gets the scene rendered from
        // its reflected camera first; other views see the sky in them.
        let main = (0..views.len())
            .max_by(|&a, &b| {
                let area = |v: &WorldView| v.viewport[2] * v.viewport[3];
                area(&views[a]).total_cmp(&area(&views[b]))
            })
            .unwrap_or(0);
        let mut views = views.to_vec();
        let mut reflections = Vec::new();
        for i in 0..views[main].meshes.len() {
            let Some(mirror) = views[main].meshes[i].mirror else {
                continue;
            };
            let k = reflections.len();
            let view = reflection_view(&views[main], mirror);
            let texture = self.reflection_target(gpu, k, target.size);
            let rt = RenderTarget {
                view: &texture,
                format: bloom::HDR_FORMAT,
                size: target.size,
                scale: target.scale,
            };
            self.render_views_with(gpu, &rt, &[view], textures, &[], false);
            views[main].meshes[i].maps.reflection = Some(WorldTexture::Reflection(k));
            reflections.push(texture);
        }
        for (j, v) in views.iter_mut().enumerate() {
            if j != main {
                for m in &mut v.meshes {
                    m.mirror = None;
                }
            }
        }
        self.render_views_with(gpu, target, &views, textures, &reflections, true);
    }

    /// Renders the main view's reflection probes that need it: six 90° views from each
    /// probe into a 2D target, copied into its cubemap's faces, then blurred down.
    fn update_probes(&mut self, gpu: &Gpu, views: &[WorldView], textures: &[&wgpu::TextureView]) {
        let main = (0..views.len()).max_by(|&a, &b| {
            let area = |v: &WorldView| v.viewport[2] * v.viewport[3];
            area(&views[a]).total_cmp(&area(&views[b]))
        });
        let Some(main) = main else {
            return;
        };
        let main_view = &views[main];
        self.probe_maps
            .retain(|id, _| main_view.probes.iter().any(|p| p.id == *id));
        for probe in main_view.probes.clone() {
            let res = probe.resolution.clamp(16, 1024).next_power_of_two();
            let current = self
                .probe_maps
                .get(&probe.id)
                .is_some_and(|m| m.size == res);
            if current && !probe.render {
                continue;
            }
            let map = match self.probe_maps.remove(&probe.id) {
                Some(m) if m.size == res => m,
                _ => environment::EnvMap::new(gpu, res),
            };
            let face_target = self.probe_face_target(gpu, res);
            for face in 0..6u32 {
                let (forward, up) = environment::face_camera(face);
                let world = platypus_core::glam::camera::rh::view::look_at_mat4(
                    probe.position,
                    probe.position + forward,
                    up,
                )
                .inverse();
                let mut v = main_view.clone();
                v.grid = false;
                v.lines.clear();
                v.camera = WorldCamera::new(
                    world,
                    Projection::Perspective {
                        fov_y_degrees: 90.0,
                    },
                    0.05,
                    1000.0,
                    (res, res),
                    main_view.camera.clear,
                );
                v.viewport = [0.0, 0.0, 1.0, 1.0];
                v.clip_plane = None;
                // Probes don't see probes or mirrors (those reflect the sky here), nor
                // layers outside their mask.
                v.probes.clear();
                v.meshes.retain(|m| in_mask(probe.culling_mask, m.layer));
                v.quads.retain(|q| in_mask(probe.culling_mask, q.layer));
                for m in &mut v.meshes {
                    m.mirror = None;
                }
                let rt = RenderTarget {
                    view: &face_target,
                    format: bloom::HDR_FORMAT,
                    size: (res, res),
                    scale: 1.0,
                };
                self.render_views_with(gpu, &rt, &[v], textures, &[], false);
                let pass = self
                    .env_pass
                    .get_or_insert_with(|| environment::EnvPass::new(gpu));
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                pass.copy_face(gpu, &mut encoder, &face_target, &map, face);
                gpu.queue.submit([encoder.finish()]);
            }
            let pass = self
                .env_pass
                .get_or_insert_with(|| environment::EnvPass::new(gpu));
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            pass.blur_mips(gpu, &mut encoder, &map);
            gpu.queue.submit([encoder.finish()]);
            self.probe_maps.insert(probe.id, map);
        }
    }

    fn probe_face_target(&mut self, gpu: &Gpu, res: u32) -> wgpu::TextureView {
        self.probe_faces
            .entry(res)
            .or_insert_with(|| {
                gpu.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("probe face"),
                        size: wgpu::Extent3d {
                            width: res,
                            height: res,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: bloom::HDR_FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            })
            .clone()
    }

    /// The bind group for a material's textures. Its sampler is the main texture's
    /// (with the quality's anisotropy and mip limit); maps share it.
    fn material_group(
        &mut self,
        gpu: &Gpu,
        key: MaterialKey,
        frame: &[&wgpu::TextureView],
        reflections: &[wgpu::TextureView],
    ) -> wgpu::BindGroup {
        let sampler_key = match key.base {
            Some(WorldTexture::Asset(id)) => {
                let t = self
                    .textures
                    .get(&id)
                    .unwrap_or(&self.textures[&MISSING_TEXTURE]);
                Some(SamplerKey {
                    filter: t.filter,
                    wrap: t.wrap,
                    anisotropy: self.anisotropic.level(t.anisotropy),
                    mip_limit: self.texture_mip_limit,
                })
            }
            _ => None,
        };
        let sampler = match sampler_key {
            Some(k) => self
                .samplers
                .entry(k)
                .or_insert_with(|| texture::sampler(&gpu.device, k))
                .clone(),
            None => self.sampler.clone(),
        };
        let view = |t: Option<WorldTexture>, linear: bool| -> &wgpu::TextureView {
            match t {
                None => &self.white,
                Some(WorldTexture::Frame(i)) => frame.get(i).copied().unwrap_or(&self.white),
                Some(WorldTexture::Asset(id)) => {
                    let a = self
                        .textures
                        .get(&id)
                        .unwrap_or(&self.textures[&MISSING_TEXTURE]);
                    if linear { &a.linear } else { &a.srgb }
                }
                Some(WorldTexture::Reflection(k)) => reflections.get(k).unwrap_or(&self.white),
            }
        };
        let views = [
            (0, view(key.base, false)),
            (2, view(key.normal, true)),
            (3, view(key.metallic_gloss, !key.metallic_srgb)),
            (4, view(key.occlusion, true)),
            (5, view(key.emission, false)),
            (6, view(key.detail, false)),
            (7, view(key.reflection, false)),
        ];
        let mut entries: Vec<wgpu::BindGroupEntry<'_>> = views
            .iter()
            .map(|&(binding, v)| wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::TextureView(v),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&sampler),
        });
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world material"),
            layout: &self.texture_layout,
            entries: &entries,
        })
    }

    /// The texture mirror `k` reflects into, made (or remade) at `size`.
    fn reflection_target(&mut self, gpu: &Gpu, k: usize, size: (u32, u32)) -> wgpu::TextureView {
        if self.reflection_targets.len() <= k {
            self.reflection_targets.resize(k + 1, None);
        }
        if let Some((view, s)) = &self.reflection_targets[k]
            && *s == size
        {
            return view.clone();
        }
        let view = gpu
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("mirror reflection"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: bloom::HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        self.reflection_targets[k] = Some((view.clone(), size));
        view
    }

    fn render_views_with(
        &mut self,
        gpu: &Gpu,
        target: &RenderTarget<'_>,
        views: &[WorldView],
        textures: &[&wgpu::TextureView],
        reflections: &[wgpu::TextureView],
        // The frame's last render (with MSAA), which post-processing applies to (not mirrors' and
        // probes', which are inputs to it).
        final_pass: bool,
    ) {
        let size = target.size;
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        // With post-processing, the world goes into an HDR texture first.
        let bloom = self
            .bloom
            .filter(|b| b.intensity > 0.0)
            .filter(|_| final_pass && self.pipeline.hdr);
        let main = (0..views.len()).max_by(|&a, &b| {
            let area = |v: &WorldView| v.viewport[2] * v.viewport[3];
            area(&views[a]).total_cmp(&area(&views[b]))
        });
        let fxaa = final_pass && main.is_some_and(|m| views[m].fxaa);
        let features = self.pipeline;
        let mut grading = self.grading;
        // Without HDR, light clips at white: no bloom, exposure or tonemapping.
        if !features.hdr {
            grading = Grading {
                delta_time: grading.delta_time,
                ..Grading::default()
            };
        }
        // Auto exposure fading out keeps the pass going until it has.
        let fading = self.bloom_pass.as_ref().is_some_and(|b| b.fading());
        let chain = final_pass && views.iter().any(|v| !v.post.is_empty());
        // Whether any pass reads depth, which the world pass then keeps.
        let keep_depth = chain && views.iter().any(|v| v.post.iter().any(|e| e.shader.depth));
        let post =
            final_pass && (bloom.is_some() || fxaa || fading || chain || !grading.is_identity());
        let hdr = post.then(|| {
            self.bloom_pass
                .get_or_insert_with(|| bloom::BloomPass::new(gpu))
                .hdr_view(gpu, size)
        });
        // FXAA reads the graded image, so that goes into a texture of its own.
        let fxaa_input = fxaa.then(|| {
            self.fxaa
                .get_or_insert_with(|| fxaa::Fxaa::new(gpu))
                .input(gpu, size, target.format)
        });
        // Post-process passes read the finished picture from a texture of their own.
        let chain_input = chain.then(|| {
            self.post_chain
                .get_or_insert_with(|| post::PostChain::new(gpu))
                .input(gpu, size, target.format)
        });
        let (color_view, color_format) = match &hdr {
            Some(view) => (view, bloom::HDR_FORMAT),
            None => (target.view, target.format),
        };
        // Mirrors and probes render without MSAA.
        let wanted = if final_pass { self.msaa } else { 1 };
        let samples = Self::supported_samples(gpu, color_format, wanted);
        let pipelines = self.pipelines(gpu, color_format, samples);
        let depth = self.depth_view(gpu, size, samples);
        let msaa = (samples > 1).then(|| self.msaa_target(gpu, size, color_format, samples));
        if self.sky_dirty {
            self.sky_dirty = false;
            self.redraw_sky(gpu);
        }

        // Everything for every view in shared buffers, with runs per view: opaque meshes
        // (grouped by mesh), then see-through meshes and quads back to front.
        let mut quads: Vec<GpuQuad> = Vec::new();
        let mut meshes: Vec<GpuMesh> = Vec::new();
        let mut lights: Vec<GpuLight> = Vec::new();
        let mut per_view: Vec<ViewRuns> = Vec::new();
        for view in views {
            let eye = view.camera.position;
            // How far along the view something is: straight-line distance in
            // perspective, distance along the view direction in orthographic (where
            // everything at one depth is equally far, like Unity's 2D sorting).
            let forward = view.camera.forward;
            let orthographic = view.camera.orthographic;
            let depth_of = |model: Mat4| {
                let at = model.w_axis.xyz();
                if orthographic {
                    (at - eye).dot(forward)
                } else {
                    at.distance_squared(eye)
                }
            };
            // The probe a mesh reflects: the smallest rendered one whose box holds its
            // centre.
            let probe_for = |m: &MeshInstance| -> Option<&ReflectionProbe> {
                let at = m.model.w_axis.xyz();
                view.probes
                    .iter()
                    .filter(|p| self.probe_maps.contains_key(&p.id))
                    .filter(|p| ((at - p.position).abs() - p.half_extents).max_element() <= 0.0)
                    .min_by(|a, b| {
                        let size = |p: &ReflectionProbe| {
                            p.half_extents.x * p.half_extents.y * p.half_extents.z
                        };
                        size(a).total_cmp(&size(b))
                    })
            };
            let gpu_mesh = |m: &MeshInstance| {
                let normal = Mat3::from_mat4(m.model).inverse().transpose();
                GpuMesh {
                    model: m.model.to_cols_array_2d(),
                    normal: Mat4::from_mat3(normal).to_cols_array_2d(),
                    color: m.color,
                    emission: [m.emission[0], m.emission[1], m.emission[2], m.smoothness],
                    params: [
                        if m.unlit { 1.0 } else { 0.0 },
                        match m.glow {
                            MeshGlow::None => 0.0,
                            MeshGlow::Additive => 1.0,
                            MeshGlow::Beam => 2.0,
                        },
                        m.metallic,
                        f32::from(u8::from(m.mirror.is_some())),
                    ],
                    tiling: m.tiling,
                    probe: probe_for(m).map_or([0.0; 4], |p| {
                        let mips = self.probe_maps.get(&p.id).map_or(0, |map| map.mips);
                        [p.position.x, p.position.y, p.position.z, mips as f32]
                    }),
                    probe_box: probe_for(m).map_or([0.0; 4], |p| {
                        let h = p.half_extents;
                        [h.x, h.y, h.z, f32::from(u8::from(p.box_projection))]
                    }),
                    layer: [
                        1 << (m.layer & 31),
                        [
                            (m.maps.normal.is_some(), map_flags::NORMAL),
                            (m.maps.metallic_gloss.is_some(), map_flags::METALLIC_GLOSS),
                            (m.maps.gltf_packing, map_flags::GLTF),
                            (m.maps.occlusion.is_some(), map_flags::OCCLUSION),
                            (m.maps.emission.is_some(), map_flags::EMISSION),
                            (m.maps.detail.is_some(), map_flags::DETAIL),
                        ]
                        .iter()
                        .filter(|(on, _)| *on)
                        .fold(0, |bits, (_, bit)| bits | bit),
                        match m.mode {
                            RenderMode::Opaque if m.color[3] < 1.0 => 2,
                            RenderMode::Opaque => 0,
                            RenderMode::Cutout => 1,
                            RenderMode::Fade => 2,
                            RenderMode::Transparent => 3,
                        },
                        0,
                    ],
                    specular: m.specular.map_or([0.0; 4], |[r, g, b]| [r, g, b, 1.0]),
                    maps: [
                        m.maps.normal_scale,
                        m.maps.occlusion_strength.clamp(0.0, 1.0),
                        m.alpha_cutoff,
                        // The first bone in this frame's list, plus one (0: rigid).
                        m.bones.map_or(0.0, |run| {
                            (self.bone_pack.borrow_mut().place(&view.bones, run) + 1) as f32
                        }),
                    ],
                    detail: m.maps.detail_tiling,
                }
            };
            let known = |m: &&MeshInstance| self.meshes.contains_key(&m.mesh);
            let frustum = cull::Frustum::from_view_proj(view.camera.view_proj);
            // Past the fog's cull distance nothing shows (measured as the fog is).
            let fog_reach = view
                .fog
                .filter(|f| f.cull)
                .and_then(|f| f.cull_distance())
                .map(|d| cull::FogReach::new(&view.camera, d));
            // What the fog hid, counted as it's culled (in view, past the fog).
            let fog_hid = std::cell::Cell::new(0usize);
            let fogged = |b: (Vec3, Vec3)| {
                let hid = fog_reach.is_some_and(|r| r.hides(b));
                fog_hid.set(fog_hid.get() + usize::from(hid));
                hid
            };
            // Skinned meshes by their bind-pose box under every bone (they move away from
            // their bounds with their bones).
            let sees = |m: &MeshInstance| {
                self.world_box(view, m)
                    .is_none_or(|(lo, hi)| frustum.aabb(lo, hi) && !fogged((lo, hi)))
            };
            // Opaque meshes, grouped by mesh and texture (instancing).
            let mut opaque: Vec<&MeshInstance> = view
                .meshes
                .iter()
                .filter(known)
                .filter(|m| !m.blended())
                .filter(|m| sees(m))
                .collect();
            let cutout = |m: &MeshInstance| m.mode == RenderMode::Cutout;
            opaque.sort_by_key(|m| {
                (
                    m.mesh,
                    m.material_key(),
                    m.double_sided,
                    cutout(m),
                    m.bones.is_some(),
                    probe_for(m).map(|p| p.id),
                )
            });
            let mut opaque_runs: Vec<Run> = Vec::new();
            for m in opaque {
                let i = meshes.len() as u32;
                meshes.push(gpu_mesh(m));
                match opaque_runs.last_mut() {
                    Some(r)
                        if r.mesh == Some(m.mesh)
                            && r.material == m.material_key()
                            && r.double_sided == m.double_sided
                            && r.cutout == cutout(m)
                            && r.skinned == m.bones.is_some()
                            && r.probe == probe_for(m).map(|p| p.id)
                            && r.range.end == i =>
                    {
                        r.range.end += 1
                    }
                    _ => opaque_runs.push(Run {
                        mesh: Some(m.mesh),
                        material: m.material_key(),
                        double_sided: m.double_sided,
                        cutout: cutout(m),
                        skinned: m.bones.is_some(),
                        probe: probe_for(m).map(|p| p.id),
                        glow: false,
                        range: i..i + 1,
                    }),
                }
            }
            // See-through meshes and quads, back to front together.
            enum Item<'a> {
                Mesh(&'a MeshInstance),
                Quad(&'a WorldQuad),
            }
            let mut items: Vec<(i32, f32, Item)> = view
                .meshes
                .iter()
                .filter(known)
                .filter(|m| m.blended())
                .filter(|m| sees(m))
                .map(|m| (0, depth_of(m.model), Item::Mesh(m)))
                .chain(
                    view.quads
                        .iter()
                        .filter(|q| {
                            let (lo, hi) = cull::QUAD_BOUNDS;
                            let b = cull::transform_aabb(lo, hi, q.model);
                            frustum.aabb(b.0, b.1) && !fogged(b)
                        })
                        .map(|q| (q.order, depth_of(q.model), Item::Quad(q))),
                )
                .collect();
            // Lower orders first, then far to near.
            items.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
            let mut blended: Vec<(bool, Run)> = Vec::new();
            for (_, _, item) in items {
                match item {
                    Item::Mesh(m) => {
                        let i = meshes.len() as u32;
                        meshes.push(gpu_mesh(m));
                        blended.push((
                            true,
                            Run {
                                mesh: Some(m.mesh),
                                material: m.material_key(),
                                double_sided: m.double_sided,
                                cutout: false,
                                skinned: m.bones.is_some(),
                                probe: probe_for(m).map(|p| p.id),
                                glow: m.glow != MeshGlow::None,
                                range: i..i + 1,
                            },
                        ));
                    }
                    Item::Quad(q) => {
                        let i = quads.len() as u32;
                        let [r, g, b, a] = q.color;
                        // Premultiplied with no coverage: blending then just adds it.
                        let cover = if q.additive { 0.0 } else { a };
                        quads.push(GpuQuad {
                            model: q.model.to_cols_array_2d(),
                            color: [r * a, g * a, b * a, cover],
                            uv: q.uv,
                            params: [
                                f32::from(u8::from(matches!(
                                    q.texture,
                                    Some(WorldTexture::Asset(_))
                                ))),
                                0.0,
                                0.0,
                                0.0,
                            ],
                        });
                        match blended.last_mut() {
                            Some((false, run))
                                if run.material == MaterialKey::texture(q.texture)
                                    && run.glow == (q.additive || q.flat)
                                    && run.range.end == i =>
                            {
                                run.range.end += 1
                            }
                            _ => blended.push((
                                false,
                                Run {
                                    mesh: None,
                                    material: MaterialKey::texture(q.texture),
                                    double_sided: true,
                                    cutout: false,
                                    skinned: false,
                                    probe: None,
                                    glow: q.additive || q.flat,
                                    range: i..i + 1,
                                },
                            )),
                        }
                    }
                }
            }
            // Point and spot lights whose range doesn't reach the view are left out.
            let mut kept: Vec<usize> = view
                .lights
                .iter()
                .enumerate()
                .filter(|(_, l)| {
                    l.kind == LightKind::Directional
                        || (frustum.sphere(l.position, l.range)
                            && !fog_reach.is_some_and(|r| {
                                r.hides((
                                    l.position - Vec3::splat(l.range),
                                    l.position + Vec3::splat(l.range),
                                ))
                            }))
                })
                .map(|(i, _)| i)
                .collect();
            // The pipeline's cap (URP's): the main light (the brightest directional one) and
            // the nearest others.
            let cap = self.pipeline.max_additional_lights;
            if kept.len() > cap.saturating_add(1) {
                let strength = |k: &usize| view.lights[*k].color.iter().sum::<f32>();
                let main = kept
                    .iter()
                    .copied()
                    .filter(|&k| view.lights[k].kind == LightKind::Directional)
                    .max_by(|a, b| strength(a).total_cmp(&strength(b)));
                let eye = view.camera.position;
                let mut others: Vec<usize> =
                    kept.iter().copied().filter(|&k| Some(k) != main).collect();
                others.sort_by(|&a, &b| {
                    let d = |k: usize| match view.lights[k].kind {
                        LightKind::Directional => 0.0,
                        _ => view.lights[k].position.distance_squared(eye),
                    };
                    d(a).total_cmp(&d(b))
                });
                others.truncate(cap);
                kept = main.into_iter().chain(others).collect();
            }
            let drawn = opaque_runs
                .iter()
                .chain(blended.iter().filter(|(mesh, _)| *mesh).map(|(_, r)| r))
                .map(|r| r.range.len())
                .sum::<usize>();
            let all_meshes = view.meshes.iter().filter(known).count();
            let drawn_quads = blended
                .iter()
                .filter(|(mesh, _)| !*mesh)
                .map(|(_, r)| r.range.len())
                .sum::<usize>();
            self.stats.meshes += drawn;
            self.stats.culled_meshes += all_meshes - drawn;
            self.stats.quads += drawn_quads;
            self.stats.culled_quads += view.quads.len() - drawn_quads;
            self.stats.fog_culled += fog_hid.get();
            self.stats.lights += kept.len();
            self.stats.culled_lights += view.lights.len() - kept.len();
            let first_light = lights.len() as u32;
            lights.extend(kept.iter().map(|&k| &view.lights[k]).map(|l| {
                let (kind, spot) = match l.kind {
                    LightKind::Directional => (0.0, [0.0; 4]),
                    LightKind::Point => (1.0, [0.0; 4]),
                    LightKind::Spot {
                        inner_degrees,
                        outer_degrees,
                    } => (
                        2.0,
                        [
                            (inner_degrees.min(outer_degrees).to_radians() / 2.0).cos(),
                            (outer_degrees.to_radians() / 2.0).cos(),
                            0.0,
                            0.0,
                        ],
                    ),
                    LightKind::Pyramid {
                        inner_degrees,
                        outer_degrees,
                        aspect,
                        right,
                    } => {
                        let (r0, u0) = spot_axes(l.direction.normalize_or(Vec3::NEG_Z));
                        (
                            2.0,
                            [
                                (inner_degrees.min(outer_degrees).to_radians() / 2.0).cos(),
                                (outer_degrees.to_radians() / 2.0).cos(),
                                aspect.max(1e-3),
                                right.dot(u0).atan2(right.dot(r0)),
                            ],
                        )
                    }
                };
                GpuLight {
                    position: [l.position.x, l.position.y, l.position.z, kind],
                    direction: [l.direction.x, l.direction.y, l.direction.z, l.range],
                    color: [l.color[0], l.color[1], l.color[2], l.volumetric.max(0.0)],
                    spot,
                    mask: [l.culling_mask, 0, 0, 0],
                    shadow: self.shadow_plan.slot(l).map_or([-1.0, 0.0, 0.0, 0.0], |s| {
                        [
                            s.first as f32,
                            s.shadows.strength.clamp(0.0, 1.0),
                            // The PCF radius in texels: 0 is hard.
                            if s.shadows.soft && self.shadow_quality == ShadowQuality::All {
                                self.soft_shadow_radius.clamp(1, 2) as f32
                            } else {
                                0.0
                            },
                            s.texel_at_1m,
                        ]
                    }),
                }
            }));
            per_view.push((opaque_runs, blended, first_light..lights.len() as u32, kept));
        }
        Self::upload(gpu, &mut self.quads, "world quads", &quads);
        // Gizmo lines, every view's after the one before.
        let mut line_ranges = Vec::with_capacity(views.len());
        let mut all_lines: Vec<GizmoLine> = Vec::new();
        for view in views {
            let start = all_lines.len() as u32;
            all_lines.extend_from_slice(&view.lines);
            line_ranges.push(start..all_lines.len() as u32);
        }
        if !all_lines.is_empty() {
            let bytes = std::mem::size_of_val(all_lines.as_slice()) as u64;
            if self.lines.size() < bytes {
                self.lines = Self::buffer(
                    gpu,
                    "world gizmo lines",
                    bytes.next_power_of_two(),
                    wgpu::BufferUsages::VERTEX,
                );
            }
            gpu.queue
                .write_buffer(&self.lines, 0, bytemuck::cast_slice(&all_lines));
        }
        // Fill triangles, the same way.
        let mut fill_ranges = Vec::with_capacity(views.len());
        let mut all_fills: Vec<FillVertex> = Vec::new();
        for view in views {
            let start = all_fills.len() as u32;
            all_fills.extend_from_slice(&view.fills);
            fill_ranges.push(start..all_fills.len() as u32);
        }
        if !all_fills.is_empty() {
            let bytes = std::mem::size_of_val(all_fills.as_slice()) as u64;
            if self.fills.size() < bytes {
                self.fills = Self::buffer(
                    gpu,
                    "world fills",
                    bytes.next_power_of_two(),
                    wgpu::BufferUsages::VERTEX,
                );
            }
            gpu.queue
                .write_buffer(&self.fills, 0, bytemuck::cast_slice(&all_fills));
        }
        let shape_runs: Vec<Vec<ShapeRun>> = views.iter().map(|v| v.shapes.clone()).collect();
        // Strokes, the same way.
        let mut stroke_ranges = Vec::with_capacity(views.len());
        let mut all_strokes: Vec<Stroke> = Vec::new();
        for view in views {
            let start = all_strokes.len() as u32;
            all_strokes.extend_from_slice(&view.strokes);
            stroke_ranges.push(start..all_strokes.len() as u32);
        }
        if !all_strokes.is_empty() {
            let bytes = std::mem::size_of_val(all_strokes.as_slice()) as u64;
            if self.strokes.size() < bytes {
                self.strokes = Self::buffer(
                    gpu,
                    "world strokes",
                    bytes.next_power_of_two(),
                    wgpu::BufferUsages::VERTEX,
                );
            }
            gpu.queue
                .write_buffer(&self.strokes, 0, bytemuck::cast_slice(&all_strokes));
        }
        Self::upload(gpu, &mut self.mesh_instances, "world meshes", &meshes);
        self.bone_pack.get_mut().upload(gpu, &mut self.bones);
        self.stats.bones = self.bone_pack.get_mut().len();
        if self.globals.size() < self.stride * views.len().max(1) as u64 {
            self.globals = Self::buffer(
                gpu,
                "world globals",
                self.stride * views.len().next_power_of_two() as u64,
                wgpu::BufferUsages::UNIFORM,
            );
        }
        let has_env = self.sky.is_some();
        let env = [
            f32::from(u8::from(has_env)),
            self.environment_intensity,
            (self.env_map.mips - 1) as f32,
            f32::from(u8::from(has_env && self.ambient_from_sky)),
        ];
        // Pre-exposure (real-world light units, the final pass): each camera's last exposure,
        // so real-world light stays inside the HDR target's range; the post pass divides it out.
        let hd = final_pass && self.physical_units;
        let pres: Vec<f32> = views
            .iter()
            .map(|view| {
                hd.then(|| self.bloom_pass.as_mut()?.pre_exposure(view.exposure_key))
                    .flatten()
                    .unwrap_or(1.0)
            })
            .collect();
        for (i, view) in views.iter().enumerate() {
            let c = view.camera.clear;
            let p = view.camera.position;
            let count = per_view[i].2.len() as f32;
            let f = view.camera.forward;
            let mut globals = GpuGlobals {
                view_proj: view.camera.view_proj.to_cols_array_2d(),
                background: [
                    (c.r * c.a) as f32,
                    (c.g * c.a) as f32,
                    (c.b * c.a) as f32,
                    c.a as f32,
                ],
                camera: [p.x, p.y, p.z, 1.0],
                ambient: [view.ambient[0], view.ambient[1], view.ambient[2], count],
                shadow: [self.shadow_plan.cascades as f32, 0.0, 0.0, 0.0],
                cascade_texels: self.shadow_plan.cascade_texels,
                inv_view_proj: view.camera.view_proj.inverse().to_cols_array_2d(),
                viewport: viewport_px(view.viewport, size)
                    .map_or([0.0, 0.0, 1.0, 1.0], |p| p.map(|v| v as f32)),
                env,
                flags: [
                    f32::from(u8::from(view.skybox && has_env)),
                    size.0 as f32,
                    size.1 as f32,
                    f32::from(u8::from(view.grid)),
                ],
                clip: view.clip_plane.unwrap_or([0.0; 4]),
                view_dir: [f.x, f.y, f.z, f32::from(u8::from(view.camera.orthographic))],
                exposure: [pres[i], 0.0, 0.0, 0.0],
                ..Zeroable::zeroed()
            };
            globals.set_fog(self.pipeline_fog(view.fog).as_ref());
            globals.set_clouds(view.clouds.as_ref());
            globals.set_fog_volumes(&view.fog_volumes);
            gpu.queue.write_buffer(
                &self.globals,
                self.stride * i as u64,
                bytemuck::bytes_of(&globals),
            );
        }
        // One lights buffer per view (they differ), bound with the view.
        let light_buffers: Vec<wgpu::Buffer> = per_view
            .iter()
            .map(|(_, _, range, _)| {
                let slice = &lights[range.start as usize..range.end as usize];
                let mut buffer = Self::storage::<GpuLight>(gpu, "world lights", slice.len());
                Self::upload(gpu, &mut buffer, "world lights", slice);
                buffer
            })
            .collect();
        let globals_groups: Vec<wgpu::BindGroup> = light_buffers
            .iter()
            .map(|lights| {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("world globals"),
                    layout: &self.globals_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &self.globals,
                                offset: 0,
                                size: wgpu::BufferSize::new(
                                    std::mem::size_of::<GpuGlobals>() as u64
                                ),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: self.quads.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.mesh_instances.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: lights.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: self.bones.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        // Material bind groups: those of kept textures only are reused across frames;
        // ones with the frame's textures (panels, reflections) are made each time.
        let quality = (self.anisotropic, self.texture_mip_limit);
        if self.texture_quality != quality {
            self.texture_quality = quality;
            self.material_groups.clear();
        }
        let keys: std::collections::BTreeSet<MaterialKey> = per_view
            .iter()
            .flat_map(|(opaque, blended, ..)| opaque.iter().chain(blended.iter().map(|(_, r)| r)))
            .map(|r| r.material)
            .chain(std::iter::once(MaterialKey::default()))
            .collect();
        let mut frame_groups: HashMap<MaterialKey, wgpu::BindGroup> = HashMap::new();
        for key in keys {
            let per_frame = key.per_frame();
            if !per_frame && self.material_groups.contains_key(&key) {
                continue;
            }
            let group = self.material_group(gpu, key, textures, reflections);
            if per_frame {
                frame_groups.insert(key, group);
            } else {
                self.material_groups.insert(key, group);
            }
        }
        let group_for = |key: &MaterialKey| {
            frame_groups
                .get(key)
                .or_else(|| self.material_groups.get(key))
                .expect("made above")
        };
        let plain = group_for(&MaterialKey::default());

        // Shadows and the environment: the sky's, and one per reflection probe.
        let lighting_group = |cube: &wgpu::TextureView| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("world lighting"),
                layout: &self.shadow_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.sun_shadows.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&self.local_shadows.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: self.shadow_matrices.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(cube),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.env_sampler),
                    },
                ],
            })
        };
        let shadow_group = lighting_group(&self.env_map.cube);
        let probe_groups: HashMap<u32, wgpu::BindGroup> = self
            .probe_maps
            .iter()
            .map(|(id, map)| (*id, lighting_group(&map.cube)))
            .collect();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        // Views with light shafts fill their fog volumes first.
        let shafts = self.pipeline.volumetric_fog;
        let volume_groups: Vec<Option<wgpu::BindGroup>> = views
            .iter()
            .enumerate()
            .map(|(i, view)| {
                view.fog
                    .and_then(|f| f.volumetric)
                    .filter(|_| shafts)
                    .map(|v| {
                        let (poses, local_count) = local_poses(&view.lights);
                        let camera = volume::VolumeCamera {
                            view_proj: view.camera.view_proj,
                            position: view.camera.position,
                            forward: view.camera.forward,
                            orthographic: view.camera.orthographic,
                            distance: v.distance.max(1e-3),
                            reprojection: v.denoising.temporal(),
                            blend: 1.0 - v.history_weight.clamp(0.0, 0.98),
                            samples: v.samples.clamp(1, 8),
                            motion_samples: v.motion_samples.clamp(1, 8),
                            lights: view
                                .lights
                                .iter()
                                .filter(|l| {
                                    l.volumetric > 0.0
                                        && (l.kind == LightKind::Directional || l.shadows.is_some())
                                })
                                .fold(u64::from(self.shadow_plan.cascades), |k, l| {
                                    k.wrapping_mul(4)
                                        .wrapping_add(1 + u64::from(l.shadows.is_some()))
                                }),
                            local_poses: poses,
                            local_count,
                        };
                        self.volume.encode(
                            gpu,
                            &mut encoder,
                            i,
                            volume::SIZES[usize::from(v.detail.min(3))],
                            camera,
                            &globals_groups[i],
                            (self.stride * i as u64) as u32,
                            &shadow_group,
                        )
                    })
            })
            .collect();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                // With MSAA, draw into the multisampled target and resolve into the colour one.
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa.as_ref().unwrap_or(color_view),
                    depth_slice: None,
                    resolve_target: msaa.as_ref().map(|_| color_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: if msaa.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: if keep_depth {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            for (i, (view, (opaque, blended, ..))) in views.iter().zip(&per_view).enumerate() {
                let Some(px) = viewport_px(view.viewport, size) else {
                    continue;
                };
                pass.set_viewport(
                    px[0] as f32,
                    px[1] as f32,
                    px[2] as f32,
                    px[3] as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(px[0], px[1], px[2], px[3]);
                pass.set_bind_group(0, &globals_groups[i], &[(self.stride * i as u64) as u32]);
                pass.set_bind_group(1, plain, &[]);
                pass.set_bind_group(2, &shadow_group, &[]);
                pass.set_bind_group(
                    3,
                    volume_groups[i].as_ref().unwrap_or(self.volume.empty()),
                    &[],
                );
                pass.set_pipeline(&pipelines.fill);
                pass.draw(0..3, 0..1);

                let draw = |pass: &mut wgpu::RenderPass<'_>, run: &Run, blend: bool| {
                    pass.set_bind_group(1, group_for(&run.material), &[]);
                    let lighting = run.probe.and_then(|id| probe_groups.get(&id));
                    pass.set_bind_group(2, lighting.unwrap_or(&shadow_group), &[]);
                    match run.mesh.and_then(|m| self.meshes.get(&m)) {
                        Some(mesh) => {
                            pass.set_pipeline(match (run.glow, blend, run.double_sided) {
                                (true, ..) => &pipelines.mesh_glow,
                                (false, b, d) if run.skinned => {
                                    &pipelines.skinned[usize::from(d) + 2 * usize::from(b)]
                                }
                                (false, true, false) => &pipelines.mesh_blend,
                                (false, true, true) => &pipelines.mesh_blend_double,
                                (false, false, false) => &pipelines.mesh,
                                (false, false, true) => &pipelines.mesh_double,
                            });
                            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                            if let (true, Some(skin)) = (run.skinned, &mesh.skin) {
                                pass.set_vertex_buffer(1, skin.slice(..));
                            }
                            pass.set_index_buffer(
                                mesh.indices.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            pass.draw_indexed(0..mesh.count, 0, run.range.clone());
                        }
                        None if run.mesh.is_none() => {
                            pass.set_pipeline(if run.glow {
                                &pipelines.quad_glow
                            } else {
                                &pipelines.quad
                            });
                            pass.draw(0..6, run.range.clone());
                        }
                        None => {}
                    }
                };
                for run in opaque {
                    draw(&mut pass, run, false);
                }
                // The grid is see-through: over the opaque meshes, under the blended ones.
                if view.grid {
                    pass.set_bind_group(1, plain, &[]);
                    pass.set_bind_group(2, &shadow_group, &[]);
                    pass.set_pipeline(&pipelines.grid);
                    pass.draw(0..3, 0..1);
                }
                // 2D shapes before the see-through things (sprites and text show over them):
                // each shape's fill then its outline, in order; without runs, every fill then
                // every outline.
                let (fills, strokes) = (fill_ranges[i].clone(), stroke_ranges[i].clone());
                if !fills.is_empty() || !strokes.is_empty() {
                    pass.set_bind_group(1, plain, &[]);
                    pass.set_bind_group(2, &shadow_group, &[]);
                    let whole = [ShapeRun {
                        fills: 0..fills.len() as u32,
                        strokes: 0..strokes.len() as u32,
                    }];
                    let runs = if shape_runs[i].is_empty() {
                        &whole[..]
                    } else {
                        &shape_runs[i][..]
                    };
                    for run in runs {
                        if !run.fills.is_empty() {
                            pass.set_vertex_buffer(0, self.fills.slice(..));
                            pass.set_pipeline(&pipelines.fills);
                            pass.draw(
                                fills.start + run.fills.start..fills.start + run.fills.end,
                                0..1,
                            );
                        }
                        if !run.strokes.is_empty() {
                            pass.set_vertex_buffer(0, self.strokes.slice(..));
                            pass.set_pipeline(&pipelines.strokes);
                            pass.draw(
                                0..6,
                                strokes.start + run.strokes.start..strokes.start + run.strokes.end,
                            );
                        }
                    }
                }
                for (_, run) in blended {
                    draw(&mut pass, run, true);
                }
                // Gizmo lines last: hidden parts faint first, then the visible ones.
                let range = line_ranges[i].clone();
                if !range.is_empty() {
                    pass.set_bind_group(1, plain, &[]);
                    pass.set_bind_group(2, &shadow_group, &[]);
                    pass.set_vertex_buffer(0, self.lines.slice(..));
                    pass.set_pipeline(&pipelines.lines_hidden);
                    pass.draw(0..6, range.clone());
                    pass.set_pipeline(&pipelines.lines);
                    pass.draw(0..6, range);
                }
            }
        }
        if post && let Some(pass) = self.bloom_pass.as_mut() {
            let out = fxaa_input
                .as_ref()
                .or(chain_input.as_ref())
                .unwrap_or(target.view);
            let out = RenderTarget {
                view: out,
                format: target.format,
                size,
                scale: target.scale,
            };
            // Each view with its camera's exposure, the biggest first (it covers the rest).
            let mut order: Vec<usize> = (0..views.len()).collect();
            if let Some(m) = main {
                order.retain(|&i| i != m);
                order.insert(0, m);
            }
            let exposure_views: Vec<bloom::ExposureView> = order
                .iter()
                .filter_map(|&i| {
                    let rect = viewport_px(views[i].viewport, size)?;
                    Some(bloom::ExposureView {
                        key: views[i].exposure_key,
                        rect,
                    })
                })
                .collect();
            pass.encode(
                gpu,
                &mut encoder,
                &out,
                bloom.as_ref(),
                grading,
                &exposure_views,
            );
            if fxaa && let Some(f) = self.fxaa.as_mut() {
                let to = chain_input.as_ref().unwrap_or(target.view);
                f.encode(gpu, &mut encoder, to, size, target.format);
            }
            if chain && let Some(c) = self.post_chain.as_mut() {
                c.encode(
                    gpu,
                    &mut encoder,
                    &post::PostFrame {
                        views,
                        target: target.view,
                        format: target.format,
                        size,
                        depth: &depth,
                        multisampled: samples > 1,
                        time: self.post_time,
                        frame: self.post_frame,
                    },
                );
            }
        }
        gpu.queue.submit([encoder.finish()]);
        if post && let Some(pass) = self.bloom_pass.as_mut() {
            pass.after_submit(gpu);
        }
    }
}

/// The shadowed spot and point lights the light volume shows, and how many there are: it trusts
/// its history less as they move.
fn local_poses(lights: &[Light]) -> ([volume::LocalPose; volume::LOCAL_POSES], usize) {
    let mut poses = [volume::LocalPose::default(); volume::LOCAL_POSES];
    let mut n = 0;
    for l in lights
        .iter()
        .filter(|l| l.kind != LightKind::Directional && l.shadows.is_some() && l.volumetric > 0.0)
    {
        if let Some(p) = poses.get_mut(n) {
            *p = volume::LocalPose {
                position: l.position,
                direction: l.direction,
                range: l.range,
            };
        }
        n += 1;
    }
    (poses, n)
}

/// `view` seen in `mirror`: the camera reflected in its plane, and everything behind the
/// plane clipped away. The projection is flipped left to right so triangles keep their
/// winding (the reflection flips it once, the flip back); the mirror samples it flipped.
fn reflection_view(view: &WorldView, mirror: Mirror) -> WorldView {
    let n = mirror.normal.normalize_or(Vec3::Y);
    let d = -n.dot(mirror.point);
    // Reflection in the plane: x - 2 (n · x + d) n.
    let reflect = Mat4::from_cols(
        (Vec3::X - 2.0 * n.x * n).extend(0.0),
        (Vec3::Y - 2.0 * n.y * n).extend(0.0),
        (Vec3::Z - 2.0 * n.z * n).extend(0.0),
        (-2.0 * d * n).extend(1.0),
    );
    let flip = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0));
    let mut v = view.clone();
    v.grid = false;
    v.lines.clear();
    v.meshes
        .retain(|m| m.mirror.is_none() && in_mask(mirror.culling_mask, m.layer));
    v.quads.retain(|q| in_mask(mirror.culling_mask, q.layer));
    v.camera.view_proj = flip * view.camera.view_proj * reflect;
    v.camera.position = reflect.transform_point3(view.camera.position);
    v.camera.forward = reflect.transform_vector3(view.camera.forward);
    // Keep only what's in front of the mirror; a hair past the plane hides the seam.
    v.clip_plane = Some([n.x, n.y, n.z, d + 1e-3]);
    v
}

/// A normalized viewport in whole pixels inside the target (`None` when empty).
fn viewport_px(v: [f32; 4], size: (u32, u32)) -> Option<[u32; 4]> {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let x0 = (v[0] * w).round().clamp(0.0, w);
    let y0 = (v[1] * h).round().clamp(0.0, h);
    let x1 = ((v[0] + v[2]) * w).round().clamp(0.0, w);
    let y1 = ((v[1] + v[3]) * h).round().clamp(0.0, h);
    (x1 > x0 && y1 > y0).then_some([x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
}

/// A normalized viewport's size in pixels, for a camera's aspect ratio.
pub fn viewport_size(v: [f32; 4], size: (u32, u32)) -> (u32, u32) {
    viewport_px(v, size).map_or((1, 1), |p| (p[2], p[3]))
}
