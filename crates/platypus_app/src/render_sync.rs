// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Feeds the 3D renderer from what a compiled game left in the `platypus_render_api` registries.
//!
//! A native game writes typed data (cameras,
//! lights, mesh renderers, materials, textures, meshes, sprites, ...). Each frame this turns that
//! data into the same op calls (`WorldViews::apply`, `texture_op`) or, for the many-item kinds
//! (mesh renderers, 2D lines), typed calls (`set_renderer`, `set_stroke`), so there is one renderer
//! path. The sync is incremental: a kind whose registries' versions (`*_version()`, bumped by
//! every mutable access) did not change is skipped; otherwise each component is compared with
//! what was last sent ([`Track`]) and sent again only when it differs, and the ones gone from the
//! registry are dropped. Transforms and active flags are read when drawing, so moves cost
//! nothing here. `PLATYPUS_RENDER_SYNC=full` rebuilds everything every frame instead (the old way, to
//! compare). Native objects get script ids through `ScriptScene::adopt`.
//!
//! Wired here: cameras (and render textures, gizmos), lights, mesh and skinned mesh renderers
//! (primitives, script meshes, model meshes, materials and their maps), sprites, 2D lines, planes,
//! mirrors, reflection probes, LOD groups, line renderers (their tube meshes), particle systems,
//! models (glTF files), textures from files and pixels, `RenderSettings` and `QualitySettings`.
//! What needs the app (world panels, cloth, fonts and stylesheets) comes back in [`Extras`].

use crate::world_view::{Kind, MaterialExtras, Renderer, Skin, StrokeSpec, WorldViews};
use platypus_core::Value;
use platypus_core::glam::Mat4;
use platypus_render::{MaterialMaps, RenderMode, WorldTexture, primitive_id};
use platypus_render_api::MeshRef;
use platypus_scene::ScriptScene;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Mesh and texture ids from this number up were made by the host for a model (`models.rs`):
/// they are not in the registries.
const ENGINE_IDS: u32 = crate::models::ENGINE_IDS;

/// Skinned renderers share the renderer table with mesh renderers: their ids start here.
pub(crate) const SKIN_BASE: u32 = 1 << 24;

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn arr(v: &[f64]) -> Value {
    Value::Array(v.iter().copied().map(n).collect())
}

/// A texture handle as an op argument: 0 is none.
fn tex(id: u32) -> Value {
    if id == 0 {
        Value::Null
    } else {
        n(f64::from(id))
    }
}

/// World colours go through `colorSpace` (see `color_space.rs`).
fn rgba(c: platypus_engine_api::color::Rgba) -> Value {
    arr(&crate::color_space::rgba(c))
}

fn rgb(c: platypus_engine_api::color::Rgba) -> Value {
    arr(&crate::color_space::rgba(c)[..3])
}

/// What the sync remembers between frames.
#[derive(Default)]
struct Cache {
    /// The last presented gizmo frame: camera id and its lines, kept until the next one.
    gizmos: Option<(u32, Vec<f64>)>,
    /// Cloth rebuild counters already handed to the app.
    cloths: HashMap<u32, f64>,
    /// Inverse bind poses of model meshes (flat, 16 per bone), by mesh id.
    bindposes: HashMap<u32, Vec<f64>>,
    settings_version: Option<u64>,
    /// The `PostProcess` registry's version as last sent.
    post_version: Option<u64>,
    quality_version: Option<u64>,
    /// Counts the frames [`resources`] uploaded something (meshes, models: what renderers and
    /// skins may wait for).
    uploads: u64,
    /// What each component was last sent as.
    sent: Sent,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

fn cache<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    CACHE.with(|c| f(&mut c.borrow_mut()))
}

/// Forgets what was sent (a compiled game reloaded: its registries start over).
pub fn reset() {
    cache(|c| *c = Cache::default());
}

/// What the sync leaves to the app: things that live outside the 3D world views.
#[derive(Default)]
pub struct Extras {
    /// `cloth.*` ops (`cloth` to set one up, `remove`), for `ClothSystem::apply`.
    pub cloths: Vec<(String, Vec<Value>)>,
    /// `render.panel` args for every live world panel (the panel's own id first); `None` while
    /// no `WorldPanel` changed.
    pub panels: Option<Vec<Vec<Value>>>,
    /// `render.text` args for every `TextMesh` (ids from [`TEXT_BASE`], in the panels' id space);
    /// `None` while no `TextMesh` changed.
    pub texts: Option<Vec<Vec<Value>>>,
}

/// `TextMesh` components are panels with ids from this number up (above the skinned renderers').
pub const TEXT_BASE: u32 = 1 << 25;

/// Whether a mesh exists (an engine mesh of a model always does).
fn mesh_exists(mesh: u32) -> bool {
    mesh >= ENGINE_IDS || platypus_render_api::read_mesh_info(MeshRef(mesh), |_| ()).is_some()
}

/// The objects whose (first) `MeshRenderer` draws the `quad` primitive.
fn quad_objects() -> std::collections::HashSet<platypus_scene::ObjectId> {
    let mut out = std::collections::HashSet::new();
    let mut seen = std::collections::HashSet::new();
    platypus_render_api::for_each_mesh_renderer(|_, d| {
        if seen.insert(d.object)
            && platypus_render_api::primitive_name(MeshRef(d.mesh)).as_deref() == Some("quad")
        {
            out.insert(d.object);
        }
    });
    out
}

/// A mirror's normal as the `mirror` op takes it: the one it was given, else +z on a quad (it
/// faces its local +z, as the old script `Mirror` had it) and null (the host's +y) otherwise.
fn mirror_normal(d: &platypus_render_api::MirrorData, on_quad: bool) -> Value {
    if d.has_normal {
        arr(&[d.normal.x, d.normal.y, d.normal.z])
    } else if on_quad {
        arr(&[0.0, 0.0, 1.0])
    } else {
        Value::Null
    }
}

/// The mesh argument of a `cloth` op: a primitive's name, or the id of an uploaded mesh.
fn mesh_arg(mesh: u32) -> Option<Value> {
    if !mesh_exists(mesh) {
        return None;
    }
    Some(match platypus_render_api::primitive_name(MeshRef(mesh)) {
        Some(name) => Value::String(name),
        None => n(f64::from(mesh)),
    })
}

/// The renderer's mesh id for a registry mesh: a primitive's built-in id, else the mesh's own.
fn mesh_id(mesh: u32) -> Result<u32, String> {
    if !mesh_exists(mesh) {
        // Not made yet: the renderer shows once it is (a quiet skip, as before).
        return Err(String::new());
    }
    match platypus_render_api::primitive_name(MeshRef(mesh)) {
        Some(name) => primitive_id(&name).ok_or_else(|| format!("no primitive `{name}`")),
        None => Ok(mesh),
    }
}

/// A finite number as the renderer's, else `default` (what the ops do with bad numbers).
fn fin(v: f64, default: f32) -> f32 {
    let v = v as f32;
    if v.is_finite() { v } else { default }
}

/// A texture handle for a material map: 0 is none.
fn map(id: u32) -> Option<WorldTexture> {
    (id != 0).then_some(WorldTexture::Asset(id))
}

/// A mesh renderer as the renderer keeps it: its object, mesh and material (what the
/// `meshRenderer` op reads from its arguments).
fn renderer(
    object: u32,
    mesh: u32,
    m: &platypus_render_api::MaterialData,
    shadow_casting_mode: &str,
) -> Result<Renderer, String> {
    let mode = match m.rendering_mode.as_str() {
        "opaque" => RenderMode::Opaque,
        "cutout" => RenderMode::Cutout,
        "fade" => RenderMode::Fade,
        "transparent" => RenderMode::Transparent,
        other => return Err(format!("unknown rendering mode `{other}`")),
    };
    let c = |v: f64| v as f32;
    // Colours (not other numbers) go through `colorSpace`.
    let col = |v: platypus_engine_api::color::Rgba| {
        let l = crate::color_space::rgba(v);
        [c(l[0]), c(l[1]), c(l[2]), c(l[3])]
    };
    Ok(Renderer {
        object,
        mesh: mesh_id(mesh)?,
        color: col(m.color),
        emission: {
            let e = col(m.emission);
            [e[0], e[1], e[2]]
        },
        smoothness: fin(m.smoothness, 0.5),
        metallic: fin(m.metallic, 0.0).clamp(0.0, 1.0),
        unlit: m.unlit,
        texture: (m.main_texture != 0).then_some(m.main_texture),
        tiling: [
            c(m.main_texture_scale.x),
            c(m.main_texture_scale.y),
            c(m.main_texture_offset.x),
            c(m.main_texture_offset.y),
        ],
        cast_shadows: shadow_casting_mode != "off",
        extras: MaterialExtras {
            maps: MaterialMaps {
                normal: map(m.normal_map),
                normal_scale: fin(m.normal_scale, 1.0),
                metallic_gloss: map(m.metallic_gloss_map),
                gltf_packing: m.gltf_packing,
                occlusion: map(m.occlusion_map),
                occlusion_strength: fin(m.occlusion_strength, 1.0),
                emission: map(m.emission_map),
                detail: map(m.detail_albedo_map),
                detail_tiling: [
                    c(m.detail_albedo_map_scale.x),
                    c(m.detail_albedo_map_scale.y),
                    c(m.detail_albedo_map_offset.x),
                    c(m.detail_albedo_map_offset.y),
                ],
                reflection: None,
            },
            specular: m.has_specular_color.then(|| {
                let s = col(m.specular_color);
                [s[0], s[1], s[2]]
            }),
            mode,
            alpha_cutoff: fin(m.alpha_cutoff, 0.5),
            double_sided: m.double_sided,
        },
    })
}

