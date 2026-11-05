// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Compiles the `sim.ts` fixture and the programs in `bench/` with `platypus_ts` and links the
//! objects into this crate's targets.
//!
//! Each program's object is cached in OUT_DIR under a key: a hash of its source, symbol prefix,
//! header base, manifests and this build script's own executable. The executable stands for the
//! compiler: cargo relinks it whenever `platypus_ts` (or a crate making the manifests) changes, so a
//! compiler edit recompiles every program and a bench edit only its own. Programs compile in
//! parallel.
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Which engine API manifests a program may call.
#[derive(Clone, Copy)]
enum Api {
    None,
    /// `platypus_demo_api` (`#[ts_api]` test namespaces).
    Demo,
    /// The UI half (`platypus_ts_ui`).
    Ui,
    /// The scene API and the math types.
    Scene,
    /// Every engine namespace.
    Engine,
}

/// The programs linked next to sim.ts: (name, symbol prefix `ts_<prefix>*`, header base).
/// Bindings go to `<name>_bindings.rs`.
const PROGRAMS: &[(&str, &str, u32)] = &[
    ("sine", "sine_", 1000),
    ("gap", "gap_", 2000),
    ("closures", "clo_", 3000),
    ("globals", "glob_", 4000),
    ("loops", "loop_", 5000),
    ("switches", "sw_", 6000),
    ("destructure", "des_", 7000),
    ("params", "par_", 8000),
    ("mathlib", "ml_", 9000),
    ("arraylib", "al_", 10000),
    ("classes", "cls_", 11000),
    ("inherit", "inh_", 12000),
    ("props", "prop_", 13000),
    ("engine", "eng_", 14000),
    ("scene", "scn_", 15000),
    ("strings", "str_", 16000),
    ("errors", "err_", 17000),
    ("stdlib", "std_", 18000),
    ("collections", "col_", 19000),
    ("json", "json_", 20000),
    ("regexp", "re_", 21000),
    ("regexp2", "re2_", 22000),
    ("date", "dt_", 23000),
    ("intl", "in_", 24000),
    ("numfmt", "nf_", 25000),
    ("datefmt", "df_", 26000),
    ("uri", "uri_", 27000),
    ("errors2", "e2_", 28000),
    ("intl4", "i4_", 29000),
    ("typed", "ty_", 30000),
    ("bigint", "bi_", 31000),
    ("logic", "lg_", 32000),
    ("iterators", "itr_", 33000),
    ("generators", "gn_", 34000),
    ("symbols", "sym_", 700),
    ("generators2", "gn2_", 800),
    ("promises", "pr_", 900),
    ("buffers", "bf_", 1000),
    ("clone", "cl_", 1100),
    ("weak", "wk_", 1200),
    ("segmenter", "sg_", 1300),
    ("locale", "lc_", 1400),
    ("dispnames", "dn_", 1500),
    ("temporal", "tp_", 1600),
    ("zoned", "zn_", 1700),
    ("monthday", "md_", 1800),
    ("rounding", "rd_", 1900),
    ("canon", "cn_", 2000),
    ("fparts", "fp_", 2100),
    ("resolved", "rs_", 2200),
    ("components", "cmp_", 2300),
    ("ui", "uic_", 2500),
    ("sine_demo", "sd_", 2600),
    ("ui_perf", "uip_", 2700),
    ("engine2", "en2_", 2800),
    ("sine_demo_full", "sdf_", 2900),
    ("generics", "gx_", 2400),
    ("nullbool", "nb_", 3000),
    ("components2", "cm2_", 3100),
    ("task", "tsk_", 4200),
    ("comp_life", "clf_", 5200),
    ("random", "rnd_", 3200),
    ("layers", "lyr_", 3300),
    ("prefs", "prf_", 3400),
    ("assets", "ast_", 3500),
    ("app", "apc_", 3600),
    ("clock", "clk_", 3700),
    ("inputs", "inp_", 3800),
    ("inputs2", "in2_", 3900),
    ("timespan", "tsp_", 4000),
    ("datetime", "dtm_", 4100),
    ("mathf", "mtf_", 4300),
    ("vectors", "vct_", 4400),
    ("rotations", "rot_", 4500),
    ("shapes", "shp_", 4600),
    ("anim_clips", "acl_", 4700),
    ("anim_player", "apl_", 4800),
    ("anim_animator", "ani_", 4900),
    ("scene_objects", "sob_", 5000),
    ("comp_query", "cqy_", 5100),
    ("scenes", "scf_", 5300),
    ("physics", "phy_", 5400),
    ("contacts", "cnt_", 6000),
    ("comp_api", "cap_", 6100),
    ("physics_comp", "phc_", 5420),
    ("callbacks", "cbk_", 7000),
    ("audio_native", "aud_", 7100),
    ("render_native", "rdn_", 7200),
    ("assets_native", "asn_", 7500),
    ("facade", "fac_", 7300),
    ("windows", "win_", 7400),
    ("outlines", "otl_", 7800),
    ("toplevel", "tl_", 7600),
    ("originals_engine", "ogn_", 7900),
];

fn api_of(name: &str) -> Api {
    match name {
        "engine" => Api::Demo,
        "ui" | "ui_perf" => Api::Ui,
        "scene" | "components" | "components2" | "sine_demo" => Api::Scene,
        "engine2" | "sine_demo_full" | "random" | "layers" | "prefs" | "assets" | "app"
        | "clock" | "inputs" | "inputs2" | "timespan" | "datetime" | "mathf" | "vectors"
        | "rotations" | "shapes" | "anim_clips" | "anim_player" | "anim_animator"
        | "scene_objects" | "comp_query" | "comp_life" | "scenes" | "physics" | "contacts"
        | "comp_api" | "physics_comp" | "callbacks" | "audio_native" | "render_native"
        | "assets_native" | "facade" | "windows" | "outlines" | "originals_engine" => Api::Engine,
        _ => Api::None,
    }
}

