# World rendering

Code: [platypus_render/src/world](../crates/platypus_render/src/world/mod.rs) (the GPU pass),
[platypus_app/world_view.rs](../crates/platypus_app/src/world_view.rs) (cameras and planes from scripts),
[platypus_app/panels.rs](../crates/platypus_app/src/panels.rs) (world panels), the drawing and input routing
in [platypus_app/main.rs](../crates/platypus_app/src/main.rs), and
`render_sync.rs` / `native_render.rs` (native registries turned into `WorldViews` ops each frame). The old
script side (`js/engine/src/rendering.ts` and the other `@platypus/engine` files, `render.*` ops sent through
`__platypus_op`, the `__platypus_*` calls back into JS) was deleted with QuickJS on 2026-10-28; the `render.*` ops
below are now the internal vocabulary between the native registries and `WorldViews`. Milestones M5.a and M5.a2. Objects and
components are in [scene-and-objects.md](scene-and-objects.md); the 2D UI renderer is in
[rendering.md](rendering.md). Textures, sprites and glTF models are in
[assets-and-models.md](assets-and-models.md); particles are in [particles.md](particles.md).

The world holds lit meshes (built-in primitives, script meshes, model meshes), quads (planes, sprites,
world panels, particles, halos) and lights. There is no ECS or scene graph on the GPU side: each frame
the app builds a flat `WorldView` per camera.

## The world pass

`WorldRenderer` ([world/mod.rs](../crates/platypus_render/src/world/mod.rs), shader `world.wgsl`) draws
`WorldView`s into a `RenderTarget`:

- `WorldView { camera: WorldCamera, viewport: [x, y, w, h], quads: Vec<WorldQuad>, meshes:
  Vec<MeshInstance>, lights: Vec<Light>, ambient, skybox, clip_plane, probes, fxaa, fog, target_texture, grid, lines, strokes, fills, shapes }`.
  The viewport is normalized, from the top-left of the target. `target_texture` names a render texture
  to draw into instead of the screen (see Render textures). `grid` and `lines` are the editor's ground grid
  and gizmo lines (see Editor grid and gizmo lines). `strokes` are thick 2D lines and `fills` their filled shapes, and `shapes` the per-shape ranges of both (see Thick 2D lines).
- `WorldCamera { view_proj, position, forward, orthographic, clear }`. `WorldCamera::new(world,
  projection, near, far, size, clear)` builds it from the camera's world matrix and a viewport `size`
  in pixels (for the aspect). `Projection` is `Perspective { fov_y_degrees }` or `Orthographic {
  half_height }`.
- `WorldQuad { model, color, texture, uv, additive, order, layer }`. The unit quad (-0.5..0.5 in x and y,
  facing +z) is placed by `model`. `color` is linear straight-alpha RGBA that multiplies the texture.
  `uv` is the part of the texture shown. `additive` adds light instead of covering (glows). `order` is
  the sorting order (see below). `layer` (0 to 31) is the object's layer, used by culling masks.
- `MeshInstance { mesh, model, color, emission, smoothness, metallic, unlit, texture, tiling, glow,
  cast_shadows, mirror, layer, maps, specular, mode, alpha_cutoff, double_sided }`. `mesh` is a
  built-in primitive id or a mesh set with `set_mesh`. `color` is linear straight-alpha RGBA;
  `emission` is light the mesh gives off (it lights nothing else); `smoothness` 0..1 is
  1 - roughness and `metallic` 0..1 is Unity's metallic workflow (see Shading); `unlit` ignores lights; `tiling` is scale (xy) and offset (zw), Unity's
  `mainTextureScale`/`mainTextureOffset`. `glow` is `MeshGlow::None`, `Additive` or `Beam`. The material fields are under Materials.
- `WorldTexture` is how quads and meshes name a texture: `Frame(i)` indexes the `textures` slice given
  to the render call (world panels, drawn fresh each frame, premultiplied alpha), and `Asset(id)` is a
  texture the renderer keeps (`set_texture`, straight alpha). `None` is plain colour. `Reflection(i)` is
  set by the renderer itself for a mirror (see Planar mirrors).

`render_views(gpu, target, views, textures)` first updates reflection probes and renders mirrors (below),
then calls `render_views_with`, which does one render pass per target:

1. The target is cleared to black. Parts no view covers stay black.
2. **Cameras draw in order**, which the app sorts by `depth` (lower first), so a higher camera draws
   over a lower one, like Unity's camera depth.
3. For each view, the pass sets the viewport and scissor to its pixel rect (`viewport_px`), then
   draws a fullscreen triangle with the **background fill** (`fs_fill`): the camera's `clear` colour,
   or the sky along each pixel's view ray when the view has `skybox` and there is a sky, replaces the
   colour in the viewport (no blending, depth compare `Always`) and writes depth 1.0. So a later
   camera fully replaces what was under its viewport, and its depth starts fresh.
4. **Opaque meshes** (not `blended()`: no glow, and mode `Opaque` with colour alpha 1, or `Cutout`) are sorted by `(mesh, texture)` and drawn instanced:
   consecutive instances of one mesh and texture are one draw call. Depth test `Less`, depth writes on.
5. **See-through things** (blended meshes: `Fade` or `Transparent` mode, alpha below 1 or a glow; and all quads) are drawn after, back to
   front in one list: first by `order` (lower first; meshes are order 0), then by distance. In a
   perspective view distance is straight-line (squared) to the camera; in an orthographic view it is
   the distance along the view direction, so everything at one depth is equally far, like Unity's 2D
   sorting. Blending is premultiplied alpha.
   - Quads and see-through meshes depth test with `LessEqual` and write depth, so layered sprites at the
     same z do not fight. Culling is off: planes show from both sides.
   - **Glow** (additive quads, `MeshGlow::Additive`, `Beam`) uses its own pipelines with **no depth
     writes**: they light what is behind them and hide nothing. A quad's colour is stored
     premultiplied with zero coverage, so blending just adds it.

Consecutive items with the same mesh or texture and kind draw in one call.

### Thick 2D lines

`WorldView.strokes: Vec<Stroke { a, b, width, color }>` holds one capsule per segment; `width` is in
world units. They come from `LineRenderer2D` through `render.stroke`. One instance per segment: `vs_stroke`
lays a quad along the segment, square to the camera's forward axis, a pixel wider than the line (the pixel
size comes from unprojecting at each end). `fs_stroke` cuts a capsule by signed distance, so edges are
smooth without MSAA and joins and caps are round. Strokes are depth-tested and blended before the
see-through things (so sprites, text and panels show over them) and before the gizmo lines (`M5-RND-24`). Known limits: where see-through segments
overlap they look darker; the width is not scaled by the object.

