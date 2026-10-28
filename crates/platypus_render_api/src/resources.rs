// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Texture` and `Mesh`: the assets a host uploads. Scripts make them; every change is queued as a
//! [`RenderCommand`] and the app reads the data with [`texture_info`] / [`mesh_info`] (and tells
//! the engine when a file finished loading with [`texture_loaded`]).

use super::*;
use platypus_engine_api::assets;
use std::collections::HashMap;

numbered_handle!(
    /// A mesh by number (1-based: 0 is "no mesh").
    MeshRef
);
numbered_handle!(
    /// A texture by number (1-based: 0 is "no texture"). The camera API has its own type for the
    /// same numbers (`targetTexture`).
    TextureRef
);

/// Something the renderer has to do about a resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderCommand {
    /// Upload (or re-upload) a texture: read it with [`texture_info`].
    Texture(TextureRef),
    TextureRemove(TextureRef),
    /// Upload a mesh: read it with [`mesh_info`].
    Mesh(MeshRef),
    MeshRemove(MeshRef),
    /// Read a glTF model: [`model_src`] is the file; answer with [`model_loaded`].
    Model(ModelRef),
}

/// A texture: loaded from a file or made from pixels or a render target.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureInfo {
    /// The file it came from, if any.
    pub src: Option<String>,
    /// `point`, `bilinear` or `trilinear`.
    pub filter_mode: String,
    /// `repeat`, `clamp` or `mirror`.
    pub wrap_mode: String,
    pub mipmaps: bool,
    pub aniso_level: u32,
    /// Data, not sRGB colours (normal maps, masks).
    pub linear: bool,
    pub width: u32,
    pub height: u32,
    pub loaded: bool,
    pub failed: bool,
    /// RGBA bytes, the top row first, for a texture made from colours.
    pub pixels: Option<Vec<u8>>,
    /// Cameras draw into it.
    pub render_texture: bool,
}

/// A mesh: a built-in primitive by name, or script data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshInfo {
    pub primitive: Option<String>,
    /// x, y, z per vertex.
    pub vertices: Vec<f64>,
    pub triangles: Vec<u32>,
    pub normals: Vec<f64>,
    /// u, v per vertex, as given (Unity's UVs run bottom-up; the renderer flips them).
    pub uv: Vec<f64>,
    /// Four bone indices per vertex (skinned meshes).
    pub joints: Vec<f64>,
    /// Four weights per vertex.
    pub weights: Vec<f64>,
    /// Each bone's inverse bind pose: 16 numbers, column by column.
    pub bindposes: Vec<f64>,
    /// The engine made it (a model's), so scenes do not save it.
    pub engine: bool,
}

#[derive(Default)]
struct State {
    textures: Vec<Option<TextureInfo>>,
    meshes: Vec<Option<MeshInfo>>,
    commands: Vec<RenderCommand>,
    primitives: HashMap<String, MeshRef>,
    /// `whenLoaded()` promises of textures still loading: (texture, resolve, reject).
    waiters: Vec<TextureWaiter>,
}

type TextureWaiter = (
    u32,
    Callback<(platypus_api::callback::HandleArg<TextureRef>,)>,
    Callback<(String,)>,
);

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

pub(crate) fn reset_resources() {
    st(|s| *s = State::default());
}

/// The resource commands since the last call, in order.
pub fn take_commands() -> Vec<RenderCommand> {
    st(|s| std::mem::take(&mut s.commands))
}

/// Whether textures, meshes or models are waiting for [`take_commands`].
pub fn has_commands() -> bool {
    st(|s| !s.commands.is_empty())
}

pub fn texture_info(t: TextureRef) -> Option<TextureInfo> {
    st(|s| s.textures.get((t.0 as usize).wrapping_sub(1))?.clone())
}

pub fn mesh_info(m: MeshRef) -> Option<MeshInfo> {
    st(|s| s.meshes.get((m.0 as usize).wrapping_sub(1))?.clone())
}

/// Reads a mesh's info without copying it (its vertices can be large).
pub fn read_mesh_info<R>(m: MeshRef, f: impl FnOnce(&MeshInfo) -> R) -> Option<R> {
    st(|s| Some(f(s.meshes.get((m.0 as usize).wrapping_sub(1))?.as_ref()?)))
}

