// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What scripts put in the 3D world for the renderer: `Camera`, `PlaneRenderer`,
//! `MeshRenderer`, `SpriteRenderer`, `ParticleSystem` and `Light` components (`render.*`
//! ops, keyed by component id), read with their GameObjects' world transforms each frame, plus script
//! meshes (`render.mesh`) and textures (`render.texture`) waiting to be uploaded and
//! the ambient light.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use platypus_core::Value;
use platypus_core::glam::{Mat4, Quat, Vec3};
use platypus_render::{
    ALL_LAYERS, Anisotropic, AutoExposure, BEAM_MESH, Bloom, CloudLayer, Color, Denoising,
    FillRule, FillVertex, Fog, FogMode, FogVolume, GizmoLine, Grading, HeightFog, Light, LightKind,
    MAX_CURVE_POINTS, MAX_FOG_VOLUMES, MaterialMaps, MeshData, MeshGlow, MeshInstance, Metering,
    Mirror, PipelineAsset, Projection, ReflectionProbe, RenderMode, ShadowQuality, Shadows,
    ShapeRun, Sky, Stroke, TextureData, TextureFilter, TextureWrap, Tonemapping, VolumetricFog,
    WorldCamera, WorldQuad, WorldTexture, WorldView, primitive_id, viewport_size,
};
use platypus_scene::{ScriptObject, ScriptScene};

use crate::particles::{self, ParticleSystem};

#[derive(Clone, Debug)]
struct Camera {
    object: ScriptObject,
    projection: Projection,
    near: f32,
    far: f32,
    /// Linear RGBA.
    clear: [f32; 4],
    /// Draw order: lower first, so higher ones draw over them (Unity's `depth`).
    depth: f32,
    /// x, y, width, height from the top-left, 0 to 1.
    rect: [f32; 4],
    /// Shows the sky behind everything (Unity's skybox clear flag).
    skybox: bool,
    /// The layers it draws, a bit each (Unity's `cullingMask`).
    culling_mask: u32,
    /// Per layer: things farther than this aren't drawn (0: only the far plane).
    cull_distances: [f32; 32],
    /// Cull distances measured straight from the camera, not along its view.
    cull_spherical: bool,
    /// FXAA over the screen (Unity's camera `antialiasing`).
    fxaa: bool,
    /// Draws into this render texture instead of the screen (`targetTexture`).
    target: Option<u32>,
    /// Draws into this screen UI `<canvas>` node's texture instead (`targetCanvas`).
    canvas: Option<u32>,
    /// Draws the editor's ground grid (`Camera.sceneGrid`).
    grid: bool,
    /// Shows `RenderSettings.fog` (`Camera.fog`).
    fog: bool,
    /// Gizmo lines only this camera shows (`render.gizmos`, from `Gizmos`).
    lines: Vec<GizmoLine>,
}

impl Camera {
    /// The fraction (0 to 1, past 1 when bigger) of the view's height something `size`
    /// metres across takes up `distance` away, as LOD groups measure it.
    fn screen_height(&self, size: f32, distance: f32) -> f32 {
        match self.projection {
            Projection::Perspective { fov_y_degrees } => {
                let half = (fov_y_degrees.clamp(1.0, 179.0).to_radians() / 2.0).tan();
                size / (2.0 * distance.max(1e-4) * half)
            }
            Projection::Orthographic { half_height } => size / (2.0 * half_height.max(1e-4)),
        }
    }

    /// Whether this camera at `position` facing `forward` draws something on `layer` at `at`.
    fn shows(&self, layer: u8, at: Vec3, position: Vec3, forward: Vec3) -> bool {
        if self.culling_mask & (1 << layer) == 0 {
            return false;
        }
        let limit = self.cull_distances[layer as usize];
        if limit <= 0.0 {
            return true;
        }
        let distance = if self.cull_spherical {
            at.distance(position)
        } else {
            (at - position).dot(forward)
        };
        distance <= limit
    }
}

/// Something drawn for an object: its layer and where it is, for cameras to cull.
struct Tagged<T> {
    layer: u8,
    at: Vec3,
    item: T,
    /// The MeshRenderer it came from, for LOD groups.
    renderer: Option<u32>,
}

impl<T> Tagged<T> {
    /// `None` for unknown objects.
    fn new(scene: &ScriptScene, object: ScriptObject, item: T) -> Option<Self> {
        let o = scene.object(object)?;
        Some(Self {
            layer: scene.layer(object),
            at: scene.scene.world_matrix(o).translation.into(),
            item,
            renderer: None,
        })
    }
}

/// A `LODGroup`: renderers per level of detail, the most detailed first, picked by how
/// tall the group looks on screen.
#[derive(Clone, Debug)]
struct LodGroup {
    object: ScriptObject,
    /// Its size in metres before the object's scale (Unity's `size`).
    size: f32,
    /// Per level: the screen height fraction (0 to 1) below which the next one takes
    /// over; below the last, nothing is drawn.
    heights: Vec<f32>,
    /// Per level: its MeshRenderers' component ids.
    renderers: Vec<Vec<u32>>,
    /// A level shown whatever the distance (`ForceLOD`).
    forced: Option<usize>,
}

impl LodGroup {
    /// The level to show at a relative screen height (`None`: culled).
    fn level(&self, height: f32) -> Option<usize> {
        if let Some(f) = self.forced {
            return Some(f.min(self.renderers.len().saturating_sub(1)));
        }
        self.heights.iter().position(|&h| height >= h)
    }
}

#[derive(Clone, Copy, Debug)]
struct Plane {
    object: ScriptObject,
    size: (f32, f32),
    color: [f32; 4],
}

/// A `MeshRenderer` (or skinned one) with its material, as the `meshRenderer` op gives it;
/// a compiled game's sync builds it directly ([`WorldViews::set_renderer`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Renderer {
    pub(crate) object: ScriptObject,
    /// A primitive's id or a script mesh's.
    pub(crate) mesh: u32,
    pub(crate) color: [f32; 4],
    pub(crate) emission: [f32; 3],
    pub(crate) smoothness: f32,
    pub(crate) metallic: f32,
    pub(crate) unlit: bool,
    pub(crate) texture: Option<u32>,
    pub(crate) tiling: [f32; 4],
    pub(crate) cast_shadows: bool,
    pub(crate) extras: MaterialExtras,
}

/// A skinned renderer's bones (objects, in the mesh's joint order) and their inverse
/// bind poses.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Skin {
    pub(crate) bones: Vec<ScriptObject>,
    pub(crate) bindposes: Vec<Mat4>,
}

/// A material's settings past the basic ones.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MaterialExtras {
    pub(crate) maps: MaterialMaps,
    pub(crate) specular: Option<[f32; 3]>,
    pub(crate) mode: RenderMode,
    pub(crate) alpha_cutoff: f32,
    pub(crate) double_sided: bool,
}

/// A table of components (ids are per kind), for [`WorldViews::remove_component`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Camera,
    Light,
    FogVolume,
    /// Mesh renderers and skinned ones (from `SKIN_BASE`), with their skins.
    Renderer,
    Sprite,
    Stroke,
    Plane,
    Mirror,
    Probe,
    Lod,
    Particles,
}

/// A `LineRenderer2D` as the `stroke` op gives it ([`WorldViews::set_stroke`]).
pub(crate) struct StrokeSpec<'a> {
    pub(crate) object: ScriptObject,
    /// Full width in world units.
    pub(crate) width: f32,
    pub(crate) color: [f32; 4],
    pub(crate) closed: bool,
    pub(crate) points: &'a [[f32; 2]],
    pub(crate) fill: Option<[f32; 4]>,
    pub(crate) nonzero: bool,
    pub(crate) order: i32,
}

/// A material's maps and modes from `meshRenderer`'s last argument:
/// `[normal|null normalScale metallicGloss|null gltfPacking occlusion|null
/// occlusionStrength emission|null detail|null [detail sx sy ox oy] [specular r g b]|null
/// mode(opaque|cutout|fade|transparent) alphaCutoff doubleSided]`.
fn material_extras(v: Option<&Value>) -> Result<MaterialExtras, String> {
    let Some(Value::Array(a)) = v else {
        return Ok(MaterialExtras {
            alpha_cutoff: 0.5,
            ..Default::default()
        });
    };
    let texture = |i: usize| -> Option<WorldTexture> {
        a.get(i)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| WorldTexture::Asset(v as u32))
    };
    let number = |i: usize, default: f32| {
        a.get(i)
            .and_then(Value::as_f64)
            .map(|v| v as f32)
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    };
    let maps = MaterialMaps {
        normal: texture(0),
        normal_scale: number(1, 1.0),
        metallic_gloss: texture(2),
        gltf_packing: matches!(a.get(3), Some(Value::Bool(true))),
        occlusion: texture(4),
        occlusion_strength: number(5, 1.0),
        emission: texture(6),
        detail: texture(7),
        detail_tiling: nums::<4>(a.get(8)).unwrap_or([1.0, 1.0, 0.0, 0.0]),
        reflection: None,
    };
    let mode = match a.get(10).and_then(Value::as_str) {
        None | Some("opaque") => RenderMode::Opaque,
        Some("cutout") => RenderMode::Cutout,
        Some("fade") => RenderMode::Fade,
        Some("transparent") => RenderMode::Transparent,
        Some(other) => return Err(format!("unknown rendering mode `{other}`")),
    };
    Ok(MaterialExtras {
        maps,
        specular: nums::<3>(a.get(9)),
        mode,
        alpha_cutoff: number(11, 0.5),
        double_sided: matches!(a.get(12), Some(Value::Bool(true))),
    })
}

/// Rendering quality scripts chose (`QualitySettings`), applied to the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    pub msaa: u32,
    pub render_scale: f32,
    pub shadows: ShadowQuality,
    pub soft_radius: u32,
    /// LOD groups pick levels as if things were this many times taller on screen.
    pub lod_bias: f32,
    /// Frames wait for the display's refresh (`QualitySettings.vSyncCount` > 0; on by default).
    pub vsync: bool,
    pub anisotropic: Anisotropic,
    /// Top mips skipped (`QualitySettings.globalTextureMipmapLimit`).
    pub texture_mip_limit: u32,
    /// The level's pipeline settings (Unity's URP asset per quality level).
    pub pipeline: PipelineAsset,
}

impl Default for Quality {
    fn default() -> Self {
        Self {
            msaa: 1,
            render_scale: 1.0,
            shadows: ShadowQuality::All,
            soft_radius: 1,
            lod_bias: 1.0,
            vsync: true,
            anisotropic: Anisotropic::Enable,
            texture_mip_limit: 0,
            pipeline: PipelineAsset::default(),
        }
    }
}

/// A `ReflectionProbe` component.
#[derive(Clone, Copy, Debug)]
struct Probe {
    object: ScriptObject,
    half_extents: Vec3,
    resolution: u32,
    box_projection: bool,
    every_frame: bool,
    culling_mask: u32,
}

#[derive(Clone, Copy, Debug)]
struct Sprite {
    object: ScriptObject,
    texture: u32,
    /// x, y, width, height in pixels from the texture's bottom-left; `None`: all of it.
    rect: Option<[f32; 4]>,
    /// 0..1 across the rect from its bottom-left: where the object's origin sits.
    pivot: [f32; 2],
    pixels_per_unit: f32,
    color: [f32; 4],
    flip: (bool, bool),
    /// Higher draws over lower (Unity's `sortingOrder`).
    order: i32,
    mode: SpriteMode,
    /// The area a sliced or tiled sprite fills, in world units (the rect's size when `None`).
    size: Option<[f32; 2]>,
    /// Left, bottom, right and top borders in pixels, for sliced sprites.
    border: [f32; 4],
}

/// How a sprite fills its area (Unity's `SpriteDrawMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpriteMode {
    /// One quad at the sprite's own size.
    Simple,
    /// Nine parts: corners keep their size, edges and the middle stretch.
    Sliced,
    /// The sprite repeated across the area, cut off at the top and right.
    Tiled,
}

/// A `LineRenderer2D`: a polyline on its object's local XY plane, drawn as thick strokes.
#[derive(Clone, Debug)]
struct Stroke2d {
    object: ScriptObject,
    /// Points in the object's space, on its XY plane.
    points: Vec<[f32; 2]>,
    /// The fill rule the triangles were made with.
    nonzero: bool,
    /// Full width in world units (not scaled with the object).
    width: f32,
    color: [f32; 4],
    /// Joins the last point back to the first.
    closed: bool,
    /// The fill's colour and its triangles (in the object's space, tessellated when the
    /// points change), if it's filled.
    fill: Option<([f32; 4], Vec<[f32; 2]>)>,
    /// Higher draws over lower; equal ones in the order they were made.
    order: i32,
}

#[derive(Clone, Copy, Debug)]
struct LightComponent {
    object: ScriptObject,
    kind: LightKind,
    /// Colour × intensity.
    color: [f32; 3],
    range: f32,
    /// A glow drawn around the light this many metres across (0: none), Unity's halo.
    halo: f32,
    /// How bright a spot light's visible beam is (0: none).
    beam: f32,
    shadows: Option<Shadows>,
    culling_mask: u32,
    /// How strongly it lights the fog (`Light.volumetricIntensity`).
    volumetric: f32,
}

/// A `FogVolume`: a box of fog on its object, its size before the object's scale.
#[derive(Clone, Copy, Debug)]
struct FogBox {
    object: ScriptObject,
    size: Vec3,
    density: f32,
    color: [f32; 3],
    blend: f32,
    ellipsoid: bool,
}

