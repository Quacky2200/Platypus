// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The math types of the script API: `Mathf`, `Vector2/3/4` (and the grid, quaternion, matrix and
//! shape types as they arrive), declared with `#[ts_api]` as `js/engine/src/math.ts` has them.
//! The values are doubles computed in the order the TypeScript did, so results are bit for bit the
//! same. Both the scene and the rest of the engine API use these types, so they live below them.

// `add`, `sub` and `mul` are the script API's names, not operators.
#![allow(clippy::should_implement_trait)]

pub mod jsnum;
pub mod math;
pub mod outlines;
pub mod quat;
pub mod shapes;
pub mod spline;

pub use math::*;
pub use outlines::*;
pub use quat::*;
pub use shapes::*;

/// The manifests (JSON) of every namespace this crate declares.
pub fn manifests() -> Vec<String> {
    vec![
        math::mathf_api::MANIFEST.to_json(),
        math::vector2_api::MANIFEST.to_json(),
        math::vector3_api::MANIFEST.to_json(),
        math::vector4_api::MANIFEST.to_json(),
        quat::quaternion_api::MANIFEST.to_json(),
        quat::matrix4x4_api::MANIFEST.to_json(),
        shapes::vector2_int_api::MANIFEST.to_json(),
        shapes::vector3_int_api::MANIFEST.to_json(),
        shapes::rect_api::MANIFEST.to_json(),
        shapes::rect_int_api::MANIFEST.to_json(),
        shapes::ray_api::MANIFEST.to_json(),
        shapes::plane_api::MANIFEST.to_json(),
        shapes::bounds_api::MANIFEST.to_json(),
        shapes::pose_api::MANIFEST.to_json(),
        outlines::shapes_api::MANIFEST.to_json(),
    ]
}
