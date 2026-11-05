// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Test harness for the native code `platypus_ts` generates from `sim.ts`: the generated bindings
//! (layouts, tags, extern functions) plus helpers to build state from Rust.
#![allow(non_snake_case, clippy::not_unsafe_ptr_arg_deref)] // a test harness: every pointer comes from the generated code

// The `ts_rt_host_*` functions the linked programs call.
use platypus_components as _;

#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod sim {
    include!(concat!(env!("OUT_DIR"), "/sim_bindings.rs"));
}

/// The sine-circle curve maths (`bench/sine.ts`), prefix `sine_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod sine {
    include!(concat!(env!("OUT_DIR"), "/sine_bindings.rs"));
}

/// Closures (`bench/closures.ts`), prefix `clo_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod clo {
    include!(concat!(env!("OUT_DIR"), "/closures_bindings.rs"));
}

/// Module-level `let` variables (`bench/globals.ts`), prefix `glob_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod glob {
    include!(concat!(env!("OUT_DIR"), "/globals_bindings.rs"));
}

/// `for(;;)`, `do...while` and labels (`bench/loops.ts`), prefix `loop_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod lp {
    include!(concat!(env!("OUT_DIR"), "/loops_bindings.rs"));
}

/// `switch` and enums (`bench/switches.ts`), prefix `sw_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod sw {
    include!(concat!(env!("OUT_DIR"), "/switches_bindings.rs"));
}

/// Destructuring (`bench/destructure.ts`), prefix `des_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod des {
    include!(concat!(env!("OUT_DIR"), "/destructure_bindings.rs"));
}

/// Default, optional and rest parameters, spread (`bench/params.ts`), prefix `par_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod par {
    include!(concat!(env!("OUT_DIR"), "/params_bindings.rs"));
}

/// The Math library surface (`bench/mathlib.ts`), prefix `ml_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod ml {
    include!(concat!(env!("OUT_DIR"), "/mathlib_bindings.rs"));
}

/// The array library (`bench/arraylib.ts`), prefix `al_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod al {
    include!(concat!(env!("OUT_DIR"), "/arraylib_bindings.rs"));
}

/// Plain classes (`bench/classes.ts`), prefix `cls_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod cls {
    include!(concat!(env!("OUT_DIR"), "/classes_bindings.rs"));
}

/// Inheritance (`bench/inherit.ts`), prefix `inh_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod inh {
    include!(concat!(env!("OUT_DIR"), "/inherit_bindings.rs"));
}

/// `@property` tables (`bench/props.ts`), prefix `prop_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod prop {
    include!(concat!(env!("OUT_DIR"), "/props_bindings.rs"));
}

/// Scripts calling an engine API (`bench/engine.ts`), prefix `eng_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod eng {
    include!(concat!(env!("OUT_DIR"), "/engine_bindings.rs"));
}

/// Uses the demo engine API, so that every binary linking this crate also links its shims (the
/// `engine` module's object calls them, and the linker only takes a crate that something uses).
pub fn demo_api_anchor() -> usize {
    platypus_demo_api::link_anchor()
}

/// Compiled scripts driving the real scene (`bench/scene.ts`), prefix `scn_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod scn {
    include!(concat!(env!("OUT_DIR"), "/scene_bindings.rs"));
}

/// Compiled components for the Rust host (`bench/components.ts`), prefix `cmp_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod cmp {
    include!(concat!(env!("OUT_DIR"), "/components_bindings.rs"));
}

/// Component properties and lookups (`bench/components2.ts`), prefix `cm2_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod cm2 {
    include!(concat!(env!("OUT_DIR"), "/components2_bindings.rs"));
}

/// Compiled UI components (`bench/ui.ts`), prefix `uic_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod uic {
    include!(concat!(env!("OUT_DIR"), "/ui_bindings.rs"));
}

/// The sine-circle demo's curve, verbatim (`bench/sine_demo.ts`), prefix `sd_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod sd {
    include!(concat!(env!("OUT_DIR"), "/sine_demo_bindings.rs"));
}

/// The UI benchmark's app (`bench/ui_perf.ts`), prefix `uip_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod uip {
    include!(concat!(env!("OUT_DIR"), "/ui_perf_bindings.rs"));
}

/// Compiled code calling `platypus_engine_api` (`bench/engine2.ts`), prefix `en2_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod en2 {
    include!(concat!(env!("OUT_DIR"), "/engine2_bindings.rs"));
}

