// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! World panels: React UI drawn on planes in the world (`WorldPanel` components, s&box's
//! name). Each panel has its own UI tree (React commits with its target id), its own
//! interaction state (hover, focus), and a texture it's drawn into each frame, which the
//! world pass puts on a quad at the panel's GameObject.
//!
//! A panel stands in its object's XY plane, facing +z, `width`×`height` UI pixels at
//! `pixels_per_meter`, with its top-left corner at (-w/2, +h/2).

use std::collections::{BTreeMap, HashMap};

use platypus_core::glam::{Mat4, Vec3, Vec4Swizzles};
use platypus_core::{Commit, NodeId, Value};
use platypus_render::{Gpu, RenderTarget, UiRenderer, WorldQuad, wgpu};
use platypus_scene::{ScriptObject, ScriptScene};
use web_ui_dom::opcodes::{APPEND_CHILD, CREATE, CREATE_TEXT, SET_STYLE, SET_TEXT};
use web_ui_dom::{Interaction, UiTree};

/// The texture format panels are drawn into (sampled as sRGB by the world pass).
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub struct Panel {
    pub tree: UiTree,
    pub interaction: Interaction,
    /// The GameObject it's on; `None` until its `WorldPanel` starts (commits can come
    /// first).
    pub object: Option<ScriptObject>,
    /// Size in UI pixels.
    pub size: (f32, f32),
    pub pixels_per_meter: f32,
    /// Texture pixels per UI pixel (sharper text up close).
    pub resolution: f32,
    /// Linear RGBA multiplied over the panel.
    pub tint: [f32; 4],
    /// Whether the pointer can use it.
    pub interactive: bool,
    /// Stylesheets added to this tree, by the app's sheet key.
    pub sheets: HashMap<u32, u32>,
    texture: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    /// A `TextMesh`'s text and look as last laid out (a colour change alone keeps it).
    text_key: Option<String>,
    /// Its texture shows that layout (TextMeshes only redraw when it changes).
    drawn: bool,
}

impl Panel {
    fn new((tree, sheets): (UiTree, HashMap<u32, u32>)) -> Self {
        Self {
            tree,
            interaction: Interaction::new(),
            object: None,
            size: (800.0, 600.0),
            pixels_per_meter: 500.0,
            resolution: 2.0,
            tint: [1.0; 4],
            interactive: true,
            sheets,
            texture: None,
            text_key: None,
            drawn: false,
        }
    }

    /// Size in metres.
    fn meters(&self) -> (f32, f32) {
        let ppm = self.pixels_per_meter.max(1e-3);
        (self.size.0 / ppm, self.size.1 / ppm)
    }

    /// Where the unit quad goes: the object's transform, scaled to the panel's size.
    fn model(&self, scene: &ScriptScene) -> Option<Mat4> {
        let object = self.object?;
        if !scene.active(object) {
            return None;
        }
        let world = Mat4::from(scene.scene.world_matrix(scene.object(object)?));
        let (w, h) = self.meters();
        Some(world * Mat4::from_scale(Vec3::new(w, h, 1.0)))
    }

    fn texture_size(&self) -> (u32, u32) {
        let s = self.resolution.clamp(0.25, 4.0);
        (
            ((self.size.0 * s).round() as u32).clamp(1, 4096),
            ((self.size.1 * s).round() as u32).clamp(1, 4096),
        )
    }
}

/// A new panel's tree, with the stylesheets added to it (by the app's sheet key).
pub type NewTree = (UiTree, HashMap<u32, u32>);

#[derive(Default)]
pub struct Panels {
    pub map: BTreeMap<u32, Panel>,
}

/// A hit on a panel: which, where in its UI pixels, and how far along the ray.
pub struct PanelHit {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub distance: f32,
}

impl Panels {
    /// The panel for a target id, created (with `new_tree`) the first time.
    pub fn get_or_create(&mut self, id: u32, new_tree: impl FnOnce() -> NewTree) -> &mut Panel {
        self.map.entry(id).or_insert_with(|| Panel::new(new_tree()))
    }

    /// `render.panel component object [w h] pixelsPerMeter resolution [r g b a] interactive`
    pub fn apply(
        &mut self,
        args: &[Value],
        new_tree: impl FnOnce() -> NewTree,
    ) -> Result<(), String> {
        let id_at = |i: usize| -> Result<u32, String> {
            args.get(i)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(|v| v as u32)
                .ok_or_else(|| format!("argument {i} must be an id"))
        };
        let num = |i: usize| {
            args.get(i)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .map(|v| v as f32)
        };
        let floats = |i: usize| -> Option<Vec<f32>> {
            match args.get(i) {
                Some(Value::Array(items)) => {
                    items.iter().map(|v| v.as_f64().map(|n| n as f32)).collect()
                }
                _ => None,
            }
        };
        let id = id_at(0)?;
        let object = id_at(1)?;
        let size = floats(2)
            .filter(|s| s.len() == 2)
            .ok_or("panel needs [width, height]")?;
        let panel = self.get_or_create(id, new_tree);
        panel.object = Some(object);
        panel.size = (size[0].max(1.0), size[1].max(1.0));
        panel.pixels_per_meter = num(3).unwrap_or(500.0).max(1e-3);
        panel.resolution = num(4).unwrap_or(2.0);
        if let Some(t) = floats(5).filter(|t| t.len() == 4) {
            panel.tint = [t[0], t[1], t[2], t[3]];
        }
        panel.interactive = !matches!(args.get(6), Some(Value::Bool(false)));
        Ok(())
    }

