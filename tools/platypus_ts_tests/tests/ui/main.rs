// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! platypus_ts tests: React UI through the native reconciler, and windows. One of five test binaries
//! by area, so an edit relinks a smaller one.

mod editor_models;
mod ui;
mod windows;

// The `ts_rt_component_*` functions the linked programs call.
use platypus_components as _;
