// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Small pieces of the object model: the `HideFlags` constants (flags that keep an object out of
//! the editor views and saved scenes; combine with `|`).

use platypus_scene::ObjectId;
use ts_api::ts_api;

/// Cleans up after objects the script destroyed: the engine components on them are removed, and
/// the list is returned so the host can drop its own (a `NativeHost` runs `onDestroy` with
/// `destroy_on_objects`). Call it after running scripts, once a frame.
pub fn process_destroyed() -> Vec<ObjectId> {
    let list = platypus_scene::api::take_destroyed();
    for o in &list {
        crate::camera::remove_on(*o);
        crate::audio_components::remove_on(*o);
        crate::spline::remove_on(*o);
    }
    list
}

#[ts_api(namespace = "HideFlags")]
pub mod hide_flags_api {
    #[property]
    #[js_name = "None"]
    pub fn none() -> f64 {
        0.0
    }

    /// Not listed in the Hierarchy.
    #[property]
    #[js_name = "HideInHierarchy"]
    pub fn hide_in_hierarchy() -> f64 {
        1.0
    }

    /// Not shown in the Inspector.
    #[property]
    #[js_name = "HideInInspector"]
    pub fn hide_in_inspector() -> f64 {
        2.0
    }

    /// Not saved with scenes in the editor.
    #[property]
    #[js_name = "DontSaveInEditor"]
    pub fn dont_save_in_editor() -> f64 {
        4.0
    }

    /// Not editable in the Inspector.
    #[property]
    #[js_name = "NotEditable"]
    pub fn not_editable() -> f64 {
        8.0
    }

    /// Not saved with scenes in a build.
    #[property]
    #[js_name = "DontSaveInBuild"]
    pub fn dont_save_in_build() -> f64 {
        16.0
    }

    /// Kept when unused assets are unloaded.
    #[property]
    #[js_name = "DontUnloadUnusedAsset"]
    pub fn dont_unload_unused_asset() -> f64 {
        32.0
    }

    /// Never saved.
    #[property]
    #[js_name = "DontSave"]
    pub fn dont_save() -> f64 {
        52.0
    }

    /// Hidden and never saved.
    #[property]
    #[js_name = "HideAndDontSave"]
    pub fn hide_and_dont_save() -> f64 {
        61.0
    }
}
