// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Every integration test of `web_ui_render` in one executable: one link instead of one per file
//! (dozens of test executables linking at once used up the machine's memory). Each file is a
//! module; run one with `cargo test -p web_ui_render <file>::`.

mod blending_over;
mod headless;
mod plan;
mod ui;
mod ui_flow;