pub struct WorldViews {
    cameras: BTreeMap<u32, Camera>,
    fog_volumes: BTreeMap<u32, FogBox>,
    planes: BTreeMap<u32, Plane>,
    renderers: BTreeMap<u32, Renderer>,
    lights: BTreeMap<u32, LightComponent>,
    sprites: BTreeMap<u32, Sprite>,
    strokes: BTreeMap<u32, Stroke2d>,
    /// Planar mirrors: their objects, the local normals of their planes and the layers
    /// they reflect.
    mirrors: BTreeMap<u32, (ScriptObject, Vec3, u32)>,
    /// Reflection probes by component.
    probes: BTreeMap<u32, Probe>,
    lod_groups: BTreeMap<u32, LodGroup>,
    /// Probes asked to render on the next frame (`render.probeRender`, or just added).
    probe_requests: std::collections::HashSet<u32>,
    particles: BTreeMap<u32, ParticleSystem>,
    /// Bloom over the world (`render.bloom`), applied by the app before drawing.
    pub bloom: Option<Bloom>,
    /// Shadow distance (metres), map size and the sun's cascades (`render.shadowSettings`).
    pub shadow_settings: (f32, u32, u32),
    /// Anti-aliasing, render scale and shadow quality (`render.quality`).
    pub quality: Quality,
    /// Exposure and tonemapping (`render.post`).
    pub grading: Grading,
    /// The script's `PostProcess` passes, which each camera's view carries.
    pub(crate) post: crate::postfx::PostSet,
    /// Cameras whose auto exposure jumps to what they see next frame (`render.exposureReset`),
    /// applied by the app.
    pub exposure_resets: Vec<u32>,
    /// Lights in real-world units (`platypus.lightUnits: "physical"`, as Unity 7 and HDRP): directional lights' intensity
    /// in lux, spot and point lights' in lumens.
    pub physical_units: bool,
    /// The sky (`render.sky`), applied by the app before drawing.
    pub sky: Option<Sky>,
    /// Reflection / sky light intensity, and whether ambient light comes from the sky.
    pub environment: (f32, bool),
    /// Fog for every camera that shows it (`render.fog`).
    fog: Option<Fog>,
    /// The cloud layer (`render.clouds`), and the wind carrying it: m/s along x and z.
    clouds: Option<(CloudLayer, [f32; 2])>,
    /// `emit` requests waiting for the next particle update (which knows where objects are).
    pending_emits: Vec<(u32, u32)>,
    ambient: [f32; 3],
    /// Script meshes to upload (`None`: remove), applied by the app before drawing.
    pub mesh_uploads: Vec<(u32, Option<MeshData>)>,
    /// Every script and model mesh's data, for mesh colliders (`MeshCollider.sharedMesh`).
    pub mesh_data: HashMap<u32, MeshData>,
    /// Model meshes and textures get engine ids from here.
    model_ids: crate::models::EngineIds,
    /// Models by file (asset key): the path scripts gave and the engine ids they got.
    model_files: HashMap<String, (String, Vec<u32>)>,
    /// Textures loaded from files: their file (asset key) and op arguments, by id.
    texture_files: HashMap<u32, (String, Vec<Value>)>,
    /// Textures to upload (`None`: remove), applied by the app before drawing.
    pub texture_uploads: Vec<(u32, Option<TextureData>)>,
    /// Cooked textures to upload (falling back to decoding them on GPUs without BC).
    pub cooked_uploads: Vec<(u32, platypus_render::CookedTexture)>,
    /// Render textures to make or resize, with their sizes.
    pub render_texture_uploads: Vec<(u32, (u32, u32))>,
    /// Render textures' sizes, for their cameras' aspect.
    render_textures: HashMap<u32, (u32, u32)>,
    /// Skinned renderers' bones, by component, and their matrices kept between frames.
    skins: crate::bone_cache::Skins,
    bone_cache: std::cell::RefCell<crate::bone_cache::BoneCache>,
    /// Loaded textures' sizes in pixels, for sprites.
    texture_sizes: HashMap<u32, (u32, u32)>,
    /// Renderers watched for animation culling, and what the last frame saw of them.
    pub(crate) culling: crate::anim_cull::Watch,
    /// The `<canvas>` nodes cameras draw into, with their texture's size.
    canvas_textures: HashMap<u32, (u32, u32)>,
}

/// Render textures of `<canvas>` elements (`Camera.targetCanvas`) are this plus the UI node id,
/// above scripts' textures and the engine's (`models::ENGINE_IDS`).
const CANVAS_TEXTURES: u32 = 0xE000_0000;

/// The render texture a camera draws into for `<canvas>` node `node`.
pub fn canvas_texture(node: u32) -> u32 {
    CANVAS_TEXTURES.wrapping_add(node)
}

/// What [`WorldViews::sync_canvases`] changed: canvases whose texture was made or resized (node,
/// texture id, size), and those no camera draws into any more.
#[derive(Debug, Default, PartialEq)]
pub struct CanvasChanges {
    pub made: Vec<(u32, u32, (u32, u32))>,
    pub gone: Vec<u32>,
}

impl Default for WorldViews {
    fn default() -> Self {
        Self {
            cameras: BTreeMap::new(),
            fog_volumes: BTreeMap::new(),
            planes: BTreeMap::new(),
            renderers: BTreeMap::new(),
            lights: BTreeMap::new(),
            sprites: BTreeMap::new(),
            strokes: BTreeMap::new(),
            mirrors: BTreeMap::new(),
            probes: BTreeMap::new(),
            lod_groups: BTreeMap::new(),
            probe_requests: std::collections::HashSet::new(),
            particles: BTreeMap::new(),
            bloom: None,
            shadow_settings: (25.0, 2048, 1),
            quality: Quality::default(),
            grading: Grading::default(),
            post: Default::default(),
            exposure_resets: Vec::new(),
            physical_units: false,
            sky: None,
            environment: (1.0, true),
            fog: None,
            clouds: None,
            pending_emits: Vec::new(),
            ambient: [0.2, 0.2, 0.22],
            mesh_uploads: Vec::new(),
            mesh_data: HashMap::new(),
            model_ids: crate::models::EngineIds::default(),
            model_files: HashMap::new(),
            texture_files: HashMap::new(),
            texture_uploads: Vec::new(),
            render_texture_uploads: Vec::new(),
            cooked_uploads: Vec::new(),
            render_textures: HashMap::new(),
            skins: Default::default(),
            bone_cache: Default::default(),
            texture_sizes: HashMap::new(),
            culling: crate::anim_cull::Watch::default(),
            canvas_textures: HashMap::new(),
        }
    }
}

impl WorldViews {
    /// Gives cameras that draw into a `<canvas>` element (`targetCanvas`) a render texture of
    /// the element's box in pixels (`size_of`; `None` while it isn't laid out, and the camera
    /// draws nothing), made again when the size changes and removed when no camera draws there.
    /// Call before the app applies the uploads; it hands the textures to the UI renderer.
    pub fn sync_canvases(&mut self, size_of: impl Fn(u32) -> Option<(u32, u32)>) -> CanvasChanges {
        let wanted: HashMap<u32, (u32, u32)> = self
            .cameras
            .values()
            .filter_map(|c| c.canvas)
            .filter_map(|node| Some((node, size_of(node)?)))
            .collect();
        let mut changes = CanvasChanges::default();
        let gone: Vec<u32> = self
            .canvas_textures
            .keys()
            .filter(|n| !wanted.contains_key(n))
            .copied()
            .collect();
        for node in gone {
            self.canvas_textures.remove(&node);
            let id = canvas_texture(node);
            self.render_textures.remove(&id);
            self.texture_uploads.push((id, None));
            changes.gone.push(node);
        }
        for (node, size) in wanted {
            if self.canvas_textures.get(&node) == Some(&size) {
                continue;
            }
            let id = canvas_texture(node);
            self.canvas_textures.insert(node, size);
            self.render_textures.insert(id, size);
            self.render_texture_uploads.push((id, size));
            changes.made.push((node, id, size));
        }
        changes.made.sort_unstable();
        changes.gone.sort_unstable();
        changes
    }

    /// Makes [`Self::sync_canvases`] report every canvas as new again (their textures weren't
    /// made: no world yet).
    pub fn forget_canvases(&mut self) {
        self.canvas_textures.clear();
    }
}

/// A flat number array (`None` if it isn't one).
fn flat(v: Option<&Value>) -> Option<Vec<f32>> {
    match v {
        Some(Value::Array(items)) => items.iter().map(|x| x.as_f64().map(|n| n as f32)).collect(),
        _ => None,
    }
}

/// A culling mask (through `f64`, so all 32 bits survive); missing or null: every layer.
fn mask(v: Option<&Value>) -> u32 {
    v.and_then(Value::as_f64).map_or(ALL_LAYERS, |m| m as u32)
}

fn nums<const N: usize>(v: Option<&Value>) -> Option<[f32; N]> {
    let Some(Value::Array(items)) = v else {
        return None;
    };
    let mut out = [0.0; N];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = items.get(i)?.as_f64()? as f32;
    }
    Some(out)
}

/// Lays a unit quad (facing +z) flat in an object's XZ plane, facing up (+y) like
/// Unity's Plane, `size` across.
pub fn lay_flat(size: (f32, f32)) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::new(size.0, size.1, 1.0),
        Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        Vec3::ZERO,
    )
}

/// Where a sprite's quad goes in its object's space, and the part of the texture it
/// shows (origin and size in top-down UV space).
fn sprite_quad(s: &Sprite, (tw, th): (u32, u32)) -> (Mat4, [f32; 4]) {
    let (tw, th) = (tw as f32, th as f32);
    let [x, y, w, h] = s.rect.unwrap_or([0.0, 0.0, tw, th]);
    let ppu = s.pixels_per_unit.max(1e-6);
    let size = Vec3::new(w / ppu, h / ppu, 1.0);
    let offset = Vec3::new(
        (0.5 - s.pivot[0]) * size.x,
        (0.5 - s.pivot[1]) * size.y,
        0.0,
    );
    let mut uv = [x / tw, 1.0 - (y + h) / th, w / tw, h / th];
    if s.flip.0 {
        uv[0] += uv[2];
        uv[2] = -uv[2];
    }
    if s.flip.1 {
        uv[1] += uv[3];
        uv[3] = -uv[3];
    }
    (
        Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, offset),
        uv,
    )
}

/// Most quads one tiled sprite may make (a huge area of a tiny tile stops here).
const MAX_SPRITE_TILES: usize = 4096;

/// The quads a sprite draws (their places in its object's space and UV rects): one for
/// simple sprites, nine for sliced ones, a grid for tiled ones.
fn sprite_quads(s: &Sprite, tex: (u32, u32)) -> Vec<(Mat4, [f32; 4])> {
    if s.mode == SpriteMode::Simple {
        return vec![sprite_quad(s, tex)];
    }
    let (tw, th) = (tex.0 as f32, tex.1 as f32);
    let [x, y, w, h] = s.rect.unwrap_or([0.0, 0.0, tw, th]);
    let ppu = s.pixels_per_unit.max(1e-6);
    let [width, height] = s.size.unwrap_or([w / ppu, h / ppu]);
    // The area's bottom-left, so the pivot lands on the object's origin.
    let (left, bottom) = (-s.pivot[0] * width, -s.pivot[1] * height);
    // `px` x `py`: pixels of the rect (from its bottom-left) shown in the area's `xs` x `ys` units.
    let cell = |px: [f32; 2], py: [f32; 2], xs: [f32; 2], ys: [f32; 2]| {
        let mut uv = [
            (x + px[0]) / tw,
            1.0 - (y + py[1]) / th,
            (px[1] - px[0]) / tw,
            (py[1] - py[0]) / th,
        ];
        if s.flip.0 {
            uv[0] += uv[2];
            uv[2] = -uv[2];
        }
        if s.flip.1 {
            uv[1] += uv[3];
            uv[3] = -uv[3];
        }
        let size = Vec3::new(xs[1] - xs[0], ys[1] - ys[0], 1.0);
        let centre = Vec3::new(
            left + (xs[0] + xs[1]) / 2.0,
            bottom + (ys[0] + ys[1]) / 2.0,
            0.0,
        );
        (
            Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, centre),
            uv,
        )
    };
    let mut quads = Vec::new();
    if s.mode == SpriteMode::Tiled {
        let (cw, ch) = (w / ppu, h / ppu);
        if cw <= 0.0 || ch <= 0.0 {
            return quads;
        }
        let (nx, ny) = ((width / cw).ceil() as usize, (height / ch).ceil() as usize);
        for iy in 0..ny {
            for ix in 0..nx {
                if quads.len() >= MAX_SPRITE_TILES {
                    return quads;
                }
                let (x0, y0) = (ix as f32 * cw, iy as f32 * ch);
                let (x1, y1) = ((x0 + cw).min(width), (y0 + ch).min(height));
                quads.push(cell(
                    [0.0, (x1 - x0) * ppu],
                    [0.0, (y1 - y0) * ppu],
                    [x0, x1],
                    [y0, y1],
                ));
            }
        }
        return quads;
    }
    // Sliced: the borders shrink together when the area is smaller than they are.
    let [bl, bb, br, bt] = s.border;
    let fit = |a: f32, b: f32, total: f32| {
        let need = (a + b) / ppu;
        if need > total && need > 0.0 {
            total / need
        } else {
            1.0
        }
    };
    let (kx, ky) = (fit(bl, br, width), fit(bb, bt, height));
    let xs = [0.0, bl * kx / ppu, width - br * kx / ppu, width];
    let ys = [0.0, bb * ky / ppu, height - bt * ky / ppu, height];
    let pxs = [0.0, bl, w - br, w];
    let pys = [0.0, bb, h - bt, h];
    for j in 0..3 {
        for i in 0..3 {
            if xs[i + 1] - xs[i] <= 0.0 || ys[j + 1] - ys[j] <= 0.0 {
                continue;
            }
            quads.push(cell(
                [pxs[i], pxs[i + 1]],
                [pys[j], pys[j + 1]],
                [xs[i], xs[i + 1]],
                [ys[j], ys[j + 1]],
            ));
        }
    }
    quads
}

impl WorldViews {
    /// Carries the cloud layer on the wind for `dt` seconds.
    pub fn advance_clouds(&mut self, dt: f32) {
        if let Some((c, wind)) = self.clouds.as_mut() {
            // Wrapped well before floats lose precision: far past any cloud's size.
            c.offset = [0, 1].map(|i| (c.offset[i] + wind[i] * dt) % 1.0e5);
        }
    }

    /// `model id src`: loads a glTF model, queuing its meshes and textures for upload.
    /// Returns its description for scripts.
    pub fn model_op(&mut self, root: &Path, args: &[Value]) -> Result<serde_json::Value, String> {
        let src = args
            .get(1)
            .and_then(Value::as_str)
            .ok_or("a model needs a path")?;
        self.load_model(root, src, &[])
    }

    /// Loads a model, giving it `reuse`'s ids first, and remembers its file and ids.
    fn load_model(
        &mut self,
        root: &Path,
        src: &str,
        reuse: &[u32],
    ) -> Result<serde_json::Value, String> {
        self.model_ids.record(reuse);
        // A cooked model (`platypus-app --cook`) when there's a fresh one.
        let model = match crate::cook::cooked_model(root, src, &mut self.model_ids) {
            Some(m) => Ok(m),
            None => crate::models::load(root, src, &mut self.model_ids),
        };
        let ids = self.model_ids.take_recorded();
        let model = model?;
        self.model_files
            .insert(crate::reload::asset_key(src), (src.to_owned(), ids));
        for (id, data) in model.meshes {
            self.mesh_data.insert(id, data.clone());
            self.mesh_uploads.push((id, Some(data)));
        }
        for (id, data) in model.textures {
            self.texture_sizes.insert(id, (data.width, data.height));
            self.texture_uploads.push((id, Some(data)));
        }
        for (id, data) in model.cooked_textures {
            self.texture_sizes.insert(id, (data.width, data.height));
            self.cooked_uploads.push((id, data));
        }
        Ok(model.description)
    }

