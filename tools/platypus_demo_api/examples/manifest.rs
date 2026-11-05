// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Prints what `#[ts_api]` generated for the demo namespace: the JSON manifest the compiler reads
//! and the TypeScript declarations.
//!
//! `cd tools && cargo run -p platypus_demo_api --example manifest`
fn main() {
    println!(
        "=== manifest (JSON) ===\n{}\n",
        platypus_demo_api::manifest_json()
    );
    println!(
        "=== declarations (.d.ts) ===\n{}",
        platypus_demo_api::declarations()
    );
}