/// A texture finished loading (`width` 0: it failed). Scripts read the size and the flags.
pub fn texture_loaded(t: TextureRef, width: u32, height: u32) {
    with_texture(t, |d| {
        d.failed = width == 0;
        d.loaded = width != 0;
        if width != 0 {
            d.width = width;
            d.height = height;
        }
    });
    // The promises that were waiting hear it (outside the state: they run script code).
    let waiting: Vec<TextureWaiter> = st(|s| {
        let (mine, rest) = std::mem::take(&mut s.waiters)
            .into_iter()
            .partition(|w| w.0 == t.0);
        s.waiters = rest;
        mine
    });
    for (_, resolve, reject) in waiting {
        if width == 0 {
            reject.call(("texture failed to load".to_string(),));
        } else {
            resolve.call((platypus_api::callback::HandleArg(t),));
        }
    }
}

fn with_texture<R>(t: TextureRef, f: impl FnOnce(&mut TextureInfo) -> R) -> Option<R> {
    st(|s| {
        s.textures
            .get_mut((t.0 as usize).checked_sub(1)?)?
            .as_mut()
            .map(f)
    })
}

fn with_mesh<R>(m: MeshRef, f: impl FnOnce(&mut MeshInfo) -> R) -> Option<R> {
    st(|s| {
        s.meshes
            .get_mut((m.0 as usize).checked_sub(1)?)?
            .as_mut()
            .map(f)
    })
}

/// The size of a texture in pixels (0 until it is known).
pub(crate) fn texture_size(t: TextureRef) -> (f64, f64) {
    texture_info(t).map_or((0.0, 0.0), |i| (f64::from(i.width), f64::from(i.height)))
}

const PRIMITIVES: [&str; 6] = ["cube", "sphere", "capsule", "cylinder", "plane", "quad"];

/// The built-in mesh of that name (`cube`, `sphere`, `capsule`, `cylinder`, `plane`, `quad`), made
/// once. An unknown name gives no mesh.
pub fn primitive_mesh(name: &str) -> MeshRef {
    if !PRIMITIVES.contains(&name) {
        eprintln!("render: unknown primitive mesh {name:?}");
        return MeshRef(0);
    }
    if let Some(m) = st(|s| s.primitives.get(name).copied()) {
        return m;
    }
    let m = new_mesh(MeshInfo {
        primitive: Some(name.to_string()),
        ..MeshInfo::default()
    });
    st(|s| s.primitives.insert(name.to_string(), m));
    m
}

/// The name of a built-in mesh.
pub fn primitive_name(m: MeshRef) -> Option<String> {
    read_mesh_info(m, |i| i.primitive.clone())?
}

/// A mesh the engine fills in (a cloth's simulated copy).
pub(crate) fn new_engine_mesh() -> MeshRef {
    new_mesh(MeshInfo {
        engine: true,
        ..MeshInfo::default()
    })
}

fn new_mesh(info: MeshInfo) -> MeshRef {
    st(|s| {
        s.meshes.push(Some(info));
        let m = MeshRef(s.meshes.len() as u32);
        s.commands.push(RenderCommand::Mesh(m));
        m
    })
}

fn new_texture(info: TextureInfo) -> TextureRef {
    st(|s| {
        s.textures.push(Some(info));
        let t = TextureRef(s.textures.len() as u32);
        s.commands.push(RenderCommand::Texture(t));
        t
    })
}

pub(crate) fn push_command(c: RenderCommand) {
    st(|s| s.commands.push(c));
}

/// A texture the host made (a model's): loaded, the size known.
pub(crate) fn new_loaded_texture(width: u32, height: u32) -> TextureRef {
    new_texture(TextureInfo {
        src: None,
        filter_mode: "bilinear".into(),
        wrap_mode: "repeat".into(),
        mipmaps: true,
        aniso_level: 1,
        linear: false,
        width,
        height,
        loaded: true,
        failed: false,
        pixels: None,
        render_texture: false,
    })
}

fn changed_texture(t: TextureRef) {
    st(|s| s.commands.push(RenderCommand::Texture(t)));
}

