// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The 3D world pass (M5.a): planes through a perspective camera, with screen UI
//! blended over them.

use platypus_core::glam::{Mat4, Quat, Vec3};
use platypus_core::{Commit, Value};
use platypus_render::testing::{assert_golden, shared_gpu};
use platypus_render::{
    ALL_LAYERS, Color, OffscreenTarget, RenderTarget, TextureData, TextureFilter, TextureWrap,
    UiRenderer, WorldCamera, WorldQuad, WorldRenderer, WorldTexture,
};
use platypus_ui::UiTree;
use platypus_ui::opcodes::*;

fn op(code: u32, args: &[Value]) -> Value {
    let mut v = vec![Value::Number(code.into())];
    v.extend_from_slice(args);
    Value::Array(v)
}

fn place(position: Vec3, rotation: Quat, size: (f32, f32)) -> Mat4 {
    Mat4::from_scale_rotation_translation(Vec3::new(size.0, size.1, 1.0), rotation, position)
}

/// A 4×4 checker, red and white, as a texture.
fn checker(gpu: &platypus_render::Gpu) -> wgpu::TextureView {
    use platypus_render::wgpu;
    let mut pixels = Vec::new();
    for y in 0..4 {
        for x in 0..4 {
            pixels.extend(if (x + y) % 2 == 0 {
                [230, 40, 40, 255]
            } else {
                [255, 255, 255, 255]
            });
        }
    }
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("checker"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(16),
            rows_per_image: None,
        },
        texture.size(),
    );
    texture.create_view(&Default::default())
}
use platypus_render::wgpu;

