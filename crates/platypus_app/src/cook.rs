// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Cooking (`platypus-app --cook [game]`): textures and models prepared ahead of time, so games
//! load them without decoding images, making mip levels or parsing glTF.
//!
//! - Textures: every mip level made, then block-compressed where the size allows (BC1
//!   for opaque or cut-out images, BC3 for smooth alpha), else kept as RGBA8.
//! - Models: their meshes (ready to upload) and textures (cooked the same way) with
//!   the description scripts get.
//!
//! Cooked files go to `.platypus/cooked/<game path>.tex` / `.model`. At run time a cooked file
//! newer than its source replaces it (a texture only when its colour space and mipmaps
//! match what was asked for). GPUs without BC get the blocks decoded first.

use std::path::{Path, PathBuf};

use platypus_render::{
    CookedTexture, MeshData, TexelFormat, TextureData, TextureFilter, TextureWrap,
};
use serde_json::Value;

use crate::models::{EngineIds, Model};

// ---- BC1 / BC3 blocks ----

fn to_565(c: [f32; 3]) -> u16 {
    let r = (c[0].clamp(0.0, 255.0) * 31.0 / 255.0).round() as u16;
    let g = (c[1].clamp(0.0, 255.0) * 63.0 / 255.0).round() as u16;
    let b = (c[2].clamp(0.0, 255.0) * 31.0 / 255.0).round() as u16;
    (r << 11) | (g << 5) | b
}

fn from_565(c: u16) -> [f32; 3] {
    let r = f32::from((c >> 11) & 31) * 255.0 / 31.0;
    let g = f32::from((c >> 5) & 63) * 255.0 / 63.0;
    let b = f32::from(c & 31) * 255.0 / 31.0;
    [r, g, b]
}

/// A colour block's four palette entries (`three`: the 3-colour mode, the last
/// transparent black).
fn palette(c0: u16, c1: u16, three: bool) -> [[f32; 4]; 4] {
    let (a, b) = (from_565(c0), from_565(c1));
    let mix = |t: f32| {
        let m = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
        [m[0], m[1], m[2], 255.0]
    };
    let [ar, ag, ab] = a;
    let [br, bg, bb] = b;
    if three {
        [[ar, ag, ab, 255.0], [br, bg, bb, 255.0], mix(0.5), [0.0; 4]]
    } else {
        [
            [ar, ag, ab, 255.0],
            [br, bg, bb, 255.0],
            mix(1.0 / 3.0),
            mix(2.0 / 3.0),
        ]
    }
}

/// One 4 × 4 colour block: endpoints from a diagonal of the colours' bounding box (the
/// one that fits best, inset a little), each texel the nearest palette entry. `cutout`:
/// texels with alpha under half become the transparent entry (BC1's 3-colour mode).
fn encode_color(texels: &[[u8; 4]; 16], cutout: bool) -> [u8; 8] {
    let opaque: Vec<[f32; 3]> = texels
        .iter()
        .filter(|t| !cutout || t[3] >= 128)
        .map(|t| [f32::from(t[0]), f32::from(t[1]), f32::from(t[2])])
        .collect();
    let (mut lo, mut hi) = ([255.0f32; 3], [0.0f32; 3]);
    for c in &opaque {
        for k in 0..3 {
            lo[k] = lo[k].min(c[k]);
            hi[k] = hi[k].max(c[k]);
        }
    }
    if opaque.is_empty() {
        (lo, hi) = ([0.0; 3], [0.0; 3]);
    }
    let inset = [0, 1, 2].map(|k| (hi[k] - lo[k]) / 16.0);
    let (hi, lo) = (
        [0, 1, 2].map(|k| hi[k] - inset[k]),
        [0, 1, 2].map(|k| lo[k] + inset[k]),
    );
    // Each of the box's four diagonals (green and blue running with red or against it).
    let mut best: Option<(f32, [u8; 8])> = None;
    for (flip_g, flip_b) in [(false, false), (true, false), (false, true), (true, true)] {
        let pick = |high: bool, k: usize| {
            let flip = (k == 1 && flip_g) || (k == 2 && flip_b);
            if high != flip { hi[k] } else { lo[k] }
        };
        let block = encode_color_with(
            texels,
            cutout,
            to_565([0, 1, 2].map(|k| pick(true, k))),
            to_565([0, 1, 2].map(|k| pick(false, k))),
        );
        let error = block_error(texels, &block, cutout);
        if best.is_none_or(|(e, _)| error < e) {
            best = Some((error, block));
        }
    }
    best.expect("four tries").1
}

