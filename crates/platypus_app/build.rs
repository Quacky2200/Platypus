// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The host's build script: the main thread's stack size, and the Platypus icon. The icon
//! (`resources/icon/icon.png`) becomes the window icon of games without their own (256 x 256
//! RGBA in `OUT_DIR`, see `info::default_icon`) and, on Windows with the MSVC linker, the file
//! icon and version info of `platypus-app.exe` (a `.res` file the linker reads), so even
//! `cargo run` builds show it.

use image::imageops::FilterType;
use image::{ImageFormat, RgbaImage};
use std::path::PathBuf;

use platypus_core::icon;

/// The window icon's side in pixels (Windows scales it to the title bar and taskbar).
const WINDOW_ICON: u32 = 256;

fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let msvc = target == "windows" && env == "msvc";
    // The main thread runs the game's compiled code and the UI reconciler, which recurse over
    // the element tree (a long Markdown page in `examples/md-reader` nests a few hundred calls
    // deep). Windows gives a program's main thread 1 MB unless the executable asks for more; a
    // debug build's frames are large, so ask for 16 MB (other platforms default to 8 MB).
    if target == "windows" {
        if msvc {
            println!("cargo:rustc-link-arg-bins=/STACK:16777216");
        } else {
            println!("cargo:rustc-link-arg-bins=-Wl,--stack,16777216");
        }
    }
    // Steam's DLL is loaded on the first Steam call, not at start-up: a missing steam_api64.dll
    // then only keeps Steam off (steam_host.rs probes for it) instead of stopping the program.
    if msvc && std::env::var_os("CARGO_FEATURE_STEAM").is_some() {
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:steam_api64.dll");
    }
    // Linux and macOS have no delay-load: libsteam_api is a start-up dependency there, looked for
    // beside the program (a game that uses `steam` ships it next to the exe).
    if std::env::var_os("CARGO_FEATURE_STEAM").is_some() {
        match target.as_str() {
            "linux" => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN"),
            "macos" => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path"),
            _ => {}
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../resources/icon/icon.png");

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let full = image::load_from_memory_with_format(icon::DEFAULT_PNG, ImageFormat::Png)
        .expect("resources/icon/icon.png decodes")
        .to_rgba8();
    // One good downscale to 256 first; the small sizes come from it (Lanczos from 1254 px at
    // every size is slow in an unoptimised build script, and looks the same).
    let large = resized(&full, WINDOW_ICON);
    std::fs::write(out.join("window_icon.rgba"), large.as_raw()).expect("writing the window icon");
    if msvc {
        let images: Vec<(u32, Vec<u8>)> = icon::ICO_SIZES
            .iter()
            .map(|&s| {
                let img = if s == WINDOW_ICON {
                    large.clone()
                } else {
                    resized(&large, s)
                };
                let mut png = Vec::new();
                img.write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
                    .expect("encoding an icon size");
                (s, png)
            })
            .collect();
        let version = env!("CARGO_PKG_VERSION");
        let quad = version_quad(version);
        let strings = [
            ("CompanyName", "The Platypus Authors"),
            ("FileDescription", icon::PRODUCT_NAME),
            ("FileVersion", version),
            ("LegalCopyright", "Copyright 2026 The Platypus Authors"),
            ("ProductName", icon::PRODUCT_NAME),
            ("ProductVersion", version),
        ];
        let res = out.join("platypus.res");
        std::fs::write(&res, icon::windows_res(&images, &strings, quad))
            .expect("writing platypus.res");
        println!("cargo:rustc-link-arg-bins={}", res.display());
    }
}

fn resized(img: &RgbaImage, side: u32) -> RgbaImage {
    image::imageops::resize(img, side, side, FilterType::Lanczos3)
}

/// `"0.1.0"` as the four numbers of a Windows file version.
fn version_quad(version: &str) -> [u16; 4] {
    let mut q = [0u16; 4];
    for (slot, part) in q.iter_mut().zip(version.split(['.', '-', '+'])) {
        *slot = part.parse().unwrap_or(0);
    }
    q
}