    /// `render.text component object text [r g b a] fontSize fontFamily|null fontWeight
    /// align maxWidth pixelsPerUnit resolution`: a `TextMesh`. Its panel holds one text
    /// element, drawn white and tinted by the colour (linear, like a sprite's), and sizes
    /// itself to the text: one line per line break, wrapped at `maxWidth` UI pixels if set.
    pub fn apply_text(
        &mut self,
        args: &[Value],
        new_tree: impl FnOnce() -> NewTree,
    ) -> Result<(), String> {
        let id_at = |i: usize| -> Result<u32, String> {
            args.get(i)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(|v| v as u32)
                .ok_or_else(|| format!("argument {i} must be an id"))
        };
        let num = |i: usize| {
            args.get(i)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .map(|v| v as f32)
        };
        let id = id_at(0)?;
        let object = id_at(1)?;
        let text = args
            .get(2)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let tint = match args.get(3) {
            Some(Value::Array(c)) if c.len() == 4 => {
                let c: Vec<f32> = c
                    .iter()
                    .filter_map(|v| v.as_f64().map(|v| v as f32))
                    .collect();
                [c[0], c[1], c[2], c[3]]
            }
            _ => [1.0; 4],
        };
        let font_size = num(4).unwrap_or(32.0).max(1.0);
        let family = match args.get(5) {
            Some(Value::String(f)) => Value::String(f.clone()),
            _ => Value::Null,
        };
        let weight = num(6).unwrap_or(400.0);
        let align = args
            .get(7)
            .and_then(Value::as_str)
            .unwrap_or("center")
            .to_string();
        let max_width = num(8).unwrap_or(0.0).max(0.0);

        let fresh = !self.map.contains_key(&id);
        let panel = self.get_or_create(id, new_tree);
        panel.object = Some(object);
        panel.pixels_per_meter = num(9).unwrap_or(100.0).max(1e-3);
        panel.resolution = num(10).unwrap_or(2.0);
        panel.tint = tint;
        panel.interactive = false;
        // A new colour (or scale) alone keeps the laid-out text: nothing more to do.
        let key = format!(
            "{text}\u{1}{font_size}\u{1}{family:?}\u{1}{weight}\u{1}{align}\u{1}{max_width}"
        );
        if panel.text_key.as_deref() == Some(key.as_str()) {
            return Ok(());
        }
        panel.text_key = Some(key);
        panel.drawn = false;

        let n = |v: f32| Value::from(f64::from(v));
        let mut ops = Vec::new();
        if fresh {
            ops.push(op(CREATE, vec![n(1.0), "text".into()]));
            ops.push(op(APPEND_CHILD, vec![n(0.0), n(1.0)]));
            ops.push(op(CREATE_TEXT, vec![n(2.0), text.into()]));
            ops.push(op(APPEND_CHILD, vec![n(1.0), n(2.0)]));
        } else {
            ops.push(op(SET_TEXT, vec![n(2.0), text.into()]));
        }
        // Positioned, so it shrinks to its text instead of stretching across the panel.
        ops.push(op(
            SET_STYLE,
            vec![
                n(1.0),
                "position".into(),
                "absolute".into(),
                "left".into(),
                n(0.0),
                "top".into(),
                n(0.0),
                "color".into(),
                "#ffffff".into(),
                "fontSize".into(),
                n(font_size),
                "fontFamily".into(),
                family,
                "fontWeight".into(),
                n(weight),
                "textAlign".into(),
                align.into(),
                "whiteSpace".into(),
                if max_width > 0.0 { "pre-wrap" } else { "pre" }.into(),
                "maxWidth".into(),
                if max_width > 0.0 {
                    n(max_width)
                } else {
                    Value::Null
                },
            ],
        ));
        if let Some(e) = panel.tree.apply_commit(&Commit { ops }).first() {
            return Err(e.to_string());
        }
        // Lay out with room to spare, then fit the panel to the text.
        panel.tree.set_size(4096.0, 4096.0);
        panel.tree.layout_if_needed();
        let (w, h) = panel.tree.content_box(NodeId(1)).map_or((1.0, 1.0), |r| {
            (r.width.ceil().max(1.0), r.height.ceil().max(1.0))
        });
        panel.size = (w, h);
        panel.tree.set_size(w, h);
        Ok(())
    }