/// Squared colour error of a block against its texels (holes ignored).
fn block_error(texels: &[[u8; 4]; 16], block: &[u8; 8], cutout: bool) -> f32 {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let pal = palette(c0, c1, cutout);
    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    texels
        .iter()
        .enumerate()
        .filter(|(_, t)| !cutout || t[3] >= 128)
        .map(|(i, t)| {
            let p = pal[((indices >> (i * 2)) & 3) as usize];
            (0..3)
                .map(|k| (p[k] - f32::from(t[k])).powi(2))
                .sum::<f32>()
        })
        .sum()
}

/// A colour block with these endpoints (put in the order the mode needs).
fn encode_color_with(texels: &[[u8; 4]; 16], cutout: bool, mut c0: u16, mut c1: u16) -> [u8; 8] {
    // 4-colour mode needs c0 > c1; the 3-colour (cut-out) mode c0 <= c1.
    if cutout {
        if c0 > c1 {
            std::mem::swap(&mut c0, &mut c1);
        }
    } else if c0 < c1 {
        std::mem::swap(&mut c0, &mut c1);
    }
    let pal = palette(c0, c1, cutout);
    let mut indices = 0u32;
    for (i, t) in texels.iter().enumerate() {
        let index = if cutout && t[3] < 128 {
            3
        } else if c0 == c1 && !cutout {
            0
        } else {
            let entries = if cutout { 3 } else { 4 };
            (0..entries)
                .min_by(|&a, &b| {
                    let d = |p: [f32; 4]| {
                        (0..3)
                            .map(|k| (p[k] - f32::from(t[k])).powi(2))
                            .sum::<f32>()
                    };
                    d(pal[a]).total_cmp(&d(pal[b]))
                })
                .unwrap_or(0) as u32
        };
        indices |= index << (i * 2);
    }
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    out[4..8].copy_from_slice(&indices.to_le_bytes());
    out
}

