// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! platypus_ts tests: engine services and maths from compiled code (time, input, assets, audio,
//! rendering data, shapes, the facade). One of five test binaries by area, so an edit relinks a
//! smaller one.

// Shared with other test binaries; each uses part of it.
#[allow(dead_code)]
#[path = "../common/cases.rs"]
mod cases;

mod app;
mod assets;
mod assets_native;
mod audio_native;
mod callbacks;
mod clock;
mod datetime;
mod facade;
mod inputs;
mod inputs2;
mod layers;
mod mathf;
mod originals_engine;
mod outlines;
mod prefs;
mod random;
mod render_native;
mod rotations;
mod shapes;
mod timespan;
mod vectors;

// The `ts_rt_host_*` functions the linked programs call.
use platypus_components as _;