fn gamma_byte(v: f64, linear: bool) -> u8 {
    let c = v.clamp(0.0, 1.0);
    let c = if linear || c <= 0.0031308 {
        if linear { c } else { c * 12.92 }
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A texture from a file. Settings left out (`""`, NaN) come from the file's `.meta` importer,
/// then the defaults.
pub fn load_texture(
    src: &str,
    filter_mode: &str,
    wrap_mode: &str,
    mipmaps: bool,
    aniso_level: f64,
    linear: bool,
) -> TextureRef {
    let path = assets::resolve(src);
    let imported: Value =
        serde_json::from_str(&assets::assets_api::importer(&path)).unwrap_or_default();
    let text = |given: &str, key: &str, default: &str| -> String {
        if !given.is_empty() {
            given.to_string()
        } else {
            imported
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or(default)
                .to_string()
        }
    };
    let aniso = if aniso_level.is_nan() {
        imported
            .get("anisoLevel")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
    } else {
        aniso_level
    };
    let info = TextureInfo {
        src: Some(path),
        filter_mode: text(filter_mode, "filterMode", "bilinear"),
        wrap_mode: text(wrap_mode, "wrapMode", "repeat"),
        // `true` is the default: the importer may turn them off or on.
        mipmaps: mipmaps
            && imported
                .get("mipmaps")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        aniso_level: aniso.round().clamp(1.0, 16.0) as u32,
        linear: linear
            || imported
                .get("linear")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        width: 0,
        height: 0,
        loaded: false,
        failed: false,
        pixels: None,
        render_texture: false,
    };
    // The same file with the same settings is one texture: a scene loaded again (or two scenes
    // using the same art) doesn't decode and upload it again.
    let same = |t: &TextureInfo| {
        t.src == info.src
            && t.filter_mode == info.filter_mode
            && t.wrap_mode == info.wrap_mode
            && t.mipmaps == info.mipmaps
            && t.aniso_level == info.aniso_level
            && t.linear == info.linear
            && t.pixels.is_none()
            && !t.render_texture
            && !t.failed
    };
    let cached = st(|s| {
        s.textures
            .iter()
            .position(|t| t.as_ref().is_some_and(same))
            .map(|i| TextureRef(i as u32 + 1))
    });
    cached.unwrap_or_else(|| new_texture(info))
}

/// How a texture is sampled between texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Point,
    Bilinear,
    Trilinear,
}

/// What a texture does outside 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapKind {
    Repeat,
    Clamp,
    Mirror,
}

#[platypus_api(
    namespace = "Texture", handles(Texture = TextureRef),
    values(Color = Rgba [r: f64, g: f64, b: f64, a: f64]),
    enums(
        /// How a texture is sampled between texels.
        FilterMode = FilterKind [Point, Bilinear, Trilinear],
        /// What a texture does outside 0 to 1.
        TextureWrapMode = WrapKind [Repeat, Clamp, Mirror]
    )
)]
pub mod texture_api {
    use super::*;

    impl TextureRef {
        /// A promise of the texture once it has loaded (resolved at once when it has); rejects with an
        /// `Error` when loading failed.
        #[promise]
        pub fn when_loaded(
            self,
            resolve: Callback<(platypus_api::callback::HandleArg<TextureRef>,)>,
            reject: Callback<(String,)>,
        ) {
            match texture_info(self) {
                Some(i) if i.failed => reject.call(("texture failed to load".to_string(),)),
                Some(i) if i.loaded => resolve.call((platypus_api::callback::HandleArg(self),)),
                Some(_) => st(|s| s.waiters.push((self.0, resolve, reject))),
                None => reject.call(("the texture was freed".to_string(),)),
            }
        }

        /// Loads a PNG or JPEG relative to the game folder (or `"guid:<guid>"`). Settings not
        /// given come from the file's `.meta` importer, then the defaults (`bilinear`, `repeat`,
        /// mipmaps on, anisotropy 1). The size is known once the host reports it (`isLoaded`).
        #[options(name = "TextureOptions", only)]
        #[optional(filter_mode, wrap_mode, mipmaps = 1, aniso_level, linear)]
        pub fn load(
            src: &str,
            filter_mode: Option<FilterKind>,
            wrap_mode: Option<WrapKind>,
            mipmaps: bool,
            aniso_level: f64,
            linear: bool,
        ) -> TextureRef {
            super::load_texture(
                src,
                filter_mode.map_or("", filter_mode_name),
                wrap_mode.map_or("", texture_wrap_mode_name),
                mipmaps,
                aniso_level,
                linear,
            )
        }

