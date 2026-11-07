// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Uint8Array` at the native boundary: a host function's `&[u8]` / `&mut [u8]` parameter reads
//! or writes the script array in place (a view passes its window), `Vec<u8>` is a copy (a result
//! is a new array), and `ts_api::Bytes` is an array the host keeps and hands back as it is. Each
//! program runs in the normal, stress GC and collection-off modes. IDs are in TESTS.md
//! (M9-TS-3611..).
use crate::cases::run_with;
use std::cell::RefCell;
use ts_api::ts_api;

thread_local! {
    static KEPT: RefCell<Vec<ts_api::Bytes>> = const { RefCell::new(Vec::new()) };
}

/// A host API over bytes.
#[ts_api(namespace = "ByteKit")]
pub mod byte_kit {
    use super::KEPT;

    /// The sum of the bytes.
    pub fn sum(b: &[u8]) -> f64 {
        b.iter().map(|&x| f64::from(x)).sum()
    }

    /// Adds `by` to every byte, in place.
    pub fn bump(b: &mut [u8], by: f64) {
        for x in b {
            *x = x.wrapping_add(by as u8);
        }
    }

    /// The bytes in reverse order, as a new array.
    pub fn reversed(b: &[u8]) -> Vec<u8> {
        b.iter().rev().copied().collect()
    }

    /// `n` bytes counting up from `from`.
    pub fn count(from: f64, n: f64) -> Vec<u8> {
        (0..n as u32).map(|i| (from as u32 + i) as u8).collect()
    }

    /// How many bytes it was given to own (none when left out).
    #[optional(b)]
    pub fn owned(b: Vec<u8>) -> f64 {
        b.len() as f64
    }

    /// Keeps `b` (the script may drop its own references); returns its slot.
    pub fn keep(b: ts_api::Bytes) -> f64 {
        KEPT.with(|k| {
            let mut k = k.borrow_mut();
            k.push(b);
            (k.len() - 1) as f64
        })
    }

    /// A kept array, the same object every time.
    pub fn kept(slot: f64) -> ts_api::Bytes {
        KEPT.with(|k| k.borrow()[slot as usize].clone())
    }

    /// A new kept array of `n` zeroed bytes; returns its slot.
    pub fn make(n: f64) -> f64 {
        keep(ts_api::Bytes::zeroed(n as usize))
    }

    /// Writes `v` at `i` of a kept array, from the host.
    pub fn poke(slot: f64, i: f64, v: f64) {
        KEPT.with(|k| {
            let b = k.borrow()[slot as usize].clone();
            // SAFETY: the script is not using the bytes during this call.
            let bytes = unsafe { b.as_mut_slice() };
            bytes[i as usize] = v as u8;
        })
    }

    /// The sum of a kept array, from the host.
    pub fn kept_sum(slot: f64) -> f64 {
        KEPT.with(|k| {
            let b = k.borrow()[slot as usize].clone();
            // SAFETY: the script is not writing the bytes during this call.
            sum(unsafe { b.as_slice() })
        })
    }

    /// Forgets every kept array.
    pub fn clear() {
        KEPT.with(|k| k.borrow_mut().clear());
    }
}

fn run(src: &str, prefix: &'static str, base: u32) -> String {
    let manifests = [byte_kit::MANIFEST.to_json()];
    let got = run_with(src, prefix, base, &manifests).unwrap_or_else(|e| panic!("{e}"));
    byte_kit::clear();
    got
}

// M9-TS-3611: `&[u8]` reads the script's bytes in place and `&mut [u8]` writes them; a
// `subarray` passes just its window; the host's writes are the script's.
#[test]
fn byte_parameters_read_and_write_the_script_array_in_place() {
    let got = run(
        r#"
function t(): string {
  const a = new Uint8Array([1, 2, 3, 250]);
  const out: string[] = [String(ByteKit.sum(a))];
  ByteKit.bump(a, 10);
  out.push(a.join(","));
  const w = a.subarray(1, 3);
  out.push(String(ByteKit.sum(w)));
  ByteKit.bump(w, 1);
  out.push(a.join(","));
  out.push(String(ByteKit.sum(new Uint8Array(0))));
  const view = new Uint8Array(new ArrayBuffer(8), 2, 3);
  view[0] = 7;
  out.push(String(ByteKit.sum(view)));
  return out.join("|");
}
"#,
        "by0_",
        3_700_000,
    );
    assert_eq!(got, "256|11,12,13,4|25|11,13,14,4|0|7");
}