/// What was last sent for one kind of component: a key per id that says whether it changed,
/// and the registry versions it was synced at (unchanged versions skip the kind).
struct Track<K> {
    seen: Option<[u64; 3]>,
    /// The key last sent per id, and the pass it was last seen in.
    items: HashMap<u32, (K, u64)>,
    pass: u64,
}

impl<K> Default for Track<K> {
    fn default() -> Self {
        Self {
            seen: None,
            items: HashMap::new(),
            pass: 0,
        }
    }
}

impl<K> Track<K> {
    /// Whether the registries this kind follows changed since the last sync (and starts a pass
    /// over them when they did).
    fn stale(&mut self, versions: [u64; 3]) -> bool {
        if self.seen == Some(versions) {
            return false;
        }
        self.seen = Some(versions);
        self.pass += 1;
        true
    }

    /// Sends one component if `same` says its last sent key differs (or it is new): `send`
    /// applies it, and an error (empty: a quiet skip) drops it from the views.
    fn update(
        &mut self,
        views: &mut WorldViews,
        kind: Kind,
        id: u32,
        same: impl FnOnce(&mut K) -> bool,
        key: impl FnOnce() -> K,
        send: impl FnOnce(&mut WorldViews) -> Result<(), String>,
    ) {
        let pass = self.pass;
        if let Some((k, seen)) = self.items.get_mut(&id) {
            *seen = pass;
            if same(k) {
                return;
            }
        }
        match send(views) {
            Ok(()) => {
                self.items.insert(id, (key(), pass));
            }
            Err(e) => {
                if !e.is_empty() {
                    tracing::warn!(target: "render", "{kind:?} {id}: {e}");
                }
                self.items.remove(&id);
                views.remove_component(kind, id);
            }
        }
    }

    /// Drops the components the last pass did not see (their registry entries are gone).
    fn sweep(&mut self, views: &mut WorldViews, kind: Kind) {
        let pass = self.pass;
        self.items.retain(|id, (_, seen)| {
            let keep = *seen == pass;
            if !keep {
                views.remove_component(kind, *id);
            }
            keep
        });
    }
}

/// What every kind of component was last sent as (see [`components`]).
#[derive(Default)]
struct Sent {
    cameras: Track<platypus_engine_api::camera::CameraData>,
    /// `resetExposure()` counters per camera, as last seen.
    exposure_resets: HashMap<u32, f64>,
    lights: Track<platypus_render_api::LightData>,
    fog_volumes: Track<platypus_render_api::FogVolumeData>,
    renderers: Track<(
        platypus_render_api::MeshRendererData,
        platypus_render_api::MaterialData,
    )>,
    skinned: Track<(
        platypus_render_api::SkinnedData,
        platypus_render_api::MaterialData,
    )>,
    sprites: Track<(
        platypus_render_api::SpriteRendererData,
        platypus_render_api::SpriteData,
    )>,
    strokes: Track<platypus_render_api::StrokeData>,
    planes: Track<platypus_render_api::PlaneData>,
    /// The mirror's data and whether it is on a quad (its default normal).
    mirrors: Track<(platypus_render_api::MirrorData, bool)>,
    probes: Track<platypus_render_api::ProbeData>,
    lods: Track<platypus_render_api::LodData>,
    particles: Track<platypus_render_api::ParticleData>,
    panels: Track<()>,
    texts: Track<()>,
    cloths: Track<()>,
}

/// A camera's settings are the same (not the exposure the renderer reads back, which changes
/// every frame and may be NaN).
fn same_camera(
    a: &platypus_engine_api::camera::CameraData,
    b: &platypus_engine_api::camera::CameraData,
) -> bool {
    a.object == b.object
        && a.fov == b.fov
        && a.near == b.near
        && a.far == b.far
        && a.background == b.background
        && a.depth == b.depth
        && a.orthographic == b.orthographic
        && a.orthographic_size == b.orthographic_size
        && a.clear_flags == b.clear_flags
        && a.rect == b.rect
        && a.culling_mask == b.culling_mask
        && a.layer_cull_distances == b.layer_cull_distances
        && a.layer_cull_spherical == b.layer_cull_spherical
        && a.antialiasing == b.antialiasing
        && a.target_texture == b.target_texture
        && a.scene_grid == b.scene_grid
        && a.fog == b.fog
        && a.exposure_resets == b.exposure_resets
}

/// `PLATYPUS_RENDER_SYNC=full`: rebuild every component each frame (the old way, to compare).
fn full_rebuild() -> bool {
    static FULL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FULL.get_or_init(|| std::env::var("PLATYPUS_RENDER_SYNC").is_ok_and(|v| v == "full"))
}

/// Uploads what the game queued (textures, meshes, models) and sends the components that
/// changed since the last frame. Returns what the app has to apply itself.
pub fn sync(views: &mut WorldViews, scene: &mut ScriptScene, root: &Path) -> Extras {
    let mut lap = crate::sync_profile::Lap::start();
    let mut extras = Extras::default();
    resources(views, scene, root);
    lap.mark("resources");
    // Line renderers redraw their tubes (they are mesh renderers on the same objects).
    platypus_render_api::pre_render_lines();
    lap.mark("line renderers");
    if platypus_render_api::has_commands() {
        resources(views, scene, root);
    }
    settings(views);
    lap.mark("settings");
    let mut sent = cache(|c| std::mem::take(&mut c.sent));
    if full_rebuild() {
        views.clear_components();
        // Particle systems keep their particles (and are compared, not rebuilt).
        let particles = std::mem::take(&mut sent.particles);
        sent = Sent {
            particles,
            ..Sent::default()
        };
        sent.particles.seen = None;
    }
    let uploads = cache(|c| c.uploads);
    let cameras_changed = components(views, scene, &mut sent, uploads, &mut extras, &mut lap);
    // Gizmo lines belong to the camera they were presented for, until the next frame of them.
    let frame = platypus_engine_api::gizmos::take_frame();
    if frame.is_some() || cameras_changed {
        if let Some(frame) = frame {
            let mut flat = Vec::with_capacity(frame.lines.len() * 10);
            for l in &frame.lines {
                flat.extend([
                    l.from.x, l.from.y, l.from.z, l.to.x, l.to.y, l.to.z, l.color.r, l.color.g,
                    l.color.b, l.color.a,
                ]);
            }
            cache(|c| c.gizmos = Some((frame.camera.0, flat)));
        }
        if let Some((camera, flat)) = cache(|c| c.gizmos.clone()) {
            // A camera that is gone has nothing to show them on.
            let _ = views.apply("gizmos", &[n(f64::from(camera)), arr(&flat)]);
        }
    }
    for (r, action, count) in platypus_render_api::take_particle_controls() {
        let mut args = vec![n(f64::from(r.0)), Value::String(action)];
        if count > 0.0 {
            args.push(n(count));
        }
        warn(views.apply("particleControl", &args), "particleControl");
    }
    lap.mark("gizmos, particle controls");
    let versions = [
        platypus_render_api::cloths_version(),
        platypus_render_api::mesh_renderers_version(),
        uploads,
    ];
    if sent.cloths.stale(versions) {
        cloths(scene, &mut extras);
    }
    lap.mark("cloths");
    cache(|c| c.sent = sent);
    extras
}