// M5-RND-05
#[test]
fn planes_in_perspective_with_ui_over_them() {
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };

    // A camera 4 m back and 2 m up, looking at the origin.
    let eye = Vec3::new(0.0, 2.0, 4.0);
    let camera_world =
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y).inverse();
    let camera = WorldCamera::perspective(
        camera_world,
        60.0,
        0.1,
        100.0,
        size,
        Color::rgb(0.05, 0.07, 0.12),
    );

    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let quads = [
        // A grey floor, 8 m square.
        WorldQuad::colored(place(Vec3::ZERO, flat, (8.0, 8.0)), [0.25, 0.27, 0.3, 1.0]),
        // A green wall behind, partly hidden by the blue panel in front of it.
        WorldQuad::colored(
            place(Vec3::new(-0.8, 0.75, -1.0), Quat::IDENTITY, (1.5, 1.5)),
            [0.1, 0.7, 0.3, 1.0],
        ),
        // A half-transparent blue panel nearer the camera: the green shows through.
        WorldQuad::colored(
            place(
                Vec3::new(-0.2, 0.6, 0.2),
                Quat::from_rotation_y(0.4),
                (1.2, 1.0),
            ),
            [0.2, 0.4, 1.0, 0.5],
        ),
        // A textured plane turned towards the camera, right of centre.
        WorldQuad {
            model: place(
                Vec3::new(1.3, 0.6, -0.3),
                Quat::from_rotation_y(-0.5),
                (1.0, 1.0),
            ),
            color: [1.0; 4],
            texture: Some(WorldTexture::Frame(0)),
            uv: [0.0, 0.0, 1.0, 1.0],
            additive: false,
            order: 0,
            layer: 0,
            flat: false,
        },
    ];
    let checker = checker(&gpu);
    let mut world = WorldRenderer::new(&gpu);
    world.render(&gpu, &rt, &camera, &quads, &[&checker]);

    // Screen UI over the world: a translucent bar along the top.
    let mut tree = UiTree::new(320.0, 200.0);
    let errors = tree.apply_commit(&Commit {
        ops: vec![
            op(CREATE, &[Value::Number(1.0), "box".into()]),
            op(
                SET_STYLE,
                &[
                    Value::Number(1.0),
                    "width".into(),
                    Value::Number(320.0),
                    "height".into(),
                    Value::Number(30.0),
                    "backgroundColor".into(),
                    "rgba(255, 200, 0, 0.6)".into(),
                ],
            ),
            op(APPEND_CHILD, &[Value::Number(0.0), Value::Number(1.0)]),
        ],
    });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
    let cmds = tree.draw_list();
    // The golden was made in linear light.
    let mut ui = UiRenderer::new(&gpu);
    ui.set_blending(platypus_render::Blending::Linear);
    ui.render_overlay(&gpu, &rt, &cmds);

    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-05", size.0, size.1, &pixels, 2);

    let at = |x: u32, y: u32| {
        let i = ((y * size.0 + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // The top bar is yellow over the dark sky; below it, the sky shows.
    let bar = at(5, 10);
    assert!(bar[0] > 150 && bar[1] > 120 && bar[2] < 80, "bar {bar:?}");
    let sky = at(5, 40);
    assert!(sky[0] < 80 && sky[2] < 120, "sky {sky:?}");
}

// M5-RND-06
#[test]
fn cameras_draw_into_their_viewports_by_depth() {
    use platypus_render::{Projection, WorldView, viewport_size};
    let gpu = shared_gpu();
    let size = (320, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let quads = vec![
        WorldQuad::colored(place(Vec3::ZERO, flat, (6.0, 6.0)), [0.3, 0.3, 0.35, 1.0]),
        WorldQuad::colored(
            place(Vec3::new(0.0, 0.5, 0.0), Quat::IDENTITY, (1.0, 1.0)),
            [0.9, 0.5, 0.1, 1.0],
        ),
    ];
    let look = |eye: Vec3| {
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y).inverse()
    };
    // Left half: perspective from the front; right half: orthographic from above.
    let left = [0.0, 0.0, 0.5, 1.0];
    let right = [0.5, 0.0, 0.5, 1.0];
    let views = [
        WorldView {
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
            camera: WorldCamera::new(
                look(Vec3::new(0.0, 2.0, 4.0)),
                Projection::Perspective {
                    fov_y_degrees: 60.0,
                },
                0.1,
                50.0,
                viewport_size(left, size),
                Color::rgb(0.1, 0.05, 0.05),
            ),
            viewport: left,
            quads: quads.clone(),
        },
        WorldView {
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
            camera: WorldCamera::new(
                Mat4::from_rotation_translation(flat, Vec3::new(0.0, 5.0, 0.0)),
                Projection::Orthographic { half_height: 4.0 },
                0.1,
                50.0,
                viewport_size(right, size),
                Color::rgb(0.05, 0.05, 0.1),
            ),
            viewport: right,
            // A red marker on the floor towards +x (east), seen from above.
            quads: [
                quads.as_slice(),
                &[WorldQuad::colored(
                    place(Vec3::new(2.0, 0.01, 0.0), flat, (1.0, 1.0)),
                    [0.9, 0.1, 0.1, 1.0],
                )],
            ]
            .concat(),
        },
        // A picture-in-picture inset over the left view (drawn after it, so on top).
        WorldView {
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
            camera: WorldCamera::new(
                look(Vec3::new(3.0, 1.0, 0.0)),
                Projection::Perspective {
                    fov_y_degrees: 50.0,
                },
                0.1,
                50.0,
                viewport_size([0.05, 0.05, 0.15, 0.3], size),
                Color::rgb(0.0, 0.2, 0.0),
            ),
            viewport: [0.05, 0.05, 0.15, 0.3],
            quads: vec![],
        },
    ];
    WorldRenderer::new(&gpu).render_views(&gpu, &rt, &views, &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-06", size.0, size.1, &pixels, 2);
    let at = |x: u32, y: u32| {
        let i = ((y * size.0 + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // The inset's green background covers the left view's top-left corner.
    assert!(at(20, 20)[1] > at(20, 20)[0] + 40, "{:?}", at(20, 20));
    // From above, the 6 m floor is a square inside the 8 m tall ortho view: the top edge
    // is background, and the marker is right of centre (east is +x on screen).
    let edge = at(240, 3);
    assert!(edge[2] > edge[0], "{edge:?}");
    let marker = at(240 + 40, 80);
    assert!(marker[0] > 150 && marker[1] < 120, "{marker:?}");
}

// M5-RND-07
#[test]
fn lit_primitives_with_lights() {
    use platypus_render::{Light, LightKind, MeshInstance, primitive_id};
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let eye = Vec3::new(0.0, 2.5, 6.0);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0.0, 0.5, 0.0), Vec3::Y)
            .inverse(),
        50.0,
        0.1,
        50.0,
        size,
        Color::rgb(0.02, 0.02, 0.03),
    );
    let mesh = |name: &str, at: Vec3, color: [f32; 4]| MeshInstance {
        mesh: primitive_id(name).unwrap(),
        model: Mat4::from_rotation_translation(Quat::from_rotation_y(0.6), at),
        color,
        emission: [0.0; 3],
        smoothness: 0.6,
        metallic: 0.0,
        unlit: false,
        texture: None,
        tiling: [1.0, 1.0, 0.0, 0.0],
        glow: platypus_render::MeshGlow::None,
        cast_shadows: true,
        mirror: None,
        layer: 0,
        ..Default::default()
    };
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.meshes = vec![
        MeshInstance {
            model: Mat4::from_scale(Vec3::splat(0.6)),
            smoothness: 0.0,
            metallic: 0.0,
            ..mesh("plane", Vec3::ZERO, [0.5, 0.5, 0.5, 1.0])
        },
        mesh("cube", Vec3::new(-1.8, 0.5, 0.0), [0.9, 0.3, 0.2, 1.0]),
        mesh("sphere", Vec3::new(0.0, 0.5, 0.0), [0.2, 0.5, 0.9, 1.0]),
        mesh("capsule", Vec3::new(1.8, 1.0, 0.0), [0.3, 0.8, 0.3, 1.0]),
        // An emissive, unlit marker and a see-through cylinder in front.
        MeshInstance {
            unlit: true,
            emission: [1.0, 0.8, 0.2],
            ..mesh("cube", Vec3::new(0.0, 2.2, 0.0), [0.2, 0.2, 0.2, 1.0])
        },
        mesh("cylinder", Vec3::new(0.9, 0.5, 1.5), [1.0, 1.0, 1.0, 0.35]),
    ];
    v.lights = vec![
        Light {
            kind: LightKind::Directional,
            color: [1.0, 0.95, 0.9],
            position: Vec3::ZERO,
            direction: Vec3::new(-0.5, -1.0, -0.6).normalize(),
            range: 0.0,
            shadows: None,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        },
        Light {
            kind: LightKind::Point,
            color: [6.0, 2.0, 1.0],
            position: Vec3::new(-1.0, 1.2, 1.2),
            direction: Vec3::ZERO,
            range: 4.0,
            shadows: None,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        },
    ];
    WorldRenderer::new(&gpu).render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-07", size.0, size.1, &pixels, 3);
}

/// An `n` × `n` two-colour checker with a red top-left texel (to see orientation).
fn checker_data(
    n: u32,
    a: [u8; 4],
    b: [u8; 4],
    filter: TextureFilter,
    mipmaps: bool,
) -> TextureData {
    let mut rgba = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let c = if x == 0 && y == 0 {
                [230, 40, 40, 255]
            } else if (x + y) % 2 == 0 {
                a
            } else {
                b
            };
            rgba.extend(c);
        }
    }
    TextureData {
        width: n,
        height: n,
        rgba,
        filter,
        wrap: TextureWrap::Repeat,
        mipmaps,
        anisotropy: 1,
        srgb: true,
    }
}

// M5-RND-08
#[test]
fn textured_meshes_and_sprites() {
    use platypus_render::{Light, LightKind, MeshGlow, MeshInstance, primitive_id};
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    let light = [235, 235, 225, 255];
    assert!(world.set_texture(
        &gpu,
        1,
        &checker_data(8, light, [60, 90, 160, 255], TextureFilter::Trilinear, true)
    ));
    // Pixel art: point-filtered, with a see-through half.
    assert!(world.set_texture(
        &gpu,
        2,
        &checker_data(
            4,
            [250, 200, 60, 255],
            [0, 0, 0, 0],
            TextureFilter::Point,
            false
        )
    ));
    assert!(!world.set_texture(
        &gpu,
        3,
        &TextureData {
            rgba: vec![0; 3],
            ..checker_data(1, light, light, TextureFilter::Point, false)
        }
    ));
    assert_eq!(world.texture_size(1), Some((8, 8)));
    assert_eq!(world.texture_size(3), None);

    let eye = Vec3::new(0.0, 2.5, 6.0);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0.0, 0.5, 0.0), Vec3::Y)
            .inverse(),
        50.0,
        0.1,
        50.0,
        size,
        Color::rgb(0.02, 0.02, 0.03),
    );
    let mesh = |name: &str, model: Mat4, tiling: [f32; 4]| MeshInstance {
        mesh: primitive_id(name).unwrap(),
        model,
        color: [1.0; 4],
        emission: [0.0; 3],
        smoothness: 0.2,
        metallic: 0.0,
        unlit: false,
        texture: Some(WorldTexture::Asset(1)),
        tiling,
        glow: MeshGlow::None,
        cast_shadows: true,
        mirror: None,
        layer: 0,
        ..Default::default()
    };
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.meshes = vec![
        // A floor tiled 6 times: mipmaps keep the distance from shimmering.
        mesh(
            "plane",
            Mat4::from_scale(Vec3::splat(0.8)),
            [6.0, 6.0, 0.0, 0.0],
        ),
        mesh(
            "cube",
            Mat4::from_rotation_translation(Quat::from_rotation_y(0.6), Vec3::new(-1.5, 0.5, 0.0)),
            [1.0, 1.0, 0.0, 0.0],
        ),
        // Half a texture per face, shifted.
        mesh(
            "sphere",
            Mat4::from_translation(Vec3::new(0.0, 0.6, 0.5)),
            [2.0, 1.0, 0.25, 0.0],
        ),
        // A see-through cube using the pixel art's alpha.
        MeshInstance {
            color: [1.0, 1.0, 1.0, 0.99],
            texture: Some(WorldTexture::Asset(2)),
            ..mesh(
                "cube",
                Mat4::from_rotation_translation(
                    Quat::from_rotation_y(-0.4),
                    Vec3::new(1.6, 0.5, -0.5),
                ),
                [1.0; 4],
            )
        },
        // A texture that doesn't exist draws the magenta missing checker.
        MeshInstance {
            texture: Some(WorldTexture::Asset(99)),
            ..mesh(
                "cube",
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(0.3),
                    Quat::IDENTITY,
                    Vec3::new(0.0, 1.8, 0.0),
                ),
                [1.0; 4],
            )
        },
    ];
    // Sprites: the pixel art upright, and flipped horizontally.
    let sprite = |x: f32, uv: [f32; 4]| WorldQuad {
        model: place(Vec3::new(x, 1.6, 1.0), Quat::IDENTITY, (0.8, 0.8)),
        color: [1.0; 4],
        texture: Some(WorldTexture::Asset(2)),
        uv,
        additive: false,
        order: 0,
        layer: 0,
        flat: false,
    };
    v.quads = vec![
        sprite(-0.6, [0.0, 0.0, 1.0, 1.0]),
        sprite(0.6, [1.0, 0.0, -1.0, 1.0]),
    ];
    v.lights = vec![Light {
        kind: LightKind::Directional,
        color: [1.0, 0.97, 0.92],
        position: Vec3::ZERO,
        direction: Vec3::new(-0.4, -1.0, -0.5).normalize(),
        range: 0.0,
        shadows: None,
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-08", size.0, size.1, &pixels, 3);
}

// M5-RND-09
#[test]
fn glows_bloom_and_spot_beams() {
    use platypus_render::{
        BEAM_MESH, Bloom, DEFAULT_PARTICLE_TEXTURE, Light, LightKind, MeshGlow, MeshInstance,
        primitive_id,
    };
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let eye = Vec3::new(0.0, 2.0, 6.0);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0.0, 1.0, 0.0), Vec3::Y)
            .inverse(),
        50.0,
        0.1,
        50.0,
        size,
        Color::rgb(0.01, 0.01, 0.02),
    );
    let mesh = |name: &str, model: Mat4, color: [f32; 4]| MeshInstance {
        mesh: primitive_id(name).unwrap(),
        model,
        color,
        emission: [0.0; 3],
        smoothness: 0.3,
        metallic: 0.0,
        unlit: false,
        texture: None,
        tiling: [1.0, 1.0, 0.0, 0.0],
        glow: MeshGlow::None,
        cast_shadows: true,
        mirror: None,
        layer: 0,
        ..Default::default()
    };
    // A spot light shining down at an angle from the upper left, with its beam.
    let spot_at = Vec3::new(-1.6, 3.0, 0.0);
    let spot_dir = Vec3::new(0.45, -1.0, 0.0).normalize();
    let spot_rot = Quat::from_rotation_arc(Vec3::NEG_Z, spot_dir);
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.ambient = [0.05, 0.05, 0.06];
    v.meshes = vec![
        mesh(
            "plane",
            Mat4::from_scale(Vec3::splat(0.8)),
            [0.5, 0.5, 0.5, 1.0],
        ),
        // A hot emissive bar (brighter than white: it blooms) and a dull cube.
        MeshInstance {
            emission: [4.0, 1.2, 0.3],
            unlit: true,
            ..mesh(
                "cube",
                Mat4::from_scale_rotation_translation(
                    Vec3::new(1.2, 0.15, 0.15),
                    Quat::IDENTITY,
                    Vec3::new(1.3, 1.6, 0.0),
                ),
                [0.1, 0.1, 0.1, 1.0],
            )
        },
        mesh(
            "cube",
            Mat4::from_translation(Vec3::new(1.4, 0.5, -0.5)),
            [0.3, 0.5, 0.8, 1.0],
        ),
        // An additive sphere: it lights what's behind it.
        MeshInstance {
            glow: MeshGlow::Additive,
            unlit: true,
            ..mesh(
                "sphere",
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(0.5),
                    Quat::IDENTITY,
                    Vec3::new(0.2, 1.8, 1.0),
                ),
                [0.2, 0.9, 0.4, 0.6],
            )
        },
        MeshInstance {
            mesh: BEAM_MESH,
            glow: MeshGlow::Beam,
            cast_shadows: false,
            mirror: None,
            unlit: true,
            ..mesh(
                "cube",
                Mat4::from_scale_rotation_translation(Vec3::new(1.2, 1.2, 3.5), spot_rot, spot_at),
                [1.0, 0.9, 0.7, 0.5],
            )
        },
    ];
    // Glowing additive spots of different sizes, one partly behind the dull cube.
    let glow = |at: Vec3, size: f32, color: [f32; 4]| WorldQuad {
        model: place(at, Quat::IDENTITY, (size, size)),
        color,
        texture: Some(WorldTexture::Asset(DEFAULT_PARTICLE_TEXTURE)),
        uv: [0.0, 0.0, 1.0, 1.0],
        additive: true,
        order: 0,
        layer: 0,
        flat: false,
    };
    v.quads = vec![
        glow(Vec3::new(-0.6, 0.6, 0.8), 0.8, [1.0, 0.5, 0.2, 1.0]),
        glow(Vec3::new(-0.3, 0.9, 0.9), 0.4, [0.4, 0.6, 1.0, 1.0]),
        glow(Vec3::new(1.4, 0.9, -1.2), 1.4, [1.0, 0.2, 0.8, 1.0]),
    ];
    v.lights = vec![Light {
        kind: LightKind::Spot {
            inner_degrees: 25.0,
            outer_degrees: 38.0,
        },
        color: [8.0, 7.0, 5.5],
        position: spot_at,
        direction: spot_dir,
        range: 6.0,
        shadows: None,
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    let mut world = WorldRenderer::new(&gpu);
    world.bloom = Some(Bloom {
        threshold: 0.9,
        intensity: 1.2,
        scatter: 0.7,
    });
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-09", size.0, size.1, &pixels, 3);
}

// M5-RND-10
#[test]
fn directional_shadows() {
    use platypus_render::{Light, LightKind, MeshGlow, MeshInstance, Shadows, primitive_id};
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let eye = Vec3::new(0.0, 3.0, 6.5);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0.0, 0.5, 0.0), Vec3::Y)
            .inverse(),
        50.0,
        0.1,
        50.0,
        size,
        Color::rgb(0.03, 0.03, 0.05),
    );
    let mesh = |name: &str, model: Mat4, color: [f32; 4]| MeshInstance {
        mesh: primitive_id(name).unwrap(),
        model,
        color,
        emission: [0.0; 3],
        smoothness: 0.3,
        metallic: 0.0,
        unlit: false,
        texture: None,
        tiling: [1.0, 1.0, 0.0, 0.0],
        glow: MeshGlow::None,
        cast_shadows: true,
        mirror: None,
        layer: 0,
        ..Default::default()
    };
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.ambient = [0.12, 0.12, 0.14];
    v.meshes = vec![
        mesh(
            "plane",
            Mat4::from_scale(Vec3::splat(0.8)),
            [0.7, 0.7, 0.72, 1.0],
        ),
        mesh(
            "cube",
            Mat4::from_rotation_translation(Quat::from_rotation_y(0.5), Vec3::new(-1.6, 0.5, 0.0)),
            [0.85, 0.35, 0.25, 1.0],
        ),
        mesh(
            "sphere",
            Mat4::from_translation(Vec3::new(0.0, 0.8, 0.3)),
            [0.3, 0.55, 0.9, 1.0],
        ),
        mesh(
            "capsule",
            Mat4::from_translation(Vec3::new(1.6, 1.0, -0.3)),
            [0.35, 0.8, 0.4, 1.0],
        ),
        // A floating plank: its shadow falls apart from it.
        mesh(
            "cube",
            Mat4::from_scale_rotation_translation(
                Vec3::new(1.4, 0.08, 0.5),
                Quat::IDENTITY,
                Vec3::new(0.2, 2.0, 1.4),
            ),
            [0.9, 0.85, 0.6, 1.0],
        ),
    ];
    v.lights = vec![Light {
        kind: LightKind::Directional,
        color: [1.0, 0.97, 0.9],
        position: Vec3::ZERO,
        direction: Vec3::new(-0.5, -1.0, -0.35).normalize(),
        range: 0.0,
        shadows: Some(Shadows {
            soft: true,
            strength: 0.9,
        }),
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    let mut world = WorldRenderer::new(&gpu);
    world.shadow_distance = 14.0;
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-10", size.0, size.1, &pixels, 3);
}

// M5-RND-11
#[test]
fn pbr_material_grid_under_a_sky() {
    use platypus_render::{Light, LightKind, MeshGlow, MeshInstance, Sky, primitive_id};
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let eye = Vec3::new(0.0, 1.6, 7.0);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0.0, 1.1, 0.0), Vec3::Y)
            .inverse(),
        42.0,
        0.1,
        100.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let ball = |x: usize, y: usize| {
        let smoothness = x as f32 / 4.0;
        let metallic = y as f32 / 2.0;
        MeshInstance {
            mesh: primitive_id("sphere").unwrap(),
            model: Mat4::from_scale_rotation_translation(
                Vec3::splat(0.8),
                Quat::IDENTITY,
                Vec3::new(-2.0 + x as f32, 0.4 + y as f32 * 0.9, 0.0),
            ),
            color: [0.9, 0.55, 0.25, 1.0],
            emission: [0.0; 3],
            smoothness,
            metallic,
            unlit: false,
            texture: None,
            tiling: [1.0, 1.0, 0.0, 0.0],
            glow: MeshGlow::None,
            cast_shadows: true,
            mirror: None,
            layer: 0,
            ..Default::default()
        }
    };
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.skybox = true;
    // Columns: rough to smooth; rows: dielectric, half, metal.
    v.meshes = (0..5)
        .flat_map(|x| (0..3).map(move |y| ball(x, y)))
        .collect();
    // A dark, fairly smooth floor reflecting the sky.
    v.meshes.push(MeshInstance {
        mesh: primitive_id("plane").unwrap(),
        model: Mat4::from_translation(Vec3::new(0.0, -0.01, 0.0)),
        color: [0.08, 0.08, 0.09, 1.0],
        smoothness: 0.85,
        metallic: 0.0,
        ..ball(0, 0)
    });
    v.lights = vec![Light {
        kind: LightKind::Directional,
        color: [2.0, 1.9, 1.7],
        position: Vec3::ZERO,
        direction: Vec3::new(-0.4, -0.6, -0.3).normalize(),
        range: 0.0,
        shadows: None,
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    let mut world = WorldRenderer::new(&gpu);
    world.set_sky(Some(Sky::default()));
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-11", size.0, size.1, &pixels, 3);
}

// M5-RND-12
#[test]
fn planar_mirror() {
    use platypus_render::{Light, LightKind, MeshGlow, MeshInstance, Mirror, Sky, primitive_id};
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // Looking at the mirror wall from an angle, from in front and to the right.
    let eye = Vec3::new(2.4, 1.9, 6.8);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(
            eye,
            Vec3::new(-0.2, 1.0, -1.0),
            Vec3::Y,
        )
        .inverse(),
        50.0,
        0.1,
        100.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let mesh = |name: &str, model: Mat4, color: [f32; 4]| MeshInstance {
        mesh: primitive_id(name).unwrap(),
        model,
        color,
        emission: [0.0; 3],
        smoothness: 0.4,
        metallic: 0.0,
        unlit: false,
        texture: None,
        tiling: [1.0, 1.0, 0.0, 0.0],
        glow: MeshGlow::None,
        cast_shadows: true,
        mirror: None,
        layer: 0,
        ..Default::default()
    };
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.skybox = true;
    let mirror_at = Vec3::new(0.0, 1.25, -1.5);
    v.meshes = vec![
        mesh(
            "plane",
            Mat4::from_scale(Vec3::splat(0.6)),
            [0.35, 0.37, 0.4, 1.0],
        ),
        mesh(
            "cube",
            Mat4::from_scale_rotation_translation(
                Vec3::splat(0.7),
                Quat::from_rotation_y(0.5),
                Vec3::new(-1.3, 0.35, 0.1),
            ),
            [0.85, 0.25, 0.2, 1.0],
        ),
        mesh(
            "sphere",
            Mat4::from_scale_rotation_translation(
                Vec3::splat(0.8),
                Quat::IDENTITY,
                Vec3::new(0.2, 0.4, -0.5),
            ),
            [0.25, 0.45, 0.9, 1.0],
        ),
        mesh(
            "capsule",
            Mat4::from_scale_rotation_translation(
                Vec3::splat(0.6),
                Quat::IDENTITY,
                Vec3::new(1.4, 0.6, 0.2),
            ),
            [0.3, 0.8, 0.35, 1.0],
        ),
        // Behind the mirror: it must not show in the reflection.
        mesh(
            "cube",
            Mat4::from_translation(Vec3::new(0.0, 0.5, -2.6)),
            [1.0, 0.9, 0.1, 1.0],
        ),
        // The mirror: a slightly blue-tinted quad facing +z, 4 × 2.5 m.
        MeshInstance {
            color: [0.92, 0.95, 1.0, 1.0],
            smoothness: 1.0,
            metallic: 1.0,
            mirror: Some(Mirror {
                normal: Vec3::Z,
                point: mirror_at,
                culling_mask: ALL_LAYERS,
            }),
            ..mesh(
                "quad",
                Mat4::from_scale_rotation_translation(
                    Vec3::new(4.0, 2.5, 1.0),
                    Quat::IDENTITY,
                    mirror_at,
                ),
                [1.0; 4],
            )
        },
    ];
    v.lights = vec![Light {
        kind: LightKind::Directional,
        color: [1.6, 1.5, 1.35],
        position: Vec3::ZERO,
        direction: Vec3::new(-0.3, -0.8, -0.5).normalize(),
        range: 0.0,
        shadows: None,
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    let mut world = WorldRenderer::new(&gpu);
    world.set_sky(Some(Sky::default()));
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-12", size.0, size.1, &pixels, 3);
}

// M5-RND-13
#[test]
fn reflection_probe_in_a_room() {
    use platypus_render::{
        Light, LightKind, MeshGlow, MeshInstance, ReflectionProbe, primitive_id,
    };
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let eye = Vec3::new(0.0, 1.5, 3.4);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0.0, 1.0, 0.0), Vec3::Y)
            .inverse(),
        60.0,
        0.05,
        50.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let block = |at: Vec3, scale: Vec3, color: [f32; 3]| MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        model: Mat4::from_scale_rotation_translation(scale, Quat::IDENTITY, at),
        color: [color[0], color[1], color[2], 1.0],
        emission: [0.0; 3],
        smoothness: 0.2,
        metallic: 0.0,
        unlit: false,
        texture: None,
        tiling: [1.0, 1.0, 0.0, 0.0],
        glow: MeshGlow::None,
        cast_shadows: false,
        mirror: None,
        layer: 0,
        ..Default::default()
    };
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.ambient = [0.25, 0.25, 0.25];
    // A 4 m room: red and green side walls, a blue back wall, white floor and ceiling,
    // open behind the camera; a chrome ball and a rough gold one in the middle.
    v.meshes = vec![
        block(
            Vec3::new(0.0, -0.05, 0.0),
            Vec3::new(4.0, 0.1, 6.0),
            [0.8, 0.8, 0.8],
        ),
        block(
            Vec3::new(0.0, 3.05, 0.0),
            Vec3::new(4.0, 0.1, 6.0),
            [0.8, 0.8, 0.8],
        ),
        block(
            Vec3::new(-2.05, 1.5, 0.0),
            Vec3::new(0.1, 3.0, 6.0),
            [0.8, 0.1, 0.08],
        ),
        block(
            Vec3::new(2.05, 1.5, 0.0),
            Vec3::new(0.1, 3.0, 6.0),
            [0.1, 0.6, 0.12],
        ),
        block(
            Vec3::new(0.0, 1.5, -3.05),
            Vec3::new(4.0, 3.0, 0.1),
            [0.15, 0.25, 0.8],
        ),
        MeshInstance {
            mesh: primitive_id("sphere").unwrap(),
            color: [0.95, 0.95, 0.95, 1.0],
            smoothness: 1.0,
            metallic: 1.0,
            ..block(Vec3::new(-0.6, 0.8, 0.0), Vec3::splat(1.1), [1.0; 3])
        },
        MeshInstance {
            mesh: primitive_id("sphere").unwrap(),
            color: [1.0, 0.77, 0.34, 1.0],
            smoothness: 0.6,
            metallic: 1.0,
            ..block(Vec3::new(0.9, 0.6, 0.6), Vec3::splat(0.8), [1.0; 3])
        },
    ];
    v.lights = vec![Light {
        kind: LightKind::Point,
        color: [6.0, 5.6, 5.0],
        position: Vec3::new(0.0, 2.6, 0.5),
        direction: Vec3::ZERO,
        range: 8.0,
        shadows: None,
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    v.probes = vec![ReflectionProbe {
        id: 1,
        position: Vec3::new(0.0, 1.2, 0.0),
        half_extents: Vec3::new(2.0, 1.5, 3.0),
        resolution: 64,
        box_projection: true,
        render: true,
        culling_mask: ALL_LAYERS,
    }];
    let mut world = WorldRenderer::new(&gpu);
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-13", size.0, size.1, &pixels, 3);
}

// M5-RND-14
#[test]
fn quality_low_and_high() {
    use platypus_render::{
        Light, LightKind, MeshGlow, MeshInstance, ShadowQuality, Shadows, primitive_id,
    };
    let gpu = shared_gpu();
    let size = (240, 160);
    let scene = || {
        let eye = Vec3::new(0.0, 2.2, 5.0);
        let camera = WorldCamera::perspective(
            platypus_core::glam::camera::rh::view::look_at_mat4(
                eye,
                Vec3::new(0.0, 0.6, 0.0),
                Vec3::Y,
            )
            .inverse(),
            45.0,
            0.1,
            50.0,
            size,
            Color::rgb(0.1, 0.12, 0.16),
        );
        let bar = |model: Mat4, color: [f32; 4]| MeshInstance {
            mesh: primitive_id("cube").unwrap(),
            model,
            color,
            emission: [0.0; 3],
            smoothness: 0.3,
            metallic: 0.0,
            unlit: false,
            texture: None,
            tiling: [1.0, 1.0, 0.0, 0.0],
            glow: MeshGlow::None,
            cast_shadows: true,
            mirror: None,
            layer: 0,
            ..Default::default()
        };
        let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
        v.meshes.push(MeshInstance {
            mesh: primitive_id("plane").unwrap(),
            ..bar(Mat4::from_scale(Vec3::splat(0.5)), [0.75, 0.75, 0.72, 1.0])
        });
        // A fence of thin, leaning bars: their edges and shadows alias at low quality.
        for i in 0..9 {
            let x = -2.0 + i as f32 * 0.5;
            v.meshes.push(bar(
                Mat4::from_scale_rotation_translation(
                    Vec3::new(0.05, 1.4, 0.05),
                    Quat::from_rotation_z(0.35),
                    Vec3::new(x, 0.65, 0.0),
                ),
                [0.85, 0.3, 0.2, 1.0],
            ));
        }
        v.lights = vec![Light {
            kind: LightKind::Directional,
            color: [1.0, 0.97, 0.9],
            position: Vec3::ZERO,
            direction: Vec3::new(-0.6, -1.0, -0.9).normalize(),
            range: 0.0,
            shadows: Some(Shadows {
                soft: true,
                strength: 0.85,
            }),
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        }];
        v
    };
    let render = |configure: &dyn Fn(&mut WorldRenderer)| {
        let target =
            OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
        let view = target.view().unwrap();
        let rt = RenderTarget {
            view: &view,
            format: target.format(),
            size,
            scale: 1.0,
        };
        let mut world = WorldRenderer::new(&gpu);
        world.shadow_distance = 10.0;
        configure(&mut world);
        world.render_views(&gpu, &rt, &[scene()], &[]);
        target.read_pixels(&gpu).unwrap()
    };
    // Low: half resolution, no MSAA, small hard shadows. High: 4× MSAA, full resolution,
    // big soft shadows with the wide filter.
    let low = render(&|w| {
        w.render_scale = 0.5;
        w.msaa = 1;
        w.shadow_resolution = 512;
        w.shadow_quality = ShadowQuality::HardOnly;
    });
    let high = render(&|w| {
        w.render_scale = 1.0;
        w.msaa = 4;
        w.shadow_resolution = 2048;
        w.soft_shadow_radius = 2;
    });
    // Side by side: low on the left.
    let row = (size.0 * 4) as usize;
    let mut pixels = Vec::with_capacity(row * 2 * size.1 as usize);
    for y in 0..size.1 as usize {
        pixels.extend_from_slice(&low[y * row..(y + 1) * row]);
        pixels.extend_from_slice(&high[y * row..(y + 1) * row]);
    }
    assert_golden("M5-RND-14", size.0 * 2, size.1, &pixels, 3);
}

// M5-RND-15
#[test]
fn light_and_mirror_culling_masks() {
    use platypus_render::{
        Light, LightKind, MeshGlow, MeshInstance, Mirror, Shadows, primitive_id,
    };
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let eye = Vec3::new(1.8, 2.4, 6.5);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(
            eye,
            Vec3::new(0.0, 0.8, -0.5),
            Vec3::Y,
        )
        .inverse(),
        50.0,
        0.1,
        50.0,
        size,
        Color::rgb(0.03, 0.03, 0.05),
    );
    let mesh = |name: &str, model: Mat4, color: [f32; 4], layer: u8| MeshInstance {
        mesh: primitive_id(name).unwrap(),
        model,
        color,
        emission: [0.0; 3],
        smoothness: 0.3,
        metallic: 0.0,
        unlit: false,
        texture: None,
        tiling: [1.0, 1.0, 0.0, 0.0],
        glow: MeshGlow::None,
        cast_shadows: true,
        mirror: None,
        layer,
        ..Default::default()
    };
    let mirror_at = Vec3::new(0.0, 1.2, -1.8);
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.ambient = [0.1, 0.1, 0.12];
    v.meshes = vec![
        mesh(
            "plane",
            Mat4::from_scale(Vec3::splat(0.8)),
            [0.45, 0.45, 0.48, 1.0],
            0,
        ),
        // Layer 1: lit by the sun, casting its shadow, but not in the mirror.
        mesh(
            "cube",
            Mat4::from_rotation_translation(Quat::from_rotation_y(0.5), Vec3::new(-1.6, 0.5, 0.0)),
            [0.85, 0.85, 0.85, 1.0],
            1,
        ),
        // Layer 2: only the red lamp lights it; it casts no sun shadow.
        mesh(
            "cube",
            Mat4::from_rotation_translation(Quat::from_rotation_y(-0.4), Vec3::new(1.5, 0.5, 0.2)),
            [0.85, 0.85, 0.85, 1.0],
            2,
        ),
        mesh(
            "sphere",
            Mat4::from_translation(Vec3::new(0.0, 0.6, 0.4)),
            [0.3, 0.55, 0.9, 1.0],
            0,
        ),
        // A mirror that skips layer 1.
        MeshInstance {
            smoothness: 1.0,
            metallic: 1.0,
            mirror: Some(Mirror {
                normal: Vec3::Z,
                point: mirror_at,
                culling_mask: !(1 << 1),
            }),
            ..mesh(
                "quad",
                Mat4::from_scale_rotation_translation(
                    Vec3::new(4.5, 2.4, 1.0),
                    Quat::IDENTITY,
                    mirror_at,
                ),
                [0.92, 0.95, 1.0, 1.0],
                0,
            )
        },
    ];
    v.lights = vec![
        Light {
            kind: LightKind::Directional,
            color: [0.9, 0.87, 0.8],
            position: Vec3::ZERO,
            direction: Vec3::new(0.7, -1.0, -0.5).normalize(),
            range: 0.0,
            shadows: Some(Shadows {
                soft: true,
                strength: 0.9,
            }),
            culling_mask: 0b011,
            volumetric: 1.0,
        },
        Light {
            kind: LightKind::Point,
            color: [6.0, 0.4, 0.3],
            position: Vec3::new(1.0, 1.6, 1.3),
            direction: Vec3::NEG_Y,
            range: 6.0,
            shadows: None,
            culling_mask: 0b100,
            volumetric: 1.0,
        },
    ];
    let mut world = WorldRenderer::new(&gpu);
    world.shadow_distance = 14.0;
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-15", size.0, size.1, &pixels, 3);
}

// M5-CUL-02
#[test]
fn frustum_culling_skips_what_cameras_cant_see_but_keeps_its_shadows() {
    use platypus_render::{
        Light, LightKind, MeshGlow, MeshInstance, Shadows, WorldStats, primitive_id,
    };
    let gpu = shared_gpu();
    let size = (128, 128);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // Straight down at the floor from 4 m up: a 60 degree view covers about 4.6 m.
    let eye = Vec3::new(0.0, 4.0, 0.0);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::NEG_Z).inverse(),
        60.0,
        0.1,
        50.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let mesh = |name: &str, model: Mat4, cast_shadows: bool| MeshInstance {
        mesh: primitive_id(name).unwrap(),
        model,
        color: [0.8, 0.8, 0.8, 1.0],
        emission: [0.0; 3],
        smoothness: 0.2,
        metallic: 0.0,
        unlit: false,
        texture: None,
        tiling: [1.0, 1.0, 0.0, 0.0],
        glow: MeshGlow::None,
        cast_shadows,
        mirror: None,
        layer: 0,
        ..Default::default()
    };
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.ambient = [0.1, 0.1, 0.1];
    v.meshes = vec![
        mesh("plane", Mat4::IDENTITY, true),
        // Behind the camera (above it): not drawn, but its shadow lands in view.
        mesh(
            "cube",
            Mat4::from_scale_rotation_translation(
                Vec3::new(1.5, 0.2, 1.5),
                Quat::IDENTITY,
                Vec3::new(-1.0, 6.0, 0.0),
            ),
            true,
        ),
        // Far off to the side and casting nothing: skipped altogether.
        mesh(
            "sphere",
            Mat4::from_translation(Vec3::new(30.0, 0.0, 0.0)),
            false,
        ),
    ];
    v.quads = vec![WorldQuad::colored(
        Mat4::from_translation(Vec3::new(0.0, 0.0, 40.0)),
        [1.0; 4],
    )];
    v.lights = vec![
        Light {
            kind: LightKind::Directional,
            color: [1.0, 1.0, 1.0],
            position: Vec3::ZERO,
            direction: Vec3::NEG_Y,
            range: 0.0,
            shadows: Some(Shadows {
                soft: false,
                strength: 1.0,
            }),
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        },
        // Out of reach of the view.
        Light {
            kind: LightKind::Point,
            color: [5.0, 0.0, 0.0],
            position: Vec3::new(40.0, 1.0, 0.0),
            direction: Vec3::NEG_Y,
            range: 5.0,
            shadows: None,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        },
    ];
    let mut world = WorldRenderer::new(&gpu);
    world.shadow_distance = 20.0;
    world.render_views(&gpu, &rt, std::slice::from_ref(&v), &[]);
    assert_eq!(
        world.stats(),
        WorldStats {
            meshes: 1,
            culled_meshes: 2,
            quads: 0,
            culled_quads: 1,
            fog_culled: 0,
            lights: 1,
            culled_lights: 1,
            shadow_maps: 1,
            shadow_casters: 2,
            bones: 0,
        }
    );
    let pixels = target.read_pixels(&gpu).unwrap();
    let at = |x: u32, y: u32| pixels[((y * size.0 + x) * 4) as usize];
    // The plank shadows the floor below it (left of centre), not the right side.
    let (shade, lit) = (at(64 - 27, 64), at(64 + 27, 64));
    assert!(shade + 60 < lit, "shadow {shade} vs lit {lit}");

    // M5-CUL-03: what was seen, for animation culling. The floor is in view; the plank is not,
    // but casts into the sun's shadow map; the far sphere is neither, casting or not.
    for (i, m) in v.meshes.iter_mut().enumerate() {
        m.visibility_key = i as u32 + 1;
    }
    world.render_views(&gpu, &rt, std::slice::from_ref(&v), &[]);
    assert_eq!(world.seen(), [1, 2]);
    v.meshes[2].cast_shadows = true;
    v.meshes[1].cast_shadows = false;
    world.render_views(&gpu, &rt, std::slice::from_ref(&v), &[]);
    assert_eq!(world.seen(), [1], "a plank casting nothing is unseen");
    v.meshes[0].visibility_key = 0;
    world.render_views(&gpu, &rt, std::slice::from_ref(&v), &[]);
    assert!(world.seen().is_empty(), "untracked meshes are not reported");

    // Looking along the ground under a high sun: a figure behind the camera is in the sun's
    // shadow map (it covers round the camera) but its shadow falls behind it, out of view.
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::new(0.0, 1.0, -10.0),
            Vec3::Y,
        )
        .inverse(),
        60.0,
        0.1,
        50.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let mut w = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    w.lights = vec![v.lights[0]];
    w.meshes = vec![
        mesh(
            "cube",
            Mat4::from_translation(Vec3::new(0.0, 0.5, -5.0)),
            true,
        ),
        mesh(
            "cube",
            Mat4::from_translation(Vec3::new(0.0, 0.5, 8.0)),
            true,
        ),
    ];
    w.meshes[0].visibility_key = 1;
    w.meshes[1].visibility_key = 2;
    world.render_views(&gpu, &rt, std::slice::from_ref(&w), &[]);
    assert_eq!(world.seen(), [1], "the figure behind is unseen");
    // With a low sun behind the camera, its shadow stretches forward into view.
    w.lights[0].direction = Vec3::new(0.0, -0.3, -1.0).normalize();
    world.render_views(&gpu, &rt, std::slice::from_ref(&w), &[]);
    assert_eq!(world.seen(), [1, 2]);
}

