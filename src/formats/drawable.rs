//! Drawables (models): LOD geometry decode and glTF / OBJ export.
//!
//! A drawable is a paged resource whose system segment starts with the
//! drawable header and whose graphics segment holds vertex and index data:
//!
//! - header: +0x08 page map, +0x10 shader group, +0x18 skeleton, +0x30/+0x40
//!   bounds, +0x50..+0x68 four LOD group pointers, +0xA8 name
//! - LOD group `{ptr models, u16 count, u16 capacity}`; model: +0x08 geometry
//!   pointers, +0x10 u16 count; geometry: +0x08 vertex buffer, +0x40 index descriptor
//! - vertex buffer: +0x08 u32 elements, +0x0C u16 stride, +0x18 data, +0x40 layout;
//!   layout: u32 offset per semantic, sizes at +0xD0, formats at +0x104,
//!   flags at +0x138 (`vertex count << 10 | 1`)
//! - vertex data starts with 16 u32 attribute descriptors (`format & 127`);
//!   position format 66 is 8-byte quantized, 99 is float3
//! - index data is clustered: per-cluster 7-bit triangle corners remapped
//!   through a u16/u32 global vertex table

use anyhow::{bail, ensure, Context, Result};
use std::fmt::Write as _;
use super::resource::{PHYSICAL_BASE, VIRTUAL_BASE};
use super::{u16_at, u32_at, u64_at};

pub const LOD_COUNT: usize = 4;
const HEADER: usize = 176;
const MAX_SYSTEM: u64 = 32 << 20;
const UV_SEMANTIC: usize = 28;

#[derive(Debug)]
pub struct Mesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub indices: Vec<u32>,
}

#[derive(Debug)]
pub struct Drawable {
    pub name: String,
    pub lod: usize,
    pub meshes: Vec<Mesh>,
    /// Geometries left out, with the reason (e.g. an unhandled vertex format).
    pub skipped: Vec<String>,
}

fn system_ptr(p: u64) -> bool { (VIRTUAL_BASE..VIRTUAL_BASE + MAX_SYSTEM).contains(&p) }
fn finite(v: [f32; 3]) -> bool { v.iter().all(|c| c.is_finite() && c.abs() < 1e7) }

fn vec3(d: &[u8], at: usize) -> Option<[f32; 3]> {
    Some([f32::from_bits(u32_at(d, at)?), f32::from_bits(u32_at(d, at + 4)?), f32::from_bits(u32_at(d, at + 8)?)])
}

/// Standalone drawable header check (drawable dictionaries and fragments wrap it differently).
pub fn looks_like(sys: &[u8]) -> bool {
    let ptr = |at| u64_at(sys, at).is_some_and(system_ptr);
    if sys.len() < HEADER || u64_at(sys, 0) != Some(0) || ![8, 16, 80, 168].into_iter().all(ptr) { return false; }
    let (Some(min), Some(max)) = (vec3(sys, 48), vec3(sys, 64)) else { return false; };
    let radius = f32::from_bits(u32_at(sys, 44).unwrap());
    finite(min) && finite(max) && (0..3).all(|i| min[i] <= max[i]) && (0.0..1e6).contains(&radius)
}

/// Decodes a drawable from the inflated blocks of its entry: system blocks
/// first, then graphics. The page map's declared system block count is tried
/// first, then every other boundary.
pub fn parse_blocks(blocks: &[Vec<u8>], lod: usize) -> Result<Drawable> {
    let first = blocks.first().context("empty entry")?;
    if !looks_like(first) {
        let what = if first.get(0x10..0x14) == Some(b"0DRP") { " (it is a 0DRP metadata resource)" } else { "" };
        bail!("not a standalone drawable{what}; drawable dictionaries and fragments are not supported yet");
    }
    ensure!(blocks.len() >= 2, "drawable entry is a single block, so its system/graphics split is unknown");
    let declared = u64_at(first, 8)
        .and_then(|p| first.get((p - VIRTUAL_BASE) as usize + 8))
        .map(|&n| n as usize)
        .filter(|n| (1..blocks.len()).contains(n));
    let mut first_error = None;
    for split in declared.into_iter().chain((1..blocks.len()).filter(|&s| Some(s) != declared)) {
        match parse(&blocks[..split].concat(), &blocks[split..].concat(), lod) {
            Ok(d) => return Ok(d),
            Err(e) => { first_error.get_or_insert(e); }
        }
    }
    Err(first_error.unwrap())
}