// M9-TS-3612: a `Vec<u8>` result is a new `Uint8Array` (with the methods and indexing of one), a
// `Vec<u8>` parameter is a copy the function owns, and an optional one left out is empty.
#[test]
fn byte_results_are_new_arrays_and_owned_parameters_copies() {
    let got = run(
        r#"
function t(): string {
  const a = new Uint8Array([5, 6, 7]);
  const r = ByteKit.reversed(a);
  r[0] = 99;
  const c = ByteKit.count(254, 4);
  const empty = ByteKit.count(0, 0);
  return [a.join(","), r.join(","), String(r.length), c.join(","), String(empty.length),
    String(ByteKit.owned(a)), String(ByteKit.owned()), String(r.byteLength), c.slice(1).join(",")].join("|");
}
"#,
        "by1_",
        3_701_000,
    );
    assert_eq!(got, "5,6,7|99,6,5|3|254,255,0,1|0|3|0|3|255,0,1");
}

// M9-TS-3613: `ts_api::Bytes` keeps the script's array (or one the host made): handed back it is
// the same object, so writes on either side are seen by the other.
#[test]
fn kept_bytes_are_shared_between_host_and_script() {
    let got = run(
        r#"
function t(): string {
  const a = new Uint8Array([1, 2, 3]);
  const slot = ByteKit.keep(a);
  const back = ByteKit.kept(slot);
  const out: string[] = [String(back === a), String(ByteKit.kept(slot) === ByteKit.kept(slot))];
  ByteKit.poke(slot, 0, 40);
  out.push(a.join(","));
  a[2] = 9;
  out.push(String(ByteKit.keptSum(slot)));
  const m = ByteKit.make(4);
  const made = ByteKit.kept(m);
  made[3] = 200;
  ByteKit.poke(m, 0, 1);
  out.push(made.join(","), String(ByteKit.keptSum(m)));
  return out.join("|");
}
"#,
        "by2_",
        3_702_000,
    );
    assert_eq!(got, "true|true|40,2,3|51|1,0,0,200|201");
}

// M9-TS-3614: arrays kept only by the host survive many collections (stress mode collects at
// every allocation), and results made by the host in a loop stay correct; the checksums agree in
// every GC mode.
#[test]
fn bytes_survive_collections_when_only_the_host_keeps_them() {
    let got = run(
        r#"
function fill(n: number, seed: number): number {
  const a = new Uint8Array(n);
  for (let i = 0; i < n; i++) a[i] = (i * 7 + seed) & 255;
  return ByteKit.keep(a);
}
function t(): string {
  const slots: number[] = [];
  for (let k = 0; k < 20; k++) slots.push(fill(64 + k, k));
  let junk = 0;
  for (let i = 0; i < 2000; i++) {
    const tmp = ByteKit.count(i, 16);
    const back = ByteKit.reversed(tmp);
    junk += back[0] + ByteKit.sum(tmp);
    const s: string[] = ["x" + i];
    junk += s.length;
  }
  let check = 0;
  for (const s of slots) {
    const b = ByteKit.kept(s);
    check += ByteKit.keptSum(s) * 3 + b[b.length - 1];
  }
  return junk + " " + check;
}
"#,
        "by3_",
        3_703_000,
    );
    // Recomputed here.
    let mut junk = 0u64;
    for i in 0..2000u64 {
        let tmp: Vec<u64> = (0..16).map(|j| (i + j) & 255).collect();
        junk += tmp[15] + tmp.iter().sum::<u64>() + 1;
    }
    let mut check = 0u64;
    for k in 0..20u64 {
        let n = 64 + k;
        let a: Vec<u64> = (0..n).map(|i| (i * 7 + k) & 255).collect();
        check += a.iter().sum::<u64>() * 3 + a[n as usize - 1];
    }
    assert_eq!(got, format!("{junk} {check}"));
}

// M9-TS-3615: the declarations say `Uint8Array` for every kind of bytes parameter and result.
#[test]
fn byte_parameters_and_results_are_declared_uint8_array() {
    let dts = web_ui_ts::types_dts::generate(&[byte_kit::MANIFEST.to_json()]).unwrap();
    for want in [
        "sum(b: Uint8Array): number;",
        "bump(b: Uint8Array, by: number): void;",
        "reversed(b: Uint8Array): Uint8Array;",
        "owned(b?: Uint8Array): number;",
        "keep(b: Uint8Array): number;",
        "kept(slot: number): Uint8Array;",
    ] {
        assert!(dts.contains(want), "{want}\n{dts}");
    }
}