    /// Loads again what came from a changed file (textures, models). Returns textures'
    /// new sizes and models' new descriptions for scripts. A model whose parts changed
    /// in number keeps its old ids for the first ones; placed copies keep their old
    /// materials and nodes (place it again to see those).
    #[allow(clippy::type_complexity)]
    pub fn reload_asset(
        &mut self,
        root: &Path,
        key: &str,
    ) -> (Vec<(u32, u32, u32)>, Vec<(String, serde_json::Value)>) {
        let mut sizes = Vec::new();
        let textures: Vec<(u32, Vec<Value>)> = self
            .texture_files
            .iter()
            .filter(|(_, (k, _))| k == key)
            .map(|(id, (_, args))| (*id, args.clone()))
            .collect();
        for (id, args) in textures {
            match self.texture_op("texture", root, &args) {
                Ok(Some(size)) => sizes.push(size),
                Ok(None) => {}
                Err(e) => tracing::warn!("reloading texture {id}: {e}"),
            }
        }
        let mut models = Vec::new();
        if let Some((src, ids)) = self.model_files.get(key).cloned() {
            match self.load_model(root, &src, &ids) {
                Ok(description) => models.push((src, description)),
                Err(e) => tracing::warn!("reloading model `{src}`: {e}"),
            }
        }
        (sizes, models)
    }

    /// A mesh's data by name (a primitive) or id (a script's or a model's).
    pub fn mesh_source(&self, mesh: Option<&Value>) -> Option<MeshData> {
        match mesh? {
            Value::String(name) => platypus_render::primitive(name),
            v => self.mesh_data.get(&(v.as_f64()? as u32)).cloned(),
        }
    }

    /// A cloth's own copy of its mesh (`target`), which it then moves.
    pub fn set_cloth_mesh(&mut self, target: u32, data: MeshData) {
        self.mesh_data.insert(target, data.clone());
        self.mesh_uploads.push((target, Some(data)));
    }

    /// New vertex positions for a cloth's mesh: normals and tangents follow.
    pub fn move_cloth_mesh(&mut self, target: u32, positions: Vec<[f32; 3]>) {
        let Some(data) = self.mesh_data.get_mut(&target) else {
            return;
        };
        if data.positions.len() != positions.len() {
            return;
        }
        data.positions = positions;
        data.normals.clear();
        data.tangents.clear();
        let moved = std::mem::take(data).complete();
        *data = moved.clone();
        // Only the latest shape needs uploading.
        self.mesh_uploads.retain(|(id, _)| *id != target);
        self.mesh_uploads.push((target, Some(moved)));
    }

    /// A texture that failed to load: sprites using it draw the renderer's missing
    /// checker (an 8 × 8 texel image) in its place.
    /// Whether the game asked for light shafts (`RenderSettings.volumetricFog`).
    /// The scene-wide fog (tests).
    #[cfg(test)]
    pub fn fog_for_test(&self) -> Option<Fog> {
        self.fog
    }

    pub fn uses_volumetric_fog(&self) -> bool {
        self.fog.is_some_and(|f| f.volumetric.is_some())
    }

    /// Empty, with lights in real-world units (`platypus.lightUnits: "physical"`) or not.
    pub fn with_physical_units(physical_units: bool) -> Self {
        Self {
            physical_units,
            ..Default::default()
        }
    }

    /// The ids of every camera, for reading their exposure back.
    /// Whether anything needs the 3D world renderer: a camera, or an upload waiting for it.
    pub fn needs_world(&self) -> bool {
        !self.cameras.is_empty()
            || !self.mesh_uploads.is_empty()
            || !self.texture_uploads.is_empty()
            || !self.cooked_uploads.is_empty()
            || !self.render_texture_uploads.is_empty()
    }

    pub fn camera_ids(&self) -> Vec<u32> {
        self.cameras.keys().copied().collect()
    }

    pub fn mark_missing(&mut self, id: u32) {
        self.texture_sizes.insert(id, (8, 8));
    }