/// Decodes one LOD of a drawable given its system and graphics segments.
pub fn parse(sys: &[u8], gfx: &[u8], lod: usize) -> Result<Drawable> {
    ensure!(looks_like(sys), "not a standalone drawable header");
    ensure!(lod < LOD_COUNT, "LOD {lod} out of range (0..{LOD_COUNT})");
    let r = Res { sys, gfx };
    let name = r.name(168)?;
    let lod_field = 80 + 8 * lod;
    ensure!(r.u64(lod_field)? != 0, "drawable has no LOD {lod}");
    let group = r.ptr(lod_field, 16)?;
    let models = r.u16(group + 8)? as usize;
    ensure!((1..=256).contains(&models) && models == r.u16(group + 10)? as usize, "bad LOD model count {models}");
    let list = r.ptr(group, models * 8)?;
    let (mut meshes, mut skipped) = (Vec::new(), Vec::new());
    for mi in 0..models {
        let model = r.ptr(list + mi * 8, 56)?;
        let count = r.u16(model + 16)? as usize;
        ensure!((1..=1024).contains(&count) && count == r.u16(model + 18)? as usize, "model {mi}: bad geometry count {count}");
        let geometries = r.ptr(model + 8, count * 8)?;
        for gi in 0..count {
            let name = format!("model_{mi}_geometry_{gi}");
            match r.ptr(geometries + gi * 8, 160).and_then(|g| r.mesh(g, name.clone())) {
                Ok(m) => meshes.push(m),
                Err(e) => skipped.push(format!("{name}: {e:#}")),
            }
        }
    }
    ensure!(!meshes.is_empty(), "no decodable geometry in LOD {lod}: {}", skipped.join("; "));
    Ok(Drawable { name, lod, meshes, skipped })
}

struct Res<'a> {
    sys: &'a [u8],
    gfx: &'a [u8],
}