/// Sends the components whose registries changed: per kind, nothing at all while its
/// registries' versions stay the same, else the ones whose data differs from what was sent
/// last, and drops the ones that are gone. Says whether any camera was sent. `uploads` counts
/// [`resources`] frames that uploaded something (renderers wait for their meshes).
fn components(
    views: &mut WorldViews,
    scene: &mut ScriptScene,
    sent: &mut Sent,
    uploads: u64,
    extras: &mut Extras,
    lap: &mut crate::sync_profile::Lap,
) -> bool {
    use platypus_engine_api::camera;
    use platypus_render_api as api;
    let mut cameras_changed = false;
    let t = &mut sent.cameras;
    if t.stale([camera::version(), 0, 0]) {
        let resets = &mut sent.exposure_resets;
        camera::for_each(|r, d| {
            let object = scene.adopt(d.object);
            t.update(
                views,
                Kind::Camera,
                r.0,
                |k| same_camera(k, d),
                || d.clone(),
                |views| {
                    cameras_changed = true;
                    let flags = if d.clear_flags == "skybox" {
                        "skybox"
                    } else {
                        "color"
                    };
                    views.apply(
                        "camera",
                        &[
                            n(f64::from(r.0)),
                            n(f64::from(object)),
                            n(f64::from(d.fov)),
                            n(f64::from(d.near)),
                            n(f64::from(d.far)),
                            rgba(d.background),
                            n(f64::from(d.depth)),
                            Value::Bool(d.orthographic),
                            n(f64::from(d.orthographic_size)),
                            arr(&d.rect),
                            Value::String(flags.into()),
                            n(f64::from(d.culling_mask)),
                            if d.layer_cull_distances.iter().any(|v| *v > 0.0) {
                                arr(&d.layer_cull_distances)
                            } else {
                                Value::Null
                            },
                            Value::Bool(d.layer_cull_spherical),
                            Value::String(d.antialiasing.clone()),
                            if d.target_texture.0 == 0 {
                                Value::Null
                            } else {
                                n(f64::from(d.target_texture.0))
                            },
                            Value::Bool(d.scene_grid),
                            Value::Bool(d.fog),
                        ],
                    )?;
                    // `resetExposure()` calls since the last time.
                    let before = resets.insert(r.0, d.exposure_resets);
                    if before.is_some_and(|b| b != d.exposure_resets) {
                        warn(
                            views.apply("exposureReset", &[n(f64::from(r.0))]),
                            "exposureReset",
                        );
                    }
                    Ok(())
                },
            );
        });
        t.sweep(views, Kind::Camera);
        sent.exposure_resets
            .retain(|id, _| t.items.contains_key(id));
    }
    lap.mark("cameras");
    let t = &mut sent.lights;
    if t.stale([api::lights_version(), 0, 0]) {
        api::for_each_light(|r, d| {
            let object = scene.adopt(d.object);
            t.update(
                views,
                Kind::Light,
                r.0,
                |k| k == d,
                || d.clone(),
                |views| {
                    views.apply(
                        "light",
                        &[
                            n(f64::from(r.0)),
                            n(f64::from(object)),
                            Value::String(d.r#type.clone()),
                            rgb(d.color),
                            n(d.intensity),
                            n(d.range),
                            n(d.spot_angle),
                            n(d.inner_spot_angle),
                            n(d.halo_size),
                            n(d.beam_intensity),
                            Value::String(d.shadows.clone()),
                            n(d.shadow_strength),
                            n(f64::from(d.culling_mask)),
                            n(d.volumetric_intensity.max(0.0)),
                            Value::String(d.spot_shape.clone()),
                            n(d.aspect_ratio.max(1e-3)),
                        ],
                    )
                },
            );
        });
        t.sweep(views, Kind::Light);
    }
    let t = &mut sent.fog_volumes;
    if t.stale([api::fog_volumes_version(), 0, 0]) {
        api::for_each_fog_volume(|r, d| {
            let object = scene.adopt(d.object);
            t.update(
                views,
                Kind::FogVolume,
                r.0,
                |k| k == d,
                || d.clone(),
                |views| {
                    views.apply(
                        "fogVolume",
                        &[
                            n(f64::from(r.0)),
                            n(f64::from(object)),
                            arr(&[d.size.x, d.size.y, d.size.z]),
                            n(d.density.max(0.0)),
                            rgb(d.color),
                            n(d.blend_distance.max(0.0)),
                            Value::String(d.shape.clone()),
                        ],
                    )
                },
            );
        });
        t.sweep(views, Kind::FogVolume);
    }
    lap.mark("lights, fog volumes");
    let materials = api::materials_version();
    let t = &mut sent.renderers;
    if t.stale([api::mesh_renderers_version(), materials, uploads]) {
        api::for_each_mesh_renderer(|r, d| {
            let object = scene.adopt(d.object);
            let found = api::read_material(api::MaterialRef(d.material), |m| {
                t.update(
                    views,
                    Kind::Renderer,
                    r.0,
                    |k| k.0 == *d && k.1 == *m,
                    || (d.clone(), m.clone()),
                    |views| {
                        let renderer = renderer(object, d.mesh, m, &d.shadow_casting_mode)?;
                        views.set_renderer(r.0, renderer, None);
                        Ok(())
                    },
                );
            });
            if found.is_none() {
                // No material: not drawn.
                t.update(
                    views,
                    Kind::Renderer,
                    r.0,
                    |_| false,
                    || unreachable!(),
                    |_| Err(String::new()),
                );
            }
        });
        t.sweep(views, Kind::Renderer);
    }
    lap.mark("mesh renderers");
    let t = &mut sent.skinned;
    if t.stale([api::skinned_mesh_renderers_version(), materials, uploads]) {
        api::for_each_skinned_mesh_renderer(|r, d| {
            let object = scene.adopt(d.object);
            let id = SKIN_BASE + r.0;
            let found = api::read_material(api::MaterialRef(d.material), |m| {
                t.update(
                    views,
                    Kind::Renderer,
                    id,
                    // Bind poses come with the mesh or model: an upload sends it again.
                    |k| k.0 == *d && k.1 == *m,
                    || (d.clone(), m.clone()),
                    |views| {
                        let bindposes = if d.mesh >= ENGINE_IDS {
                            cache(|c| c.bindposes.get(&d.mesh).cloned()).unwrap_or_default()
                        } else {
                            api::read_mesh_info(MeshRef(d.mesh), |i| i.bindposes.clone())
                                .unwrap_or_default()
                        };
                        if bindposes.len() != d.bones.len() * 16 {
                            return Err(String::new());
                        }
                        let bones = d.bones.iter().map(|b| scene.adopt(*b)).collect();
                        let bindposes = bindposes
                            .as_chunks::<16>()
                            .0
                            .iter()
                            .map(|p| Mat4::from_cols_array(&p.map(|v| v as f32)))
                            .collect();
                        let renderer = renderer(object, d.mesh, m, &d.shadow_casting_mode)?;
                        views.set_renderer(id, renderer, Some(Skin { bones, bindposes }));
                        Ok(())
                    },
                );
            });
            if found.is_none() {
                t.update(
                    views,
                    Kind::Renderer,
                    id,
                    |_| false,
                    || unreachable!(),
                    |_| Err(String::new()),
                );
            }
        });
        t.sweep(views, Kind::Renderer);
    }
    lap.mark("skinned renderers");
    let t = &mut sent.sprites;
    if t.stale([api::sprite_renderers_version(), api::sprites_version(), 0]) {
        api::for_each_sprite_renderer(|r, d| {
            let object = scene.adopt(d.object);
            let found = api::read_sprite(api::SpriteRef(d.sprite), |s| {
                t.update(
                    views,
                    Kind::Sprite,
                    r.0,
                    |k| k.0 == *d && k.1 == *s,
                    || (d.clone(), s.clone()),
                    |views| {
                        views.apply(
                            "sprite",
                            &[
                                n(f64::from(r.0)),
                                n(f64::from(object)),
                                n(f64::from(s.texture)),
                                if s.has_rect {
                                    arr(&s.rect)
                                } else {
                                    Value::Null
                                },
                                arr(&[s.pivot.x, s.pivot.y]),
                                n(s.pixels_per_unit),
                                rgba(d.color),
                                Value::Bool(d.flip_x),
                                Value::Bool(d.flip_y),
                                n(f64::from(d.sorting_order)),
                                Value::String(d.draw_mode.clone()),
                                if d.size.x > 0.0 && d.size.y > 0.0 {
                                    arr(&[d.size.x, d.size.y])
                                } else {
                                    Value::Null
                                },
                                if s.border == [0.0; 4] {
                                    Value::Null
                                } else {
                                    arr(&s.border)
                                },
                            ],
                        )
                    },
                );
            });
            if found.is_none() {
                t.update(
                    views,
                    Kind::Sprite,
                    r.0,
                    |_| false,
                    || unreachable!(),
                    |_| Err(String::new()),
                );
            }
        });
        t.sweep(views, Kind::Sprite);
    }
    lap.mark("sprites");
    let t = &mut sent.strokes;
    if t.stale([api::line_renderer2ds_version(), 0, 0]) {
        api::for_each_line_renderer2d(|r, d| {
            let object = scene.adopt(d.object);
            t.update(
                views,
                Kind::Stroke,
                r.0,
                |k| k == d,
                || d.clone(),
                |views| {
                    let points: Vec<[f32; 2]> =
                        d.points.iter().map(|p| [p.x as f32, p.y as f32]).collect();
                    let c = |c: platypus_engine_api::color::Rgba| {
                        crate::color_space::rgba(c).map(|v| v as f32)
                    };
                    if !d.width.is_finite() {
                        return Err("argument 2 must be a number".into());
                    }
                    views.set_stroke(
                        r.0,
                        StrokeSpec {
                            object,
                            width: d.width as f32,
                            color: c(d.color),
                            closed: d.r#loop,
                            points: &points,
                            fill: d.fill_color.map(c),
                            nonzero: d.fill_rule == "nonzero",
                            order: d.sorting_order,
                        },
                    );
                    Ok(())
                },
            );
        });
        t.sweep(views, Kind::Stroke);
    }
    lap.mark("lines 2d");
    let t = &mut sent.planes;
    if t.stale([api::plane_renderers_version(), 0, 0]) {
        api::for_each_plane_renderer(|r, d| {
            let object = scene.adopt(d.object);
            t.update(
                views,
                Kind::Plane,
                r.0,
                |k| k == d,
                || d.clone(),
                |views| {
                    views.apply(
                        "plane",
                        &[
                            n(f64::from(r.0)),
                            n(f64::from(object)),
                            arr(&[d.size.x, d.size.y]),
                            rgba(d.color),
                        ],
                    )
                },
            );
        });
        t.sweep(views, Kind::Plane);
    }
    let t = &mut sent.mirrors;
    // A renderer added or changed after the mirror can change its default normal too.
    if t.stale([api::mirrors_version(), api::mesh_renderers_version(), 0]) {
        let quads = quad_objects();
        api::for_each_mirror(|r, d| {
            let object = scene.adopt(d.object);
            let on_quad = quads.contains(&d.object);
            t.update(
                views,
                Kind::Mirror,
                r.0,
                |k| k.0 == *d && k.1 == on_quad,
                || (d.clone(), on_quad),
                |views| {
                    views.apply(
                        "mirror",
                        &[
                            n(f64::from(r.0)),
                            n(f64::from(object)),
                            mirror_normal(d, on_quad),
                            n(f64::from(d.culling_mask)),
                        ],
                    )
                },
            );
        });
        t.sweep(views, Kind::Mirror);
    }
    let t = &mut sent.probes;
    if t.stale([api::reflection_probes_version(), 0, 0]) {
        api::for_each_reflection_probe(|r, d| {
            let object = scene.adopt(d.object);
            // A probe renders when it is new, changed, or asked to render again.
            let mut again = false;
            t.update(
                views,
                Kind::Probe,
                r.0,
                |k| {
                    again = k.render_requests != d.render_requests;
                    k.render_requests = d.render_requests;
                    k == d
                },
                || d.clone(),
                |views| {
                    views.apply(
                        "probe",
                        &[
                            n(f64::from(r.0)),
                            n(f64::from(object)),
                            arr(&[d.size.x, d.size.y, d.size.z]),
                            n(d.resolution),
                            Value::Bool(d.box_projection),
                            Value::Bool(d.refresh_mode == "everyFrame"),
                            n(f64::from(d.culling_mask)),
                        ],
                    )
                },
            );
            if again && t.items.get(&r.0).is_some_and(|(k, _)| k == d) {
                warn(views.apply("probeRender", &[n(f64::from(r.0))]), "probe");
            }
        });
        t.sweep(views, Kind::Probe);
    }
    let t = &mut sent.lods;
    if t.stale([api::lod_groups_version(), 0, 0]) {
        api::for_each_lod_group(|r, d| {
            let object = scene.adopt(d.object);
            t.update(
                views,
                Kind::Lod,
                r.0,
                |k| k == d,
                || d.clone(),
                |views| {
                    let heights: Vec<f64> = d.lods.iter().map(|l| l.0).collect();
                    let levels = Value::Array(
                        d.lods
                            .iter()
                            .map(|l| Value::Array(l.1.iter().map(|id| n(f64::from(*id))).collect()))
                            .collect(),
                    );
                    views.apply(
                        "lodGroup",
                        &[
                            n(f64::from(r.0)),
                            n(f64::from(object)),
                            n(d.size),
                            arr(&heights),
                            levels,
                            n(f64::from(d.forced)),
                        ],
                    )
                },
            );
        });
        t.sweep(views, Kind::Lod);
    }
    lap.mark("planes, mirrors, probes, lods");
    let t = &mut sent.particles;
    if t.stale([api::particle_systems_version(), 0, 0]) {
        api::for_each_particle_system(|r, d| {
            let object = scene.adopt(d.object);
            let first = !t.items.contains_key(&r.0);
            t.update(
                views,
                Kind::Particles,
                r.0,
                // The counts the renderer reports back are not settings.
                |k| {
                    k.particle_count = d.particle_count;
                    k.alive = d.alive;
                    k == d
                },
                || d.clone(),
                |views| {
                    views.apply(
                        "particles",
                        &[
                            n(f64::from(r.0)),
                            n(f64::from(object)),
                            Value::Array(particle_pairs(d)),
                            Value::Bool(first && d.play_on_awake),
                        ],
                    )
                },
            );
        });
        t.sweep(views, Kind::Particles);
    }
    lap.mark("particles");
    if sent.panels.stale([api::world_panels_version(), 0, 0]) {
        let mut panels = Vec::new();
        api::for_each_world_panel(|r, d| {
            let object = scene.adopt(d.object);
            panels.push(vec![
                n(f64::from(r.0)),
                n(f64::from(object)),
                arr(&[d.width, d.height]),
                n(d.pixels_per_meter),
                n(d.resolution),
                rgba(d.tint),
                Value::Bool(d.interactive),
            ]);
        });
        extras.panels = Some(panels);
    }
    if sent.texts.stale([api::text_meshs_version(), 0, 0]) {
        let mut texts = Vec::new();
        api::for_each_text_mesh(|r, d| {
            let object = scene.adopt(d.object);
            texts.push(vec![
                n(f64::from(TEXT_BASE + r.0)),
                n(f64::from(object)),
                Value::String(d.text.clone()),
                rgba(d.color),
                n(d.font_size),
                if d.font.is_empty() {
                    Value::Null
                } else {
                    Value::String(d.font.clone())
                },
                n(d.font_weight),
                Value::String(d.alignment.clone()),
                n(d.max_width),
                n(d.pixels_per_unit),
                n(d.resolution),
            ]);
        });
        extras.texts = Some(texts);
    }
    lap.mark("panel and text specs");
    cameras_changed
}

/// `RenderSettings` and `QualitySettings`, when they changed.
fn settings(views: &mut WorldViews) {
    let post = platypus_render_api::post_version();
    if cache(|c| c.post_version.replace(post)) != Some(post) {
        views.post.set(platypus_render_api::post_passes());
    }
    let v = platypus_render_api::version();
    let settings_changed = cache(|c| c.settings_version.replace(v)) != Some(v);
    if settings_changed {
        let s = platypus_render_api::settings();
        warn(
            views.apply("ambient", &[n(0.0), rgb(s.ambient_light)]),
            "ambient",
        );
        let sky = match &s.skybox {
            None => vec![n(0.0), Value::Null],
            Some(k) => vec![
                n(0.0),
                rgb(k.zenith),
                rgb(k.horizon),
                rgb(k.ground),
                k.sun_direction
                    .map_or(Value::Null, |d| arr(&[d.x, d.y, d.z])),
                rgb(k.sun_color),
                n(k.sun_size),
                n(s.reflection_intensity),
                Value::Bool(s.ambient_mode == "skybox"),
            ],
        };
        warn(views.apply("sky", &sky), "sky");
        let bloom = match &s.bloom {
            None => vec![n(0.0), Value::Null],
            Some(b) => vec![n(0.0), n(b.threshold), n(b.intensity), n(b.scatter)],
        };
        warn(views.apply("bloom", &bloom), "bloom");
        warn(views.apply("fog", &fog_args(&s.fog)), "fog");
        let clouds = match &s.cloud_layer {
            None => vec![n(0.0), Value::Null],
            Some(c) => vec![
                n(0.0),
                arr(&[
                    c.altitude,
                    c.size,
                    c.coverage,
                    c.opacity,
                    c.softness,
                    c.tint.r,
                    c.tint.g,
                    c.tint.b,
                    c.wind_speed,
                    c.wind_direction,
                    c.fade_distance,
                ]),
            ],
        };
        warn(views.apply("clouds", &clouds), "clouds");
    }
    let v = platypus_engine_api::screen::quality_version();
    let quality_changed = cache(|c| c.quality_version.replace(v)) != Some(v);
    // Post settings follow both: the player's auto exposure switch is a quality setting.
    if settings_changed || quality_changed {
        let s = platypus_render_api::settings();
        let auto = platypus_engine_api::screen::quality().auto_exposure;
        warn(views.apply("post", &post_args(&s, auto)), "post");
    }
    if quality_changed {
        let q = platypus_engine_api::screen::quality();
        warn(
            views.apply(
                "quality",
                &[
                    n(0.0),
                    n(f64::from(q.anti_aliasing)),
                    n(q.render_scale),
                    Value::String(q.shadows.clone()),
                    n(match q.soft_shadow_quality.as_str() {
                        "high" => 3.0,
                        "medium" => 2.0,
                        _ => 1.0,
                    }),
                    n(q.lod_bias),
                    n(f64::from(q.v_sync_count)),
                    Value::String(q.anisotropic_filtering.clone()),
                    n(f64::from(q.global_texture_mipmap_limit)),
                    Value::Bool(q.volumetric_fog),
                    Value::Bool(q.hdr),
                    n(f64::from(q.additional_lights)),
                    Value::Bool(q.additional_light_shadows),
                ],
            ),
            "quality",
        );
        warn(
            views.apply(
                "shadowSettings",
                &[
                    n(0.0),
                    n(q.shadow_distance),
                    n(f64::from(q.shadow_resolution)),
                    n(f64::from(q.shadow_cascades)),
                ],
            ),
            "shadowSettings",
        );
    }
}

/// The `fog` op's arguments for the fog settings (`fog 0 null` clears the air).
fn fog_args(f: &platypus_render_api::Fog) -> Vec<Value> {
    if !f.enabled {
        return vec![n(0.0), Value::Null];
    }
    let height = f
        .height
        .as_ref()
        .map_or(Value::Null, |h| arr(&[h.density, h.base_height, h.falloff]));
    let shafts = f.volumetric.as_ref().map_or(Value::Null, |v| {
        let detail = ["low", "medium", "high", "ultra"]
            .iter()
            .position(|d| *d == v.resolution)
            .unwrap_or(2);
        let denoising = ["none", "reprojection", "gaussian", "both"]
            .iter()
            .position(|d| *d == v.denoising)
            .unwrap_or(3);
        arr(&[
            v.intensity,
            v.anisotropy,
            v.steps,
            v.distance,
            v.albedo.r,
            v.albedo.g,
            v.albedo.b,
            v.sharp_distance,
            v.sharp_samples,
            detail as f64,
            denoising as f64,
            v.history_weight,
            v.samples,
            v.motion_samples,
        ])
    });
    vec![
        n(0.0),
        Value::String(f.mode.clone()),
        rgb(f.color),
        n(f.density),
        n(f.start),
        n(f.end),
        height,
        shafts,
        Value::Bool(f.culling),
    ]
}

/// The `post` op's arguments: post exposure, tonemapping, the auto exposure (when the quality
/// level lets it run), the manual exposure and the blend time.
fn post_args(s: &platypus_render_api::RenderSettingsData, auto_allowed: bool) -> Vec<Value> {
    let auto = match (&s.auto_exposure, auto_allowed) {
        (Some(a), true) => {
            let metering = ["average", "centerWeighted", "spot"]
                .iter()
                .position(|m| *m == a.metering)
                .unwrap_or(1);
            let curve: Vec<(f64, f64)> = a.curve.iter().take(8).copied().collect();
            let mut v = vec![
                a.min_ev,
                a.max_ev,
                a.compensation,
                a.speed_dark_to_light,
                a.speed_light_to_dark,
                metering as f64,
                a.low_percent,
                a.high_percent,
                if a.hold { 1.0 } else { 0.0 },
                a.dark_adaptation,
                a.dark_below,
                curve.len() as f64,
            ];
            for (ev, stops) in curve {
                v.extend([ev, stops]);
            }
            arr(&v)
        }
        _ => Value::Null,
    };
    vec![
        n(0.0),
        n(s.post_exposure),
        Value::String(s.tonemapping.clone()),
        auto,
        s.fixed_exposure.map_or(Value::Null, n),
        n(s.exposure_transition),
    ]
}

/// `[key, value, ...]` of a particle system, as the TypeScript's `pairs()` makes them.
fn particle_pairs(d: &platypus_render_api::ParticleData) -> Vec<Value> {
    let range = |m: [f64; 2]| {
        if m[0] == m[1] { n(m[0]) } else { arr(&m) }
    };
    let mut colors = crate::color_space::rgba(d.start_color).to_vec();
    if let Some(c) = d.start_color2 {
        colors.extend(crate::color_space::rgba(c));
    }
    let shape = match d.shape_type.as_str() {
        "point" => vec![Value::String("point".into())],
        "cone" => vec![
            Value::String("cone".into()),
            n(d.shape_angle),
            n(d.shape_radius),
        ],
        "box" => vec![
            Value::String("box".into()),
            n(d.shape_size.x),
            n(d.shape_size.y),
            n(d.shape_size.z),
        ],
        other => vec![Value::String(other.into()), n(d.shape_radius)],
    };
    let kv = |k: &str, v: Value| [Value::String(k.into()), v];
    let mut out: Vec<Value> = Vec::new();
    for pair in [
        kv("duration", n(d.duration)),
        kv("looping", Value::Bool(d.r#loop)),
        kv("startDelay", n(d.start_delay)),
        kv("lifetime", range(d.start_lifetime)),
        kv("speed", range(d.start_speed)),
        kv("size", range(d.start_size)),
        kv("rotation", range(d.start_rotation)),
        kv("angularVelocity", range(d.angular_velocity)),
        kv("color", arr(&colors)),
        kv("gravity", n(d.gravity_modifier)),
        kv("drag", n(d.drag)),
        kv("worldSpace", Value::Bool(d.simulation_space == "world")),
        kv("maxParticles", n(f64::from(d.max_particles))),
        kv("rate", n(d.rate_over_time)),
        kv(
            "bursts",
            arr(&d.bursts.iter().flatten().copied().collect::<Vec<_>>()),
        ),
        kv("shape", Value::Array(shape)),
        kv("texture", tex(d.texture)),
        kv("additive", Value::Bool(d.blend_mode == "additive")),
        kv("seed", n(f64::from(d.random_seed))),
    ] {
        out.extend(pair);
    }
    if let Some(k) = &d.size_over_lifetime {
        out.extend(kv(
            "sizeOverLifetime",
            arr(&k.iter().flatten().copied().collect::<Vec<_>>()),
        ));
    }
    if let Some(k) = &d.color_over_lifetime {
        let flat: Vec<f64> = k
            .iter()
            .flat_map(|(t, c)| {
                let l = crate::color_space::rgba(*c);
                [*t, l[0], l[1], l[2], l[3]]
            })
            .collect();
        out.extend(kv("colorOverLifetime", arr(&flat)));
    }
    if let Some(s) = d.texture_sheet {
        out.extend(kv("sheet", arr(&s)));
    }
    out
}

/// Cloth: a `cloth` op for every new or rebuilt one, `remove` for the ones that are gone.
fn cloths(scene: &mut ScriptScene, extras: &mut Extras) {
    // A cloth a program made starts when the host first sees it (the TypeScript's `onStart`),
    // once its object has a mesh renderer.
    for (r, d) in platypus_render_api::cloths() {
        if d.simulated_mesh == 0 && !platypus_render_api::mesh_renderers_on(d.object).is_empty() {
            r.rebuild();
        }
    }
    let live = platypus_render_api::cloths();
    let ids: HashSet<u32> = live.iter().map(|(r, _)| r.0).collect();
    let gone: Vec<u32> = cache(|c| {
        let gone = c
            .cloths
            .keys()
            .filter(|k| !ids.contains(k))
            .copied()
            .collect();
        c.cloths.retain(|k, _| ids.contains(k));
        gone
    });
    for id in gone {
        extras
            .cloths
            .push(("remove".into(), vec![n(f64::from(id))]));
    }
    for (r, d) in live {
        if d.simulated_mesh == 0
            || cache(|c| c.cloths.insert(r.0, d.rebuild_requests)) == Some(d.rebuild_requests)
        {
            continue;
        }
        let object = scene.adopt(d.object);
        let source = match mesh_arg(d.source_mesh) {
            Some(m) => m,
            None => continue,
        };

        let mut colliders = Vec::new();
        for o in &d.sphere_colliders {
            let c = platypus_physics_api::SphereColliderRef::find_on(*o);
            if c.0 != 0 {
                colliders.push(Value::Array(vec![
                    Value::String("sphere".into()),
                    n(f64::from(scene.adopt(*o))),
                    n(c.radius()),
                    arr(&[c.center().x, c.center().y, c.center().z]),
                ]));
            }
        }
        for o in &d.capsule_colliders {
            let c = platypus_physics_api::CapsuleColliderRef::find_on(*o);
            if c.0 != 0 {
                colliders.push(Value::Array(vec![
                    Value::String("capsule".into()),
                    n(f64::from(scene.adopt(*o))),
                    n(c.radius()),
                    n(c.height()),
                    arr(&[c.center().x, c.center().y, c.center().z]),
                ]));
            }
        }
        let pins: Vec<f64> = d.pins.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
        let args = vec![
            n(f64::from(r.0)),
            n(f64::from(object)),
            source,
            n(f64::from(d.simulated_mesh)),
            arr(&[
                d.stretching_stiffness,
                d.bending_stiffness,
                d.damping,
                d.friction,
                d.solver_iterations,
            ]),
            arr(&[
                d.external_acceleration.x,
                d.external_acceleration.y,
                d.external_acceleration.z,
            ]),
            arr(&[
                d.random_acceleration.x,
                d.random_acceleration.y,
                d.random_acceleration.z,
            ]),
            Value::Bool(d.use_gravity),
            arr(&pins),
            n(d.pin_radius),
            match d.pin_box {
                Some((a, b)) => arr(&[a.x, a.y, a.z, b.x, b.y, b.z]),
                None => Value::Null,
            },
            Value::Array(colliders),
        ];
        extras.cloths.push(("cloth".into(), args));
    }
}

fn warn(r: Result<(), String>, what: &str) {
    if let Err(e) = r {
        tracing::warn!(target: "render", "{what}: {e}");
    }
}

/// Inverse bind poses of a model's skinned meshes, for [`resources`]: by mesh id.
fn model_bindposes(description: &serde_json::Value) {
    let skins = description["skins"].as_array().cloned().unwrap_or_default();
    let nodes = description["nodes"].as_array().cloned().unwrap_or_default();
    for node in nodes {
        let Some(skin) = node["skin"].as_u64().and_then(|s| skins.get(s as usize)) else {
            continue;
        };
        let inverse: Vec<f64> = skin["inverseBind"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_f64)
            .collect();
        for p in node["primitives"].as_array().into_iter().flatten() {
            if let Some(mesh) = p["mesh"].as_u64() {
                cache(|c| c.bindposes.insert(mesh as u32, inverse.clone()));
            }
        }
    }
}

/// Textures, meshes and models the game made since the last frame.
fn resources(views: &mut WorldViews, scene: &mut ScriptScene, root: &Path) {
    use platypus_render_api::{RenderCommand, TextureRef};
    let commands = platypus_render_api::take_commands();
    if !commands.is_empty() {
        cache(|c| c.uploads += 1);
    }
    for cmd in commands {
        match cmd {
            RenderCommand::Texture(t) => {
                let Some(info) = platypus_render_api::texture_info(t) else {
                    continue;
                };
                if info.render_texture {
                    let args = vec![
                        n(f64::from(t.0)),
                        n(f64::from(info.width)),
                        n(f64::from(info.height)),
                    ];
                    warn(views.apply("renderTexture", &args), "renderTexture");
                    platypus_render_api::texture_loaded(t, info.width, info.height);
                    continue;
                }
                let pixels = info.pixels.as_ref().map_or(Value::Null, |p| {
                    Value::Array(p.iter().map(|b| n(f64::from(*b))).collect())
                });
                let args = vec![
                    n(f64::from(t.0)),
                    info.src.clone().map_or(Value::Null, Value::String),
                    Value::String(info.filter_mode.clone()),
                    Value::String(info.wrap_mode.clone()),
                    Value::Bool(info.mipmaps),
                    n(f64::from(info.width)),
                    n(f64::from(info.height)),
                    pixels,
                    n(f64::from(info.aniso_level)),
                    Value::Bool(info.linear),
                ];
                match views.texture_op("texture", root, &args) {
                    Ok(Some((_, w, h))) => platypus_render_api::texture_loaded(t, w, h),
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!(target: "render", "texture {}: {e}", t.0);
                        views.mark_missing(t.0);
                        platypus_render_api::texture_loaded(t, 0, 0);
                    }
                }
            }
            RenderCommand::TextureRemove(TextureRef(id)) => {
                let _ = views.texture_op("textureRemove", root, &[n(f64::from(id))]);
            }
            RenderCommand::Mesh(m) => {
                let Some(info) = platypus_render_api::mesh_info(m) else {
                    continue;
                };
                // Primitives are built in; an engine mesh (a cloth's copy) is filled in later.
                if info.primitive.is_some() || info.engine {
                    continue;
                }
                let f = |v: &[f64]| arr(v);
                let tri: Vec<f64> = info.triangles.iter().map(|i| f64::from(*i)).collect();
                // Unity's UVs run bottom-up; the renderer's top-down (as `Mesh.set` does).
                let uv: Vec<f64> = info
                    .uv
                    .chunks(2)
                    .flat_map(|t| [t[0], 1.0 - t.get(1).copied().unwrap_or(0.0)])
                    .collect();
                let args = vec![
                    n(f64::from(m.0)),
                    f(&info.vertices),
                    f(&info.normals),
                    f(&uv),
                    f(&tri),
                    f(&info.joints),
                    f(&info.weights),
                ];
                warn(views.apply("mesh", &args), "mesh");
            }
            RenderCommand::MeshRemove(MeshRef(id)) => {
                warn(
                    views.apply("mesh", &[n(f64::from(id)), Value::Null]),
                    "mesh",
                );
            }
            RenderCommand::Model(m) => {
                let Some(src) = platypus_render_api::model_src(m) else {
                    continue;
                };
                let args = [n(f64::from(m.0)), Value::String(src.clone())];
                match views.model_op(root, &args) {
                    Ok(description) => {
                        model_bindposes(&description);
                        // Building the nodes makes GameObjects: the scene must be current.
                        let _scope = platypus_scene::api::enter(&mut scene.scene);
                        platypus_render_api::model_loaded(m, Some(&description.to_string()));
                    }
                    Err(e) => {
                        tracing::warn!(target: "render", "model `{src}`: {e}");
                        let _scope = platypus_scene::api::enter(&mut scene.scene);
                        platypus_render_api::model_loaded(m, None);
                    }
                }
            }
        }
    }
}

/// The components a full rebuild from the registries makes, as text (fresh views, nothing
/// remembered): what the incremental sync's views must match.
#[cfg(test)]
fn full_rebuild_dump(scene: &mut ScriptScene) -> String {
    let mut fresh = WorldViews::default();
    let uploads = cache(|c| c.uploads);
    components(
        &mut fresh,
        scene,
        &mut Sent::default(),
        uploads,
        &mut Extras::default(),
        &mut crate::sync_profile::Lap::start(),
    );
    fresh.components_dump()
}

#[cfg(test)]
mod tests {
    use super::super::native::{Bundle, Native};
    use super::*;
    use web_ui_dom::UiTree;

    // M9-API-1700
    #[test]
    fn the_incremental_sync_matches_a_full_rebuild_under_random_changes() {
        platypus_render_api::reset();
        platypus_engine_api::camera::reset();
        reset();
        let mut scene = ScriptScene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let bundle = Bundle {
            source: "class Chaos extends Component {
                seed = 12345;
                objs: GameObject[] = [];
                mats: Material[] = [];
                lights: Light[] = [];
                shapes: LineRenderer2D[] = [];
                lines: LineRenderer[] = [];
                planes: PlaneRenderer[] = [];
                bursts: ParticleSystem[] = [];
                cam: Camera | null = null;
                rnd(): number {
                  this.seed = (this.seed * 1103515245 + 12345) % 2147483648;
                  return this.seed / 2147483648;
                }
                pick(n: number): number {
                  return Math.floor(this.rnd() * n);
                }
                override onStart(): void {
                  this.cam = new GameObject(\"Cam\").addComponent(Camera, { fieldOfView: 50 });
                  for (let i = 0; i < 3; i++) {
                    this.mats.push(Material.create({ color: new Color(this.rnd(), this.rnd(), this.rnd(), 1) }));
                  }
                }
                override onUpdate(): void {
                  for (let k = 0; k < 4; k++) {
                    this.step();
                  }
                }
                step(): void {
                  const op = this.pick(14);
                  if (op === 0 || this.objs.length === 0) {
                    const go = new GameObject(\"Box\");
                    go.addComponent(MeshRenderer, { mesh: this.rnd() < 0.5 ? \"cube\" : \"sphere\", material: this.mats[this.pick(this.mats.length)] });
                    this.objs.push(go);
                  } else if (op === 1) {
                    const i = this.pick(this.objs.length);
                    this.objs[i].destroy();
                    this.objs.splice(i, 1);
                  } else if (op === 2) {
                    this.mats[this.pick(this.mats.length)].color = new Color(this.rnd(), this.rnd(), this.rnd(), 1);
                  } else if (op === 3) {
                    const r = this.objs[this.pick(this.objs.length)].getComponent(MeshRenderer);
                    if (r !== null) {
                      r.material = this.mats[this.pick(this.mats.length)];
                    }
                  } else if (op === 4) {
                    if (this.lights.length < 2 || this.rnd() < 0.5) {
                      this.lights.push(new GameObject(\"Lamp\").addComponent(Light, { type: \"point\", intensity: this.rnd() * 3 }));
                    } else {
                      const i = this.pick(this.lights.length);
                      this.lights[i].gameObject.destroy();
                      this.lights.splice(i, 1);
                    }
                  } else if (op === 5 && this.lights.length > 0) {
                    this.lights[this.pick(this.lights.length)].intensity = this.rnd() * 5;
                  } else if (op === 6 && this.cam !== null) {
                    this.cam.fieldOfView = 30 + this.rnd() * 60;
                  } else if (op === 7) {
                    if (this.shapes.length < 2) {
                      this.shapes.push(new GameObject(\"Shape\").addComponent(LineRenderer2D, { width: 0.1, loop: true }));
                    }
                    const s = this.shapes[this.pick(this.shapes.length)];
                    s.points = Shapes.circle(new Vector2(this.rnd(), 0), 0.5 + this.rnd(), 3 + this.pick(6));
                    s.fillColor = this.rnd() < 0.5 ? null : new Color(1, 0, 0, 1);
                  } else if (op === 8) {
                    this.objs[this.pick(this.objs.length)].enabled = this.rnd() < 0.5;
                  } else if (op === 9) {
                    this.mats.push(Material.create({ color: new Color(this.rnd(), 1, 1, 1), smoothness: this.rnd() }));
                  } else if (op === 10) {
                    if (this.planes.length < 2) {
                      this.planes.push(new GameObject(\"Plane\").addComponent(PlaneRenderer, { size: new Vector2(1, 1) }));
                    }
                    this.planes[this.pick(this.planes.length)].size = new Vector2(1 + this.rnd(), 1 + this.rnd());
                  } else if (op === 11) {
                    if (this.bursts.length < 2) {
                      this.bursts.push(new GameObject(\"Burst\").addComponent(ParticleSystem, { rateOverTime: 5 }));
                    } else {
                      this.bursts[this.pick(this.bursts.length)].rateOverTime = 1 + this.pick(20);
                    }
                  } else if (op === 12) {
                    if (this.lines.length < 2) {
                      this.lines.push(new GameObject(\"Cable\").addComponent(LineRenderer, { width: 0.05, material: this.mats[0] }));
                    }
                    const l = this.lines[this.pick(this.lines.length)];
                    l.positions = [new Vector3(0, 0, 0), new Vector3(this.rnd(), 1, 0), new Vector3(2, this.rnd(), 0)];
                  } else if (op === 13 && this.lines.length > 0) {
                    const i = this.pick(this.lines.length);
                    this.lines[i].gameObject.destroy();
                    this.lines.splice(i, 1);
                  }
                }
              }
              register(Chaos);"
                .into(),
            ..Default::default()
        };
        let mut native = Native::load(bundle, &mut scene.scene, &mut ui).unwrap();
        let mut views = WorldViews::default();
        for frame in 0..150 {
            native.frame(&mut scene.scene, &mut ui, 0.016, 16.0 * frame as f64);
            sync(&mut views, &mut scene, Path::new("."));
            let incremental = views.components_dump();
            assert_eq!(
                incremental,
                full_rebuild_dump(&mut scene),
                "frame {frame}: the incremental views differ from a full rebuild"
            );
        }
        // Something was drawn and things came and went.
        let dump = views.components_dump();
        assert!(dump.contains("Renderer {"), "{dump}");
    }

    // M9-API-1701
    #[test]
    fn an_unchanged_frame_sends_nothing() {
        platypus_render_api::reset();
        platypus_engine_api::camera::reset();
        reset();
        let mut scene = ScriptScene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let bundle = Bundle {
            source: "class Still extends Component {
                override onStart(): void {
                  new GameObject(\"Cam\").addComponent(Camera, { fieldOfView: 50 });
                  const m = Material.create({ color: new Color(1, 0, 0, 1) });
                  for (let i = 0; i < 20; i++) {
                    new GameObject(\"Box\").addComponent(MeshRenderer, { mesh: \"cube\", material: m });
                  }
                }
              }
              register(Still);"
                .into(),
            ..Default::default()
        };
        let mut native = Native::load(bundle, &mut scene.scene, &mut ui).unwrap();
        let mut views = WorldViews::default();
        native.frame(&mut scene.scene, &mut ui, 0.016, 16.0);
        sync(&mut views, &mut scene, Path::new("."));
        let versions = cache(|c| c.sent.renderers.seen);
        let pass = cache(|c| c.sent.renderers.pass);
        native.frame(&mut scene.scene, &mut ui, 0.016, 32.0);
        sync(&mut views, &mut scene, Path::new("."));
        // The registries did not change: the renderers were not even walked.
        assert_eq!(cache(|c| c.sent.renderers.seen), versions);
        assert_eq!(cache(|c| c.sent.renderers.pass), pass);
        assert_eq!(views.components_dump(), full_rebuild_dump(&mut scene));
    }

    // M9-API-620
    #[test]
    fn a_compiled_games_camera_light_and_cube_reach_the_renderers_views() {
        let mut scene = ScriptScene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let bundle = Bundle {
            source: "class Stage extends Component {\n\
                override onStart(): void {\n\
                  const cam = new GameObject(\"Cam\");\n\
                  cam.addComponent(Camera, { fieldOfView: 50 });\n\
                  cam.transform.position = new Vector3(0, 0, 5);\n\
                  const sun = new GameObject(\"Sun\");\n\
                  sun.addComponent(Light, { type: \"point\", intensity: 2 });\n\
                  const box = new GameObject(\"Box\");\n\
                  box.addComponent(MeshRenderer, { mesh: \"cube\", material: new Material({ color: new Color(1, 0, 0, 1) }) });\n\
                }\n\
              }\n\
              register(Stage);"
                .into(),
            ..Default::default()
        };
        let mut native = Native::load(bundle, &mut scene.scene, &mut ui).unwrap();
        native.frame(&mut scene.scene, &mut ui, 0.016, 16.0);
        let mut views = WorldViews::default();
        sync(&mut views, &mut scene, Path::new("."));
        scene.scene.update_world();
        let v = views.views(&scene, (200, 100), &[]);
        assert_eq!(v.len(), 1, "one camera");
        assert_eq!(v[0].meshes.len(), 1);
        assert_eq!(v[0].meshes[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(v[0].lights.len(), 1);
    }

    // M5-RND-26
    //
    // A `Mirror` without a normal faces its local +z on a quad (the wall in examples/materials) and
    // +y on anything else; a written normal wins; a quad renderer added after the mirror turns it.
    #[test]
    fn a_mirror_on_a_quad_faces_z_unless_given_a_normal() {
        use platypus_core::glam::Vec3;
        platypus_render_api::reset();
        reset();
        let mut scene = ScriptScene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let bundle = Bundle {
            source: "class Stage extends Component {
                late: GameObject | null = null;
                frames = 0;
                override onStart(): void {
                  const wall = new GameObject(\"Wall\");
                  wall.addComponent(MeshRenderer, { mesh: \"quad\" });
                  wall.addComponent(Mirror);
                  const floor = new GameObject(\"Floor\");
                  floor.addComponent(MeshRenderer, { mesh: \"plane\" });
                  floor.addComponent(Mirror);
                  const tilted = new GameObject(\"Tilted\");
                  tilted.addComponent(MeshRenderer, { mesh: \"quad\" });
                  tilted.addComponent(Mirror, { normal: new Vector3(1, 0, 0) });
                  const late = new GameObject(\"Late\");
                  late.addComponent(Mirror);
                  this.late = late;
                }
                override onUpdate(): void {
                  this.frames++;
                  if (this.frames === 2 && this.late !== null) this.late.addComponent(MeshRenderer, { mesh: \"quad\" });
                }
              }
              register(Stage);"
                .into(),
            ..Default::default()
        };
        let mut native = Native::load(bundle, &mut scene.scene, &mut ui).unwrap();
        let mut views = WorldViews::default();
        native.frame(&mut scene.scene, &mut ui, 0.016, 16.0);
        sync(&mut views, &mut scene, Path::new("."));
        let normals = |v: &WorldViews| {
            v.mirror_normals()
                .into_iter()
                .map(|(_, n)| n)
                .collect::<Vec<_>>()
        };
        assert_eq!(normals(&views), [Vec3::Z, Vec3::Y, Vec3::X, Vec3::Y]);
        native.frame(&mut scene.scene, &mut ui, 0.016, 32.0);
        sync(&mut views, &mut scene, Path::new("."));
        assert_eq!(normals(&views), [Vec3::Z, Vec3::Y, Vec3::X, Vec3::Z]);
    }

    // M9-API-700
    #[test]
    fn settings_planes_and_quality_reach_the_renderers_views() {
        platypus_render_api::reset();
        reset();
        let mut scene = ScriptScene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let bundle = Bundle {
            source: "class Stage extends Component {
                override onStart(): void {
                  const cam = new GameObject(\"Cam\");
                  cam.addComponent(Camera, { fieldOfView: 50 });
                  cam.transform.position = new Vector3(0, 0, 5);
                  const p = new GameObject(\"Plane\");
                  p.addComponent(PlaneRenderer, { size: new Vector2(2, 3) });
                  RenderSettings.skybox = { sunDirection: null };
                  RenderSettings.bloom = { threshold: 1, intensity: 0.5, scatter: 0.4 };
                  RenderSettings.tonemapping = \"aces\";
                }
              }
              register(Stage);"
                .into(),
            ..Default::default()
        };
        let mut native = Native::load(bundle, &mut scene.scene, &mut ui).unwrap();
        native.frame(&mut scene.scene, &mut ui, 0.016, 16.0);
        let mut views = WorldViews::default();
        let extras = sync(&mut views, &mut scene, Path::new("."));
        assert!(extras.panels.unwrap_or_default().is_empty());
        scene.scene.update_world();
        let v = views.views(&scene, (200, 100), &[]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].quads.len(), 1, "the plane");
        assert!(views.sky.is_some(), "the skybox");
        assert!(views.bloom.is_some());
        assert_eq!(
            views.grading.tonemapping,
            platypus_render::Tonemapping::Aces
        );
        // Nothing changed: the second sync leaves the settings alone and keeps the plane.
        sync(&mut views, &mut scene, Path::new("."));
        scene.scene.update_world();
        assert_eq!(views.views(&scene, (200, 100), &[])[0].quads.len(), 1);
    }

    // M7-FX-09
    #[test]
    fn post_process_passes_reach_their_cameras_views_in_order() {
        platypus_render_api::reset();
        platypus_engine_api::screen::reset();
        reset();
        let mut scene = ScriptScene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let bundle = Bundle {
            source: "class Stage extends Component {
                override onStart(): void {
                  const a = new GameObject(\"A\").addComponent(Camera, {});
                  const b = new GameObject(\"B\").addComponent(Camera, { depth: 1 });
                  const vignette = PostProcess.create(\"vignette\");
                  vignette.setFloat(\"intensity\", 0.9);
                  const blur = PostProcess.create(\"blur\");
                  blur.order = -1;
                  const mine = PostProcess.createCustom(
                    \"fn effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> { return color * tint; }\",
                    [\"tint:color\"]);
                  mine.setColor(\"tint\", new Color(1, 0.5, 0.25, 1));
                  mine.camera = b;
                  PostProcess.create(\"pixelate\").enabled = false;
                }
              }
              register(Stage);"
                .into(),
            ..Default::default()
        };
        let mut native = Native::load(bundle, &mut scene.scene, &mut ui).unwrap();
        native.frame(&mut scene.scene, &mut ui, 0.016, 16.0);
        let mut views = WorldViews::default();
        sync(&mut views, &mut scene, Path::new("."));
        scene.scene.update_world();
        let v = views.views(&scene, (200, 100), &[]);
        assert_eq!(v.len(), 2);
        let names = |view: &platypus_render::WorldView| -> Vec<String> {
            view.post.iter().map(|e| e.shader.name.clone()).collect()
        };
        // The disabled pass is left out; blur runs first by its order; the custom one is aimed
        // at the second camera only.
        assert_eq!(names(&v[0]), ["blur", "vignette"]);
        assert_eq!(names(&v[1]), ["blur", "vignette", "custom"]);
        assert_eq!(v[0].post[1].params[0][0], 0.9);
        assert_eq!(v[1].post[2].params[0], [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(v[1].post[2].shader.params[0].0, "tint");
        // The renderer's shader is the same one next frame (its pipeline stays cached).
        let id = v[0].post[0].shader.id;
        native.frame(&mut scene.scene, &mut ui, 0.016, 32.0);
        sync(&mut views, &mut scene, Path::new("."));
        assert_eq!(
            views.views(&scene, (200, 100), &[])[0].post[0].shader.id,
            id
        );
    }

    // M9-API-1501
    #[test]
    fn fog_exposure_lights_and_fog_volumes_reach_the_renderers_views() {
        platypus_render_api::reset();
        platypus_engine_api::screen::reset();
        reset();
        let mut scene = ScriptScene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let bundle = Bundle {
            source: "class Stage extends Component {
                override onStart(): void {
                  const cam = new GameObject(\"Cam\");
                  const c = cam.addComponent(Camera, { fieldOfView: 50 });
                  c.fog = false;
                  cam.transform.position = new Vector3(0, 0, 5);
                  RenderSettings.fog = true;
                  RenderSettings.fogMode = \"linear\";
                  RenderSettings.fogStartDistance = 2;
                  RenderSettings.fogEndDistance = 50;
                  RenderSettings.fogColor = new Color(0.5, 0.6, 0.7, 1);
                  const low = true;
                  RenderSettings.heightFog = low ? { density: 0.1, baseHeight: 1, falloff: 0.3 } : null;
                  RenderSettings.volumetricFog = { intensity: 0.5, steps: 8, albedo: new Color(1, 0.5, 0.25, 1) };
                  RenderSettings.autoExposure = { minEV: -4, compensation: 1, metering: \"spot\" };
                  RenderSettings.fixedExposure = 12;
                  RenderSettings.exposureTransition = 2;
                  const lamp = new GameObject(\"Lamp\");
                  lamp.addComponent(Light, { type: \"spot\", spotShape: \"pyramid\", aspectRatio: 3, volumetricIntensity: 0.25 });
                  const mist = new GameObject(\"Mist\", { position: new Vector3(3, 1, -4), scale: new Vector3(2, 1, 1) });
                  mist.addComponent(FogVolume, { size: new Vector3(4, 2, 4), density: 0.7, shape: \"ellipsoid\" });
                }
              }
              register(Stage);"
                .into(),
            ..Default::default()
        };
        let mut native = Native::load(bundle, &mut scene.scene, &mut ui).unwrap();
        native.frame(&mut scene.scene, &mut ui, 0.016, 16.0);
        let mut views = WorldViews::default();
        sync(&mut views, &mut scene, Path::new("."));
        scene.scene.update_world();
        let v = views.views(&scene, (200, 100), &[]);
        assert_eq!(v.len(), 1);
        // The camera opted out of fog (and of the fog volumes); the other cameras would show it.
        assert_eq!(v[0].fog, None);
        assert!(v[0].fog_volumes.is_empty());
        let fog = views.fog_for_test().expect("fog");
        assert_eq!(fog.mode, platypus_render::FogMode::Linear);
        assert_eq!((fog.start, fog.end), (2.0, 50.0));
        let height = fog.height.expect("height fog");
        assert_eq!((height.density, height.base_height), (0.1, 1.0));
        let shafts = fog.volumetric.expect("light shafts");
        assert_eq!((shafts.intensity, shafts.steps), (0.5, 8));
        assert_eq!(shafts.albedo, [1.0, 0.5, 0.25]);
        let auto = views.grading.auto_exposure.expect("auto exposure");
        assert_eq!((auto.min_ev, auto.compensation), (-4.0, 1.0));
        assert_eq!(auto.metering, platypus_render::Metering::Spot);
        assert_eq!(views.grading.fixed_ev, Some(12.0));
        assert_eq!(views.grading.transition, 2.0);
        assert_eq!(v[0].lights.len(), 1);
        assert_eq!(v[0].lights[0].volumetric, 0.25);
        assert!(matches!(
            v[0].lights[0].kind,
            platypus_render::LightKind::Pyramid { aspect, .. } if aspect == 3.0
        ));
        // The player's quality switch turns auto exposure off without the game changing anything.
        platypus_engine_api::screen::quality_api::set_auto_exposure(false);
        sync(&mut views, &mut scene, Path::new("."));
        assert!(views.grading.auto_exposure.is_none());
        assert_eq!(views.grading.fixed_ev, Some(12.0));
        // Setting the fog to nothing clears the air.
        platypus_render_api::render_settings_api::set_fog(false);
        sync(&mut views, &mut scene, Path::new("."));
        assert!(views.fog_for_test().is_none());
    }

    // M7-CLOUD-04
    #[test]
    fn a_compiled_games_cloud_layer_reaches_the_views_with_its_defaults() {
        platypus_render_api::reset();
        platypus_engine_api::screen::reset();
        reset();
        let mut scene = ScriptScene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let bundle = Bundle {
            source: "class Stage extends Component {
                override onStart(): void {
                  new GameObject(\"Cam\").addComponent(Camera);
                  RenderSettings.cloudLayer = { altitude: 40, coverage: 0.55, tint: new Color(0.6, 0.62, 0.66, 1), windSpeed: 2, windDirection: 90 };
                }
              }
              register(Stage);"
                .into(),
            ..Default::default()
        };
        let mut native = Native::load(bundle, &mut scene.scene, &mut ui).unwrap();
        native.frame(&mut scene.scene, &mut ui, 0.016, 16.0);
        let mut views = WorldViews::default();
        sync(&mut views, &mut scene, Path::new("."));
        scene.scene.update_world();
        let c = views.views(&scene, (64, 64), &[])[0]
            .clouds
            .expect("a cloud layer");
        // Given keys, and the defaults for the rest.
        assert_eq!((c.altitude, c.coverage), (40.0, 0.55));
        assert_eq!((c.size, c.opacity, c.fade), (2000.0, 1.0, 20000.0));
        assert_eq!(c.color, [0.6, 0.62, 0.66]);
        views.advance_clouds(0.5);
        let o = views.views(&scene, (64, 64), &[])[0].clouds.unwrap().offset;
        assert!(o[0].abs() < 1e-5 && (o[1] - 1.0).abs() < 1e-5, "{o:?}");
        platypus_render_api::render_settings_api::set_cloud_layer(None);
        sync(&mut views, &mut scene, Path::new("."));
        assert_eq!(views.views(&scene, (64, 64), &[])[0].clouds, None);
    }

    // M9-API-1511, M9-TS-2887
    #[test]
    fn filled_shapes_text_meshes_and_a_spline_rider_reach_the_renderer() {
        platypus_render_api::reset();
        platypus_engine_api::screen::reset();
        reset();
        let mut scene = ScriptScene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let bundle = Bundle {
            source: "class Show extends Component {
                override onStart(): void {
                  new GameObject(\"Cam\", { position: new Vector3(0, 0, 10) }).addComponent(Camera, { orthographic: true, orthographicSize: 5 });
                  const shape = new GameObject(\"Shape\", { position: new Vector3(1, 2, 0) }).addComponent(LineRenderer2D, { width: 0.1, loop: true });
                  shape.points = Shapes.rectangle(0, 0, 2, 1);
                  shape.fillColor = new Color(1, 0, 0, 0.5);
                  shape.fillRule = \"nonzero\";
                  shape.sortingOrder = 5;
                  const bare = new GameObject(\"Bare\").addComponent(LineRenderer2D);
                  bare.points = Shapes.circle(new Vector2(0, 0), 1, 8);
                  const rider = new GameObject(\"Rider\");
                  const a = rider.addComponent(SplineAnimate);
                  a.spline = new Spline([new Vector3(0, 0, 0), new Vector3(4, 0, 0)]);
                  a.duration = 4;
                  a.elapsed = 1;
                  a.playing = false;
                  a.place();
                  const label = new GameObject(\"Label\", { position: new Vector3(0, -1, 0) }).addComponent(TextMesh, { text: `x ${rider.transform.position.x.toFixed(2)}`, fontSize: 20, color: new Color(0, 1, 0, 1), font: \"Inter\", maxWidth: 300 });
                }
              }
              register(Show);"
                .into(),
            ..Default::default()
        };
        let mut native = Native::load(bundle, &mut scene.scene, &mut ui).unwrap();
        native.frame(&mut scene.scene, &mut ui, 0.016, 16.0);
        let mut views = WorldViews::default();
        let extras = sync(&mut views, &mut scene, Path::new("."));
        scene.scene.update_world();
        // The text mesh goes out as a `render.text` spec in the panels' id space; the rider went a
        // quarter of the way along 4 units.
        let texts = extras.texts.expect("text meshes were made");
        assert_eq!(texts.len(), 1);
        let args = &texts[0];
        assert_eq!(args[0].as_f64(), Some(f64::from(TEXT_BASE + 1)));
        assert_eq!(args[2].as_str(), Some("x 1.00"));
        assert_eq!(args[4].as_f64(), Some(20.0));
        assert_eq!(args[5].as_str(), Some("Inter"));
        assert_eq!(args[8].as_f64(), Some(300.0));
        // Shapes: the filled one carries its fill, rule and order into the view; the other none.
        let lines = platypus_render_api::line_renderer2ds();
        assert_eq!(lines.len(), 2);
        let filled = lines
            .iter()
            .find(|(_, d)| d.fill_color.is_some())
            .expect("a filled shape");
        assert_eq!(filled.1.fill_rule, "nonzero");
        assert_eq!(filled.1.sorting_order, 5);
        assert_eq!(filled.1.points.len(), 4);
        assert!(
            !lines
                .iter()
                .any(|(_, d)| d.fill_color.is_none() && d.sorting_order != 0)
        );
        let v = views.views(&scene, (200, 100), &[]);
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].strokes.len(),
            4 + 7,
            "a closed rectangle and an open octagon"
        );
        assert_eq!(v[0].shapes.len(), 2);
        assert_eq!(
            v[0].fills.len(),
            6,
            "two triangles for the filled rectangle, none for the other"
        );
    }
}