    /// `texture id src filter wrap mipmaps [width height [r g b a ...]]`: loads image
    /// file `src` (relative to the game folder) or, with `src` null, takes the pixels
    /// given (sRGB bytes, top row first). `textureRemove id` drops one. Returns the
    /// loaded texture's id and size.
    pub fn texture_op(
        &mut self,
        name: &str,
        root: &Path,
        args: &[Value],
    ) -> Result<Option<(u32, u32, u32)>, String> {
        let id = args
            .first()
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0)
            .ok_or("a texture needs an id")? as u32;
        if name == "textureRemove" {
            self.texture_files.remove(&id);
            self.texture_sizes.remove(&id);
            self.render_textures.remove(&id);
            self.texture_uploads.push((id, None));
            return Ok(None);
        }
        // Textures from files reload when the file changes.
        if let Some(Value::String(src)) = args.get(1)
            && !src.trim_start().starts_with("data:")
        {
            self.texture_files
                .insert(id, (crate::reload::asset_key(src), args.to_vec()));
        }
        let filter = match args.get(2).and_then(Value::as_str) {
            Some(f) => TextureFilter::parse(f).ok_or_else(|| format!("unknown filter `{f}`"))?,
            None => TextureFilter::default(),
        };
        let wrap = match args.get(3).and_then(Value::as_str) {
            Some(w) => TextureWrap::parse(w).ok_or_else(|| format!("unknown wrap `{w}`"))?,
            None => TextureWrap::default(),
        };
        let mipmaps = !matches!(args.get(4), Some(Value::Bool(false)));
        let srgb = !matches!(args.get(9), Some(Value::Bool(true)));
        // A cooked texture (`platypus-app --cook`) when there's a fresh one made the same way.
        if let Some(Value::String(src)) = args.get(1)
            && !src.trim_start().starts_with("data:")
            && let Some(mut cooked) = crate::cook::cooked_texture(root, src, srgb, mipmaps)
        {
            // Sampling settings are the script's.
            cooked.filter = filter;
            cooked.wrap = wrap;
            if let Some(a) = args.get(8).and_then(Value::as_f64) {
                cooked.anisotropy = a.clamp(0.0, 16.0) as u16;
            }
            let size = (cooked.width, cooked.height);
            self.texture_sizes.insert(id, size);
            self.cooked_uploads.push((id, cooked));
            return Ok(Some((id, size.0, size.1)));
        }
        let (width, height, rgba) = match args.get(1) {
            Some(Value::String(src)) => {
                let bytes = if src.trim_start().starts_with("data:") {
                    platypus_core::decode_data_url(src).ok_or("bad data URL")?
                } else {
                    platypus_core::read_asset(root, src).map_err(|e| e.to_string())?
                };
                let mut image = image::load_from_memory(&bytes)
                    .map_err(|e| format!("`{src}`: {e}"))?
                    .to_rgba8();
                let (w, h) = image.dimensions();
                // Colour for transparent pixels, so filtering doesn't leave a pale halo.
                platypus_core::bleed_transparent(&mut image, w as usize, h as usize, 8);
                (w, h, image.into_raw())
            }
            _ => {
                let w = args.get(5).and_then(Value::as_f64).unwrap_or(0.0) as u32;
                let h = args.get(6).and_then(Value::as_f64).unwrap_or(0.0) as u32;
                let px = flat(args.get(7)).ok_or("a texture needs a file or pixels")?;
                (
                    w,
                    h,
                    px.into_iter().map(|v| v.clamp(0.0, 255.0) as u8).collect(),
                )
            }
        };
        let data = TextureData {
            width,
            height,
            rgba,
            filter,
            wrap,
            mipmaps,
            anisotropy: args
                .get(8)
                .and_then(Value::as_f64)
                .map_or(1, |a| a.clamp(0.0, 16.0) as u16),
            srgb,
        };
        if !data.is_valid() {
            return Err(format!(
                "texture {id}: {width}x{height} doesn't match its pixels"
            ));
        }
        self.texture_sizes.insert(id, (width, height));
        self.texture_uploads.push((id, Some(data)));
        Ok(Some((id, width, height)))
    }

    /// Drops one component a compiled game no longer has (`render_sync`).
    pub(crate) fn remove_component(&mut self, kind: Kind, id: u32) {
        match kind {
            Kind::Camera => {
                self.cameras.remove(&id);
            }
            Kind::Light => {
                self.lights.remove(&id);
            }
            Kind::FogVolume => {
                self.fog_volumes.remove(&id);
            }
            Kind::Renderer => {
                self.renderers.remove(&id);
                self.skins.remove(&id);
            }
            Kind::Sprite => {
                self.sprites.remove(&id);
            }
            Kind::Stroke => {
                self.strokes.remove(&id);
            }
            Kind::Plane => {
                self.planes.remove(&id);
            }
            Kind::Mirror => {
                self.mirrors.remove(&id);
            }
            Kind::Probe => {
                self.probes.remove(&id);
            }
            Kind::Lod => {
                self.lod_groups.remove(&id);
            }
            Kind::Particles => {
                self.particles.remove(&id);
            }
        }
    }

    /// Drops every component but the particle systems (whose particles live here): a full
    /// rebuild of a compiled game's components (`render_sync`'s check mode).
    pub(crate) fn clear_components(&mut self) {
        self.cameras.clear();
        self.lights.clear();
        self.fog_volumes.clear();
        self.renderers.clear();
        self.skins.clear();
        self.sprites.clear();
        self.strokes.clear();
        self.planes.clear();
        self.mirrors.clear();
        self.probes.clear();
        self.lod_groups.clear();
    }

    /// A mesh renderer (`meshRenderer`), with its bones if it is skinned.
    pub(crate) fn set_renderer(&mut self, id: u32, renderer: Renderer, skin: Option<Skin>) {
        match skin {
            Some(s) => self.skins.insert(id, s),
            None => self.skins.remove(&id),
        };
        self.renderers.insert(id, renderer);
    }

    /// A 2D line or shape (`stroke`): unchanged points keep their fill's triangles.
    pub(crate) fn set_stroke(&mut self, id: u32, s: StrokeSpec) {
        let rule = if s.nonzero {
            FillRule::NonZero
        } else {
            FillRule::EvenOdd
        };
        let fill = s.fill.map(|color| {
            match self.strokes.get(&id) {
                Some(old) if old.points == s.points && old.nonzero == s.nonzero => {
                    old.fill.as_ref().map(|(_, tris)| tris.clone())
                }
                _ => None,
            }
            .map_or_else(
                || (color, platypus_render::tessellate_fill(s.points, rule)),
                |tris| (color, tris),
            )
        });
        let stroke = Stroke2d {
            object: s.object,
            points: s.points.to_vec(),
            nonzero: s.nonzero,
            fill,
            order: s.order,
            width: s.width.max(0.0),
            color: s.color,
            closed: s.closed,
        };
        self.strokes.insert(id, stroke);
    }

    /// The components as text, in id order: what a full rebuild must match (`render_sync`'s
    /// equivalence test). Particle systems by their settings (their particles are a simulation).
    /// The mirrors' local normals by component id, for tests.
    #[cfg(test)]
    pub(crate) fn mirror_normals(&self) -> Vec<(u32, Vec3)> {
        self.mirrors.iter().map(|(id, m)| (*id, m.1)).collect()
    }

    #[cfg(test)]
    pub(crate) fn components_dump(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let cameras: BTreeMap<_, _> = self
            .cameras
            .iter()
            .map(|(id, c)| {
                (
                    id,
                    Camera {
                        lines: Vec::new(),
                        ..c.clone()
                    },
                )
            })
            .collect();
        let _ = writeln!(s, "cameras {cameras:?}");
        let _ = writeln!(s, "lights {:?}", self.lights);
        let _ = writeln!(s, "fog volumes {:?}", self.fog_volumes);
        let _ = writeln!(s, "renderers {:?}", self.renderers);
        let skins: BTreeMap<_, _> = self.skins.iter().collect();
        let _ = writeln!(s, "skins {skins:?}");
        let _ = writeln!(s, "sprites {:?}", self.sprites);
        let _ = writeln!(s, "strokes {:?}", self.strokes);
        let _ = writeln!(s, "planes {:?}", self.planes);
        let _ = writeln!(s, "mirrors {:?}", self.mirrors);
        let _ = writeln!(s, "probes {:?}", self.probes);
        let _ = writeln!(s, "lods {:?}", self.lod_groups);
        for (id, p) in &self.particles {
            let _ = writeln!(s, "particles {id} {} {:?}", p.object, p.settings);
        }
        s
    }

    /// Runs one `render.*` op.
    pub fn apply(&mut self, name: &str, args: &[Value]) -> Result<(), String> {
        let num = |i: usize| -> Result<f32, String> {
            args.get(i)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("argument {i} must be a number"))
        };
        // Ids through `f64`: `f32` can't hold integers past 2^24.
        let id_at = |i: usize| -> Result<u32, String> {
            args.get(i)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(|v| v as u32)
                .ok_or_else(|| format!("argument {i} must be an id"))
        };
        let id = id_at(0)?;
        match name {
            // `camera component object fov near far [r g b a] depth orthographic
            // orthographicSize [x y w h] clearFlags(skybox|color) cullingMask
            // [32 cull distances]|null cullSpherical antialiasing targetTexture|null grid
            // fog targetCanvas(UI node)|null`
            "camera" => {
                let orthographic = matches!(args.get(7), Some(Value::Bool(true)));
                let projection = if orthographic {
                    Projection::Orthographic {
                        half_height: num(8).unwrap_or(5.0),
                    }
                } else {
                    Projection::Perspective {
                        fov_y_degrees: num(2)?,
                    }
                };
                let camera = Camera {
                    object: id_at(1)?,
                    projection,
                    near: num(3)?,
                    far: num(4)?,
                    clear: nums::<4>(args.get(5)).unwrap_or([0.05, 0.06, 0.09, 1.0]),
                    depth: num(6).unwrap_or(0.0),
                    rect: nums::<4>(args.get(9)).unwrap_or([0.0, 0.0, 1.0, 1.0]),
                    skybox: args.get(10).and_then(Value::as_str) == Some("skybox"),
                    culling_mask: mask(args.get(11)),
                    cull_distances: nums::<32>(args.get(12)).unwrap_or([0.0; 32]),
                    cull_spherical: matches!(args.get(13), Some(Value::Bool(true))),
                    fxaa: args.get(14).and_then(Value::as_str) == Some("fxaa"),
                    target: match args.get(15) {
                        Some(Value::Null) | None => None,
                        _ => Some(id_at(15)?),
                    },
                    grid: matches!(args.get(16), Some(Value::Bool(true))),
                    fog: !matches!(args.get(17), Some(Value::Bool(false))),
                    canvas: match args.get(18) {
                        Some(Value::Null) | None => None,
                        _ => Some(id_at(18)?),
                    },
                    // Kept from before: gizmos come separately.
                    lines: self
                        .cameras
                        .remove(&id)
                        .map(|c| c.lines)
                        .unwrap_or_default(),
                };
                self.cameras.insert(id, camera);
            }
            // `gizmos camera [ax ay az bx by bz r g b a ...]`: the lines this camera
            // shows until the next call.
            "gizmos" => {
                let flat = flat(args.get(1)).unwrap_or_default();
                let lines = flat
                    .as_chunks::<10>()
                    .0
                    .iter()
                    .map(|l| GizmoLine {
                        a: [l[0], l[1], l[2]],
                        b: [l[3], l[4], l[5]],
                        color: [l[6], l[7], l[8], l[9]],
                    })
                    .filter(|l| l.a.iter().chain(&l.b).all(|v| v.is_finite()))
                    .collect();
                self.cameras
                    .get_mut(&id)
                    .ok_or("gizmos: no such camera")?
                    .lines = lines;
            }
            // `plane component object [w h] [r g b a]`
            "plane" => {
                let [w, h] = nums::<2>(args.get(2)).ok_or("plane needs a size")?;
                let plane = Plane {
                    object: id_at(1)?,
                    size: (w, h),
                    color: nums::<4>(args.get(3)).unwrap_or([1.0; 4]),
                };
                self.planes.insert(id, plane);
            }
            // `mesh id [positions xyz...] [normals] [uvs] [indices] [joints 4 per vertex]
            // [weights 4 per vertex]`; `mesh id null` removes it.
            "mesh" => {
                if matches!(args.get(1), Some(Value::Null) | None) {
                    self.mesh_uploads.push((id, None));
                    self.mesh_data.remove(&id);
                    return Ok(());
                }
                let positions = flat(args.get(1)).ok_or("mesh needs positions")?;
                let normals = flat(args.get(2)).unwrap_or_default();
                let uvs = flat(args.get(3)).unwrap_or_default();
                let indices = flat(args.get(4)).ok_or("mesh needs indices")?;
                let data = MeshData {
                    positions: positions.as_chunks::<3>().0.to_vec(),
                    normals: normals.as_chunks::<3>().0.to_vec(),
                    uvs: uvs.as_chunks::<2>().0.to_vec(),
                    tangents: Vec::new(),
                    joints: flat(args.get(5))
                        .unwrap_or_default()
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|j| j.map(|i| i.clamp(0.0, 65535.0) as u16))
                        .collect(),
                    weights: flat(args.get(6))
                        .unwrap_or_default()
                        .as_chunks::<4>()
                        .0
                        .to_vec(),
                    indices: indices.into_iter().map(|i| i as u32).collect(),
                }
                .complete();
                if !data.is_valid() {
                    return Err(format!(
                        "mesh {id}: indices out of range or not whole triangles"
                    ));
                }
                self.mesh_data.insert(id, data.clone());
                self.mesh_uploads.push((id, Some(data)));
            }
            // `meshRenderer component object mesh(name | id) [r g b a] [er eg eb] smoothness
            // unlit texture|null [scale x y, offset x y] castShadows metallic [maps and
            // modes] (see `material_extras`) [bone objects]|null [bindposes, 16 each]`
            "meshRenderer" => {
                let extras = material_extras(args.get(11))?;
                let skin = match args.get(12) {
                    Some(Value::Array(bones)) => {
                        let bones: Vec<ScriptObject> = bones
                            .iter()
                            .map(|b| b.as_f64().map_or(u32::MAX, |b| b as u32))
                            .collect();
                        let flat = flat(args.get(13)).unwrap_or_default();
                        let bindposes: Vec<Mat4> = flat
                            .as_chunks::<16>()
                            .0
                            .iter()
                            .map(Mat4::from_cols_array)
                            .collect();
                        if bindposes.len() != bones.len() {
                            return Err(format!(
                                "{} bones but {} bind poses",
                                bones.len(),
                                bindposes.len()
                            ));
                        }
                        Some(Skin { bones, bindposes })
                    }
                    _ => None,
                };
                let mesh = match args.get(2) {
                    Some(Value::String(name)) => {
                        primitive_id(name).ok_or_else(|| format!("no primitive `{name}`"))?
                    }
                    _ => id_at(2)?,
                };
                let renderer = Renderer {
                    object: id_at(1)?,
                    mesh,
                    color: nums::<4>(args.get(3)).unwrap_or([1.0; 4]),
                    emission: nums::<3>(args.get(4)).unwrap_or([0.0; 3]),
                    smoothness: num(5).unwrap_or(0.5),
                    metallic: num(10).unwrap_or(0.0).clamp(0.0, 1.0),
                    unlit: matches!(args.get(6), Some(Value::Bool(true))),
                    texture: match args.get(7) {
                        Some(Value::Null) | None => None,
                        _ => Some(id_at(7)?),
                    },
                    tiling: nums::<4>(args.get(8)).unwrap_or([1.0, 1.0, 0.0, 0.0]),
                    cast_shadows: !matches!(args.get(9), Some(Value::Bool(false))),
                    extras,
                };
                self.set_renderer(id, renderer, skin);
            }
            // `sprite component object texture [x y w h]|null [pivot x y] pixelsPerUnit
            // [r g b a] flipX flipY sortingOrder`
            "sprite" => {
                let sprite = Sprite {
                    object: id_at(1)?,
                    texture: id_at(2)?,
                    rect: nums::<4>(args.get(3)),
                    pivot: nums::<2>(args.get(4)).unwrap_or([0.5, 0.5]),
                    pixels_per_unit: num(5).unwrap_or(100.0),
                    color: nums::<4>(args.get(6)).unwrap_or([1.0; 4]),
                    flip: (
                        matches!(args.get(7), Some(Value::Bool(true))),
                        matches!(args.get(8), Some(Value::Bool(true))),
                    ),
                    order: num(9).unwrap_or(0.0) as i32,
                    mode: match args.get(10).and_then(Value::as_str) {
                        Some("sliced") => SpriteMode::Sliced,
                        Some("tiled") => SpriteMode::Tiled,
                        _ => SpriteMode::Simple,
                    },
                    size: nums::<2>(args.get(11)),
                    border: nums::<4>(args.get(12)).unwrap_or([0.0; 4]),
                };
                self.sprites.insert(id, sprite);
            }
            // `stroke component object width [r g b a] closed [x0 y0 x1 y1 ...]`
            // `... [fill r g b a]|null evenodd|nonzero sortingOrder`: the points as a closed,
            // filled shape; higher orders draw over lower.
            "stroke" => {
                let flat = flat(args.get(5)).ok_or("stroke needs [x, y, ...]")?;
                self.set_stroke(
                    id,
                    StrokeSpec {
                        object: id_at(1)?,
                        width: num(2)?,
                        color: nums::<4>(args.get(3)).unwrap_or([1.0; 4]),
                        closed: matches!(args.get(4), Some(Value::Bool(true))),
                        points: flat.as_chunks::<2>().0,
                        fill: nums::<4>(args.get(6)),
                        nonzero: args.get(7).and_then(Value::as_str) == Some("nonzero"),
                        order: num(8).unwrap_or(0.0) as i32,
                    },
                );
            }
            // `light component object type [r g b] intensity range spotAngle innerSpotAngle
            // haloSize beamIntensity shadows(none|hard|soft) shadowStrength cullingMask
            // volumetricIntensity spotShape(cone|pyramid) aspectRatio`
            "light" => {
                let kind = match args.get(2).and_then(Value::as_str) {
                    Some("directional") => LightKind::Directional,
                    Some("point") => LightKind::Point,
                    Some("spot") if args.get(14).and_then(Value::as_str) == Some("pyramid") => {
                        LightKind::Pyramid {
                            outer_degrees: num(6).unwrap_or(30.0),
                            inner_degrees: num(7).unwrap_or(21.8),
                            aspect: num(15).unwrap_or(1.0).max(1e-3),
                            right: Vec3::X,
                        }
                    }
                    Some("spot") => LightKind::Spot {
                        outer_degrees: num(6).unwrap_or(30.0),
                        inner_degrees: num(7).unwrap_or(21.8),
                    },
                    other => return Err(format!("unknown light type {other:?}")),
                };
                let [r, g, b] = nums::<3>(args.get(3)).unwrap_or([1.0; 3]);
                let mut intensity = num(4).unwrap_or(1.0);
                if self.physical_units {
                    // Into the renderer's units (Unity Built-in's: diffuse light isn't divided
                    // by pi): lux / pi for the sun; lumens to candela (/ 4 pi, all round, as
                    // HDRP's spots without "reflector"), then / pi, for lamps.
                    use std::f32::consts::PI;
                    intensity /= match kind {
                        LightKind::Directional => PI,
                        _ => 4.0 * PI * PI,
                    };
                }
                let light = LightComponent {
                    object: id_at(1)?,
                    kind,
                    color: [r * intensity, g * intensity, b * intensity],
                    range: num(5).unwrap_or(10.0),
                    halo: num(8).unwrap_or(0.0).max(0.0),
                    beam: num(9).unwrap_or(0.0).max(0.0),
                    shadows: match args.get(10).and_then(Value::as_str) {
                        Some(mode @ ("hard" | "soft")) => Some(Shadows {
                            soft: mode == "soft",
                            strength: num(11).unwrap_or(1.0).clamp(0.0, 1.0),
                        }),
                        _ => None,
                    },
                    culling_mask: mask(args.get(12)),
                    volumetric: num(13).unwrap_or(1.0).max(0.0),
                };
                self.lights.insert(id, light);
            }
            // `particles component object [key, value, ...] playOnAwake`: new settings keep
            // a system's particles.
            "particles" => {
                let Some(Value::Array(pairs)) = args.get(2) else {
                    return Err("particles needs [key, value, ...]".into());
                };
                let settings = particles::Settings::parse(pairs)?;
                match self.particles.get_mut(&id) {
                    Some(p) => p.set_settings(settings),
                    None => {
                        let mut p = ParticleSystem::new(id_at(1)?, settings);
                        if matches!(args.get(3), Some(Value::Bool(true))) {
                            p.play();
                        }
                        self.particles.insert(id, p);
                    }
                }
            }
            // `particleControl component play|stop|stopClear|pause|clear|emit [count]`
            "particleControl" => {
                let Some(p) = self.particles.get_mut(&id) else {
                    return Err(format!("no particle system {id}"));
                };
                match args.get(1).and_then(Value::as_str) {
                    Some("play") => p.play(),
                    Some("stop") => p.stop(),
                    Some("stopClear") => {
                        p.stop();
                        p.clear();
                    }
                    Some("pause") => p.pause(),
                    Some("clear") => p.clear(),
                    Some("emit") => {
                        // Emitted where the object is when the frame's particles update.
                        self.pending_emits
                            .push((id, num(2).unwrap_or(1.0).max(0.0) as u32));
                    }
                    other => return Err(format!("unknown particle action {other:?}")),
                }
            }
            // `sky 0 [zenith rgb] [horizon rgb] [ground rgb] [sun dir xyz]|null [sun rgb]
            // sunSize intensity ambientFromSky`; `sky 0 null` removes it.
            "sky" => {
                if matches!(args.get(1), Some(Value::Null) | None) {
                    self.sky = None;
                    return Ok(());
                }
                let d = Sky::default();
                self.sky = Some(Sky {
                    zenith: nums::<3>(args.get(1)).unwrap_or(d.zenith),
                    horizon: nums::<3>(args.get(2)).unwrap_or(d.horizon),
                    ground: nums::<3>(args.get(3)).unwrap_or(d.ground),
                    sun_direction: nums::<3>(args.get(4)).map(Vec3::from_array),
                    sun_color: nums::<3>(args.get(5)).unwrap_or(d.sun_color),
                    sun_size: num(6).unwrap_or(d.sun_size),
                });
                self.environment = (
                    num(7).unwrap_or(1.0).max(0.0),
                    !matches!(args.get(8), Some(Value::Bool(false))),
                );
            }
            // `probe component object [sx sy sz] resolution boxProjection everyFrame
            // cullingMask`
            "probe" => {
                let [x, y, z] = nums::<3>(args.get(2)).unwrap_or([10.0; 3]);
                let probe = Probe {
                    object: id_at(1)?,
                    half_extents: Vec3::new(x, y, z).abs() / 2.0,
                    resolution: num(3).unwrap_or(128.0).clamp(16.0, 1024.0) as u32,
                    box_projection: matches!(args.get(4), Some(Value::Bool(true))),
                    every_frame: matches!(args.get(5), Some(Value::Bool(true))),
                    culling_mask: mask(args.get(6)),
                };
                self.probes.insert(id, probe);
                self.probe_requests.insert(id);
            }
            // `probeRender component`: render it again on the next frame.
            "probeRender" => {
                self.probe_requests.insert(id);
            }
            // `mirror component object [nx ny nz] cullingMask`: the object's MeshRenderer
            // reflects the scene in the plane through its origin with this (local) normal.
            "mirror" => {
                let normal = nums::<3>(args.get(2)).map_or(Vec3::Y, Vec3::from_array);
                self.mirrors
                    .insert(id, (id_at(1)?, normal, mask(args.get(3))));
            }
            // `lodGroup component object size [heights] [[renderer ids] per level] forced|-1`
            "lodGroup" => {
                let Some(Value::Array(levels)) = args.get(4) else {
                    return Err("lodGroup needs [[renderers], ...]".into());
                };
                let renderers = levels
                    .iter()
                    .map(|level| match level {
                        Value::Array(ids) => ids
                            .iter()
                            .map(|v| v.as_f64().map(|v| v as u32).ok_or("renderer ids"))
                            .collect::<Result<Vec<u32>, _>>(),
                        _ => Err("renderer ids"),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let heights = flat(args.get(3)).unwrap_or_default();
                if heights.len() != renderers.len() {
                    return Err("lodGroup needs a height per level".into());
                }
                let forced = num(5).ok().filter(|f| *f >= 0.0).map(|f| f as usize);
                self.lod_groups.insert(
                    id,
                    LodGroup {
                        object: id_at(1)?,
                        size: num(2).unwrap_or(1.0).max(1e-4),
                        heights,
                        renderers,
                        forced,
                    },
                );
            }
            // `quality 0 antiAliasing renderScale shadows(disable|hardOnly|all) softRadius
            // lodBias vSyncCount anisotropic(disable|enable|forceEnable) textureMipLimit`
            "quality" => {
                self.quality = Quality {
                    msaa: num(1).unwrap_or(1.0).clamp(1.0, 8.0) as u32,
                    render_scale: num(2).unwrap_or(1.0).clamp(0.25, 2.0),
                    shadows: match args.get(3).and_then(Value::as_str) {
                        Some("disable") => ShadowQuality::Disable,
                        Some("hardOnly") => ShadowQuality::HardOnly,
                        _ => ShadowQuality::All,
                    },
                    soft_radius: num(4).unwrap_or(1.0).clamp(1.0, 3.0) as u32,
                    lod_bias: num(5).unwrap_or(1.0).max(0.01),
                    // Left out: the default (on).
                    vsync: num(6).ok().is_none_or(|v| v >= 1.0),
                    anisotropic: match args.get(7).and_then(Value::as_str) {
                        Some("disable") => Anisotropic::Disable,
                        Some("forceEnable") => Anisotropic::ForceEnable,
                        _ => Anisotropic::Enable,
                    },
                    texture_mip_limit: num(8).unwrap_or(0.0).clamp(0.0, 3.0) as u32,
                    // The level's pipeline settings: light shafts, HDR, additional lights (-1:
                    // no limit) and their shadows.
                    pipeline: PipelineAsset {
                        volumetric_fog: !matches!(args.get(9), Some(Value::Bool(false))),
                        hdr: !matches!(args.get(10), Some(Value::Bool(false))),
                        max_additional_lights: num(11)
                            .ok()
                            .filter(|n| *n >= 0.0)
                            .map_or(usize::MAX, |n| n.min(256.0) as usize),
                        additional_light_shadows: !matches!(args.get(12), Some(Value::Bool(false))),
                    },
                };
            }
            // `shadowSettings 0 distance resolution cascades` (Unity's quality settings).
            "shadowSettings" => {
                let resolution = num(2).unwrap_or(2048.0).clamp(256.0, 8192.0) as u32;
                let cascades = num(3).unwrap_or(1.0).clamp(1.0, 4.0) as u32;
                self.shadow_settings = (num(1)?.max(1.0), resolution, cascades);
            }
            // `renderTexture id width height`: a texture cameras draw into (removed with
            // `textureRemove`).
            "renderTexture" => {
                let size = (
                    num(1)?.clamp(1.0, 16384.0) as u32,
                    num(2)?.clamp(1.0, 16384.0) as u32,
                );
                self.render_textures.insert(id, size);
                self.texture_sizes.insert(id, size);
                self.render_texture_uploads.push((id, size));
            }
            // `post 0 postExposure(stops) tonemapping(none|neutral|aces) auto fixedEV
            // transition`: `auto` is `[minEV, maxEV, compensation, darkToLight, lightToDark,
            // metering (0 average, 1 centre-weighted, 2 spot), lowPercent, highPercent, hold,
            // darkAdaptation, darkBelow, curve points, ev0, stops0, ev1, stops1, ...]` or
            // null; `fixedEV` the manual
            // exposure (EV100) or null; `transition` seconds auto exposure blends in and out.
            "post" => {
                let auto = flat(args.get(3)).filter(|v| v.len() >= 5).map(|v| {
                    let at = |i: usize, d: f32| v.get(i).copied().unwrap_or(d);
                    let mut curve = [[0.0; 2]; MAX_CURVE_POINTS];
                    let points = (at(11, 0.0).max(0.0) as usize).min(MAX_CURVE_POINTS);
                    for (k, p) in curve.iter_mut().take(points).enumerate() {
                        *p = [at(12 + 2 * k, 0.0), at(13 + 2 * k, 0.0).clamp(-10.0, 10.0)];
                    }
                    AutoExposure {
                        min_ev: v[0].clamp(-20.0, 30.0),
                        max_ev: v[1].clamp(-20.0, 30.0),
                        compensation: v[2].clamp(-10.0, 10.0),
                        speed_dark_to_light: v[3].clamp(0.0, 100.0),
                        speed_light_to_dark: v[4].clamp(0.0, 100.0),
                        metering: match at(5, 1.0) as u32 {
                            0 => Metering::Average,
                            2 => Metering::Spot,
                            _ => Metering::CenterWeighted,
                        },
                        low_percent: at(6, 0.0).clamp(0.0, 100.0),
                        high_percent: at(7, 100.0).clamp(0.0, 100.0),
                        hold: at(8, 0.0) > 0.5,
                        dark_adaptation: at(9, 1.0).clamp(0.0, 1.0),
                        dark_below: at(10, 5.0).clamp(-20.0, 30.0),
                        curve,
                        curve_points: points as u8,
                    }
                });
                self.grading = Grading {
                    exposure: 2f32.powf(num(1).unwrap_or(0.0).clamp(-10.0, 10.0)),
                    tonemapping: match args.get(2).and_then(Value::as_str) {
                        Some("neutral") => Tonemapping::Neutral,
                        Some("aces") => Tonemapping::Aces,
                        _ => Tonemapping::None,
                    },
                    auto_exposure: auto,
                    fixed_ev: num(4).ok().map(|v| v.clamp(-20.0, 30.0)),
                    transition: num(5).unwrap_or(0.0).clamp(0.0, 60.0),
                    delta_time: self.grading.delta_time,
                };
            }
            // `exposureReset cameraId`: that camera's auto exposure jumps to what it sees.
            "exposureReset" => self.exposure_resets.push(id),
            // `bloom 0 threshold intensity scatter`; `bloom 0 null` turns it off.
            "bloom" => {
                self.bloom = match args.get(1) {
                    Some(Value::Null) | None => None,
                    _ => Some(Bloom {
                        threshold: num(1)?.max(0.0),
                        intensity: num(2)?.max(0.0),
                        scatter: num(3)?.clamp(0.0, 1.0),
                    }),
                };
            }
            // `fog 0 mode(linear|exponential|exponentialSquared) [r g b] density start end
            // [heightDensity baseHeight falloff]|null [intensity anisotropy steps distance r g
            // b]|null cull`; `fog 0 null` clears the air.
            // `clouds 0 [altitude size coverage opacity softness r g b windSpeed windDirection
            // fade]|null`: the wind's direction in degrees from +x towards +z.
            "clouds" => {
                self.clouds = match args.get(1) {
                    Some(Value::Array(v)) => {
                        let n = |i: usize, d: f32| match v.get(i) {
                            Some(Value::Number(x)) => *x as f32,
                            _ => d,
                        };
                        let (speed, dir) = (n(8, 0.0), n(9, 0.0).to_radians());
                        let offset = self.clouds.map_or([0.0; 2], |(c, _)| c.offset);
                        Some((
                            CloudLayer {
                                altitude: n(0, 1500.0),
                                size: n(1, 2000.0),
                                coverage: n(2, 0.5),
                                opacity: n(3, 1.0),
                                softness: n(4, 0.3),
                                color: [n(5, 0.8), n(6, 0.8), n(7, 0.82)],
                                offset,
                                fade: n(10, 20000.0),
                            },
                            [speed * dir.cos(), speed * dir.sin()],
                        ))
                    }
                    _ => None,
                };
            }
            "fog" => {
                self.fog = match args.get(1) {
                    Some(Value::Null) | None => None,
                    _ => Some(Fog {
                        mode: match args.get(1).and_then(Value::as_str) {
                            Some("linear") => FogMode::Linear,
                            Some("exponential") => FogMode::Exponential,
                            Some("exponentialSquared") => FogMode::ExponentialSquared,
                            other => return Err(format!("unknown fog mode {other:?}")),
                        },
                        color: nums::<3>(args.get(2)).ok_or("fog needs [r, g, b]")?,
                        density: num(3)?.max(0.0),
                        start: num(4)?,
                        end: num(5)?,
                        height: nums::<3>(args.get(6)).map(|[density, base_height, falloff]| {
                            HeightFog {
                                density: density.max(0.0),
                                base_height,
                                falloff: falloff.max(1e-4),
                            }
                        }),
                        volumetric: flat(args.get(7)).filter(|v| v.len() >= 4).map(|v| {
                            VolumetricFog {
                                intensity: v[0].max(0.0),
                                anisotropy: v[1].clamp(-0.95, 0.95),
                                steps: v[2].clamp(1.0, 16.0) as u32,
                                distance: v[3].max(0.1),
                                albedo: match v[4..] {
                                    [r, g, b, ..] => [r, g, b].map(|c| c.max(0.0)),
                                    _ => [1.0; 3],
                                },
                                sharp_distance: v.get(7).copied().unwrap_or(0.0).max(0.0),
                                sharp_samples: v.get(8).copied().unwrap_or(32.0).clamp(4.0, 128.0)
                                    as u32,
                                detail: v.get(9).copied().unwrap_or(2.0).clamp(0.0, 3.0) as u8,
                                denoising: match v.get(10).copied().unwrap_or(3.0) as u32 {
                                    0 => Denoising::None,
                                    1 => Denoising::Reprojection,
                                    2 => Denoising::Gaussian,
                                    _ => Denoising::Both,
                                },
                                history_weight: v.get(11).copied().unwrap_or(0.9).clamp(0.0, 0.98),
                                samples: v.get(12).copied().unwrap_or(1.0).clamp(1.0, 8.0) as u32,
                                motion_samples: v.get(13).copied().unwrap_or(2.0).clamp(1.0, 8.0)
                                    as u32,
                            }
                        }),
                        cull: !matches!(args.get(8), Some(Value::Bool(false))),
                    }),
                };
            }
            // `fogVolume component object [sx sy sz] density [r g b] blendDistance
            // shape(box|ellipsoid)`
            "fogVolume" => {
                let [x, y, z] = nums::<3>(args.get(2)).ok_or("fogVolume needs [x, y, z]")?;
                self.fog_volumes.insert(
                    id,
                    FogBox {
                        object: id_at(1)?,
                        size: Vec3::new(x, y, z).abs(),
                        density: num(3)?.max(0.0),
                        color: nums::<3>(args.get(4)).ok_or("fogVolume needs [r, g, b]")?,
                        blend: num(5).unwrap_or(1.0).max(0.0),
                        ellipsoid: args.get(6).and_then(Value::as_str) == Some("ellipsoid"),
                    },
                );
            }
            // `ambient 0 [r g b]`
            "ambient" => {
                self.ambient = nums::<3>(args.get(1)).ok_or("ambient needs [r, g, b]")?;
            }
            "remove" => {
                self.cameras.remove(&id);
                self.fog_volumes.remove(&id);
                self.planes.remove(&id);
                self.renderers.remove(&id);
                self.skins.remove(&id);
                self.lights.remove(&id);
                self.sprites.remove(&id);
                self.strokes.remove(&id);
                self.mirrors.remove(&id);
                self.probes.remove(&id);
                self.particles.remove(&id);
                self.lod_groups.remove(&id);
            }
            other => return Err(format!("unknown render op `{other}`")),
        }
        Ok(())
    }

    /// The probes asked for have been rendered (call after drawing a frame).
    pub fn probes_rendered(&mut self) {
        self.probe_requests.clear();
    }

    /// What the last frame saw of animated objects that cull (`roots`), watching their
    /// renderers this frame.
    pub(crate) fn animation_sight(
        &mut self,
        scene: &ScriptScene,
        roots: &[platypus_scene::ObjectId],
    ) -> crate::anim_cull::Sight {
        let renderers = self.renderers.iter().map(|(id, r)| (*id, r.object));
        self.culling
            .sight(scene, renderers, roots, crate::anim_cull::blockers)
    }

    /// Advances particle systems on active objects by `dt` (scaled) seconds, emitting
    /// from where their objects are now. Returns `[id, count, alive (0/1), ...]` for
    /// systems whose count or state changed, for `__platypus_particles`.
    pub fn update_particles(&mut self, dt: f32, scene: &ScriptScene) -> Vec<f64> {
        let world = |o: ScriptObject| scene.object(o).map(|id| scene.scene.world_matrix(id));
        for (id, count) in self.pending_emits.drain(..) {
            if let Some(p) = self.particles.get_mut(&id)
                && let Some(w) = world(p.object)
            {
                p.emit(count, &w);
            }
        }
        for p in self.particles.values_mut() {
            if !scene.active(p.object) {
                continue;
            }
            if let Some(w) = world(p.object) {
                p.update(dt, &w);
            }
        }
        let mut changed = Vec::new();
        for (id, p) in &mut self.particles {
            if let Some((count, alive)) = p.take_change() {
                changed.extend([f64::from(*id), count as f64, f64::from(u8::from(alive))]);
            }
        }
        changed
    }

    /// Every active camera's view (lowest depth first, then the order they were added),
    /// each seeing the planes and `extra` quads (with their objects) on the layers it
    /// draws; empty without cameras (then only screen UI is drawn).
    pub fn views(
        &self,
        scene: &ScriptScene,
        size: (u32, u32),
        extra: &[(ScriptObject, WorldQuad)],
    ) -> Vec<WorldView> {
        let world = |o: ScriptObject| scene.object(o).map(|id| scene.scene.world_matrix(id));
        // Each `LineRenderer2D`'s points through its object, as world segments, and its
        // fill's triangles.
        let mut shapes: Vec<&Stroke2d> = self.strokes.values().collect();
        shapes.sort_by_key(|s| s.order);
        let strokes: Vec<Tagged<(Vec<Stroke>, Vec<FillVertex>)>> = shapes
            .into_iter()
            .filter(|s| scene.active(s.object) && s.points.len() >= 2)
            .filter_map(|s| {
                let m = Mat4::from(world(s.object)?);
                let p: Vec<[f32; 3]> = s
                    .points
                    .iter()
                    .map(|&[x, y]| m.transform_point3(Vec3::new(x, y, 0.0)).into())
                    .collect();
                let n = p.len();
                // A zero width draws no outline (a fill on its own).
                let count = match (s.width > 0.0, s.closed) {
                    (false, _) => 0,
                    (true, true) => n,
                    (true, false) => n - 1,
                };
                let segments = (0..count)
                    .map(|i| Stroke {
                        a: p[i],
                        b: p[(i + 1) % n],
                        width: s.width,
                        color: s.color,
                    })
                    .collect();
                let fill = s.fill.as_ref().map_or_else(Vec::new, |(color, triangles)| {
                    triangles
                        .iter()
                        .map(|&[x, y]| FillVertex {
                            position: m.transform_point3(Vec3::new(x, y, 0.0)).into(),
                            color: *color,
                        })
                        .collect()
                });
                Tagged::new(scene, s.object, (segments, fill))
            })
            .collect();
        let quads: Vec<Tagged<WorldQuad>> = self
            .planes
            .values()
            .filter(|p| scene.active(p.object))
            .filter_map(|p| {
                let quad =
                    WorldQuad::colored(Mat4::from(world(p.object)?) * lay_flat(p.size), p.color);
                Tagged::new(scene, p.object, quad)
            })
            .chain(
                self.sprites
                    .values()
                    .filter(|s| scene.active(s.object))
                    .flat_map(|s| {
                        // Drawn once its texture has loaded.
                        let size = self.texture_sizes.get(&s.texture).copied();
                        let to_world = world(s.object).map(Mat4::from);
                        size.zip(to_world)
                            .into_iter()
                            .flat_map(move |(size, to_world)| {
                                sprite_quads(s, size)
                                    .into_iter()
                                    .filter_map(move |(local, uv)| {
                                        let quad = WorldQuad {
                                            model: to_world * local,
                                            color: s.color,
                                            texture: Some(WorldTexture::Asset(s.texture)),
                                            uv,
                                            additive: false,
                                            flat: true,
                                            order: s.order,
                                            layer: 0,
                                        };
                                        Tagged::new(scene, s.object, quad)
                                    })
                            })
                    }),
            )
            .chain(
                extra
                    .iter()
                    .filter_map(|&(object, quad)| Tagged::new(scene, object, quad)),
            )
            .collect();
        // Reflection probes where their objects are.
        let probes: Vec<ReflectionProbe> = self
            .probes
            .iter()
            .filter(|(_, p)| scene.active(p.object))
            .filter_map(|(&id, p)| {
                Some(ReflectionProbe {
                    id,
                    position: world(p.object)?.translation.into(),
                    half_extents: p.half_extents,
                    resolution: p.resolution,
                    box_projection: p.box_projection,
                    render: p.every_frame || self.probe_requests.contains(&id),
                    culling_mask: p.culling_mask,
                })
            })
            .collect();
        // Mirrors: their objects' planes in the world.
        let mirror_planes: HashMap<ScriptObject, Mirror> = self
            .mirrors
            .values()
            .filter_map(|&(object, normal, culling_mask)| {
                let w = Mat4::from(world(object)?);
                Some((
                    object,
                    Mirror {
                        normal: w.transform_vector3(normal).normalize_or(Vec3::Y),
                        point: w.transform_point3(Vec3::ZERO),
                        culling_mask,
                    },
                ))
            })
            .collect();
        // Skinned renderers' bone matrices, one list for every view (`bone_cache.rs`).
        let mut cache = self.bone_cache.borrow_mut();
        let (bones, skinned) = cache.update(&self.skins, scene);
        let meshes: Vec<Tagged<MeshInstance>> = self
            .renderers
            .iter()
            .filter(|(_, r)| scene.active(r.object))
            .filter_map(|(&id, r)| {
                let mesh = MeshInstance {
                    mesh: r.mesh,
                    model: Mat4::from(world(r.object)?),
                    color: r.color,
                    emission: r.emission,
                    smoothness: r.smoothness,
                    metallic: r.metallic,
                    unlit: r.unlit,
                    texture: r.texture.map(WorldTexture::Asset),
                    tiling: r.tiling,
                    glow: MeshGlow::None,
                    cast_shadows: r.cast_shadows,
                    mirror: mirror_planes.get(&r.object).copied(),
                    layer: 0,
                    maps: r.extras.maps,
                    specular: r.extras.specular,
                    mode: r.extras.mode,
                    alpha_cutoff: r.extras.alpha_cutoff,
                    double_sided: r.extras.double_sided,
                    bones: skinned.get(&id).copied(),
                    visibility_key: self.culling.key(id),
                };
                Some(Tagged {
                    renderer: Some(id),
                    ..Tagged::new(scene, r.object, mesh)?
                })
            })
            .collect();
        // LOD groups on active objects: where they are, how big, and every renderer of
        // theirs (each camera shows one level's).
        let lods: Vec<(Vec3, f32, &LodGroup)> = self
            .lod_groups
            .values()
            .filter(|g| scene.active(g.object))
            .filter_map(|g| {
                let (scale, _, at) = world(g.object)?.to_scale_rotation_translation();
                Some((at, g.size * scale.abs().max_element(), g))
            })
            .collect();
        // Visible spot light beams: cones as long as the light's range.
        let meshes: Vec<Tagged<MeshInstance>> = meshes
            .into_iter()
            .chain(
                self.lights
                    .values()
                    .filter(|l| l.beam > 0.0 && scene.active(l.object))
                    .filter_map(|l| {
                        let LightKind::Spot { outer_degrees, .. } = l.kind else {
                            return None;
                        };
                        let (_, rotation, position) =
                            world(l.object)?.to_scale_rotation_translation();
                        let radius = l.range * (outer_degrees.min(170.0).to_radians() / 2.0).tan();
                        let [r, g, b] = l.color;
                        let peak = r.max(g).max(b).max(1e-6) / l.beam;
                        let beam = MeshInstance {
                            mesh: BEAM_MESH,
                            model: Mat4::from_scale_rotation_translation(
                                Vec3::new(radius, radius, l.range),
                                rotation,
                                position,
                            ),
                            color: [r / peak, g / peak, b / peak, 1.0],
                            emission: [0.0; 3],
                            smoothness: 0.0,
                            metallic: 0.0,
                            unlit: true,
                            texture: None,
                            tiling: [1.0, 1.0, 0.0, 0.0],
                            glow: MeshGlow::Beam,
                            cast_shadows: false,
                            mirror: None,
                            layer: 0,
                            ..Default::default()
                        };
                        Tagged::new(scene, l.object, beam)
                    }),
            )
            .collect();
        // Light halos: a glow at each light, facing each camera.
        let halos: Vec<Tagged<(f32, [f32; 4])>> = self
            .lights
            .values()
            .filter(|l| l.halo > 0.0 && scene.active(l.object))
            .filter_map(|l| {
                let [r, g, b] = l.color;
                // The light's hue at a steady brightness, whatever its intensity.
                let peak = r.max(g).max(b).max(1e-6);
                Tagged::new(
                    scene,
                    l.object,
                    (l.halo, [r / peak, g / peak, b / peak, 0.8]),
                )
            })
            .collect();
        let lights: Vec<Light> = self
            .lights
            .values()
            .filter(|l| scene.active(l.object))
            .filter_map(|l| {
                let (_, rotation, position) = world(l.object)?.to_scale_rotation_translation();
                Some(Light {
                    // A pyramid's width runs along its object's +x.
                    kind: match l.kind {
                        LightKind::Pyramid {
                            inner_degrees,
                            outer_degrees,
                            aspect,
                            ..
                        } => LightKind::Pyramid {
                            inner_degrees,
                            outer_degrees,
                            aspect,
                            right: rotation * Vec3::X,
                        },
                        kind => kind,
                    },
                    color: l.color,
                    position,
                    // Lights shine along their object's forward (-z), like Unity's.
                    direction: rotation * Vec3::NEG_Z,
                    range: l.range,
                    shadows: l.shadows,
                    culling_mask: l.culling_mask,
                    volumetric: l.volumetric,
                })
            })
            .collect();
        // Fog volumes in the world: centred on their objects, stretched by their scale.
        let fog_volumes: Vec<Tagged<FogVolume>> = self
            .fog_volumes
            .values()
            .filter(|f| scene.active(f.object))
            .filter_map(|f| {
                let (scale, _, centre) = world(f.object)?.to_scale_rotation_translation();
                Tagged::new(
                    scene,
                    f.object,
                    FogVolume {
                        centre,
                        half_extents: f.size * scale.abs() * 0.5,
                        density: f.density,
                        color: f.color,
                        blend: f.blend,
                        ellipsoid: f.ellipsoid,
                    },
                )
            })
            .collect();
        let mut cameras: Vec<(u32, &Camera)> = self
            .cameras
            .iter()
            .filter(|(_, c)| scene.active(c.object))
            .map(|(&id, c)| (id, c))
            .collect();
        cameras.sort_by(|a, b| a.1.depth.total_cmp(&b.1.depth));
        cameras
            .into_iter()
            .filter_map(|(camera_id, camera)| {
                // Cameras ignore scale: only where they are and which way they face.
                let (_, rotation, position) = world(camera.object)?.to_scale_rotation_translation();
                let forward = rotation * Vec3::NEG_Z;
                let shows = |layer: u8, at: Vec3| camera.shows(layer, at, position, forward);
                // Renderers in a LOD group's other levels (or all of them when it's culled).
                let mut lod_hidden = std::collections::HashSet::new();
                for &(at, size, group) in &lods {
                    let height =
                        camera.screen_height(size, at.distance(position)) * self.quality.lod_bias;
                    let level = group.level(height);
                    for (i, level_renderers) in group.renderers.iter().enumerate() {
                        if Some(i) != level {
                            lod_hidden.extend(level_renderers.iter().copied());
                        }
                    }
                    // Ones also in the shown level stay.
                    if let Some(l) = level {
                        for r in &group.renderers[l] {
                            lod_hidden.remove(r);
                        }
                    }
                }
                let mut quads: Vec<WorldQuad> = quads
                    .iter()
                    .filter(|q| shows(q.layer, q.at))
                    .map(|q| WorldQuad {
                        layer: q.layer,
                        ..q.item
                    })
                    .collect();
                // Particles and halos face this camera.
                for p in self.particles.values().filter(|p| scene.active(p.object)) {
                    if let Some(w) = world(p.object)
                        && shows(scene.layer(p.object), w.translation.into())
                    {
                        let first = quads.len();
                        p.quads(&w, rotation, &mut quads);
                        let layer = scene.layer(p.object);
                        for q in &mut quads[first..] {
                            q.layer = layer;
                        }
                    }
                }
                for halo in halos.iter().filter(|h| shows(h.layer, h.at)) {
                    let (at, (size, color)) = (halo.at, halo.item);
                    // Pulled half its size towards the camera, so walls and panes near
                    // the light don't slice the glow in two.
                    let towards = (position - at).normalize_or_zero();
                    let at = at + towards * (size * 0.5).min(at.distance(position) * 0.5);
                    quads.push(WorldQuad {
                        model: Mat4::from_scale_rotation_translation(
                            Vec3::new(size, size, 1.0),
                            rotation,
                            at,
                        ),
                        color,
                        texture: Some(WorldTexture::Asset(particles::DEFAULT_TEXTURE)),
                        uv: [0.0, 0.0, 1.0, 1.0],
                        additive: true,
                        flat: false,
                        order: 0,
                        layer: halo.layer,
                    });
                }
                // A camera with a missing render texture draws nothing.
                let target = camera.target.or(camera.canvas.map(canvas_texture));
                let (target, target_size) = match target {
                    Some(t) => (Some(t), *self.render_textures.get(&t)?),
                    None => (None, size),
                };
                let [r, g, b, a] = camera.clear;
                let clear = Color {
                    r: r as f64,
                    g: g as f64,
                    b: b as f64,
                    a: a as f64,
                };
                // This camera's shapes, in order, each one's fill and outline together.
                let (mut shown_strokes, mut shown_fills, mut shown_shapes) =
                    (Vec::new(), Vec::new(), Vec::new());
                for t in strokes.iter().filter(|t| shows(t.layer, t.at)) {
                    let (f, s) = (shown_fills.len() as u32, shown_strokes.len() as u32);
                    shown_strokes.extend_from_slice(&t.item.0);
                    shown_fills.extend_from_slice(&t.item.1);
                    shown_shapes.push(ShapeRun {
                        fills: f..shown_fills.len() as u32,
                        strokes: s..shown_strokes.len() as u32,
                    });
                }
                Some(WorldView {
                    meshes: meshes
                        .iter()
                        .filter(|m| shows(m.layer, m.at))
                        .filter(|m| m.renderer.is_none_or(|r| !lod_hidden.contains(&r)))
                        .map(|m| MeshInstance {
                            layer: m.layer,
                            ..m.item
                        })
                        .collect(),
                    lights: lights.clone(),
                    ambient: self.ambient,
                    skybox: camera.skybox,
                    clip_plane: None,
                    probes: probes.clone(),
                    fxaa: camera.fxaa,
                    post: self.post.for_camera(camera_id),
                    target_texture: target,
                    bones: bones.clone(),
                    grid: camera.grid,
                    lines: camera.lines.clone(),
                    strokes: shown_strokes,
                    fills: shown_fills,
                    shapes: shown_shapes,
                    fog: self.fog.filter(|_| camera.fog),
                    clouds: self.clouds.map(|(c, _)| c),
                    exposure_key: camera_id,
                    fog_volumes: if camera.fog {
                        // The nearest ones the camera's layers show, up to the renderer's limit.
                        let mut near: Vec<FogVolume> = fog_volumes
                            .iter()
                            .filter(|t| shows(t.layer, t.at))
                            .map(|t| t.item)
                            .collect();
                        near.sort_by(|a, b| {
                            a.centre
                                .distance_squared(position)
                                .total_cmp(&b.centre.distance_squared(position))
                        });
                        near.truncate(MAX_FOG_VOLUMES);
                        near
                    } else {
                        Vec::new()
                    },
                    camera: WorldCamera::new(
                        Mat4::from_rotation_translation(rotation, position),
                        camera.projection,
                        camera.near,
                        camera.far,
                        viewport_size(camera.rect, target_size),
                        clear,
                    ),
                    viewport: camera.rect,
                    quads,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // M9-API-913
    #[test]
    fn vsync_is_on_by_default_and_follows_the_quality_op() {
        assert!(Quality::default().vsync);
        let mut views = WorldViews::default();
        let q = |v: Option<f64>| {
            let mut args: Vec<Value> = vec![
                0.0.into(),
                1.0.into(),
                1.0.into(),
                "all".into(),
                1.0.into(),
                1.0.into(),
            ];
            args.extend(v.map(Value::from));
            args
        };
        views.apply("quality", &q(Some(0.0))).unwrap();
        assert!(!views.quality.vsync);
        views.apply("quality", &q(None)).unwrap();
        assert!(views.quality.vsync, "left out: the default");
        views.apply("quality", &q(Some(0.0))).unwrap();
        views.apply("quality", &q(Some(1.0))).unwrap();
        assert!(views.quality.vsync);
        views.apply("quality", &q(Some(0.0))).unwrap();
        assert!(!views.quality.vsync);
    }

    // M7-HD-01
    #[test]
    fn the_hd_pipeline_reads_lights_in_lux_and_lumens() {
        let light = |physical: bool, kind: &str, intensity: f64| {
            let mut views = WorldViews::with_physical_units(physical);
            let args = [
                Value::from(1.0),
                Value::from(2.0),
                Value::String(kind.into()),
                Value::Array(vec![Value::from(1.0), Value::from(1.0), Value::from(1.0)]),
                Value::from(intensity),
            ];
            views.apply("light", &args).unwrap();
            views.lights[&1].color[0]
        };
        use std::f32::consts::PI;
        // As it is outside HD; the sun's lux / pi, a lamp's lumens / 4 pi² in HD.
        assert_eq!(light(false, "directional", 3.0), 3.0);
        assert!((light(true, "directional", 100_000.0) - 100_000.0 / PI).abs() < 1.0);
        assert!((light(true, "point", 800.0) - 800.0 / (4.0 * PI * PI)).abs() < 1e-3);
    }

    // M5-AST-01
    #[test]
    fn changed_textures_and_models_reload_under_their_ids() {
        let dir = std::env::temp_dir().join(format!("platypus-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        let png = |w: u32, h: u32| {
            let mut bytes = Vec::new();
            image::RgbaImage::new(w, h)
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .unwrap();
            bytes
        };
        std::fs::write(dir.join("assets/crate.png"), png(4, 4)).unwrap();
        let fixtures =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        std::fs::copy(
            fixtures.join("assets/worm.glb"),
            dir.join("assets/worm.glb"),
        )
        .unwrap();
        let mut views = WorldViews::default();
        let args = [Value::from(5.0), Value::String("./assets/crate.png".into())];
        assert_eq!(
            views.texture_op("texture", &dir, &args).unwrap(),
            Some((5, 4, 4))
        );
        views
            .model_op(
                &dir,
                &[Value::from(1.0), Value::String("assets/worm.glb".into())],
            )
            .unwrap();
        let mesh_ids: Vec<u32> = views.mesh_uploads.drain(..).map(|(id, _)| id).collect();
        views.texture_uploads.clear();
        // The texture file grows: it loads again under id 5 with its new size.
        std::fs::write(dir.join("assets/crate.png"), png(8, 2)).unwrap();
        let (sizes, models) = views.reload_asset(&dir, "assets/crate.png");
        assert_eq!(sizes, [(5, 8, 2)]);
        assert!(models.is_empty());
        assert!(matches!(views.texture_uploads.as_slice(), [(5, Some(_))]));
        // The model loads again with the same mesh ids.
        let (_, models) = views.reload_asset(&dir, "assets/worm.glb");
        assert_eq!(models.len(), 1);
        let again: Vec<u32> = views.mesh_uploads.drain(..).map(|(id, _)| id).collect();
        assert_eq!(again, mesh_ids);
        // Unrelated files change nothing.
        let (sizes, models) = views.reload_asset(&dir, "assets/other.png");
        assert!(sizes.is_empty() && models.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sprite(rect: Option<[f32; 4]>, pivot: [f32; 2], flip: (bool, bool)) -> Sprite {
        Sprite {
            object: 1,
            texture: 1,
            rect,
            pivot,
            pixels_per_unit: 16.0,
            color: [1.0; 4],
            flip,
            order: 0,
            mode: SpriteMode::Simple,
            size: None,
            border: [0.0; 4],
        }
    }

    // RDY-TEX-01
    #[test]
    fn tiled_sprites_repeat_across_their_size_and_cut_off_at_the_edge() {
        // A 16 px tile is a metre; a 2.5 x 1 m area is two whole tiles and a half one.
        let mut s = sprite(None, [0.0, 0.0], (false, false));
        s.mode = SpriteMode::Tiled;
        s.size = Some([2.5, 1.0]);
        let quads = sprite_quads(&s, (16, 16));
        assert_eq!(quads.len(), 3);
        let place = |(m, _): &(Mat4, [f32; 4])| {
            let (scale, _, pos) = m.to_scale_rotation_translation();
            (scale.x, pos.x)
        };
        assert_eq!(place(&quads[0]), (1.0, 0.5));
        assert_eq!(place(&quads[2]), (0.5, 2.25));
        // The cut tile shows the left half of the texture.
        assert_eq!(quads[2].1, [0.0, 0.0, 0.5, 1.0]);
        // A huge area of a tiny tile is capped, not unbounded.
        s.size = Some([1.0e6, 1.0e6]);
        assert_eq!(sprite_quads(&s, (16, 16)).len(), MAX_SPRITE_TILES);
    }

    // RDY-TEX-01
    #[test]
    fn sliced_sprites_keep_their_corners_and_stretch_the_rest() {
        // A 48 px sprite (3 m) with 16 px borders, drawn 8 x 5 m: corners stay a metre.
        let mut s = sprite(None, [0.5, 0.5], (false, false));
        s.mode = SpriteMode::Sliced;
        s.size = Some([8.0, 5.0]);
        s.border = [16.0; 4];
        let quads = sprite_quads(&s, (48, 48));
        assert_eq!(quads.len(), 9);
        let (scale, _, pos) = quads[0].0.to_scale_rotation_translation();
        assert_eq!((scale.x, scale.y), (1.0, 1.0));
        assert_eq!((pos.x, pos.y), (-3.5, -2.0));
        let (middle, _, _) = quads[4].0.to_scale_rotation_translation();
        assert_eq!((middle.x, middle.y), (6.0, 3.0));
        // The middle shows the middle third of the texture.
        assert_eq!(quads[4].1[2], 1.0 / 3.0);
        // Smaller than its borders: they shrink together instead of overlapping.
        s.size = Some([1.0, 5.0]);
        let narrow = sprite_quads(&s, (48, 48));
        let bottom = narrow[0].0.to_scale_rotation_translation().2.y;
        let widths: f32 = narrow
            .iter()
            .map(|(m, _)| m.to_scale_rotation_translation())
            .filter(|(_, _, pos)| pos.y == bottom)
            .map(|(scale, _, _)| scale.x)
            .sum();
        assert!((widths - 1.0).abs() < 1e-5, "{widths}");
    }

    // M5-TEX-02
    #[test]
    fn sprites_are_sized_by_pixels_per_unit_and_sit_on_their_pivot() {
        // A 16 × 32 cell from the bottom-right of a 64 × 64 sheet, pivot at its feet.
        let (m, uv) = sprite_quad(
            &sprite(Some([48.0, 0.0, 16.0, 32.0]), [0.5, 0.0], (false, false)),
            (64, 64),
        );
        let (scale, _, pos) = m.to_scale_rotation_translation();
        assert_eq!(scale, Vec3::new(1.0, 2.0, 1.0));
        // The quad's centre is a metre above the origin, so its bottom edge is on it.
        assert_eq!(pos, Vec3::new(0.0, 1.0, 0.0));
        // Bottom-up pixels become top-down UVs: rows 0..32 from the bottom are v 0.5..1.
        assert_eq!(uv, [0.75, 0.5, 0.25, 0.5]);
        let (_, flipped) = sprite_quad(&sprite(None, [0.5, 0.5], (true, true)), (64, 64));
        assert_eq!(flipped, [1.0, 1.0, -1.0, -1.0]);
    }

    // M5-TEX-01
    #[test]
    fn textures_from_pixels_queue_uploads_and_report_their_size() {
        let mut views = WorldViews::default();
        let root = Path::new(".");
        let px = Value::Array((0..8).map(|v| Value::from(f64::from(v))).collect());
        let args = [
            Value::from(4.0),
            Value::Null,
            Value::String("point".into()),
            Value::String("clamp".into()),
            Value::Bool(false),
            Value::from(1.0),
            Value::from(2.0),
            px,
        ];
        assert_eq!(
            views.texture_op("texture", root, &args),
            Ok(Some((4, 1, 2)))
        );
        let (id, data) = views.texture_uploads.pop().unwrap();
        let data = data.unwrap();
        assert_eq!(
            (id, data.filter, data.wrap, data.mipmaps),
            (4, TextureFilter::Point, TextureWrap::Clamp, false)
        );
        // Pixels that don't fill the size are refused, as are paths outside the game.
        let mut bad = args.clone();
        bad[6] = Value::from(3.0);
        assert!(views.texture_op("texture", root, &bad).is_err());
        let mut outside = args.clone();
        outside[1] = Value::String("../secret.png".into());
        assert!(views.texture_op("texture", root, &outside).is_err());
        assert_eq!(
            views.texture_op("textureRemove", root, &[Value::from(4.0)]),
            Ok(None)
        );
        assert!(matches!(views.texture_uploads.pop(), Some((4, None))));
    }

    // M5-SKN-05
    #[test]
    fn parts_of_one_skeleton_share_their_bone_matrices() {
        let n = Value::from;
        let mut scene = ScriptScene::new();
        let mut views = WorldViews::default();
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        for object in 2..=5 {
            let at = Value::Array(vec![n(f64::from(object)), n(0.0), n(0.0)]);
            scene
                .apply(
                    "create",
                    &[n(f64::from(object)), "bone".into(), Value::Null, at],
                )
                .unwrap();
        }
        scene.scene.update_world();
        views
            .apply(
                "camera",
                &[
                    n(100.0),
                    n(1.0),
                    n(60.0),
                    n(0.1),
                    n(1000.0),
                    Value::Null,
                    n(0.0),
                    Value::Bool(false),
                    n(5.0),
                    Value::Null,
                    "color".into(),
                ],
            )
            .unwrap();
        let renderer = |object: u32| Renderer {
            object,
            mesh: 1,
            color: [1.0; 4],
            emission: [0.0; 3],
            smoothness: 0.0,
            metallic: 0.0,
            unlit: false,
            texture: None,
            tiling: [1.0, 1.0, 0.0, 0.0],
            cast_shadows: true,
            extras: MaterialExtras::default(),
        };
        let skin = |bones: Vec<ScriptObject>| Skin {
            bindposes: vec![Mat4::IDENTITY; bones.len()],
            bones,
        };
        // A body and its joints on one skeleton (bones 2, 3), and a second figure (4, 5).
        views.set_renderer(10, renderer(2), Some(skin(vec![2, 3])));
        views.set_renderer(11, renderer(2), Some(skin(vec![2, 3])));
        views.set_renderer(12, renderer(4), Some(skin(vec![4, 5])));
        // Same bones, other bind poses: its own matrices.
        let mut odd = skin(vec![4, 5]);
        odd.bindposes[1] = Mat4::from_translation(Vec3::X);
        views.set_renderer(13, renderer(4), Some(odd));
        let v = views.views(&scene, (64, 64), &[]).remove(0);
        assert_eq!(v.bones.len(), 6, "three skeletons' worth, not four");
        let mut ranges: Vec<(u32, u32)> = v.meshes.iter().filter_map(|m| m.bones).collect();
        ranges.sort_unstable();
        ranges.dedup();
        assert_eq!(ranges.len(), 3);
        // Each range holds its bones' world matrices times their inverse bind poses.
        for m in &v.meshes {
            let (start, count) = m.bones.unwrap();
            assert_eq!(count, 2);
            let x = v.bones[start as usize].w_axis.x;
            assert!(x == 2.0 || x == 4.0, "{x}");
        }
    }

    // M5-LAY-02
    #[test]
    fn cameras_draw_only_their_layers_within_each_layers_distance() {
        let n = Value::from;
        let at = |x: f64, z: f64| Value::Array(vec![n(x), n(0.0), n(z)]);
        let mut scene = ScriptScene::new();
        let mut views = WorldViews::default();
        // A camera at the origin looking down -z; cubes ahead on layers 0, 3 and 5.
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        for (object, x, z, layer) in [
            (2.0, 0.0, -5.0, 0.0),
            (3.0, 0.0, -20.0, 3.0),
            (4.0, 8.0, -5.0, 5.0),
        ] {
            scene
                .apply("create", &[n(object), "cube".into(), Value::Null, at(x, z)])
                .unwrap();
            scene.apply("layer", &[n(object), n(layer)]).unwrap();
            views
                .apply(
                    "meshRenderer",
                    &[n(object + 10.0), n(object), "cube".into()],
                )
                .unwrap();
        }
        scene.scene.update_world();
        let camera = |mask: Option<f64>, distances: Option<[f64; 2]>, spherical: bool| {
            let mut far = vec![n(0.0); 32];
            if let Some([layer3, layer5]) = distances {
                far[3] = n(layer3);
                far[5] = n(layer5);
            }
            vec![
                n(100.0),
                n(1.0),
                n(60.0),
                n(0.1),
                n(1000.0),
                Value::Null,
                n(0.0),
                Value::Bool(false),
                n(5.0),
                Value::Null,
                "color".into(),
                mask.map_or(Value::Null, n),
                Value::Array(far),
                Value::Bool(spherical),
            ]
        };
        let seen = |views: &WorldViews| -> Vec<Vec<f32>> {
            views
                .views(&scene, (64, 64), &[])
                .iter()
                .map(|v| {
                    let mut x: Vec<f32> = v.meshes.iter().map(|m| m.model.w_axis.z).collect();
                    x.sort_by(f32::total_cmp);
                    x
                })
                .collect()
        };
        // Default: every layer, no distances.
        views.apply("camera", &camera(None, None, false)).unwrap();
        assert_eq!(seen(&views), vec![vec![-20.0, -5.0, -5.0]]);
        // Layer 5 off (Unity's ~(1 << 5) as an unsigned mask).
        let mask = f64::from(!(1u32 << 5));
        views
            .apply("camera", &camera(Some(mask), None, false))
            .unwrap();
        assert_eq!(seen(&views), vec![vec![-20.0, -5.0]]);
        // Layer 3 only out to 10 m: the far cube goes.
        views
            .apply("camera", &camera(None, Some([10.0, 0.0]), false))
            .unwrap();
        assert_eq!(seen(&views), vec![vec![-5.0, -5.0]]);
        // Layer 5 out to 6 m: its cube is 5 m along the view but 9.4 m away, so only a
        // spherical distance hides it.
        views
            .apply("camera", &camera(None, Some([0.0, 6.0]), false))
            .unwrap();
        assert_eq!(seen(&views), vec![vec![-20.0, -5.0, -5.0]]);
        views
            .apply("camera", &camera(None, Some([0.0, 6.0]), true))
            .unwrap();
        assert_eq!(seen(&views), vec![vec![-20.0, -5.0]]);
    }

    // M5-RND-27
    #[test]
    fn canvas_cameras_get_a_texture_that_follows_the_box() {
        let n = Value::from;
        let mut scene = ScriptScene::new();
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        scene.scene.update_world();
        let mut views = WorldViews::default();
        let camera = |canvas: Value| {
            let mut a = vec![n(100.0), n(1.0), n(60.0), n(0.1), n(1000.0)];
            a.resize(18, Value::Null);
            a.push(canvas);
            a
        };
        views.apply("camera", &camera(n(7.0))).unwrap();
        let tex = canvas_texture(7);
        // Not laid out yet: no texture, and the camera draws nothing (not on the screen either).
        assert_eq!(views.sync_canvases(|_| None), CanvasChanges::default());
        assert!(views.views(&scene, (64, 64), &[]).is_empty());
        let size = std::cell::Cell::new((40, 30));
        let changes = views.sync_canvases(|node| (node == 7).then(|| size.get()));
        assert_eq!(changes.made, vec![(7, tex, (40, 30))]);
        assert_eq!(views.render_texture_uploads, vec![(tex, (40, 30))]);
        let v = views.views(&scene, (64, 64), &[]);
        assert_eq!(v[0].target_texture, Some(tex));
        // The same size: nothing; a new one: made again.
        let same = views.sync_canvases(|node| (node == 7).then(|| size.get()));
        assert_eq!(same, CanvasChanges::default());
        size.set((80, 30));
        let changes = views.sync_canvases(|node| (node == 7).then(|| size.get()));
        assert_eq!(changes.made, vec![(7, tex, (80, 30))]);
        // Back to the screen: the texture goes.
        views.texture_uploads.clear();
        views.apply("camera", &camera(Value::Null)).unwrap();
        let changes = views.sync_canvases(|_| Some((1, 1)));
        assert_eq!((changes.made.len(), changes.gone), (0, vec![7]));
        assert!(matches!(views.texture_uploads[..], [(t, None)] if t == tex));
        assert_eq!(views.views(&scene, (64, 64), &[])[0].target_texture, None);
    }

    // M5-RND-24
    #[test]
    fn strokes_follow_their_object_close_loops_and_keep_to_their_layer() {
        let n = Value::from;
        let mut scene = ScriptScene::new();
        let mut views = WorldViews::default();
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        // The line's object sits at (10, 0, -5) on layer 3.
        let at = Value::Array(vec![n(10.0), n(0.0), n(-5.0)]);
        scene
            .apply("create", &[n(2.0), "curve".into(), Value::Null, at])
            .unwrap();
        scene.apply("layer", &[n(2.0), n(3.0)]).unwrap();
        scene.scene.update_world();
        let stroke = |closed: bool| {
            vec![
                n(20.0),
                n(2.0),
                n(0.5),
                Value::Array(vec![n(1.0), n(0.0), n(0.0), n(1.0)]),
                Value::Bool(closed),
                Value::Array([0.0, 0.0, 1.0, 0.0, 1.0, 1.0].map(n).to_vec()),
            ]
        };
        let camera = |mask: Option<f64>| {
            vec![
                n(100.0),
                n(1.0),
                n(60.0),
                n(0.1),
                n(1000.0),
                Value::Null,
                n(0.0),
                Value::Bool(false),
                n(5.0),
                Value::Null,
                "color".into(),
                mask.map_or(Value::Null, n),
            ]
        };
        let strokes = |views: &WorldViews| views.views(&scene, (64, 64), &[])[0].strokes.clone();
        views.apply("camera", &camera(None)).unwrap();
        views.apply("stroke", &stroke(true)).unwrap();
        // Three points closed: three segments, in world space, the last back to the first.
        let s = strokes(&views);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].a, [10.0, 0.0, -5.0]);
        assert_eq!(s[0].b, [11.0, 0.0, -5.0]);
        assert_eq!(s[2].b, s[0].a);
        assert_eq!((s[0].width, s[0].color), (0.5, [1.0, 0.0, 0.0, 1.0]));
        // Open: two.
        views.apply("stroke", &stroke(false)).unwrap();
        assert_eq!(strokes(&views).len(), 2);
        // A camera without layer 3 doesn't draw it.
        let mask = f64::from(!(1u32 << 3));
        views.apply("camera", &camera(Some(mask))).unwrap();
        assert!(strokes(&views).is_empty());
        views.apply("camera", &camera(None)).unwrap();
        views.apply("remove", &[n(20.0)]).unwrap();
        assert!(strokes(&views).is_empty());
    }

    // M5-RND-25
    #[test]
    fn filled_shapes_send_their_triangles_through_their_object() {
        let n = Value::from;
        let mut scene = ScriptScene::new();
        let mut views = WorldViews::default();
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        let at = Value::Array(vec![n(10.0), n(0.0), n(-5.0)]);
        scene
            .apply("create", &[n(2.0), "square".into(), Value::Null, at])
            .unwrap();
        scene.scene.update_world();
        let mut camera = vec![n(100.0), n(1.0), n(60.0), n(0.1), n(1000.0)];
        camera.extend([Value::Null, n(0.0), Value::Bool(false), n(5.0)]);
        views.apply("camera", &camera).unwrap();
        let square = |width: f64, fill: Value| {
            vec![
                n(20.0),
                n(2.0),
                n(width),
                Value::Array(vec![n(1.0); 4]),
                Value::Bool(true),
                Value::Array([0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0].map(n).to_vec()),
                fill,
                "evenodd".into(),
            ]
        };
        let view = |views: &WorldViews| views.views(&scene, (64, 64), &[]).remove(0);
        // Filled with no outline: two triangles, moved with the object, in the fill colour.
        let green = Value::Array(vec![n(0.0), n(1.0), n(0.0), n(1.0)]);
        views.apply("stroke", &square(0.0, green)).unwrap();
        let v = view(&views);
        assert!(v.strokes.is_empty());
        assert_eq!(v.fills.len(), 6);
        assert!(v.fills.iter().all(|f| f.color == [0.0, 1.0, 0.0, 1.0]));
        assert!(v.fills.iter().any(|f| f.position == [11.0, 1.0, -5.0]));
        // An outline and no fill.
        views.apply("stroke", &square(0.1, Value::Null)).unwrap();
        let v = view(&views);
        assert_eq!((v.strokes.len(), v.fills.len()), (4, 0));
    }

    // M5-RND-25
    #[test]
    fn shapes_draw_by_sorting_order_each_fill_with_its_outline() {
        let n = Value::from;
        let mut scene = ScriptScene::new();
        let mut views = WorldViews::default();
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        scene
            .apply("create", &[n(2.0), "shapes".into(), Value::Null])
            .unwrap();
        scene.scene.update_world();
        let mut camera = vec![n(100.0), n(1.0), n(60.0), n(0.1), n(1000.0)];
        camera.extend([Value::Null, n(0.0), Value::Bool(false), n(5.0)]);
        views.apply("camera", &camera).unwrap();
        // Made first but ordered on top: a filled, outlined triangle; then a plain line.
        let shape = |id: f64, order: f64, fill: bool, points: &[f64]| {
            vec![
                n(id),
                n(2.0),
                n(0.1),
                Value::Array(vec![n(1.0); 4]),
                Value::Bool(fill),
                Value::Array(points.iter().map(|&p| n(p)).collect()),
                if fill {
                    Value::Array(vec![n(1.0); 4])
                } else {
                    Value::Null
                },
                "evenodd".into(),
                n(order),
            ]
        };
        views
            .apply(
                "stroke",
                &shape(20.0, 5.0, true, &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
            )
            .unwrap();
        views
            .apply("stroke", &shape(21.0, 0.0, false, &[0.0, 0.0, 2.0, 2.0]))
            .unwrap();
        let v = views.views(&scene, (64, 64), &[]).remove(0);
        // The line (order 0) first, then the triangle's fill and its three sides.
        assert_eq!(
            v.shapes,
            vec![
                ShapeRun {
                    fills: 0..0,
                    strokes: 0..1
                },
                ShapeRun {
                    fills: 0..3,
                    strokes: 1..4
                },
            ]
        );
        assert_eq!(v.strokes[0].b, [2.0, 2.0, 0.0]);
    }

    // M5-LOD-01
    #[test]
    fn lod_groups_show_one_level_by_screen_height() {
        let n = Value::from;
        let mut scene = ScriptScene::new();
        let mut views = WorldViews::default();
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        scene
            .apply("create", &[n(2.0), "tree".into(), Value::Null])
            .unwrap();
        // A 60 degree camera at the origin looking down -z.
        views
            .apply("camera", &[n(10.0), n(1.0), n(60.0), n(0.1), n(1000.0)])
            .unwrap();
        let group = |views: &mut WorldViews, forced: f64| {
            let levels = Value::Array(vec![
                Value::Array(vec![n(20.0)]),
                Value::Array(vec![n(21.0)]),
            ]);
            let heights = Value::Array(vec![n(0.5), n(0.1)]);
            views
                .apply(
                    "lodGroup",
                    &[n(30.0), n(2.0), n(2.0), heights, levels, n(forced)],
                )
                .unwrap();
        };
        group(&mut views, -1.0);
        // Two renderers on the tree, told apart by colour: detailed (20, red) and
        // simple (21, blue).
        views
            .apply(
                "meshRenderer",
                &[
                    n(20.0),
                    n(2.0),
                    "cube".into(),
                    Value::Array(vec![n(1.0), n(0.0), n(0.0), n(1.0)]),
                ],
            )
            .unwrap();
        views
            .apply(
                "meshRenderer",
                &[
                    n(21.0),
                    n(2.0),
                    "cube".into(),
                    Value::Array(vec![n(0.0), n(0.0), n(1.0), n(1.0)]),
                ],
            )
            .unwrap();
        let level = |scene: &mut ScriptScene, views: &WorldViews, z: f64| -> Vec<&str> {
            let at = Value::Array(vec![n(0.0), n(0.0), n(z)]);
            scene.apply("local", &[n(2.0), at]).unwrap();
            scene.scene.update_world();
            views
                .views(scene, (100, 100), &[])
                .remove(0)
                .meshes
                .iter()
                .map(|m| {
                    if m.color[0] > 0.5 {
                        "detailed"
                    } else {
                        "simple"
                    }
                })
                .collect()
        };
        // 2 m tall: 1.15 of the view at 1.5 m, 0.35 at 5 m, 0.06 at 30 m.
        assert_eq!(level(&mut scene, &views, -1.5), ["detailed"]);
        assert_eq!(level(&mut scene, &views, -5.0), ["simple"]);
        assert!(
            level(&mut scene, &views, -30.0).is_empty(),
            "culled when tiny"
        );
        // A bias of 2 counts it twice as tall: detailed at 5 m.
        views
            .apply(
                "quality",
                &[n(0.0), n(1.0), n(1.0), "all".into(), n(1.0), n(2.0)],
            )
            .unwrap();
        assert_eq!(level(&mut scene, &views, -5.0), ["detailed"]);
        // Forced levels win at any distance.
        group(&mut views, 1.0);
        assert_eq!(level(&mut scene, &views, -1.5), ["simple"]);
        assert_eq!(level(&mut scene, &views, -300.0), ["simple"]);
    }

    // M7-CLOUD-02
    #[test]
    fn the_cloud_layer_reaches_cameras_and_drifts_on_the_wind() {
        let n = Value::from;
        let mut scene = ScriptScene::new();
        let mut views = WorldViews::default();
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        scene.scene.update_world();
        views
            .apply(
                "camera",
                &[
                    n(100.0),
                    n(1.0),
                    n(60.0),
                    n(0.1),
                    n(1000.0),
                    Value::Null,
                    n(0.0),
                    Value::Bool(false),
                    n(5.0),
                ],
            )
            .unwrap();
        let clouds = |views: &WorldViews| views.views(&scene, (64, 64), &[])[0].clouds;
        assert_eq!(clouds(&views), None, "none by default");
        let layer = [60.0, 40.0, 0.6, 0.9, 0.4, 0.5, 0.52, 0.55, 2.0, 90.0, 300.0];
        views
            .apply(
                "clouds",
                &[n(0.0), Value::Array(layer.iter().map(|v| n(*v)).collect())],
            )
            .unwrap();
        let c = clouds(&views).expect("a cloud layer");
        assert_eq!(
            (c.altitude, c.size, c.coverage, c.fade),
            (60.0, 40.0, 0.6, 300.0)
        );
        assert_eq!(c.color, [0.5, 0.52, 0.55]);
        // 2 m/s towards +z (90 degrees from +x), for half a second.
        views.advance_clouds(0.5);
        let o = clouds(&views).unwrap().offset;
        assert!(o[0].abs() < 1e-5 && (o[1] - 1.0).abs() < 1e-5, "{o:?}");
        views.apply("clouds", &[n(0.0), Value::Null]).unwrap();
        assert_eq!(clouds(&views), None);
    }

    // M7-FOG-03
    #[test]
    fn fog_reaches_cameras_that_show_it() {
        let n = Value::from;
        let mut scene = ScriptScene::new();
        let mut views = WorldViews::default();
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        scene.scene.update_world();
        let camera = |fog: Option<bool>| {
            let mut args = vec![
                n(100.0),
                n(1.0),
                n(60.0),
                n(0.1),
                n(1000.0),
                Value::Null,
                n(0.0),
                Value::Bool(false),
                n(5.0),
                Value::Null,
                "color".into(),
                Value::Null,
                Value::Null,
                Value::Bool(false),
                "none".into(),
                Value::Null,
                Value::Bool(false),
            ];
            args.extend(fog.map(Value::Bool));
            args
        };
        let fog = |views: &WorldViews| views.views(&scene, (64, 64), &[])[0].fog;
        views.apply("camera", &camera(None)).unwrap();
        assert_eq!(fog(&views), None, "clear air by default");
        let color = Value::Array(vec![n(0.5), n(0.6), n(0.7)]);
        views
            .apply(
                "fog",
                &[
                    n(0.0),
                    "linear".into(),
                    color.clone(),
                    n(0.02),
                    n(10.0),
                    n(80.0),
                ],
            )
            .unwrap();
        // Cameras show it unless they opt out.
        assert_eq!(
            fog(&views),
            Some(Fog {
                mode: FogMode::Linear,
                color: [0.5, 0.6, 0.7],
                density: 0.02,
                start: 10.0,
                end: 80.0,
                height: None,
                volumetric: None,
                cull: true,
            })
        );
        // Height fog comes as a seventh argument (M7-FOG-04).
        let height = Value::Array(vec![n(0.1), n(-2.0), n(0.5)]);
        views
            .apply(
                "fog",
                &[
                    n(0.0),
                    "exponential".into(),
                    color.clone(),
                    n(0.0),
                    n(0.0),
                    n(0.0),
                    height,
                ],
            )
            .unwrap();
        assert_eq!(
            fog(&views).and_then(|f| f.height),
            Some(HeightFog {
                density: 0.1,
                base_height: -2.0,
                falloff: 0.5,
            })
        );
        // Light shafts come as an eighth, clamped (M7-FOG-05).
        let shafts = Value::Array(vec![n(2.0), n(1.5), n(200.0), n(40.0)]);
        views
            .apply(
                "fog",
                &[
                    n(0.0),
                    "exponential".into(),
                    color.clone(),
                    n(0.01),
                    n(0.0),
                    n(0.0),
                    Value::Null,
                    shafts,
                ],
            )
            .unwrap();
        assert_eq!(
            fog(&views).and_then(|f| f.volumetric),
            Some(VolumetricFog {
                intensity: 2.0,
                anisotropy: 0.95,
                steps: 16,
                distance: 40.0,
                albedo: [1.0; 3],
                sharp_distance: 0.0,
                sharp_samples: 32,
                detail: 2,
                denoising: Denoising::Both,
                history_weight: 0.9,
                samples: 1,
                motion_samples: 2,
            })
        );
        views.apply("camera", &camera(Some(false))).unwrap();
        assert_eq!(fog(&views), None);
        views.apply("camera", &camera(Some(true))).unwrap();
        assert!(fog(&views).is_some());
        let bad = views.apply(
            "fog",
            &[n(0.0), "thick".into(), color, n(0.0), n(0.0), n(0.0)],
        );
        assert!(bad.is_err());
        views.apply("fog", &[n(0.0), Value::Null]).unwrap();
        assert_eq!(fog(&views), None);
    }

    // M7-FOG-06
    #[test]
    fn fog_volumes_follow_their_object_and_scale() {
        let n = Value::from;
        let mut scene = ScriptScene::new();
        let mut views = WorldViews::default();
        scene
            .apply("create", &[n(1.0), "camera".into(), Value::Null])
            .unwrap();
        scene
            .apply("create", &[n(2.0), "mist".into(), Value::Null])
            .unwrap();
        // At (3, 1, -4), twice as wide.
        let local = [2.0, 3.0, 1.0, -4.0, 0.0, 0.0, 0.0, 1.0, 2.0, 1.0, 1.0];
        scene
            .apply("locals", &[Value::Array(local.map(n).to_vec())])
            .unwrap();
        scene.scene.update_world();
        let camera = |fog: bool| {
            vec![
                n(100.0),
                n(1.0),
                n(60.0),
                n(0.1),
                n(1000.0),
                Value::Null,
                n(0.0),
                Value::Bool(false),
                n(5.0),
                Value::Null,
                "color".into(),
                Value::Null,
                Value::Null,
                Value::Bool(false),
                "none".into(),
                Value::Null,
                Value::Bool(false),
                Value::Bool(fog),
            ]
        };
        let volumes =
            |views: &WorldViews| views.views(&scene, (64, 64), &[])[0].fog_volumes.clone();
        views.apply("camera", &camera(true)).unwrap();
        views
            .apply(
                "fogVolume",
                &[
                    n(20.0),
                    n(2.0),
                    Value::Array(vec![n(4.0), n(2.0), n(6.0)]),
                    n(0.8),
                    Value::Array(vec![n(0.7), n(0.7), n(0.8)]),
                    n(0.5),
                ],
            )
            .unwrap();
        let v = volumes(&views);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].centre, Vec3::new(3.0, 1.0, -4.0));
        assert_eq!(
            (v[0].density, v[0].color, v[0].blend),
            (0.8, [0.7, 0.7, 0.8], 0.5)
        );
        // Half the size, stretched by the object's scale.
        assert_eq!(v[0].half_extents, Vec3::new(4.0, 1.0, 3.0));
        // Cameras that hide fog hide these too; `remove` drops them.
        views.apply("camera", &camera(false)).unwrap();
        assert!(volumes(&views).is_empty());
        views.apply("camera", &camera(true)).unwrap();
        views.apply("remove", &[n(20.0)]).unwrap();
        assert!(volumes(&views).is_empty());
    }
}