/// The whole sine-circle demo component (`bench/sine_demo_full.ts`), prefix `sdf_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod sdf {
    include!(concat!(env!("OUT_DIR"), "/sine_demo_full_bindings.rs"));
}

/// Keeps the engine API shims linked into every binary: the `engine2` module's object calls them.
pub fn engine_api_anchor() -> usize {
    [
        platypus_engine_api::color::color_api::platypus_Color_white as *const () as usize,
        platypus_engine_api::time::time_api::platypus_Time_time as *const () as usize,
        platypus_engine_api::input::input_api::platypus_input_getKey as *const () as usize,
        platypus_engine_api::gizmos::gizmos_api::platypus_Gizmos_clear as *const () as usize,
        platypus_engine_api::camera::camera_api::platypus_Camera_attach as *const () as usize,
        platypus_engine_api::screen::screen_api::platypus_Screen_width as *const () as usize,
        platypus_engine_api::screen::quality_api::platypus_QualitySettings_vSyncCount as *const ()
            as usize,
        platypus_engine_api::color::color_api::platypus_Color_lerp as *const () as usize,
        platypus_engine_api::audio::audio_api::platypus_audio_load as *const () as usize,
        platypus_scenes::scene_manager_api::platypus_SceneManager_loadScene as *const () as usize,
        platypus_physics_api::physics_api::platypus_Physics_step as *const () as usize,
        platypus_physics_api::components::components_api::platypus_Rigidbody_attach as *const ()
            as usize,
        platypus_physics_api::joints::joints_api::platypus_FixedJoint_attach as *const () as usize,
        platypus_physics_api::physics2d::physics2d_api::platypus_Rigidbody2D_attach as *const ()
            as usize,
        platypus_render_api::mesh_api::platypus_Mesh_create as *const () as usize,
        platypus_render_api::texture_api::platypus_Texture_load as *const () as usize,
        platypus_render_api::light_api::platypus_Light_attach as *const () as usize,
        platypus_engine_api::window::window_api::platypus_Window_create as *const () as usize,
    ]
    .iter()
    .fold(0, |a, p| a ^ p)
}

#[used]
static ENGINE_API: fn() -> usize = engine_api_anchor;

/// Keeps the UI bridge (`platypus_ts_ui`) linked into every binary of this crate: the `ui` module's
/// object calls its `ts_ui_*` functions.
#[used]
static UI_BRIDGE: fn() -> usize = platypus_ts_ui::link_anchor;

/// Uses `platypus_scene`, so that binaries linking this crate also link its `Scene` API shims.
pub fn scene_anchor() -> usize {
    platypus_scene::Scene::new().len()
}

/// Strings in compiled code (`bench/strings.ts`), prefix `str_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod strs {
    include!(concat!(env!("OUT_DIR"), "/strings_bindings.rs"));
}

/// Exceptions in compiled code (`bench/errors.ts`), prefix `err_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod errs {
    include!(concat!(env!("OUT_DIR"), "/errors_bindings.rs"));
}

/// The standard library in compiled code (`bench/stdlib.ts`), prefix `std_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod stdl {
    include!(concat!(env!("OUT_DIR"), "/stdlib_bindings.rs"));
}

/// `Map` and `Set` in compiled code (`bench/collections.ts`), prefix `col_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod cols {
    include!(concat!(env!("OUT_DIR"), "/collections_bindings.rs"));
}

/// JSON in compiled code (`bench/json.ts`), prefix `json_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod jsn {
    include!(concat!(env!("OUT_DIR"), "/json_bindings.rs"));
}

/// Regular expressions in compiled code (`bench/regexp.ts`), prefix `re_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod rex {
    include!(concat!(env!("OUT_DIR"), "/regexp_bindings.rs"));
}

/// String methods that take a regular expression (`bench/regexp2.ts`), prefix `re2_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod rex2 {
    include!(concat!(env!("OUT_DIR"), "/regexp2_bindings.rs"));
}

/// `Date` in compiled code (`bench/date.ts`), prefix `dt_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod dat {
    include!(concat!(env!("OUT_DIR"), "/date_bindings.rs"));
}

/// Intl in compiled code (`bench/intl.ts`), prefix `in_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod itl {
    include!(concat!(env!("OUT_DIR"), "/intl_bindings.rs"));
}

/// Number formatting in compiled code (`bench/numfmt.ts`), prefix `nf_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod nfm {
    include!(concat!(env!("OUT_DIR"), "/numfmt_bindings.rs"));
}

