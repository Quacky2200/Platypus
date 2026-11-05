// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Shared types for every Platypus crate: typed IDs, script values and the op buffer
//! that carries commands from scripts to the engine.

pub use glam;

/// The UI target id of a window's tree: `WINDOW_TARGET | handle` in `__platypus_commit(ops, target)`
/// (0 is the screen, smaller ids are world panels).
pub const WINDOW_TARGET: u32 = 0x4000_0000;

mod assets;
pub mod glob;
pub mod icon;
mod ids;
pub mod meta;
mod ops;
pub mod payload;
pub mod project;
pub mod project_edit;
pub mod renderer;
mod value;

pub use assets::{
    AssetFs, RangeRead, asset_fs, decode_data_url, plan_warning, read_asset, read_asset_string,
    resolve_asset, resolve_asset_quiet, set_asset_fs, set_pack_plan, with_asset_fs,
};
pub use ids::{ComponentId, EntityId, NodeId};
pub use ops::{Commit, Op, OpBuffer};
pub use value::Value;
