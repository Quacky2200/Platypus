// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! web_ui_ts tests: the language (classes, closures, generators, promises, modules, top-level
//! await, the bun-compared case files, the JIT). One of five test binaries by area, so an edit
//! relinks a smaller one.

// Shared with other test binaries; each uses part of it.
#[allow(dead_code)]
#[path = "../common/cases.rs"]
mod cases;

mod arraylib;
mod arrays2;
mod big_frame;
mod bigint;
mod byteapis;
mod bytes;
mod classes;
mod closures;
mod collections;
mod const_closure;
mod debug_lines;
mod debugger;
mod destructure;
mod dynamic;
mod dynamic_calls;
mod editor_gaps;
mod errors;
mod errors2;
mod game_stack;
mod generators;
mod generators2;
mod generics;
mod globals;
mod ifaces;
mod indexed;
mod inherit;
mod iterators;
mod jit;
mod lang2;
mod lang3;
mod leftovers;
mod logic;
mod loops;
mod misc;
mod nullbool;
mod originals;
mod params;
mod promises;
mod promises2;
mod props;
mod readers;
mod sroa;
mod startup;
mod strings;
mod switches;
mod symbols;
mod tables;
mod tla;
mod tla_modules;
mod toplevel;
mod tuples;
mod unions;
mod utility;
mod weak;

// The `ts_rt_host_*` functions the linked programs call.
use platypus_components as _;