**Fills.** `WorldView.fills: Vec<FillVertex { position, color }>` holds triangles (three vertices each) in
world space ([fill.rs](../crates/platypus_render/src/world/fill.rs)). `tessellate_fill(points, FillRule)` turns a
closed outline into triangles with lyon. Points that are not finite are skipped, and fewer than three
leave nothing. `FillRule` is `EvenOdd` (the default, GDI's `Alternate`: a crossing star's middle is a hole)
or `NonZero` (GDI's `Winding`: the star is solid). `WorldViews` tessellates when the `render.stroke` op
arrives (in the object's local space) and moves the triangles through the object's matrix each frame. The
fills are drawn by `vs_shape` / `fs_shape` with the strokes, each fill just before its outline, so an outline sits on its fill. Fill
edges are only as smooth as MSAA makes them (an outline hides them). A stroke `width` of 0 draws no
outline, only the fill (`M5-RND-25`).

**Draw order.** `WorldView.shapes: Vec<ShapeRun { fills, strokes }>` gives each shape's range of `fills`
(vertices) and `strokes` (segments). Shapes draw one at a time in that order, each fill then its outline, so a
later shape covers an earlier one whole. Empty `shapes` draws every fill, then every stroke. All of it comes
before the see-through things and after the opaque meshes (still depth-tested). `render.stroke` takes a
final `sortingOrder`; `WorldViews::views` sorts shapes by it, then by component id (creation order), and
builds one run per shape for each camera (`M5-RND-25`).

### Editor grid and gizmo lines

Two optional parts of a view, for the editor's Scene view. Both are left out of mirror and probe renders.

- **Grid** (`WorldView.grid`, from `Camera.sceneGrid`). Drawn after the opaque meshes and before the
  see-through things: a fullscreen triangle (`fs_grid`) that intersects each pixel's view ray with the
  plane y = 0, so there is no geometry. Lines every 1 m and every 10 m are anti-aliased with `fwidth`
  of the plane coordinates, fade with distance, and the X axis is red and the Z axis blue. The shader
  writes `frag_depth` of the hit point, so meshes hide the grid by the normal depth test (`Less`);
  depth is not written, so things drawn after it are unaffected. Rays that miss the plane are discarded.
- **Lines** (`WorldView.lines: Vec<GizmoLine { a, b, color }>`, from `render.gizmos`). Drawn last in
  the view, over everything. One instanced vertex buffer holds the segments; `vs_line` expands each
  to a screen-space quad of constant width (so lines stay thin at any distance), clipping the segment
  to the near plane first. Two pipelines draw the same lines: depth `LessEqual` at full alpha, and
  `Greater` at 0.25 of the alpha, so a line behind a mesh shows faintly. Neither writes depth.

Only the screen's last pass is the **final pass** (`render_views_with(..., final_pass)`): bloom, exposure,
tonemapping and FXAA run there. Mirror, probe and render texture renders are ungraded and unsmoothed.

One quad storage buffer and one mesh-instance buffer hold every view's data. A uniform buffer holds a
`GpuGlobals` per view (`view_proj`, background, shadow matrix and settings), picked with a dynamic
offset, and a lights buffer is bound per view. Group 1 is the material (`material_group`, one bind group
per `MaterialKey`): the main texture and sampler, then the normal, metallic, occlusion, emission, detail
and reflection textures (8 bindings; missing maps bind a 1 texel default). Group 2 holds the shadow
arrays, the sky cube and the shadow matrices. Buffers grow to a power of two. `GpuGlobals` also holds
`inv_view_proj` and the viewport (for the skybox), `env` (sky present, intensity, last mip, ambient from
sky), `flags` (draw the sky; target size) and `clip` (the clip plane). Per mesh instance, `GpuMesh`
carries `metallic` and the mirror flag (in `params`) and the probe it uses. The depth textures are
`Depth32Float`, kept per size in a map (the screen, mirrors and probe faces; cleared when it holds
more than 8). `viewport_size(rect, size)` gives a
viewport's pixel size, for a camera's aspect. `render(...)` is the single-camera shortcut.

A 0x0 target returns early.

## Meshes and lights

**Meshes.** [mesh.rs](../crates/platypus_render/src/world/mesh.rs): `MeshData { positions, normals, uvs,
tangents, indices }`. Meshes are right-handed, y up, counter-clockwise front faces, with Unity's sizes (a 1 m
cube, a sphere of radius 0.5, a 2 m capsule, a 10 m plane). `primitive(name)` builds `cube`, `sphere`,
`capsule`, `cylinder`, `plane` and `quad`; every renderer registers them at `PRIMITIVE_BASE +
index` (`primitive_id`). Script and model meshes use ids below `PRIMITIVE_BASE`. `MeshData::complete`
makes up smooth normals, zero UVs and tangents when they are missing (tangents point along increasing u,
`w` gives the bitangent's side, `M5-MAT-01`), and `is_valid` refuses indices that are out
of range or not whole triangles. `beam()` is the open cone for spot-light beams (`BEAM_MESH`). Mesh UVs
run top-down; the shader flips them so tiling works bottom-up like Unity's.

**Shading.** `vs_mesh`/`fs_mesh` in `world.wgsl`. The surface colour is `texture * colour`; opaque meshes
ignore the texture's alpha, see-through ones use it. Shading is metallic-roughness PBR, Cook-Torrance
like Unity's Standard shader:

- `roughness = 1 - smoothness` (clamped to at least 0.045), `f0 = mix(0.04, colour, metallic)`, and the
  diffuse albedo is `colour * (1 - metallic)`. So a metal has tinted reflections and no diffuse.
- Every light in the view adds `(kd + spec * PI) * light colour * N.L`. `spec` is `ggx` (normal
  distribution) times `smith` (Schlick geometry, Unreal's k for direct light) times `fresnel`
  (Schlick) over `4 N.V N.L`; `kd = (1 - fresnel) * albedo`. Diffuse is not divided by pi (light
  intensities are Unity's), so the specular is scaled up by pi to match.
- `LightKind::Directional` (parallel rays along `direction`), `Point` (fades out at `range`), `Spot`
  (a cone with soft edges between an inner and outer angle) and `Pyramid` feed that sum. A `Pyramid` is
  HDRP's pyramid spot: `outer_degrees` across its height, `aspect` times as wide (in tangent), edges
  softened from `inner_degrees`, its width along `right` (the light object's +x, set by the app). A thin
  wide one is a sheet of light (a scanner, a slot). `spot_falloff` in `world.wgsl` gives the cone or
  pyramid falloff, `spot_bound_cos` the cone round it, and `spot_axes` the two axes across the direction
  (the same in `mod.rs`) that the pyramid's turn is measured from. `GpuLight.spot` carries cosines in x/y,
  the aspect in z (0: a cone) and the turn in w.
- Light from all around comes from the environment (below): `albedo * ambient_light(n)` for the diffuse
  and `env_brdf_approx(f0, roughness, N.V) * reflected` for the specular. `env_brdf_approx` is Karis's
  analytic fit of the split-sum environment BRDF, so there is no lookup texture.
- `emission` is added after lighting. Back faces are lit with the flipped normal.
- Shadows scale each light's `N.L` term (see Shadows).

### Skinned meshes

`MeshData` can also hold `joints` and `weights` (four per vertex). A mesh with them gets a second vertex
buffer of `SkinVertex { joints: [u32; 4], weights: [f32; 4] }` (attributes 4 and 5). `MeshInstance::bones`
is `Some((first, count))` for a skinned renderer, pointing into `WorldView::bones`: one matrix per bone,
the bone's world matrix times its inverse bind pose, kept by `WorldViews`' bone cache
(`platypus_app/src/bone_cache.rs`) from the renderer's `Skin`
([animation-3d.md](animation-3d.md#skinned-meshes) has the script side). The list is an `Arc<Vec<Mat4>>`
shared by every view; a skeleton's run is worked out again only when one of its bones moved since it was
built (`Scene::moved_since`: `update_world` moves `Scene::epoch` on, a move stamps the object and all below
it, `M5-OBJ-11`), so a culled crowd costs one check per bone (`M5-CUL-07`).

- The main pass has four extra pipelines (`skinned`: opaque or see-through, one- or two-sided) using
  `vs_mesh_skinned` with the normal `fs_mesh`. Bone matrices go in a storage buffer (binding 4 of the
  mesh group). Only the runs of skinned meshes some pass draws (a view, a mirror, a probe, the shadow maps)
  go in it, each once a frame (`world/bone_pack.rs`: placed by list and run the first time a pass draws one,
  converted then, and each pass uploads only what was placed since; `WorldStats::bones` counts them); the
  instance's `maps.w` carries the packed first bone plus one (0: a rigid mesh).
- `skin_matrix` blends the four bone matrices by weight. The object's own `model` matrix is ignored:
  bones are already in world space.
- The shadow pass has `vs_shadow_skinned` and reads the same packed list, so skinned casters bend the shadow.
  Skinned cut-outs cast solid shadows. Skinned and rigid casters are drawn in separate runs.
- Skinned meshes are frustum culled in every view by their bind-pose box under every bone
  (`WorldRenderer::world_box`; the mesh bounds would be wrong, as bones move away from them). The boxes are cached
  per frame in `skin_boxes` and shared by the shadow pass, `note_seen` and each view (`M5-CUL-05`).
- Renderers with identical bones and bind poses (a crowd of one skeleton) share one run of bone matrices in
  `views()` (`M5-SKN-05`).
- The golden is `M5-RND-21`.

### Materials

`MaterialMaps` and the other `MeshInstance` fields follow Unity's Standard shader. Maps are
`WorldTexture`s, each optional:

- **Normal map** (`normal`, `normal_scale`): tangent space, green up, read as linear data. `vs_mesh` builds
  the tangent frame from the mesh's tangents; `fs_mesh` perturbs the normal.
- **Metallic and smoothness** (`metallic_gloss`): metallic in red and smoothness in alpha. The material's
  metallic is replaced by the map's and its smoothness scales the map's. With `gltf_packing` it is read
  the glTF way: occlusion in red, roughness in green, metallic in blue, multiplying the factors.
- **Occlusion** (`occlusion`, `occlusion_strength`): darkens sky light and reflections only, not direct light.
- **Emission** (`emission`) multiplies the emission colour.
- **Detail** (`detail`, `detail_tiling`) is multiplied over the albedo (x2 in gamma space, so mid grey
  changes nothing) with its own tiling.
- **Specular workflow**: `specular: Some(rgb)` is the head-on reflectance, `metallic` is ignored, the
  albedo is darkened by the specular's strength, and the metallic map is read as sRGB colour (rgb) plus
  smoothness (a). `MaterialKey.metallic_srgb` picks that view.
- **Rendering mode** (`RenderMode`): `Opaque` (a colour alpha below 1 still draws as `Fade`), `Cutout`
  (holes where alpha is below `alpha_cutoff`; the shadow pass has its own entry points,
  `vs_shadow_cutout` / `fs_shadow_cutout`, so shadows get the holes), `Fade` (everything fades) and
  `Transparent` (highlights and reflections stay, for glass). `double_sided` draws back faces too.

Textures keep two views: `Rgba8UnormSrgb` for colours and a linear `Rgba8Unorm` view of the same data
for maps, chosen per binding. A texture's `srgb` flag (false for `linear` textures) also makes the CPU
mip chain average in linear space. Samplers are keyed by filter, wrap and anisotropy (`SamplerKey`).

**Lights from scripts.** `Light` sends `render.light component object type [r g b] intensity range
spotAngle innerSpotAngle haloSize beamIntensity shadows(none|hard|soft) shadowStrength cullingMask volumetricIntensity spotShape(cone|pyramid) aspectRatio`. `Light.spotShape` and `aspectRatio` (default 1) make a spot light a pyramid, `spotAngle` high. Colour is
multiplied by intensity before it reaches `Light`. A light shines along its object's -z, like
Unity's. `RenderSettings.ambientLight` sends `render.ambient` (the default is a dim blue-grey).

**Halos.** With `haloSize` above 0 the app adds an additive quad at the light that faces each camera,
using the soft spot texture (`DEFAULT_PARTICLE_TEXTURE`) in the light's hue at steady brightness. It is
pulled half its size towards the camera, so nearby walls do not slice it in two.

**Beams.** With `beamIntensity` above 0 on a spot light, the app adds a `BEAM_MESH` instance with
`MeshGlow::Beam`: a cone as long as the range and as wide as the outer angle, unlit and additive. The
shader (`m.params.y > 1.5`) fades it where the surface turns away from the eye and towards the open end.

## Sky and environment

`WorldRenderer::set_sky(Option<Sky>)` and [environment.rs](../crates/platypus_render/src/world/environment.rs)
with `env.wgsl`. `Sky { zenith, horizon, ground, sun_direction, sun_color, sun_size }` is a procedural
sky in linear HDR colours (the sun is usually brighter than 1). Scripts set it with
`RenderSettings.skybox` (`ProceduralSky`, all fields optional), which sends `render.sky`; `null` removes it.

- **The map.** When the sky changes, the next render redraws it (`redraw_sky`) into a fresh `EnvMap`: a
  128-texel `Rgba16Float` cube with a full mip chain. `EnvPass::draw_sky` draws `fs_sky` into each face
  (`sky_color`: horizon-to-zenith gradient, a soft step down to the ground, a sun disc with a soft edge and
  a glow) and `blur_mips` fills mips 1.. with a 4-tap box from the mip above, each face on its own (seams
  blur a little differently at face edges). Without a sky the renderer holds a 1-texel black cube, and
  `env.x` is 0.
- **Background.** A camera with `clearFlags: "skybox"` (`render.camera` last argument `skybox`; the
  default `solidColor` sends `color`) sets `WorldView.skybox`. `fs_fill` unprojects each pixel with
  `inv_view_proj` and samples mip 0 along that ray. With no sky it falls back to the camera colour.
- **Cloud layer.** `WorldView.clouds` (`CloudLayer { altitude, size, coverage, opacity, softness, color, offset, fade }`, HDRP's
  cloud layer; `RenderSettings.cloudLayer` sends `render.clouds`) is drawn by `fs_fill` over the background colour or sky
  (`with_clouds`): each ray looking up meets the deck at `altitude`, five octaves of value noise there (scaled by `size`, shifted
  by the wind's `offset`) give cover past `1 - coverage` with soft edges, thick parts a little darker underneath, fading as
  `exp(-t / fade)` along the ray. It is part of the background, so distance fog leaves it alone and height fog covers it.
  `WorldViews::advance_clouds(dt)` moves `offset` by the wind each frame (`M7-CLOUD-01`..`04`).
- **Reflections.** `environment(dir, roughness, ...)` samples the cube along the reflected view
  direction at `lod = sqrt(roughness) * last_mip`, so rough surfaces see the blurry mips. Without a sky it
  returns the view's ambient colour. Everything is multiplied by `environment_intensity`
  (`RenderSettings.reflectionIntensity`).
- **Sky light.** `ambient_light(n)` samples the blurriest mip (about the average sky) along the normal
  instead of the flat `ambient` colour, when there is a sky and `ambient_from_sky` is on
  (`RenderSettings.ambientMode`, `"skybox"` by default; `"color"` keeps `ambientLight`). The app copies
  `sky` and `environment` from `WorldViews` to `WorldRenderer` before each draw (`main.rs`).
- The cube is bound in group 2 with the shadow map; only a probe changes it (see Reflection probes).

## Planar mirrors

A `Mirror` component on an object with a `MeshRenderer` sends `render.mirror component object [nx ny nz]`
(the local normal: with no normal set, +z on a quad `MeshRenderer` and +y otherwise; a written normal wins, `M5-RND-26`). `WorldViews::views` turns it into
`MeshInstance.mirror = Some(Mirror { normal, point })` in world space (the object's matrix applied to the
normal and its origin).

`render_views` handles mirrors before the real pass. The **main view** (biggest viewport) is the only one
reflected:

1. For each mirror mesh in it, `reflection_view` clones the view with: the camera moved by the
   reflection matrix `x - 2 (n . x + d) n`; a clip plane `[n, d + 1e-3]` so only what is in front of the
   mirror is drawn (the hair hides the seam); the projection flipped left to right (`flip * view_proj *
   reflect`) because a reflection reverses triangle winding and the flip reverses it back, so culling
   still works; and all mirror meshes removed (no mirrors in mirrors).
2. `render_views_with` draws that view into its own `Rgba16Float` texture (`reflection_target(k)`,
   one per mirror, the size of the target; bloom is paused). The clip is `clipped()` in the shader, a
   `discard` for any fragment with `dot(clip.xyz, p) + clip.w < 0`.
3. The mirror mesh gets `WorldTexture::Reflection(k)` and `params.w = 1`. In `fs_mesh` its colour is
   white times the mesh colour (the tint), and the specular `reflected` term is read from that texture at
   the **screen position**, mirrored in x within the viewport (`viewport.x * 2 + viewport.z - pos.x`),
   because the reflection was rendered flipped. The usual Fresnel and metallic terms still apply.
4. Other views have `mirror` cleared, so they see the sky (or probe) in the mirror instead.

Each mirror costs one more render of the scene. The mirror must be a flat plane through the object's origin.

## Reflection probes

A `ReflectionProbe` component sends `render.probe component object [sx sy sz] resolution boxProjection
everyFrame` (the size is halved into `half_extents`; resolution is clamped to 16..1024). `WorldViews::views`
gives every camera the active probes as `ReflectionProbe { id, position, half_extents, resolution,
box_projection, render }`. `render` is true when `everyFrame` is on or the probe was asked for:
adding or re-sending the op and `renderProbe()` (`render.probeRender`) put it in `probe_requests`, and
the app clears that set after each draw (`probes_rendered`). So `refreshMode: "viaScripting"` renders
when added and on `renderProbe()`.

- **Capture.** `update_probes` runs first in `render_views`, using the main view's probe list. Probes no
  longer listed lose their cubemap. A probe is captured when it has no map of the right size or `render`
  is set: six views of the main view's world from `position` (90 degree perspective, near 0.05, far 1000,
  `face_camera(face)` for direction and up) are each drawn into one shared `Rgba16Float` 2D target per
  face size. `EnvPass::copy_face` copies it into the cube face, flipping x (`fs_copy_flip`: a camera's
  image and a cube face are mirror images). Then `blur_mips` fills the chain. Probe views have no probes
  and no mirrors, so they see the sky there.
- **Which probe a mesh uses.** `probe_for`: the smallest-volume probe with a captured map whose box holds
  the mesh's centre (`model.w_axis`). No probe: the sky. The choice goes into `GpuMesh.probe` and
  `probe_box` (centre and mip count, half extents and box-projection flag). Runs of opaque meshes are
  split by probe id, so each run binds that probe's cube in group 2 (`lighting_group`, one bind group per
  probe map).
- **Box projection.** With `box_projection`, `environment` intersects the reflected ray with the probe's
  box from the shaded point and samples the cube towards that hit as seen from the probe centre, so walls
  line up and floors look grounded. Without it the probe acts as infinitely far.
- **Sampling.** `lod = sqrt(roughness) * (mips - 1)` of the probe's cube, times `environment_intensity`.
  A probe replaces only the specular reflection; diffuse sky light still comes from the sky.

## Shadows

[shadows.rs](../crates/platypus_render/src/world/shadows.rs). `WorldRenderer::draw_shadows` plans and draws
all shadow maps **once a frame**, from the main screen view (the biggest viewport, not a render
texture's). Every view drawn that frame reads the same maps: other cameras, mirrors, probe faces and
render textures. Casters are the main view's opaque and cutout meshes with `cast_shadows` on (cutout
casters keep their holes).

`shadows::plan(camera, lights, settings)` returns a `ShadowPlan`: a `ShadowSlot` per casting light (a light
with `Shadows { soft, strength }`, strength above 0), a matrix per map, and texel sizes for the normal
offset.

- **Sun.** The first directional light with shadows gets `shadow_cascades` (1 to 4) cascades in a
  `Depth32Float` array of `shadow_resolution` texels. With 2 or more, the main camera's view out to
  `shadow_distance` is split into slices that grow with distance, each fitted with a sphere and an
  orthographic map, moved in whole texels so edges do not crawl. One cascade is a single sphere that
  distance across in front of the camera.
- **Spot and point lights.** A spot light gets one perspective map (a little wider than its cone; for a pyramid, than the cone round its corners, `bounding_degrees`), a
  point light a cube of six 90 degree maps (faces +x, -x, +y, -y, +z, -z). They share a second array at
  half the sun's resolution with at most `MAX_LOCAL_LAYERS` (24) layers. Lights are taken nearest the
  camera first and a light that no longer fits goes without. The maps reach the light's `range`.
- `ShadowArray` holds each texture array: one view to sample, one per layer to draw into. Arrays are
  recreated when the size or layer count changes. `GpuLight.shadow` carries the slot's first matrix
  index, strength, filter radius and texel size; `shadow_matrices` is a storage buffer in group 2.
- **Shader.** `sun_shadow` takes the first cascade whose map holds the point (offset along the normal by
  1.5 texels); `local_shadow` picks the cube face from the direction to the light (the offset grows with
  distance). Both use `pcf`, a (2r + 1)^2 box of comparison samples, and mix the result with 1 by the
  strength. Outside every map a point is lit.
- `WorldStats::shadow_maps` counts the maps drawn in the frame (`M5-RND-20` checks 4 + 1 + 6).
- Scripts set the distance, size and cascades with `QualitySettings.shadowDistance` (default 25 m),
  `shadowResolution` (default 2048, 256 to 8192) and `shadowCascades` (1, 2 or 4; Low 1, Medium 2, High and
  Ultra 4): `render.shadowSettings 0 distance resolution cascades`. The app copies them to `WorldRenderer`
  before each draw.

Each `MeshInstance` has `cast_shadows` (`MeshRenderer.shadowCastingMode`, `on` / `off`): opaque runs are
grouped by it too, and the shadow pass draws only the casting ones.

## Bloom and post-processing

[bloom.rs](../crates/platypus_render/src/world/bloom.rs) and `bloom.wgsl`, [fxaa.rs](../crates/platypus_render/src/world/fxaa.rs).
Post effects run only in the screen's **final pass**.

**Bloom.** `RenderSettings.bloom` (`{ threshold, intensity, scatter }`, default off) sends `render.bloom`; the
app sets `WorldRenderer::bloom`. With bloom on (`intensity` above 0), `render_views` draws the world into an
`Rgba16Float` texture (`HDR_FORMAT`), so emission and hot lights can be brighter than white.
`BloomPass::encode` picks the bright parts above `threshold` (with a soft knee), blurs them down a chain
of half-size textures (at most 7 levels, fewer for small targets) and back up. `scatter` sets how far the
blur spreads. Pipelines are cached per colour format and the textures per size.

**Exposure and tonemapping.** `Grading { exposure, tonemapping }` is applied after bloom by `fs_grade`
as the HDR texture is copied to the target. `Tonemapping` is `None` (clipped at white), `Neutral`
(Khronos PBR Neutral: colours kept until a soft shoulder) or `Aces` (filmic). Exposure multiplies the light
by 2 to the stops. `RenderSettings.tonemapping` (`"none"`, `"neutral"`, `"aces"`) and `postExposure` (stops,
-10 to 10) send `render.post 0 postExposure tonemapping`; the app sets `WorldRenderer::grading`. A
non-identity grading also draws through the HDR texture.

**Auto exposure.** `Grading.auto_exposure: Option<AutoExposure>` (`min_ev` -10, `max_ev` 20, `compensation` 0,
`speed_dark_to_light` 3, `speed_light_to_dark` 1) adapts the exposure to the view, like HDRP's Automatic mode.
The HDRP-style settings are in `AutoExposure`; the state is per camera.

- **Per camera.** Each `WorldView` has an `exposure_key`; the app sets it to the camera's id. `BloomPass` keeps
  `eyes: HashMap<u32, Eye>`. An `Eye` has a 16-byte `meter` storage buffer (x: the adapted exposure in stops,
  y: 1 once set, z: the metered EV100), a `ViewParams` uniform (`rect` in uv, `weight`, `snap`, `hold`), a
  `MAP_READ` staging buffer for read-back and a blend `weight` (0..1). `encode` takes an `ExposureView { key,
  rect }` per view: the biggest first (drawn over everything), the others with scissor rects. The meter
  compute runs once per view over its own rect; the prefilter and the composite draw per view with that
  view's eye. An eye unused for 600 frames is dropped. HDR textures are still cached per size, but no longer
  hold the meter. Split screens each adapt to what they show. Test `M7-EXP-02`.
- **Metering.** `cs_meter` (one workgroup of 256) bins the log2 luminance of a 64x64 grid of the view's rect
  into a 256-bin histogram (-16..+16). Samples are weighted by `Metering`: `Average` (all alike),
  `CenterWeighted` (`exp(-|d|^2*6)`, the default) or `Spot` (a circle of radius 0.12 in the middle). The
  average is taken between `low_percent` and `high_percent` of the histogram (0 and 100 by default). That
  becomes EV100 (+3), clamped to `min_ev..max_ev`, and gives the target stops `-ev - log2(1.2) +
  compensation` (HDRP's `1/(1.2*2^EV100)`). Test `M7-EXP-03`.
- **Soft floor and curve.** Below `dark_below` (EV100, 5) the metered EV becomes `dark_below + (ev -
  dark_below) * dark_adaptation` (1 by default: no change), so a dark place brightens only part of the way
  instead of hitting `min_ev`'s wall. The compensation curve has up to `MAX_CURVE_POINTS` (8) points of
  (EV100, stops), joined by straight lines, flat past the ends, and is added to `compensation`. Test
  `M7-EXP-06`.
- **Adapting.** The first use snaps to the target (so does a per-eye `snap`, set for new cameras and
  `reset_exposure`); after that the meter eases with `1 - exp(-dt*speed)`, using `speed_dark_to_light` when the
  target is lower (the view got brighter). `hold` keeps the adapted value as it is. `Grading.delta_time` is set
  by the app from the frame time; the pass caps it at 1/15 s.
- **Blending.** `Grading.fixed_ev: Option<f32>` is a manual exposure in EV100 (`None`: x1), and
  `Grading.transition` is the seconds auto exposure takes to blend in and out. Each eye's `weight` moves
  towards 1 while auto is on and 0 while it is off, by `dt / transition` (at once for 0). The shader multiplies
  by `exposure * exp2(mix(fixed_stops, adapted, weight))` (`auto_scale()`). A change of `fixed_ev` also
  eases over `transition` (smoothstep): each eye keeps `fixed`, `fixed_from`, `fixed_to` and `fixed_t`
  (a new camera starts at the target), and the shader reads the eased `fixed`, not `fixed_ev`. `BloomPass::fading()` keeps the post
  pass running while a weight fades out after auto is switched off. Test `M7-EXP-04`.
- **Bloom.** The prefilter threshold is applied to the light times `auto_scale()`, not to post exposure, so
  what blooms is what looks bright now.
- **Read-back.** After submit, `after_submit` maps each eye's staging buffer (`device.poll(Poll)`); `collect`
  reads them next frame into `ExposureReading { ev, scene_ev }` (the EV100 in use, and how bright it metered;
  NaN before the first). `WorldRenderer::exposure(key)` and `reset_exposure(key)` expose them. When a camera's
  values change, the app sets `Camera.exposure` and
  `Camera.sceneBrightness` (natively; the `__platypus_exposure` JS call is gone). `Camera.resetExposure()` sends `render.exposureReset id`; `WorldViews` queues it
  and the app applies it. Test `M7-EXP-05`.
- **Script settings.** `RenderSettings.autoExposure` is `{ minEV, maxEV, compensation, speedDarkToLight,
  speedLightToDark, metering ("average" | "centerWeighted" | "spot"), lowPercent, highPercent, darkAdaptation,
  darkBelow, curve: [ev, stops][], hold }` or `null` (the default). `RenderSettings.fixedExposure` (EV100 or
  `null`) and `exposureTransition` (seconds) go with it, and `freezeExposure()` turns auto off after copying
  the main camera's `exposure` into `fixedExposure`, so nothing changes on screen.
  `QualitySettings.autoExposure` (on by default, saved with quality) is the player's switch: off, the
  game's `auto` is sent as null, so it blends out to the manual exposure. All of it rides on `render.post`.
  Test `M7-EXP-01`.

**FXAA.** `Camera.antialiasing = "fxaa"` (camera op argument 14) sets `WorldView.fxaa`. If the main view
has it, `FxaaPass` runs over the finished, tonemapped image: it finds edges by gamma-space luma contrast
and blends along them. It is cheaper than MSAA and smooths texture and shading edges too, a little
blurrier. Its textures are cached per size.

**Post-process passes.** After FXAA, a view's `WorldView::post` (scripts' `PostProcess` passes: built-in effects
or their own WGSL) run over its part of the picture, with depth and normals available; see
[postprocessing.md](postprocessing.md). With any pass present the frame takes the HDR path like bloom, the world
pass keeps its depth buffer when a pass reads it, and the chain ping-pongs between two target-format textures
before its last pass draws into the target.

## Fog

`RenderSettings.fog` (off by default) with `fogMode` (`"linear"`, `"exponential"`, `"exponentialSquared"`),
`fogColor` (linear), `fogDensity` (per metre, for the exponential modes) and `fogStartDistance` /
`fogEndDistance` (linear) send `render.fog`; the defaults are Unity's and the settings are saved with the
scene. The app keeps one `Fog { mode, color, density, start, end }` and gives it to every `WorldView`
whose camera shows it (`Camera.fog`, `render.camera` argument 17, on by default; off for a minimap).

- **Shader.** `GpuGlobals` has `fog` (rgb colour, w mode: 0 none, 1 linear, 2 exponential, 3 squared),
  `fog_params` (density, start, end) and `view_dir` (the camera's forward axis, w = 1 if orthographic).
  `fog_amount(p)` is the share of fog between the camera and the world position `p`; `fogged(c, p)`
  applies it to a premultiplied colour, so what a pixel covers turns to fog colour and light it only adds
  (alpha 0: glows, beams) fades out. `fs_mesh`, `fs_quad`, `fs_shape` and `fs_stroke` call it, so sprites,
  particles, panels, text and 2D shapes fog like meshes.
- **Distance.** Perspective cameras measure straight to the point, so turning the camera doesn't move
  the fog. Orthographic cameras use depth along the view, so a 2D scene fogs by layer.
- **Height fog.** `RenderSettings.heightFog` (`{ density, baseHeight, falloff }`, `null` by default, used
  only while `fog` is on) sends a seventh `render.fog` argument and sets `Fog.height: Option<HeightFog>`
  and `GpuGlobals.fog_height` (x density, y base height, z falloff, w on). The density is `density` per
  metre at `baseHeight`, thinning by e every `1 / falloff` metres above it and thickening below.
  `height_clear(a, b)` integrates it exactly along a segment, and `fog_amount` multiplies it into the
  distance fog's transmittance. Orthographic views measure from the camera plane. For height fog alone,
  use the exponential mode with `fogDensity` 0.
- **Background.** Distance fog leaves the background fill (colour or skybox) clear. `fs_fill` applies height
  fog to it out to the far plane, so the horizon sinks into the fog. Match the camera's background colour
  to `fogColor` for a seamless horizon with distance fog alone.
- **Light shafts.** `RenderSettings.volumetricFog` (`{ intensity, anisotropy, steps, distance }`, defaults
  1, 0.4, 4, 50, plus an optional `albedo` colour, white by default, `sharpDistance` / `sharpSamples`, below, and `resolution` (`"low"|"medium"|"high"|"ultra"`, `high` by default); `null` by default, used only while
  `fog` is on, saved with scenes) sends an eighth `render.fog` argument, `[intensity anisotropy steps
  distance r g b sharpDistance sharpSamples detail]` (albedo and the last three optional; `detail` 0..3 is `Fog.volumetric.detail`; anisotropy clamped to +-0.95, steps to 1..16, sharpSamples to 4..128), and sets `Fog.volumetric:
  Option<VolumetricFog>` and `GpuGlobals.fog_volume` (x intensity, y anisotropy, z steps, w reach; 0 off).
  `volume.rs` (`VolumePass`) owns an `Rgba16Float` 3D texture per view slot, sized by `volume::SIZES` (`VolumetricFog.detail`: low 160x90x48, medium 240x135x64, high 320x180x96, ultra 480x270x128; finer means sharper beams and a dearer fill). `encode` takes the size and remakes the slot's texture when it changes. The pass is temporal. Per slot it also keeps two
  "lit cell" 3D textures (ping-pong), a uniform buffer (`GpuFrame` / WGSL `VolumeFrame`: previous
  `view_proj`, previous camera and ortho flag, previous forward and distance, jitter xyz, blend w) and the
  last `VolumeCamera`. `VolumeCamera.lights` is a key of which directional lights light the fog, which of them
  have shadows and the sun's cascade count; when it changes the history is dropped (blend 1) instead of blended out. Once per view with shafts, before the render pass, `cs_volume_light` (4x4x4
  workgroups) lights every cell with one `scattered_all` sample at a Halton(2,3,5) point inside the cell
  (64 points, `JITTER`; frame 0 = the cell middle), reprojects the cell's middle into last frame's lit cells
  (`last_cell`) and blends with `1 - history_weight` (1.0 with no history: the first frame or after a size
  change; clamped to 0.02..1). Cells so average over their whole extent, so bars' shadows don't come out in stair steps, for
  about an eighth of the shadow lookups. Then the compute entry `cs_volume` (8x8 workgroups, one thread per
  screen cell) walks the cell's view ray and reads the lit cells (trilinear). Slices are spaced by the square of their index out to the reach
  (`slice_distance(w) = reach * w * w`), with `steps` sub-samples per slice. Each sample adds the drop in
  `fog_clear` times the lit cell (`scattered_all`, above) and the running total is stored at each slice centre. `scattered_all`
  sums every directional light with `Light.volumetricIntensity` > 0 (in `GpuLight.color.w`): a hard
  shadow (`sun_shadow` / `local_shadow` with PCF radius 0), times `phase(cos)` (Henyey-Greenstein,
  isotropic = 1). `in_volume(light)` says which lights are in the volume: directional lights, and spot and
  point lights with shadows (their beams are cut into blades by bars, slats and planks, finer than a
  pixel's ray samples can follow). Spot and point lights without shadows are worked out per pixel
  (below). The pass binds the view's globals (dynamic offset), the lighting group and group 3 bindings 1 (`volume_out`, write-only), 3 (`light_history`), 4 (frame uniform), 5 (`light_in`) and 6 (`light_out`), so the globals and shadow group layouts are visible to COMPUTE.
  Draws read the volume through group 3 (binding 0 `texture_3d`, binding 2 a linear sampler; the world
  pipeline layout has this fourth group) in `shafts(p, px, distance)`: a sample at the screen uv and
  `sqrt(min(len, last) / reach)`. Past the last slice the sun lights the rest of the fog as at its middle.
  The result is tinted by `albedo` (`VolumetricFog.albedo`, `GpuGlobals.fog_albedo`) and scaled by
  `intensity`. The albedo is separate from the fog colour, which is how the fog looks in ambient light: a
  night fog can be dark yet glow white under a lamp. Views without shafts bind
  `VolumePass::empty()`, a black 1-texel volume. `fs_fill` adds shafts to the background only when height
  fog is on. The cost doesn't depend on overdraw.
- **Denoising.** `volumetricFog.denoising` (`"none" | "reprojection" | "gaussian" | "both"`, `both` by
  default; HDRP's modes) is `Denoising` on `VolumetricFog`, sent as the 11th `render.fog` number
  (0..3). `temporal()` (reprojection) blends with last frame's cells; without it every cell is sampled at
  its middle and stands alone. `spatial()` (gaussian) makes `lit_at` read four taps half a cell out across
  the screen as the volume is added up (`GpuGlobals.fog_sharp.z` = 1) so each cell's grain is smoothed
  without softening beams; `reprojection` alone saves those reads (slower GPUs). Other settings:
  `historyWeight` (0.9, up to 0.98; 12th number), `samples` (1..8 shadow samples per cell per frame; 13th)
  and `motionSamples` (1..8, default 2; 14th), see motion below.
- **Grain, moire and motion.** Each cell shifts the frame's Halton offset by `cell_hash(id)` (a
  Cranley-Patterson rotation), so a regular pattern (blind slats, a grate) leaves grain the blending
  smooths, not moire bands from neighbouring cells all erring alike. The first frame, with no history,
  samples cell middles. History is read by `history_at`, a 5-tap Catmull-Rom filter (the bilinear
  trick, corners left out, clamped at zero), so reprojecting each frame doesn't blur it. A cell whose
  history moved by some cells takes more of the new frame (`MOTION_TRUST` 0.15 per cell). Extra samples
  follow an R3 sequence; cells whose view moved, or that have no history, take `motionSamples`
  (`VolumeFrame.samples` x/y), so the cost is paid only while the camera moves.
- **Moving lights.** `VolumeCamera.local_poses` (up to `LOCAL_POSES` = 4 shadowed spot or point lights:
  position, direction, range) tell `light_shift` how far the furthest light's beam swept since last
  frame (its move plus its turn times range; more lights than that count as 1 m). `VolumeFrame.samples.z`
  carries it. The further it swept, the more of the new frame counts (`LIGHT_TRUST` 4 per metre), capped
  at a quarter (`light_fresh`), so a sweeping beam leaves a short trail, not smeared old places.
- **Depth bias.** `volume_to(uv, depth)` reads the volume a slice and a half short of a surface (as
  Unreal does): cells at a surface's depth reach behind it, and a wall would show the beam on its far
  side as a disc of haze (`M7-FOG-15`).
- **God rays.** The volume's cells blur beams finer than themselves (window bars, grates, leaves). With
  `volumetricFog.sharpDistance` > 0 (default 0, off; `VolumetricFog.sharp_distance`, `GpuGlobals.fog_sharp` x)
  the directional lights' shafts within that distance are worked out per pixel by `sharp_sun` in `world.wgsl`:
  `sharpSamples` (default 32, `fog_sharp` y) points on the ray, spaced by the square of their index, so
  closer together near the eye, are tested with `sun_visible` (the sun's shadow map, PCF radius 1). Where
  two neighbouring points disagree (one lit, one shadowed), the step is binary-searched (`EDGE_SEARCH` = 6)
  for the exact edge and only the lit length counts. There is no per-pixel dither, so beams don't shimmer
  or lens as the camera moves. It suits large openings (a canopy, a roof hole); a shadow thinner than a
  step, wholly between two samples, is missed and thin bars alias into a hatch: use a finer `resolution`
  for those, or more samples.
  then supplies only what lies beyond: `shafts` adds `sharp_sun` minus `volume_to(uv, near)` (the volume's
  light out to a distance) to the volume's total. It costs `sharpSamples` shadow lookups per fogged pixel.
- **Spot and point shafts.** The volume's cells are several pixels wide, which blurred spot cones, so
  spot and point lights without shadows light the fog per pixel (shadowed ones go through the volume,
  above). `local_shafts(eye, dir, len, distance)` finds each such
  light's stretch of the view ray: inside its range sphere and, for a spot, its cone (a quadratic), so the
  cone has sharp edges up to its tip and a point light a round glow. Up to three stretches (before, inside
  and after the cone) are tested at their middles; each lit one gets 12 fixed samples
  (`LOCAL_SHAFT_SAMPLES`). The fog's transmittance (`media_clear`: distance and height fog and the
  volumes) is taken at the stretch's ends and shared evenly between the samples. `shafts` adds the result
  to the volume's.
- **Fog volumes.** `FogVolume` (a component; size default 10x2x10, `density` 0.5, `color` (0.75, 0.78, 0.82),
  `blendDistance` 1) is a box of fog in its own colour: mist over a pond, smoke in a room. It sends
  `render.fogVolume` and follows its object's position and scale (world-aligned; rotation is ignored).
  Each camera gets the 8 nearest volumes (`MAX_FOG_VOLUMES`) its layers show, none when `Camera.fog` is
  off, as `WorldView.fog_volumes: Vec<FogVolume { centre, half_extents, density, color, blend }>`.
  `render.remove` and destroyed objects drop them. `GpuGlobals.fog_local` (x count) and `local_fogs`
  (`GpuFogVolume`: centre + density, half extents + blend, colour; colour w = 1 for an ellipsoid) carry
  them to the shader. `shape` (`"box"` by default, or `"ellipsoid"`; `render.fogVolume` seventh argument,
  `FogVolume.ellipsoid`) makes a soft round blob inside the box, with the edge fade measured in from the
  ellipsoid's surface.
  `local_fog_clear(a, b, i)` finds with a slab test where the view segment crosses the box, then takes 8
  fixed samples of `density` times a smoothstep edge fade over `blend` metres, so it is smooth from pixel
  to pixel. `local_clear` multiplies the volumes together, and `through_volumes` applies each in its own
  colour after the global fog in `fogged`, and in `fs_fill` for the background. Lit fog (the shafts)
  is added last, after the volumes, so a lamp glows in its mist rather than being darkened by it. They work without
  `RenderSettings.fog`. `cs_volume` multiplies `local_clear` into its transmittance, so light shafts
  light the volumes.
- **Culling.** `Fog.cull` (`RenderSettings.fogCulling`, default on; the ninth `render.fog` argument)
  skips what the distance fog hides completely. `Fog::cull_distance()` is the linear end, `ln(255) /
  density` (exponential) or `sqrt(ln 255) / density` (squared): under one 8-bit step gets through.
  `cull::FogReach` tests a world box against it: perspective cameras by the distance to the box's nearest
  point, orthographic ones by depth. Meshes, quads and point/spot lights wholly past it are skipped and
  counted in `WorldStats::fog_culled`. Height fog and fog volumes never cull. With a background unlike the
  fog colour, culled things would have been fog-coloured silhouettes, so turn culling off there.
  Scripts read `RenderStats` (`meshes`, `culledMeshes`, `quads`, `culledQuads`, `fogCulled`, `lights`,
  `culledLights`, `shadowMaps`, `shadowCasters`); the host hands them to the native `RenderStats` each frame they change.
- **Left clear.** The editor grid and gizmo lines.

`examples/fishtank` uses linear fog for its depth haze. `examples/fog` is a misty clearing with a pergola
and a lamp post, showing every kind of fog (menu or keys F, H, L, Q). `examples/fog-teapots` (teapots
fading into fog), `fog-raider` (a PS1-style scene, renamed from `fog-ps1`), `fog-blocks` (a block hill in
fog and cloud), `fog-park` (a Victorian park in the fog) and `fog-white-room` (the Animus's endless white room: a
clear bubble in white fog under the cloud layer, an animated mannequin) show it in scenes. `godrays-cell` (a barred prison window, dust; keys G/F/D/T), `godrays-room` (a broken roof, mullioned window and shutters; G/F/D) and `godrays-forest` (canopy beams; G/F/Space) show god rays. `fog-world` is an endless streamed block
world: 16x16-column chunk meshes, recycled around a wandering camera, with linear fog as the render
distance and a spare ring of chunks past the fog to show fog culling. In a release build with vsync off,
culling drew 45-50 meshes against 65-73 (16-20 hidden by fog), about 2.2 against 2.7 ms a frame. It has presets on 1-4 (very small, 2 chunks; medium, 6; large, 10; far: no fog, 14), Space to pause, a time budget for chunk building, and log lines with chunks, meshes drawn and culled and a pop-in count (chunks missing inside the fog). Chunks are built into reused typed arrays and `WorldStats` counts what the fog hid during culling itself.

## Render textures

A `RenderTexture(width, height)` is a `Texture` that
cameras draw into. It sends `render.renderTexture id width height` (`setSize` sends it again; up to 16384
texels); the app calls `WorldRenderer::set_render_texture`, which makes a `RENDER_TEXTURE_FORMAT`
(`Rgba8UnormSrgb`) texture kept under that id. Materials, sprites and quads show it like any asset
texture (`WorldTexture::Asset(id)`). `Texture.destroy()` removes it.

`Camera.targetTexture` (camera op argument 15, `null` clears it) sets `WorldView.target_texture`. A camera
with a target texture is not `Camera.main` and is not a screen view.

`render_views` draws in this order: shadows, then the render textures (`render_into_texture`, each
texture's cameras in depth order, with the world's lights and sky but no bloom, grading or FXAA), then the
screen. So the screen sees this frame's picture. A camera cannot sample its own texture: in its own
views, things showing it draw plain colour; other render textures show their latest picture. Scenes save
`RenderTexture` (size) like other textures (`M5-RTX-01`).

## Screen UI over the world

`UiRenderer::render_overlay(gpu, target, cmds)` ([ui/mod.rs](../crates/web_ui_render/src/ui/mod.rs)) is
`render` with two changes: the canvas starts **transparent** instead of cleared, and its final blit
**blends** (premultiplied alpha) onto what the target holds, with `LoadOp::Load`. The blit pipeline is
cached by `(format, overlay)`.

That canvas holds only UI, so backdrop filters and blend modes there would see no world. When the
draw list has either (`needs_backdrop`: a `BackdropBlur`, or a `PushLayer` with a blend mode),
`UiRenderer::render_over(gpu, target, backdrop, cmds)` is used instead: the world is drawn into
`backdrop_target` (a cached texture of the target's size and format that can be sampled, unlike a
swapchain image), the canvas is **seeded** with a copy of it (a blit into canvas 0 in place of its
clear), the UI is drawn as usual, and the final blit **replaces** the target's pixels. Frames
without those effects keep the cheaper overlay path.

`draw_frame` in `main.rs` picks the path:

- No active camera: `UiRenderer::render` over the plain `CLEAR` colour, as before M5.a.
- UI that reads the world: `render_views` into `backdrop_target`, then `render_over`.
- Otherwise: `WorldRenderer::render_views`, then `render_overlay`.

Screenshot runs and window frames both go through `draw_frame`.

## Components from scripts

`Camera`, `PlaneRenderer`, `MeshRenderer`, `Light`, `SpriteRenderer`, `ParticleSystem`, `Mirror`,
`ReflectionProbe`, `LODGroup` and `WorldPanel` are native components (`platypus_render_api`). They reach the renderer as
`render.*` ops keyed by the component id, and `WorldViews`
([world_view.rs](../crates/platypus_app/src/world_view.rs)) stores them in one map per kind.

| Op | Arguments |
|---|---|
| `render.camera` | `component object fov near far [r g b a] depth orthographic orthographicSize [x y w h] clearFlags(skybox\|color) cullingMask [32 cull distances]\|null cullSpherical antialiasing targetTexture\|null grid` (`grid` is `Camera.sceneGrid`) |
| `render.gizmos` | `camera [ax ay az bx by bz r g b a ...]` (10 floats per line segment, world space; the camera shows them until replaced, and a later `render.camera` keeps them; non-finite lines are dropped) |
| `render.stroke` | `component object width [r g b a] closed [x0 y0 x1 y1 ...] [fill r g b a]\|null evenodd\|nonzero sortingOrder` (the last three are optional: a fill colour and rule make the points a closed, filled shape; `sortingOrder` is the draw order among shapes; width 0 draws the fill alone; points on the object's local XY plane; `views()` turns them into world segments through the object's matrix, skips them by the camera's culling mask, and `render.remove` or destroying the object drops them) |
| `render.plane` | `component object [w h] [r g b a]` |
| `render.mesh` | `id [positions] [normals] [uvs] [indices] [joints, 4 per vertex] [weights, 4 per vertex]` (a script mesh; `id null` removes it) |
| `render.meshRenderer` | `component object mesh(name \| id) [r g b a] [er eg eb] smoothness unlit texture\|null [scale x y, offset x y] castShadows metallic ... [bone objects]\|null [bind poses, 16 each]` (the last two for `SkinnedMeshRenderer`) |
| `render.light` | `component object type [r g b] intensity range spotAngle innerSpotAngle haloSize beamIntensity shadows shadowStrength cullingMask` |
| `render.sprite` | `component object texture [x y w h]\|null [pivot x y] pixelsPerUnit [r g b a] flipX flipY sortingOrder` |
| `render.particles`, `render.particleControl` | see [particles.md](particles.md) |
| `render.texture`, `render.textureRemove`, `render.model` | see [assets-and-models.md](assets-and-models.md) |
| `render.ambient` | `0 [r g b]` |
| `render.fog` | `0 mode(linear\|exponential\|exponentialSquared) [r g b] density start end [heightDensity baseHeight falloff]\|null [intensity anisotropy steps distance [r g b [sharpDistance sharpSamples [detail]]]]\|null cull`, or `0 null` for clear air |
| `render.fogVolume` | `component object [sx sy sz] density [r g b] blendDistance shape(box\|ellipsoid)` |
| `render.sky` | `0 [zenith rgb] [horizon rgb] [ground rgb] [sun dir xyz]\|null [sun rgb] sunSize intensity ambientFromSky`, or `0 null` for none |
| `render.mirror` | `component object [nx ny nz] cullingMask` (local plane normal) |
| `render.probe`, `render.probeRender` | `component object [sx sy sz] resolution boxProjection everyFrame cullingMask`; `component` (render again next frame) |
| `render.bloom` | `0 threshold intensity scatter`, or `0 null` for off |
| `render.post` | `0 exposureStops tonemapping auto fixedEV transition`; `auto` is `[minEV, maxEV, compensation, darkToLight, lightToDark, metering(0 average, 1 centre, 2 spot), lowPercent, highPercent, hold, darkAdaptation, darkBelow, curvePoints, ev0, stops0, ...]` or null; `fixedEV` is EV100 or null |
| `render.exposureReset` | `cameraId` (that camera's auto exposure jumps to what it sees) |
| `render.quality` | `0 antiAliasing renderScale shadows(disable\|hardOnly\|all) softRadius lodBias vSyncCount anisotropic mipLimit volumetricFog hdr additionalLights(-1: all) additionalLightShadows` (see Quality settings) |
| `render.lodGroup` | `component object size [heights per level] [[renderer ids] per level] forcedLevel(-1: none)` |
| `render.shadowSettings` | `0 distance resolution` |
| `render.panel` | `component object [w h] pixelsPerMeter resolution [r g b a] interactive` (handled by `Panels`) |
| `render.text` | `component object text [r g b a] fontSize fontFamily\|null fontWeight align maxWidth pixelsPerUnit resolution` (a `TextMesh`; handled by `Panels::apply_text`, see World panels) |
| `render.remove` | `component` (removes a camera, plane, mesh renderer, light, sprite, particle system, mirror, probe, panel or text mesh) |

The component re-sends its op when a property changes. The defaults are a dark clear colour, `depth`
0 and a full-screen `rect`. A bad op (unknown primitive, mesh with bad indices) logs a warning.

`MeshRenderer` takes a primitive name or a `Mesh` and a `Material` (`color`, `emission`, `smoothness`,
`metallic`, `unlit`, `mainTexture`, tiling; `Material.create` and `with` take named settings, and
`withColor` and `withTexture` make copies; see [assets-and-models.md](assets-and-models.md#materials-and-meshes)). `Mesh.create` sends script
data (`Mesh.createFlat` does too, from flat arrays). `GameObject.createPrimitive(type)` makes an
object with the matching mesh and collider.

`WorldViews::views(scene, size, extra)` runs every frame, after `update_world`:

- Keeps cameras whose object is active, sorted by `depth` (a stable sort, so ties keep the order they
  were added).
- Takes only the position and rotation from a camera's world matrix. Scale is ignored.
- Builds the shared lists once: planes, then sprites (only once their texture size is known), then
  `extra` (the panel quads) become quads; mesh renderers plus spot-light beams become `MeshInstance`s;
  lights become `Light`s; halos are kept as positions.
- Builds the probe list (active probe objects) and each mirror's world plane, and sets `skybox` from
  the camera's `clearFlags`.
- Gives each camera a `WorldView` with clones of those lists, plus per-camera quads: the particles'
  quads and the halos, built facing that camera's rotation.
- A plane lies flat in its object's XZ plane facing +y, like Unity's Plane (`lay_flat`).
- With no cameras it returns an empty list, so only screen UI is drawn.

Components on destroyed objects are dropped by `forget_objects`.

A `Camera` has `sceneGrid` (default off, not saved with scenes), and `Gizmos` is
Unity's immediate-mode line drawing: `color`, `matrix`, `drawLine`, `drawRay`, `drawWireCube`,
`drawWireDisc`, `drawWireSphere` and `drawFrustum` append world-space segments (through `matrix`, in
`color`) to one list, `lineCount` reads it, `clear()` drops it, and `present(camera)` sends it as
`render.gizmos` and starts over. The lines belong to that camera until the next `present`, so calling it
with nothing drawn clears them. Unlike Unity, nothing calls `OnDrawGizmos`: the editor draws what it wants
and presents it each frame (see [editor.md](editor.md#the-scene-view)). `Camera.main` skips cameras whose
object has `HideFlags.HideInHierarchy`.

`LineRenderer2D` draws thick lines on the object's local XY plane:
`points`, `width`, `color` and `loop`, plus `fillColor` (null: no fill), `fillRule` (`evenodd` by default,
or `nonzero`) and `sortingOrder` (higher draws over lower, equal ones in creation order; sprites and text show over shapes). A fill always closes the shape, and a `width` of 0 leaves just the fill. It sends the points
flat at most once per frame (in `onPreRender`) and sends `render.remove` when destroyed.

`Shapes` makes points for `LineRenderer2D`, after GDI's drawing calls:
`rectangle`, `roundedRectangle`, `ellipse`, `circle`, `arc`, `pie`, `regularPolygon`, `polygon`, `triangle`,
`bezier`, and the cardinal splines `curve` and `closedCurve` (tension 0.5 is Catmull-Rom). They lie on the XY
plane with y up, and angles are in degrees counter-clockwise from +x. Closed shapes do not repeat the first
point: set `loop`, or add a `fillColor`.

`Spline` and `SplineAnimate` are the core of Unity's Splines (`M5-SPL-01`). A
`Spline` is a list of knots (`position`, `tangentIn`, `tangentOut`, `mode`); every segment is a cubic Bezier.
A knot's `TangentMode` is `auto` (a cardinal spline through its neighbours: a third of `tension` times the chord
across the knot, so 0.5 is Catmull-Rom like GDI's curves), `bezier` (its own handles, relative to the knot) or
`linear` (a third of the way to each neighbour, so straight runs keep an even speed). `closed` joins the last
knot to the first. `addKnot`, `insertKnot`, `removeKnot`, `setPosition`, `setTangents` (makes the knot
`bezier`) and `setMode` edit it; `version` goes up on every change. `evaluate(t)` and `tangent(t)` take `t` 0 to 1
by distance, through a length table (32 samples per segment, rebuilt after a change), so speed is steady.
The control points are cached in a `Float64Array` (12 numbers per segment, cleared on change) and evaluated with plain numbers (`pointInto`), for lengths and `nearest` too. `segmentPoint(i, u)` uses a segment's own parameter. `length`, `sample(perSegment)` (points for
`LineRenderer2D`) and `nearest(p)` (`t`, point, distance) read it. `SplineAnimate` moves its object along
`spline` (set from code, not saved) over `duration` seconds: `loopMode` is `loop`, `pingPong` or `once`,
`alignment` `path` turns the object to face along the spline (z rotation on a flat one, else a look rotation),
`container` puts the spline in another object's space, `playing` pauses it, and `elapsed` jumps. `place()`
moves it now.

`CameraFollow` and `CameraOrbit` are script components that move a camera object in `onLateUpdate`.
They have no Rust side.

## Coordinates

- Right-handed, +y up. A camera looks down its **-z**. Projections have depth 0 to 1
  (`glam::camera::rh::proj::directx`), and `Matrix4x4` in `platypus_math` matches, so a camera's matrices
  in TypeScript equal what the renderer uses.
- A quad faces +z. A panel's top-left corner is at (-w/2, +h/2) in its object's space.
- **Screen points are top-left UI pixels**: the window's pixels divided by the DPI scale and the UI
  scale. These are the units of `Screen.width` and `height` and of pointer event `clientX/Y`.
  Viewport points are 0 to 1 from the top-left, and NDC y is flipped (`1 - v * 2`).
- `Camera` conversions in TypeScript: `screenToViewportPoint`, `viewportToScreenPoint`,
  `viewportPointToRay`, `screenPointToRay`, `worldToViewportPoint`, `worldToScreenPoint`,
  `viewportToWorldPoint`, `screenToWorldPoint`. `z` is the distance in front of the camera. A camera's
  `rect` maps the screen to its viewport. `pixelRect` and `aspect` follow `Screen`. `Camera.main` is the
  first active camera added, and `Camera.allCameras` lists them in draw order.

## World panels

A `WorldPanel` component puts React UI on a plane. In a script:

```ts
const panel = menu.addComponent(WorldPanel, { width: 640, height: 400, pixelsPerMeter: 170 });
createRoot(panel).render(<Menu />);
```

**Panel roots.** `createRoot(panel)` gives the panel its own native reconciler (`platypus_ui_native::Runtime`)
whose ops go to the panel's tree, keyed by the component id; the screen is target 0. (The QuickJS commit
targets, `__platypus_commit(ops, target)`, `ScriptRuntime::drain_targeted_commits` and `App::apply_script_output`
are gone.) `Panels::get_or_create(target, ...)` makes the panel's tree on first use.

**Panel trees.** Each `Panel` has its own `UiTree` and `Interaction` (hover, focus, scroll state):

- `new_panel_tree` builds the tree with the system fonts, replays every font the scripts loaded
  (`font_sources`) and adds every stylesheet (`sheet_sources`: `game.css` under the key `GAME_CSS`, and
  `addStyleSheet` sheets under their script ids). Later `loadFont`, `addStyleSheet` and
  `removeStyleSheet` calls update the screen and all existing panels, so every tree has the same CSS.
- Size, `pixelsPerMeter` (default 500), `resolution` (default 2, clamped 0.25 to 4) and `tint` come
  from `render.panel`. Size is clamped to at least 1 UI pixel.

**TextMesh.** A `TextMesh` is text in the world with no React
root: `text`, `fontSize`, `color`, `font`, `fontWeight`, `alignment`, `maxWidth`, `pixelsPerUnit` (default 100)
and `resolution`. It sends `render.text` at most once per frame and `render.remove` when destroyed.
`Panels::apply_text` makes a panel (not interactive) and builds its UI tree in Rust: one absolutely
positioned `text` element, so it shrink-wraps its text. The text is white and the colour is the panel's
tint, like a sprite. `whiteSpace` is `pre`, or `pre-wrap` when `maxWidth` is above 0, with `maxWidth` set as
the element's max width. The tree is laid out with room to spare, then the panel is fitted to the text's
content box (at least 1 pixel) and drawn centred on its object (`M5-TXT-01`). The panel remembers what it
laid out (`text_key`: text, font size, family, weight, alignment, max width). A new colour or scale alone only
changes the quad's tint and skips layout, and the texture is redrawn only after a new layout (`drawn`). Other
panels are redrawn every frame (a TODO in TODO.md).

**Textures.** `Panels::render` runs every frame before the world pass. For each active panel (a TextMesh whose
layout is already drawn at the same size keeps its texture and skips the rest) it sets
the tree size and clock, loads images, builds the draw list and draws it with
`UiRenderer::render` into the panel's own `Rgba8UnormSrgb` texture (cleared transparent). The texture
is `size * resolution` pixels, capped at 4096, and is recreated when that changes. It returns one
`WorldQuad` per panel (model = the object's world matrix scaled to the size in metres, `color` = tint)
and the texture views. The quads go to `WorldViews::views` as `extra`, so every camera sees them.
Panels on inactive objects are skipped, and `forget_objects` drops panels on destroyed objects.

**Input routing.** `Surface` is `Screen` or `Panel(id)`. `App` keeps `pointer_on`, `keys_on`,
`buttons_down` and `pointer_px`, and `handle_on(surface, input)` runs one `Input` on that surface's
tree and interaction.

- `route_pointer` runs on mouse move. With no button down it asks `surface_at(px, py)`; the surface
  that is left gets `PointerLeave`, the new one gets `PointerMove`.
- `surface_at`: if the **screen UI** has an element at the point, it wins. Otherwise
  `camera_ray(px, py)` casts a ray, `Panels::hit` picks the nearest interactive panel it meets, and
  the point becomes the hit in that panel's UI pixels. Otherwise it is the screen.
- **`pointerEvents: none` rule.** Screen UI wins wherever `UiTree::hit_test` finds an element other
  than the root. A full-screen layout box (a HUD wrapper) would therefore hide every panel behind it.
  Layout boxes that cover the screen must set `pointerEvents: "none"`; children with their own
  handlers still get events.
- A drag stays with the surface it started on. While a button is down, the position is projected onto
  that panel's plane with `Panels::project` (outside its bounds too), or `(-1e6, -1e6)` if the ray
  misses.
- Press, release, wheel and cursor go to `pointer_on`. The cursor shape and image come from that
  surface's tree. A press sets `keys_on`, so keys, text and IME go to the surface clicked last (and the
  `focus` op also sets it, through `surface_of(node)`). `Modifiers` and `WindowBlur` go to every
  surface. Each panel gets `Input::Refresh` per frame.
- IME is allowed when the focused surface has an editor. For a panel the candidate-window area is not
  set.
- `camera_ray` takes the last view whose viewport contains the point (the camera drawn on top), then
  unprojects the near and far NDC points through the inverse `view_proj`. `ray_quad` intersects the
  ray with the quad plane in the quad's own space and returns `(u, v, distance)` with (0, 0) at the
  top-left. `None` means parallel or behind.

## Layers and culling

Every GameObject has a `layer` from 0 to 31 (`GameObject.layer`, op `scene.layer id n`, default 0;
`ScriptScene::layer` reads it, 0 for unknown ids). `LayerMask`
(the `LayerMask` API) names layers: Unity's built-ins (0 Default, 1 TransparentFX,
2 Ignore Raycast, 4 Water, 5 UI) plus the game's `project.json` `layers` (`{ "8": "Player" }`,
passed on by `info.rs`). A mask is a `u32`, a bit per layer; scripts may use signed masks (`~mask`),
which are made unsigned before they cross the op boundary. Missing or null masks mean every layer
(`ALL_LAYERS`).

**What a mask filters.**
- **Camera** `cullingMask`: the camera draws only meshes, planes, sprites, panels, particles and halos
  on those layers. `layerCullDistances` (32 numbers, 0: out to the far plane) hides a layer's things
  beyond a distance, measured along the view, or straight from the camera with `layerCullSpherical`.
  The app does this in `WorldViews::views` with `Camera::shows(layer, at, position, forward)`, using
  each object's origin. Every list is held as `Tagged { layer, at, item }` and filtered per camera,
  so the `WorldView`s already hold only what that camera draws. Panels return `(object, quad)` pairs
  so their quads carry the panel object's layer.
- **Light** `cullingMask`: `fs_mesh` checks the mask against each instance's `layer` (`in_mask`), so a
  light lights only those layers. The shadow-casting light's mask also picks which meshes cast its
  shadows.
- **Mirror** and **ReflectionProbe** `cullingMask`: the layers drawn into the reflection or cubemap.
  Set them before the component starts.

**Frustum culling** ([cull.rs](../crates/platypus_render/src/world/cull.rs)) runs in `render_views_with`,
per view. `Frustum::from_view_proj` takes the six planes from the matrix (wgpu depth 0 to 1; works for
perspective and orthographic) and tests spheres and boxes. Meshes use the box of their mesh data
(`bounds`) moved by the instance matrix (`transform_aabb`); quads use `QUAD_BOUNDS`.
- Opaque runs get a `visible` flag. Opaque meshes out of view are skipped in the colour pass but
  **still drawn into the shadow map**, so a plank above the camera still shadows the floor in view.
  Runs are split by `visible` as well as by mesh and texture.
- See-through meshes and quads out of view are dropped before sorting.
- Point and spot lights whose range sphere misses the frustum are dropped; directional lights never
  are. The shadow light's index is looked up again in the kept list for each view.
- Shadow casters outside every shadow layer of their lights are left out of the shadow maps.
- `WorldRenderer::stats()` returns `WorldStats` (meshes, quads and lights drawn and culled, summed
  over the views of the last `render_views` call, and `shadow_casters`, the casters drawn into shadow maps),
  which the tests use.

**LOD groups.** `LODGroup` (`size`, `setLODs([{ screenRelativeTransitionHeight,
renderers }])`, `forceLOD(i)`) sends `render.lodGroup`. Levels go from most to least detailed, and the
heights must go down. For each camera `views()` computes how tall the group looks: `Camera::screen_height(size, distance)`
is `size / (2 · distance · tan(fov / 2))` (perspective) or `size / (2 · half_height)` (orthographic),
where `size` is the group's size times the largest world scale of its object. That is multiplied by
`QualitySettings.lodBias` (`Quality.lod_bias`; Low 0.4, Medium 0.7, High 1, Ultra 2). The first
level whose height is at most that value is shown; below the last, nothing. The renderers of the other
levels are left out of that camera's `meshes` (`Tagged.renderer` holds the MeshRenderer id). A forced
level wins at any distance. Groups are removed with `render.remove` or when their object is destroyed.

`examples/layers` shows camera masks and cull distances (with a minimap camera that sees only map
markers), field of view and render distance, and LOD groups with `lodBias`. Light, probe and mirror
masks are covered by the `M5-RND-15` golden.

## Quality settings

`WorldRenderer` has the knobs; scripts set them through `QualitySettings`
(`QualitySettings`, native), whose levels (Low / Medium / High / Ultra, overridable in the game's
`package.json` under `platypus.quality`) send `render.quality` (anti-aliasing, render scale, shadow mode,
soft filter, `lodBias`, `vSyncCount`, anisotropic filtering, texture mip limit) and `render.shadowSettings`
(distance, map size, cascades). The default level is applied when the
game starts; the app copies the values onto the renderer before each frame.

- **MSAA** (`msaa`): the main world pass draws into a multisampled colour target (cached by size,
  format and count) with a multisampled depth buffer, resolving into the target (or the bloom HDR
  texture). Pipelines are cached by format and sample count. `supported_samples` lowers the count to
  what the GPU allows for the format and depth: without `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`
  (requested at device creation when the adapter has it) only WebGPU's guaranteed 1 and 4. Mirror
  and probe renders stay single-sampled.
- **Render scale** (`render_scale`, 0.25 to 2): `render_views` draws the whole world (probes,
  mirrors, bloom) into a texture of the scaled size (`upscale.rs`) and stretches it over the target
  with a bilinear blit. Screen UI is drawn afterwards at full resolution.
- **Vsync** (`vSyncCount`, 0 or 1, on by default; `Quality.vsync`): the app calls `WindowSurface::set_vsync` before each frame
  (never waiting for it in screenshot runs), which switches between `AutoVsync` and `AutoNoVsync`. With it off, `Application.targetFrameRate` caps the
  frame rate (see [platform.md](platform.md)). The quality choice is saved in `PlayerPrefs` as `platypus.quality`
  and restored at startup; its vsync is kept only with `playerVSync: true` (every save writes it), else the level's is used.
- **Shadow quality** (`shadow_quality`: `Disable`, `HardOnly`, `All`; `soft_shadow_radius` 1 to 3: `QualitySettings.softShadowQuality` `low`, `medium` or `high`):
  the shadow pass is skipped when disabled, soft shadows become one sample under `HardOnly`, and the
  soft filter is a (2r + 1)^2 box of comparison samples (3x3, 5x5 or 7x7; the radius is in `GpuLight.shadow`).
- **Texture quality** (`anisotropicFiltering`: `disable`, `enable`, `forceEnable`; `globalTextureMipmapLimit`
  0 to 3): `QualitySettings` sends them in `render.quality` (`M5-QUA-02`). `Anisotropic` sets how
  each texture's `anisoLevel` (a Texture option, 1 to 16, trilinear only) is honoured: `Disable` makes it 1,
  `Enable` uses the texture's level, `ForceEnable` uses at least 9. The mip limit drops that many top
  levels of every texture. Samplers are cached per filter, wrap and anisotropy.

### Render pipeline

Platypus has one render pipeline, as Unity 7 does (it keeps URP; HDRP and the Built-in pipeline go
away). `PipelineAsset` (`world/pipeline.rs`, Unity's URP asset) is plain settings, all on by
default: `volumetric_fog`, `max_additional_lights`, `additional_light_shadows` and `hdr`. Each frame
the app copies the quality level's asset onto `WorldRenderer.pipeline` (`world_view::Quality.pipeline`,
parsed from `render.quality` arguments 9 to 12; a light count of -1 means no cap). What they do:

- **No `volumetric_fog`:** the fog's light shafts and god rays are dropped; fog is drawn without them.
- **`max_additional_lights`:** a view gets the main light (the brightest directional) plus the nearest N others.
- **No `additional_light_shadows`:** the shadow plan has no spot or point light maps (the main light still casts).
- **No `hdr`:** no post pass, so no bloom, exposure or tonemapping; colours clip at white.

The quality levels set them: Low (the phones' tier) has no shafts, 8 additional lights and no local
shadows; Medium 32 lights and no local shadows; High and Ultra everything. `QualitySettings` has
`volumetricFog`, `hdr`, `additionalLights` and `additionalLightShadows` (and `autoExposure`).
`platypus.quality.default` in `package.json` is a level name, or a map by `operatingSystemFamily` with a
`default` key (`{ "Android": "Low", "default": "High" }`). `platypus.renderPipeline` and
`renderPipelineSettings` no longer pick anything: they log a warning. (History: a Built-in /
Universal / HD choice existed briefly before this.) Tests `M7-RP-01` to `M7-RP-03`.

### Real-world light units

`platypus.lightUnits` in `package.json` is `"physical"` or `"legacy"` (the default: Platypus's own units);
scripts read it as `GraphicsSettings.lightUnits`. It is a project switch, not a pipeline
(`Manifest.physical_units` in `info.rs`, passed to `WorldViews::with_physical_units` and
`WorldRenderer.physical_units`). With `physical`:

- Light intensity is read as lux for directional lights (divided by pi into the renderer's units) and
  as lumens for spot and point lights (divided by 4 pi squared: lumens to candela over a sphere,
  then by pi). The app does this when it handles the light op.
- **Pre-exposure:** in the final pass everything drawn is multiplied by each camera's last exposure,
  so real-world values (a 200000 nit view is past fp16's 65504) fit the HDR target, and the post pass
  divides it out. Test `M7-HD-01`.
- `Camera.aperture`, `shutterSpeed` and `iso` (defaults f/16, 1/200 s, ISO 200, HDRP's) give
  `physicalExposure` = log2(N^2 / t) - log2(ISO / 100), an EV100 to use as `RenderSettings.fixedExposure`.
- `Light.illuminanceAt(point, normal?)` is a light meter: the lux from every light (lumens to candela over
  4 pi, inverse square, cosine), ignoring shadows. `Light.measureIlluminance` also counts shadows with
  physics rays (walls need colliders) and resolves after the next physics tick. Test `M7-HD-02`;
  `examples/light-units` is the manual check.

## Cooked texture uploads

`WorldRenderer::set_texture` builds the mip chain (`mip_chain`) and calls the shared upload with a
`CookedTexture { width, height, format, srgb, mips, filter, wrap, anisotropy }`. `TexelFormat` is `Rgba8`,
`Bc1` (8 bytes per 4 x 4 block) or `Bc3` (16). `set_cooked_texture` uploads such levels as they are, with
the matching wgpu format (the sRGB variant when `srgb`; a linear view format for the sampling path) and
whole-block row sizes. It returns false for data that fails `is_valid`, is too big, or is BC when the GPU
lacks `TEXTURE_COMPRESSION_BC` (`supports_bc`; `gpu.rs` requests it when the adapter has it). The app
then decodes the cooked texture and uses `set_texture`. How cooking makes them:
[assets-and-models.md](assets-and-models.md#cooking).

## Limits

- Only the first directional light with shadows casts sun shadows; spot and point lights share 24 layers,
  nearest first. Casters come from the main screen view's opaque and cutout meshes (no transparent casters).
- Bloom, exposure settings and tonemapping are one setting for every camera (auto exposure's adapted state is per camera); FXAA follows the main view's camera.
  Mirrors, probes and render textures are ungraded.
- Mirrors are flat, rendered only for the main view, show no mirrors inside mirrors, and cost a full
  extra scene render each. Other views see the sky in them.
- Probes are captured from the main view's world without mirrors or other probes. A mesh uses one probe
  (by its centre), with no blending between probes; the cube mips are a box blur, not a GGX
  prefilter. The sky is one fixed 128-texel map, and only specular reflections use probes.
- There is no screen-space reflection.
- Skinning is four bones per vertex, in the vertex shader. A moving skeleton's matrices are built every frame,
  seen or not (the frustum test needs its bone box); only drawn ones are uploaded.
- Frustum culling is conservative: meshes are tested by the bounding box of their mesh, quads by the unit
  quad, and nothing is occlusion culled. Cull distances are per layer, not per object.
- Panels re-render every frame while visible, with no dirty tracking; only TextMesh panels skip it until their text changes.
- Panels are not occluded by other world quads for hit testing: the nearest interactive panel wins.
- Plane quads are not hit tested.
- Pointer events on a panel use UI pixels of the panel, not the screen scale.

## Tests

Goldens in [platypus_render/tests/world.rs](../crates/platypus_render/tests/world.rs): `M5-RND-05` (a floor
in perspective, a plane seen through a half-transparent one, a textured plane, and screen UI blended
over the view) and `M5-RND-06` (a perspective view, an orthographic top-down view and an inset
drawn in depth order). `M5-RND-07` (lit primitives), `-08` (textured meshes and sprites), `-09` (bloom, additive glows and a
beam) and `-10` (directional shadows) cover the M5.a2 work. The materials work adds `M5-RND-11` (a smoothness by metallic sphere grid under the
procedural sky, with the skybox as background), `M5-RND-12` (a planar mirror: reflected camera, clip plane
and screen-space sampling) and `M5-RND-13` (a box-projected reflection probe in a room). Later goldens: `M5-RND-17` (material maps and
rendering modes under a sky), `-18` (tonemapping, exposure and FXAA), `-19` (a render texture shown on a
monitor quad) and `-20` (cascades, a spot light and a point light; it also checks the shadow map count), and `-21`
(a tube skinned to two bones, bent, with a bent shadow), `-22` (the ground grid: axes, fade, hidden by a box) `-23` (gizmo lines: constant width, faint behind a mesh) `-24` (thick 2D strokes: round joins and caps, soft edges, a 2-pixel line, hidden behind a cube) and `-25` (filled shapes: tessellation units, both fill rules, a golden, the `world_view` unit and JS units). Cooked textures are tested with cooking
(`M5-AST-06`, see [assets-and-models.md](assets-and-models.md#tests)).
Unit tests: `M5-MAT-01` (tangents, `mesh.rs`) and `M5-SHD-01` (shadow planning, `shadows.rs`). The JS units
`M5-MAT-02` (`Material` ops and scene saves), `M5-RTX-01` (render textures) and `M5-SHD-02` (cascades) in
`js/engine/test` were deleted with QuickJS, as were the "JS unit" parts of the `M7-FOG` and `M7-SPOT` tests below.
Render pipeline and light units: `M7-RP-01`..`03`, `M7-HD-01`, `M7-HD-02`.
Auto exposure: `M7-EXP-01` (adapts to the view), `-02` (per camera, split screens), `-03` (metering modes and percentiles), `-04` (fixed exposure, blending, hold, freeze), `-05` (read-back, `Camera.exposure`, reset), `-06` (soft floor and compensation curve).

Fog: `M7-FOG-01` (golden: the three modes against clear air, background clear), `M7-FOG-02` (orthographic depth fog; quads, shapes and strokes fog) and `M7-FOG-03` (`render.fog` and `Camera.fog` reaching `WorldView.fog`, in `world_view.rs`; JS units in `fog.test.ts`) and `M7-FOG-04` (height fog: golden, unit and JS unit; the horizon sinks into it) and `M7-FOG-05` (light shafts: golden, unit and JS unit; the froxel volume) and `M7-FOG-06` (fog volumes: golden, unit in `world_view.rs` and JS unit) and `M7-FOG-07` (fog culling: `FogReach` unit, integration counts, JS unit) and `M7-FOG-08` (spot cones and lantern glows per pixel: golden) and `M7-FOG-09` (typed arrays arriving as number lists, `typed_arrays_arrive_as_number_lists` in the script conformance tests) and `M7-FOG-10` (god rays: golden of a grate of thin bars; the ripple along a row is 30% higher with them) and `M7-FOG-11` (the temporal volume: averaged frames ripple less than one) and `M7-FOG-12` (a turning sun's beams follow without trailing) and `M7-FOG-13` (the volume's noise settings and denoising modes) and `M7-FOG-14` (a turning camera's beams: motion trust, Catmull-Rom history, `motionSamples`) and `M7-FOG-15` (depth bias; shadowed local lights in the volume; light motion). Pyramid spot lights: `M7-SPOT-01` (a wide, thin rectangle, turned with its object) and `M7-SPOT-02` (`spotShape` and `aspectRatio` in the light op, JS unit).
`examples/materials` is the manual check. `M5-PNL-02` (rays and panel quads) is a unit test in `panels.rs`, `M5-PNL-01`
(per-root commits) and `M5-CAM-01` (camera conversions) were JS tests, deleted with QuickJS.
`examples/world` is the manual check (`M5-PNL-03`).