    pub fn remove(&mut self, id: u32) {
        self.map.remove(&id);
    }

    /// Lays out and draws every visible panel into its texture. Returns the quads to
    /// put in the world (texture `i` being the `i`th of the returned views).
    pub fn render(
        &mut self,
        gpu: &Gpu,
        renderer: &mut UiRenderer,
        scene: &ScriptScene,
        now_ms: f64,
        mut load_images: impl FnMut(&mut UiTree, &mut UiRenderer),
    ) -> (Vec<(ScriptObject, WorldQuad)>, Vec<wgpu::TextureView>) {
        let (mut quads, mut views) = (Vec::new(), Vec::new());
        for panel in self.map.values_mut() {
            let (Some(object), Some(model)) = (panel.object, panel.model(scene)) else {
                continue;
            };
            let size = panel.texture_size();
            // A TextMesh's texture stays good until its text is laid out again: a new colour
            // is only the quad's tint.
            let kept = panel.text_key.is_some()
                && panel.drawn
                && panel.texture.as_ref().is_some_and(|t| t.2 == size);
            if !kept {
                panel.tree.set_size(panel.size.0, panel.size.1);
                panel.tree.set_time(now_ms);
                load_images(&mut panel.tree, renderer);
            }
            let cmds = if kept {
                Vec::new()
            } else {
                panel.tree.draw_list()
            };
            if panel.texture.as_ref().is_none_or(|t| t.2 != size) {
                let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("world panel"),
                    size: wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let view = texture.create_view(&Default::default());
                panel.texture = Some((texture, view, size));
            }
            let (_, view, _) = panel.texture.as_ref().expect("just made");
            let transparent = platypus_render::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            };
            if !kept {
                renderer.render(
                    gpu,
                    &RenderTarget {
                        view,
                        format: FORMAT,
                        size,
                        scale: size.0 as f32 / panel.size.0,
                    },
                    transparent,
                    &cmds,
                );
                panel.drawn = true;
            }
            let quad = WorldQuad {
                model,
                color: panel.tint,
                texture: Some(platypus_render::WorldTexture::Frame(views.len())),
                uv: [0.0, 0.0, 1.0, 1.0],
                additive: false,
                flat: false,
                order: 0,
                layer: 0,
            };
            quads.push((object, quad));
            views.push(view.clone());
        }
        (quads, views)
    }

    /// The nearest interactive panel the ray meets, and where on it.
    pub fn hit(&self, scene: &ScriptScene, origin: Vec3, dir: Vec3) -> Option<PanelHit> {
        let mut best: Option<PanelHit> = None;
        for (&id, panel) in &self.map {
            if !panel.interactive {
                continue;
            }
            let Some(model) = panel.model(scene) else {
                continue;
            };
            let Some((u, v, distance)) = ray_quad(model, origin, dir) else {
                continue;
            };
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue;
            }
            if best.as_ref().is_none_or(|b| distance < b.distance) {
                best = Some(PanelHit {
                    id,
                    x: u * panel.size.0,
                    y: v * panel.size.1,
                    distance,
                });
            }
        }
        best
    }

    /// Where the ray meets a panel's plane in its UI pixels (outside its bounds too), for
    /// a drag that started on it.
    pub fn project(
        &self,
        scene: &ScriptScene,
        id: u32,
        origin: Vec3,
        dir: Vec3,
    ) -> Option<(f32, f32)> {
        let panel = self.map.get(&id)?;
        let (u, v, _) = ray_quad(panel.model(scene)?, origin, dir)?;
        Some((u * panel.size.0, v * panel.size.1))
    }
}

/// Where a ray meets the unit quad `model` places: (u, v) from its top-left (0 to 1
/// inside it) and the distance along the ray. `None` if it's parallel or behind.
fn ray_quad(model: Mat4, origin: Vec3, dir: Vec3) -> Option<(f32, f32, f32)> {
    let inv = model.inverse();
    let o = inv.transform_point3(origin);
    let d = inv.transform_vector3(dir);
    if d.z.abs() < 1e-9 {
        return None;
    }
    let t = -o.z / d.z;
    if t < 0.0 {
        return None;
    }
    let p = o + d * t;
    let world = model.transform_point3(p);
    Some((
        p.x + 0.5,
        0.5 - p.y,
        world.distance(origin) * if t >= 0.0 { 1.0 } else { -1.0 },
    ))
}