        /// A texture from linear colours, bottom row first like Unity's `SetPixels`
        /// (`width x height` of them).
        #[options(name = "TextureOptions", only)]
        #[optional(filter_mode, wrap_mode, mipmaps = 1, aniso_level, linear)]
        #[allow(clippy::too_many_arguments)]
        pub fn create(
            width: f64,
            height: f64,
            colors: &[Rgba],
            filter_mode: Option<FilterKind>,
            wrap_mode: Option<WrapKind>,
            mipmaps: bool,
            aniso_level: f64,
            linear: bool,
        ) -> TextureRef {
            let (w, h) = (width.max(1.0) as usize, height.max(1.0) as usize);
            if colors.len() != w * h {
                eprintln!("Texture.create: {} colours for {w}x{h}", colors.len());
                return TextureRef(0);
            }
            let mut px = vec![0u8; w * h * 4];
            for y in 0..h {
                // The renderer takes the top row first.
                let row = h - 1 - y;
                for x in 0..w {
                    let c = colors[y * w + x];
                    let i = (row * w + x) * 4;
                    px[i] = gamma_byte(c.r, linear);
                    px[i + 1] = gamma_byte(c.g, linear);
                    px[i + 2] = gamma_byte(c.b, linear);
                    px[i + 3] = (c.a.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
            new_texture(TextureInfo {
                src: None,
                filter_mode: filter_mode_name(filter_mode.unwrap_or(FilterKind::Bilinear)).into(),
                wrap_mode: texture_wrap_mode_name(wrap_mode.unwrap_or(WrapKind::Repeat)).into(),
                mipmaps,
                aniso_level: if aniso_level.is_nan() {
                    1
                } else {
                    aniso_level.round().clamp(1.0, 16.0) as u32
                },
                linear,
                width: w as u32,
                height: h as u32,
                loaded: true,
                failed: false,
                pixels: Some(px),
                render_texture: false,
            })
        }

        /// A texture cameras draw into (Unity's `RenderTexture`): set it as a camera's
        /// `targetTexture`, then show it on any material or sprite.
        pub fn create_render_texture(width: f64, height: f64) -> TextureRef {
            new_texture(TextureInfo {
                src: None,
                filter_mode: "bilinear".into(),
                wrap_mode: "clamp".into(),
                mipmaps: false,
                aniso_level: 1,
                linear: false,
                width: width.round().max(1.0) as u32,
                height: height.round().max(1.0) as u32,
                loaded: true,
                failed: false,
                pixels: None,
                render_texture: true,
            })
        }

        /// Resizes a render texture (cameras drawing into it adapt their aspect).
        pub fn set_size(self, width: f64, height: f64) {
            if with_texture(self, |d| {
                d.width = width.round().max(1.0) as u32;
                d.height = height.round().max(1.0) as u32;
            })
            .is_some()
            {
                changed_texture(self);
            }
        }

        /// The file it came from ("" for a texture made from colours).
        #[property]
        pub fn src(self) -> String {
            texture_info(self).and_then(|i| i.src).unwrap_or_default()
        }

        #[property]
        pub fn width(self) -> f64 {
            texture_size(self).0
        }

        #[property]
        pub fn height(self) -> f64 {
            texture_size(self).1
        }

        /// Whether its size is known.
        #[property]
        pub fn is_loaded(self) -> bool {
            texture_info(self).is_some_and(|i| i.loaded)
        }

        /// True when loading failed (the host logs why).
        #[property]
        pub fn failed(self) -> bool {
            texture_info(self).is_some_and(|i| i.failed)
        }

        /// Holds data rather than sRGB colours (normal maps, masks).
        #[property]
        pub fn linear(self) -> bool {
            texture_info(self).is_some_and(|i| i.linear)
        }

        #[property]
        pub fn is_render_texture(self) -> bool {
            texture_info(self).is_some_and(|i| i.render_texture)
        }

        /// `point`, `bilinear` or `trilinear`.
        #[property]
        pub fn filter_mode(self) -> FilterKind {
            texture_info(self)
                .and_then(|i| filter_mode_parse(&i.filter_mode))
                .unwrap_or(FilterKind::Bilinear)
        }

        /// Changing it uploads the texture again. Another name is ignored.
        pub fn set_filter_mode(self, mode: Option<FilterKind>) {
            if let Some(mode) = mode {
                with_texture(self, |d| d.filter_mode = filter_mode_name(mode).to_string());
                changed_texture(self);
            }
        }

        /// `repeat`, `clamp` or `mirror`.
        #[property]
        pub fn wrap_mode(self) -> WrapKind {
            texture_info(self)
                .and_then(|i| texture_wrap_mode_parse(&i.wrap_mode))
                .unwrap_or(WrapKind::Repeat)
        }

        /// Another name is ignored.
        pub fn set_wrap_mode(self, mode: Option<WrapKind>) {
            if let Some(mode) = mode {
                with_texture(self, |d| {
                    d.wrap_mode = texture_wrap_mode_name(mode).to_string()
                });
                changed_texture(self);
            }
        }

        /// Sharper at grazing angles, 1 to 16.
        #[property]
        pub fn aniso_level(self) -> f64 {
            texture_info(self).map_or(1.0, |i| f64::from(i.aniso_level))
        }

        pub fn set_aniso_level(self, n: f64) {
            with_texture(self, |d| d.aniso_level = n.round().clamp(1.0, 16.0) as u32);
            changed_texture(self);
        }

        /// Frees it; things still using it draw the magenta missing checker.
        pub fn destroy(self) {
            if st(|s| {
                s.textures
                    .get_mut((self.0 as usize).wrapping_sub(1))
                    .and_then(Option::take)
                    .is_some()
            }) {
                st(|s| s.commands.push(RenderCommand::TextureRemove(self)));
            }
        }
    }
}

fn flat3(a: &[V3]) -> Vec<f64> {
    a.iter().flat_map(|v| [v.x, v.y, v.z]).collect()
}

fn flat2(a: &[V2]) -> Vec<f64> {
    a.iter().flat_map(|v| [v.x, v.y]).collect()
}

pub(crate) fn set_mesh_data(
    m: MeshRef,
    vertices: &[V3],
    triangles: &[f64],
    normals: &[V3],
    uv: &[V2],
) {
    with_mesh(m, |d| {
        d.vertices = flat3(vertices);
        d.triangles = triangles.iter().map(|t| *t as u32).collect();
        d.normals = flat3(normals);
        d.uv = flat2(uv);
    });
    st(|s| s.commands.push(RenderCommand::Mesh(m)));
}

/// A mesh from flat lists (x, y, z / u, v per vertex), as `Mesh.createFlat` takes them.
pub(crate) fn set_mesh_flat(
    m: MeshRef,
    positions: &[f64],
    triangles: &[f64],
    normals: &[f64],
    uv: &[f64],
) {
    with_mesh(m, |d| {
        d.vertices = positions.to_vec();
        d.triangles = triangles.iter().map(|t| *t as u32).collect();
        d.normals = normals.to_vec();
        d.uv = uv.to_vec();
    });
    st(|s| s.commands.push(RenderCommand::Mesh(m)));
}

/// The bones that move a vertex: up to four bone indices and their weights.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneInfluence {
    pub indices: Vec<f64>,
    pub weights: Vec<f64>,
}

/// Four bone indices and four weights per vertex (flat), a short list padded with zeros.
fn bone_arrays(bones: &[BoneInfluence]) -> (Vec<f64>, Vec<f64>) {
    let mut joints = Vec::with_capacity(bones.len() * 4);
    let mut weights = Vec::with_capacity(bones.len() * 4);
    for b in bones {
        for k in 0..4 {
            joints.push(b.indices.get(k).copied().unwrap_or(0.0));
            weights.push(b.weights.get(k).copied().unwrap_or(0.0));
        }
    }
    (joints, weights)
}

#[platypus_api(
    namespace = "Mesh", handles(Mesh = MeshRef),
    values(
        Vector3 = V3 [x: f64, y: f64, z: f64],
        Vector2 = V2 [x: f64, y: f64],
        Matrix4x4 = M4 [m0: f64, m1: f64, m2: f64, m3: f64, m4: f64, m5: f64, m6: f64, m7: f64,
                        m8: f64, m9: f64, m10: f64, m11: f64, m12: f64, m13: f64, m14: f64, m15: f64]
    ),
    records(
        /// The bones that move one vertex: up to four bone indices, and a weight for each.
        BoneWeights = BoneInfluence [indices: Vec<f64>, weights: Vec<f64>]
    )
)]
pub mod mesh_api {
    use super::*;