/// Date formatting in compiled code (`bench/datefmt.ts`), prefix `df_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod dfm {
    include!(concat!(env!("OUT_DIR"), "/datefmt_bindings.rs"));
}

/// `bench/uri.ts`, prefix `uri_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod uri_t {
    include!(concat!(env!("OUT_DIR"), "/uri_bindings.rs"));
}

/// `bench/errors2.ts`, prefix `en2_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod errors2_t {
    include!(concat!(env!("OUT_DIR"), "/errors2_bindings.rs"));
}

/// `bench/intl4.ts`, prefix `i4_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod intl4_t {
    include!(concat!(env!("OUT_DIR"), "/intl4_bindings.rs"));
}

/// `bench/typed.ts`, prefix `ty_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod typed_t {
    include!(concat!(env!("OUT_DIR"), "/typed_bindings.rs"));
}

/// `bench/bigint.ts`, prefix `bi_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod bigint_t {
    include!(concat!(env!("OUT_DIR"), "/bigint_bindings.rs"));
}

/// `bench/logic.ts`, prefix `lg_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod logic_t {
    include!(concat!(env!("OUT_DIR"), "/logic_bindings.rs"));
}

/// `bench/iterators.ts`, prefix `itr_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod iterators_t {
    include!(concat!(env!("OUT_DIR"), "/iterators_bindings.rs"));
}

/// `bench/generators.ts`, prefix `gn_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod generators_t {
    include!(concat!(env!("OUT_DIR"), "/generators_bindings.rs"));
}

/// `bench/symbols.ts`, prefix `sym_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod symbols_t {
    include!(concat!(env!("OUT_DIR"), "/symbols_bindings.rs"));
}

/// `bench/generators2.ts`, prefix `gn2_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod generators2_t {
    include!(concat!(env!("OUT_DIR"), "/generators2_bindings.rs"));
}

/// `bench/promises.ts`, prefix `pr_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod promises_t {
    include!(concat!(env!("OUT_DIR"), "/promises_bindings.rs"));
}

/// `bench/buffers.ts`, prefix `bf_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod buffers_t {
    include!(concat!(env!("OUT_DIR"), "/buffers_bindings.rs"));
}

/// `bench/clone.ts`, prefix `cl_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod clone_t {
    include!(concat!(env!("OUT_DIR"), "/clone_bindings.rs"));
}

/// `bench/weak.ts`, prefix `wk_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod weak_t {
    include!(concat!(env!("OUT_DIR"), "/weak_bindings.rs"));
}

/// `bench/segmenter.ts`, prefix `sg_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod segmenter_t {
    include!(concat!(env!("OUT_DIR"), "/segmenter_bindings.rs"));
}

/// `bench/locale.ts`, prefix `lc_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod locale_t {
    include!(concat!(env!("OUT_DIR"), "/locale_bindings.rs"));
}

/// `bench/dispnames.ts`, prefix `dn_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod dispnames_t {
    include!(concat!(env!("OUT_DIR"), "/dispnames_bindings.rs"));
}

/// `bench/temporal.ts`, prefix `tp_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod temporal_t {
    include!(concat!(env!("OUT_DIR"), "/temporal_bindings.rs"));
}

/// `bench/zoned.ts`, prefix `zn_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod zoned_t {
    include!(concat!(env!("OUT_DIR"), "/zoned_bindings.rs"));
}

/// `bench/monthday.ts`, prefix `md_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod monthday_t {
    include!(concat!(env!("OUT_DIR"), "/monthday_bindings.rs"));
}

/// `bench/rounding.ts`, prefix `rd_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod rounding_t {
    include!(concat!(env!("OUT_DIR"), "/rounding_bindings.rs"));
}

/// `bench/canon.ts`, prefix `cn_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod canon_t {
    include!(concat!(env!("OUT_DIR"), "/canon_bindings.rs"));
}

/// `bench/fparts.ts`, prefix `fp_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod fparts_t {
    include!(concat!(env!("OUT_DIR"), "/fparts_bindings.rs"));
}

/// `bench/resolved.ts`, prefix `rs_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod resolved_t {
    include!(concat!(env!("OUT_DIR"), "/resolved_bindings.rs"));
}

/// `bench/generics.ts`, prefix `gx_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod generics_t {
    include!(concat!(env!("OUT_DIR"), "/generics_bindings.rs"));
}