/// The ray through a window pixel from the camera drawn there last (the highest depth
/// whose viewport contains it), as (origin, direction).
pub fn camera_ray(
    views: &[platypus_render::WorldView],
    size: (u32, u32),
    x: f32,
    y: f32,
) -> Option<(Vec3, Vec3)> {
    let (w, h) = (size.0 as f32, size.1 as f32);
    views.iter().rev().find_map(|view| {
        let [vx, vy, vw, vh] = view.viewport;
        let (u, v) = ((x / w - vx) / vw, (y / h - vy) / vh);
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return None;
        }
        let inv = view.camera.view_proj.inverse();
        let (nx, ny) = (u * 2.0 - 1.0, 1.0 - v * 2.0);
        let near = inv * platypus_core::glam::Vec4::new(nx, ny, 0.0, 1.0);
        let far = inv * platypus_core::glam::Vec4::new(nx, ny, 1.0, 1.0);
        let (near, far) = (near.xyz() / near.w, far.xyz() / far.w);
        Some((near, (far - near).normalize()))
    })
}

/// A UI commit op: `[code, args...]`.
fn op(code: u32, args: Vec<Value>) -> Value {
    let mut v = vec![Value::from(code)];
    v.extend(args);
    Value::Array(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    // M5-PNL-02
    #[test]
    fn rays_find_points_on_panels() {
        // A 2×1 m quad at z = -5, facing the camera at the origin.
        let model = Mat4::from_scale_rotation_translation(
            Vec3::new(2.0, 1.0, 1.0),
            platypus_core::glam::Quat::IDENTITY,
            Vec3::new(0.0, 0.0, -5.0),
        );
        let (u, v, d) = ray_quad(model, Vec3::ZERO, Vec3::NEG_Z).unwrap();
        assert!((u - 0.5).abs() < 1e-5 && (v - 0.5).abs() < 1e-5 && (d - 5.0).abs() < 1e-4);
        // Towards its top-left corner.
        let (u, v, _) =
            ray_quad(model, Vec3::ZERO, Vec3::new(-1.0, 0.5, -5.0).normalize()).unwrap();
        assert!(u.abs() < 1e-4 && v.abs() < 1e-4, "{u} {v}");
        // Behind the ray and parallel to the quad: no hit.
        assert!(ray_quad(model, Vec3::ZERO, Vec3::Z).is_none());
        assert!(ray_quad(model, Vec3::ZERO, Vec3::X).is_none());
    }

    // M5-TXT-01
    #[test]
    fn text_meshes_size_their_panel_to_the_text_and_take_its_colour_as_tint() {
        let tree = || (UiTree::new(1.0, 1.0), HashMap::new());
        let text = |s: &str, max_width: f64| {
            vec![
                Value::from(7.0),
                Value::from(3.0),
                s.into(),
                Value::Array(vec![1.0.into(), 0.5.into(), 0.0.into(), 1.0.into()]),
                Value::from(20.0),
                Value::Null,
                Value::from(400.0),
                "center".into(),
                Value::from(max_width),
                Value::from(50.0),
                Value::from(2.0),
            ]
        };
        let mut panels = Panels::default();
        panels.apply_text(&text("Hi", 0.0), tree).unwrap();
        let p = &panels.map[&7];
        let (short, line) = p.size;
        assert!(short > 5.0 && line > 15.0 && line < 40.0, "{:?}", p.size);
        assert_eq!(
            (p.tint, p.pixels_per_meter, p.interactive),
            ([1.0, 0.5, 0.0, 1.0], 50.0, false)
        );
        assert_eq!(p.object, Some(3));
        // Longer text, the same tree: wider, one line.
        panels
            .apply_text(&text("Hello there, world", 0.0), tree)
            .unwrap();
        let (wide, same) = panels.map[&7].size;
        assert!(wide > short * 3.0 && (same - line).abs() < 1.0);
        // A line break: two lines.
        panels.apply_text(&text("Hello\nthere", 0.0), tree).unwrap();
        assert!(panels.map[&7].size.1 > line * 1.8);
        // Wrapped at 60 UI pixels: narrower, several lines.
        panels
            .apply_text(&text("Hello there, world", 60.0), tree)
            .unwrap();
        let (w, h) = panels.map[&7].size;
        assert!(w <= 60.0 && h > line * 1.8, "{w} x {h}");
        // Once drawn, a new colour alone keeps the texture; new text needs a redraw.
        panels.map.get_mut(&7).unwrap().drawn = true;
        let mut recoloured = text("Hello there, world", 60.0);
        recoloured[3] = Value::Array(vec![0.0.into(), 1.0.into(), 0.0.into(), 0.5.into()]);
        panels.apply_text(&recoloured, tree).unwrap();
        let p = &panels.map[&7];
        assert!(p.drawn);
        assert_eq!((p.tint, p.size), ([0.0, 1.0, 0.0, 0.5], (w, h)));
        panels.apply_text(&text("Bye", 60.0), tree).unwrap();
        assert!(!panels.map[&7].drawn);
    }
}