    impl MeshRef {
        /// A triangle mesh made by a script, from an object of its data (the keys are the
        /// parameters). Triangles are index triples, counter-clockwise seen from the front.
        /// vertices: The positions.
        /// triangles: Three vertex indices for each triangle.
        /// normals: One per vertex (made up when left out).
        /// uv: Texture coordinates, one per vertex (made up when left out).
        /// boneWeights: Skinned meshes: the bones that move each vertex.
        /// bindposes: Skinned meshes: each bone's inverse bind pose.
        #[options(name = "MeshData", only, object)]
        #[optional(normals, uv, bone_weights, bindposes)]
        pub fn create(
            vertices: &[V3],
            triangles: &[f64],
            normals: &[V3],
            uv: &[V2],
            bone_weights: &[BoneInfluence],
            bindposes: &[M4],
        ) -> MeshRef {
            let m = super::new_mesh(MeshInfo::default());
            super::set_mesh_data(m, vertices, triangles, normals, uv);
            if !bone_weights.is_empty() {
                let (joints, weights) = super::bone_arrays(bone_weights);
                with_mesh(m, |d| {
                    d.joints = joints;
                    d.weights = weights;
                });
                st(|s| s.commands.push(RenderCommand::Mesh(m)));
            }
            if !bindposes.is_empty() {
                with_mesh(m, |d| {
                    d.bindposes = bindposes.iter().flat_map(|p| p.m).collect();
                });
            }
            m
        }