// M5-CUL-06
#[test]
fn a_render_texture_camera_counts_for_what_was_seen() {
    use platypus_render::MeshInstance;
    let gpu = shared_gpu();
    let size = (64, 64);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let camera = |eye: Vec3, at: Vec3| {
        WorldCamera::perspective(
            platypus_core::glam::camera::rh::view::look_at_mat4(eye, at, Vec3::Y).inverse(),
            60.0,
            0.1,
            50.0,
            size,
            Color::rgb(0.0, 0.0, 0.0),
        )
    };
    // A figure at the origin; the screen's camera looks away from it.
    let figure = MeshInstance {
        mesh: platypus_render::primitive_id("cube").unwrap(),
        visibility_key: 7,
        ..Default::default()
    };
    let mut screen = platypus_render::WorldView::new(
        camera(Vec3::new(0.0, 1.0, 5.0), Vec3::new(0.0, 1.0, 20.0)),
        [0.0, 0.0, 1.0, 1.0],
    );
    screen.meshes = vec![figure];
    let mut world = WorldRenderer::new(&gpu);
    world.render_views(&gpu, &rt, std::slice::from_ref(&screen), &[]);
    assert!(world.seen().is_empty());
    // A second camera draws it into a render texture (what a tool window shows through an
    // `image`: secondary windows draw no world of their own): seen.
    world.set_render_texture(&gpu, 3, (32, 32));
    let mut monitor = platypus_render::WorldView::new(
        camera(Vec3::new(0.0, 1.0, 5.0), Vec3::ZERO),
        [0.0, 0.0, 1.0, 1.0],
    );
    monitor.meshes = vec![figure];
    monitor.target_texture = Some(3);
    world.render_views(&gpu, &rt, &[screen, monitor], &[]);
    assert_eq!(world.seen(), [7]);
}

// M5-CUL-05
#[test]
fn skinned_meshes_are_culled_by_their_bones_and_leave_shadow_maps_they_miss() {
    use platypus_render::{Light, LightKind, MeshData, MeshInstance, Shadows};
    let gpu = shared_gpu();
    let size = (96, 96);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // A half-metre box, every vertex on bone 0.
    let mut cube = MeshData::default();
    for i in 0..8u32 {
        let p = [
            if i & 1 == 0 { -0.25 } else { 0.25 },
            if i & 2 == 0 { 0.0 } else { 0.5 },
            if i & 4 == 0 { -0.25 } else { 0.25 },
        ];
        cube.positions.push(p);
        cube.normals.push([0.0, 1.0, 0.0]);
        cube.uvs.push([0.0, 0.0]);
        cube.joints.push([0, 0, 0, 0]);
        cube.weights.push([1.0, 0.0, 0.0, 0.0]);
    }
    cube.indices.extend([
        0, 1, 3, 0, 3, 2, 4, 6, 7, 4, 7, 5, 0, 4, 5, 0, 5, 1, 2, 3, 7, 2, 7, 6,
    ]);
    let mut world = WorldRenderer::new(&gpu);
    assert!(world.set_mesh(&gpu, 9, &cube));
    // Looking down at the origin from 6 m; the sun's maps cover what is near the camera.
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(
            Vec3::new(0.0, 6.0, 0.01),
            Vec3::ZERO,
            Vec3::NEG_Z,
        )
        .inverse(),
        60.0,
        0.1,
        50.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    // Three figures by their bones: under the camera, 4 m off screen beside it (but in the
    // sun's maps), and 200 m away (in neither). Their `model` is ignored.
    v.bones = vec![
        Mat4::IDENTITY,
        Mat4::from_translation(Vec3::new(4.5, 0.0, 0.0)),
        Mat4::from_translation(Vec3::new(200.0, 0.0, 0.0)),
    ]
    .into();
    v.meshes = (0..3)
        .map(|i| MeshInstance {
            mesh: 9,
            bones: Some((i, 1)),
            cast_shadows: true,
            visibility_key: i + 1,
            ..Default::default()
        })
        .collect();
    v.lights = vec![Light {
        kind: LightKind::Directional,
        color: [1.0, 1.0, 1.0],
        position: Vec3::ZERO,
        direction: Vec3::new(-0.3, -1.0, 0.2).normalize(),
        range: 0.0,
        shadows: Some(Shadows {
            soft: false,
            strength: 1.0,
        }),
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    world.shadow_distance = 20.0;
    world.render_views(&gpu, &rt, std::slice::from_ref(&v), &[]);
    let stats = world.stats();
    assert_eq!((stats.meshes, stats.culled_meshes), (1, 2), "{stats:?}");
    assert_eq!(
        stats.shadow_casters, 2,
        "the far one casts into no map: {stats:?}"
    );
    // Animation culling sees through the same boxes (the one beside casts into view).
    assert_eq!(world.seen(), [1, 2]);

    // M5-CUL-07: only the drawn figures' bones go to the GPU, once for the shadow maps and
    // every view sharing the list; the far one's never.
    assert_eq!(stats.bones, 2, "{stats:?}");
    let second = v.clone();
    world.render_views(&gpu, &rt, &[v.clone(), second], &[]);
    assert_eq!(world.stats().bones, 2, "one copy for both views");
    let pixels = target.read_pixels(&gpu).unwrap();
    // The same frame as one view's (the second draws over the first, alike).
    world.render_views(&gpu, &rt, std::slice::from_ref(&v), &[]);
    assert_eq!(target.read_pixels(&gpu).unwrap(), pixels);
}

// M5-RND-16
#[test]
fn backdrop_blur_over_the_world_blurs_the_world() {
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // Straight at a wall of hard-edged vertical stripes, black and white.
    let camera = WorldCamera::perspective(
        Mat4::from_translation(Vec3::new(0.0, 0.0, 4.0)),
        60.0,
        0.1,
        100.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let quads: Vec<WorldQuad> = (0..16)
        .map(|i| {
            WorldQuad::colored(
                place(
                    Vec3::new(-3.75 + i as f32 * 0.5, 0.0, 0.0),
                    Quat::IDENTITY,
                    (0.5, 6.0),
                ),
                if i % 2 == 0 {
                    [1.0; 4]
                } else {
                    [0.0, 0.0, 0.0, 1.0]
                },
            )
        })
        .collect();

    // A frosted panel in the middle of the screen.
    let mut tree = UiTree::new(320.0, 200.0);
    let errors = tree.apply_commit(&Commit {
        ops: vec![
            op(CREATE, &[Value::Number(1.0), "box".into()]),
            op(
                SET_STYLE,
                &[
                    Value::Number(1.0),
                    "position".into(),
                    "absolute".into(),
                    "left".into(),
                    Value::Number(80.0),
                    "top".into(),
                    Value::Number(50.0),
                    "width".into(),
                    Value::Number(160.0),
                    "height".into(),
                    Value::Number(100.0),
                    "borderRadius".into(),
                    Value::Number(16.0),
                    "backgroundColor".into(),
                    "rgba(40, 60, 120, 0.25)".into(),
                    "backdropFilter".into(),
                    "blur(10px)".into(),
                ],
            ),
            op(APPEND_CHILD, &[Value::Number(0.0), Value::Number(1.0)]),
        ],
    });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
    let cmds = tree.draw_list();
    assert!(platypus_render::needs_backdrop(&cmds));

    // The app's path: the world into the backdrop texture, the UI over a copy of it.
    let mut ui = UiRenderer::new(&gpu);
    ui.set_blending(platypus_render::Blending::Linear);
    let backdrop = ui.backdrop_target(&gpu, size, rt.format);
    let world_rt = RenderTarget {
        view: &backdrop,
        format: rt.format,
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    world.render(&gpu, &world_rt, &camera, &quads, &[]);
    ui.render_over(&gpu, &rt, &backdrop, &cmds);

    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-16", size.0, size.1, &pixels, 2);
    // The biggest jump in brightness between neighbouring pixels along a row: sharp
    // stripes outside the panel, smooth ones inside.
    let jump = |y: u32, xs: std::ops::Range<u32>| {
        xs.map(|x| {
            let at = |x: u32| pixels[((y * size.0 + x) * 4) as usize] as i32;
            (at(x + 1) - at(x)).abs()
        })
        .max()
        .unwrap()
    };
    let outside = jump(20, 20..300);
    let inside = jump(100, 100..220);
    assert!(outside > 200, "stripes outside are sharp: {outside}");
    assert!(
        inside < 60,
        "stripes behind the panel are blurred: {inside}"
    );
}

/// A `TextureData` from a function of (x, y) in 0..1 (top row first) to RGBA bytes.
fn painted(
    n: u32,
    srgb: bool,
    filter: TextureFilter,
    f: impl Fn(f32, f32) -> [u8; 4],
) -> TextureData {
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            rgba.extend(f((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32));
        }
    }
    TextureData {
        width: n,
        height: n,
        rgba,
        filter,
        wrap: TextureWrap::Repeat,
        mipmaps: true,
        anisotropy: 1,
        srgb,
    }
}

// M5-RND-17
#[test]
fn material_maps_and_rendering_modes() {
    use platypus_render::{
        Light, LightKind, MaterialMaps, MeshInstance, RenderMode, Shadows, Sky, primitive_id,
    };
    let gpu = shared_gpu();
    let size = (480, 270);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    world.set_sky(Some(Sky::default()));
    // 1: a normal map of round bumps (data, not sRGB).
    world.set_texture(
        &gpu,
        1,
        &painted(64, false, TextureFilter::Trilinear, |x, y| {
            let (cx, cy) = ((x * 4.0).fract() - 0.5, (y * 4.0).fract() - 0.5);
            let r2 = cx * cx + cy * cy;
            // A dome's slope inside each cell; flat outside. y runs down the image,
            // so the green (up) slope is -cy.
            let (nx, ny) = if r2 < 0.16 {
                (cx * 2.0, -cy * 2.0)
            } else {
                (0.0, 0.0)
            };
            let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
            let b = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
            [b(nx), b(ny), b(nz), 255]
        }),
    );
    // 2: stripes of occlusion (green).
    world.set_texture(
        &gpu,
        2,
        &painted(32, false, TextureFilter::Bilinear, |_, y| {
            let o = if (y * 6.0).fract() < 0.5 { 40 } else { 255 };
            [o, o, o, 255]
        }),
    );
    // 3: a leaf-like cut-out: a round blob with holes, white.
    world.set_texture(
        &gpu,
        3,
        &painted(64, true, TextureFilter::Bilinear, |x, y| {
            let (cx, cy) = (x - 0.5, y - 0.5);
            let inside = cx * cx + cy * cy < 0.2;
            let hole = ((x * 5.0).fract() - 0.5).hypot((y * 5.0).fract() - 0.5) < 0.18;
            [90, 200, 80, if inside && !hole { 255 } else { 0 }]
        }),
    );
    // 4: an emission pattern: glowing grid lines.
    world.set_texture(
        &gpu,
        4,
        &painted(32, true, TextureFilter::Bilinear, |x, y| {
            let line = (x * 4.0).fract() < 0.15 || (y * 4.0).fract() < 0.15;
            if line {
                [255, 200, 80, 255]
            } else {
                [0, 0, 0, 255]
            }
        }),
    );
    // 5: a detail checker, mid grey (no change) and darker squares.
    world.set_texture(
        &gpu,
        5,
        &painted(16, true, TextureFilter::Point, |x, y| {
            let on = ((x * 4.0) as u32 + (y * 4.0) as u32).is_multiple_of(2);
            let v = if on { 188 } else { 120 };
            [v, v, v, 255]
        }),
    );
    let eye = Vec3::new(0.0, 2.4, 6.2);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0.0, 0.7, 0.0), Vec3::Y)
            .inverse(),
        45.0,
        0.1,
        100.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let at = |x: f32, y: f32, z: f32| Mat4::from_translation(Vec3::new(x, y, z));
    let id = |name: &str| primitive_id(name).unwrap();
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.skybox = true;
    v.meshes = vec![
        // The floor, catching the cut-out's shadow with holes.
        MeshInstance {
            mesh: id("plane"),
            color: [0.22, 0.22, 0.24, 1.0],
            smoothness: 0.2,
            ..Default::default()
        },
        // Back row: a bumpy cube lit across its face, an occlusion-striped sphere and a
        // specular-workflow sphere (green-tinted reflectance on a dark base).
        MeshInstance {
            mesh: id("cube"),
            model: at(-2.4, 0.7, -1.0) * Mat4::from_scale(Vec3::splat(1.3)),
            color: [0.8, 0.75, 0.7, 1.0],
            smoothness: 0.45,
            maps: MaterialMaps {
                normal: Some(WorldTexture::Asset(1)),
                normal_scale: 1.5,
                ..Default::default()
            },
            ..Default::default()
        },
        MeshInstance {
            mesh: id("sphere"),
            model: at(-0.8, 0.7, -1.0) * Mat4::from_scale(Vec3::splat(1.3)),
            color: [0.9, 0.9, 0.9, 1.0],
            smoothness: 0.3,
            maps: MaterialMaps {
                occlusion: Some(WorldTexture::Asset(2)),
                ..Default::default()
            },
            ..Default::default()
        },
        MeshInstance {
            mesh: id("sphere"),
            model: at(0.8, 0.7, -1.0) * Mat4::from_scale(Vec3::splat(1.3)),
            color: [0.08, 0.08, 0.1, 1.0],
            smoothness: 0.85,
            specular: Some([0.3, 0.9, 0.4]),
            ..Default::default()
        },
        // An emission-mapped cube with a detail checker over its albedo.
        MeshInstance {
            mesh: id("cube"),
            model: at(2.4, 0.7, -1.0) * Mat4::from_scale(Vec3::splat(1.2)),
            color: [0.5, 0.5, 0.6, 1.0],
            emission: [1.5, 1.5, 1.5],
            maps: MaterialMaps {
                emission: Some(WorldTexture::Asset(4)),
                detail: Some(WorldTexture::Asset(5)),
                detail_tiling: [2.0, 2.0, 0.0, 0.0],
                ..Default::default()
            },
            ..Default::default()
        },
        // Front row: a double-sided cut-out leaf, then a red bar behind a faded
        // sphere and a transparent glass one (the glass keeps its highlight).
        MeshInstance {
            mesh: id("quad"),
            model: at(-1.3, 0.9, 1.6)
                * Mat4::from_rotation_y(0.9)
                * Mat4::from_rotation_x(-0.3)
                * Mat4::from_scale(Vec3::splat(1.1)),
            texture: Some(WorldTexture::Asset(3)),
            mode: RenderMode::Cutout,
            double_sided: true,
            ..Default::default()
        },
        MeshInstance {
            mesh: id("cube"),
            model: at(1.3, 0.55, 0.7) * Mat4::from_scale(Vec3::new(2.6, 0.3, 0.3)),
            color: [0.85, 0.1, 0.1, 1.0],
            ..Default::default()
        },
        MeshInstance {
            mesh: id("sphere"),
            model: at(0.6, 0.55, 1.7) * Mat4::from_scale(Vec3::splat(0.9)),
            color: [0.6, 0.8, 1.0, 0.3],
            smoothness: 0.95,
            mode: RenderMode::Fade,
            ..Default::default()
        },
        MeshInstance {
            mesh: id("sphere"),
            model: at(1.9, 0.55, 1.7) * Mat4::from_scale(Vec3::splat(0.9)),
            color: [0.6, 0.8, 1.0, 0.3],
            smoothness: 0.95,
            mode: RenderMode::Transparent,
            ..Default::default()
        },
    ];
    v.lights = vec![Light {
        kind: LightKind::Directional,
        color: [1.5, 1.45, 1.35],
        position: Vec3::ZERO,
        direction: Vec3::new(0.8, -0.45, -0.35).normalize(),
        range: 0.0,
        shadows: Some(Shadows {
            soft: true,
            strength: 0.9,
        }),
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-17", size.0, size.1, &pixels, 3);
}

// M5-RND-18
#[test]
fn tonemapping_exposure_and_fxaa() {
    use platypus_render::{
        Grading, Light, LightKind, MeshInstance, Sky, Tonemapping, primitive_id,
    };
    let gpu = shared_gpu();
    let size = (200, 150);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // Bright, saturated balls (lit past white) and a thin tilted bar with hard edges.
    let scene = |fxaa: bool| {
        let eye = Vec3::new(0.0, 1.4, 4.2);
        let camera = WorldCamera::perspective(
            platypus_core::glam::camera::rh::view::look_at_mat4(
                eye,
                Vec3::new(0.0, 0.6, 0.0),
                Vec3::Y,
            )
            .inverse(),
            45.0,
            0.1,
            50.0,
            size,
            Color::rgb(0.0, 0.0, 0.0),
        );
        let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
        v.skybox = true;
        v.fxaa = fxaa;
        let ball = |x: f32, color: [f32; 4]| MeshInstance {
            mesh: primitive_id("sphere").unwrap(),
            model: Mat4::from_translation(Vec3::new(x, 0.5, 0.0)),
            color,
            smoothness: 0.7,
            ..Default::default()
        };
        v.meshes = vec![
            MeshInstance {
                mesh: primitive_id("plane").unwrap(),
                color: [0.3, 0.3, 0.32, 1.0],
                ..Default::default()
            },
            ball(-1.1, [1.0, 0.2, 0.1, 1.0]),
            ball(0.0, [0.2, 1.0, 0.3, 1.0]),
            ball(1.1, [0.2, 0.4, 1.0, 1.0]),
            MeshInstance {
                mesh: primitive_id("cube").unwrap(),
                model: Mat4::from_translation(Vec3::new(0.0, 1.35, 0.3))
                    * Mat4::from_rotation_z(0.18)
                    * Mat4::from_scale(Vec3::new(3.0, 0.05, 0.05)),
                color: [1.0, 1.0, 1.0, 1.0],
                unlit: true,
                ..Default::default()
            },
        ];
        v.lights = vec![Light {
            kind: LightKind::Directional,
            color: [4.0, 3.8, 3.5],
            position: Vec3::ZERO,
            direction: Vec3::new(-0.3, -0.8, -0.5).normalize(),
            range: 0.0,
            shadows: None,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        }];
        v
    };
    let mut world = WorldRenderer::new(&gpu);
    world.set_sky(Some(Sky::default()));
    // Clipped, neutral, ACES a stop darker, then ACES with FXAA.
    let looks = [
        (Grading::default(), false),
        (
            Grading {
                exposure: 1.0,
                tonemapping: Tonemapping::Neutral,
                ..Default::default()
            },
            false,
        ),
        (
            Grading {
                exposure: 0.5,
                tonemapping: Tonemapping::Aces,
                ..Default::default()
            },
            false,
        ),
        (
            Grading {
                exposure: 0.5,
                tonemapping: Tonemapping::Aces,
                ..Default::default()
            },
            true,
        ),
    ];
    let shots: Vec<Vec<u8>> = looks
        .iter()
        .map(|&(grading, fxaa)| {
            world.grading = grading;
            world.render_views(&gpu, &rt, &[scene(fxaa)], &[]);
            target.read_pixels(&gpu).unwrap()
        })
        .collect();
    // FXAA changed the bar's edges, and only near edges.
    let changed = shots[2]
        .chunks(4)
        .zip(shots[3].chunks(4))
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 8))
        .count();
    let total = (size.0 * size.1) as usize;
    assert!(
        changed > 50 && changed < total / 5,
        "FXAA changed {changed} pixels"
    );
    // Two by two.
    let row = size.0 as usize * 4;
    let mut pixels = Vec::new();
    for pair in shots.chunks(2) {
        for y in 0..size.1 as usize {
            pixels.extend_from_slice(&pair[0][y * row..(y + 1) * row]);
            pixels.extend_from_slice(&pair[1][y * row..(y + 1) * row]);
        }
    }
    assert_golden("M5-RND-18", size.0 * 2, size.1 * 2, &pixels, 3);
}

// M5-RND-19
#[test]
fn a_camera_draws_into_a_render_texture_shown_on_a_monitor() {
    use platypus_render::{Light, LightKind, MeshInstance, primitive_id};
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    const SCREEN_TEXTURE: u32 = 50;
    world.set_render_texture(&gpu, SCREEN_TEXTURE, (128, 96));
    let id = |n: &str| primitive_id(n).unwrap();
    // A red cube, a green sphere and a blue capsule on a floor; a monitor (a quad showing
    // the render texture) on a post to the right.
    let meshes = vec![
        MeshInstance {
            mesh: id("plane"),
            color: [0.4, 0.4, 0.42, 1.0],
            ..Default::default()
        },
        MeshInstance {
            mesh: id("cube"),
            model: Mat4::from_translation(Vec3::new(-1.2, 0.5, 0.0)),
            color: [0.9, 0.15, 0.1, 1.0],
            ..Default::default()
        },
        MeshInstance {
            mesh: id("sphere"),
            model: Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)),
            color: [0.1, 0.8, 0.2, 1.0],
            ..Default::default()
        },
        MeshInstance {
            mesh: id("capsule"),
            model: Mat4::from_translation(Vec3::new(1.2, 1.0, 0.0))
                * Mat4::from_scale(Vec3::splat(0.5)),
            color: [0.15, 0.3, 0.95, 1.0],
            ..Default::default()
        },
        MeshInstance {
            mesh: id("quad"),
            model: Mat4::from_translation(Vec3::new(2.6, 1.4, 0.8))
                * Mat4::from_rotation_y(-0.5)
                * Mat4::from_scale(Vec3::new(1.6, 1.2, 1.0)),
            texture: Some(WorldTexture::Asset(SCREEN_TEXTURE)),
            unlit: true,
            ..Default::default()
        },
    ];
    let lights = vec![Light {
        kind: LightKind::Directional,
        color: [1.6, 1.5, 1.4],
        position: Vec3::ZERO,
        direction: Vec3::new(-0.4, -0.8, -0.5).normalize(),
        range: 0.0,
        shadows: None,
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    let look = |eye: Vec3, at: Vec3| {
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, at, Vec3::Y).inverse()
    };
    // The texture camera looks down from above and behind, in a yellow fog colour.
    let mut security = platypus_render::WorldView::new(
        WorldCamera::perspective(
            look(Vec3::new(0.0, 4.0, -4.0), Vec3::new(0.3, 0.4, 0.0)),
            55.0,
            0.1,
            50.0,
            (128, 96),
            Color::rgb(0.6, 0.55, 0.2),
        ),
        [0.0, 0.0, 1.0, 1.0],
    );
    security.meshes = meshes.clone();
    security.lights = lights.clone();
    security.target_texture = Some(SCREEN_TEXTURE);
    let mut main = platypus_render::WorldView::new(
        WorldCamera::perspective(
            look(Vec3::new(0.5, 2.2, 6.0), Vec3::new(0.8, 0.8, 0.0)),
            45.0,
            0.1,
            50.0,
            size,
            Color::rgb(0.05, 0.06, 0.09),
        ),
        [0.0, 0.0, 1.0, 1.0],
    );
    main.meshes = meshes;
    main.lights = lights;
    // Twice: the second frame shows the same (the texture is drawn before the screen).
    for _ in 0..2 {
        world.render_views(&gpu, &rt, &[security.clone(), main.clone()], &[]);
    }
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-19", size.0, size.1, &pixels, 3);
}

