// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Every integration test of `platypus_app` in one executable: one link instead of one per file
//! (dozens of test executables linking at once used up the machine's memory). Each file is a
//! module; run one with `cargo test -p platypus_app <file>::`.

mod bench_smoke;
mod crash_report;
mod dap_session;
mod exe_icon;
mod fps_smoke;
mod licenses;
mod video_prompt_smoke;