impl Res<'_> {
    fn u8(&self, at: usize) -> Result<u8> { self.sys.get(at).copied().with_context(|| format!("system read at {at:#x} out of bounds")) }
    fn u16(&self, at: usize) -> Result<u16> { u16_at(self.sys, at).with_context(|| format!("system read at {at:#x} out of bounds")) }
    fn u32(&self, at: usize) -> Result<u32> { u32_at(self.sys, at).with_context(|| format!("system read at {at:#x} out of bounds")) }
    fn u64(&self, at: usize) -> Result<u64> { u64_at(self.sys, at).with_context(|| format!("system read at {at:#x} out of bounds")) }

    /// System offset of the `size`-byte object the pointer at `at` refers to.
    fn ptr(&self, at: usize, size: usize) -> Result<usize> {
        let p = self.u64(at)?;
        let off = p.checked_sub(VIRTUAL_BASE).filter(|o| o.saturating_add(size as u64) <= self.sys.len() as u64);
        off.map(|o| o as usize).with_context(|| format!("system pointer {p:#x} at {at:#x} (+{size:#x}) out of range"))
    }

    /// The `size` graphics bytes the pointer at `at` refers to.
    fn gfx(&self, at: usize, size: usize) -> Result<&[u8]> {
        let p = self.u64(at)?;
        let off = p.checked_sub(PHYSICAL_BASE).filter(|o| o.saturating_add(size as u64) <= self.gfx.len() as u64);
        let off = off.with_context(|| format!("graphics pointer {p:#x} at {at:#x} (+{size:#x}) out of range"))? as usize;
        Ok(&self.gfx[off..off + size])
    }

    fn name(&self, at: usize) -> Result<String> {
        let s = self.ptr(at, 1)?;
        let bytes = &self.sys[s..self.sys.len().min(s + 256)];
        let end = bytes.iter().position(|&b| b == 0).context("unterminated drawable name")?;
        Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
    }

    fn mesh(&self, geometry: usize, name: String) -> Result<Mesh> {
        let vb = self.ptr(geometry + 8, 80)?;
        let layout = self.ptr(vb + 64, 320)?;
        let flags = self.u32(layout + 0x138)?;
        let count = (flags >> 10) as usize;
        ensure!(flags & 1 != 0 && (1..=1_000_000).contains(&count), "bad vertex layout flags {flags:#x}");
        let len = self.u32(vb + 8)? as usize * self.u16(vb + 12)? as usize;
        let vertex = self.gfx(vb + 24, len)?;
        let at = self.u32(layout)? as usize;
        let (size, format) = (self.u8(layout + 0xD0)?, self.u8(layout + 0x104)?);
        let descriptor = u32_at(vertex, 0).context("vertex buffer shorter than its descriptors")?;
        let stride = match descriptor & 127 {
            99 => 12,
            66 => 8,
            f => bail!("unsupported position format {f} (handled: 66 quantized, 99 float3)"),
        };
        ensure!(size as usize == stride && format == if stride == 12 { 6 } else { 16 },
            "position format {} disagrees with layout size {size} / format {format}", descriptor & 127);
        ensure!(at >= 96 && at + count * stride <= len, "positions {at:#x}+{count}x{stride} overflow the {len:#x}-byte vertex buffer");
        let positions: Vec<[f32; 3]> = if stride == 12 {
            (0..count).map(|i| vec3(vertex, at + i * 12).unwrap()).collect()
        } else {
            ensure!(at >= 128, "quantized positions overlap their origin/scale at +0x60");
            let (bias, axis, scale) = (vec3(vertex, 96).unwrap(), u32_at(vertex, 108).unwrap(), vec3(vertex, 112).unwrap());
            ensure!(axis <= 2 && finite(bias) && finite(scale) && scale.iter().all(|&s| s >= 0.0), "bad quantization origin/scale");
            (0..count).map(|i| {
                let q = u64_at(vertex, at + i * 8).unwrap();
                let lanes = [(q >> 8) & 0xFF_FFFF, ((q >> 32) & 0xFFFF) << 8 | (q & 0xFF), q >> 48];
                let [x, y, z] = [0, 1, 2].map(|k| (f64::from(bias[k]) + f64::from(scale[k]) * lanes[k] as f64) as f32);
                match axis { 0 => [z, y, x], 1 => [y, z, x], _ => [x, y, z] }
            }).collect()
        };
        ensure!(positions.iter().all(|&p| finite(p)), "non-finite vertex positions");
        let uvs = self.uvs(layout, count, vertex)?;
        let indices = self.indices(geometry, count)?;
        Ok(Mesh { name, positions, uvs, indices })
    }

    /// TEXCOORD0 as float2 or scaled snorm16x2; `None` when absent or in another encoding.
    fn uvs(&self, layout: usize, count: usize, vertex: &[u8]) -> Result<Option<Vec<[f32; 2]>>> {
        let size = self.u8(layout + 0xD0 + UV_SEMANTIC)? as usize;
        let format = self.u8(layout + 0x104 + UV_SEMANTIC)?;
        let float = match (size, format) { (8, 16) => true, (4, 38) => false, _ => return Ok(None) };
        let at = self.u32(layout + UV_SEMANTIC * 4)? as usize;
        let Some(native) = u32_at(vertex, 12) else { return Ok(None) };
        if at + count * size > vertex.len() || (native >> 7) & 0x1FF_FFFC != at as u32
            || ((native >> 3) & 60) as usize != size || native & 15 != if float { 2 } else { 8 } {
            return Ok(None);
        }
        let scale = if float { 1.0 } else {
            match u32_at(vertex, 124).map(f32::from_bits) { Some(s) if s.is_finite() && s > 0.0 && s <= 1e6 => s, _ => return Ok(None) }
        };
        let uv: Vec<[f32; 2]> = (0..count).map(|i| {
            let p = at + i * size;
            if float { [0, 4].map(|o| f32::from_bits(u32_at(vertex, p + o).unwrap())) }
            else { [0, 2].map(|o| (f32::from(u16_at(vertex, p + o).unwrap() as i16) / 32767.0).max(-1.0) * scale) }
        }).collect();
        Ok(uv.iter().flatten().all(|c| c.is_finite() && c.abs() <= 1e6).then_some(uv))
    }

    fn indices(&self, geometry: usize, vertex_count: usize) -> Result<Vec<u32>> {
        let desc = self.ptr(geometry + 64, 16)?;
        let ib = self.ptr(desc, 64)?;
        ensure!(self.u16(ib + 12)? == 4, "index buffer stride is not 4");
        let len = self.u32(ib + 8)? as usize * 4;
        ensure!(len >= 32, "index buffer too short");
        let data = self.gfx(ib + 24, len)?;
        let word = |at: usize| u32_at(data, at).context("index data truncated");
        let head = word(0)?;
        let palette = head & 0x4000_0000 != 0;
        let clusters = (head & if palette { 0xFFFF } else { 0x7FFF_FFFF }) as usize;
        let (triangles, refs) = (word(4)? as usize, word(8)? as usize);
        let binding = self.u32(desc + 12)?;
        ensure!((1..=2048).contains(&clusters) && triangles > 0 && refs > 0
            && clusters == self.u16(desc + 8)? as usize
            && triangles == (self.u16(desc + 10)? as usize | ((binding & 127) as usize) << 16)
            && refs == ((binding >> 8) & 0x7F_FFFF) as usize,
            "index header ({clusters} clusters, {triangles} triangles, {refs} refs) disagrees with its descriptor");
        let ref_size = if head & 0x8000_0000 != 0 { 2 } else { 4 };
        let tri_at = 12 + clusters * 16;
        let refs_at = tri_at + triangles * 4;
        ensure!(refs_at + refs * ref_size <= len, "cluster tables overflow the {len:#x}-byte index buffer");
        let table: Vec<u32> = (0..refs).map(|i| match ref_size {
            2 => u32::from(u16_at(data, refs_at + i * 2).unwrap()),
            _ => u32_at(data, refs_at + i * 4).unwrap(),
        }).collect();
        ensure!(table.iter().all(|&v| (v as usize) < vertex_count), "vertex reference past {vertex_count} vertices");
        let mut out = Vec::with_capacity(triangles * 3);
        let (mut ref_next, mut tri_next) = (0, 0);
        for cluster in 0..clusters {
            let (packed, offsets) = (word(12 + cluster * 16)?, word(16 + cluster * 16)?);
            let verts = (packed & 127) as usize + 1;
            let tris = ((packed >> 7) & 127) as usize + 1;
            let vstart = (offsets & 0xFFFF | ((packed >> 14) & 1) << 16) as usize;
            let tstart = (offsets >> 16 | ((packed >> 15) & 1) << 16) as usize;
            ensure!(vstart == ref_next && tstart == tri_next && vstart + verts <= refs && tstart + tris <= triangles,
                "cluster {cluster} ranges are not contiguous");
            for t in tstart..tstart + tris {
                let w = word(tri_at + t * 4)?;
                ensure!((w >> 21) as usize == cluster, "triangle {t} tagged for cluster {} not {cluster}", w >> 21);
                for corner in 0..3 {
                    let local = ((w >> (corner * 7)) & 127) as usize;
                    ensure!(local < verts, "triangle {t} corner past its cluster's {verts} vertices");
                    out.push(table[vstart + local]);
                }
            }
            (ref_next, tri_next) = (ref_next + verts, tri_next + tris);
        }
        ensure!(ref_next == refs && tri_next == triangles, "clusters do not cover the index tables");
        Ok(out)
    }
}

