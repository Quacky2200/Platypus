# Textures, sprites and models

Code: the native `Texture`, `Model`, `Mesh` and `Material` API (`platypus_render_api`; the `textures.ts`, `models.ts` and
`rendering.ts` of `js/engine` and the `__platypus_textures` / `__platypus_models` calls into JS were removed with QuickJS);
[world_view.rs](../crates/platypus_app/src/world_view.rs) (`texture_op`, `model_op`) and
[models.rs](../crates/platypus_app/src/models.rs) (glTF) and [models_fbx.rs](../crates/platypus_app/src/models_fbx.rs) (FBX) in the app;
[world/texture.rs](../crates/platypus_render/src/world/texture.rs) and
[world/mod.rs](../crates/platypus_render/src/world/mod.rs) (`set_texture`, `set_mesh`) in the renderer.
How these are drawn is in [world-rendering.md](world-rendering.md).

## Textures

A `Texture` is an image the world draws with (not the UI's `image` element, which has its own path in
`platypus_ui`). Scripts make one two ways:

- `Texture.load(src, { filterMode, wrapMode, mipmaps, anisoLevel, linear })` reads a PNG or JPEG from the game folder.
  The same file with the same settings gives the same texture (a scene loaded again shares it; `RDY-TEX-02`). Fully
  transparent pixels are bled on load: they take the average colour of their nearest visible neighbours
  (`platypus_core::bleed_transparent`, alpha untouched), so filtering leaves no pale halo round a sprite (`RDY-RND-02`).
- `Texture.create(width, height, colors, options)` takes linear `Color`s, bottom row first like Unity's
  `SetPixels`. They are converted to sRGB bytes, top row first, before they cross.

