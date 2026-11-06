// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    web_ui_symbols::write_symbol_table();
}
