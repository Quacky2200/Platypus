// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `LayerMask`: the 32 render layers. Unity's built-in names come first; the game names the rest in
//! its `package.json` (`"platypus": { "layers": { "8": "Player" } }`), which the host passes to
//! [`set_game_layers`]. Masks are numbers (a bit per layer), like `js/engine/src/layers.ts`.

use std::cell::RefCell;
use ts_api::ts_api;

const BUILT_IN: [(u32, &str); 5] = [
    (0, "Default"),
    (1, "TransparentFX"),
    (2, "Ignore Raycast"),
    (4, "Water"),
    (5, "UI"),
];

thread_local! {
    static GAME: RefCell<Vec<(u32, String)>> = const { RefCell::new(Vec::new()) };
}

/// The game's layer names (the manifest's `platypus.layers`). Entries outside 0..32 or with an empty
/// name are skipped.
pub fn set_game_layers(names: impl IntoIterator<Item = (u32, String)>) {
    GAME.with(|g| {
        *g.borrow_mut() = names
            .into_iter()
            .filter(|(l, n)| *l < 32 && !n.is_empty())
            .collect();
    });
}

/// Layer names in the order scripts see them: the built-ins, then the game's (a game name for a
/// built-in number replaces it in place).
pub fn names() -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = BUILT_IN.iter().map(|(l, n)| (*l, n.to_string())).collect();
    GAME.with(|g| {
        for (l, n) in g.borrow().iter() {
            match out.iter_mut().find(|e| e.0 == *l) {
                Some(e) => e.1 = n.clone(),
                None => out.push((*l, n.clone())),
            }
        }
    });
    out
}

/// The 32 layers' names for a project's `layers` setting (`{"8": "Player"}`), built-ins first as
/// [`names`] has them; "" for a layer without a name. For the editor, whose own layers aren't the
/// project's.
pub fn names_for(layers: &serde_json::Value) -> Vec<String> {
    let mut out = vec![String::new(); 32];
    for (l, n) in BUILT_IN {
        out[l as usize] = n.to_string();
    }
    for (k, v) in layers.as_object().into_iter().flatten() {
        if let (Ok(l), Some(n)) = (k.parse::<usize>(), v.as_str())
            && l < 32
            && !n.is_empty()
        {
            out[l] = n.to_owned();
        }
    }
    out
}

/// JavaScript's `ToUint32`.
fn to_u32(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc().rem_euclid(4_294_967_296.0)) as u32
}

#[ts_api(namespace = "LayerMask")]
pub mod layer_mask_api {
    use super::{names, to_u32};

    /// Every layer (Unity's `~0`).
    #[property]
    pub fn everything() -> f64 {
        4_294_967_295.0
    }

    #[property]
    pub fn nothing() -> f64 {
        0.0
    }

    /// A layer's number by name, or -1.
    pub fn name_to_layer(name: &str) -> f64 {
        names()
            .iter()
            .find(|(_, n)| n == name)
            .map_or(-1.0, |(l, _)| f64::from(*l))
    }

    /// A layer's name, or "" if it has none.
    pub fn layer_to_name(layer: f64) -> String {
        names()
            .into_iter()
            .find(|(l, _)| f64::from(*l) == layer)
            .map(|(_, n)| n)
            .unwrap_or_default()
    }

    /// A mask with the named layers' bits set (unknown names are skipped). Up to four names.
    #[optional(n0, n1, n2, n3)]
    pub fn get_mask(n0: &str, n1: &str, n2: &str, n3: &str) -> f64 {
        let all = names();
        let mut mask = 0u32;
        for name in [n0, n1, n2, n3] {
            if let Some((l, _)) = all.iter().find(|(_, n)| n == name) {
                mask |= 1 << l;
            }
        }
        f64::from(mask)
    }

    /// Whether a mask includes a layer.
    pub fn contains(mask: f64, layer: f64) -> bool {
        // `1 << layer` shifts by the layer modulo 32.
        let shift = to_u32(layer) & 31;
        to_u32(mask) & (1u32 << shift) != 0
    }
}
