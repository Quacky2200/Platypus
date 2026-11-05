// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

include!("../../tools/symbols_build.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../tools/symbols_build.rs");
    write_symbol_table();
}