fn alpha_palette(a0: u8, a1: u8) -> [f32; 8] {
    let (a, b) = (f32::from(a0), f32::from(a1));
    let mut p = [a, b, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    if a0 > a1 {
        for i in 1..7 {
            p[i + 1] = (a * (7 - i) as f32 + b * i as f32) / 7.0;
        }
    } else {
        for i in 1..5 {
            p[i + 1] = (a * (5 - i) as f32 + b * i as f32) / 5.0;
        }
        p[6] = 0.0;
        p[7] = 255.0;
    }
    p
}

/// A BC3 alpha block: the extremes as endpoints (8-value mode), 3-bit indices.
fn encode_alpha(texels: &[[u8; 4]; 16]) -> [u8; 8] {
    let a0 = texels.iter().map(|t| t[3]).max().unwrap_or(255);
    let a1 = texels.iter().map(|t| t[3]).min().unwrap_or(255);
    let (a0, a1) = if a0 == a1 {
        (a0, a1)
    } else {
        (a0.max(a1), a0.min(a1))
    };
    let pal = alpha_palette(a0, a1);
    let mut bits = 0u64;
    for (i, t) in texels.iter().enumerate() {
        let index = if a0 == a1 {
            0
        } else {
            (0..8)
                .min_by(|&x, &y| {
                    (pal[x] - f32::from(t[3]))
                        .abs()
                        .total_cmp(&(pal[y] - f32::from(t[3])).abs())
                })
                .unwrap_or(0) as u64
        };
        bits |= index << (i * 3);
    }
    let mut out = [0u8; 8];
    out[0] = a0;
    out[1] = a1;
    out[2..8].copy_from_slice(&bits.to_le_bytes()[0..6]);
    out
}

/// The 4 × 4 texels of the block at (`bx`, `by`), edges repeated past the image.
fn block(rgba: &[u8], w: u32, h: u32, bx: u32, by: u32) -> [[u8; 4]; 16] {
    let mut out = [[0u8; 4]; 16];
    for y in 0..4 {
        for x in 0..4 {
            let (sx, sy) = ((bx * 4 + x).min(w - 1), (by * 4 + y).min(h - 1));
            let i = (sy as usize * w as usize + sx as usize) * 4;
            out[(y * 4 + x) as usize].copy_from_slice(&rgba[i..i + 4]);
        }
    }
    out
}

/// Compresses an RGBA8 image.
pub fn encode(rgba: &[u8], w: u32, h: u32, format: TexelFormat) -> Vec<u8> {
    if format == TexelFormat::Rgba8 {
        return rgba.to_vec();
    }
    let mut out = Vec::with_capacity(format.level_bytes(w, h));
    for by in 0..h.div_ceil(4) {
        for bx in 0..w.div_ceil(4) {
            let texels = block(rgba, w, h, bx, by);
            match format {
                TexelFormat::Bc1 => {
                    let cutout = texels.iter().any(|t| t[3] < 128);
                    out.extend(encode_color(&texels, cutout));
                }
                _ => {
                    out.extend(encode_alpha(&texels));
                    out.extend(encode_color(&texels, false));
                }
            }
        }
    }
    out
}

/// Decompresses a level back to RGBA8 (GPUs without BC, and tests).
pub fn decode(data: &[u8], w: u32, h: u32, format: TexelFormat) -> Vec<u8> {
    if format == TexelFormat::Rgba8 {
        return data.to_vec();
    }
    let mut out = vec![0u8; w as usize * h as usize * 4];
    let size = format.block_bytes() as usize;
    for (n, chunk) in data.chunks(size).enumerate() {
        let (bx, by) = (n as u32 % w.div_ceil(4), n as u32 / w.div_ceil(4));
        let (alpha, color) = if format == TexelFormat::Bc3 {
            (Some(&chunk[0..8]), &chunk[8..16])
        } else {
            (None, &chunk[0..8])
        };
        let c0 = u16::from_le_bytes([color[0], color[1]]);
        let c1 = u16::from_le_bytes([color[2], color[3]]);
        let pal = palette(c0, c1, format == TexelFormat::Bc1 && c0 <= c1);
        let indices = u32::from_le_bytes([color[4], color[5], color[6], color[7]]);
        let alphas = alpha.map(|a| {
            let mut bits = [0u8; 8];
            bits[0..6].copy_from_slice(&a[2..8]);
            (alpha_palette(a[0], a[1]), u64::from_le_bytes(bits))
        });
        for i in 0..16u32 {
            let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
            if x >= w || y >= h {
                continue;
            }
            let p = pal[((indices >> (i * 2)) & 3) as usize];
            let a = alphas.map_or(p[3], |(pal, bits)| pal[((bits >> (i * 3)) & 7) as usize]);
            let o = (y as usize * w as usize + x as usize) * 4;
            out[o..o + 4].copy_from_slice(&[p[0], p[1], p[2], a].map(|v| v.round() as u8));
        }
    }
    out
}

/// A texture cooked: its mips, compressed when its size is whole blocks.
pub fn cook_texture(data: &TextureData) -> CookedTexture {
    let alpha: Vec<u8> = data.rgba.chunks(4).map(|t| t[3]).collect();
    let format = if !data.width.is_multiple_of(4) || !data.height.is_multiple_of(4) {
        TexelFormat::Rgba8
    } else if alpha.iter().all(|&a| a == 255 || a == 0) {
        TexelFormat::Bc1
    } else {
        TexelFormat::Bc3
    };
    let mips = platypus_render::mip_chain(data)
        .into_iter()
        .map(|(px, w, h)| encode(&px, w, h, format))
        .collect();
    CookedTexture {
        width: data.width,
        height: data.height,
        format,
        srgb: data.srgb,
        mips,
        filter: data.filter,
        wrap: data.wrap,
        anisotropy: data.anisotropy,
    }
}

/// A cooked texture's first level as plain RGBA8 (mips are made again on upload).
pub fn uncook_texture(c: &CookedTexture) -> TextureData {
    TextureData {
        width: c.width,
        height: c.height,
        rgba: decode(&c.mips[0], c.width, c.height, c.format),
        filter: c.filter,
        wrap: c.wrap,
        mipmaps: c.mips.len() > 1,
        anisotropy: c.anisotropy,
        srgb: c.srgb,
    }
}

// ---- Files ----

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend(v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend(b);
    }
    fn floats(&mut self, f: &[f32]) {
        self.u32(f.len() as u32);
        for v in f {
            self.0.extend(v.to_le_bytes());
        }
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.u32()? as usize;
        Some(self.take(n)?.to_vec())
    }
    fn floats(&mut self) -> Option<Vec<f32>> {
        let n = self.u32()? as usize;
        let raw = self.take(n.checked_mul(4)?)?;
        Some(
            raw.as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect(),
        )
    }
}

