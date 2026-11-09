// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The reviewed tables: the licences the tool knows, in the order a choice is made, and the
//! crates it lets through although they break a rule. A new entry needs a person to look at the
//! crate first; the reason says what they found.

/// The licences the tool knows, in order of preference: of `MIT OR Apache-2.0` Platypus uses
/// the earlier one (the permissive notice-only licences first, public-domain ones next, MPL last).
pub const KNOWN: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-3-Clause",
    "BSD-2-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "BSL-1.0",
    "0BSD",
    "Unlicense",
    "CC0-1.0",
    "MPL-2.0",
];

/// Licences whose text has no per-project part beyond copyright lines: the text is printed once
/// and other wordings of it are not (they differ in layout or appendix only). The others (MIT,
/// BSD, ISC, Zlib) print every distinct wording, since a project may change them (BSD's
/// "Neither the name of ..." clause).
pub const FIXED_TEXT: &[&str] = &[
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "Unicode-3.0",
    "BSL-1.0",
    "Unlicense",
    "CC0-1.0",
    "MPL-2.0",
];

/// What an [`Allow`] entry lets through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The crate ships no licence file for its licence: the section's standard text is used,
    /// with the authors from its `Cargo.toml`.
    NoLicenceFile,
    /// A licence string the tool does not know (listed under its own name, with its own text).
    UnknownLicence,
    /// A GPL, LGPL or AGPL licence.
    Copyleft,
}

/// A crate (or, ending in `*`, a family of crates by name prefix) let through one rule.
#[derive(Debug, Clone, Copy)]
pub struct Allow {
    pub krate: &'static str,
    pub rule: Rule,
    pub reason: &'static str,
}

impl Allow {
    pub fn matches(&self, name: &str) -> bool {
        match self.krate.strip_suffix('*') {
            Some(prefix) => name.starts_with(prefix),
            None => name == self.krate,
        }
    }
}