// M5-RND-20
#[test]
fn cascaded_sun_spot_and_point_light_shadows() {
    use platypus_render::{Light, LightKind, MeshInstance, ReflectionProbe, Shadows, primitive_id};
    let gpu = shared_gpu();
    let size = (360, 220);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let id = |n: &str| primitive_id(n).unwrap();
    let shadows = Some(Shadows {
        soft: true,
        strength: 1.0,
    });
    let eye = Vec3::new(0.0, 3.0, 9.0);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(
            eye,
            Vec3::new(0.0, 0.5, -6.0),
            Vec3::Y,
        )
        .inverse(),
        50.0,
        0.1,
        200.0,
        size,
        Color::rgb(0.03, 0.03, 0.05),
    );
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.ambient = [0.08, 0.08, 0.1];
    // A long floor with posts receding into the distance (the sun's cascades), a cube
    // under a spot light on the left and a ring of pillars round a point light on the
    // right.
    v.meshes.push(MeshInstance {
        mesh: id("plane"),
        model: Mat4::from_scale(Vec3::new(2.0, 1.0, 6.0))
            * Mat4::from_translation(Vec3::new(0.0, 0.0, -3.0)),
        color: [0.6, 0.6, 0.62, 1.0],
        smoothness: 0.1,
        ..Default::default()
    });
    for i in 0..8 {
        v.meshes.push(MeshInstance {
            mesh: id("cube"),
            model: Mat4::from_translation(Vec3::new(0.0, 0.75, 2.0 - i as f32 * 6.0))
                * Mat4::from_scale(Vec3::new(0.3, 1.5, 0.3)),
            color: [0.8, 0.75, 0.6, 1.0],
            ..Default::default()
        });
    }
    v.meshes.push(MeshInstance {
        mesh: id("cube"),
        model: Mat4::from_translation(Vec3::new(-3.5, 0.5, 2.5)),
        color: [0.9, 0.3, 0.2, 1.0],
        ..Default::default()
    });
    for k in 0..6 {
        let a = k as f32 / 6.0 * std::f32::consts::TAU;
        v.meshes.push(MeshInstance {
            mesh: id("cylinder"),
            model: Mat4::from_translation(Vec3::new(3.5 + a.cos() * 1.3, 0.5, 2.0 + a.sin() * 1.3))
                * Mat4::from_scale(Vec3::new(0.25, 0.5, 0.25)),
            color: [0.3, 0.5, 0.9, 1.0],
            ..Default::default()
        });
    }
    v.lights = vec![
        Light {
            kind: LightKind::Directional,
            color: [0.9, 0.85, 0.8],
            position: Vec3::ZERO,
            direction: Vec3::new(0.6, -0.5, 0.25).normalize(),
            range: 0.0,
            shadows,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        },
        Light {
            kind: LightKind::Spot {
                inner_degrees: 30.0,
                outer_degrees: 45.0,
            },
            color: [6.0, 5.0, 3.0],
            position: Vec3::new(-2.6, 3.0, 1.6),
            direction: Vec3::new(-0.3, -1.0, 0.3).normalize(),
            range: 8.0,
            shadows,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        },
        Light {
            kind: LightKind::Point,
            color: [2.0, 4.0, 6.0],
            position: Vec3::new(3.5, 0.6, 2.0),
            direction: Vec3::NEG_Y,
            range: 6.0,
            shadows,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        },
    ];
    let mut world = WorldRenderer::new(&gpu);
    world.shadow_distance = 50.0;
    world.shadow_cascades = 4;
    world.render_views(&gpu, &rt, &[v.clone()], &[]);
    // Four cascades, one spot map and six point faces, drawn once.
    assert_eq!(world.stats().shadow_maps, 4 + 1 + 6);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-20", size.0, size.1, &pixels, 3);

    // A probe refreshed every frame reuses the frame's shadow maps (it used to draw
    // the sun's map again for each of its six faces).
    let mut probed = v;
    probed.probes.push(ReflectionProbe {
        id: 1,
        position: Vec3::new(0.0, 1.0, 0.0),
        half_extents: Vec3::splat(10.0),
        resolution: 32,
        box_projection: false,
        render: true,
        culling_mask: ALL_LAYERS,
    });
    world.render_views(&gpu, &rt, &[probed], &[]);
    assert_eq!(world.stats().shadow_maps, 4 + 1 + 6);
}

