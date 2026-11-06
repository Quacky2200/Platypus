// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! web_ui_ts tests: the standard library (Math, JSON, RegExp, typed arrays and buffers, structured
//! clone, URI, Date, Intl and Temporal). One of five test binaries by area, so an edit relinks a
//! smaller one.

mod buffers;
mod canon;
mod clone;
mod date;
mod datefmt;
mod dispnames;
mod fparts;
mod intl;
mod intl4;
mod json;
mod locale;
mod mathlib;
mod monthday;
mod numfmt;
mod regexp;
mod regexp2;
mod resolved;
mod rounding;
mod segmenter;
mod stdlib;
mod temporal;
mod typed;
mod uri;
mod zoned;

// The `ts_rt_host_*` functions the linked programs call.
use platypus_components as _;
