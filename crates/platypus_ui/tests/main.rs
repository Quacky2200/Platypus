// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Every integration test of `platypus_ui` in one executable: one link instead of one per file
//! (dozens of test executables linking at once used up the machine's memory). Each file is a
//! module; run one with `cargo test -p platypus_ui <file>::`.

mod clip;
mod common;
mod css;
mod dialog;
mod direction;
mod dnd;
mod editor;
mod escape_clip;
mod float;
mod focus;
mod hit;
mod image;
mod inspect;
mod layout_cache;
mod layout_fixtures;
mod layout_props;
mod motion;
mod navigation;
mod pointer;
mod positioning;
mod scroll;
mod selection;
mod svg;
mod table_grid;
mod text;
mod text_props;
mod tree;
mod ui_tables;
mod units;
mod validity;
mod wide_keywords;
