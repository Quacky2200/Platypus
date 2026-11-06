// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Windows resources of the packaged game: web-ui's `web_ui_build::pe` with the version strings
//! taken from the game's metadata. Must run before the payload is appended and before any signing
//! (`docs/research/platypus-code-signing.md`).

use crate::package::Metadata;
use platypus_core::icon::PRODUCT_NAME;
pub use web_ui_build::pe::{ExeInfo, Resources, read_resources, version_info, version_quad};

/// The version strings of `meta`: Task Manager and Explorer show FileDescription as the
/// program's name: the description, else the title, else Platypus.
pub fn exe_info(meta: &Metadata) -> ExeInfo {
    let product = if meta.title.is_empty() {
        PRODUCT_NAME.to_string()
    } else {
        meta.title.clone()
    };
    ExeInfo {
        description: if meta.description.is_empty() {
            product.clone()
        } else {
            meta.description.clone()
        },
        product,
        company: meta.company.clone(),
        version: meta.version.clone(),
        copyright: meta.copyright.clone(),
        exe: meta.exe.clone(),
        comments: (meta.license != "private").then(|| format!("License: {}", meta.license)),
    }
}

/// `host` (a PE file) with the icon (`.ico` bytes) and version info of `meta` set.
pub fn set_resources(
    host: Vec<u8>,
    meta: &Metadata,
    ico: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    web_ui_build::pe::set_resources(host, &exe_info(meta), ico)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use platypus_core::project::Project;
    use serde_json::json;

    fn meta() -> Metadata {
        let pkg = json!({"name": "demo", "version": "1.2.3", "platypus": {
            "title": "Demo Game", "company": "Acme Ltd", "copyright": "(c) 2026 Acme",
            "description": "A demo", "exe": "demo"}});
        Metadata::from_project(&Project::from_package_json(&pkg, "demo"))
    }

    // M9-TS-2430
    #[test]
    fn version_info_and_icon_round_trip_through_a_pe_file() {
        // This test program is itself a PE file.
        let host = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        let png = {
            let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([10, 200, 30, 255]));
            let mut out = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
                .unwrap();
            out
        };
        let ico = crate::icon::to_ico(&png).unwrap();
        let out = set_resources(host.clone(), &meta(), Some(&ico)).unwrap();
        let r = read_resources(&out).unwrap();
        let get = |k: &str| {
            r.strings
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("ProductName"), Some("Demo Game"));
        assert_eq!(get("CompanyName"), Some("Acme Ltd"));
        assert_eq!(get("FileDescription"), Some("A demo"));
        assert_eq!(get("LegalCopyright"), Some("(c) 2026 Acme"));
        assert_eq!(get("FileVersion"), Some("1.2.3"));
        assert_eq!(get("ProductVersion"), Some("1.2.3"));
        assert_eq!(get("OriginalFilename"), Some("demo.exe"));
        assert_eq!(r.file_version, [1, 2, 3, 0]);
        assert_eq!(r.product_version, [1, 2, 3, 0]);
        assert_eq!(r.icon_sizes, crate::icon::SIZES.map(|s| (s, s)));
        // A payload appended after the edit is found again.
        let mut with = out;
        platypus_core::payload::write_payload(&mut with, &[("a".into(), vec![1])]).unwrap();
        let found = platypus_core::payload::Archive::new(std::io::Cursor::new(with)).unwrap();
        assert!(found.is_some());
    }
}
