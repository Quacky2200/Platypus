// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Cameras that draw into a screen UI `<canvas>` element (`Camera.targetCanvas`): each one's
//! render texture follows the element's box at the UI's pixel scale, and the UI renderer shows
//! the texture where it draws the element (`web_ui_dom::canvas_key`).

use platypus_render::{Gpu, UiRenderer};
use web_ui_dom::UiTree;

use crate::lazy_world::LazyWorld;
use crate::world_view::{CanvasChanges, WorldViews};

/// A `<canvas>` node's box in physical pixels (`scale` per UI pixel); `None` when it isn't a laid
/// out canvas element.
fn canvas_pixels(ui: &UiTree, node: u32, scale: f32) -> Option<(u32, u32)> {
    let id = platypus_core::NodeId(node);
    if ui.kind(id) != Some(web_ui_dom::CANVAS) {
        return None;
    }
    let r = ui.rect(id)?;
    let px = |v: f32| (v * scale).round() as u32;
    let size = (px(r.width), px(r.height));
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// Before the world view's uploads are applied: sizes the canvas cameras' textures.
pub fn sync(view: &mut WorldViews, ui: &UiTree, scale: f32) -> CanvasChanges {
    view.sync_canvases(|node| canvas_pixels(ui, node, scale))
}

/// After they are: hands the textures made to the UI renderer and takes back those gone. Without
/// a world (nothing made) the canvases are forgotten, to be made again next frame.
pub fn show(
    changes: &CanvasChanges,
    view: &mut WorldViews,
    world: &mut LazyWorld,
    renderer: &mut UiRenderer,
    gpu: &Gpu,
) {
    for node in &changes.gone {
        renderer.remove_external_image(&web_ui_dom::canvas_key(platypus_core::NodeId(*node)));
    }
    let Some(world) = world.built() else {
        if !changes.made.is_empty() {
            view.forget_canvases();
        }
        return;
    };
    for (node, id, size) in &changes.made {
        if let Some(texture) = world.render_texture_view(*id) {
            let key = web_ui_dom::canvas_key(platypus_core::NodeId(*node));
            renderer.set_external_image(gpu, &key, texture, *size);
        }
    }
}
