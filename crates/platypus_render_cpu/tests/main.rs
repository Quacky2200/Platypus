// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Every integration test of `platypus_render_cpu` in one executable: one link instead of one per file
//! (dozens of test executables linking at once used up the machine's memory). Each file is a
//! module; run one with `cargo test -p platypus_render_cpu <file>::`.

#[cfg(feature = "vello")]
mod blending;
#[cfg(feature = "vello")]
mod gradients;
mod vs_gles;
