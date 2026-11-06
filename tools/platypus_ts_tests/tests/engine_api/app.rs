// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/app.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_app_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-621..).
use platypus_ts_tests::app_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The manifest and the machine as the engine installs them.
fn reset() {
    use platypus_engine_api::app::{self, AppInfo, SystemInfo};
    app::reset();
    app::take_requests();
    app::set_app(AppInfo {
        name: "demo".into(),
        product_name: "Demo Game".into(),
        subtitle: String::new(),
        version: "1.2.3".into(),
        company_name: "Acme".into(),
        identifier: "com.acme.demo".into(),
        engine_version: "0.9.1".into(),
        is_editor: false,
        run_in_background: false,
        target_frame_rate: 144,
        args: Vec::new(),
    });
    app::set_system(SystemInfo {
        operating_system: "Windows 11 Pro".into(),
        operating_system_family: "Windows".into(),
        operating_system_version: "10.0.22631".into(),
        device_type: "Desktop".into(),
        device_name: "PC-1".into(),
        processor_type: "Ryzen".into(),
        processor_count: 16,
        system_memory_size: 32768,
        architecture: "x86_64".into(),
        language: "en-GB".into(),
        touch_supported: false,
        graphics_device_name: "RTX".into(),
        graphics_device_vendor_id: 4318,
        graphics_device_type: "Vulkan".into(),
        graphics_device_kind: "DiscreteGpu".into(),
        graphics_driver: "555.1".into(),
        screen_width: 2560,
        screen_height: 1440,
        screen_refresh_rate: 165,
        dpi: 109,
    });
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_apc_init() };
    reset();
    f();
    ts_rt_gc_stress(true);
    reset();
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    reset();
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/app_expected.json")).unwrap()
}

fn check_str(name: &str, got: *mut u8) {
    let want: Vec<u16> = expected()[name]["s"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u16)
        .collect();
    assert!(!got.is_null(), "{name} returned null");
    let got = unsafe { Str::from_raw(got as *const Str) }.to_utf16();
    assert_eq!(
        String::from_utf16_lossy(&got),
        String::from_utf16_lossy(&want),
        "{name}"
    );
}

// M9-TS-621
#[test]
fn manifest_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("manifest", ts_apc_manifest());
    });
}

// M9-TS-622
#[test]
fn machine_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("machine", ts_apc_machine());
    });
}

// M9-TS-623
#[test]
fn settable_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("settable", ts_apc_settable());
    });
}

// M9-TS-3361
#[test]
fn application_args_and_quit_with_an_exit_code_are_native() {
    use platypus_engine_api::app::{self, AppInfo, Request};
    let src = "export function t(): string {
  const a = Application.args;
  a.push(\"changed\");
  Application.quit();
  Application.quit(2.7);
  return Application.args.join(\"|\") + \" \" + a.length;
}";
    let manifests = [app::application_api::MANIFEST.to_json()];
    let _ = app::take_requests();
    app::set_app(AppInfo {
        args: vec!["--goto".into(), "a.md#b".into()],
        ..Default::default()
    });
    let got = crate::cases::run_with(src, "apq_", 2_320_000, &manifests).unwrap();
    assert_eq!(got, "--goto|a.md#b 3");
    // Three GC modes, two quits each: the default code and a truncated one.
    let quits: Vec<Request> = app::take_requests();
    assert_eq!(quits.len(), 6, "{quits:?}");
    assert_eq!(&quits[..2], &[Request::Quit(0), Request::Quit(2)]);
    app::reset();
    app::set_app(AppInfo::default());
    let dts = app::application_api::MANIFEST.to_dts();
    assert!(dts.contains("args: string[]"), "{dts}");
    assert!(
        dts.contains("function quit(exitCode?: number): void;"),
        "{dts}"
    );
}

// M9-TS-3364
//
// `input.on` with every kind `js/engine`'s `InputEvents` has, in compiled code: key events carry
// `code`, `key` and `repeat`, pad events `gamepad` and `button` or `axis` and `value`; the
// returned function stops the listener.
#[test]
fn input_on_hears_keys_buttons_and_axes_with_their_fields() {
    use platypus_engine_api::input;
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let src = "const log: string[] = [];
let stopKeys: () => void = () => {};
export function setup(): void {
  stopKeys = input.on(\"keydown\", (e) => { log.push(`down:${e.code}:${e.key}:${e.repeat}`); });
  input.on(\"keyup\", (e) => { log.push(`up:${e.code}`); });
  input.on(\"gamepadbuttondown\", (e) => { log.push(`btn:${e.button}:${e.gamepad.name}`); });
  input.on(\"gamepadaxis\", (e) => { log.push(`axis:${e.axis}:${e.value}`); });
}
export function stop(): void { stopKeys(); }
export function result(): string { return log.join(\" \"); }";
    let mut manifests = platypus_engine_api::platypus_math::manifests();
    manifests.extend(platypus_engine_api::manifests());
    let game = web_ui_ts::jit::compile_and_load(
        src,
        &web_ui_ts::jit::JitOptions {
            prefix: "inq_",
            struct_base: 2_330_000,
            manifests: &manifests,
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    let call = |name: &str| {
        let f: unsafe extern "C" fn() =
            unsafe { std::mem::transmute(game.function(name).unwrap()) };
        unsafe { f() };
    };
    call("setup");
    input::gamepad_connected(3, "Pad");
    input::key_down_with_text("KeyW", "w", false);
    input::key_down_with_text("KeyW", "w", true);
    input::key_up_with_text("KeyW", "w");
    input::gamepad_button(3, "South", 1.0);
    input::gamepad_axis(3, "LeftStickX", -0.5);
    call("stop");
    input::key_down_with_text("KeyA", "a", false);
    let r: unsafe extern "C" fn() -> *mut u8 =
        unsafe { std::mem::transmute(game.function("result").unwrap()) };
    let got = String::from_utf16_lossy(&unsafe { Str::from_raw(r() as *const Str) }.to_utf16());
    assert_eq!(
        got,
        "down:KeyW:w:false down:KeyW:w:true up:KeyW btn:South:Pad axis:LeftStickX:-0.5"
    );
    input::reset();
}