/// `bench/nullbool.ts`, prefix `nb_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod nullbool_t {
    include!(concat!(env!("OUT_DIR"), "/nullbool_bindings.rs"));
}

/// `bench/random.ts`, prefix `rnd_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod random_t {
    include!(concat!(env!("OUT_DIR"), "/random_bindings.rs"));
}

/// `bench/layers.ts`, prefix `lyr_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod layers_t {
    include!(concat!(env!("OUT_DIR"), "/layers_bindings.rs"));
}

/// `bench/prefs.ts`, prefix `prf_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod prefs_t {
    include!(concat!(env!("OUT_DIR"), "/prefs_bindings.rs"));
}

/// `bench/assets.ts`, prefix `ast_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod assets_t {
    include!(concat!(env!("OUT_DIR"), "/assets_bindings.rs"));
}

/// `bench/assets_native.ts`, prefix `asn_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod assets_native_t {
    include!(concat!(env!("OUT_DIR"), "/assets_native_bindings.rs"));
}

/// `bench/render_native.ts`, prefix `rdn_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod render_native_t {
    include!(concat!(env!("OUT_DIR"), "/render_native_bindings.rs"));
}

/// `bench/audio_native.ts`, prefix `aud_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod audio_native_t {
    include!(concat!(env!("OUT_DIR"), "/audio_native_bindings.rs"));
}

/// `bench/callbacks.ts`, prefix `cbk_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod callbacks_t {
    include!(concat!(env!("OUT_DIR"), "/callbacks_bindings.rs"));
}

/// `bench/app.ts`, prefix `apc_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod app_t {
    include!(concat!(env!("OUT_DIR"), "/app_bindings.rs"));
}

/// `bench/clock.ts`, prefix `clk_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod clock_t {
    include!(concat!(env!("OUT_DIR"), "/clock_bindings.rs"));
}

/// `bench/inputs.ts`, prefix `inp_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod inputs_t {
    include!(concat!(env!("OUT_DIR"), "/inputs_bindings.rs"));
}

/// `bench/inputs2.ts`, prefix `in2_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod inputs2_t {
    include!(concat!(env!("OUT_DIR"), "/inputs2_bindings.rs"));
}

/// `bench/timespan.ts`, prefix `tsp_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod timespan_t {
    include!(concat!(env!("OUT_DIR"), "/timespan_bindings.rs"));
}

/// `bench/datetime.ts`, prefix `dtm_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod datetime_t {
    include!(concat!(env!("OUT_DIR"), "/datetime_bindings.rs"));
}

/// `Task.delay` and friends (`bench/task.ts`), prefix `tsk_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod task_t {
    include!(concat!(env!("OUT_DIR"), "/task_bindings.rs"));
}

/// `bench/mathf.ts`, prefix `mtf_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod mathf_t {
    include!(concat!(env!("OUT_DIR"), "/mathf_bindings.rs"));
}

/// `bench/vectors.ts`, prefix `vct_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod vectors_t {
    include!(concat!(env!("OUT_DIR"), "/vectors_bindings.rs"));
}

/// `bench/rotations.ts`, prefix `rot_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod rotations_t {
    include!(concat!(env!("OUT_DIR"), "/rotations_bindings.rs"));
}

/// `bench/shapes.ts`, prefix `shp_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod shapes_t {
    include!(concat!(env!("OUT_DIR"), "/shapes_bindings.rs"));
}

/// `bench/anim_clips.ts`, prefix `acl_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod anim_clips_t {
    include!(concat!(env!("OUT_DIR"), "/anim_clips_bindings.rs"));
}

/// `bench/anim_player.ts`, prefix `apl_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod anim_player_t {
    include!(concat!(env!("OUT_DIR"), "/anim_player_bindings.rs"));
}

/// `bench/anim_animator.ts`, prefix `ani_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod anim_animator_t {
    include!(concat!(env!("OUT_DIR"), "/anim_animator_bindings.rs"));
}

/// `bench/scene_objects.ts`, prefix `sob_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod scene_objects_t {
    include!(concat!(env!("OUT_DIR"), "/scene_objects_bindings.rs"));
}

/// `bench/comp_query.ts`, prefix `cqy_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod comp_query_t {
    include!(concat!(env!("OUT_DIR"), "/comp_query_bindings.rs"));
}

/// Component lifecycle and removal (`bench/comp_life.ts`), prefix `clf_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod comp_life_t {
    include!(concat!(env!("OUT_DIR"), "/comp_life_bindings.rs"));
}