Both send `render.texture id src filter wrap mipmaps [width height [pixels...]]`. `src` is `null` for
pixel data. Defaults: bilinear, repeat, mipmaps on, `anisoLevel` 1, `linear` false. `anisoLevel` (1 to 16,
trilinear only) sharpens grazing angles within what `QualitySettings.anisotropicFiltering` allows.
`linear: true` marks data (normal maps, masks) rather than colours: the renderer reads it without sRGB
decoding and averages its mips in linear space. `Texture.whenLoaded()` returns a promise.
Scene files and hot reloads save both kinds with their options: a loaded one by its file, a made one
with its pixels (`Texture._fromBytes` makes it again), so materials keep their textures after a reload.
Only the engine's own textures (a model's) aren't saved on their own; they come back with their model.
A `RenderTexture` is a texture cameras draw into (see [world-rendering.md](world-rendering.md#render-textures)).

**Ids.** Scripts count textures up from 1. The engine's own (a model's) start at `1 << 30`
(`ENGINE_IDS`), so they never meet.

**Loading is synchronous in the app.** `WorldViews::texture_op` reads the file (a relative path, no
`..`, through `platypus_core::read_asset`, i.e. the packs or folder the app mounted, see `docs/platypus-build.md`; `data:` URLs work too), decodes it with the `image` crate to RGBA8 and
queues a `TextureData` in `texture_uploads`. The host collects `(id, width, height)` and
reports them to the `Texture` registry after the batch of ops, which sets `width`, `height` and
`isLoaded` and resolves `whenLoaded`. A failure logs a warning and reports 0 x 0 (`failed`). The size
is also kept in `texture_sizes`, which sprites need before they can be laid out. `Texture.destroy()`
sends `render.textureRemove`, and things still using it draw the checker.

**Upload.** Before each draw the app moves `texture_uploads` into the renderer
(`WorldRenderer::set_texture`). Textures are `Rgba8UnormSrgb`, so sampling decodes to linear; a second
`Rgba8Unorm` view of the same data (`AssetTexture` keeps both) is used when a material reads the texture as
a map. With
`mipmaps` on, the CPU builds every level (`texture::half`): each texel averages up to 2 x 2 texels **in
linear light** (not encoded values), weighted by alpha so see-through texels do not darken their
neighbours. A texture over the GPU's maximum size is refused with a warning. Samplers are shared per
`(filter, wrap, anisotropy)` (`SamplerKey`): `Point` is nearest and crisp, `Bilinear` blends texels but not mip levels,
`Trilinear` blends both; wrap is `Repeat`, `Clamp` or `Mirror`.

**Alpha.** Kept textures hold **straight** alpha; the frame textures of world panels hold premultiplied.
Quads carry a flag (`params.x`) so the shader treats each right.

### The missing-texture checker and the default spot

Two textures exist in every `WorldRenderer`, made in `new`:

- `MISSING_TEXTURE` (`u32::MAX - 1`): a magenta and black 4 x 4 checker (`texture::missing`), crisp and
  repeating. A `WorldTexture::Asset(id)` whose texture was never set, failed to load or was removed is
  drawn with it, so missing assets stand out instead of passing as white. The check is in `render_views`
  (`self.textures.get(&id).map_or(missing, ...)`). Sprites of a texture that failed also show it: the
  app's `mark_missing` gives the failed id an 8 x 8 size so the sprite still lays out.
- `DEFAULT_PARTICLE_TEXTURE` (`u32::MAX`): a white spot fading smoothly to clear at its edge
  (`texture::soft_spot`). Particles and light halos use it when they have no texture of their own.

Neither can be removed.

## Materials and meshes

`Material(color, emission, smoothness, unlit, mainTexture, tiling, offset, metallic)` is a plain value,
Unity's Standard material (metallic workflow). `smoothness` 0..1 is 1 - roughness; `metallic` 0..1 (default
0) makes a metal: tinted reflections and no diffuse (the shading is in
[world-rendering.md](world-rendering.md#meshes-and-lights)). A `MeshRenderer` sends it with its mesh in
`render.meshRenderer` (the last arguments are `castShadows` and `metallic`); the texture goes by id (`null`:
plain colour).

`Material.create({ color, emission, smoothness, metallic, unlit, mainTexture, mainTextureScale,
mainTextureOffset })` builds one from named settings, and `material.with({...})` copies with some changed;
anything left out keeps its default or current value. `withColor` and `withTexture(texture, scale, offset)`
are shortcuts for `with`.

**Maps and modes.** `create` and `with` also take Unity's Standard shader settings: `normalMap` and
`normalScale`, `metallicGlossMap` (metallic in red, smoothness in alpha), `specularColor` (switches to the
specular workflow, and the map then holds specular colour and smoothness), `occlusionMap` and
`occlusionStrength`, `emissionMap`, `detailAlbedoMap` with its own `detailAlbedoMapScale` and
`detailAlbedoMapOffset`, `renderingMode` (`"opaque"`, `"cutout"`, `"fade"`, `"transparent"`, type
`RenderingMode`), `alphaCutoff` and `doubleSided`. Load normal and metallic maps with `linear: true`. They
travel in the last argument of `render.meshRenderer` (`_extras`, an array that is only sent when a value
differs from the default); the app reads it in `material_extras` into `MaterialMaps`, `RenderMode` and the
rest of `MeshInstance`. How they are shaded is in
[world-rendering.md](world-rendering.md#materials). Scenes save a material's extra settings only when
they differ from the defaults, and a texture's `linear` and `anisoLevel`.

`Mesh.create({ positions, normals, uvs, indices })` sends `render.mesh id ...`. `WorldViews::apply`
builds a `MeshData`, completes it (`complete`: smooth normals, zero UVs and tangents when missing), checks it
(`is_valid`) and keeps a copy in `mesh_data` (for mesh colliders) as well as queueing the upload.
`Mesh.set` replaces the data and `Mesh.destroy` removes it (`render.mesh id null`); renderers using a
removed mesh draw nothing. The renderer also keeps a mesh's buffers by id (`set_mesh`, `remove_mesh`).

`Mesh.createFlat({ positions, triangles, normals?, uv? })` and `mesh.setFlat(data)` take flat number lists, plain or typed arrays
(x, y, z per vertex; u, v in Unity's bottom-up space, flipped for the engine). They send the same
`render.mesh` op without making an object per vertex: the fast way for big, rebuilt meshes such as terrain chunks. `setFlat` passes typed arrays straight through to the op and only builds plain arrays when a scene saves the mesh (`Mesh._data` is a getter), so reused `Float32Array` / `Uint32Array` buffers upload cheaply (a chunk went from 1.7 to 0.6 ms).

**Mesh colliders from render meshes.** A `MeshCollider` with `sharedMesh` asks for a collider whose shape is
`renderMesh [mesh id, convex 0/1, scale x y z]`. The app rewrites it in
`WorldViews::resolve_mesh_collider` into a `mesh` or `convex` shape holding that mesh's (scaled)
positions and indices from `mesh_data`, then hands it to physics. So scripts never send big arrays
twice.

## Sprites

`Sprite.create(texture, rect, pivot, pixelsPerUnit)` names part of a texture: `rect` in pixels from the
texture's bottom-left (all of it when null), `pivot` 0..1 across the rect from its bottom-left (where
the object's origin sits), `pixelsPerUnit` texels per metre (default 100). `Sprite.grid(texture,
columns, rows)` cuts a sheet left to right, top to bottom.

`SpriteRenderer` has `sprite`, `color`, `flipX`, `flipY` and `sortingOrder`, and sends `render.sprite`.
In `WorldViews::views`, a sprite becomes a `WorldQuad` once its texture's size is known
(`texture_sizes`). `sprite_quad` scales the unit quad to `rect / pixelsPerUnit`, shifts it so the pivot
is at the object's origin, and works out the UV rectangle in top-down space (a flip makes the UV
size negative). `drawMode` (`simple`, `sliced`, `tiled`) and `size` (metres) change how the quad(s) fill an area
(`sprite_quads` in `world_view.rs`): `sliced` keeps the corners cut by `Sprite.border` (`setBorder`; pixels, shrinking
together in a small area) and stretches the rest into nine parts; `tiled` repeats the tile across `size`, cutting the last
one off (capped in number). Both are saved in scene files (`RDY-TEX-01`). The quad faces the object's +z, is unlit, and sorts with the other see-through things
by `order`, then distance (see [world-rendering.md](world-rendering.md#the-world-pass)): in an
orthographic 2D game `sortingOrder` decides what is on top.

## glTF models

`Model.load(src)` (cached per path) sends `render.model id src` and returns at once. `Model` has
`isLoaded`, `failed`, `whenLoaded()`, `materials` and `meshes` (after loading), and
`instantiate({ position, rotation, scale, parent, colliders })`.

**In the app.** `model_op` calls `models::load` ([models.rs](../crates/platypus_app/src/models.rs)) with
the game folder:

- The `.glb`/`.gltf` file and anything it names (buffers, images) must be inside the game folder
  (`resolve_asset`; `data:` URIs work; URIs are percent-decoded).
- Each material's textures (base colour, emissive, normal, metallic-roughness, occlusion) become textures
  under engine ids, with the sampler's filter and wrap mapped (`Nearest` mag filter is `Point`;
  otherwise trilinear when the min filter has mips). Colours (base, emissive) are sRGB and data (normal,
  metallic-roughness, occlusion) is linear, so one image used both ways is two textures. Textures are
  shared between materials that use the same image and role. An image that cannot be read gives the
  material `MISSING_TEXTURE`.
- Each triangle primitive becomes its own mesh under an engine id (`EngineIds`, counting up from
  `1 << 30`). Other primitive modes and malformed meshes are skipped with a warning. Missing normals and
  UVs are made up. Tangents are read from the file, or made from the UVs (glTF's convention matches ours).
- The result is a description: `{ nodes, roots, materials, skins, animations }`. Nodes carry a decomposed transform,
  children and primitives (`mesh`, `material`, `bounds`); materials carry colour, emission, smoothness
  (1 - roughness), `metallic` (glTF's `metallicFactor`), `unlit` and the texture id and size. Also `alpha` mode (`alphaMode`: opaque, mask or blend),
  `alphaCutoff`, `doubleSided`, and the map ids: `normal` (id, scale), `metallicRoughness`, `occlusion`
  (id, strength) and `emissive`. Only `blend` materials keep their colour alpha.
- Skinned primitives also carry per-vertex `joints` (indices into the node's skin) and `weights` (made
  to sum to 1) in their `MeshData`. A node names its `skin`. `skins` lists each skin's joint nodes and
  `inverseBind` matrices (column-major, 16 numbers per joint, identity when the file has none).
  `animations` lists channels with the node, `path` (`translation`/`rotation`/`scale`),
  `interpolation` (`linear`/`step`/`cubic`) and the `times` and `values` (cubic splines keep in-tangent,
  value, out-tangent per key). Morph target channels are skipped.
- Meshes go to `mesh_uploads` and `mesh_data`, textures to `texture_uploads`, and the description is
  reported to the `Model` registry (nothing if it failed) after the ops batch.

**In scripts.** `instantiate` makes a GameObject named after the
file with the model's nodes as children (each a `GameObject` with the node's transform), and a
`MeshRenderer` per primitive with the right `Material`. In scripts, `mask` becomes `cutout`, `blend` becomes `fade`, and the metallic-roughness map is
read with `gltfPacking` (occlusion red, roughness green, metallic blue). A node with several primitives gets one child
object per part. With `colliders: "mesh"` (exact triangles, for static scenery) or `"convex"` each
renderer also gets a `MeshCollider` with `sharedMesh`. `instantiate` before loading finishes works: the
nodes appear when the model arrives. Skinned parts become a `SkinnedMeshRenderer` (bones from the
skin's joints, bind poses from `inverseBind`) once every bone object exists, and the file's animations
become `Model.animations` (`AnimationClip`s with paths from the copy's root); a copy with any gets an
`Animation` playing the first ([animation-3d.md](animation-3d.md)). The root remembers its source (`_model`) and the nodes are marked
`_modelPart`, so a saved scene stores the model path and rebuilds the nodes on load
([scene-and-objects.md](scene-and-objects.md#scenes)).

## FBX models

`Model.load("x.fbx")` works like a glTF: `models::load` sends a path ending in `.fbx` (any case) to
[models_fbx.rs](../crates/platypus_app/src/models_fbx.rs), which makes the same `Model` (engine ids,
`{ nodes, roots, materials, skins, animations }`), so `instantiate`, colliders, skins and animations
need nothing new. The instance is named after the file without `.fbx`. Binary and ASCII files load
through the `ufbx` crate (MIT OR Unlicense, its C sources built with `cc`; reviewed in
`tools/platypus_notices/src/tables.rs`).

- **Units and axes.** ufbx converts to metres and Y-up, right-handed (`SpaceConversion::ModifyGeometry`):
  no root node is added. A mesh node keeps the file's axes in its geometry and gets a rotation that
  turns them, so a Z-up centimetre file comes in as metres, Y-up. FBX geometric transforms (pivots) go
  into the geometry too.
- **Meshes** are split by material, one engine mesh per material part, made once however many nodes
  use the mesh. Missing normals are generated. V is flipped (FBX's goes up the image, ours down).
- **Materials and textures.** Materials come in the scene's order. A texture is the embedded content,
  else the file the material names, beside the model (an absolute path from the artist's machine
  falls back to its file name beside the model). It must be inside the game folder; one that cannot
  be read gives `MISSING_TEXTURE`.
- **Nodes** are the file's node tree with decomposed transforms.
- **Skins.** One per skin deformer: the joints are the clusters' bones and `inverseBind` comes from
  each cluster's geometry-to-bone matrix. Each vertex keeps its four strongest weights, made to sum
  to 1.
- **Takes.** Each take (animation stack) is baked with `bake_anim` into linear translation, rotation
  and scale keys per node, the same `animations` list a glTF gives.

- **Scale factor.** The `.meta` importer's `scaleFactor` (`{"importer": {"scaleFactor": 0.5}}`, Unity's model
  import scale) scales the model: it is passed to ufbx as the target unit (`1 / scaleFactor` metres per unit).
  A missing or non-positive factor is 1 (`M5-FBX-06`).

Not read: blend shapes, cameras, lights, vertex colours, a second UV set, and `.meta` importer options
for FBX beyond the default model ones and `scaleFactor`.

The test files `cube.fbx` (a 100 cm cube in a Z-up, centimetre file, two materials, one textured with
`cube_tex.png`), and `rig.fbx` (a column skinned to two bones with one take) are written by
`tests/fixtures/assets/make_fbx.py` (ASCII FBX 7.4, no downloads). Tests `M5-FBX-01..05` are in
`models_fbx.rs`.

## The asset database

Every asset in a game folder can have a `<file>.meta` next to it (JSON: `guid` and `importer`).
`platypus-build assets <game> [--prune]` writes them, and `platypus-build dev` runs it before building
(the Bun `scripts/assets.ts` and `scripts/dev.ts` are deleted).

- Assets are found by extension: textures (png, jpg, jpeg, svg, hdr), models (glb, gltf, fbx, bin), audio (wav,
  ogg, mp3, flac), fonts and other JSON data. Code, builds, `node_modules`, `package.json` and other
  config are skipped.
- A new asset gets a fresh GUID and its kind's default importer. Texture defaults are `filterMode`
  `bilinear`, `wrapMode` `repeat`, `mipmaps`, `anisoLevel` 1 and `linear: false`; models `colliders`
  `none`; audio `stream: false`. Existing metas are never rewritten.
- A file moved without its `.meta` keeps its GUID when it is unambiguous (one orphaned meta and one new
  file with that name). Orphaned metas are only reported, and deleted with `--prune`.
- The app reads the metas at start-up with `asset_db::scan` ([asset_db.rs](../crates/platypus_app/src/asset_db.rs))
  and hands `[{ path, guid, importer }]` to the native `Assets` (`native_pump::seed_asset_index`). Unreadable metas are skipped with a warning.
- Programs also have `Assets.exists(path)`, `list(glob)` (a `string[]`) and `packOf(path)` over every
  loadable asset, not only the ones with a `.meta` (`docs/platypus-api.md`).
- In scripts: `Assets.all`, `guidToPath(guid)`, `pathToGuid(path)`, `importer(path)` and `resolve(src)`
  (assets.ts). `Texture.load`, `Model.load` and `new Sound` accept
  `"guid:<guid>"` sources. `Texture.load` takes unset options from the importer, and a `Sound` streams when
  its importer says so. Scenes save the GUID next to the path of textures, sounds and models and load by
  GUID first, so renamed files are still found.

## Hot reload of assets

While a game runs from a folder (not in screenshot runs), an `AssetWatcher`
([reload.rs](../crates/platypus_app/src/reload.rs)) watches it recursively with `notify`, ignoring `build/`,
`node_modules/`, `target/` and hidden folders. `poll` reports files created or modified once they have been
quiet for `SETTLE`, as game paths (`asset_key`: forward slashes, no leading `./`). `App::tick` polls it
and calls `reload_assets`:

- **Textures.** `WorldViews` remembers each file texture's op arguments (`texture_files`). A changed file
  runs the same op again under the same id, so every material using it updates, and scripts hear the new
  size through the same report.
- **Models.** `model_files` keeps the path and engine ids a model got. `EngineIds::record(replay)` hands out
  the old ids first, so meshes and textures keep their ids; extra parts get new ones. Placed copies keep
  their old materials and nodes (place the model again to see new ones).
- **Sounds.** `App::sound_files` keeps the `audio.load` arguments by file; the sound loads again under its
  id. Playbacks already running keep the old audio.
- Then the host calls the `Assets.onChanged(fn)` listeners (`reload_assets`) (it returns a function
  that stops listening), for anything else scripts read from files. A throwing listener does not stop the
  others.

## Cooking

`platypus-app --cook <game folder>` ([cook.rs](../crates/platypus_app/src/cook.rs)) prepares textures and models so the
game loads them without decoding images, making mip levels or parsing glTF. It needs only the folder, not
a build. It logs how many files it cooked, lists the ones that failed, then exits.

- **Textures** (png, jpg, jpeg) are read with their `.meta` importer settings (filter, wrap, mipmaps,
  aniso, linear). Every mip level is made (`platypus_render::mip_chain`), then block-compressed when the
  size is a multiple of 4 both ways: BC1 (8 bytes per 4 x 4 block) when every texel's alpha is 0 or 255,
  BC3 (16 bytes) for smooth alpha. Other sizes stay RGBA8. `encode` and `decode` do the blocks: endpoints
  come from the best diagonal of the block's colour bounding box, texels take the nearest palette entry,
  and BC1 uses its 3-colour mode for cut-outs.
  Transparent pixels are bled before the mips are made. This is cook `VERSION` 2, so older cooked files are redone.
- **Models** (glb, gltf, fbx) are loaded as usual with recorded engine ids, then saved with their meshes,
  cooked textures and description (ids are renumbered on load).
- Files go to `.platypus/cooked/<game path>.tex` and `.model` (`cooked_path`). It is a hidden folder, so the
  asset watcher and the next cook skip it.
- At run time `WorldViews::texture_op` and `model_op` ask `cooked_texture` / `cooked_model` first. A
  cooked file is used only when it is newer than its source and its `.meta` (importer settings such as a model's
  `scaleFactor` change what is cooked), and a texture only when its colour space and
  mipmap setting match the request. The filter, wrap and anisotropy stay the script's. Cooked textures go
  to `cooked_uploads`; the app calls `WorldRenderer::set_cooked_texture`, which uploads the levels as they
  are (rows of whole blocks). It refuses invalid data, and BC formats on a GPU without
  `TEXTURE_COMPRESSION_BC` (`supports_bc`; the device asks for the feature when the adapter has it). Then
  `uncook_texture` decodes the first level to RGBA8 and the normal path makes the mips.

## Limits

- Textures are 2D only, RGBA8 (or BC1/BC3 when cooked), and all mip levels are built on the CPU
  (cooking does it ahead of time).
- Loading blocks the frame it happens in; there is no async or streaming load.
- glTF: base colour, emissive, normal, metallic-roughness and occlusion textures, factors, alpha modes,
  double-sided, skins and node animations. No extensions, morph targets or cameras, and
  only triangle meshes.
- Hot reload of the scripts makes a new `WorldViews`, and scripts load their textures and models again (see
  [scripting.md](scripting.md#hot-reload)). Asset reload keeps ids but not placed copies' materials.
- Cooked textures are BC1/BC3 or RGBA8 only (no BC7 or ASTC), and the encoder is a fast single-pass one.
  Cooked files are not packed into an archive.

## Tests

`M5-TEX-01` and `M5-TEX-02` (texture ops, sprite sizing and pivot) are unit tests in `world_view.rs`;
`M5-GLB-01` (the glTF loader) is in `models.rs` (its render-mesh collider test in `world_view.rs` was removed with
the `physics.collider` op). Goldens `M5-RND-08` cover textured meshes
and sprites and the missing checker. The JS units (`js/engine/test/{textures,models,meshes}.test.ts`) were deleted; the
fixture model is `tests/fixtures/assets/house.glb`. Newer IDs: `M5-AST-01` (the asset watcher in
`reload.rs`; textures and models reloading under their ids in `world_view.rs`), `M5-AST-02` and
`M5-AST-05` (`Assets.onChanged`, GUIDs and importers; the JS test is gone), `M5-AST-03`
(the `.meta` writer, now `platypus-build assets`; its Bun test `scripts/assets.test.ts` is gone), `M5-AST-04` (`asset_db.rs`) and `M5-AST-06` (cooking, in `cook.rs`). Skins and
animations in glTF are `M5-SKN-01` and `-02` ([animation-3d.md](animation-3d.md#tests)).