        /// A mesh from flat number lists, for meshes made in bulk (terrain chunks). A list left
        /// out is empty.
        /// positions: x, y, z per vertex.
        /// triangles: Three vertex indices for each triangle.
        /// normals: x, y, z per vertex.
        /// uv: u, v per vertex, in Unity's bottom-up space.
        #[options(name = "FlatMeshData", only, object)]
        #[optional(normals, uv)]
        #[ts_param(
            positions = "NumberList",
            triangles = "NumberList",
            normals = "NumberList",
            uv = "NumberList"
        )]
        pub fn create_flat(
            positions: &[f64],
            triangles: &[f64],
            normals: &[f64],
            uv: &[f64],
        ) -> MeshRef {
            let m = super::new_mesh(MeshInfo::default());
            super::set_mesh_flat(m, positions, triangles, normals, uv);
            m
        }

        /// Replaces the mesh's data from flat lists (see `createFlat`).
        /// positions: x, y, z per vertex.
        /// triangles: Three vertex indices for each triangle.
        /// normals: x, y, z per vertex.
        /// uv: u, v per vertex, in Unity's bottom-up space.
        #[options(name = "FlatMeshData", only, object)]
        #[optional(normals, uv)]
        #[ts_param(
            positions = "NumberList",
            triangles = "NumberList",
            normals = "NumberList",
            uv = "NumberList"
        )]
        pub fn set_flat(self, positions: &[f64], triangles: &[f64], normals: &[f64], uv: &[f64]) {
            super::set_mesh_flat(self, positions, triangles, normals, uv);
        }

        /// A built-in mesh by name: `cube`, `sphere`, `capsule`, `cylinder`, `plane` or `quad`
        /// (what `mesh: "cube"` means).
        pub fn from_string(name: &str) -> MeshRef {
            super::primitive_mesh(name)
        }

        /// Replaces the mesh's data (everything drawing it updates).
        #[optional(normals, uv)]
        pub fn set(self, vertices: &[V3], triangles: &[f64], normals: &[V3], uv: &[V2]) {
            super::set_mesh_data(self, vertices, triangles, normals, uv);
        }

        /// Skinned meshes: four bone indices and four weights per vertex (flat).
        pub fn set_bone_weights(self, joints: &[f64], weights: &[f64]) {
            with_mesh(self, |d| {
                d.joints = joints.to_vec();
                d.weights = weights.to_vec();
            });
            st(|s| s.commands.push(RenderCommand::Mesh(self)));
        }

        /// Skinned meshes: each bone's inverse bind pose, 16 numbers (column by column) each.
        pub fn set_bindposes(self, matrices: &[f64]) {
            with_mesh(self, |d| d.bindposes = matrices.to_vec());
        }

        #[property]
        pub fn vertex_count(self) -> f64 {
            read_mesh_info(self, |i| (i.vertices.len() / 3) as f64).unwrap_or(0.0)
        }

        #[property]
        pub fn triangle_count(self) -> f64 {
            read_mesh_info(self, |i| (i.triangles.len() / 3) as f64).unwrap_or(0.0)
        }

        /// The name of a built-in mesh ("" for any other).
        #[property]
        pub fn primitive(self) -> String {
            super::primitive_name(self).unwrap_or_default()
        }

        /// Frees it (renderers still using it draw nothing).
        pub fn destroy(self) {
            if st(|s| {
                s.meshes
                    .get_mut((self.0 as usize).wrapping_sub(1))
                    .and_then(Option::take)
                    .is_some()
            }) {
                st(|s| s.commands.push(RenderCommand::MeshRemove(self)));
            }
        }
    }
}