const TEXTURE_MAGIC: &[u8; 4] = b"IRTX";
const MODEL_MAGIC: &[u8; 4] = b"IRMD";
const VERSION: u32 = 2; // 2: transparent pixels carry their neighbours' colour

fn write_texture(w: &mut Writer, t: &CookedTexture) {
    w.0.extend(TEXTURE_MAGIC);
    w.u32(VERSION);
    w.u32(t.width);
    w.u32(t.height);
    w.u8(match t.format {
        TexelFormat::Rgba8 => 0,
        TexelFormat::Bc1 => 1,
        TexelFormat::Bc3 => 2,
    });
    w.u8(u8::from(t.srgb));
    w.u8(match t.filter {
        TextureFilter::Point => 0,
        TextureFilter::Bilinear => 1,
        TextureFilter::Trilinear => 2,
    });
    w.u8(match t.wrap {
        TextureWrap::Repeat => 0,
        TextureWrap::Clamp => 1,
        TextureWrap::Mirror => 2,
    });
    w.u16(t.anisotropy);
    w.u16(t.mips.len() as u16);
    for m in &t.mips {
        w.bytes(m);
    }
}

fn read_texture(r: &mut Reader<'_>) -> Option<CookedTexture> {
    if r.take(4)? != TEXTURE_MAGIC || r.u32()? != VERSION {
        return None;
    }
    let (width, height) = (r.u32()?, r.u32()?);
    let format = match r.u8()? {
        0 => TexelFormat::Rgba8,
        1 => TexelFormat::Bc1,
        2 => TexelFormat::Bc3,
        _ => return None,
    };
    let srgb = r.u8()? != 0;
    let filter = match r.u8()? {
        0 => TextureFilter::Point,
        1 => TextureFilter::Bilinear,
        _ => TextureFilter::Trilinear,
    };
    let wrap = match r.u8()? {
        0 => TextureWrap::Repeat,
        1 => TextureWrap::Clamp,
        _ => TextureWrap::Mirror,
    };
    let anisotropy = r.u16()?;
    let count = r.u16()?;
    let mips = (0..count).map(|_| r.bytes()).collect::<Option<Vec<_>>>()?;
    let t = CookedTexture {
        width,
        height,
        format,
        srgb,
        mips,
        filter,
        wrap,
        anisotropy,
    };
    t.is_valid().then_some(t)
}

pub fn texture_bytes(t: &CookedTexture) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    write_texture(&mut w, t);
    w.0
}

pub fn texture_from_bytes(bytes: &[u8]) -> Option<CookedTexture> {
    read_texture(&mut Reader(bytes))
}

fn write_mesh(w: &mut Writer, m: &MeshData) {
    w.floats(m.positions.as_flattened());
    w.floats(m.normals.as_flattened());
    w.floats(m.uvs.as_flattened());
    w.floats(m.tangents.as_flattened());
    let joints: Vec<f32> = m.joints.iter().flatten().map(|&j| f32::from(j)).collect();
    w.floats(&joints);
    w.floats(m.weights.as_flattened());
    w.u32(m.indices.len() as u32);
    for i in &m.indices {
        w.u32(*i);
    }
}

fn read_mesh(r: &mut Reader<'_>) -> Option<MeshData> {
    fn arrays<const N: usize>(f: Vec<f32>) -> Vec<[f32; N]> {
        f.as_chunks::<N>().0.to_vec()
    }
    let positions = arrays::<3>(r.floats()?);
    let normals = arrays::<3>(r.floats()?);
    let uvs = arrays::<2>(r.floats()?);
    let tangents = arrays::<4>(r.floats()?);
    let joints = arrays::<4>(r.floats()?)
        .into_iter()
        .map(|j| j.map(|v| v as u16))
        .collect();
    let weights = arrays::<4>(r.floats()?);
    let count = r.u32()? as usize;
    let indices = (0..count).map(|_| r.u32()).collect::<Option<Vec<_>>>()?;
    let m = MeshData {
        positions,
        normals,
        uvs,
        tangents,
        joints,
        weights,
        indices,
    };
    m.is_valid().then_some(m)
}