/// The Window API from compiled code (`bench/windows.ts`), prefix `win_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod windows_t {
    include!(concat!(env!("OUT_DIR"), "/windows_bindings.rs"));
}

/// `bench/toplevel.ts`, prefix `tl_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod toplevel_t {
    include!(concat!(env!("OUT_DIR"), "/toplevel_bindings.rs"));
}

/// Programs that probe the collector's root analysis (`bench/gap.ts`), prefix `gap_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod gap {
    include!(concat!(env!("OUT_DIR"), "/gap_bindings.rs"));
}

use platypus_ts_rt::{
    Array, Closure, ts_rt_arr_new, ts_rt_arr_push, ts_rt_closure_new, ts_rt_gc_stress,
    ts_rt_obj_new, ts_rt_root_add,
};
use sim::*;

/// Allocates a zeroed script object of type `T` whose bindings header is `hdr` (`HDR_<Name>`).
/// Host allocations never collect.
pub fn alloc<T>(hdr: u64) -> *mut T {
    ts_rt_obj_new(hdr, std::mem::size_of::<T>()) as *mut T
}

/// Keeps a script object (and what it reaches) alive across calls into generated code.
pub fn root<T>(p: *mut T) -> *mut T {
    ts_rt_root_add(p as *mut u8);
    p
}

/// `PLATYPUS_TS_GC=stress` collects at every allocation of generated code and poisons freed memory
/// (a missing root then crashes loudly); `off` never collects on its own.
pub fn gc_mode_from_env() {
    match std::env::var("PLATYPUS_TS_GC").as_deref() {
        Ok("stress") => ts_rt_gc_stress(true),
        Ok("off") => platypus_ts_rt::ts_rt_gc_enable(false),
        _ => {}
    }
}

/// Pushes numbers onto a `number[]`.
pub fn push_f64s(arr: *mut Array, vals: &[f64]) {
    for v in vals {
        // SAFETY: `arr` is a runtime array.
        unsafe { ts_rt_arr_push(arr, v.to_bits()) };
    }
}

pub fn push_ptr<T>(arr: *mut Array, p: *mut T) {
    // SAFETY: `arr` is a runtime array.
    unsafe { ts_rt_arr_push(arr, p as u64) };
}

/// `buildWalls(w, h, rows)` from run-length rows.
pub fn build_walls(w: usize, h: usize, rows: &[Vec<f64>]) -> *mut Walls {
    let outer = ts_rt_arr_new(rows.len() as i64, 1);
    for r in rows {
        let inner = ts_rt_arr_new(r.len() as i64, 0);
        for v in r {
            unsafe { ts_rt_arr_push(inner, v.to_bits()) };
        }
        push_ptr(outer, inner);
    }
    root(unsafe { ts_buildWalls(w as f64, h as f64, root(outer)) })
}

/// A `(map: number) => Walls` closure that always returns `walls`.
pub fn walls_closure(walls: *mut Walls) -> *mut Closure {
    extern "C" fn same(env: *mut u8, _map: f64) -> *mut Walls {
        env as *mut Walls
    }
    root(ts_rt_closure_new(same as *const u8, walls as *mut u8))
}

pub fn new_input(move_x: f64, move_y: f64, aim_x: f64, aim_y: f64, fire: bool) -> *mut PlayerInput {
    let p = root(alloc::<PlayerInput>(HDR_PlayerInput));
    unsafe {
        (*p).moveX = move_x;
        (*p).moveY = move_y;
        (*p).aimX = aim_x;
        (*p).aimY = aim_y;
        (*p).fire = fire as u64;
    }
    p
}

pub fn inputs(list: &[*mut PlayerInput]) -> *mut Array {
    let a = root(ts_rt_arr_new(list.len() as i64, 1));
    for p in list {
        push_ptr(a, *p);
    }
    a
}

pub fn new_zombie(
    id: f64,
    x: f64,
    y: f64,
    hp: f64,
    attack_timer: Option<f64>,
    target_id: f64,
) -> *mut Zombie {
    let z = alloc::<Zombie>(HDR_Zombie);
    unsafe {
        (*z).id = id;
        (*z).x = x;
        (*z).y = y;
        (*z).hp = hp;
        (*z).maxHp = hp;
        (*z).attackTimer = attack_timer.unwrap_or_else(null_num);
        (*z).targetId = target_id;
    }
    z
}

pub fn arr_len(a: *mut Array) -> usize {
    unsafe { (*a).len() }
}