pub(crate) fn resource_manifests() -> Vec<String> {
    vec![
        texture_api::MANIFEST.to_json(),
        mesh_api::MANIFEST.to_json(),
    ]
}

fn numbers_of(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}

fn texture_kind() -> AssetKind {
    AssetKind {
        name: "Texture".into(),
        also: vec!["RenderTexture".into()],
        shared: true,
        create: Box::new(|def| {
            let s = |k: &str| {
                def.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            if def.get("type").and_then(Value::as_str) == Some("RenderTexture") {
                let n = |k: &str| def.get(k).and_then(Value::as_f64).unwrap_or(1.0);
                return Ok(u64::from(
                    TextureRef::create_render_texture(n("width"), n("height")).0,
                ));
            }
            // By GUID when its file is known, so renamed files are still found.
            let mut src = s("src");
            if let Some(g) = def.get("guid").and_then(Value::as_str) {
                let p = assets::assets_api::guid_to_path(g);
                if !p.is_empty() {
                    src = p;
                }
            }
            let t = load_texture(
                &src,
                &s("filterMode"),
                &s("wrapMode"),
                def.get("mipmaps").and_then(Value::as_bool).unwrap_or(true),
                def.get("anisoLevel")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::NAN),
                def.get("linear").and_then(Value::as_bool).unwrap_or(false),
            );
            Ok(u64::from(t.0))
        }),
        describe: Box::new(|h| {
            let i = texture_info(TextureRef(h as u32))?;
            if i.render_texture {
                return Some(
                    json!({ "type": "RenderTexture", "width": i.width, "height": i.height }),
                );
            }
            let src = i.src?;
            let guid = assets::assets_api::path_to_guid(&src);
            let mut m = Map::new();
            m.insert("type".into(), json!("Texture"));
            m.insert("src".into(), json!(src));
            if !guid.is_empty() {
                m.insert("guid".into(), json!(guid));
            }
            m.insert("filterMode".into(), json!(i.filter_mode));
            m.insert("wrapMode".into(), json!(i.wrap_mode));
            m.insert("mipmaps".into(), json!(i.mipmaps));
            if i.aniso_level != 1 {
                m.insert("anisoLevel".into(), json!(i.aniso_level));
            }
            if i.linear {
                m.insert("linear".into(), json!(true));
            }
            Some(Value::Object(m))
        }),
    }
}

fn mesh_kind() -> AssetKind {
    AssetKind {
        name: "Mesh".into(),
        also: Vec::new(),
        shared: true,
        create: Box::new(|def| {
            let m = new_mesh(MeshInfo {
                vertices: numbers_of(def.get("vertices")),
                triangles: numbers_of(def.get("triangles"))
                    .iter()
                    .map(|t| *t as u32)
                    .collect(),
                normals: numbers_of(def.get("normals")),
                uv: numbers_of(def.get("uv")),
                joints: numbers_of(def.get("joints")),
                weights: numbers_of(def.get("weights")),
                bindposes: numbers_of(def.get("bindposes")),
                ..MeshInfo::default()
            });
            Ok(u64::from(m.0))
        }),
        describe: Box::new(|h| {
            let i = mesh_info(MeshRef(h as u32))?;
            if i.primitive.is_some() || i.engine {
                return None;
            }
            let mut m = Map::new();
            m.insert("type".into(), json!("Mesh"));
            m.insert("vertices".into(), json!(i.vertices));
            m.insert("triangles".into(), json!(i.triangles));
            m.insert("normals".into(), json!(i.normals));
            m.insert("uv".into(), json!(i.uv));
            if !i.joints.is_empty() {
                m.insert("joints".into(), json!(i.joints));
                m.insert("weights".into(), json!(i.weights));
            }
            if !i.bindposes.is_empty() {
                m.insert("bindposes".into(), json!(i.bindposes));
            }
            Some(Value::Object(m))
        }),
    }
}

pub(crate) fn register_resource_kinds() {
    platypus_kinds::register_asset(texture_kind());
    platypus_kinds::register_asset(mesh_kind());
}