/// The manifests of each `Api`, made once (not once per program).
struct Manifests {
    demo: Vec<String>,
    ui: Vec<String>,
    scene: Vec<String>,
    engine: Vec<String>,
}

impl Manifests {
    fn new() -> Self {
        let mut scene = vec![platypus_scene::api::scene_api::MANIFEST.to_json()];
        scene.extend(platypus_engine_api::platypus_math::manifests());
        scene.extend(platypus_components::manifests());
        Manifests {
            demo: vec![platypus_demo_api::manifest_json()],
            ui: vec![platypus_ts_ui::manifest_json()],
            scene,
            engine: engine_manifests(),
        }
    }

    fn get(&self, api: Api) -> &[String] {
        match api {
            Api::None => &[],
            Api::Demo => &self.demo,
            Api::Ui => &self.ui,
            Api::Scene => &self.scene,
            Api::Engine => &self.engine,
        }
    }
}

/// Every engine API namespace a compiled program may call.
fn engine_manifests() -> Vec<String> {
    let mut all = vec![platypus_scene::api::scene_api::MANIFEST.to_json()];
    all.extend(platypus_engine_api::manifests());
    all.extend(platypus_scenes::manifests());
    all.extend(platypus_physics_api::manifests());
    all.extend(platypus_render_api::manifests());
    all.extend(platypus_components::manifests());
    all
}

/// A program whose cached object is stale.
struct Job<'a> {
    name: &'static str,
    prefix: &'static str,
    base: u32,
    /// The source after bundling its imports (what the compiler sees).
    source: String,
    api: &'a [String],
    key: u64,
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=build.rs");
    let compiler = compiler_hash();
    let manifests = Manifests::new();

    let sim_path = manifest.join("fixtures/sim.ts");
    println!("cargo:rerun-if-changed={}", sim_path.display());
    let sim = std::fs::read_to_string(&sim_path).expect("read sim.ts");
    let sim_key = key(compiler, "sim", "", 0, &sim, &[]);
    if !fresh(&dir, "sim", sim_key) {
        let t = std::time::Instant::now();
        let out = platypus_ts::compile_source(&sim, "speed")
            .unwrap_or_else(|e| panic!("platypus_ts: {e}"));
        println!(
            "cargo:warning=platypus_ts compiled sim.ts in {:?} (lower {:?}, cranelift {:?})",
            t.elapsed(),
            out.lower_time,
            out.codegen_time
        );
        std::fs::write(dir.join("sim.ir.txt"), &out.ir).unwrap();
        store(&dir, "sim", sim_key, &out);
    }
    println!("cargo:rustc-link-arg={}", dir.join("sim.obj").display());

    let mut stale = Vec::new();
    for &(name, prefix, base) in PROGRAMS {
        let path = manifest.join(format!("bench/{name}.ts"));
        println!("cargo:rerun-if-changed={}", path.display());
        // `toplevel` imports a second file: bundle it like `platypus-build` does.
        let source = if name == "toplevel" {
            let dep = manifest.join("bench/toplevel_dep.ts");
            println!("cargo:rerun-if-changed={}", dep.display());
            platypus_ts::modules::bundle(&path)
                .unwrap_or_else(|e| panic!("platypus_ts {name}: {e}"))
                .source
        } else {
            std::fs::read_to_string(&path).unwrap()
        };
        let api = manifests.get(api_of(name));
        let key = key(compiler, name, prefix, base, &source, api);
        if !fresh(&dir, name, key) {
            stale.push(Job {
                name,
                prefix,
                base,
                source,
                api,
                key,
            });
        }
        println!(
            "cargo:rustc-link-arg={}",
            dir.join(format!("{name}.obj")).display()
        );
    }

    // Compile the stale ones on every core; each worker takes the next job.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let errors: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers.min(stale.len()))
            .map(|_| {
                s.spawn(|| {
                    let mut errors = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(j) = stale.get(i) else {
                            break;
                        };
                        match platypus_ts::compile_with_manifests(
                            &j.source, "speed", j.prefix, j.base, j.api,
                        ) {
                            Ok(out) => store(&dir, j.name, j.key, &out),
                            Err(e) => errors.push(format!("platypus_ts {}: {e}", j.name)),
                        }
                    }
                    errors
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

/// A hash of this build script's executable: it changes whenever the compiler does.
fn compiler_hash() -> u64 {
    let exe = std::env::current_exe().expect("build script path");
    let mut h = DefaultHasher::new();
    std::fs::read(exe).expect("read build script").hash(&mut h);
    h.finish()
}

fn key(compiler: u64, name: &str, prefix: &str, base: u32, source: &str, api: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    (compiler, name, prefix, base, source, api).hash(&mut h);
    h.finish()
}

/// Whether OUT_DIR holds `name`'s object and bindings made under key `k`.
fn fresh(dir: &Path, name: &str, k: u64) -> bool {
    let stamp = std::fs::read_to_string(dir.join(format!("{name}.key"))).unwrap_or_default();
    stamp == format!("{k:016x}")
        && dir.join(format!("{name}.obj")).exists()
        && dir.join(format!("{name}_bindings.rs")).exists()
}

/// Writes a program's object and bindings, then its key (last, so a half-written entry is stale).
fn store(dir: &Path, name: &str, k: u64, out: &platypus_ts::Output) {
    std::fs::write(dir.join(format!("{name}.obj")), &out.object).unwrap();
    std::fs::write(dir.join(format!("{name}_bindings.rs")), &out.bindings).unwrap();
    std::fs::write(dir.join(format!("{name}.key")), format!("{k:016x}")).unwrap();
}