/// Wavefront OBJ. V is flipped from the top-left (D3D) to the bottom-left convention.
pub fn to_obj(d: &Drawable) -> String {
    let mut s = format!("# forge9 drawable export: {} LOD{}; positions in source (Z-up) space, no materials\n", d.name.replace(['\r', '\n'], "_"), d.lod);
    let (mut base, mut base_uv) = (1usize, 1usize);
    for m in &d.meshes {
        let _ = writeln!(s, "o {}", m.name);
        for p in &m.positions { let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]); }
        for uv in m.uvs.iter().flatten() { let _ = writeln!(s, "vt {} {}", uv[0], 1.0 - uv[1]); }
        for t in m.indices.chunks_exact(3) {
            let corner = |i: u32| match m.uvs { Some(_) => format!("{}/{}", base + i as usize, base_uv + i as usize), None => (base + i as usize).to_string() };
            let _ = writeln!(s, "f {} {} {}", corner(t[0]), corner(t[1]), corner(t[2]));
        }
        base += m.positions.len();
        if m.uvs.is_some() { base_uv += m.positions.len(); }
    }
    s
}

/// glTF 2.0 JSON plus its binary buffer. `uri` names an external .bin; `None` is for GLB.
pub fn to_gltf(d: &Drawable, uri: Option<&str>) -> (String, Vec<u8>) {
    let (mut bin, mut views, mut accessors, mut meshes) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut view = |bin: &mut Vec<u8>, bytes: Vec<u8>, target: u32| {
        views.push(format!(r#"{{"buffer":0,"byteOffset":{},"byteLength":{},"target":{target}}}"#, bin.len(), bytes.len()));
        bin.extend(bytes);
        views.len() - 1
    };
    for m in &d.meshes {
        let n = m.positions.len();
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &m.positions { for k in 0..3 { min[k] = min[k].min(p[k]); max[k] = max[k].max(p[k]); } }
        let v = view(&mut bin, m.positions.iter().flatten().flat_map(|c| c.to_le_bytes()).collect(), 34962);
        accessors.push(format!(r#"{{"bufferView":{v},"componentType":5126,"count":{n},"type":"VEC3","min":[{},{},{}],"max":[{},{},{}]}}"#,
            min[0], min[1], min[2], max[0], max[1], max[2]));
        let mut attributes = format!(r#""POSITION":{}"#, accessors.len() - 1);
        if let Some(uvs) = &m.uvs {
            let v = view(&mut bin, uvs.iter().flatten().flat_map(|c| c.to_le_bytes()).collect(), 34962);
            accessors.push(format!(r#"{{"bufferView":{v},"componentType":5126,"count":{n},"type":"VEC2"}}"#));
            let _ = write!(attributes, r#","TEXCOORD_0":{}"#, accessors.len() - 1);
        }
        let v = view(&mut bin, m.indices.iter().flat_map(|i| i.to_le_bytes()).collect(), 34963);
        accessors.push(format!(r#"{{"bufferView":{v},"componentType":5125,"count":{},"type":"SCALAR"}}"#, m.indices.len()));
        meshes.push(format!(r#"{{"name":"{}","primitives":[{{"attributes":{{{attributes}}},"indices":{}}}]}}"#, m.name, accessors.len() - 1));
    }
    let children: Vec<String> = (1..=meshes.len()).map(|i| i.to_string()).collect();
    let mut nodes = vec![format!(r#"{{"name":"drawable","rotation":[-0.70710677,0,0,0.70710677],"children":[{}]}}"#, children.join(","))];
    nodes.extend((0..meshes.len()).map(|i| format!(r#"{{"mesh":{i}}}"#)));
    let uri = uri.map_or(String::new(), |u| format!(r#","uri":"{u}""#));
    let json = format!(
        r#"{{"asset":{{"version":"2.0","generator":"forge9"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{}],"meshes":[{}],"accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}{uri}}}]}}"#,
        nodes.join(","), meshes.join(","), accessors.join(","), views.join(","), bin.len());
    (json, bin)
}

/// Binary glTF container (all buffer views are 4-byte multiples already).
pub fn to_glb(d: &Drawable) -> Vec<u8> {
    let (json, bin) = to_gltf(d, None);
    let mut json = json.into_bytes();
    json.resize(json.len().next_multiple_of(4), b' ');
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    for word in [0x4654_6C67, 2, total as u32, json.len() as u32, 0x4E4F_534A] { out.extend(u32::to_le_bytes(word)); }
    out.extend(json);
    for word in [bin.len() as u32, 0x004E_4942] { out.extend(u32::to_le_bytes(word)); }
    out.extend(bin);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(buf: &mut Vec<u8>, at: usize, bytes: &[u8]) {
        if buf.len() < at + bytes.len() { buf.resize(at + bytes.len(), 0); }
        buf[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn alloc(buf: &mut Vec<u8>, size: usize) -> usize {
        let at = buf.len().next_multiple_of(16);
        buf.resize(at + size, 0);
        at
    }
    fn sp(off: usize) -> [u8; 8] { (VIRTUAL_BASE + off as u64).to_le_bytes() }
    fn gp(off: usize) -> [u8; 8] { (PHYSICAL_BASE + off as u64).to_le_bytes() }
    fn f32s(v: &[f32]) -> Vec<u8> { v.iter().flat_map(|f| f.to_le_bytes()).collect() }

    /// A quad geometry: `format` 66 (quantized + float2 UVs), 99 (float3), or anything else.
    fn geometry(sys: &mut Vec<u8>, gfx: &mut Vec<u8>, format: u32) -> usize {
        let mut v = vec![0u8; 96];
        let (at, size, lfmt) = if format == 66 { (128, 8u8, 16u8) } else { (96, 12, 6) };
        put(&mut v, 0, &(format | (at as u32) << 7).to_le_bytes());
        if format == 66 {
            put(&mut v, 96, &f32s(&[10.0, 20.0, 30.0]));
            put(&mut v, 108, &2u32.to_le_bytes());
            put(&mut v, 112, &f32s(&[0.5, 0.5, 0.5]));
            for (i, [x, y, z]) in [[0u64, 0, 0], [2, 0, 0], [2, 2, 0], [0, 2, 4]].into_iter().enumerate() {
                let q = x << 8 | (y & 0xFF) | (y >> 8) << 32 | z << 48;
                put(&mut v, at + i * 8, &q.to_le_bytes());
            }
            put(&mut v, 12, &(160u32 << 7 | 8 << 3 | 2).to_le_bytes());
            put(&mut v, 160, &f32s(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]));
        } else {
            put(&mut v, at, &f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0]));
        }
        v.resize(v.len().next_multiple_of(16), 0);
        let vdata = alloc(gfx, v.len());
        put(gfx, vdata, &v);

        // Header {flags|clusters, triangles, refs}, one 16-byte cluster, 2 triangles, 4 u16 refs in reverse.
        let mut ix = Vec::new();
        for w in [0x8000_0001u32, 2, 4, 3 | 1 << 7, 0, 0, 0, 1 << 7 | 2 << 14, 2 << 7 | 3 << 14] { ix.extend(w.to_le_bytes()); }
        for r in [3u16, 2, 1, 0] { ix.extend(r.to_le_bytes()); }
        ix.resize(48, 0);
        let idata = alloc(gfx, ix.len());
        put(gfx, idata, &ix);

        let g = alloc(sys, 160);
        let vb = alloc(sys, 80);
        let layout = alloc(sys, 320);
        let desc = alloc(sys, 16);
        let ib = alloc(sys, 64);
        put(sys, g + 8, &sp(vb));
        put(sys, g + 64, &sp(desc));
        put(sys, vb + 8, &(v.len() as u32 / 16).to_le_bytes());
        put(sys, vb + 12, &16u16.to_le_bytes());
        put(sys, vb + 24, &gp(vdata));
        put(sys, vb + 64, &sp(layout));
        put(sys, layout, &(at as u32).to_le_bytes());
        put(sys, layout + 0xD0, &[size]);
        put(sys, layout + 0x104, &[lfmt]);
        if format == 66 {
            put(sys, layout + UV_SEMANTIC * 4, &160u32.to_le_bytes());
            put(sys, layout + 0xD0 + UV_SEMANTIC, &[8]);
            put(sys, layout + 0x104 + UV_SEMANTIC, &[16]);
        }
        put(sys, layout + 0x138, &(4u32 << 10 | 1).to_le_bytes());
        put(sys, desc, &sp(ib));
        put(sys, desc + 8, &1u16.to_le_bytes());
        put(sys, desc + 10, &2u16.to_le_bytes());
        put(sys, desc + 12, &(4u32 << 8).to_le_bytes());
        put(sys, ib + 8, &(ix.len() as u32 / 4).to_le_bytes());
        put(sys, ib + 12, &4u16.to_le_bytes());
        put(sys, ib + 24, &gp(idata));
        g
    }

    /// System and graphics blocks of a one-model drawable with quantized, float3 and unsupported geometries.
    fn synthetic() -> Vec<Vec<u8>> {
        let (mut sys, mut gfx) = (vec![0u8; 192], Vec::new());
        put(&mut sys, 44, &f32s(&[2.0, 10.0, 20.0, 30.0]));
        put(&mut sys, 64, &f32s(&[11.0, 21.0, 32.0]));
        let pages = alloc(&mut sys, 16);
        put(&mut sys, pages + 8, &[1, 1]);
        let shaders = alloc(&mut sys, 64);
        let name = alloc(&mut sys, 16);
        put(&mut sys, name, b"synthetic_prop\0");
        let group = alloc(&mut sys, 16);
        let list = alloc(&mut sys, 8);
        let model = alloc(&mut sys, 56);
        let geoms = alloc(&mut sys, 24);
        for (i, f) in [66, 99, 48].into_iter().enumerate() {
            let g = geometry(&mut sys, &mut gfx, f);
            put(&mut sys, geoms + i * 8, &sp(g));
        }
        for (at, p) in [(8, pages), (16, shaders), (80, group), (168, name)] { put(&mut sys, at, &sp(p)); }
        put(&mut sys, group, &sp(list));
        put(&mut sys, group + 8, &[1, 0, 1, 0]);
        put(&mut sys, list, &sp(model));
        put(&mut sys, model + 8, &sp(geoms));
        put(&mut sys, model + 16, &[3, 0, 3, 0]);
        vec![sys, gfx]
    }

    #[test]
    fn decodes_synthetic_drawable() {
        let d = parse_blocks(&synthetic(), 0).unwrap();
        assert_eq!((d.name.as_str(), d.meshes.len(), d.skipped.len()), ("synthetic_prop", 2, 1));
        assert!(d.skipped[0].contains("unsupported position format 48"), "{}", d.skipped[0]);
        let q = &d.meshes[0];
        assert_eq!(q.positions, [[10.0, 20.0, 30.0], [11.0, 20.0, 30.0], [11.0, 21.0, 30.0], [10.0, 21.0, 32.0]]);
        assert_eq!(q.uvs.as_deref(), Some(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]][..]));
        // refs table is reversed, so local (0,1,2),(0,2,3) remaps to (3,2,1),(3,1,0)
        assert_eq!(q.indices, [3, 2, 1, 3, 1, 0]);
        let f = &d.meshes[1];
        assert_eq!((f.positions[2], f.uvs.is_none()), ([1.0, 1.0, 0.0], true));
        assert!(parse_blocks(&synthetic(), 1).unwrap_err().to_string().contains("no LOD 1"));

        let glb = to_glb(&d);
        assert_eq!((&glb[0..4], u32_at(&glb, 8).unwrap() as usize), (&b"glTF"[..], glb.len()));
        let json_len = u32_at(&glb, 12).unwrap() as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_len]).unwrap();
        assert!(json.contains(r#""TEXCOORD_0":1"#) && json.contains(r#""min":[10,20,30],"max":[11,21,32]"#), "{json}");
        assert_eq!(u32_at(&glb, 20 + json_len).unwrap() as usize, glb.len() - 28 - json_len);
        let obj = to_obj(&d);
        assert_eq!((obj.matches("\nv ").count(), obj.matches("\nvt ").count(), obj.matches("\nf ").count()), (8, 4, 4));
        assert!(obj.contains("f 4/4 3/3 2/2") && obj.contains("f 8 6 5"), "{obj}");

        // `FORGE9_FIXTURE_OUT=path` writes the entry as a zlib block chain, for `repack` + `model` end-to-end runs.
        if let Ok(path) = std::env::var("FORGE9_FIXTURE_OUT") {
            use std::io::Write;
            let mut out = Vec::new();
            for block in synthetic() {
                let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                z.write_all(&block).unwrap();
                out.extend(z.finish().unwrap());
                out.resize(out.len().next_multiple_of(16), 0);
            }
            std::fs::write(path, out).unwrap();
        }
    }
}