/// What to do with a licence file below the top of a crate (code or data it bundles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedUse {
    /// Bundled into the build: its text goes into the notices, with what it is.
    Include(&'static str),
    /// Not built into the host, or already covered: why.
    Skip(&'static str),
}

/// A licence file inside a crate, by the crate's name and the file's path in it (`/`).
#[derive(Debug, Clone, Copy)]
pub struct Nested {
    pub krate: &'static str,
    pub path: &'static str,
    pub what: NestedUse,
}

/// The tables `render` checks against; tests pass their own.
#[derive(Debug, Clone, Copy)]
pub struct Tables {
    pub allow: &'static [Allow],
    pub nested: &'static [Nested],
}

/// The reviewed tables.
pub const REVIEWED: Tables = Tables {
    allow: ALLOW,
    nested: NESTED,
};

const NO_FILE: Rule = Rule::NoLicenceFile;

/// Crates published to crates.io without a licence file. Each was looked at: its `Cargo.toml`
/// names a standard licence (and its repository), so the standard text of that licence is used
/// with the authors from `Cargo.toml`.
const ALLOW: &[Allow] = &[
    Allow {
        krate: "asn1-rs-impl",
        rule: NO_FILE,
        reason: "rusticata/asn1-rs: MIT/Apache-2.0 in Cargo.toml (a proc macro, reviewed 2026-11-09)",
    },
    Allow {
        krate: "clipboard-win",
        rule: NO_FILE,
        reason: "BSL-1.0 (Boost) in Cargo.toml; the standard Boost text applies",
    },
    Allow {
        krate: "cranelift-*",
        rule: NO_FILE,
        reason: "Bytecode Alliance (wasmtime repository): Apache-2.0 WITH LLVM-exception, the text its sibling crates ship",
    },
    Allow {
        krate: "wasmtime-internal-*",
        rule: NO_FILE,
        reason: "Bytecode Alliance (wasmtime repository): Apache-2.0 WITH LLVM-exception, the text its sibling crates ship",
    },
    Allow {
        krate: "ufbx",
        rule: NO_FILE,
        reason: "ufbx/ufbx-rust: MIT OR Unlicense in Cargo.toml; the crate and the ufbx C sources it compiles in (ufbx/ufbx.c, ufbx.h by the same author, MIT or public domain) ship no licence file, so the standard MIT text with the crate's authors applies (reviewed 2026-10-29)",
    },
    Allow {
        krate: "dasp_sample",
        rule: NO_FILE,
        reason: "RustAudio/dasp: MIT OR Apache-2.0 in Cargo.toml",
    },
    Allow {
        krate: "gilrs",
        rule: NO_FILE,
        reason: "gilrs-project/gilrs: Apache-2.0/MIT in Cargo.toml",
    },
    Allow {
        krate: "gilrs-core",
        rule: NO_FILE,
        reason: "gilrs-project/gilrs: Apache-2.0/MIT in Cargo.toml",
    },
    Allow {
        krate: "gl_generator",
        rule: NO_FILE,
        reason: "brendanzab/gl-rs: Apache-2.0 in Cargo.toml",
    },
    Allow {
        krate: "khronos_api",
        rule: NO_FILE,
        reason: "brendanzab/gl-rs: Apache-2.0 in Cargo.toml (the Khronos API registry files)",
    },
    Allow {
        krate: "kira",
        rule: NO_FILE,
        reason: "tesselode/kira: MIT OR Apache-2.0 in Cargo.toml",
    },
    Allow {
        krate: "lyon_*",
        rule: NO_FILE,
        reason: "nical/lyon: MIT OR Apache-2.0 in Cargo.toml",
    },
    Allow {
        krate: "oxc_*",
        rule: NO_FILE,
        reason: "oxc-project/oxc: MIT in Cargo.toml",
    },
    Allow {
        krate: "parry3d",
        rule: NO_FILE,
        reason: "dimforge/parry: Apache-2.0 in Cargo.toml",
    },
    Allow {
        krate: "rapier3d",
        rule: NO_FILE,
        reason: "dimforge/rapier: Apache-2.0 in Cargo.toml",
    },
    Allow {
        krate: "profiling*",
        rule: NO_FILE,
        reason: "aclysma/profiling: MIT OR Apache-2.0 in Cargo.toml",
    },
    Allow {
        krate: "rstar",
        rule: NO_FILE,
        reason: "georust/rstar: MIT OR Apache-2.0 in Cargo.toml",
    },
    Allow {
        krate: "selectors",
        rule: NO_FILE,
        reason: "servo/stylo: MPL-2.0 in Cargo.toml",
    },
    Allow {
        krate: "spirv",
        rule: NO_FILE,
        reason: "gfx-rs/rspirv: Apache-2.0 in Cargo.toml",
    },
    Allow {
        krate: "taffy",
        rule: NO_FILE,
        reason: "DioxusLabs/taffy: MIT in Cargo.toml",
    },
    Allow {
        krate: "wuff",
        rule: NO_FILE,
        reason: "nicoburns/wuff: MIT in Cargo.toml",
    },
];

const NESTED: &[Nested] = &[
    Nested {
        krate: "fontdb",
        path: "tests/fonts/LICENSE.txt",
        what: NestedUse::Skip("test fonts, not built into the crate"),
    },
    Nested {
        krate: "gilrs",
        path: "SDL_GameControllerDB/LICENSE",
        what: NestedUse::Include("SDL_GameControllerDB, the controller mappings gilrs builds in"),
    },
    Nested {
        krate: "libz-sys",
        path: "src/zlib/LICENSE",
        what: NestedUse::Skip("zlib: the hand-written zlib section"),
    },
    Nested {
        krate: "libz-sys",
        path: "src/zlib/contrib/dotzlib/LICENSE_1_0.txt",
        what: NestedUse::Skip("zlib's .NET wrapper, not built"),
    },
    Nested {
        krate: "libz-sys",
        path: "src/zlib/contrib/minizip/LICENSE.Info-Zip",
        what: NestedUse::Skip("minizip, not built"),
    },
    Nested {
        krate: "libz-sys",
        path: "src/zlib-ng/LICENSE.md",
        what: NestedUse::Skip(
            "zlib-ng, built only with libz-sys's zlib-ng features, which are off",
        ),
    },
    Nested {
        krate: "mozangle",
        path: "gfx/angle/checkout/LICENSE",
        what: NestedUse::Skip("ANGLE: the hand-written ANGLE section"),
    },
    Nested {
        krate: "regex-syntax",
        path: "src/unicode_tables/LICENSE-UNICODE",
        what: NestedUse::Include("The Unicode data tables regex-syntax is generated from"),
    },
    Nested {
        krate: "ring",
        path: "src/polyfill/once_cell/LICENSE-APACHE",
        what: NestedUse::Skip(
            "once_cell's Apache-2.0 text, printed once with the other Apache-2.0 crates",
        ),
    },
    Nested {
        krate: "ring",
        path: "src/polyfill/once_cell/LICENSE-MIT",
        what: NestedUse::Include("once_cell, the lazy-init code ring includes"),
    },
    Nested {
        krate: "ring",
        path: "third_party/fiat/LICENSE",
        what: NestedUse::Include("fiat-crypto, the field arithmetic ring compiles in (Apache-2.0)"),
    },
    Nested {
        krate: "tracing-core",
        path: "src/spin/LICENSE",
        what: NestedUse::Include("spin, the lock code tracing-core includes"),
    },
    Nested {
        krate: "zstd-sys",
        path: "zstd/COPYING",
        what: NestedUse::Skip(
            "GPL-2.0, the other choice of zstd's dual licence; Platypus uses its BSD licence",
        ),
    },
    Nested {
        krate: "zstd-sys",
        path: "zstd/LICENSE",
        what: NestedUse::Include("Zstandard, the compression library zstd-sys compiles in"),
    },
];