/// A model as cooked bytes: its description (engine ids as they were) and the ids in the
/// order they were handed out, then meshes and textures.
pub fn model_bytes(model: &Model, ids: &[u32]) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    w.0.extend(MODEL_MAGIC);
    w.u32(VERSION);
    let head = serde_json::json!({ "description": model.description, "ids": ids });
    w.bytes(head.to_string().as_bytes());
    w.u32(model.meshes.len() as u32);
    for (id, m) in &model.meshes {
        w.u32(*id);
        write_mesh(&mut w, m);
    }
    w.u32(model.textures.len() as u32);
    for (id, t) in &model.textures {
        w.u32(*id);
        write_texture(&mut w, &cook_texture(t));
    }
    w.0
}

/// A cooked model's parts under fresh ids from `ids` (a reload's old ones first), and
/// its description with the ids changed to match.
pub fn model_from_bytes(bytes: &[u8], ids: &mut EngineIds) -> Option<Model> {
    let mut r = Reader(bytes);
    if r.take(4)? != MODEL_MAGIC || r.u32()? != VERSION {
        return None;
    }
    let head: Value = serde_json::from_slice(&r.bytes()?).ok()?;
    let old: Vec<u32> = head["ids"]
        .as_array()?
        .iter()
        .filter_map(|v| v.as_u64().map(|v| v as u32))
        .collect();
    let map: std::collections::HashMap<u32, u32> =
        old.iter().map(|&o| (o, ids.next_id())).collect();
    let mut description = head["description"].clone();
    renumber(&mut description, &map);
    let mut meshes = Vec::new();
    for _ in 0..r.u32()? {
        let id = r.u32()?;
        meshes.push((*map.get(&id)?, read_mesh(&mut r)?));
    }
    let mut cooked_textures = Vec::new();
    for _ in 0..r.u32()? {
        let id = r.u32()?;
        cooked_textures.push((*map.get(&id)?, read_texture(&mut r)?));
    }
    Some(Model {
        meshes,
        textures: Vec::new(),
        cooked_textures,
        description,
    })
}

/// Engine ids in a description (numbers from 2^30 up, nothing else is that big) mapped
/// to new ones.
fn renumber(v: &mut Value, map: &std::collections::HashMap<u32, u32>) {
    match v {
        Value::Number(n) => {
            if let Some(new) = n.as_u64().and_then(|x| map.get(&(x as u32))) {
                *v = Value::from(*new);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| renumber(x, map)),
        Value::Object(o) => o.values_mut().for_each(|x| renumber(x, map)),
        _ => {}
    }
}

/// Where a game file's cooked form goes.
pub fn cooked_path(root: &Path, src: &str, extension: &str) -> PathBuf {
    root.join(".platypus/cooked")
        .join(format!("{}.{extension}", crate::reload::asset_key(src)))
}

/// The cooked file for `src` if it's newer than the source and its `.meta` (importer settings
/// such as a model's `scaleFactor` change what is cooked).
fn fresh(root: &Path, src: &str, extension: &str) -> Option<Vec<u8>> {
    let source = platypus_core::resolve_asset(root, src)?;
    let cooked = cooked_path(root, src, extension);
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let made = modified(&cooked)?;
    if made < modified(&source)? {
        return None;
    }
    let mut meta = source.clone().into_os_string();
    meta.push(".meta");
    if modified(Path::new(&meta)).is_some_and(|m| made < m) {
        return None;
    }
    std::fs::read(cooked).ok()
}

/// A cooked texture for `src` when there's a fresh one with the colour space and
/// mipmaps asked for.
pub fn cooked_texture(root: &Path, src: &str, srgb: bool, mipmaps: bool) -> Option<CookedTexture> {
    let t = texture_from_bytes(&fresh(root, src, "tex")?)?;
    (t.srgb == srgb && (t.mips.len() > 1) == mipmaps).then_some(t)
}

/// A cooked model for `src` when there's a fresh one.
pub fn cooked_model(root: &Path, src: &str, ids: &mut EngineIds) -> Option<Model> {
    model_from_bytes(&fresh(root, src, "model")?, ids)
}

// ---- Cooking a game ----

/// What a cook did.
#[derive(Debug, Default, PartialEq)]
pub struct CookReport {
    pub textures: usize,
    pub models: usize,
    pub failed: Vec<String>,
}

/// Cooks every texture (PNG, JPEG) and model (glTF) in a game folder, with the import
/// settings from their `.meta` files.
pub fn cook_game(root: &Path) -> CookReport {
    let importers: std::collections::HashMap<String, Value> = crate::asset_db::scan(root)
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| Some((e["path"].as_str()?.to_owned(), e["importer"].clone())))
                .collect()
        })
        .unwrap_or_default();
    let mut report = CookReport::default();
    let mut files = Vec::new();
    collect(root, root, &mut files);
    for src in files {
        let ext = src.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let importer = importers.get(&src).cloned().unwrap_or(Value::Null);
        let result = match ext.as_str() {
            "png" | "jpg" | "jpeg" => {
                cook_texture_file(root, &src, &importer).map(|_| report.textures += 1)
            }
            "glb" | "gltf" | "fbx" => cook_model_file(root, &src).map(|_| report.models += 1),
            _ => continue,
        };
        if let Err(e) = result {
            report.failed.push(format!("{src}: {e}"));
        }
    }
    report
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if !matches!(name.as_str(), "build" | "node_modules" | "src" | "target") {
                collect(root, &path, out);
            }
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(crate::reload::asset_key(&rel.to_string_lossy()));
        }
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