/// Steps `state` for whole 60 Hz frames, like the test file's `run`.
pub fn run(state: *mut SimState, seconds: f64, input: *mut Array, walls: *mut Closure) {
    let frames = (seconds * 60.0).round() as usize;
    for _ in 0..frames {
        unsafe { ts_step(state, input, 1.0 / 60.0, walls) };
    }
}

const W: usize = 1022;
const H: usize = 750;

/// The benchmark scenario (the same as `bench/bench.ts`): one player auto-firing an MP5 that
/// never runs dry or dies, zombies respawning, and the root handles Rust needs.
pub struct Busy {
    pub state: *mut SimState,
    pub walls: *mut Closure,
    pub input: *mut Array,
}

pub fn busy_state() -> Busy {
    unsafe {
        ts_init();
        let open = build_walls(W, H, &vec![vec![W as f64]; H]);
        let walls = walls_closure(open);
        let state = root(ts_createState(7.0, 6.0));
        let p = ts_addPlayer(state);
        ts_startLevel(state, walls);
        let gun = alloc::<Gun>(HDR_Gun);
        (*gun).name = TAG_MP5 as u64;
        (*gun).rounds = 1e9;
        (*gun).clip = 1e9;
        (*gun).damage = 40.0;
        (*gun).auto = 1;
        (*p).gun = gun;
        (*p).health = 1e9;
        (*state).zombiesLeft = 1e9;
        // 50 bullets that stay where they are (the MP5's own last a fraction of a second).
        for i in 0..50 {
            let b = alloc::<Bullet>(HDR_Bullet);
            (*b).id = 5000.0 + i as f64;
            (*b).x = 100.0 + i as f64 * 10.0;
            (*b).y = 100.0;
            (*b).targetId = null_num();
            (*b).ownerId = 1.0;
            (*b).life = 1e9;
            push_ptr((*state).bullets, b);
        }
        let input = inputs(&[new_input(0.0, 0.0, 900.0, 375.0, true)]);
        Busy {
            state,
            walls,
            input,
        }
    }
}

/// The same fingerprint of a state as `checksum` in `bench/bench.ts`.
pub fn busy_checksum(s: *mut SimState) -> f64 {
    unsafe {
        let st = &*s;
        let p = &*(*st.players).get::<*mut Player>(0);
        let mut sum = st.level * 1000.0
            + p.score
            + p.kills * 7.0
            + arr_len(st.bullets) as f64 * 13.0
            + arr_len(st.zombies) as f64 * 17.0
            + st.seed;
        for i in 0..arr_len(st.zombies) {
            let z = &*(*st.zombies).get::<*mut Zombie>(i);
            sum += z.x + z.y;
        }
        sum
    }
}

/// Compiled programs keep module globals in static data that every thread shares, while each
/// thread has its own heap; tests that initialise and run one must not overlap.
pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Scene files, `instantiate` and prefabs (`bench/scenes.ts`), prefix `scn_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod scenes_t {
    include!(concat!(env!("OUT_DIR"), "/scenes_bindings.rs"));
}

/// Physics from compiled code (`bench/physics.ts`), prefix `phy_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod physics_t {
    include!(concat!(env!("OUT_DIR"), "/physics_bindings.rs"));
}

/// Contact hooks on components (`bench/contacts.ts`), prefix `cnt_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod contacts_t {
    include!(concat!(env!("OUT_DIR"), "/contacts_bindings.rs"));
}

/// The component / GameObject surface of scene.ts (`bench/comp_api.ts`), prefix `cap_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod comp_api_t {
    include!(concat!(env!("OUT_DIR"), "/comp_api_bindings.rs"));
}

/// Physics components from compiled code (`bench/physics_comp.ts`), prefix `phc_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod physics_comp_t {
    include!(concat!(env!("OUT_DIR"), "/physics_comp_bindings.rs"));
}

/// `bench/facade.ts`, prefix `fac_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod facade_t {
    include!(concat!(env!("OUT_DIR"), "/facade_bindings.rs"));
}

/// `bench/outlines.ts` (the facade's `Shapes` and `Spline` next to the native ones), prefix `otl_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod outlines_t {
    include!(concat!(env!("OUT_DIR"), "/outlines_bindings.rs"));
}

/// `bench/originals_engine.ts`, prefix `ogn_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod originals_engine_t {
    include!(concat!(env!("OUT_DIR"), "/originals_engine_bindings.rs"));
}
