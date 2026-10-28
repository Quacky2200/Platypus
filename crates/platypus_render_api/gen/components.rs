// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The data of the generated components, for `build.rs`. Converted from the former Python generator.

use super::{Component, Dv, Field};

pub const COMPONENTS: &[Component] = &[
    Component {
        name: r#"Light"#,
        low: r#"light"#,
        reference: r#"LightRef"#,
        store: r#"LIGHTS"#,
        data: r#"LightData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"type"#,
                rs: r#"r#type"#,
                ty: r#"enum:directional|point|spot"#,
                default: Dv::Str(r#"directional"#),
                doc: r#"`directional`, `point` or `spot`."#,
            },
            Field {
                ts: r#"color"#,
                rs: r#"color"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[1.0, 1.0, 1.0, 1.0]),
                doc: r#""#,
            },
            Field {
                ts: r#"intensity"#,
                rs: r#"intensity"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#""#,
            },
            Field {
                ts: r#"range"#,
                rs: r#"range"#,
                ty: r#"f64"#,
                default: Dv::Float(10.0),
                doc: r#"How far point and spot lights reach (metres)."#,
            },
            Field {
                ts: r#"spotAngle"#,
                rs: r#"spot_angle"#,
                ty: r#"f64"#,
                default: Dv::Float(30.0),
                doc: r#"A spot light's full cone angle in degrees."#,
            },
            Field {
                ts: r#"innerSpotAngle"#,
                rs: r#"inner_spot_angle"#,
                ty: r#"f64"#,
                default: Dv::Float(21.8),
                doc: r#"Where a spot light starts fading towards its edge (degrees, full angle)."#,
            },
            Field {
                ts: r#"haloSize"#,
                rs: r#"halo_size"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"A glow around the light this many metres across, in its colour (0: none)."#,
            },
            Field {
                ts: r#"beamIntensity"#,
                rs: r#"beam_intensity"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"A spot light's visible beam as long as `range` (0: none)."#,
            },
            Field {
                ts: r#"shadows"#,
                rs: r#"shadows"#,
                ty: r#"enum:none|hard|soft"#,
                default: Dv::Str(r#"none"#),
                doc: r#"Shadows it casts (Unity's `LightShadows`)."#,
            },
            Field {
                ts: r#"shadowStrength"#,
                rs: r#"shadow_strength"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#"How dark its shadows are, 0 to 1."#,
            },
            Field {
                ts: r#"cullingMask"#,
                rs: r#"culling_mask"#,
                ty: r#"mask"#,
                default: Dv::Int(4294967295),
                doc: r#"The layers it lights, a bit each (all by default)."#,
            },
            Field {
                ts: r#"volumetricIntensity"#,
                rs: r#"volumetric_intensity"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#"How strongly it lights the fog for light shafts (`RenderSettings.volumetricFog`): 1 as brightly as it lights surfaces, 0 not at all (HDRP's volumetric multiplier)."#,
            },
            Field {
                ts: r#"spotShape"#,
                rs: r#"spot_shape"#,
                ty: r#"enum:cone|pyramid"#,
                default: Dv::Str(r#"cone"#),
                doc: r#"A spot light's shape (HDRP's): a `cone`, or a `pyramid` `spotAngle` high and `aspectRatio` times as wide (in tangent), its width along the object's x."#,
            },
            Field {
                ts: r#"aspectRatio"#,
                rs: r#"aspect_ratio"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#"A pyramid spot light's width over its height."#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[r#"v3"#],
        items: None,
        api: Some(
            r#"
        /// How many lights exist.
        pub fn count() -> f64 {
            super::lights().len() as f64
        }

        /// The `index`-th light, in the order they were made (none past the end).
        pub fn at(index: f64) -> LightRef {
            super::lights()
                .get(index as usize)
                .map_or(LightRef(0), |(r, _)| *r)
        }

        /// A light meter: how much light falls on `point` from every light (lux in physical light units), facing `normal` if given. Shadows are not counted.
        #[optional(normal)]
        #[like]
        pub fn illuminance_at(point: V3, normal: Option<V3>) -> f64 {
            crate::light_meter::illuminance(point, normal, false)
        }

        /// A light meter that counts shadows: each light only if a physics ray from `point` reaches it (walls need colliders). Resolves after the next physics step.
        #[optional(normal)]
        #[like]
        #[promise]
        pub fn measure_illuminance(
            point: V3,
            normal: Option<V3>,
            resolve: Callback<(f64,)>,
            reject: Callback<(String,)>,
        ) {
            // Nothing makes it fail.
            drop(reject);
            platypus_physics_api::physics_api::after_next_step(Callback::from_fn(move |()| {
                resolve.call((crate::light_meter::illuminance(point, normal, true),));
            }));
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"FogVolume"#,
        low: r#"fog_volume"#,
        reference: r#"FogVolumeRef"#,
        store: r#"FOG_VOLUMES"#,
        data: r#"FogVolumeData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"size"#,
                rs: r#"size"#,
                ty: r#"v3"#,
                default: Dv::Tuple(&[10.0, 2.0, 10.0]),
                doc: r#"The box's size in metres, before the object's scale."#,
            },
            Field {
                ts: r#"density"#,
                rs: r#"density"#,
                ty: r#"f64"#,
                default: Dv::Float(0.5),
                doc: r#"How thick it is inside, per metre (0.5: a metre of it hides 40% of what is behind)."#,
            },
            Field {
                ts: r#"color"#,
                rs: r#"color"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[0.75, 0.78, 0.82, 1.0]),
                doc: r#"Its colour (linear)."#,
            },
            Field {
                ts: r#"blendDistance"#,
                rs: r#"blend_distance"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#"How far in from its faces it fades in, in metres (0: hard edges)."#,
            },
            Field {
                ts: r#"shape"#,
                rs: r#"shape"#,
                ty: r#"enum:box|ellipsoid"#,
                default: Dv::Str(r#"box"#),
                doc: r#"`box`, or `ellipsoid`: a soft round blob inside the box (mist over a pond)."#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: None,
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"PlaneRenderer"#,
        low: r#"plane_renderer"#,
        reference: r#"PlaneRendererRef"#,
        store: r#"PLANES"#,
        data: r#"PlaneData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"size"#,
                rs: r#"size"#,
                ty: r#"v2"#,
                default: Dv::Tuple(&[1.0, 1.0]),
                doc: r#"Metres across."#,
            },
            Field {
                ts: r#"color"#,
                rs: r#"color"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[1.0, 1.0, 1.0, 1.0]),
                doc: r#""#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: None,
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"Mirror"#,
        low: r#"mirror"#,
        reference: r#"MirrorRef"#,
        store: r#"MIRRORS"#,
        data: r#"MirrorData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"hasNormal"#,
                rs: r#"has_normal"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#"Whether `normal` was set; otherwise up (+y) for planes and +z for quads."#,
            },
            Field {
                ts: r#"normal"#,
                rs: r#"normal"#,
                ty: r#"v3"#,
                default: Dv::Tuple(&[0.0, 1.0, 0.0]),
                doc: r#"The plane's normal in the object's space."#,
            },
            Field {
                ts: r#"cullingMask"#,
                rs: r#"culling_mask"#,
                ty: r#"mask"#,
                default: Dv::Int(4294967295),
                doc: r#"The layers it reflects, a bit each (all by default; set before it starts)."#,
            },
        ],
        skip_save: &[r#"hasNormal"#, r#"normal"#],
        save_if_changed: &[],
        set_hook: &[(r#"normal"#, r#"d.has_normal = true;"#)],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: Some(
            r#"m.insert("normal".into(), if d.has_normal { json!({ "$type": "Vector3", "value": [d.normal.x, d.normal.y, d.normal.z] }) } else { Value::Null });"#,
        ),
        extra_load: Some(
            r#""normal" => { if v.is_null() { d.has_normal = false; Some(()) } else { v3_of(v).map(|n| { d.has_normal = true; d.normal = n; }) } }"#,
        ),
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: None,
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"ReflectionProbe"#,
        low: r#"reflection_probe"#,
        reference: r#"ReflectionProbeRef"#,
        store: r#"PROBES"#,
        data: r#"ProbeData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"size"#,
                rs: r#"size"#,
                ty: r#"v3"#,
                default: Dv::Tuple(&[10.0, 10.0, 10.0]),
                doc: r#"The box of influence, centred on the object (metres)."#,
            },
            Field {
                ts: r#"resolution"#,
                rs: r#"resolution"#,
                ty: r#"f64"#,
                default: Dv::Float(128.0),
                doc: r#"The cubemap's face size in texels (a power of two, 16 to 1024)."#,
            },
            Field {
                ts: r#"boxProjection"#,
                rs: r#"box_projection"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#"Correct reflections for the box's walls (rooms)."#,
            },
            Field {
                ts: r#"cullingMask"#,
                rs: r#"culling_mask"#,
                ty: r#"mask"#,
                default: Dv::Int(4294967295),
                doc: r#"The layers it captures, a bit each (all by default; set before it starts)."#,
            },
            Field {
                ts: r#"refreshMode"#,
                rs: r#"refresh_mode"#,
                ty: r#"enum:onAwake|everyFrame|viaScripting"#,
                default: Dv::Str(r#"onAwake"#),
                doc: r#"When it renders: once, every frame, or on `renderProbe()`."#,
            },
            Field {
                ts: r#"renderRequests"#,
                rs: r#"render_requests"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"How many times `renderProbe()` was called (the host renders when it grows)."#,
            },
        ],
        skip_save: &[r#"renderRequests"#],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: Some(
            r#"
        /// Captures the scene again on the next frame (after moving things, say).
        pub fn render_probe(self) {
            with_reflection_probe(self, |d| d.render_requests += 1.0);
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"WorldPanel"#,
        low: r#"world_panel"#,
        reference: r#"WorldPanelRef"#,
        store: r#"PANELS"#,
        data: r#"PanelData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"width"#,
                rs: r#"width"#,
                ty: r#"f64"#,
                default: Dv::Float(800.0),
                doc: r#"Width in UI pixels."#,
            },
            Field {
                ts: r#"height"#,
                rs: r#"height"#,
                ty: r#"f64"#,
                default: Dv::Float(600.0),
                doc: r#"Height in UI pixels."#,
            },
            Field {
                ts: r#"pixelsPerMeter"#,
                rs: r#"pixels_per_meter"#,
                ty: r#"f64"#,
                default: Dv::Float(500.0),
                doc: r#"UI pixels per metre: 500 makes an 800x600 panel 1.6 x 1.2 m."#,
            },
            Field {
                ts: r#"resolution"#,
                rs: r#"resolution"#,
                ty: r#"f64"#,
                default: Dv::Float(2.0),
                doc: r#"Texture pixels per UI pixel: higher keeps text sharp up close."#,
            },
            Field {
                ts: r#"tint"#,
                rs: r#"tint"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[1.0, 1.0, 1.0, 1.0]),
                doc: r#"A colour multiplied over the panel (alpha fades it)."#,
            },
            Field {
                ts: r#"interactive"#,
                rs: r#"interactive"#,
                ty: r#"bool"#,
                default: Dv::Bool(true),
                doc: r#"Whether the pointer can use it."#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: Some(
            r#"
        /// Its width in metres.
        #[property]
        pub fn world_width(self) -> f64 {
            with_world_panel(self, |d| d.width / d.pixels_per_meter).unwrap_or(0.0)
        }

        /// Its height in metres.
        #[property]
        pub fn world_height(self) -> f64 {
            with_world_panel(self, |d| d.height / d.pixels_per_meter).unwrap_or(0.0)
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"SpriteRenderer"#,
        low: r#"sprite_renderer"#,
        reference: r#"SpriteRendererRef"#,
        store: r#"SPRITE_RENDERERS"#,
        data: r#"SpriteRendererData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"sprite"#,
                rs: r#"sprite"#,
                ty: r#"sprite"#,
                default: Dv::Int(0),
                doc: r#""#,
            },
            Field {
                ts: r#"color"#,
                rs: r#"color"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[1.0, 1.0, 1.0, 1.0]),
                doc: r#""#,
            },
            Field {
                ts: r#"flipX"#,
                rs: r#"flip_x"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#""#,
            },
            Field {
                ts: r#"flipY"#,
                rs: r#"flip_y"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#""#,
            },
            Field {
                ts: r#"sortingOrder"#,
                rs: r#"sorting_order"#,
                ty: r#"i32"#,
                default: Dv::Int(0),
                doc: r#"Draw order: higher draws over lower, whatever the distance (Unity's `sortingOrder`)."#,
            },
            Field {
                ts: r#"drawMode"#,
                rs: r#"draw_mode"#,
                ty: r#"enum:simple|sliced|tiled"#,
                default: Dv::Str(r#"simple"#),
                doc: r#"How the sprite fills its area (Unity's `SpriteDrawMode`): `simple` is one quad at its own size, `sliced` keeps the sprite's `border` corners and stretches the rest, `tiled` repeats it across `size`, cut off at the top and right."#,
            },
            Field {
                ts: r#"size"#,
                rs: r#"size"#,
                ty: r#"v2"#,
                default: Dv::Tuple(&[0.0, 0.0]),
                doc: r#"The area a sliced or tiled sprite fills, in metres; (0, 0) is the sprite's own size."#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[r#"drawMode"#, r#"size"#],
        set_hook: &[],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: None,
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"LineRenderer2D"#,
        low: r#"line_renderer2d"#,
        reference: r#"LineRenderer2DRef"#,
        store: r#"STROKES"#,
        data: r#"StrokeData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"width"#,
                rs: r#"width"#,
                ty: r#"f64"#,
                default: Dv::Float(0.05),
                doc: r#"Full width, in world units."#,
            },
            Field {
                ts: r#"color"#,
                rs: r#"color"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[1.0, 1.0, 1.0, 1.0]),
                doc: r#""#,
            },
            Field {
                ts: r#"loop"#,
                rs: r#"r#loop"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#"Joins the last point back to the first (a closed shape)."#,
            },
            Field {
                ts: r#"fillColor"#,
                rs: r#"fill_color"#,
                ty: r#"color?"#,
                default: Dv::None,
                doc: r#"Fills the shape in this colour (null: no fill); a filled shape always closes."#,
            },
            Field {
                ts: r#"fillRule"#,
                rs: r#"fill_rule"#,
                ty: r#"enum:evenodd|nonzero"#,
                default: Dv::Str(r#"evenodd"#),
                doc: r#"Where a self-crossing outline is inside: `evenodd` leaves a star's middle empty (GDI's `Alternate`), `nonzero` fills it (`Winding`)."#,
            },
            Field {
                ts: r#"sortingOrder"#,
                rs: r#"sorting_order"#,
                ty: r#"i32"#,
                default: Dv::Int(0),
                doc: r#"Draw order among 2D shapes: higher draws over lower, equal ones in the order they were made."#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[r#"pub points: Vec<V2>,"#],
        extra_defaults: &[r#"points: Vec::new(),"#],
        extra_save: Some(
            r#"m.insert("points".into(), Value::Array(d.points.iter().map(|p| json!({ "$type": "Vector2", "value": [p.x, p.y] })).collect()));"#,
        ),
        extra_load: Some(
            r#""points" => v.as_array().and_then(|a| a.iter().map(v2_of).collect::<Option<Vec<V2>>>()).map(|p| d.points = p)"#,
        ),
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[r#"v2"#],
        items: None,
        api: Some(
            r#"
        /// The points (on the object's XY plane): a copy, so write a new array to change them.
        #[property]
        pub fn points(self) -> Vec<V2> {
            with_line_renderer2d(self, |d| d.points.clone()).unwrap_or_default()
        }

        /// Replaces the points (on the object's XY plane); a new array redraws it.
        #[like]
        pub fn set_points(self, points: &[V2]) {
            with_line_renderer2d(self, |d| d.points = points.to_vec());
        }

        #[property]
        pub fn point_count(self) -> f64 {
            with_line_renderer2d(self, |d| d.points.len() as f64).unwrap_or(0.0)
        }

        /// The i-th point (the origin past the end).
        pub fn point_at(self, index: f64) -> V2 {
            with_line_renderer2d(self, |d| d.points.get(index as usize).copied())
                .flatten()
                .unwrap_or(V2 { x: 0.0, y: 0.0 })
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"TextMesh"#,
        low: r#"text_mesh"#,
        reference: r#"TextMeshRef"#,
        store: r#"TEXT_MESHES"#,
        data: r#"TextMeshData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"text"#,
                rs: r#"text"#,
                ty: r#"string"#,
                default: Dv::Str(r#""#),
                doc: r#"The text; a line break starts a new line."#,
            },
            Field {
                ts: r#"fontSize"#,
                rs: r#"font_size"#,
                ty: r#"f64"#,
                default: Dv::Float(32.0),
                doc: r#"In UI pixels."#,
            },
            Field {
                ts: r#"color"#,
                rs: r#"color"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[1.0, 1.0, 1.0, 1.0]),
                doc: r#""#,
            },
            Field {
                ts: r#"font"#,
                rs: r#"font"#,
                ty: r#"string"#,
                default: Dv::Str(r#""#),
                doc: r#"A family loaded with `loadFont` (empty: Platypus's default, Inter)."#,
            },
            Field {
                ts: r#"fontWeight"#,
                rs: r#"font_weight"#,
                ty: r#"f64"#,
                default: Dv::Float(400.0),
                doc: r#"100 to 900; 400 is regular, 700 bold."#,
            },
            Field {
                ts: r#"alignment"#,
                rs: r#"alignment"#,
                ty: r#"enum:left|center|right"#,
                default: Dv::Str(r#"center"#),
                doc: r#"How lines line up with each other."#,
            },
            Field {
                ts: r#"maxWidth"#,
                rs: r#"max_width"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"Wraps lines longer than this many UI pixels (0: only at line breaks)."#,
            },
            Field {
                ts: r#"pixelsPerUnit"#,
                rs: r#"pixels_per_unit"#,
                ty: r#"f64"#,
                default: Dv::Float(100.0),
                doc: r#"UI pixels per world unit (like a sprite's pixels per unit)."#,
            },
            Field {
                ts: r#"resolution"#,
                rs: r#"resolution"#,
                ty: r#"f64"#,
                default: Dv::Float(2.0),
                doc: r#"Texture pixels per UI pixel: more is sharper and costs more memory."#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: None,
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"MeshRenderer"#,
        low: r#"mesh_renderer"#,
        reference: r#"MeshRendererRef"#,
        store: r#"MESH_RENDERERS"#,
        data: r#"MeshRendererData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"mesh"#,
                rs: r#"mesh"#,
                ty: r#"mesh"#,
                default: Dv::Str(r#"=primitive_mesh("cube").0"#),
                doc: r#"A built-in mesh by name (`mesh: "cube"`) or a script `Mesh`."#,
            },
            Field {
                ts: r#"material"#,
                rs: r#"material"#,
                ty: r#"material"#,
                default: Dv::Str(r#"=create_material().0"#),
                doc: r#""#,
            },
            Field {
                ts: r#"shadowCastingMode"#,
                rs: r#"shadow_casting_mode"#,
                ty: r#"enum:on|off"#,
                default: Dv::Str(r#"on"#),
                doc: r#"Whether it casts shadows (Unity's `shadowCastingMode`: `on` or `off`)."#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: Some(
            r#"
        /// Whether the last frame drawn saw it: in a camera's view, or casting a shadow that can
        /// fall in one (Unity's `Renderer.isVisible`). Asking starts watching it: false until a
        /// frame has been drawn since.
        #[property]
        pub fn is_visible(self) -> bool {
            crate::is_visible(crate::RendererId::Mesh(self.0))
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: Some(r#"!internal_renderer(*r)"#),
    },
    Component {
        name: r#"SkinnedMeshRenderer"#,
        low: r#"skinned_mesh_renderer"#,
        reference: r#"SkinnedMeshRendererRef"#,
        store: r#"SKINNED"#,
        data: r#"SkinnedData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"mesh"#,
                rs: r#"mesh"#,
                ty: r#"mesh"#,
                default: Dv::Str(r#"=primitive_mesh("cube").0"#),
                doc: r#"The mesh (with bone weights and bind poses)."#,
            },
            Field {
                ts: r#"material"#,
                rs: r#"material"#,
                ty: r#"material"#,
                default: Dv::Str(r#"=create_material().0"#),
                doc: r#""#,
            },
            Field {
                ts: r#"shadowCastingMode"#,
                rs: r#"shadow_casting_mode"#,
                ty: r#"enum:on|off"#,
                default: Dv::Str(r#"on"#),
                doc: r#"Whether it casts shadows."#,
            },
            Field {
                ts: r#"rootBone"#,
                rs: r#"root_bone"#,
                ty: r#"object"#,
                default: Dv::None,
                doc: r#"The skeleton's top bone."#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[r#"pub bones: Vec<ObjectId>,"#],
        extra_defaults: &[r#"bones: Vec::new(),"#],
        extra_save: Some(
            r#"m.insert("bones".into(), Value::Array(d.bones.iter().map(|b| object_ref(b.to_raw())).collect()));"#,
        ),
        extra_load: Some(
            r#""bones" => v.as_array().and_then(|a| a.iter().map(|x| object_of(x).flatten()).collect::<Option<Vec<ObjectId>>>()).map(|b| d.bones = b)"#,
        ),
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: Some(
            r#"
        /// Appends a bone: the objects the mesh's bones follow, in the order its bone weights and
        /// bind poses use. Not a `MeshRenderer` to `getComponent`.
        pub fn add_bone(self, bone: ObjectId) {
            with_skinned_mesh_renderer(self, |d| d.bones.push(bone));
        }

        pub fn clear_bones(self) {
            with_skinned_mesh_renderer(self, |d| d.bones.clear());
        }

        /// The bones, in the order the mesh's bone weights and bind poses use.
        #[property]
        pub fn bones(self) -> Vec<ObjectId> {
            with_skinned_mesh_renderer(self, |d| d.bones.clone()).unwrap_or_default()
        }

        pub fn set_bones(self, bones: &[ObjectId]) {
            with_skinned_mesh_renderer(self, |d| d.bones = bones.to_vec());
        }

        #[property]
        pub fn bone_count(self) -> f64 {
            with_skinned_mesh_renderer(self, |d| d.bones.len() as f64).unwrap_or(0.0)
        }

        /// The i-th bone (no object past the end).
        pub fn bone_at(self, index: f64) -> ObjectId {
            with_skinned_mesh_renderer(self, |d| d.bones.get(index as usize).copied())
                .flatten()
                .unwrap_or_else(none_object)
        }

        /// Whether the last frame drawn saw it: in a camera's view (judged by a box holding
        /// every bone's image of its bind pose), or casting a shadow that can fall in one
        /// (Unity's `Renderer.isVisible`). Asking starts watching it: false until a frame has
        /// been drawn since.
        #[property]
        pub fn is_visible(self) -> bool {
            crate::is_visible(crate::RendererId::Skinned(self.0))
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"LODGroup"#,
        low: r#"lod_group"#,
        reference: r#"LODGroupRef"#,
        store: r#"LOD_GROUPS"#,
        data: r#"LodData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"size"#,
                rs: r#"size"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#"The group's size in metres before its object's scale (its height, say)."#,
            },
            Field {
                ts: r#"forced"#,
                rs: r#"forced"#,
                ty: r#"i32"#,
                default: Dv::Int(-1),
                doc: r#"The level always shown (-1: chosen by distance)."#,
            },
        ],
        skip_save: &[r#"forced"#],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[
            r#"/// Per level: the screen height below which the next level takes over, and its renderers."#,
            r#"pub lods: Vec<(f64, Vec<u32>)>,"#,
        ],
        extra_defaults: &[r#"lods: Vec::new(),"#],
        extra_save: Some(
            r#"m.insert("lods".into(), Value::Array(d.lods.iter().map(|(h, rs)| json!({ "screenRelativeTransitionHeight": h, "renderers": rs.iter().map(|r| component_ref("MeshRenderer", u64::from(*r))).collect::<Vec<_>>() })).collect()));"#,
        ),
        extra_load: Some(
            r#""lods" => v.as_array().and_then(|a| a.iter().map(|l| { let h = l.get("screenRelativeTransitionHeight")?.as_f64()?; let rs = l.get("renderers")?.as_array()?.iter().map(|r| r.get("handle").and_then(Value::as_u64).map(|h| h as u32)).collect::<Option<Vec<u32>>>()?; Some((h, rs)) }).collect::<Option<Vec<_>>>()).map(|l| d.lods = l)"#,
        ),
        extra_handles: &[r#"MeshRenderer = MeshRendererRef"#],
        extra_records: &[
            r#"LodLevel = LodLevel [screen_relative_transition_height: f64, renderers: Vec<MeshRenderer>]"#,
        ],
        extra_value_types: &[],
        items: Some(
            r#"/// One level of a LOD group: shown down to a screen height, with its renderers.
#[derive(Clone, Debug, PartialEq)]
pub struct LodLevel {
    pub screen_relative_transition_height: f64,
    pub renderers: Vec<MeshRendererRef>,
}"#,
        ),
        api: Some(
            r#"
        /// Replaces the levels, nearest first (a level whose height is above the one before is
        /// ignored). Returns the group.
        #[js_name = "setLODs"]
        pub fn replace_lods(self, levels: &[LodLevel]) -> LODGroupRef {
            with_lod_group(self, |d| {
                d.lods.clear();
                for l in levels {
                    if d.lods.last().is_none_or(|p| l.screen_relative_transition_height <= p.0) {
                        d.lods.push((
                            l.screen_relative_transition_height,
                            l.renderers.iter().map(|r| r.0).collect(),
                        ));
                    }
                }
            });
            self
        }

        /// Removes every level.
        pub fn clear_lods(self) {
            with_lod_group(self, |d| d.lods.clear());
        }

        /// Adds a level after the others, shown down to this screen height (a fraction of the
        /// view's height, 0 to 1). Heights must go down from the first level: a higher one is
        /// ignored.
        pub fn add_lod(self, screen_relative_transition_height: f64) {
            with_lod_group(self, |d| {
                if d.lods.last().is_none_or(|l| screen_relative_transition_height <= l.0) {
                    d.lods.push((screen_relative_transition_height, Vec::new()));
                }
            });
        }

        /// Adds a renderer to a level.
        pub fn add_lod_renderer(self, lod: f64, renderer: MeshRendererRef) {
            with_lod_group(self, |d| {
                if let Some(l) = d.lods.get_mut(lod as usize) {
                    l.1.push(renderer.0);
                }
            });
        }

        #[property]
        pub fn lod_count(self) -> f64 {
            with_lod_group(self, |d| d.lods.len() as f64).unwrap_or(0.0)
        }

        /// The transition height of a level.
        pub fn lod_height(self, lod: f64) -> f64 {
            with_lod_group(self, |d| d.lods.get(lod as usize).map(|l| l.0))
                .flatten()
                .unwrap_or(0.0)
        }

        pub fn lod_renderer_count(self, lod: f64) -> f64 {
            with_lod_group(self, |d| d.lods.get(lod as usize).map(|l| l.1.len() as f64))
                .flatten()
                .unwrap_or(0.0)
        }

        pub fn lod_renderer_at(self, lod: f64, index: f64) -> MeshRendererRef {
            MeshRendererRef(
                with_lod_group(self, |d| {
                    d.lods
                        .get(lod as usize)
                        .and_then(|l| l.1.get(index as usize))
                        .copied()
                })
                .flatten()
                .unwrap_or(0),
            )
        }

        /// Always shows this level (-1: back to choosing by distance).
        pub fn force_lod(self, index: f64) {
            with_lod_group(self, |d| d.forced = index as i32);
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"CameraFollow"#,
        low: r#"camera_follow"#,
        reference: r#"CameraFollowRef"#,
        store: r#"FOLLOWS"#,
        data: r#"FollowData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"target"#,
                rs: r#"target"#,
                ty: r#"object"#,
                default: Dv::None,
                doc: r#"The object to follow."#,
            },
            Field {
                ts: r#"offset"#,
                rs: r#"offset"#,
                ty: r#"v3"#,
                default: Dv::Tuple(&[0.0, 3.0, 6.0]),
                doc: r#"Where to sit, from the target."#,
            },
            Field {
                ts: r#"offsetInTargetSpace"#,
                rs: r#"offset_in_target_space"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#"Rotate the offset with the target (behind a car), or keep it in world space."#,
            },
            Field {
                ts: r#"smoothTime"#,
                rs: r#"smooth_time"#,
                ty: r#"f64"#,
                default: Dv::Float(0.25),
                doc: r#"About how long catching up takes, in seconds (0: instantly)."#,
            },
            Field {
                ts: r#"lookAtTarget"#,
                rs: r#"look_at_target"#,
                ty: r#"bool"#,
                default: Dv::Bool(true),
                doc: r#""#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[r#"pub velocity: [f64; 3],"#],
        extra_defaults: &[r#"velocity: [0.0; 3],"#],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: Some(
            r#"
/// Runs every `CameraFollow`: follows its target from an offset, easing into place, and looks at
/// it. The host calls it once a frame after the other late updates (`onLateUpdate`).
pub fn late_update_camera_follows(dt: f64) {
    use platypus_math::mathf_api::smooth_damp;
    use platypus_scene::api::scene_api as scene;
    for (r, d) in camera_follows() {
        let Some(target) = d.target.filter(|t| platypus_scene::api::with_scene(|s| s.contains(*t)).unwrap_or(false)) else {
            continue;
        };
        let offset = if d.offset_in_target_space {
            scene::rotation(target).rotate(d.offset)
        } else {
            d.offset
        };
        let target_pos = scene::position(target);
        let goal = target_pos.add(offset);
        let at = scene::position(d.object);
        if d.smooth_time <= 0.0 {
            scene::set_position(d.object, goal);
        } else {
            let dt = dt.max(1e-4);
            let mut velocity = d.velocity;
            let mut axis = |i: usize, a: f64, b: f64| {
                let r = smooth_damp(a, b, velocity[i], d.smooth_time, dt, f64::NAN);
                velocity[i] = r.y;
                r.x
            };
            let p = V3 { x: axis(0, at.x, goal.x), y: axis(1, at.y, goal.y), z: axis(2, at.z, goal.z) };
            scene::set_position(d.object, p);
            with_camera_follow(r, |d| d.velocity = velocity);
        }
        if d.look_at_target {
            platypus_scene::api::TransformRef(d.object).look_at(target_pos);
        }
    }
}
"#,
        ),
        api: None,
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"CameraOrbit"#,
        low: r#"camera_orbit"#,
        reference: r#"CameraOrbitRef"#,
        store: r#"ORBITS"#,
        data: r#"OrbitData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"target"#,
                rs: r#"target"#,
                ty: r#"object"#,
                default: Dv::None,
                doc: r#"Orbits this object (or `center` when there is none)."#,
            },
            Field {
                ts: r#"center"#,
                rs: r#"center"#,
                ty: r#"v3"#,
                default: Dv::Tuple(&[0.0, 0.0, 0.0]),
                doc: r#"Orbits this point when there is no target."#,
            },
            Field {
                ts: r#"distance"#,
                rs: r#"distance"#,
                ty: r#"f64"#,
                default: Dv::Float(8.0),
                doc: r#""#,
            },
            Field {
                ts: r#"yaw"#,
                rs: r#"yaw"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"Degrees."#,
            },
            Field {
                ts: r#"pitch"#,
                rs: r#"pitch"#,
                ty: r#"f64"#,
                default: Dv::Float(20.0),
                doc: r#"Degrees."#,
            },
            Field {
                ts: r#"minPitch"#,
                rs: r#"min_pitch"#,
                ty: r#"f64"#,
                default: Dv::Float(-80.0),
                doc: r#""#,
            },
            Field {
                ts: r#"maxPitch"#,
                rs: r#"max_pitch"#,
                ty: r#"f64"#,
                default: Dv::Float(80.0),
                doc: r#""#,
            },
            Field {
                ts: r#"autoRotate"#,
                rs: r#"auto_rotate"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"Degrees per second."#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: None,
        extra_load: None,
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: Some(
            r#"
/// Runs every `CameraOrbit` (once a frame after the other late updates).
pub fn late_update_camera_orbits(dt: f64) {
    use platypus_math::{Q, mathf_api::repeat};
    use platypus_scene::api::scene_api as scene;
    for (r, d) in camera_orbits() {
        let yaw = repeat(d.yaw + d.auto_rotate * dt, 360.0);
        let pitch = d.pitch.clamp(d.min_pitch.min(d.max_pitch), d.max_pitch.max(d.min_pitch));
        with_camera_orbit(r, |d| {
            d.yaw = yaw;
            d.pitch = pitch;
        });
        let center = match d.target {
            Some(t) if platypus_scene::api::with_scene(|s| s.contains(t)).unwrap_or(false) => scene::position(t),
            _ => d.center,
        };
        let rotation = Q::euler(-pitch, yaw, 0.0);
        let back = V3 { x: 0.0, y: 0.0, z: 1.0 };
        scene::set_position(d.object, center.add(rotation.rotate(back).scale(d.distance)));
        platypus_scene::api::TransformRef(d.object).look_at(center);
    }
}
"#,
        ),
        api: None,
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"ParticleSystem"#,
        low: r#"particle_system"#,
        reference: r#"ParticleSystemRef"#,
        store: r#"PARTICLES"#,
        data: r#"ParticleData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"duration"#,
                rs: r#"duration"#,
                ty: r#"f64"#,
                default: Dv::Float(5.0),
                doc: r#"Seconds per cycle."#,
            },
            Field {
                ts: r#"loop"#,
                rs: r#"r#loop"#,
                ty: r#"bool"#,
                default: Dv::Bool(true),
                doc: r#""#,
            },
            Field {
                ts: r#"playOnAwake"#,
                rs: r#"play_on_awake"#,
                ty: r#"bool"#,
                default: Dv::Bool(true),
                doc: r#""#,
            },
            Field {
                ts: r#"startDelay"#,
                rs: r#"start_delay"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#""#,
            },
            Field {
                ts: r#"gravityModifier"#,
                rs: r#"gravity_modifier"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"Multiplies gravity (9.81 m/s2 down)."#,
            },
            Field {
                ts: r#"drag"#,
                rs: r#"drag"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"Fraction of speed lost per second."#,
            },
            Field {
                ts: r#"simulationSpace"#,
                rs: r#"simulation_space"#,
                ty: r#"enum:local|world"#,
                default: Dv::Str(r#"local"#),
                doc: r#"`world`: particles stay where they were emitted when the object moves."#,
            },
            Field {
                ts: r#"maxParticles"#,
                rs: r#"max_particles"#,
                ty: r#"i32"#,
                default: Dv::Int(1000),
                doc: r#""#,
            },
            Field {
                ts: r#"randomSeed"#,
                rs: r#"random_seed"#,
                ty: r#"i32"#,
                default: Dv::Int(1),
                doc: r#""#,
            },
            Field {
                ts: r#"rateOverTime"#,
                rs: r#"rate_over_time"#,
                ty: r#"f64"#,
                default: Dv::Float(10.0),
                doc: r#"Particles emitted per second."#,
            },
            Field {
                ts: r#"texture"#,
                rs: r#"texture"#,
                ty: r#"texture"#,
                default: Dv::Int(0),
                doc: r#"The particle image (a soft round spot by default)."#,
            },
            Field {
                ts: r#"blendMode"#,
                rs: r#"blend_mode"#,
                ty: r#"enum:alpha|additive"#,
                default: Dv::Str(r#"alpha"#),
                doc: r#"`additive` makes them glow."#,
            },
            Field {
                ts: r#"stopAction"#,
                rs: r#"stop_action"#,
                ty: r#"enum:none|destroy"#,
                default: Dv::Str(r#"none"#),
                doc: r#"What happens once it has stopped and its particles are gone."#,
            },
            Field {
                ts: r#"shapeType"#,
                rs: r#"shape_type"#,
                ty: r#"enum:point|sphere|hemisphere|cone|box"#,
                default: Dv::Str(r#"cone"#),
                doc: r#"Where particles start; they go along the object's up (+y)."#,
            },
            Field {
                ts: r#"shapeRadius"#,
                rs: r#"shape_radius"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#"Sphere, hemisphere and cone."#,
            },
            Field {
                ts: r#"shapeAngle"#,
                rs: r#"shape_angle"#,
                ty: r#"f64"#,
                default: Dv::Float(25.0),
                doc: r#"Cone: degrees from straight up."#,
            },
            Field {
                ts: r#"shapeSize"#,
                rs: r#"shape_size"#,
                ty: r#"v3"#,
                default: Dv::Tuple(&[1.0, 1.0, 1.0]),
                doc: r#"Box."#,
            },
        ],
        skip_save: &[
            r#"shapeType"#,
            r#"shapeRadius"#,
            r#"shapeAngle"#,
            r#"shapeSize"#,
        ],
        save_if_changed: &[],
        set_hook: &[(r#"playOnAwake"#, r#"d.alive = v;"#)],
        extra_fields: &[
            r#"pub start_lifetime: [f64; 2],"#,
            r#"pub start_speed: [f64; 2],"#,
            r#"pub start_size: [f64; 2],"#,
            r#"pub start_rotation: [f64; 2],"#,
            r#"pub angular_velocity: [f64; 2],"#,
            r#"pub start_color: Rgba,"#,
            r#"pub start_color2: Option<Rgba>,"#,
            r#"/// time, count, cycles, interval"#,
            r#"pub bursts: Vec<[f64; 4]>,"#,
            r#"/// time, value"#,
            r#"pub size_over_lifetime: Option<Vec<[f64; 2]>>,"#,
            r#"pub color_over_lifetime: Option<Vec<(f64, Rgba)>>,"#,
            r#"/// tilesX, tilesY, cycles"#,
            r#"pub texture_sheet: Option<[f64; 3]>,"#,
            r#"pub alive: bool,"#,
            r#"pub paused: bool,"#,
            r#"pub particle_count: u32,"#,
        ],
        extra_defaults: &[
            r#"start_lifetime: [5.0, 5.0],"#,
            r#"start_speed: [5.0, 5.0],"#,
            r#"start_size: [1.0, 1.0],"#,
            r#"start_rotation: [0.0, 0.0],"#,
            r#"angular_velocity: [0.0, 0.0],"#,
            r#"start_color: Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },"#,
            r#"start_color2: None,"#,
            r#"bursts: Vec::new(),"#,
            r#"size_over_lifetime: None,"#,
            r#"color_over_lifetime: None,"#,
            r#"texture_sheet: None,"#,
            r#"alive: true,"#,
            r#"paused: false,"#,
            r#"particle_count: 0,"#,
        ],
        extra_save: Some(
            r#"for (k, v) in [("startLifetime", d.start_lifetime), ("startSpeed", d.start_speed), ("startSize", d.start_size), ("startRotation", d.start_rotation), ("angularVelocity", d.angular_velocity)] { m.insert(k.into(), minmax_json(v)); }
                m.insert("startColor".into(), start_color_json(d.start_color, d.start_color2, true));
                m.insert("bursts".into(), Value::Array(d.bursts.iter().map(|b| json!({ "time": b[0], "count": b[1], "cycles": b[2], "interval": b[3] })).collect()));
                m.insert("shape".into(), shape_json(&d.shape_type, d.shape_radius, d.shape_angle, d.shape_size));
                m.insert("sizeOverLifetime".into(), d.size_over_lifetime.as_ref().map_or(Value::Null, |k| Value::Array(k.iter().map(|k| json!({ "time": k[0], "value": k[1] })).collect())));
                m.insert("colorOverLifetime".into(), d.color_over_lifetime.as_ref().map_or(Value::Null, |k| Value::Array(k.iter().map(|(t, c)| json!({ "time": t, "color": color_json(*c) })).collect())));
                m.insert("textureSheet".into(), d.texture_sheet.map_or(Value::Null, |s| json!({ "tilesX": s[0], "tilesY": s[1], "cycles": s[2] })));"#,
        ),
        extra_load: Some(
            r#"    "startLifetime" => minmax_of(v).map(|x| d.start_lifetime = x),
                "startSpeed" => minmax_of(v).map(|x| d.start_speed = x),
                "startSize" => minmax_of(v).map(|x| d.start_size = x),
                "startRotation" => minmax_of(v).map(|x| d.start_rotation = x),
                "angularVelocity" => minmax_of(v).map(|x| d.angular_velocity = x),
                "startColor" => start_color_of(v).map(|(a, b)| { d.start_color = a; d.start_color2 = b; }),
                "bursts" => v.as_array().and_then(|a| a.iter().map(|b| Some([b.get("time")?.as_f64()?, b.get("count")?.as_f64()?, b.get("cycles").and_then(Value::as_f64).unwrap_or(1.0), b.get("interval").and_then(Value::as_f64).unwrap_or(0.01)])).collect::<Option<Vec<_>>>()).map(|b| d.bursts = b),
                "shape" => shape_of(v).map(|(t, r, a, s)| { d.shape_type = t; if let Some(r) = r { d.shape_radius = r; } if let Some(a) = a { d.shape_angle = a; } if let Some(s) = s { d.shape_size = s; } }),
                "sizeOverLifetime" => if v.is_null() { d.size_over_lifetime = None; Some(()) } else { v.as_array().and_then(|a| a.iter().map(|k| Some([k.get("time")?.as_f64()?, k.get("value")?.as_f64()?])).collect::<Option<Vec<_>>>()).map(|k| d.size_over_lifetime = Some(k)) },
                "colorOverLifetime" => if v.is_null() { d.color_over_lifetime = None; Some(()) } else { v.as_array().and_then(|a| a.iter().map(|k| Some((k.get("time")?.as_f64()?, color_of(k.get("color")?)?))).collect::<Option<Vec<_>>>()).map(|k| d.color_over_lifetime = Some(k)) },
                "textureSheet" => if v.is_null() { d.texture_sheet = None; Some(()) } else { (|| Some([v.get("tilesX")?.as_f64()?, v.get("tilesY")?.as_f64()?, v.get("cycles").and_then(Value::as_f64).unwrap_or(1.0)]))().map(|s| d.texture_sheet = Some(s)) }"#,
        ),
        extra_handles: &[],
        extra_records: &[
            r#"ParticleBurst = ParticleBurst [time: f64, count: f64, cycles: Option<f64>, interval: Option<f64>]"#,
            r#"ParticleShapeSpec = ShapeSpec [shape_type: string as "type", angle: Option<f64>, radius: Option<f64>, size: Option<Vector3Like>]"#,
            r#"ColorKey = ColorKey [time: f64, color: Color]"#,
            r#"SizeKey = SizeKey [time: f64, value: f64]"#,
        ],
        extra_value_types: &[r#"color"#, r#"v3"#],
        items: Some(
            r#"/// A particle burst as a plain object: `cycles` defaults to 1, `interval` to 0.01.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleBurst {
    pub time: f64,
    pub count: f64,
    pub cycles: Option<f64>,
    pub interval: Option<f64>,
}

/// Where particles start, as a plain object: the keys present are applied.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeSpec {
    pub shape_type: String,
    pub angle: Option<f64>,
    pub radius: Option<f64>,
    pub size: Option<V3>,
}

/// A key of the colour over a particle's life.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorKey {
    pub time: f64,
    pub color: Rgba,
}

/// A key of the size over a particle's life.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeKey {
    pub time: f64,
    pub value: f64,
}"#,
        ),
        api: Some(
            r#"
        /// Seconds a particle lives. A fixed value, or `start_lifetime_max` above it for a random one per particle.
        #[property]
        pub fn start_lifetime(self) -> f64 {
            with_particle_system(self, |d| d.start_lifetime[0]).unwrap_or(0.0)
        }

        /// Sets both ends.
        pub fn set_start_lifetime(self, v: f64) {
            with_particle_system(self, |d| d.start_lifetime = [v, v]);
        }

        #[property]
        pub fn start_lifetime_max(self) -> f64 {
            with_particle_system(self, |d| d.start_lifetime[1]).unwrap_or(0.0)
        }

        /// A random value between `min` and `max` per particle.
        pub fn set_start_lifetime_range(self, min: f64, max: f64) {
            with_particle_system(self, |d| d.start_lifetime = [min, max]);
        }

        /// Metres per second. A fixed value, or `start_speed_max` above it for a random one per particle.
        #[property]
        pub fn start_speed(self) -> f64 {
            with_particle_system(self, |d| d.start_speed[0]).unwrap_or(0.0)
        }

        /// Sets both ends.
        pub fn set_start_speed(self, v: f64) {
            with_particle_system(self, |d| d.start_speed = [v, v]);
        }

        #[property]
        pub fn start_speed_max(self) -> f64 {
            with_particle_system(self, |d| d.start_speed[1]).unwrap_or(0.0)
        }

        /// A random value between `min` and `max` per particle.
        pub fn set_start_speed_range(self, min: f64, max: f64) {
            with_particle_system(self, |d| d.start_speed = [min, max]);
        }

        /// Metres. A fixed value, or `start_size_max` above it for a random one per particle.
        #[property]
        pub fn start_size(self) -> f64 {
            with_particle_system(self, |d| d.start_size[0]).unwrap_or(0.0)
        }

        /// Sets both ends.
        pub fn set_start_size(self, v: f64) {
            with_particle_system(self, |d| d.start_size = [v, v]);
        }

        #[property]
        pub fn start_size_max(self) -> f64 {
            with_particle_system(self, |d| d.start_size[1]).unwrap_or(0.0)
        }

        /// A random value between `min` and `max` per particle.
        pub fn set_start_size_range(self, min: f64, max: f64) {
            with_particle_system(self, |d| d.start_size = [min, max]);
        }

        /// Degrees. A fixed value, or `start_rotation_max` above it for a random one per particle.
        #[property]
        pub fn start_rotation(self) -> f64 {
            with_particle_system(self, |d| d.start_rotation[0]).unwrap_or(0.0)
        }

        /// Sets both ends.
        pub fn set_start_rotation(self, v: f64) {
            with_particle_system(self, |d| d.start_rotation = [v, v]);
        }

        #[property]
        pub fn start_rotation_max(self) -> f64 {
            with_particle_system(self, |d| d.start_rotation[1]).unwrap_or(0.0)
        }

        /// A random value between `min` and `max` per particle.
        pub fn set_start_rotation_range(self, min: f64, max: f64) {
            with_particle_system(self, |d| d.start_rotation = [min, max]);
        }

        /// Degrees per second. A fixed value, or `angular_velocity_max` above it for a random one per particle.
        #[property]
        pub fn angular_velocity(self) -> f64 {
            with_particle_system(self, |d| d.angular_velocity[0]).unwrap_or(0.0)
        }

        /// Sets both ends.
        pub fn set_angular_velocity(self, v: f64) {
            with_particle_system(self, |d| d.angular_velocity = [v, v]);
        }

        #[property]
        pub fn angular_velocity_max(self) -> f64 {
            with_particle_system(self, |d| d.angular_velocity[1]).unwrap_or(0.0)
        }

        /// A random value between `min` and `max` per particle.
        pub fn set_angular_velocity_range(self, min: f64, max: f64) {
            with_particle_system(self, |d| d.angular_velocity = [min, max]);
        }

        /// A colour, or `setStartColorRange` for a random mix of two per particle.
        #[property]
        pub fn start_color(self) -> Rgba {
            with_particle_system(self, |d| d.start_color).unwrap_or(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 })
        }

        pub fn set_start_color(self, c: Rgba) {
            with_particle_system(self, |d| {
                d.start_color = c;
                d.start_color2 = None;
            });
        }

        pub fn set_start_color_range(self, a: Rgba, b: Rgba) {
            with_particle_system(self, |d| {
                d.start_color = a;
                d.start_color2 = Some(b);
            });
        }


        /// `startLifetime = [min, max]`: a random value between the two per particle.
        #[setter(start_lifetime)]
        pub fn set_start_lifetime_list(self, range: &[f64]) {
            if let [min, max, ..] = range {
                self.set_start_lifetime_range(*min, *max);
            }
        }

        /// `startSpeed = [min, max]`: a random value between the two per particle.
        #[setter(start_speed)]
        pub fn set_start_speed_list(self, range: &[f64]) {
            if let [min, max, ..] = range {
                self.set_start_speed_range(*min, *max);
            }
        }

        /// `startSize = [min, max]`: a random value between the two per particle.
        #[setter(start_size)]
        pub fn set_start_size_list(self, range: &[f64]) {
            if let [min, max, ..] = range {
                self.set_start_size_range(*min, *max);
            }
        }

        /// `startRotation = [min, max]`: a random value between the two per particle.
        #[setter(start_rotation)]
        pub fn set_start_rotation_list(self, range: &[f64]) {
            if let [min, max, ..] = range {
                self.set_start_rotation_range(*min, *max);
            }
        }

        /// `angularVelocity = [min, max]`: a random value between the two per particle.
        #[setter(angular_velocity)]
        pub fn set_angular_velocity_list(self, range: &[f64]) {
            if let [min, max, ..] = range {
                self.set_angular_velocity_range(*min, *max);
            }
        }

        /// `startColor = [a, b]`: a random mix of the two per particle.
        #[setter(start_color)]
        pub fn set_start_color_list(self, colors: &[Rgba]) {
            if let [a, b, ..] = colors {
                self.set_start_color_range(*a, *b);
            }
        }

        /// The second colour of a range (the first when there is none).
        #[property]
        pub fn start_color_max(self) -> Rgba {
            with_particle_system(self, |d| d.start_color2.unwrap_or(d.start_color)).unwrap_or(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 })
        }

        /// Adds a burst: `count` particles at `time` seconds into each cycle, firing `cycles`
        /// times (0 keeps firing every `interval`).
        #[optional(cycles = 1, interval = 0.01)]
        pub fn add_burst(self, time: f64, count: f64, cycles: f64, interval: f64) {
            with_particle_system(self, |d| d.bursts.push([time, count, cycles, interval]));
        }

        pub fn clear_bursts(self) {
            with_particle_system(self, |d| d.bursts.clear());
        }

        /// The bursts.
        #[property]
        pub fn bursts(self) -> Vec<ParticleBurst> {
            with_particle_system(self, |d| {
                d.bursts
                    .iter()
                    .map(|b| ParticleBurst { time: b[0], count: b[1], cycles: Some(b[2]), interval: Some(b[3]) })
                    .collect()
            })
            .unwrap_or_default()
        }

        /// Replaces the bursts.
        pub fn set_bursts(self, bursts: &[ParticleBurst]) {
            with_particle_system(self, |d| {
                d.bursts = bursts
                    .iter()
                    .map(|b| [b.time, b.count, b.cycles.unwrap_or(1.0), b.interval.unwrap_or(0.01)])
                    .collect();
            });
        }

        /// Where particles start.
        #[property]
        pub fn shape(self) -> Option<ShapeSpec> {
            with_particle_system(self, |d| ShapeSpec {
                shape_type: d.shape_type.clone(),
                angle: Some(d.shape_angle),
                radius: Some(d.shape_radius),
                size: Some(d.shape_size),
            })
        }

        /// Sets the shape and the keys given; the others keep their values.
        pub fn set_shape(self, spec: Option<ShapeSpec>) {
            let Some(spec) = spec else { return };
            with_particle_system(self, |d| {
                if ["point", "sphere", "hemisphere", "cone", "box"].contains(&spec.shape_type.as_str()) {
                    d.shape_type = spec.shape_type.clone();
                }
                if let Some(a) = spec.angle {
                    d.shape_angle = a;
                }
                if let Some(r) = spec.radius {
                    d.shape_radius = r;
                }
                if let Some(s) = spec.size {
                    d.shape_size = s;
                }
            });
        }

        /// The colour over life (empty for none).
        #[property]
        pub fn color_over_lifetime(self) -> Vec<ColorKey> {
            with_particle_system(self, |d| {
                d.color_over_lifetime
                    .as_ref()
                    .map(|k| k.iter().map(|(time, color)| ColorKey { time: *time, color: *color }).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
        }

        pub fn set_color_over_lifetime(self, keys: &[ColorKey]) {
            with_particle_system(self, |d| {
                d.color_over_lifetime = (!keys.is_empty()).then(|| keys.iter().map(|k| (k.time, k.color)).collect());
            });
        }

        /// The size over life (empty for none).
        #[property]
        pub fn size_over_lifetime(self) -> Vec<SizeKey> {
            with_particle_system(self, |d| {
                d.size_over_lifetime
                    .as_ref()
                    .map(|k| k.iter().map(|k| SizeKey { time: k[0], value: k[1] }).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
        }

        pub fn set_size_over_lifetime(self, keys: &[SizeKey]) {
            with_particle_system(self, |d| {
                d.size_over_lifetime = (!keys.is_empty()).then(|| keys.iter().map(|k| [k.time, k.value]).collect());
            });
        }

        #[property]
        pub fn burst_count(self) -> f64 {
            with_particle_system(self, |d| d.bursts.len() as f64).unwrap_or(0.0)
        }

        /// Adds a key to the size over the particle's life: `time` from 0 (birth) to 1 (death),
        /// linear between keys; `value` multiplies the start size.
        pub fn add_size_key(self, time: f64, value: f64) {
            with_particle_system(self, |d| d.size_over_lifetime.get_or_insert_with(Vec::new).push([time, value]));
        }

        pub fn clear_size_over_lifetime(self) {
            with_particle_system(self, |d| d.size_over_lifetime = None);
        }

        /// Adds a key to the colour over life; it multiplies the start colour (alpha too: fade out
        /// with alpha 0 at time 1).
        pub fn add_color_key(self, time: f64, color: Rgba) {
            with_particle_system(self, |d| d.color_over_lifetime.get_or_insert_with(Vec::new).push((time, color)));
        }

        pub fn clear_color_over_lifetime(self) {
            with_particle_system(self, |d| d.color_over_lifetime = None);
        }

        /// A flipbook: the texture cut into `tilesX` x `tilesY` frames, played `cycles` times per life.
        #[optional(cycles = 1)]
        pub fn set_texture_sheet(self, tiles_x: f64, tiles_y: f64, cycles: f64) {
            with_particle_system(self, |d| d.texture_sheet = Some([tiles_x, tiles_y, cycles]));
        }

        pub fn clear_texture_sheet(self) {
            with_particle_system(self, |d| d.texture_sheet = None);
        }

        /// Live particles (as of the last report).
        #[property]
        pub fn particle_count(self) -> f64 {
            with_particle_system(self, |d| f64::from(d.particle_count)).unwrap_or(0.0)
        }

        /// Emitting or has particles left, and not paused.
        #[property]
        pub fn is_playing(self) -> bool {
            with_particle_system(self, |d| d.alive && !d.paused).unwrap_or(false)
        }

        #[property]
        pub fn is_paused(self) -> bool {
            with_particle_system(self, |d| d.paused).unwrap_or(false)
        }

        /// Starts (or resumes) emitting.
        pub fn play(self) {
            with_particle_system(self, |d| {
                d.paused = false;
                d.alive = true;
            });
            control(self, "play", 0.0);
        }

        /// Stops emitting; with `clear`, removes the live particles too.
        pub fn stop(self, clear: bool) {
            control(self, if clear { "stopClear" } else { "stop" }, 0.0);
        }

        pub fn pause(self) {
            with_particle_system(self, |d| d.paused = true);
            control(self, "pause", 0.0);
        }

        pub fn clear(self) {
            control(self, "clear", 0.0);
        }

        /// Emits `count` particles at once (works while stopped, too).
        pub fn emit(self, count: f64) {
            control(self, "emit", count);
        }

        /// Calls `f()` once it has stopped and its particles are gone.
        pub fn on_stopped(self, f: Callback<()>) {
            add_stopped(self, f);
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"LineRenderer"#,
        low: r#"line_renderer"#,
        reference: r#"LineRendererRef"#,
        store: r#"LINES"#,
        data: r#"LineData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"width"#,
                rs: r#"width"#,
                ty: r#"f64"#,
                default: Dv::Float(0.05),
                doc: r#"Tube thickness (diameter, metres)."#,
            },
            Field {
                ts: r#"sides"#,
                rs: r#"sides"#,
                ty: r#"i32"#,
                default: Dv::Int(8),
                doc: r#"Points around the tube."#,
            },
            Field {
                ts: r#"smoothing"#,
                rs: r#"smoothing"#,
                ty: r#"i32"#,
                default: Dv::Int(3),
                doc: r#"Extra points between each pair of positions, for curves."#,
            },
            Field {
                ts: r#"material"#,
                rs: r#"material"#,
                ty: r#"material"#,
                default: Dv::Str(r#"=create_line_material().0"#),
                doc: r#""#,
            },
        ],
        skip_save: &[],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[
            r#"/// The points it goes through, in world space."#,
            r#"pub positions: Vec<V3>,"#,
        ],
        extra_defaults: &[r#"positions: Vec::new(),"#],
        extra_save: Some(
            r#"m.insert("positions".into(), Value::Array(d.positions.iter().map(|p| v3_json(*p)).collect()));"#,
        ),
        extra_load: Some(
            r#""positions" => v.as_array().and_then(|a| a.iter().map(v3_of).collect::<Option<Vec<V3>>>()).map(|p| d.positions = p)"#,
        ),
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[r#"v3"#],
        items: None,
        api: Some(
            r#"
        /// The points it goes through (world space): a copy, so write a new array to change them.
        #[property]
        pub fn positions(self) -> Vec<V3> {
            with_line_renderer(self, |d| d.positions.clone()).unwrap_or_default()
        }

        /// Replaces the points (world space); a new array redraws the tube.
        #[like]
        pub fn set_positions(self, points: &[V3]) {
            with_line_renderer(self, |d| d.positions = points.to_vec());
        }

        #[property]
        pub fn position_count(self) -> f64 {
            with_line_renderer(self, |d| d.positions.len() as f64).unwrap_or(0.0)
        }

        /// The i-th point (the origin past the end).
        pub fn position_at(self, index: f64) -> V3 {
            with_line_renderer(self, |d| d.positions.get(index as usize).copied())
                .flatten()
                .unwrap_or(V3::ZERO)
        }

        /// Rebuilds the tube now if the points or its look changed (the host does it before each
        /// frame with `pre_render_lines`).
        pub fn redraw(self) {
            line_redraw(self);
        }
"#,
        ),
        remove_hook: Some(r#"line_removed(r);"#),
        awake_hook: Some(r#"line_redraw(r);"#),
        attach_hook: Some(r#"line_attached(r);"#),
        kind_on_filter: None,
    },
    Component {
        name: r#"Cloth"#,
        low: r#"cloth"#,
        reference: r#"ClothRef"#,
        store: r#"CLOTHS_STORE"#,
        data: r#"ClothData"#,
        asset: false,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"stretchingStiffness"#,
                rs: r#"stretching_stiffness"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#"0 (rubbery) to 1 (keeps its edges' lengths)."#,
            },
            Field {
                ts: r#"bendingStiffness"#,
                rs: r#"bending_stiffness"#,
                ty: r#"f64"#,
                default: Dv::Float(0.3),
                doc: r#"0 (folds freely) to 1 (resists folding)."#,
            },
            Field {
                ts: r#"damping"#,
                rs: r#"damping"#,
                ty: r#"f64"#,
                default: Dv::Float(0.1),
                doc: r#"0..1: how much of its motion it loses each second."#,
            },
            Field {
                ts: r#"useGravity"#,
                rs: r#"use_gravity"#,
                ty: r#"bool"#,
                default: Dv::Bool(true),
                doc: r#""#,
            },
            Field {
                ts: r#"externalAcceleration"#,
                rs: r#"external_acceleration"#,
                ty: r#"v3"#,
                default: Dv::Tuple(&[0.0, 0.0, 0.0]),
                doc: r#"A steady acceleration on every vertex (steady wind)."#,
            },
            Field {
                ts: r#"randomAcceleration"#,
                rs: r#"random_acceleration"#,
                ty: r#"v3"#,
                default: Dv::Tuple(&[0.0, 0.0, 0.0]),
                doc: r#"Up to this much more, varying (gusts)."#,
            },
            Field {
                ts: r#"friction"#,
                rs: r#"friction"#,
                ty: r#"f64"#,
                default: Dv::Float(0.5),
                doc: r#"0..1: how much sliding over colliders slows it."#,
            },
            Field {
                ts: r#"solverIterations"#,
                rs: r#"solver_iterations"#,
                ty: r#"f64"#,
                default: Dv::Float(8.0),
                doc: r#"Constraint passes per step: more is stiffer, and slower to run."#,
            },
            Field {
                ts: r#"pinRadius"#,
                rs: r#"pin_radius"#,
                ty: r#"f64"#,
                default: Dv::Float(0.05),
                doc: r#"Vertices within this of the pins stay put."#,
            },
            Field {
                ts: r#"rebuildRequests"#,
                rs: r#"rebuild_requests"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"How many times the simulation was (re)started (the host restarts when it grows)."#,
            },
            Field {
                ts: r#"sourceMesh"#,
                rs: r#"source_mesh"#,
                ty: r#"mesh"#,
                default: Dv::Int(0),
                doc: r#"The mesh the simulation started from."#,
            },
            Field {
                ts: r#"simulatedMesh"#,
                rs: r#"simulated_mesh"#,
                ty: r#"mesh"#,
                default: Dv::Int(0),
                doc: r#"The mesh the engine fills in each step; the object's renderer draws it."#,
            },
        ],
        skip_save: &[r#"rebuildRequests"#, r#"sourceMesh"#, r#"simulatedMesh"#],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[
            r#"/// Vertices within `pin_radius` of these points (object space) stay put."#,
            r#"pub pins: Vec<V3>,"#,
            r#"/// Vertices in this object-space box (min, max) stay put."#,
            r#"pub pin_box: Option<(V3, V3)>,"#,
            r#"pub sphere_colliders: Vec<ObjectId>,"#,
            r#"pub capsule_colliders: Vec<ObjectId>,"#,
        ],
        extra_defaults: &[
            r#"pins: Vec::new(),"#,
            r#"pin_box: None,"#,
            r#"sphere_colliders: Vec::new(),"#,
            r#"capsule_colliders: Vec::new(),"#,
        ],
        extra_save: Some(
            r#"m.insert("pins".into(), Value::Array(d.pins.iter().map(|p| v3_json(*p)).collect()));
                m.insert("pinBox".into(), d.pin_box.map_or(Value::Null, |(a, b)| json!({ "min": v3_json(a), "max": v3_json(b) })));
                m.insert("sphereColliders".into(), Value::Array(d.sphere_colliders.iter().map(|o| object_ref(o.to_raw())).collect()));
                m.insert("capsuleColliders".into(), Value::Array(d.capsule_colliders.iter().map(|o| object_ref(o.to_raw())).collect()));"#,
        ),
        extra_load: Some(
            r#"    "pins" => v.as_array().and_then(|a| a.iter().map(v3_of).collect::<Option<Vec<V3>>>()).map(|p| d.pins = p),
                "pinBox" => if v.is_null() { d.pin_box = None; Some(()) } else { (|| Some((v3_of(v.get("min")?)?, v3_of(v.get("max")?)?)))().map(|b| d.pin_box = Some(b)) },
                "sphereColliders" => v.as_array().and_then(|a| a.iter().map(|x| object_of(x).flatten()).collect::<Option<Vec<ObjectId>>>()).map(|o| d.sphere_colliders = o),
                "capsuleColliders" => v.as_array().and_then(|a| a.iter().map(|x| object_of(x).flatten()).collect::<Option<Vec<ObjectId>>>()).map(|o| d.capsule_colliders = o)"#,
        ),
        extra_handles: &[],
        extra_records: &[r#"ClothBox = ClothBox [min: Vector3Like, max: Vector3Like]"#],
        extra_value_types: &[r#"v3"#],
        items: Some(
            r#"/// A cloth's pin box: vertices inside it stay put.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClothBox {
    pub min: V3,
    pub max: V3,
}"#,
        ),
        api: Some(
            r#"
        /// Starts the simulation again from the mesh with the current settings. The object needs a
        /// `MeshRenderer`: it then draws the simulated copy. (The TypeScript started on `onStart`;
        /// here a scene load does it, and a script calls `rebuild()` after setting things up.)
        pub fn rebuild(self) {
            cloth_rebuild(self);
        }

        /// A point (object space): vertices near it stay put.
        pub fn add_pin(self, point: V3) {
            with_cloth(self, |d| d.pins.push(point));
        }

        pub fn clear_pins(self) {
            with_cloth(self, |d| d.pins.clear());
        }

        /// Vertices in this object-space box stay put (a flag's edge on its pole); `null` for
        /// none.
        #[property]
        pub fn pin_box(self) -> Option<ClothBox> {
            with_cloth(self, |d| d.pin_box)
                .flatten()
                .map(|(min, max)| ClothBox { min, max })
        }

        pub fn set_pin_box(self, b: Option<ClothBox>) {
            with_cloth(self, |d| d.pin_box = b.map(|b| (b.min, b.max)));
        }

        /// The points (object space) near which vertices stay put.
        #[property]
        pub fn pins(self) -> Vec<V3> {
            with_cloth(self, |d| d.pins.clone()).unwrap_or_default()
        }

        pub fn set_pins(self, pins: &[V3]) {
            with_cloth(self, |d| d.pins = pins.to_vec());
        }

        /// Objects with a `SphereCollider` the cloth cannot pass through.
        #[property]
        pub fn sphere_colliders(self) -> Vec<ObjectId> {
            with_cloth(self, |d| d.sphere_colliders.clone()).unwrap_or_default()
        }

        pub fn set_sphere_colliders(self, objects: &[ObjectId]) {
            with_cloth(self, |d| d.sphere_colliders = objects.to_vec());
        }

        /// Objects with a `CapsuleCollider` the cloth cannot pass through.
        #[property]
        pub fn capsule_colliders(self) -> Vec<ObjectId> {
            with_cloth(self, |d| d.capsule_colliders.clone()).unwrap_or_default()
        }

        pub fn set_capsule_colliders(self, objects: &[ObjectId]) {
            with_cloth(self, |d| d.capsule_colliders = objects.to_vec());
        }

        /// An object with a `SphereCollider` the cloth cannot pass through.
        pub fn add_sphere_collider(self, object: ObjectId) {
            with_cloth(self, |d| d.sphere_colliders.push(object));
        }

        /// An object with a `CapsuleCollider` the cloth cannot pass through.
        pub fn add_capsule_collider(self, object: ObjectId) {
            with_cloth(self, |d| d.capsule_colliders.push(object));
        }

        pub fn clear_colliders(self) {
            with_cloth(self, |d| {
                d.sphere_colliders.clear();
                d.capsule_colliders.clear();
            });
        }
"#,
        ),
        remove_hook: Some(r#"cloth_removed(r);"#),
        awake_hook: Some(r#"cloth_rebuild(r);"#),
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"Material"#,
        low: r#"material"#,
        reference: r#"MaterialRef"#,
        store: r#"MATERIALS"#,
        data: r#"MaterialData"#,
        asset: true,
        shared: false,
        create_name: None,
        fields: &[
            Field {
                ts: r#"color"#,
                rs: r#"color"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[1.0, 1.0, 1.0, 1.0]),
                doc: r#"Base colour (linear; alpha below 1 makes it see-through). For metals, the reflection's tint."#,
            },
            Field {
                ts: r#"emission"#,
                rs: r#"emission"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[0.0, 0.0, 0.0, 1.0]),
                doc: r#"Light it gives off by itself (linear, can exceed 1 for glow)."#,
            },
            Field {
                ts: r#"smoothness"#,
                rs: r#"smoothness"#,
                ty: r#"f64"#,
                default: Dv::Float(0.5),
                doc: r#"0 (rough, matte) to 1 (polished, mirror-like): 1 - roughness."#,
            },
            Field {
                ts: r#"unlit"#,
                rs: r#"unlit"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#"Ignore lights: just the colour plus emission."#,
            },
            Field {
                ts: r#"mainTexture"#,
                rs: r#"main_texture"#,
                ty: r#"texture"#,
                default: Dv::Int(0),
                doc: r#"Multiplies the colour; its alpha counts when `color.a` < 1."#,
            },
            Field {
                ts: r#"mainTextureScale"#,
                rs: r#"main_texture_scale"#,
                ty: r#"v2"#,
                default: Dv::Tuple(&[1.0, 1.0]),
                doc: r#"How many times the texture repeats across the mesh's UVs."#,
            },
            Field {
                ts: r#"mainTextureOffset"#,
                rs: r#"main_texture_offset"#,
                ty: r#"v2"#,
                default: Dv::Tuple(&[0.0, 0.0]),
                doc: r#"Where the texture starts (in its 0..1 space)."#,
            },
            Field {
                ts: r#"metallic"#,
                rs: r#"metallic"#,
                ty: r#"f64"#,
                default: Dv::Float(0.0),
                doc: r#"0 (plastic, stone, wood, skin) to 1 (metal)."#,
            },
            Field {
                ts: r#"normalMap"#,
                rs: r#"normal_map"#,
                ty: r#"texture"#,
                default: Dv::Int(0),
                doc: r#"Bumps from a tangent-space normal map (load it with `linear`)."#,
            },
            Field {
                ts: r#"normalScale"#,
                rs: r#"normal_scale"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#"How strong the normal map's bumps are."#,
            },
            Field {
                ts: r#"metallicGlossMap"#,
                rs: r#"metallic_gloss_map"#,
                ty: r#"texture"#,
                default: Dv::Int(0),
                doc: r#"Metallic in red and smoothness in alpha."#,
            },
            Field {
                ts: r#"hasSpecularColor"#,
                rs: r#"has_specular_color"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#"The specular workflow is on."#,
            },
            Field {
                ts: r#"specularColor"#,
                rs: r#"specular_color"#,
                ty: r#"color"#,
                default: Dv::Tuple(&[0.0, 0.0, 0.0, 1.0]),
                doc: r#"The specular workflow's reflectance head-on; `metallic` is then ignored."#,
            },
            Field {
                ts: r#"occlusionMap"#,
                rs: r#"occlusion_map"#,
                ty: r#"texture"#,
                default: Dv::Int(0),
                doc: r#"Ambient occlusion (green channel)."#,
            },
            Field {
                ts: r#"occlusionStrength"#,
                rs: r#"occlusion_strength"#,
                ty: r#"f64"#,
                default: Dv::Float(1.0),
                doc: r#""#,
            },
            Field {
                ts: r#"emissionMap"#,
                rs: r#"emission_map"#,
                ty: r#"texture"#,
                default: Dv::Int(0),
                doc: r#"Multiplies `emission`."#,
            },
            Field {
                ts: r#"detailAlbedoMap"#,
                rs: r#"detail_albedo_map"#,
                ty: r#"texture"#,
                default: Dv::Int(0),
                doc: r#"Fine detail multiplied over the colour."#,
            },
            Field {
                ts: r#"detailAlbedoMapScale"#,
                rs: r#"detail_albedo_map_scale"#,
                ty: r#"v2"#,
                default: Dv::Tuple(&[1.0, 1.0]),
                doc: r#""#,
            },
            Field {
                ts: r#"detailAlbedoMapOffset"#,
                rs: r#"detail_albedo_map_offset"#,
                ty: r#"v2"#,
                default: Dv::Tuple(&[0.0, 0.0]),
                doc: r#""#,
            },
            Field {
                ts: r#"renderingMode"#,
                rs: r#"rendering_mode"#,
                ty: r#"enum:opaque|cutout|fade|transparent"#,
                default: Dv::Str(r#"opaque"#),
                doc: r#"`opaque`, `cutout` (holes where alpha < `alphaCutoff`), `fade` or `transparent` (glass)."#,
            },
            Field {
                ts: r#"alphaCutoff"#,
                rs: r#"alpha_cutoff"#,
                ty: r#"f64"#,
                default: Dv::Float(0.5),
                doc: r#"With `cutout`: alpha below this is a hole."#,
            },
            Field {
                ts: r#"doubleSided"#,
                rs: r#"double_sided"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#"Draws back faces too, lit from that side."#,
            },
            Field {
                ts: r#"gltfPacking"#,
                rs: r#"gltf_packing"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#"glTF's packing of the metallic map (internal)."#,
            },
        ],
        skip_save: &[r#"hasSpecularColor"#, r#"specularColor"#],
        save_if_changed: &[
            r#"normalMap"#,
            r#"normalScale"#,
            r#"metallicGlossMap"#,
            r#"occlusionMap"#,
            r#"occlusionStrength"#,
            r#"emissionMap"#,
            r#"detailAlbedoMap"#,
            r#"detailAlbedoMapScale"#,
            r#"detailAlbedoMapOffset"#,
            r#"renderingMode"#,
            r#"alphaCutoff"#,
            r#"doubleSided"#,
            r#"gltfPacking"#,
        ],
        set_hook: &[(r#"specularColor"#, r#"d.has_specular_color = true;"#)],
        extra_fields: &[],
        extra_defaults: &[],
        extra_save: Some(
            r#"if d.has_specular_color { m.insert("specularColor".into(), json!({ "$type": "Color", "value": [d.specular_color.r, d.specular_color.g, d.specular_color.b, d.specular_color.a] })); }"#,
        ),
        extra_load: Some(
            r#""specularColor" => { if v.is_null() { d.has_specular_color = false; Some(()) } else { color_of(v).map(|c| { d.has_specular_color = true; d.specular_color = c; }) } }"#,
        ),
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[],
        items: None,
        api: Some(
            r#"
        /// Turns the specular workflow off again (`specularColor = null`).
        pub fn clear_specular_color(self) {
            with_material(self, |d| d.has_specular_color = false);
        }

        /// `new Material(color, emission, smoothness, unlit, mainTexture, mainTextureScale,
        /// mainTextureOffset, metallic)`: a material made from its common settings, each optional.
        #[allow(clippy::too_many_arguments)]
        #[constructor]
        #[optional(color, emission, smoothness = 0.5, unlit, main_texture, main_texture_scale, main_texture_offset, metallic)]
        #[ts_param(main_texture = "Texture | null")]
        pub fn construct(
            color: Rgba,
            emission: Rgba,
            smoothness: f64,
            unlit: bool,
            main_texture: TextureRef,
            main_texture_scale: V2,
            main_texture_offset: V2,
            metallic: f64,
        ) -> MaterialRef {
            let m = Self::create();
            let or = |v: Rgba, d: Rgba| if v.r.is_nan() { d } else { v };
            let or2 = |v: V2, d: V2| if v.x.is_nan() || v.y.is_nan() { d } else { v };
            with_material(m, |d| {
                d.color = or(color, Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                d.emission = or(emission, Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 });
                d.smoothness = smoothness;
                d.unlit = unlit;
                if main_texture.0 != 0 {
                    d.main_texture = main_texture.0;
                }
                d.main_texture_scale = or2(main_texture_scale, V2 { x: 1.0, y: 1.0 });
                d.main_texture_offset = or2(main_texture_offset, V2 { x: 0.0, y: 0.0 });
                d.metallic = if metallic.is_nan() { 0.0 } else { metallic };
            });
            m
        }

        /// A copy with another colour.
        pub fn with_color(self, color: Rgba) -> MaterialRef {
            let copy = self.copy();
            with_material(copy, |d| d.color = color);
            copy
        }

        /// A copy with another main texture.
        pub fn with_texture(self, texture: TextureRef) -> MaterialRef {
            let copy = self.copy();
            with_material(copy, |d| d.main_texture = texture.0);
            copy
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
    Component {
        name: r#"Sprite"#,
        low: r#"sprite"#,
        reference: r#"SpriteRef"#,
        store: r#"SPRITES"#,
        data: r#"SpriteData"#,
        asset: true,
        shared: false,
        create_name: Some(r#"empty"#),
        fields: &[
            Field {
                ts: r#"texture"#,
                rs: r#"texture"#,
                ty: r#"texture"#,
                default: Dv::Int(0),
                doc: r#"The texture it is cut from."#,
            },
            Field {
                ts: r#"hasRect"#,
                rs: r#"has_rect"#,
                ty: r#"bool"#,
                default: Dv::Bool(false),
                doc: r#"Whether `rect` is set; otherwise the whole texture."#,
            },
            Field {
                ts: r#"pivot"#,
                rs: r#"pivot"#,
                ty: r#"v2"#,
                default: Dv::Tuple(&[0.5, 0.5]),
                doc: r#"Where the object's origin sits (0..1 across the rect from its bottom-left)."#,
            },
            Field {
                ts: r#"pixelsPerUnit"#,
                rs: r#"pixels_per_unit"#,
                ty: r#"f64"#,
                default: Dv::Float(100.0),
                doc: r#"Texels that make a metre."#,
            },
        ],
        skip_save: &[r#"hasRect"#],
        save_if_changed: &[],
        set_hook: &[],
        extra_fields: &[
            r#"pub rect: [f64; 4],"#,
            r#"/// Left, bottom, right and top borders in pixels (the corners a sliced sprite keeps)."#,
            r#"pub border: [f64; 4],"#,
        ],
        extra_defaults: &[r#"rect: [0.0; 4],"#, r#"border: [0.0; 4],"#],
        extra_save: Some(
            r#"m.insert("rect".into(), if d.has_rect { json!({ "$type": "Rect", "value": d.rect }) } else { Value::Null });
        if d.border != [0.0; 4] { m.insert("border".into(), json!({ "$type": "Vector4", "value": d.border })); }"#,
        ),
        extra_load: Some(
            r#""rect" => { if v.is_null() { d.has_rect = false; Some(()) } else { rect_of(v).map(|r| { d.has_rect = true; d.rect = r; }) } }
            "border" => { if v.is_null() { d.border = [0.0; 4]; Some(()) } else { rect_of(v).map(|r| d.border = r) } }"#,
        ),
        extra_handles: &[],
        extra_records: &[],
        extra_value_types: &[r#"v2"#, r#"rect"#],
        items: None,
        api: Some(
            r#"
        /// Draws only this part of the texture: x, y, width, height in pixels from the
        /// texture's bottom-left.
        pub fn set_rect(self, x: f64, y: f64, width: f64, height: f64) {
            with_sprite(self, |d| {
                d.has_rect = true;
                d.rect = [x, y, width, height];
            });
        }

        /// The borders in pixels (left, bottom, right, top) a `sliced` sprite keeps at its size.
        pub fn set_border(self, left: f64, bottom: f64, right: f64, top: f64) {
            with_sprite(self, |d| d.border = [left, bottom, right, top]);
        }

        /// A sprite of a texture: all of it, centred, `pixelsPerUnit` (default 100) texels to a metre.
        #[optional(pixels_per_unit = 100)]
        pub fn create(texture: TextureRef, pixels_per_unit: f64) -> SpriteRef {
            let s = super::create_sprite();
            with_sprite(s, |d| {
                d.texture = texture.0;
                d.pixels_per_unit = pixels_per_unit;
            });
            s
        }

        /// A sprite of a texture cut to `rect` (x, y, width, height in pixels from the texture's
        /// bottom-left; `null` for all of it), with a pivot (centred by default) and
        /// `pixelsPerUnit` (100 by default).
        #[js_name = "create"]
        #[optional(pivot, pixels_per_unit = 100)]
        pub fn create_cut(texture: TextureRef, rect: Option<Rect>, pivot: V2, pixels_per_unit: f64) -> SpriteRef {
            let s = Self::create(texture, pixels_per_unit);
            if let Some(r) = rect {
                s.set_rect(r.x, r.y, r.width, r.height);
            }
            if !pivot.x.is_nan() && !pivot.y.is_nan() {
                s.set_pivot(pivot);
            }
            s
        }

        /// One cell of a sprite sheet cut into `columns` x `rows` equal cells, read left to right,
        /// top to bottom (needs the texture loaded).
        pub fn grid_cell(texture: TextureRef, columns: f64, rows: f64, index: f64) -> SpriteRef {
            let (w, h) = super::texture_size(texture);
            let (cw, ch) = (w / columns.max(1.0), h / rows.max(1.0));
            let (c, r) = (index % columns.max(1.0), (index / columns.max(1.0)).floor());
            let s = super::create_sprite();
            with_sprite(s, |d| {
                d.texture = texture.0;
                d.has_rect = true;
                d.rect = [c * cw, h - (r + 1.0) * ch, cw, ch];
            });
            s
        }

        /// Every cell of a sprite sheet cut into `columns` x `rows` cells, row by row from the
        /// top, each with this pivot and `pixelsPerUnit` when given.
        #[optional(pivot, pixels_per_unit)]
        pub fn grid(texture: TextureRef, columns: f64, rows: f64, pivot: V2, pixels_per_unit: f64) -> Vec<SpriteRef> {
            let mut cells = Vec::new();
            for r in 0..rows.max(0.0) as usize {
                for c in 0..columns.max(0.0) as usize {
                    let s = Self::grid_cell(texture, columns, rows, r as f64 * columns + c as f64);
                    with_sprite(s, |d| {
                        if !pivot.x.is_nan() {
                            d.pivot = pivot;
                        }
                        if !pixels_per_unit.is_nan() {
                            d.pixels_per_unit = pixels_per_unit;
                        }
                    });
                    cells.push(s);
                }
            }
            cells
        }

        /// Its size in world units (zero until the texture has loaded, for a whole-texture sprite).
        #[property]
        pub fn size(self) -> V2 {
            let (tw, th) = with_sprite(self, |d| super::texture_size(TextureRef(d.texture))).unwrap_or((0.0, 0.0));
            with_sprite(self, |d| {
                let (w, h) = if d.has_rect { (d.rect[2], d.rect[3]) } else { (tw, th) };
                V2 { x: w / d.pixels_per_unit, y: h / d.pixels_per_unit }
            })
            .unwrap_or(V2 { x: 0.0, y: 0.0 })
        }
"#,
        ),
        remove_hook: None,
        awake_hook: None,
        attach_hook: None,
        kind_on_filter: None,
    },
];