fn cook_texture_file(root: &Path, src: &str, importer: &Value) -> Result<(), String> {
    let path = platypus_core::resolve_asset(root, src).ok_or("outside the game folder")?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let mut image = image::load_from_memory(&bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (width, height) = image.dimensions();
    platypus_core::bleed_transparent(&mut image, width as usize, height as usize, 8);
    let s = |k: &str| importer.get(k).and_then(Value::as_str);
    let data = TextureData {
        width,
        height,
        rgba: image.into_raw(),
        filter: s("filterMode")
            .and_then(TextureFilter::parse)
            .unwrap_or_default(),
        wrap: s("wrapMode")
            .and_then(TextureWrap::parse)
            .unwrap_or_default(),
        mipmaps: importer
            .get("mipmaps")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        anisotropy: importer
            .get("anisoLevel")
            .and_then(Value::as_u64)
            .unwrap_or(1) as u16,
        srgb: !importer
            .get("linear")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    write(
        &cooked_path(root, src, "tex"),
        &texture_bytes(&cook_texture(&data)),
    )
}

fn cook_model_file(root: &Path, src: &str) -> Result<(), String> {
    let mut ids = EngineIds::default();
    ids.record(&[]);
    let model = crate::models::load(root, src, &mut ids)?;
    let order = ids.take_recorded();
    write(
        &cooked_path(root, src, "model"),
        &model_bytes(&model, &order),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smooth gradient with a sharp-edged hole: BC's typical worst case.
    fn image(w: u32, h: u32, alpha: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend([(x * 255 / w) as u8, (y * 255 / h) as u8, 128, alpha(x, y)]);
            }
        }
        px
    }

    fn error(a: &[u8], b: &[u8]) -> f32 {
        let total: u64 = a
            .iter()
            .zip(b)
            .map(|(x, y)| u64::from(x.abs_diff(*y)))
            .sum();
        total as f32 / a.len() as f32
    }

    // M5-AST-06
    #[test]
    fn bc_blocks_round_trip_closely_and_keep_cutouts_and_alpha() {
        let opaque = image(16, 16, |_, _| 255);
        let bc1 = encode(&opaque, 16, 16, TexelFormat::Bc1);
        assert_eq!(bc1.len(), TexelFormat::Bc1.level_bytes(16, 16));
        // Red and green varying separately is BC1's hard case; it stays close enough.
        let back = decode(&bc1, 16, 16, TexelFormat::Bc1);
        assert!(
            error(&opaque, &back) < 7.0,
            "bc1 error {}",
            error(&opaque, &back)
        );
        // A gradient in one direction (with colours running against each other) is close.
        let mut ramp = Vec::new();
        for _y in 0..8 {
            for x in 0..16u32 {
                ramp.extend([(x * 16) as u8, (255 - x * 16) as u8, 60, 255]);
            }
        }
        let back = decode(
            &encode(&ramp, 16, 8, TexelFormat::Bc1),
            16,
            8,
            TexelFormat::Bc1,
        );
        assert!(
            error(&ramp, &back) < 2.5,
            "ramp error {}",
            error(&ramp, &back)
        );
        // A cut-out: transparent texels stay transparent (BC1's 3-colour mode).
        let holes = image(8, 8, |x, y| if (x + y) % 3 == 0 { 0 } else { 255 });
        let back = decode(
            &encode(&holes, 8, 8, TexelFormat::Bc1),
            8,
            8,
            TexelFormat::Bc1,
        );
        for (a, b) in holes.chunks(4).zip(back.chunks(4)) {
            assert_eq!(a[3] == 0, b[3] == 0);
        }
        // Smooth alpha: BC3's interpolated alpha.
        let fade = image(8, 8, |x, _| (x * 32) as u8);
        let back = decode(
            &encode(&fade, 8, 8, TexelFormat::Bc3),
            8,
            8,
            TexelFormat::Bc3,
        );
        let alpha_err = fade
            .chunks(4)
            .zip(back.chunks(4))
            .map(|(a, b)| a[3].abs_diff(b[3]))
            .max()
            .unwrap();
        assert!(alpha_err <= 12, "alpha error {alpha_err}");
        // Odd sizes decode within their bounds.
        let odd = image(5, 3, |_, _| 255);
        assert_eq!(
            decode(
                &encode(&odd, 5, 3, TexelFormat::Bc1),
                5,
                3,
                TexelFormat::Bc1
            )
            .len(),
            60
        );
    }

    // M5-AST-06
    #[test]
    fn cooked_textures_and_models_load_back_and_go_stale_with_their_sources() {
        let data = TextureData {
            width: 16,
            height: 8,
            rgba: image(16, 8, |_, _| 255),
            filter: TextureFilter::Trilinear,
            wrap: TextureWrap::Clamp,
            mipmaps: true,
            anisotropy: 4,
            srgb: true,
        };
        let cooked = cook_texture(&data);
        assert_eq!(cooked.format, TexelFormat::Bc1);
        assert_eq!(cooked.mips.len(), 5);
        assert!(cooked.is_valid());
        assert_eq!(
            texture_from_bytes(&texture_bytes(&cooked)),
            Some(cooked.clone())
        );
        // Not whole blocks: kept as RGBA8.
        let odd = TextureData {
            width: 6,
            rgba: image(6, 8, |_, _| 255),
            ..data.clone()
        };
        assert_eq!(cook_texture(&odd).format, TexelFormat::Rgba8);
        assert_eq!(uncook_texture(&cook_texture(&odd)).rgba, odd.rgba);

        // A game folder: cook it, then the cooked forms load (and stale ones don't).
        let dir = std::env::temp_dir().join(format!("platypus-cook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        let mut png = Vec::new();
        image::RgbaImage::from_raw(16, 8, data.rgba.clone())
            .unwrap()
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(dir.join("assets/a.png"), &png).unwrap();
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/assets");
        std::fs::copy(fixtures.join("house.glb"), dir.join("assets/house.glb")).unwrap();
        let report = cook_game(&dir);
        assert_eq!(
            (report.textures, report.models),
            (1, 1),
            "{:?}",
            report.failed
        );
        let t = cooked_texture(&dir, "assets/a.png", true, true).unwrap();
        assert_eq!((t.width, t.height, t.format), (16, 8, TexelFormat::Bc1));
        // A linear request doesn't take the sRGB cook.
        assert!(cooked_texture(&dir, "assets/a.png", false, true).is_none());
        // The model: same parts as loading the glTF, its description pointing at the new ids.
        let mut ids = EngineIds::default();
        let direct =
            crate::models::load(&dir, "assets/house.glb", &mut EngineIds::default()).unwrap();
        ids.next_id();
        let m = cooked_model(&dir, "assets/house.glb", &mut ids).unwrap();
        assert_eq!(m.meshes.len(), direct.meshes.len());
        assert_eq!(m.cooked_textures.len(), direct.textures.len());
        let first = m.meshes[0].0;
        assert_eq!(
            m.description["nodes"][1]["primitives"][0]["mesh"],
            serde_json::json!(m.meshes[1].0)
        );
        assert!(
            first > crate::models::ENGINE_IDS + 1,
            "fresh ids, not the cooked ones"
        );
        assert_eq!(m.meshes[0].1, direct.meshes[0].1);
        // Touching the source makes the cook stale.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("assets/a.png"), &png).unwrap();
        assert!(cooked_texture(&dir, "assets/a.png", true, true).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
