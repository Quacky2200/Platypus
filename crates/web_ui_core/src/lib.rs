// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The types every web-ui crate shares: `Value` (what crosses into scripts), ids, the UI
//! commit and ops, asset reading (`read_asset`, the asset file system, `data:` URLs) and how
//! translucent colours blend. No dependency on any other web-ui or Platypus crate.

mod assets;
mod blending;
mod ids;
mod ops;
mod value;

pub use assets::{
    AssetFs, RangeRead, asset_fs, decode_data_url, plan_warning, read_asset, read_asset_string,
    resolve_asset, resolve_asset_quiet, set_asset_fs, set_pack_plan, with_asset_fs,
};
pub use blending::Blending;
pub use ids::{ComponentId, EntityId, NodeId};
pub use ops::{Commit, Op, OpBuffer};
pub use value::Value;