// M5-RND-21
#[test]
fn skinned_tube_bends_with_its_bones_and_casts_a_bent_shadow() {
    use platypus_render::{Light, LightKind, MeshData, MeshInstance, Shadows, primitive_id};
    let gpu = shared_gpu();
    let size = (240, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // A square tube 2 m tall, rings every 0.25 m, blending from bone 0 (at the bottom)
    // to bone 1 (1 m up) between 0.75 and 1.25 m.
    let mut tube = MeshData::default();
    let corners = [
        (-0.15f32, -0.15f32),
        (0.15, -0.15),
        (0.15, 0.15),
        (-0.15, 0.15),
    ];
    let rings = 9;
    for r in 0..rings {
        let y = r as f32 * 0.25;
        let t = ((y - 0.75) / 0.5).clamp(0.0, 1.0);
        for (x, z) in corners {
            tube.positions.push([x, y, z]);
            let l = (x * x + z * z).sqrt();
            tube.normals.push([x / l, 0.0, z / l]);
            tube.uvs.push([0.0, 0.0]);
            tube.joints.push([0, 1, 0, 0]);
            tube.weights.push([1.0 - t, t, 0.0, 0.0]);
        }
    }
    for r in 0..rings - 1 {
        for k in 0..4u32 {
            let a = r * 4 + k;
            let b = r * 4 + (k + 1) % 4;
            tube.indices.extend([a, b, b + 4, a, b + 4, a + 4]);
        }
    }
    let mut world = WorldRenderer::new(&gpu);
    assert!(world.set_mesh(&gpu, 7, &tube));
    // Bone 1 bent 45 degrees about z at its joint (1 m up); bone matrices are world
    // times inverse bind.
    let joint = Vec3::new(0.0, 1.0, 0.0);
    let bone1 = Mat4::from_translation(joint) * Mat4::from_rotation_z(0.785);
    let bones = vec![Mat4::IDENTITY, bone1 * Mat4::from_translation(-joint)];
    let eye = Vec3::new(0.0, 1.4, 4.5);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0.0, 0.9, 0.0), Vec3::Y)
            .inverse(),
        45.0,
        0.1,
        50.0,
        size,
        Color::rgb(0.05, 0.06, 0.09),
    );
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.bones = bones.into();
    v.meshes = vec![
        MeshInstance {
            mesh: primitive_id("plane").unwrap(),
            color: [0.55, 0.55, 0.58, 1.0],
            ..Default::default()
        },
        MeshInstance {
            mesh: 7,
            // Ignored for skinned meshes: the bones place it.
            model: Mat4::from_translation(Vec3::new(50.0, 0.0, 0.0)),
            color: [0.85, 0.45, 0.2, 1.0],
            bones: Some((0, 2)),
            ..Default::default()
        },
    ];
    v.lights = vec![Light {
        kind: LightKind::Directional,
        color: [1.6, 1.5, 1.4],
        position: Vec3::ZERO,
        direction: Vec3::new(0.5, -0.8, -0.4).normalize(),
        range: 0.0,
        shadows: Some(Shadows {
            soft: true,
            strength: 1.0,
        }),
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M5-RND-21", size.0, size.1, &pixels, 3);
}

// M5-RND-22
#[test]
fn editor_grid_lies_on_the_ground_with_coloured_axes_behind_objects() {
    use platypus_render::{MeshInstance, primitive_id};
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    let eye = Vec3::new(6.0, 4.0, 8.0);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y).inverse(),
        60.0,
        0.1,
        500.0,
        size,
        Color::rgb(0.2, 0.2, 0.22),
    );
    let view_proj = camera.view_proj;
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.grid = true;
    // An unlit cube on the origin hides the axes' crossing.
    v.meshes = vec![MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        model: Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)),
        color: [0.9, 0.7, 0.2, 1.0],
        unlit: true,
        ..Default::default()
    }];
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    // Where a world point lands on screen.
    let at = |p: Vec3| {
        let c = view_proj.project_point3(p);
        let x = ((c.x + 1.0) / 2.0 * size.0 as f32) as usize;
        let y = ((1.0 - c.y) / 2.0 * size.1 as f32) as usize;
        let i = (y * size.0 as usize + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let x_axis = at(Vec3::new(3.0, 0.0, 0.0));
    assert!(x_axis[0] > x_axis[2] + 40, "the X axis is red: {x_axis:?}");
    let z_axis = at(Vec3::new(0.0, 0.0, 3.0));
    assert!(z_axis[2] > z_axis[0] + 40, "the Z axis is blue: {z_axis:?}");
    let cube = at(Vec3::new(0.0, 0.5, 0.0));
    assert!(
        cube[0] > 230 && cube[1] > 200 && cube[2] < 140,
        "the cube hides the grid: {cube:?}"
    );
    assert_golden("M5-RND-22", size.0, size.1, &pixels, 3);
}

// M5-RND-23
#[test]
fn gizmo_lines_keep_their_width_and_show_faintly_behind_things() {
    use platypus_render::{GizmoLine, MeshInstance, primitive_id};
    let gpu = shared_gpu();
    let size = (240, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    let eye = Vec3::new(0.0, 0.0, 6.0);
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y).inverse(),
        50.0,
        0.1,
        100.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let view_proj = camera.view_proj;
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.meshes = vec![MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        color: [0.2, 0.2, 0.25, 1.0],
        unlit: true,
        ..Default::default()
    }];
    let white = [1.0, 1.0, 1.0, 1.0];
    // A wire square in front of the cube, and a line behind it (hidden in the middle),
    // and one reaching behind the camera.
    let square = [
        [-1.5, -1.0, 1.0],
        [1.5, -1.0, 1.0],
        [1.5, 1.0, 1.0],
        [-1.5, 1.0, 1.0],
    ];
    for k in 0..4 {
        v.lines.push(GizmoLine {
            a: square[k],
            b: square[(k + 1) % 4],
            color: white,
        });
    }
    v.lines.push(GizmoLine {
        a: [-2.0, 0.0, -1.0],
        b: [2.0, 0.0, -1.0],
        color: [1.0, 0.8, 0.2, 1.0],
    });
    v.lines.push(GizmoLine {
        a: [2.0, -1.8, 0.0],
        b: [2.0, -1.8, 20.0],
        color: [0.3, 0.6, 1.0, 1.0],
    });
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    let at = |p: Vec3| {
        let c = view_proj.project_point3(p);
        let x = ((c.x + 1.0) / 2.0 * size.0 as f32) as usize;
        let y = ((1.0 - c.y) / 2.0 * size.1 as f32) as usize;
        let i = (y * size.0 as usize + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let edge = at(Vec3::new(0.0, 1.0, 1.0));
    assert!(edge.iter().all(|&c| c > 200), "the square's edge: {edge:?}");
    let shown = at(Vec3::new(-1.8, 0.0, -1.0));
    assert!(shown[0] > 200, "the line beside the cube: {shown:?}");
    let hidden = at(Vec3::new(0.0, 0.0, -1.0));
    assert!(
        hidden[0] > 70 && hidden[0] < 170,
        "the line behind the cube is faint: {hidden:?}"
    );
    assert_golden("M5-RND-23", size.0, size.1, &pixels, 3);
}

// M5-RND-24
#[test]
fn strokes_are_smooth_capsules_with_round_joins_and_hide_behind_things() {
    use platypus_render::{MeshInstance, Projection, Stroke, primitive_id};
    let gpu = shared_gpu();
    let size = (240, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    // A 2D camera: 40 pixels a unit (160 pixels tall, a half height of 2).
    let camera = WorldCamera::new(
        Mat4::from_translation(Vec3::new(0.0, 0.0, 10.0)),
        Projection::Orthographic { half_height: 2.0 },
        0.1,
        100.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let view_proj = camera.view_proj;
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    // A grey cube in front of where the blue stroke runs.
    v.meshes = vec![MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        model: Mat4::from_translation(Vec3::new(1.5, -1.0, 0.0))
            * Mat4::from_scale(Vec3::splat(0.8)),
        color: [0.3, 0.3, 0.3, 1.0],
        unlit: true,
        ..Default::default()
    }];
    let white = [1.0, 1.0, 1.0, 1.0];
    // A white zigzag 0.3 units (12 pixels) wide.
    let zigzag = [
        [-2.4, 1.2, 0.5],
        [-1.2, 0.2, 0.5],
        [0.0, 1.2, 0.5],
        [1.2, 0.2, 0.5],
    ];
    for k in 0..3 {
        v.strokes.push(Stroke {
            a: zigzag[k],
            b: zigzag[k + 1],
            width: 0.3,
            color: white,
        });
    }
    // A thin orange line (2 pixels) and a blue one behind the cube.
    v.strokes.push(Stroke {
        a: [-2.5, -1.5, 0.0],
        b: [0.5, -1.5, 0.0],
        width: 0.05,
        color: [1.0, 0.5, 0.1, 1.0],
    });
    v.strokes.push(Stroke {
        a: [0.5, -1.0, -1.0],
        b: [2.8, -1.0, -1.0],
        width: 0.25,
        color: [0.2, 0.4, 1.0, 1.0],
    });
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    let px = |p: Vec3| {
        let c = view_proj.project_point3(p);
        let x = ((c.x + 1.0) / 2.0 * size.0 as f32) as usize;
        let y = ((1.0 - c.y) / 2.0 * size.1 as f32) as usize;
        (x, y)
    };
    let at = |p: Vec3| {
        let (x, y) = px(p);
        let i = (y * size.0 as usize + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // Filled along the segments and at the joints (round joins leave no notch).
    for p in [
        Vec3::new(-1.8, 0.7, 0.5),
        Vec3::new(-1.2, 0.2, 0.5),
        Vec3::new(0.0, 1.2, 0.5),
    ] {
        let c = at(p);
        assert!(c.iter().all(|&v| v > 200), "white at {p}: {c:?}");
    }
    // Round caps: just past the end it's still drawn; a square cap's corner isn't.
    let end = Vec3::new(1.2, 0.2, 0.5);
    let dir = (end - Vec3::new(0.0, 1.2, 0.5)).normalize();
    let across = Vec3::new(-dir.y, dir.x, 0.0);
    let cap = at(end + dir * 0.1);
    assert!(cap[0] > 200, "inside the round cap: {cap:?}");
    let corner = at(end + dir * 0.14 + across * 0.14);
    assert!(corner[0] < 60, "outside the round cap's corner: {corner:?}");
    let outside = at(Vec3::new(-1.2, -0.25, 0.5));
    assert!(outside[0] < 30, "below the joint: {outside:?}");
    // Soft edges: a column across a segment has partly covered pixels.
    let (x, y) = px(Vec3::new(-1.8, 0.7, 0.5));
    let partial = (y.saturating_sub(15)..y + 15)
        .map(|row| pixels[(row * size.0 as usize + x) * 4])
        .filter(|&v| v > 30 && v < 220)
        .count();
    assert!(
        partial >= 2,
        "partly covered pixels at the edges: {partial}"
    );
    // A 2-pixel line still shows.
    let thin = at(Vec3::new(-1.0, -1.5, 0.0));
    assert!(thin[0] > 150, "the thin line: {thin:?}");
    // The blue line shows beside the cube and is hidden by it.
    let beside = at(Vec3::new(2.4, -1.0, -1.0));
    assert!(
        beside[2] > 150 && beside[0] < 150,
        "beside the cube: {beside:?}"
    );
    let behind = at(Vec3::new(1.5, -1.0, -1.0));
    assert!(
        (behind[2] as i32 - behind[0] as i32).abs() < 20,
        "the cube hides the blue line: {behind:?}"
    );
    assert_golden("M5-RND-24", size.0, size.1, &pixels, 3);
}

// M5-RND-25
#[test]
fn fills_follow_their_rule_and_sit_under_their_outlines() {
    use platypus_render::{FillRule, FillVertex, Projection, Stroke, tessellate_fill};
    let gpu = shared_gpu();
    let size = (240, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    let camera = WorldCamera::new(
        Mat4::from_translation(Vec3::new(0.0, 0.0, 10.0)),
        Projection::Orthographic { half_height: 2.0 },
        0.1,
        100.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let view_proj = camera.view_proj;
    let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    let mut fill = |points: &[[f32; 2]], rule: FillRule, color: [f32; 4]| {
        for p in tessellate_fill(points, rule) {
            v.fills.push(FillVertex {
                position: [p[0], p[1], 0.0],
                color,
            });
        }
    };
    // A five-pointed star drawn in one stroke, so its outline crosses itself.
    let star = |cx: f32, cy: f32, r: f32| -> Vec<[f32; 2]> {
        (0..5)
            .map(|i| {
                let a = (90.0 + 144.0 * i as f32).to_radians();
                [cx + r * a.cos(), cy + r * a.sin()]
            })
            .collect()
    };
    let square = [[-2.6, -1.5], [-1.0, -1.5], [-1.0, 0.1], [-2.6, 0.1]];
    fill(&square, FillRule::EvenOdd, [1.0, 0.5, 0.1, 1.0]);
    fill(
        &star(0.0, -0.5, 1.0),
        FillRule::EvenOdd,
        [0.2, 0.4, 1.0, 1.0],
    );
    fill(
        &star(2.0, -0.5, 0.9),
        FillRule::NonZero,
        [0.2, 1.0, 0.3, 1.0],
    );
    let circle: Vec<[f32; 2]> = (0..48)
        .map(|i| {
            let a = (i as f32 * 7.5).to_radians();
            [-1.8 + 0.5 * a.cos(), 1.2 + 0.5 * a.sin()]
        })
        .collect();
    fill(&circle, FillRule::EvenOdd, [1.0, 0.0, 0.0, 0.5]);
    // The square's outline, over its fill.
    for k in 0..4 {
        let [a, b] = [square[k], square[(k + 1) % 4]];
        v.strokes.push(Stroke {
            a: [a[0], a[1], 0.0],
            b: [b[0], b[1], 0.0],
            width: 0.08,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    }
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    let at = |x: f32, y: f32| {
        let c = view_proj.project_point3(Vec3::new(x, y, 0.0));
        let px = ((c.x + 1.0) / 2.0 * size.0 as f32) as usize;
        let py = ((1.0 - c.y) / 2.0 * size.1 as f32) as usize;
        let i = (py * size.0 as usize + px) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let inside = at(-1.8, -0.7);
    assert!(
        inside[0] > 200 && inside[1] > 150 && inside[2] < 120,
        "the square's orange fill (linear 1, 0.5, 0.1): {inside:?}"
    );
    let edge = at(-1.0, -0.7);
    assert!(
        edge.iter().all(|&c| c > 200),
        "the outline over the fill: {edge:?}"
    );
    let hole = at(0.0, -0.5);
    assert!(
        hole.iter().all(|&c| c < 30),
        "even-odd leaves the star's middle empty: {hole:?}"
    );
    let arm = at(0.0, 0.25);
    assert!(arm[2] > 150, "a point of the even-odd star: {arm:?}");
    let solid = at(2.0, -0.5);
    assert!(
        solid[1] > 150,
        "non-zero fills the star's middle: {solid:?}"
    );
    let glass = at(-1.8, 1.2);
    assert!(
        glass[0] > 120 && glass[0] < 230 && glass[1] < 30,
        "the see-through circle: {glass:?}"
    );
    assert_golden("M5-RND-25", size.0, size.1, &pixels, 3);
}

/// A linear colour channel as the sRGB byte the target stores.
fn srgb_byte(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let s = if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

fn close_to(a: [u8; 3], b: [u8; 3], tolerance: i32) -> bool {
    a.iter()
        .zip(b)
        .all(|(&x, y)| (x as i32 - y as i32).abs() <= tolerance)
}

// M7-FOG-01
#[test]
fn fog_modes_fade_things_towards_the_fog_colour_with_distance() {
    use platypus_render::{
        Fog, FogMode, MeshInstance, Projection, WorldView, primitive_id, viewport_size,
    };
    let gpu = shared_gpu();
    let size = (320, 240);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    let fog_color = [0.6, 0.7, 0.8];
    // Red cubes at these distances, spread across the view, each the same size on screen.
    let distances = [3.0f32, 8.0, 15.0, 30.0, 60.0];
    let centres: Vec<Vec3> = distances
        .iter()
        .enumerate()
        .map(|(i, &d)| Vec3::new(d * (-0.5 + 0.25 * i as f32), 0.0, -d))
        .collect();
    let fogs = [
        None,
        Some(Fog {
            mode: FogMode::Linear,
            color: fog_color,
            start: 5.0,
            end: 40.0,
            // The cube past the end shows as a fog-coloured silhouette (here, against a
            // background of another colour), so nothing is culled.
            cull: false,
            ..Default::default()
        }),
        Some(Fog {
            mode: FogMode::Exponential,
            color: fog_color,
            density: 0.05,
            ..Default::default()
        }),
        Some(Fog {
            mode: FogMode::ExponentialSquared,
            color: fog_color,
            density: 0.03,
            ..Default::default()
        }),
    ];
    // Top left: clear air; top right: linear; bottom: exponential and squared.
    let rects = [
        [0.0, 0.0, 0.5, 0.5],
        [0.5, 0.0, 0.5, 0.5],
        [0.0, 0.5, 0.5, 0.5],
        [0.5, 0.5, 0.5, 0.5],
    ];
    let views: Vec<WorldView> = rects
        .iter()
        .zip(fogs)
        .map(|(&rect, fog)| {
            let camera = WorldCamera::new(
                Mat4::IDENTITY,
                Projection::Perspective {
                    fov_y_degrees: 60.0,
                },
                0.1,
                200.0,
                viewport_size(rect, size),
                Color::rgb(0.05, 0.05, 0.08),
            );
            let mut v = WorldView::new(camera, rect);
            v.meshes = centres
                .iter()
                .map(|&c| MeshInstance {
                    mesh: primitive_id("cube").unwrap(),
                    model: Mat4::from_translation(c) * Mat4::from_scale(Vec3::splat(-c.z * 0.12)),
                    color: [1.0, 0.1, 0.1, 1.0],
                    unlit: true,
                    ..Default::default()
                })
                .collect();
            v.fog = fog;
            v
        })
        .collect();
    let view_projs: Vec<Mat4> = views.iter().map(|v| v.camera.view_proj).collect();
    world.render_views(&gpu, &rt, &views, &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    // A cube's middle in view `k`.
    let at = |k: usize, i: usize| {
        let c = view_projs[k].project_point3(centres[i]);
        let r = rects[k];
        let x = (r[0] + (c.x + 1.0) / 2.0 * r[2]) * size.0 as f32;
        let y = (r[1] + (1.0 - c.y) / 2.0 * r[3]) * size.1 as f32;
        let p = (y as usize * size.0 as usize + x as usize) * 4;
        [pixels[p], pixels[p + 1], pixels[p + 2]]
    };
    let red = [255, srgb_byte(0.1), srgb_byte(0.1)];
    let fog = fog_color.map(srgb_byte);
    // Clear air: the farthest cube keeps its colour.
    assert!(close_to(at(0, 4), red, 2), "no fog: {:?}", at(0, 4));
    // Linear: nothing before the start, all fog past the end, partway between.
    assert!(
        close_to(at(1, 0), red, 2),
        "before the start: {:?}",
        at(1, 0)
    );
    assert!(close_to(at(1, 4), fog, 2), "past the end: {:?}", at(1, 4));
    let mid = at(1, 3);
    assert!(
        !close_to(mid, red, 20) && !close_to(mid, fog, 20),
        "between: {mid:?}"
    );
    // Exponential: some fog at once; squared: clearer nearby; both thick far away.
    let (exp_near, sq_near) = (at(2, 0), at(3, 0));
    assert!(
        exp_near[1] > sq_near[1] + 10,
        "exponential fogs nearby more than squared: {exp_near:?} {sq_near:?}"
    );
    assert!(
        close_to(at(2, 4), fog, 25),
        "exponential far: {:?}",
        at(2, 4)
    );
    assert!(close_to(at(3, 4), fog, 25), "squared far: {:?}", at(3, 4));
    // The background isn't fogged.
    let p = (size.0 as usize * 2 - 2) * 4;
    let corner = [pixels[p], pixels[p + 1], pixels[p + 2]];
    let clear = [srgb_byte(0.05), srgb_byte(0.05), srgb_byte(0.08)];
    assert!(close_to(corner, clear, 2), "background: {corner:?}");
    assert_golden("M7-FOG-01", size.0, size.1, &pixels, 3);
}

// M7-FOG-04
#[test]
fn height_fog_lies_low_and_covers_the_horizon() {
    use platypus_render::{
        Fog, FogMode, HeightFog, MeshInstance, Projection, WorldView, primitive_id,
    };
    let gpu = shared_gpu();
    let size = (320, 240);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    let fog_color = [0.6, 0.7, 0.8];
    // Level, 4 m up, looking down -z.
    let camera = WorldCamera::new(
        Mat4::from_translation(Vec3::new(0.0, 4.0, 0.0)),
        Projection::Perspective {
            fov_y_degrees: 60.0,
        },
        0.1,
        200.0,
        size,
        Color::rgb(0.05, 0.1, 0.3),
    );
    let view_proj = camera.view_proj;
    let mut v = WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    let cube = |model: Mat4, color: [f32; 4]| MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        model,
        color,
        unlit: true,
        ..Default::default()
    };
    // A grey floor and white pillars 10 m tall.
    v.meshes.push(cube(
        Mat4::from_translation(Vec3::new(0.0, -0.05, -100.0))
            * Mat4::from_scale(Vec3::new(400.0, 0.1, 200.0)),
        [0.25, 0.25, 0.25, 1.0],
    ));
    for (x, z) in [(-4.0, -15.0), (3.0, -30.0), (-9.0, -60.0)] {
        v.meshes.push(cube(
            Mat4::from_translation(Vec3::new(x, 5.0, z))
                * Mat4::from_scale(Vec3::new(1.5, 10.0, 1.5)),
            [1.0, 1.0, 1.0, 1.0],
        ));
    }
    // Height fog alone: exponential distance fog with no density.
    v.fog = Some(Fog {
        mode: FogMode::Exponential,
        color: fog_color,
        density: 0.0,
        height: Some(HeightFog {
            density: 0.15,
            base_height: 0.0,
            falloff: 0.5,
        }),
        ..Default::default()
    });
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    let pixel = |x: usize, y: usize| {
        let i = (y * size.0 as usize + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let at = |p: Vec3| {
        let c = view_proj.project_point3(p);
        pixel(
            ((c.x + 1.0) / 2.0 * size.0 as f32) as usize,
            ((1.0 - c.y) / 2.0 * size.1 as f32) as usize,
        )
    };
    let fog = fog_color.map(srgb_byte);
    let foggy = |c: [u8; 3]| {
        (0..3)
            .map(|i| (c[i] as i32 - fog[i] as i32).abs())
            .sum::<i32>()
    };
    // The near pillar: its foot in the fog, its top above it.
    let (foot, top) = (
        at(Vec3::new(-4.0, 0.5, -14.2)),
        at(Vec3::new(-4.0, 9.0, -14.2)),
    );
    assert!(foggy(top) > foggy(foot) + 30, "foot {foot:?}, top {top:?}");
    // The background: thick fog just above the horizon, clear high up.
    let horizon = pixel(size.0 as usize - 4, size.1 as usize / 2 - 3);
    assert!(foggy(horizon) < 40, "the horizon: {horizon:?}");
    // Looking 30° up through the fog's thin top: about 8% fog.
    let high = pixel(size.0 as usize - 4, 2);
    assert!(
        foggy(high) > 150 && high[2] > high[0] + 50,
        "high up: {high:?}"
    );
    // The floor far away is fog; up close it's still grey-ish.
    let far_floor = at(Vec3::new(10.0, 0.0, -150.0));
    assert!(foggy(far_floor) < 30, "the far floor: {far_floor:?}");
    let near_floor = at(Vec3::new(0.0, 0.0, -8.0));
    assert!(foggy(near_floor) > 60, "the near floor: {near_floor:?}");
    assert_golden("M7-FOG-04", size.0, size.1, &pixels, 3);
}

// M7-FOG-05
#[test]
fn sunlight_through_a_slatted_roof_makes_shafts_in_volumetric_fog() {
    use platypus_render::{
        Fog, FogMode, Light, LightKind, MeshInstance, Shadows, VolumetricFog, WorldView,
        primitive_id, viewport_size,
    };
    let gpu = shared_gpu();
    let size = (320, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let cube = |model: Mat4, color: [f32; 4]| MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        model,
        color,
        cast_shadows: true,
        ..Default::default()
    };
    let box_at = |centre: Vec3, extent: Vec3| {
        Mat4::from_scale_rotation_translation(extent, Quat::IDENTITY, centre)
    };
    let mut meshes = vec![
        // The floor and a back wall the view rays end on.
        cube(
            box_at(Vec3::new(0.0, -0.1, 0.0), Vec3::new(30.0, 0.2, 20.0)),
            [0.5, 0.5, 0.5, 1.0],
        ),
        cube(
            box_at(Vec3::new(0.0, 4.0, -6.5), Vec3::new(30.0, 8.0, 1.0)),
            [0.5, 0.5, 0.5, 1.0],
        ),
    ];
    // The roof: slats 1.2 m wide with 0.8 m gaps, running front to back.
    for k in -6..=6 {
        meshes.push(cube(
            box_at(
                Vec3::new(k as f32 * 2.0, 6.0, 0.0),
                Vec3::new(1.2, 0.2, 14.0),
            ),
            [0.3, 0.3, 0.3, 1.0],
        ));
    }
    let sun = Light {
        kind: LightKind::Directional,
        color: [3.0, 2.85, 2.6],
        position: Vec3::ZERO,
        direction: Vec3::new(0.35, -1.0, 0.0).normalize(),
        range: 0.0,
        shadows: Some(Shadows {
            soft: false,
            strength: 1.0,
        }),
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    };
    let rects = [[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 0.5, 1.0]];
    let fog = Fog {
        mode: FogMode::Exponential,
        color: [0.2, 0.21, 0.24],
        density: 0.06,
        ..Default::default()
    };
    let views: Vec<WorldView> = rects
        .iter()
        .enumerate()
        .map(|(k, &rect)| {
            let camera = WorldCamera::perspective(
                platypus_core::glam::camera::rh::view::look_at_mat4(
                    Vec3::new(0.0, 1.5, 8.0),
                    Vec3::new(0.0, 3.0, 0.0),
                    Vec3::Y,
                )
                .inverse(),
                60.0,
                0.1,
                60.0,
                viewport_size(rect, size),
                Color::rgb(0.0, 0.0, 0.0),
            );
            let mut v = WorldView::new(camera, rect);
            v.ambient = [0.03, 0.03, 0.035];
            v.meshes = meshes.clone();
            v.lights = vec![sun];
            v.fog = Some(Fog {
                volumetric: (k == 1).then(|| VolumetricFog {
                    intensity: 0.25,
                    distance: 30.0,
                    ..Default::default()
                }),
                ..fog
            });
            v
        })
        .collect();
    let view_projs: Vec<Mat4> = views.iter().map(|v| v.camera.view_proj).collect();
    let mut world = WorldRenderer::new(&gpu);
    world.shadow_distance = 30.0;
    world.render_views(&gpu, &rt, &views, &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    // Green along a row of the back wall, 3 m up, seen through the air under the roof.
    let row = |k: usize| -> Vec<i32> {
        (-30..=30)
            .map(|i| {
                let c = view_projs[k].project_point3(Vec3::new(i as f32 * 0.1, 3.0, -6.0));
                let r = rects[k];
                let x = (r[0] + (c.x + 1.0) / 2.0 * r[2]) * size.0 as f32;
                let y = (1.0 - c.y) / 2.0 * size.1 as f32;
                pixels[(y as usize * size.0 as usize + x as usize) * 4 + 1] as i32
            })
            .collect()
    };
    let spread = |r: &[i32]| r.iter().max().unwrap() - r.iter().min().unwrap();
    let mean = |r: &[i32]| r.iter().sum::<i32>() / r.len() as i32;
    let (plain, lit) = (row(0), row(1));
    // Without shafts the wall (the sun only grazes it) is evenly dim; with them the air
    // in front glows, in bands where the sun comes through the gaps.
    assert!(spread(&plain) < 12, "plain fog is even: {plain:?}");
    assert!(
        mean(&lit) > mean(&plain) + 20,
        "lit fog is brighter: {lit:?}"
    );
    assert!(
        spread(&lit) > 25,
        "shafts and shadows between them: {lit:?}"
    );
    assert_golden("M7-FOG-05", size.0, size.1, &pixels, 6);
}

// M7-FOG-10
#[test]
fn god_rays_keep_a_grates_thin_shadows_sharp() {
    use platypus_render::{
        Fog, FogMode, Light, LightKind, MeshInstance, Shadows, VolumetricFog, WorldView,
        primitive_id, viewport_size,
    };
    let gpu = shared_gpu();
    let size = (320, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let cube = |model: Mat4, color: [f32; 4]| MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        model,
        color,
        cast_shadows: true,
        ..Default::default()
    };
    let box_at = |centre: Vec3, extent: Vec3| {
        Mat4::from_scale_rotation_translation(extent, Quat::IDENTITY, centre)
    };
    let mut meshes = vec![
        // The floor and a back wall the view rays end on.
        cube(
            box_at(Vec3::new(0.0, -0.1, 0.0), Vec3::new(30.0, 0.2, 20.0)),
            [0.5, 0.5, 0.5, 1.0],
        ),
        cube(
            box_at(Vec3::new(0.0, 4.0, -6.5), Vec3::new(30.0, 8.0, 1.0)),
            [0.5, 0.5, 0.5, 1.0],
        ),
    ];
    // A grate for a roof: bars 0.12 m wide every 0.4 m, finer than the volume's cells.
    for k in -40..=40 {
        meshes.push(cube(
            box_at(
                Vec3::new(k as f32 * 0.4, 6.0, 0.0),
                Vec3::new(0.12, 0.2, 14.0),
            ),
            [0.3, 0.3, 0.3, 1.0],
        ));
    }
    let sun = Light {
        kind: LightKind::Directional,
        color: [3.0, 2.85, 2.6],
        position: Vec3::ZERO,
        direction: Vec3::new(0.35, -1.0, 0.0).normalize(),
        range: 0.0,
        shadows: Some(Shadows {
            soft: false,
            strength: 1.0,
        }),
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    };
    let rects = [[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 0.5, 1.0]];
    let fog = Fog {
        mode: FogMode::Exponential,
        color: [0.2, 0.21, 0.24],
        density: 0.06,
        ..Default::default()
    };
    let views: Vec<WorldView> = rects
        .iter()
        .enumerate()
        .map(|(k, &rect)| {
            let camera = WorldCamera::perspective(
                platypus_core::glam::camera::rh::view::look_at_mat4(
                    Vec3::new(0.0, 1.5, 8.0),
                    Vec3::new(0.0, 3.0, 0.0),
                    Vec3::Y,
                )
                .inverse(),
                60.0,
                0.1,
                60.0,
                viewport_size(rect, size),
                Color::rgb(0.0, 0.0, 0.0),
            );
            let mut v = WorldView::new(camera, rect);
            v.ambient = [0.03, 0.03, 0.035];
            v.meshes = meshes.clone();
            v.lights = vec![sun];
            v.fog = Some(Fog {
                volumetric: Some(VolumetricFog {
                    intensity: 0.25,
                    distance: 30.0,
                    sharp_distance: if k == 1 { 20.0 } else { 0.0 },
                    ..Default::default()
                }),
                ..fog
            });
            v
        })
        .collect();
    let view_projs: Vec<Mat4> = views.iter().map(|v| v.camera.view_proj).collect();
    let mut world = WorldRenderer::new(&gpu);
    world.shadow_distance = 30.0;
    world.render_views(&gpu, &rt, &views, &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    // Green along a row of the back wall, 3 m up, seen through the air under the roof.
    let row = |k: usize| -> Vec<i32> {
        (-30..=30)
            .map(|i| {
                let c = view_projs[k].project_point3(Vec3::new(i as f32 * 0.1, 3.0, -6.0));
                let r = rects[k];
                let x = (r[0] + (c.x + 1.0) / 2.0 * r[2]) * size.0 as f32;
                let y = (1.0 - c.y) / 2.0 * size.1 as f32;
                pixels[(y as usize * size.0 as usize + x as usize) * 4 + 1] as i32
            })
            .collect()
    };
    // How much the row ripples (the sum of steps between neighbours): thin beams show as
    // ripples the volume smooths more (both soften the edges a little).
    let ripple = |r: &[i32]| r.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<i32>();
    let (soft, sharp) = (row(0), row(1));
    assert!(
        ripple(&sharp) > ripple(&soft) * 11 / 10,
        "thin beams: {} against {} ({sharp:?} / {soft:?})",
        ripple(&sharp),
        ripple(&soft)
    );
    assert_golden("M7-FOG-10", size.0, size.1, &pixels, 6);
}

/// A grate of thin bars for a roof, a floor and a back wall in exponential fog, lit by a
/// sun tilted `tilt` degrees from straight down (towards +x), seen from under the roof
/// through a low light volume: `WorldView`s for `size`, and their cameras.
fn grate_view(size: (u32, u32), tilt: f32) -> platypus_render::WorldView {
    use platypus_render::{
        Fog, FogMode, Light, LightKind, MeshInstance, Shadows, VolumetricFog, WorldView,
        primitive_id,
    };
    let cube = |model: Mat4, color: [f32; 4]| MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        model,
        color,
        cast_shadows: true,
        ..Default::default()
    };
    let box_at = |centre: Vec3, extent: Vec3| {
        Mat4::from_scale_rotation_translation(extent, Quat::IDENTITY, centre)
    };
    let mut meshes = vec![
        cube(
            box_at(Vec3::new(0.0, -0.1, 0.0), Vec3::new(30.0, 0.2, 20.0)),
            [0.5, 0.5, 0.5, 1.0],
        ),
        cube(
            box_at(Vec3::new(0.0, 4.0, -6.5), Vec3::new(30.0, 8.0, 1.0)),
            [0.5, 0.5, 0.5, 1.0],
        ),
    ];
    for k in -40..=40 {
        meshes.push(cube(
            box_at(
                Vec3::new(k as f32 * 0.4, 6.0, 0.0),
                Vec3::new(0.12, 0.2, 14.0),
            ),
            [0.3, 0.3, 0.3, 1.0],
        ));
    }
    let t = tilt.to_radians();
    let sun = Light {
        kind: LightKind::Directional,
        color: [3.0, 2.85, 2.6],
        position: Vec3::ZERO,
        direction: Vec3::new(t.sin(), -t.cos(), 0.0),
        range: 0.0,
        shadows: Some(Shadows {
            soft: false,
            strength: 1.0,
        }),
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    };
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(
            Vec3::new(0.0, 1.5, 8.0),
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::Y,
        )
        .inverse(),
        60.0,
        0.1,
        60.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let mut v = WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.ambient = [0.03, 0.03, 0.035];
    v.meshes = meshes;
    v.lights = vec![sun];
    v.fog = Some(Fog {
        mode: FogMode::Exponential,
        color: [0.2, 0.21, 0.24],
        density: 0.06,
        volumetric: Some(VolumetricFog {
            intensity: 0.25,
            distance: 30.0,
            detail: 0,
            ..Default::default()
        }),
        ..Default::default()
    });
    v
}

/// Green along a row of `grate_view`'s back wall, 3 m up, seen through the air under the roof.
fn grate_row(view: &platypus_render::WorldView, size: (u32, u32), pixels: &[u8]) -> Vec<i32> {
    (-30..=30)
        .map(|i| {
            let c = view
                .camera
                .view_proj
                .project_point3(Vec3::new(i as f32 * 0.1, 3.0, -6.0));
            let x = (c.x + 1.0) / 2.0 * size.0 as f32;
            let y = (1.0 - c.y) / 2.0 * size.1 as f32;
            pixels[(y as usize * size.0 as usize + x as usize) * 4 + 1] as i32
        })
        .collect()
}

/// The same grate rendered by a new renderer for `frames` frames, the sun at `tilt`.
fn grate_render(size: (u32, u32), tilts: &[f32]) -> Vec<u8> {
    grate_render_with(size, tilts, platypus_render::Denoising::Both)
}

/// As `grate_render`, with the light volume's denoising.
fn grate_render_with(
    size: (u32, u32),
    tilts: &[f32],
    denoising: platypus_render::Denoising,
) -> Vec<u8> {
    let gpu = shared_gpu();
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    world.shadow_distance = 30.0;
    for &tilt in tilts {
        let mut v = grate_view(size, tilt);
        if let Some(f) = v.fog.as_mut()
            && let Some(vol) = f.volumetric.as_mut()
        {
            vol.denoising = denoising;
        }
        world.render_views(&gpu, &rt, &[v], &[]);
    }
    target.read_pixels(&gpu).unwrap()
}

// M7-FOG-11
#[test]
fn the_light_volume_averages_its_cells_over_frames_into_smooth_beams() {
    let size = (320, 160);
    let view = grate_view(size, 20.0);
    // How much the row ripples (the sum of steps between neighbours). Without denoising each
    // cell takes one sample at its middle every frame, so bars' shadows make all-or-nothing
    // cells, stair steps; with it each cell is shaded by how much of it is lit.
    let ripple = |r: &[i32]| r.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<i32>();
    let frames = [20.0; 32];
    let one = grate_row(
        &view,
        size,
        &grate_render_with(size, &frames, platypus_render::Denoising::None),
    );
    let many = grate_row(&view, size, &grate_render(size, &frames));
    assert!(
        ripple(&many) < ripple(&one),
        "averaged {} against one sample a cell {} ({many:?} / {one:?})",
        ripple(&many),
        ripple(&one)
    );
}

// M7-FOG-12
#[test]
fn the_light_volume_follows_a_turning_sun_without_trailing_its_old_beams() {
    let size = (320, 160);
    // A day passing quickly: the sun turns 0.25 degrees a frame, 15 degrees in all.
    let turning: Vec<f32> = (0..=60).map(|i| 5.0 + i as f32 * 0.25).collect();
    let moving = grate_render(size, &turning);
    // The same sun held still at the end: where the beams should be.
    let still = grate_render(size, &[20.0; 32]);
    let diff = |a: &[u8], b: &[u8]| {
        a.iter()
            .zip(b)
            .map(|(x, y)| (*x as i32 - *y as i32).abs())
            .sum::<i32>() as f32
            / a.len() as f32
    };
    let lag = diff(&moving, &still);
    // Against the sun 2.5 degrees (10 frames) back, to scale it.
    let behind = diff(&grate_render(size, &[17.5; 32]), &still);
    assert!(
        lag < behind * 0.5,
        "moving sun differs by {lag} a channel, a sun 2.5 degrees back by {behind}"
    );
}

// M7-EXP-01
#[test]
fn auto_exposure_adapts_dim_and_bright_views_to_the_same_grey_and_takes_time_to() {
    use platypus_render::{AutoExposure, Grading, WorldView};
    let gpu = shared_gpu();
    let size = (64, 48);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // An empty view of one flat brightness.
    let lit = |level: f64| {
        let camera = WorldCamera::perspective(
            Mat4::IDENTITY,
            60.0,
            0.1,
            10.0,
            size,
            Color::rgb(level, level, level),
        );
        WorldView::new(camera, [0.0, 0.0, 1.0, 1.0])
    };
    let mut world = WorldRenderer::new(&gpu);
    world.grading = Grading {
        auto_exposure: Some(AutoExposure::default()),
        delta_time: 1.0 / 60.0,
        ..Default::default()
    };
    let mut middle = |level: f64, frames: usize| {
        for _ in 0..frames {
            world.render_views(&gpu, &rt, &[lit(level)], &[]);
        }
        let pixels = target.read_pixels(&gpu).unwrap();
        pixels[((size.1 / 2 * size.0 + size.0 / 2) * 4) as usize] as i32
    };
    // Settled, a dim view and one twenty times brighter look alike.
    let dim = middle(0.05, 300);
    let first = middle(1.0, 1);
    let later = middle(1.0, 20);
    let bright = middle(1.0, 300);
    assert!(
        (dim - bright).abs() <= 4,
        "settled: dim {dim}, bright {bright}"
    );
    // Stepping into the light dazzles, then eases off.
    assert!(
        first > bright + 60,
        "dazzled {first} against settled {bright}"
    );
    assert!(
        later < first && later > bright,
        "{first} then {later} then {bright}"
    );
}

/// Renders `views` (flat backgrounds, or a bright patch) for `frames` frames with `world`'s
/// grading, and reads back the centre pixel of each view (green, 0..255).
fn exposure_frames(
    world: &mut WorldRenderer,
    size: (u32, u32),
    views: &[platypus_render::WorldView],
    frames: usize,
) -> Vec<i32> {
    let gpu = shared_gpu();
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    for _ in 0..frames {
        world.render_views(&gpu, &rt, views, &[]);
    }
    let pixels = target.read_pixels(&gpu).unwrap();
    views
        .iter()
        .map(|v| {
            let x = ((v.viewport[0] + v.viewport[2] * 0.5) * size.0 as f32) as usize;
            let y = ((v.viewport[1] + v.viewport[3] * 0.25) * size.1 as f32) as usize;
            pixels[(y * size.0 as usize + x) * 4 + 1] as i32
        })
        .collect()
}

/// An empty view of one flat brightness over `viewport`, for camera `key`.
fn flat_view(level: f64, viewport: [f32; 4], key: u32) -> platypus_render::WorldView {
    let size = (64, 48);
    let camera = WorldCamera::perspective(
        Mat4::IDENTITY,
        60.0,
        0.1,
        10.0,
        size,
        Color::rgb(level, level, level),
    );
    let mut v = platypus_render::WorldView::new(camera, viewport);
    v.exposure_key = key;
    v
}

/// A dim view with an emissive patch in its lower half (`patch`: its brightness).
fn patch_view(dim: f64, patch: f32) -> platypus_render::WorldView {
    use platypus_render::{MeshInstance, primitive_id};
    let mut v = flat_view(dim, [0.0, 0.0, 1.0, 1.0], 0);
    v.meshes.push(MeshInstance {
        mesh: primitive_id("quad").unwrap(),
        model: Mat4::from_scale_rotation_translation(
            Vec3::new(4.0, 1.2, 1.0),
            Quat::IDENTITY,
            Vec3::new(0.0, -1.8, -4.0),
        ),
        color: [0.0, 0.0, 0.0, 1.0],
        emission: [patch, patch, patch],
        unlit: true,
        ..Default::default()
    });
    v
}

fn auto_grading(auto: platypus_render::AutoExposure) -> platypus_render::Grading {
    platypus_render::Grading {
        auto_exposure: Some(auto),
        delta_time: 1.0 / 60.0,
        ..Default::default()
    }
}

// M7-EXP-02
#[test]
fn each_camera_adapts_on_its_own_even_on_one_screen() {
    use platypus_render::AutoExposure;
    let size = (128, 48);
    let mut world = WorldRenderer::new(&shared_gpu());
    world.grading = auto_grading(AutoExposure::default());
    // Split screen: a dim view on the left, one twenty times brighter on the right.
    let views = [
        flat_view(0.05, [0.0, 0.0, 0.5, 1.0], 1),
        flat_view(1.0, [0.5, 0.0, 0.5, 1.0], 2),
    ];
    let settled = exposure_frames(&mut world, size, &views, 300);
    // Each settles to the same grey (with one shared state, one would be far off).
    assert!(
        (settled[0] - settled[1]).abs() <= 4,
        "left {} right {}",
        settled[0],
        settled[1]
    );
}

// M7-EXP-03
#[test]
fn histogram_metering_ignores_a_small_bright_patch_between_its_percentiles() {
    use platypus_render::AutoExposure;
    let size = (64, 48);
    let render = |low: f32, high: f32| {
        let mut world = WorldRenderer::new(&shared_gpu());
        world.grading = auto_grading(AutoExposure {
            metering: platypus_render::Metering::Average,
            low_percent: low,
            high_percent: high,
            ..Default::default()
        });
        exposure_frames(&mut world, size, &[patch_view(0.05, 400.0)], 300)[0]
    };
    // Averaging everything, the blazing patch darkens the rest; between 10% and 80% it's
    // left out, and the dim background shows as it would alone.
    let all = render(0.0, 100.0);
    let middle = render(10.0, 80.0);
    let alone = {
        let mut world = WorldRenderer::new(&shared_gpu());
        world.grading = auto_grading(AutoExposure {
            metering: platypus_render::Metering::Average,
            ..Default::default()
        });
        exposure_frames(
            &mut world,
            size,
            &[flat_view(0.05, [0.0, 0.0, 1.0, 1.0], 0)],
            300,
        )[0]
    };
    assert!(
        middle > all + 20,
        "between percentiles {middle}, everything {all}"
    );
    assert!(
        (middle - alone).abs() <= 6,
        "between percentiles {middle}, without the patch {alone}"
    );
}

// M7-EXP-04
#[test]
fn auto_exposure_blends_into_the_manual_exposure_over_its_transition() {
    use platypus_render::AutoExposure;
    let size = (64, 48);
    let views = [flat_view(0.01, [0.0, 0.0, 1.0, 1.0], 0)];
    let mut world = WorldRenderer::new(&shared_gpu());
    // The manual exposure leaves light as it is (EV100 -log2(1.2)): the dim view stays dim.
    let fixed = -(1.2f32.log2());
    world.grading = platypus_render::Grading {
        fixed_ev: Some(fixed),
        transition: 1.0,
        ..auto_grading(AutoExposure::default())
    };
    let adapted = exposure_frames(&mut world, size, &views, 300)[0];
    world.grading.auto_exposure = None;
    let halfway = exposure_frames(&mut world, size, &views, 30)[0];
    let after = exposure_frames(&mut world, size, &views, 60)[0];
    let manual = {
        let mut w = WorldRenderer::new(&shared_gpu());
        w.grading = platypus_render::Grading {
            fixed_ev: Some(fixed),
            ..Default::default()
        };
        exposure_frames(&mut w, size, &views, 2)[0]
    };
    assert!(adapted > manual + 40, "adapted {adapted}, manual {manual}");
    assert!(
        halfway < adapted - 10 && halfway > manual + 10,
        "halfway {halfway} between {adapted} and {manual}"
    );
    assert!(
        (after - manual).abs() <= 2,
        "after the transition {after}, manual {manual}"
    );
}

// M7-EXP-05
#[test]
fn scripts_read_each_cameras_exposure_back_and_can_reset_it() {
    use platypus_render::AutoExposure;
    let size = (64, 48);
    let mut world = WorldRenderer::new(&shared_gpu());
    world.grading = auto_grading(AutoExposure::default());
    let dim = [flat_view(0.05, [0.0, 0.0, 1.0, 1.0], 7)];
    exposure_frames(&mut world, size, &dim, 120);
    let r = world.exposure(7).expect("read back");
    // A flat 0.05 metres as EV100 log2(0.05 × 8) and is shown at that exposure.
    let expected = (0.05f32 * 8.0).log2();
    assert!(
        (r.scene_ev - expected).abs() < 0.2,
        "metered {} expected {expected}",
        r.scene_ev
    );
    assert!(
        (r.ev - expected).abs() < 0.2,
        "exposure {} expected {expected}",
        r.ev
    );
    assert!(world.exposure(8).is_none());
    // A cut to a bright view: with a reset it shows settled at once, without one it dazzles.
    let bright = [flat_view(1.0, [0.0, 0.0, 1.0, 1.0], 7)];
    let mut other = WorldRenderer::new(&shared_gpu());
    other.grading = world.grading;
    exposure_frames(&mut other, size, &dim, 120);
    let dazzled = exposure_frames(&mut other, size, &bright, 1)[0];
    world.reset_exposure(7);
    let reset = exposure_frames(&mut world, size, &bright, 1)[0];
    let settled = exposure_frames(&mut world, size, &bright, 300)[0];
    assert!(
        dazzled > settled + 60,
        "dazzled {dazzled}, settled {settled}"
    );
    assert!(
        (reset - settled).abs() <= 4,
        "reset {reset}, settled {settled}"
    );
}

// M7-EXP-06
#[test]
fn a_soft_floor_and_a_curve_keep_dark_places_darker_than_full_adaptation() {
    use platypus_render::AutoExposure;
    let size = (64, 48);
    let views = [flat_view(0.005, [0.0, 0.0, 1.0, 1.0], 0)];
    let render = |auto: AutoExposure| {
        let mut world = WorldRenderer::new(&shared_gpu());
        world.grading = auto_grading(auto);
        exposure_frames(&mut world, size, &views, 300)[0]
    };
    let full = render(AutoExposure::default());
    let soft = render(AutoExposure {
        dark_adaptation: 0.5,
        dark_below: 3.0,
        ..Default::default()
    });
    let mut curve = [[0.0; 2]; platypus_render::MAX_CURVE_POINTS];
    curve[0] = [-4.0, -2.0];
    curve[1] = [4.0, 0.0];
    let curved = render(AutoExposure {
        curve,
        curve_points: 2,
        ..Default::default()
    });
    assert!(soft < full - 20, "soft floor {soft}, full {full}");
    assert!(curved < full - 20, "curve {curved}, full {full}");
}

// M7-RP-02
#[test]
fn a_quality_levels_pipeline_asset_leaves_out_shafts_and_caps_lights() {
    use platypus_render::{Fog, FogMode, Light, LightKind, PipelineAsset, VolumetricFog};
    let size = (64, 48);
    // Forty point lights in view: a level capped at 32 draws the nearest 32 (no directional:
    // no main light).
    let mut lit = flat_view(0.01, [0.0, 0.0, 1.0, 1.0], 0);
    lit.lights = (0..40)
        .map(|i| Light {
            kind: LightKind::Point,
            color: [1.0, 1.0, 1.0],
            position: Vec3::new(
                (i % 8) as f32 - 3.5,
                (i / 8) as f32 - 2.0,
                -6.0 - i as f32 * 0.1,
            ),
            direction: Vec3::NEG_Z,
            range: 1.0,
            shadows: None,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        })
        .collect();
    // Light shafts asked for: a level without them renders as if they weren't.
    lit.fog = Some(Fog {
        mode: FogMode::Exponential,
        color: [0.0, 0.0, 0.0],
        density: 0.05,
        volumetric: Some(VolumetricFog::default()),
        ..Default::default()
    });
    let mobile = PipelineAsset {
        volumetric_fog: false,
        max_additional_lights: 32,
        ..Default::default()
    };
    let render = |asset: PipelineAsset, view: &platypus_render::WorldView| {
        let mut world = WorldRenderer::new(&shared_gpu());
        world.pipeline = asset;
        let pixel = exposure_frames(&mut world, size, std::slice::from_ref(view), 1)[0];
        (world.stats().lights, pixel)
    };
    assert_eq!(render(PipelineAsset::default(), &lit).0, 40);
    assert_eq!(render(mobile, &lit).0, 32);
    let mut no_shafts = lit.clone();
    no_shafts.fog = no_shafts.fog.map(|f| Fog {
        volumetric: None,
        ..f
    });
    assert_eq!(render(mobile, &lit).1, render(mobile, &no_shafts).1);
}

// M7-RP-03
#[test]
fn a_pipeline_assets_hdr_and_additional_light_shadows_settings_apply() {
    use platypus_render::{Light, LightKind, PipelineAsset, Shadows, Tonemapping};
    let size = (64, 48);
    // A bright view, past white: tonemapped it rolls off; without HDR it clips.
    let bright = [flat_view(3.0, [0.0, 0.0, 1.0, 1.0], 0)];
    let shade = |hdr: bool| {
        let mut world = WorldRenderer::new(&shared_gpu());
        world.pipeline = PipelineAsset {
            hdr,
            ..Default::default()
        };
        world.grading.tonemapping = Tonemapping::Aces;
        exposure_frames(&mut world, size, &bright, 1)[0]
    };
    assert!(shade(true) < 250, "tonemapped {}", shade(true));
    assert_eq!(shade(false), 255);
    // A spot light with shadows: a level without additional light shadows gives it none.
    let mut lit = flat_view(0.0, [0.0, 0.0, 1.0, 1.0], 0);
    lit.lights = vec![Light {
        kind: LightKind::Spot {
            inner_degrees: 20.0,
            outer_degrees: 30.0,
        },
        color: [1.0, 1.0, 1.0],
        position: Vec3::new(0.0, 0.0, -2.0),
        direction: Vec3::NEG_Z,
        range: 8.0,
        shadows: Some(Shadows {
            soft: false,
            strength: 1.0,
        }),
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    let maps = |asset: PipelineAsset| {
        let mut world = WorldRenderer::new(&shared_gpu());
        world.pipeline = asset;
        exposure_frames(&mut world, size, std::slice::from_ref(&lit), 1);
        world.stats().shadow_maps
    };
    assert_eq!(
        maps(PipelineAsset {
            additional_light_shadows: false,
            ..Default::default()
        }),
        0
    );
    assert!(maps(PipelineAsset::default()) > 0);
}

// M7-HD-01
#[test]
fn real_world_units_pre_expose_light_too_bright_for_the_hdr_target() {
    use platypus_render::AutoExposure;
    let size = (64, 48);
    // Sunlit concrete in real units is thousands of nits; past fp16's 65504 the HDR target
    // overflows (or clamps). A view at 200000 nits: pre-exposed (real-world units on), it
    // settles to the same
    // grey as a view at 2 nits does, metered across the histogram's range.
    let settle = |level: f64| {
        let mut world = WorldRenderer::new(&shared_gpu());
        world.physical_units = true;
        world.grading = auto_grading(AutoExposure {
            max_ev: 28.0,
            ..Default::default()
        });
        exposure_frames(
            &mut world,
            size,
            &[flat_view(level, [0.0, 0.0, 1.0, 1.0], 0)],
            300,
        )[0]
    };
    let dim = settle(2.0);
    let blazing = settle(200_000.0);
    assert!(
        (blazing - dim).abs() <= 4,
        "200000 nits {blazing}, 2 nits {dim}"
    );
}

// M7-FOG-15
#[test]
fn a_spot_light_behind_a_wall_puts_no_haze_in_front_of_it() {
    use platypus_render::{
        Fog, FogMode, Light, LightKind, MeshInstance, Shadows, VolumetricFog, WorldView,
        primitive_id,
    };
    let size = (160, 120);
    let gpu = shared_gpu();
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let tv = target.view().unwrap();
    let rt = RenderTarget {
        view: &tv,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // A solid wall 2 m ahead; behind it a green spot light shines straight at its back. In
    // front (the camera's side) there's no light, so no haze: the cells at the wall's depth
    // reach a little behind it, into the beam, and reading them showed its round
    // cross-section on the wall.
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(
            Vec3::new(0.0, 1.5, 2.0),
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::Y,
        )
        .inverse(),
        60.0,
        0.1,
        40.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let mut v = WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.ambient = [0.0, 0.0, 0.0];
    v.meshes = vec![MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        model: Mat4::from_scale_rotation_translation(
            Vec3::new(8.0, 6.0, 0.05),
            Quat::IDENTITY,
            Vec3::new(0.0, 1.5, 0.0),
        ),
        color: [0.5, 0.5, 0.5, 1.0],
        cast_shadows: true,
        ..Default::default()
    }];
    v.lights = vec![Light {
        kind: LightKind::Spot {
            inner_degrees: 8.0,
            outer_degrees: 12.0,
        },
        color: [0.5, 6.0, 0.8],
        position: Vec3::new(0.0, 1.5, -4.0),
        direction: Vec3::Z,
        range: 10.0,
        shadows: Some(Shadows {
            soft: false,
            strength: 1.0,
        }),
        culling_mask: ALL_LAYERS,
        volumetric: 1.0,
    }];
    v.fog = Some(Fog {
        mode: FogMode::Exponential,
        color: [0.0, 0.0, 0.0],
        density: 0.1,
        volumetric: Some(VolumetricFog {
            intensity: 2.0,
            distance: 14.0,
            ..Default::default()
        }),
        ..Default::default()
    });
    let mut world = WorldRenderer::new(&gpu);
    for _ in 0..16 {
        world.render_views(&gpu, &rt, std::slice::from_ref(&v), &[]);
    }
    let pixels = target.read_pixels(&gpu).unwrap();
    let green = |x: u32, y: u32| pixels[((y * size.0 + x) * 4 + 1) as usize] as i32;
    // The wall's middle (where the beam meets its back) against well outside the beam.
    let middle = green(size.0 / 2, size.1 / 2);
    let aside = green(size.0 / 8, size.1 / 2);
    assert!(middle <= aside + 2, "middle {middle}, aside {aside}");
}

// M7-CLOUD-01
#[test]
fn a_cloud_layer_covers_the_sky_behind_the_distance_fog() {
    use platypus_render::{CloudLayer, Fog, FogMode, WorldView};
    let size = (64, 48);
    let gpu = shared_gpu();
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let tv = target.view().unwrap();
    let rt = RenderTarget {
        view: &tv,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // Looking up at a blue background through dense red fog (which the background ignores).
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(
            Vec3::ZERO,
            Vec3::new(0.0, 1.0, -0.3),
            Vec3::Z,
        )
        .inverse(),
        60.0,
        0.1,
        100.0,
        size,
        Color::rgb(0.05, 0.1, 0.6),
    );
    let render = |clouds: Option<CloudLayer>| {
        let mut v = WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
        v.fog = Some(Fog {
            mode: FogMode::Exponential,
            color: [1.0, 0.0, 0.0],
            density: 0.5,
            ..Default::default()
        });
        v.clouds = clouds;
        let mut world = WorldRenderer::new(&gpu);
        world.render_views(&gpu, &rt, std::slice::from_ref(&v), &[]);
        let px = target.read_pixels(&gpu).unwrap();
        let i = ((size.1 / 2 * size.0 + size.0 / 2) * 4) as usize;
        [px[i] as i32, px[i + 1] as i32, px[i + 2] as i32]
    };
    let clear = render(None);
    // Overcast: every bit of the deck is cloud, light grey.
    let cloudy = render(Some(CloudLayer {
        altitude: 50.0,
        size: 30.0,
        coverage: 1.0,
        softness: 0.1,
        color: [0.7, 0.7, 0.7],
        ..Default::default()
    }));
    assert!(clear[2] > clear[0] + 60, "blue sky {clear:?}");
    assert!(
        (cloudy[0] - cloudy[2]).abs() < 30 && cloudy[0] > 120,
        "grey cloud {cloudy:?}"
    );
}

// M7-SPOT-01
#[test]
fn a_pyramid_spot_light_lights_a_wide_thin_rectangle_turned_with_its_object() {
    use platypus_render::{Light, LightKind, MeshInstance, WorldView, primitive_id};
    let size = (160, 120);
    let gpu = shared_gpu();
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let tv = target.view().unwrap();
    let rt = RenderTarget {
        view: &tv,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let camera = WorldCamera::perspective(
        platypus_core::glam::camera::rh::view::look_at_mat4(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::ZERO,
            Vec3::Y,
        )
        .inverse(),
        60.0,
        0.1,
        40.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    // Lit points on a wall 4 m from the light, which is 10 degrees high and 4 times as wide:
    // about 0.35 m above and below its middle, 1.4 m to either side.
    let lit_at = |right: Vec3| {
        let mut v = WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
        v.ambient = [0.0, 0.0, 0.0];
        v.meshes = vec![MeshInstance {
            mesh: primitive_id("cube").unwrap(),
            model: Mat4::from_scale_rotation_translation(
                Vec3::new(10.0, 10.0, 0.1),
                Quat::IDENTITY,
                Vec3::new(0.0, 0.0, -0.05),
            ),
            color: [0.8, 0.8, 0.8, 1.0],
            ..Default::default()
        }];
        v.lights = vec![Light {
            kind: LightKind::Pyramid {
                inner_degrees: 8.0,
                outer_degrees: 10.0,
                aspect: 4.0,
                right,
            },
            color: [20.0, 20.0, 20.0],
            position: Vec3::new(0.0, 0.0, 4.0),
            direction: Vec3::NEG_Z,
            range: 10.0,
            shadows: None,
            culling_mask: ALL_LAYERS,
            volumetric: 0.0,
        }];
        let mut world = WorldRenderer::new(&gpu);
        world.render_views(&gpu, &rt, std::slice::from_ref(&v), &[]);
        let pixels = target.read_pixels(&gpu).unwrap();
        move |x: f32, y: f32| {
            let c = camera.view_proj.project_point3(Vec3::new(x, y, 0.0));
            let px = ((c.x + 1.0) / 2.0 * size.0 as f32) as u32;
            let py = ((1.0 - c.y) / 2.0 * size.1 as f32) as u32;
            pixels[((py * size.0 + px) * 4) as usize] > 40
        }
    };
    let flat = lit_at(Vec3::X);
    assert!(flat(0.0, 0.0) && flat(1.0, 0.0) && flat(-1.0, 0.0));
    assert!(!flat(0.0, 0.7) && !flat(0.0, -0.7) && !flat(1.8, 0.0));
    // Turned a quarter about its direction: tall and thin.
    let tall = lit_at(Vec3::Y);
    assert!(tall(0.0, 1.0) && tall(0.0, -1.0));
    assert!(!tall(0.7, 0.0) && !tall(-0.7, 0.0));
}

// M7-FOG-13
/// How much the haze changes from one frame to the next with the camera still under a
/// grate's beams (all of it noise): the mean change in green over the frame (0..255).
fn grate_shimmer(volumetric: platypus_render::VolumetricFog) -> f32 {
    let gpu = shared_gpu();
    let size = (320, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    world.shadow_distance = 30.0;
    let frame = |i: usize| {
        let mut v = grate_view(size, 20.0);
        let turn = Mat4::IDENTITY;
        let _ = i;
        v.camera = WorldCamera::perspective(
            (platypus_core::glam::camera::rh::view::look_at_mat4(
                Vec3::new(0.0, 1.5, 8.0),
                Vec3::new(0.0, 3.0, 0.0),
                Vec3::Y,
            )
            .inverse())
                * turn,
            60.0,
            0.1,
            60.0,
            size,
            Color::rgb(0.0, 0.0, 0.0),
        );
        if let Some(f) = v.fog.as_mut() {
            f.volumetric = Some(volumetric);
        }
        v
    };
    let mut last: Vec<u8> = Vec::new();
    let mut total = 0.0;
    let mut count = 0;
    for i in 0..90 {
        world.render_views(&gpu, &rt, &[frame(i)], &[]);
        if i >= 60 {
            let pixels = target.read_pixels(&gpu).unwrap();
            if !last.is_empty() {
                let d: i64 = pixels
                    .chunks(4)
                    .zip(last.chunks(4))
                    .map(|(a, b)| (a[1] as i64 - b[1] as i64).abs())
                    .sum();
                total += d as f32 / (size.0 * size.1) as f32;
                count += 1;
            }
            last = pixels;
        }
    }
    total / count as f32
}

#[test]
fn more_history_and_samples_take_the_shimmer_out_of_fine_beams() {
    use platypus_render::{Denoising, VolumetricFog};
    let base = VolumetricFog {
        intensity: 0.25,
        distance: 30.0,
        detail: 2,
        ..Default::default()
    };
    let none = grate_shimmer(VolumetricFog {
        denoising: Denoising::None,
        ..base
    });
    let both = grate_shimmer(base);
    let smooth = grate_shimmer(VolumetricFog {
        history_weight: 0.96,
        samples: 4,
        ..base
    });
    eprintln!("shimmer: none {none:.3}, both {both:.3}, history 0.96 x4 samples {smooth:.3}");
    // Without denoising nothing moves (but beams step); with it, the default shimmers a
    // little, and more history and samples take most of that out.
    assert!(none < 0.005, "no jitter, no shimmer: {none}");
    assert!(
        smooth * 3.0 < both,
        "more history and samples: {smooth} against {both}"
    );
}

/// The grate's beams after `frames` frames, the camera turning `turn` degrees a frame towards
/// the same final view whatever `turn` is.
fn grate_after(volumetric: platypus_render::VolumetricFog, turn: f32, frames: usize) -> Vec<u8> {
    let gpu = shared_gpu();
    let size = (320, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    world.shadow_distance = 30.0;
    for i in 0..frames {
        let mut v = grate_view(size, 20.0);
        // Turning towards the final view, which every run ends on.
        let left = (frames - 1 - i) as f32 * turn;
        v.camera = WorldCamera::perspective(
            platypus_core::glam::camera::rh::view::look_at_mat4(
                Vec3::new(0.0, 1.5, 8.0),
                Vec3::new(0.0, 3.0, 0.0),
                Vec3::Y,
            )
            .inverse()
                * Mat4::from_rotation_y(left.to_radians()),
            60.0,
            0.1,
            60.0,
            size,
            Color::rgb(0.0, 0.0, 0.0),
        );
        if let Some(f) = v.fog.as_mut() {
            f.volumetric = Some(volumetric);
        }
        world.render_views(&gpu, &rt, &[v], &[]);
    }
    target.read_pixels(&gpu).unwrap()
}

/// How far a turning camera's beams are from a still camera's (settled) at the same view:
/// blur and smear, the mean change in green (0..255).
fn grate_smear(volumetric: platypus_render::VolumetricFog) -> f32 {
    let still = grate_after(volumetric, 0.0, 120);
    let moving = grate_after(volumetric, 0.15, 120);
    let d: i64 = moving
        .chunks(4)
        .zip(still.chunks(4))
        .map(|(a, b)| (a[1] as i64 - b[1] as i64).abs())
        .sum();
    d as f32 / (still.len() / 4) as f32
}

// M7-FOG-14
#[test]
fn turning_cameras_beams_stay_close_to_a_still_cameras() {
    use platypus_render::VolumetricFog;
    let base = VolumetricFog {
        intensity: 0.25,
        distance: 30.0,
        detail: 2,
        ..Default::default()
    };
    let default = grate_smear(base);
    let heavy = grate_smear(VolumetricFog {
        history_weight: 0.96,
        samples: 4,
        ..base
    });
    let still_samples = grate_smear(VolumetricFog {
        motion_samples: 1,
        ..base
    });
    eprintln!(
        "smear: default {default:.3} (1 sample moving: {still_samples:.3}), history 0.96 x4 samples {heavy:.3}"
    );
    // A long history smeared beams while turning (0.38 against 0.27 before cells trusted it
    // less the further it moved); now it's no worse than the default.
    assert!(
        heavy < default + 0.02 && default < still_samples,
        "smear: default {default} (1 sample moving {still_samples}), history 0.96 x4 samples {heavy}"
    );
}

// M7-EXP-07
#[test]
fn a_change_of_manual_exposure_eases_over_the_transition() {
    let size = (64, 48);
    let views = [flat_view(0.05, [0.0, 0.0, 1.0, 1.0], 0)];
    let mut world = WorldRenderer::new(&shared_gpu());
    world.grading = platypus_render::Grading {
        fixed_ev: Some(0.0),
        transition: 1.0,
        delta_time: 1.0 / 60.0,
        ..Default::default()
    };
    let before = exposure_frames(&mut world, size, &views, 5)[0];
    // Two stops darker: half a second in it's partway, after the second it's there.
    world.grading.fixed_ev = Some(2.0);
    let halfway = exposure_frames(&mut world, size, &views, 30)[0];
    let after = exposure_frames(&mut world, size, &views, 40)[0];
    let target = {
        let mut w = WorldRenderer::new(&shared_gpu());
        w.grading = platypus_render::Grading {
            fixed_ev: Some(2.0),
            ..Default::default()
        };
        exposure_frames(&mut w, size, &views, 2)[0]
    };
    assert!(before > target + 20, "before {before}, target {target}");
    assert!(
        halfway < before - 5 && halfway > target + 5,
        "halfway {halfway} between {before} and {target}"
    );
    assert!(
        (after - target).abs() <= 1,
        "after {after}, target {target}"
    );
}

// M7-FOG-06
#[test]
fn fog_volumes_hold_mist_in_a_box_with_soft_edges() {
    use platypus_render::{FogVolume, MeshInstance, Projection, WorldView, primitive_id};
    let gpu = shared_gpu();
    let size = (320, 200);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let camera = WorldCamera::new(
        Mat4::from_rotation_translation(Quat::from_rotation_x(-0.15), Vec3::new(0.0, 2.0, 10.0)),
        Projection::Perspective {
            fov_y_degrees: 50.0,
        },
        0.1,
        100.0,
        size,
        Color::rgb(0.02, 0.03, 0.06),
    );
    let view_proj = camera.view_proj;
    let mut v = WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    let cube = |model: Mat4, color: [f32; 4]| MeshInstance {
        mesh: primitive_id("cube").unwrap(),
        model,
        color,
        unlit: true,
        ..Default::default()
    };
    // Dark ground, a blue pond, and red posts: one behind the mist, one beside it.
    v.meshes = vec![
        cube(
            Mat4::from_scale_rotation_translation(
                Vec3::new(40.0, 0.2, 40.0),
                Quat::IDENTITY,
                Vec3::new(0.0, -0.1, 0.0),
            ),
            [0.05, 0.08, 0.04, 1.0],
        ),
        cube(
            Mat4::from_scale_rotation_translation(
                Vec3::new(5.0, 0.02, 3.0),
                Quat::IDENTITY,
                Vec3::new(-1.0, 0.01, 0.0),
            ),
            [0.05, 0.1, 0.3, 1.0],
        ),
        cube(
            Mat4::from_scale_rotation_translation(
                Vec3::new(0.3, 2.0, 0.3),
                Quat::IDENTITY,
                Vec3::new(-1.0, 1.0, -3.0),
            ),
            [1.0, 0.1, 0.1, 1.0],
        ),
        cube(
            Mat4::from_scale_rotation_translation(
                Vec3::new(0.3, 2.0, 0.3),
                Quat::IDENTITY,
                Vec3::new(4.5, 1.0, -3.0),
            ),
            [1.0, 0.1, 0.1, 1.0],
        ),
    ];
    // Mist over the pond: thick in the middle, fading over its last half metre.
    v.fog_volumes = vec![FogVolume {
        centre: Vec3::new(-1.0, 0.6, 0.0),
        half_extents: Vec3::new(3.0, 0.6, 2.0),
        density: 1.5,
        color: [0.7, 0.75, 0.8],
        blend: 0.5,
        ellipsoid: false,
    }];
    let mut world = WorldRenderer::new(&gpu);
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    let at = |p: Vec3| {
        let c = view_proj.project_point3(p);
        let x = ((c.x + 1.0) / 2.0 * size.0 as f32) as usize;
        let y = ((1.0 - c.y) / 2.0 * size.1 as f32) as usize;
        let i = (y * size.0 as usize + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let mist = [0.7f32, 0.75, 0.8].map(srgb_byte);
    let red = [255, srgb_byte(0.1), srgb_byte(0.1)];
    // The post behind the mist hides low down and shows above it; the other is clear.
    let hidden = at(Vec3::new(-1.0, 0.3, -2.85));
    assert!(close_to(hidden, mist, 40), "behind the mist: {hidden:?}");
    let above = at(Vec3::new(-1.0, 1.8, -2.85));
    assert!(close_to(above, red, 6), "above the mist: {above:?}");
    let beside = at(Vec3::new(4.5, 0.3, -2.85));
    assert!(close_to(beside, red, 6), "beside the mist: {beside:?}");
    // Its top edge is soft: partway between the post and the mist.
    let edge = at(Vec3::new(-1.0, 0.9, -2.85));
    assert!(
        !close_to(edge, red, 20) && !close_to(edge, mist, 20),
        "the soft top: {edge:?}"
    );
    assert_golden("M7-FOG-06", size.0, size.1, &pixels, 3);
}

// M7-FOG-07
#[test]
fn fog_culls_what_it_hides_and_counts_it() {
    use platypus_render::{
        Fog, FogMode, MeshInstance, Projection, WorldStats, WorldView, primitive_id,
    };
    let gpu = shared_gpu();
    let size = (64, 64);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let fog = [0.5, 0.6, 0.7];
    let camera = WorldCamera::new(
        Mat4::IDENTITY,
        Projection::Perspective {
            fov_y_degrees: 60.0,
        },
        0.1,
        500.0,
        size,
        Color::rgb(0.5, 0.6, 0.7),
    );
    let mut v = WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    // Cubes straight ahead at 10, 30, 50 and 200 m, and quads at 10 and 200 m.
    v.meshes = [10.0f32, 30.0, 50.0, 200.0]
        .iter()
        .map(|&d| MeshInstance {
            mesh: primitive_id("cube").unwrap(),
            model: Mat4::from_translation(Vec3::new(0.0, 0.0, -d)),
            color: [1.0, 0.0, 0.0, 1.0],
            unlit: true,
            ..Default::default()
        })
        .collect();
    v.quads = [10.0f32, 200.0]
        .iter()
        .map(|&d| {
            WorldQuad::colored(
                Mat4::from_translation(Vec3::new(0.0, 0.0, -d)),
                [0.0, 1.0, 0.0, 0.5],
            )
        })
        .collect();
    let stats = |fog: Option<Fog>| {
        let mut world = WorldRenderer::new(&gpu);
        let mut v = v.clone();
        v.fog = fog;
        world.render_views(&gpu, &rt, &[v], &[]);
        world.stats()
    };
    // Linear fog ending at 40 m hides the cubes at 50 and 200 m and the far quad.
    let linear = Fog {
        mode: FogMode::Linear,
        color: fog,
        start: 5.0,
        end: 40.0,
        ..Default::default()
    };
    let s = stats(Some(linear));
    assert_eq!(
        (
            s.meshes,
            s.culled_meshes,
            s.quads,
            s.culled_quads,
            s.fog_culled
        ),
        (2, 2, 1, 1, 3),
        "{s:?}"
    );
    // Exponential squared at 0.02 per metre is opaque at about 117 m.
    let squared = Fog {
        mode: FogMode::ExponentialSquared,
        color: fog,
        density: 0.02,
        ..Default::default()
    };
    let d = squared.cull_distance().unwrap();
    assert!((d - 117.7).abs() < 0.5, "{d}");
    assert_eq!(stats(Some(squared)).fog_culled, 2);
    // Culling off, no fog, or no density: nothing is hidden.
    let s = stats(Some(Fog {
        cull: false,
        ..linear
    }));
    assert_eq!((s.meshes, s.fog_culled), (4, 0));
    assert_eq!(
        stats(None),
        WorldStats {
            meshes: 4,
            quads: 2,
            ..Default::default()
        }
    );
    let thin = Fog {
        mode: FogMode::Exponential,
        density: 0.0,
        ..Default::default()
    };
    assert_eq!(thin.cull_distance(), None);
    assert_eq!(stats(Some(thin)).fog_culled, 0);
}

// M7-FOG-08
#[test]
fn spot_and_point_lights_light_the_fog_with_sharp_cones_and_round_glows() {
    use platypus_render::{
        Fog, FogMode, Light, LightKind, Projection, VolumetricFog, WorldView, viewport_size,
    };
    let gpu = shared_gpu();
    let size = (320, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // Dark fog, nothing but lights: a spot shining down on the left, a point glowing on the
    // right.
    let camera = WorldCamera::new(
        Mat4::from_translation(Vec3::new(0.0, 1.5, 8.0)),
        Projection::Perspective {
            fov_y_degrees: 50.0,
        },
        0.1,
        60.0,
        viewport_size([0.0, 0.0, 1.0, 1.0], size),
        Color::rgb(0.01, 0.01, 0.02),
    );
    let view_proj = camera.view_proj;
    let mut v = WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    v.ambient = [0.0; 3];
    // A black wall behind, for the view rays to end on.
    v.meshes = vec![platypus_render::MeshInstance {
        mesh: platypus_render::primitive_id("cube").unwrap(),
        model: Mat4::from_scale_rotation_translation(
            Vec3::new(40.0, 20.0, 0.2),
            Quat::IDENTITY,
            Vec3::new(0.0, 0.0, -5.0),
        ),
        color: [0.0, 0.0, 0.0, 1.0],
        unlit: true,
        ..Default::default()
    }];
    v.lights = vec![
        Light {
            kind: LightKind::Spot {
                inner_degrees: 36.0,
                outer_degrees: 40.0,
            },
            color: [30.0, 30.0, 30.0],
            position: Vec3::new(-2.5, 3.0, 0.0),
            direction: Vec3::NEG_Y,
            range: 6.0,
            shadows: None,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        },
        Light {
            kind: LightKind::Point,
            color: [4.0, 4.0, 4.0],
            position: Vec3::new(2.5, 2.0, 0.0),
            direction: Vec3::NEG_Y,
            range: 4.0,
            shadows: None,
            culling_mask: ALL_LAYERS,
            volumetric: 1.0,
        },
    ];
    v.fog = Some(Fog {
        mode: FogMode::Exponential,
        color: [0.01, 0.01, 0.02],
        density: 0.08,
        volumetric: Some(VolumetricFog {
            intensity: 1.0,
            anisotropy: 0.0,
            ..Default::default()
        }),
        ..Default::default()
    });
    let mut world = WorldRenderer::new(&gpu);
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    let green = |p: Vec3| {
        let c = view_proj.project_point3(p);
        let x = ((c.x + 1.0) / 2.0 * size.0 as f32) as usize;
        let y = ((1.0 - c.y) / 2.0 * size.1 as f32) as usize;
        pixels[(y * size.0 as usize + x) * 4 + 1] as i32
    };
    // The cone, 2 m below the spot, is about 1.46 m across (a 20° half angle): lit fog
    // inside, dark just outside, with a sharp edge.
    let inside = green(Vec3::new(-2.5 + 0.4, 1.0, 0.0));
    let outside = green(Vec3::new(-2.5 + 1.1, 1.0, 0.0));
    assert!(inside > outside + 60, "inside {inside}, outside {outside}");
    let edge = |x: f32| green(Vec3::new(-2.5 + x, 1.0, 0.0));
    assert!(
        edge(0.62) > edge(0.82) + 40,
        "a sharp edge: {} then {}",
        edge(0.62),
        edge(0.82)
    );
    // Above the spot there's no light; the point glows all round, brightest at its middle.
    assert!(green(Vec3::new(-2.5, 3.6, 0.0)) < 30, "above the spot");
    let (centre, near, far) = (
        green(Vec3::new(2.5, 2.0, 0.0)),
        green(Vec3::new(2.5, 3.0, 0.0)),
        green(Vec3::new(2.5, 4.5, 0.0)),
    );
    assert!(
        centre > near && near > far,
        "the glow falls off: {centre} {near} {far}"
    );
    assert_golden("M7-FOG-08", size.0, size.1, &pixels, 4);
}

// M7-FOG-02
#[test]
fn orthographic_fog_goes_by_depth_and_covers_quads_shapes_and_strokes() {
    use platypus_render::{FillVertex, Fog, FogMode, Projection, Stroke, WorldView};
    let gpu = shared_gpu();
    let size = (240, 160);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    let mut world = WorldRenderer::new(&gpu);
    let camera = WorldCamera::new(
        Mat4::from_translation(Vec3::new(0.0, 0.0, 10.0)),
        Projection::Orthographic { half_height: 2.0 },
        0.1,
        100.0,
        size,
        Color::rgb(0.0, 0.0, 0.0),
    );
    let view_proj = camera.view_proj;
    let mut v = WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
    let white = [1.0, 1.0, 1.0, 1.0];
    // Two white quads at the same depth, one in the middle and one near the edge, and a
    // third farther back.
    for (x, z) in [(-2.2, 0.0), (0.0, 0.0), (2.2, -6.0)] {
        v.quads.push(WorldQuad::colored(
            Mat4::from_translation(Vec3::new(x, 1.0, z)) * Mat4::from_scale(Vec3::splat(0.8)),
            white,
        ));
    }
    // A filled triangle and a stroke, both 10 m away.
    for p in [[-2.6, -1.6], [-1.4, -1.6], [-2.0, -0.6]] {
        v.fills.push(FillVertex {
            position: [p[0], p[1], 0.0],
            color: white,
        });
    }
    v.strokes.push(Stroke {
        a: [0.0, -1.0, 0.0],
        b: [2.5, -1.0, 0.0],
        width: 0.3,
        color: white,
    });
    v.fog = Some(Fog {
        mode: FogMode::Linear,
        color: [0.0, 0.0, 1.0],
        start: 0.0,
        end: 20.0,
        ..Default::default()
    });
    world.render_views(&gpu, &rt, &[v], &[]);
    let pixels = target.read_pixels(&gpu).unwrap();
    let at = |x: f32, y: f32, z: f32| {
        let c = view_proj.project_point3(Vec3::new(x, y, z));
        let px = ((c.x + 1.0) / 2.0 * size.0 as f32) as usize;
        let py = ((1.0 - c.y) / 2.0 * size.1 as f32) as usize;
        let i = (py * size.0 as usize + px) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // Halfway into the fog: white half mixed with blue.
    let half = [srgb_byte(0.5), srgb_byte(0.5), 255];
    let (edge, middle) = (at(-2.2, 1.0, 0.0), at(0.0, 1.0, 0.0));
    assert!(close_to(middle, half, 3), "the middle quad: {middle:?}");
    assert_eq!(edge, middle, "the same depth, the same fog");
    // 16 m deep: 80% fog.
    let back = at(2.2, 1.0, -6.0);
    let deeper = [srgb_byte(0.2), srgb_byte(0.2), 255];
    assert!(close_to(back, deeper, 3), "the deeper quad: {back:?}");
    let fill = at(-2.0, -1.3, 0.0);
    assert!(close_to(fill, half, 3), "the fill: {fill:?}");
    let stroke = at(1.2, -1.0, 0.0);
    assert!(close_to(stroke, half, 3), "the stroke: {stroke:?}");
}

// RDY-RND-03
#[test]
fn flat_sprites_at_one_depth_are_not_cut_by_a_tilt() {
    let gpu = shared_gpu();
    let size = (160, 120);
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: target.format(),
        size,
        scale: 1.0,
    };
    // A big green sprite, and over it (order 1) a red one at the same depth that is spinning:
    // its turn about z carries a rounding error about x and y, so parts of it are a hair behind.
    let red_pixels = |flat: bool, spin: f32, error: f32| {
        let camera = WorldCamera::perspective(
            platypus_core::glam::camera::rh::view::look_at_mat4(
                Vec3::new(0.0, 0.0, 6.0),
                Vec3::ZERO,
                Vec3::Y,
            )
            .inverse(),
            40.0,
            0.1,
            50.0,
            size,
            Color::rgb(0.0, 0.0, 0.0),
        );
        let mut v = platypus_render::WorldView::new(camera, [0.0, 0.0, 1.0, 1.0]);
        v.ambient = [1.0; 3];
        let green = WorldQuad {
            flat,
            order: 0,
            ..WorldQuad::colored(
                place(Vec3::ZERO, Quat::IDENTITY, (4.0, 3.0)),
                [0.0, 0.8, 0.0, 1.0],
            )
        };
        let rotation = Quat::from_euler(platypus_core::glam::EulerRot::XYZ, error, error, spin);
        let red = WorldQuad {
            flat,
            order: 1,
            ..WorldQuad::colored(
                place(Vec3::ZERO, rotation, (1.0, 1.0)),
                [0.9, 0.0, 0.0, 1.0],
            )
        };
        v.quads = vec![green, red];
        let mut world = WorldRenderer::new(&gpu);
        world.render_views(&gpu, &rt, &[v], &[]);
        let pixels = target.read_pixels(&gpu).unwrap();
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 150 && p[1] < 60)
            .count()
    };
    let flat = red_pixels(true, 0.6, 1e-4);
    let depth_written = red_pixels(false, 0.6, 1e-4);
    // The whole red square shows (about 756 pixels), however it is turned...
    assert!(flat > 740, "a flat sprite lost pixels: {flat}");
    assert_eq!(flat, red_pixels(true, 0.6, 0.0));
    // ...where a quad that writes depth loses a good part of it to the other's depth.
    assert!(depth_written < flat - 100, "{depth_written} vs {flat}");
}
