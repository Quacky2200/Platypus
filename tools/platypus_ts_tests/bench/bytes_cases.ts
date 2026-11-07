// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Byte and text APIs for tests/lang/byteapis.rs: instanceof on typed arrays, TextEncoder /
// TextDecoder, atob / btoa, Uint8Array.from / of / base64 / hex, Buffer and Blob. Each case (split
// at `//## name`) defines `function t(): string`; the test compiles it with web_ui_ts (normal,
// GC-stress, GC-off) and compares it with bun's (`bun bench/gen_cases_expected.ts bytes_cases.ts`).
//## instanceof_on_typed_and_dynamic_values
function t(): string {
  const u8 = new Uint8Array(2);
  const f32 = new Float32Array(2);
  const buf = Buffer.alloc(2);
  const anys: any[] = [u8, f32, buf, "s", 5, [1], new Int16Array(1), u8.subarray(1)];
  const out: string[] = [];
  for (let i = 0; i < anys.length; i++) {
    const v = anys[i];
    out.push(
      [
        v instanceof Uint8Array,
        v instanceof Float32Array,
        v instanceof Int16Array,
        v instanceof Uint16Array,
        Buffer.isBuffer(v),
      ]
        .map((b) => (b ? "1" : "0"))
        .join(""),
    );
  }
  out.push(String(u8 instanceof Uint8Array) + String(u8 instanceof Float32Array) + String(buf instanceof Uint8Array));
  const narrowed: any = f32;
  if (narrowed instanceof Float32Array) out.push("len" + narrowed.length);
  return out.join(",");
}
//## text_encoder_encode_and_into
function t(): string {
  const enc = new TextEncoder();
  const a = enc.encode("héllo €😀");
  const lone = enc.encode("a\ud800b");
  const dest = new Uint8Array(6);
  const r = enc.encodeInto("h€llo", dest);
  const dest2 = new Uint8Array(3);
  const r2 = enc.encodeInto("a😀", dest2);
  return `${enc.encoding}|${Array.from(a).join(" ")}|${Array.from(lone).join(" ")}|${r.read}|${r.written}|${Array.from(dest).join(" ")}|${r2.read}|${r2.written}|${enc.encode("").length}`;
}
//## text_decoder_basics
function t(): string {
  const dec = new TextDecoder();
  const bytes = new TextEncoder().encode("héllo €😀");
  const withBom = new Uint8Array([0xef, 0xbb, 0xbf, 0x68, 0x69]);
  const bad = new Uint8Array([0x68, 0xff, 0x69, 0xe2, 0x82]);
  const keep = new TextDecoder("utf-8", { ignoreBOM: true });
  const u16 = new TextDecoder("utf-16le");
  const u16bytes = new Uint8Array([0x68, 0x00, 0x3d, 0xd8, 0x00, 0xde, 0x41]);
  const latin = new TextDecoder("latin1");
  const cp = new Uint8Array([0x80, 0xe9, 0x41]);
  const out = [
    dec.encoding,
    dec.decode(bytes),
    dec.decode(withBom),
    String(dec.decode(withBom).length),
    String(keep.decode(withBom).length),
    dec.decode(bad),
    u16.decode(u16bytes),
    u16.encoding,
    latin.encoding,
    latin.decode(cp),
    String(dec.decode().length),
  ];
  return out.map((s) => JSON.stringify(s)).join(",");
}
//## text_decoder_fatal_and_labels
function t(): string {
  const out: string[] = [];
  const fatal = new TextDecoder("utf-8", { fatal: true });
  try {
    fatal.decode(new Uint8Array([0x68, 0xff]));
    out.push("no throw");
  } catch (e) {
    out.push((e as Error).name);
  }
  out.push(fatal.decode(new Uint8Array([0x68, 0x69])));
  out.push(String(fatal.fatal) + String(new TextDecoder().fatal));
  try {
    new TextDecoder("nonsense");
    out.push("no throw");
  } catch (e) {
    out.push((e as Error).name);
  }
  out.push(new TextDecoder(" UTF8 ").encoding);
  return out.join(",");
}
//## text_decoder_streaming
function t(): string {
  const dec = new TextDecoder();
  const all = new TextEncoder().encode("a€😀b");
  const parts: string[] = [];
  for (let i = 0; i < all.length; i++) {
    parts.push(dec.decode(all.subarray(i, i + 1), { stream: true }));
  }
  parts.push(dec.decode());
  const bom = new TextDecoder();
  const b1 = bom.decode(new Uint8Array([0xef]), { stream: true });
  const b2 = bom.decode(new Uint8Array([0xbb, 0xbf, 0x41]));
  const cut = new TextDecoder();
  const c1 = cut.decode(new Uint8Array([0xe2, 0x82]), { stream: true });
  const c2 = cut.decode();
  const u = new TextDecoder("utf-16le");
  const u1 = u.decode(new Uint8Array([0x3d]), { stream: true });
  const u2 = u.decode(new Uint8Array([0xd8, 0x00]), { stream: true });
  const u3 = u.decode(new Uint8Array([0xde]));
  return [JSON.stringify(parts), JSON.stringify([b1, b2, c1, c2]), JSON.stringify([u1, u2, u3])].join("|");
}
//## atob_and_btoa
function t(): string {
  const out: string[] = [];
  out.push(btoa("hello"), btoa(""), btoa("a"), btoa("ab"), btoa("\xff\xfe"));
  out.push(atob("aGVsbG8="), atob("aGVsbG8"), atob(" aGVs bG8= "), atob(""), atob("YQ=="), atob("YQ"));
  out.push(Array.from(atob("//79")).map((c) => c.charCodeAt(0)).join(" "));
  const bad = ["a", "ab=c", "====", "a-b_", "Ā", "YQ==="];
  for (const b of bad) {
    try {
      out.push(atob(b));
    } catch (e) {
      out.push((e as Error).name);
    }
  }
  try {
    btoa("Ā");
  } catch (e) {
    out.push((e as Error).name);
  }
  return out.map((s) => JSON.stringify(s)).join(",");
}
//## typed_from_of_and_map
function t(): string {
  const a = Uint8Array.from([1, 2, 300]);
  const b = Uint8Array.of(7, 8, 9);
  const c = Uint8Array.from("abc", (ch) => ch.charCodeAt(0));
  const d = Float32Array.from([1.5, 2.5], (x) => x * 2);
  const e = Int16Array.from(new Uint8Array([5, 6]));
  const f = Uint8Array.from(atob("aGk="), (ch) => ch.charCodeAt(0));
  return [Array.from(a), Array.from(b), Array.from(c), Array.from(d), Array.from(e), Array.from(f)].map((x) => x.join(" ")).join("|");
}
//## uint8array_base64_and_hex
function t(): string {
  const out: string[] = [];
  const b = new Uint8Array([251, 255, 254, 1]);
  out.push(b.toBase64(), b.toBase64({ alphabet: "base64url" }), b.toBase64({ omitPadding: true }), b.toHex());
  out.push(Array.from(Uint8Array.fromBase64("+//+AQ==")).join(" "));
  out.push(Array.from(Uint8Array.fromBase64("-__-AQ", { alphabet: "base64url" })).join(" "));
  out.push(Array.from(Uint8Array.fromBase64(" YW Jj\n")).join(" "));
  out.push(Array.from(Uint8Array.fromBase64("YWI", { lastChunkHandling: "loose" })).join(" "));
  out.push(Array.from(Uint8Array.fromBase64("YWJjZA", { lastChunkHandling: "stop-before-partial" })).join(" "));
  out.push(Array.from(Uint8Array.fromHex("00ff7Fa0")).join(" "));
  for (const f of [
    () => Uint8Array.fromBase64("YWI", { lastChunkHandling: "strict" }),
    () => Uint8Array.fromBase64("YR==", { lastChunkHandling: "strict" }),
    () => Uint8Array.fromBase64("Y"),
    () => Uint8Array.fromBase64("YW=Jj"),
    () => Uint8Array.fromBase64("-_", { alphabet: "base64" }),
    () => Uint8Array.fromHex("abc"),
    () => Uint8Array.fromHex("zz"),
  ]) {
    try {
      f();
      out.push("ok");
    } catch (e) {
      out.push((e as Error).name);
    }
  }
  const target = new Uint8Array(4);
  const r = target.setFromBase64("YWJjZGVm");
  out.push(r.read + ":" + r.written + ":" + Array.from(target).join(" "));
  const h = new Uint8Array(3);
  const r2 = h.setFromHex("a1b2c3d4");
  out.push(r2.read + ":" + r2.written + ":" + h.toHex());
  return out.join(",");
}
//## buffer_encodings_round_trip
function t(): string {
  const out: string[] = [];
  const text = "héllo €😀 \0 end";
  for (const enc of ["utf8", "utf16le", "ucs2", "latin1", "binary", "ascii", "hex", "base64", "base64url"] as const) {
    const b = Buffer.from(text, enc);
    out.push(enc + ":" + b.length + ":" + b.toString("hex") + ":" + JSON.stringify(b.toString(enc)));
  }
  out.push(Buffer.from("zz", "hex").length + "," + Buffer.from("abc", "hex").length + "," + Buffer.from("abzz12", "hex").toString("hex"));
  out.push(Buffer.from("YW Jj ZA==", "base64").toString() + Buffer.from("YWJj_-8", "base64").toString("hex") + Buffer.from("YQ", "base64").toString());
  out.push(Buffer.from([0xef, 0xbb, 0xbf, 0x41]).toString().length + "," + Buffer.from([0x68, 0xff, 0xe2, 0x82]).toString().length);
  out.push(Buffer.from("héllo").toString("utf8", 1, 3) + "," + Buffer.from("hello").toString("utf8", -5, 99) + Buffer.from("hello").toString("latin1", 2));
  out.push(Buffer.byteLength("héllo €") + "," + Buffer.byteLength("héllo", "utf16le") + "," + Buffer.byteLength("abcd", "hex") + "," + Buffer.byteLength(Buffer.alloc(5)));
  out.push(String(Buffer.isEncoding("UTF-8")) + String(Buffer.isEncoding("nope")));
  out.push("" + Buffer.from("x\ud800y").toString("hex") + "," + Buffer.from([0xe9, 0x41]).toString("ascii"));
  return out.join("\n");
}
//## buffer_read_write_every_width
function t(): string {
  const out: string[] = [];
  const b = Buffer.alloc(16);
  b.writeUInt8(200, 0);
  b.writeInt8(-5, 1);
  b.writeUInt16LE(0xbeef, 2);
  b.writeUInt16BE(0xbeef, 4);
  b.writeInt16LE(-2, 6);
  b.writeInt16BE(-300, 8);
  b.writeUInt32LE(0xdeadbeef, 10);
  out.push(b.toString("hex"));
  out.push([b.readUInt8(0), b.readInt8(0), b.readInt8(1), b.readUInt16LE(2), b.readUInt16BE(2), b.readUInt16BE(4), b.readInt16LE(6), b.readInt16BE(8), b.readUInt32LE(10), b.readInt32LE(10), b.readUInt32BE(10), b.readInt32BE(10)].join(","));
  const c = Buffer.alloc(24);
  c.writeInt32LE(-123456, 0);
  c.writeInt32BE(-123456, 4);
  c.writeUInt32BE(4000000000, 8);
  c.writeFloatLE(1.5, 12);
  c.writeFloatBE(-2.25, 16);
  c.writeDoubleLE(Math.PI, 0);
  out.push(c.toString("hex"));
  out.push(`${c.readFloatLE(12)},${c.readFloatBE(16)},${c.readDoubleLE(0)},${c.readDoubleBE(0) > 0}`);
  const d = Buffer.alloc(8);
  d.writeBigUInt64LE(0x0102030405060708n);
  out.push(d.toString("hex") + "," + d.readBigUInt64LE() + "," + d.readBigUInt64BE() + "," + d.readBigInt64BE());
  d.writeBigInt64BE(-2n);
  out.push(d.toString("hex") + "," + d.readBigInt64BE() + "," + d.readBigUInt64BE() + "," + d.readBigInt64LE());
  const e = Buffer.alloc(8);
  e.writeUIntBE(0x123456789a, 0, 5);
  e.writeIntLE(-1000, 5, 3);
  out.push(e.toString("hex") + "," + e.readUIntBE(0, 5) + "," + e.readUIntLE(0, 6) + "," + e.readIntLE(5, 3) + "," + e.readIntBE(5, 3) + "," + e.readUintLE(0, 2));
  out.push(String(b.writeUInt16LE(1, 14)) + "," + String(b.writeDoubleBE(1, 8)));
  return out.join("\n");
}
//## buffer_read_write_errors
function t(): string {
  const out: string[] = [];
  const b = Buffer.alloc(4);
  const tries: [string, () => unknown][] = [
    ["read past end", () => b.readUInt32LE(1)],
    ["read at end", () => b.readUInt8(4)],
    ["read negative", () => b.readUInt8(-1)],
    ["read fraction", () => b.readUInt8(1.5)],
    ["read too short", () => Buffer.alloc(1).readUInt16LE(0)],
    ["write range 8", () => b.writeUInt8(256, 0)],
    ["write range neg", () => b.writeUInt8(-1, 0)],
    ["write int8", () => b.writeInt8(128, 0)],
    ["write u16", () => b.writeUInt16BE(70000, 0)],
    ["write i16", () => b.writeInt16LE(-32769, 0)],
    ["write u32", () => b.writeUInt32LE(4294967296, 0)],
    ["write i32", () => b.writeInt32LE(-2147483649, 0)],
    ["write big range", () => b.writeUInt32LE(12345678901234, 0)],
    ["write offset", () => b.writeUInt32LE(1, 1)],
    ["write float offset", () => b.writeFloatLE(1, 2)],
    ["bigint range", () => Buffer.alloc(8).writeBigUInt64LE(-1n)],
    ["bigint big", () => Buffer.alloc(8).writeBigInt64LE(2n ** 63n)],
    ["uint bytes", () => b.readUIntLE(0, 7)],
    ["wide range", () => Buffer.alloc(8).writeUIntBE(2 ** 40, 0, 5)],
    ["alloc neg", () => Buffer.alloc(-1)],
    ["encoding", () => Buffer.from("a", "nope" as BufferEncoding)],
    ["swap16", () => Buffer.alloc(3).swap16()],
  ];
  for (const [name, f] of tries) {
    try {
      f();
      out.push(name + ": ok");
    } catch (e) {
      // Node's largest size is not Bun's: only the error's name for that one.
      out.push(name + ": " + (e as Error).name + (name === "alloc neg" ? "" : ": " + (e as Error).message));
    }
  }
  return out.join("\n");
}
//## buffer_methods
function t(): string {
  const out: string[] = [];
  const a = Buffer.from("hello world");
  const b = Buffer.from("hello there");
  out.push(String(a.equals(b)) + a.equals(Buffer.from("hello world")) + "," + a.compare(b) + b.compare(a) + a.compare(a));
  out.push(Buffer.compare(a, b) + "," + Buffer.concat([a, Buffer.from("!")]).toString() + "," + Buffer.concat([a, b], 5).toString() + "," + Buffer.concat([a], 14).length + "," + Buffer.concat([]).length);
  out.push([a.indexOf("o"), a.indexOf("o", 5), a.indexOf("o", -3), a.lastIndexOf("o"), a.lastIndexOf("o", 5), a.indexOf("xyz"), a.indexOf(111), a.indexOf(Buffer.from("wor")), a.indexOf(""), a.lastIndexOf(""), a.indexOf("", 50)].join(",") + `,${a.includes("lo w")},${a.includes(0)}`);
  const f = Buffer.alloc(6);
  f.fill("ab");
  const g = Buffer.alloc(6, 7);
  const h = Buffer.alloc(5, "xyz");
  const i = Buffer.alloc(4);
  i.fill(0x1ff, 1, 3);
  const j = Buffer.alloc(5, Buffer.from([1, 2]));
  out.push([f.toString(), g.toString("hex"), h.toString(), i.toString("hex"), j.toString("hex")].join(","));
  const s = a.slice(2, 5);
  const sub = a.subarray(-5);
  s[0] = 0x4c;
  out.push(s.toString() + "," + sub.toString() + "," + a.toString() + "," + String(Buffer.isBuffer(s)) + "," + s.length + "," + s.byteOffset);
  const dst = Buffer.alloc(8, ".");
  const n = a.copy(dst, 2, 0, 4);
  const n2 = a.copy(dst, 6);
  out.push(n + "," + n2 + "," + dst.toString());
  out.push(JSON.stringify(Buffer.from([1, 2, 3])) + "," + JSON.stringify(Buffer.alloc(0)));
  const w = Buffer.alloc(6);
  out.push(w.write("héllo") + "," + w.toString("hex") + "," + w.write("€€€", 1) + "," + w.toString("hex") + "," + w.write("abc", 4, 1) + "," + w.write("ff", 0, "hex"));
  out.push(Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]).swap16().toString("hex") + "," + Buffer.from([1, 2, 3, 4]).swap32().toString("hex") + "," + Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]).swap64().toString("hex"));
  out.push(String(Buffer.from(a)) + "," + `${Buffer.from("tpl")}` + "," + (Buffer.from("x") + "y"));
  const ab = new ArrayBuffer(8);
  const viaAb = Buffer.from(ab, 2, 4);
  viaAb[0] = 9;
  out.push(new Uint8Array(ab)[2] + "," + viaAb.length + "," + Buffer.from(new Uint8Array([5, 6])).toString("hex") + "," + Buffer.from(new Uint16Array([0x101, 0x1ff])).toString("hex"));
  const u8: Uint8Array = Buffer.from("abc");
  out.push(u8.length + "," + u8[1] + "," + Buffer.from(u8.buffer, u8.byteOffset, u8.length).toString());
  out.push(a.readUInt8(1) + "," + a.length + "," + [...Buffer.from("ab")].join("."));
  return out.join("\n");
}
//## blob_parts_slice_and_reads
function t(): string {
  const b = new Blob(["héllo ", new Uint8Array([0x77, 0x6f]), new Blob(["rld"])], { type: "Text/Plain" });
  const s = b.slice(1, -2, "x/y");
  const neg = b.slice(-3);
  const empty = new Blob();
  const typed = new Blob([new Float32Array([1])]);
  const odd = new Blob(["a"], { type: "badĀ" });
  return `${b.size},${b.type},${s.size},${s.type},${neg.size},${empty.size},${empty.type},${typed.size},${odd.type}`;
}
//## blob_promises
function t(): string {
  const b = new Blob(["héllo ", new Uint8Array([0x77, 0x6f]), "rld"]);
  let result = "pending";
  b.text().then((s) => {
    result = s;
  });
  let size = 0;
  b.arrayBuffer().then((ab) => {
    size = ab.byteLength;
  });
  let first = 0;
  new Blob(["\u{fe}"]).arrayBuffer().then((ab) => {
    first = new Uint8Array(ab)[0];
  });
  let len = 0;
  b.bytes().then((u) => {
    len = u.length;
  });
  return "" + (result === "pending") + size + first + len;
}
//## buffers_kept_through_other_values
function mk(i: number): Buffer {
  return Buffer.from("item" + i);
}
function t(): string {
  const keep: any[] = [];
  const nested: Buffer[] = [];
  for (let i = 0; i < 200; i++) {
    const b = mk(i);
    nested.push(b);
    if (i % 3 === 0) keep.push(b.subarray(2));
    if (i % 5 === 0) keep.push(new Uint8Array(b.buffer, b.byteOffset, 2));
    new Array(50).fill(i);
  }
  let sum = 0;
  for (const k of keep) {
    if (k instanceof Uint8Array) sum += k.length * 7 + k[0];
  }
  const joined = Buffer.concat(nested).toString().length;
  return sum + "," + joined + "," + nested[199].toString();
}
//## encode_decode_loop_checksum
function t(): string {
  const enc = new TextEncoder();
  const dec = new TextDecoder();
  let sum = 0;
  for (let i = 0; i < 20000; i++) {
    const s = "n" + i + "é€";
    const bytes = enc.encode(s);
    const back = dec.decode(bytes);
    const buf = Buffer.from(back, "utf8");
    sum += bytes.length + back.length + buf.toString("base64").length + buf.toString("hex").charCodeAt(1);
  }
  return String(sum);
}
