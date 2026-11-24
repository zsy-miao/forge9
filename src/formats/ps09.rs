//! PS09: the driver pipeline cache (shaders.rpf entry #1, zlib).
//!
//! Sections follow each other without padding: header (0x80), graphics PSO
//! records (0x70 each), compute PSO records (0x18), metadata index and shader
//! index (`u32 offset, u32 size` each), metadata area, code area.
//!
//! Header: `PS 00 09`, ASCII id at 0x04, driver version at 0x24; at 0x50
//! graphics, compute, metadata and shader counts; at 0x60 metadata and code
//! bytes. A graphics record is `u64 hash, u32 state meta, u32 outputs meta,
//! u64 shader hash` then 7 slots of `i32 shader, i32 meta` at 0x38 (-1 when
//! empty): 0 pixel, 1 vertex, 2 mesh (NGG), 6 fetch shader. A compute record is
//! `u64 hash, u32 state meta, u32 meta, u32 shader, u32 shader meta`.

use anyhow::{bail, Result};
use super::{u32_at, u64_at};

pub const SLOT_PIXEL: usize = 0;
pub const SLOT_VERTEX: usize = 1;
pub const SLOT_MESH: usize = 2;
pub const SLOT_NAMES: [&str; 7] = ["PS", "VS", "MS", "S3", "S4", "S5", "FS"];

#[derive(Debug, Clone)]
pub struct Header {
    pub id: String,
    pub driver: String,
    pub graphics: u32,
    pub compute: u32,
    pub metas: u32,
    pub shaders: u32,
    pub meta_bytes: u32,
    pub code_bytes: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub shader: u32,
    pub meta: u32,
}

#[derive(Debug, Clone)]
pub struct GraphicsPso {
    pub hash: u64,
    pub state_meta: u32,
    pub outputs_meta: u32,
    pub shader_hash: u64,
    pub slots: [Option<Slot>; 7],
}

impl GraphicsPso {
    pub fn is_mesh(&self) -> bool { self.slots[SLOT_MESH].is_some() }
    pub fn shader_count(&self) -> usize { self.slots.iter().flatten().count() }
}

#[derive(Debug, Clone)]
pub struct ComputePso {
    pub hash: u64,
    pub state_meta: u32,
    pub meta: u32,
    pub shader: u32,
    pub shader_meta: u32,
}

#[derive(Debug, Clone)]
pub struct Cache {
    pub header: Header,
    pub graphics: Vec<GraphicsPso>,
    pub compute: Vec<ComputePso>,
    /// Byte ranges of each metadata entry and code block in the blob.
    pub metas: Vec<std::ops::Range<usize>>,
    pub shaders: Vec<std::ops::Range<usize>>,
}

fn ascii(b: &[u8]) -> String {
    String::from_utf8_lossy(b.split(|&c| c == 0).next().unwrap_or(b)).into_owned()
}

pub fn parse(data: &[u8]) -> Result<Cache> {
    if !data.starts_with(b"PS\x00\x09") { bail!("not a PS09 pipeline cache"); }
    if data.len() < 0x80 { bail!("PS09 header truncated"); }
    let w = |o| u32_at(data, o).unwrap();
    let header = Header {
        id: ascii(&data[0x04..0x24]),
        driver: ascii(&data[0x24..0x48]),
        graphics: w(0x50), compute: w(0x54), metas: w(0x58), shaders: w(0x5C),
        meta_bytes: w(0x60), code_bytes: w(0x64),
    };
    let (ng, nc, nm, ns) = (header.graphics as usize, header.compute as usize, header.metas as usize, header.shaders as usize);
    let gfx_at = 0x80;
    let cs_at = gfx_at + 0x70 * ng;
    let mi_at = cs_at + 0x18 * nc;
    let si_at = mi_at + 8 * nm;
    let marea = si_at + 8 * ns;
    let carea = marea + header.meta_bytes as usize;
    if carea > data.len() { bail!("PS09 records and metadata ({carea:#x} bytes) exceed the blob ({:#x})", data.len()); }

    let r = |o| u32_at(data, o).unwrap();
    let graphics = (0..ng).map(|i| {
        let at = gfx_at + 0x70 * i;
        let slots = std::array::from_fn(|s| {
            let (shader, meta) = (r(at + 0x38 + 8 * s), r(at + 0x3C + 8 * s));
            (shader != u32::MAX).then_some(Slot { shader, meta })
        });
        GraphicsPso { hash: u64_at(data, at).unwrap(), state_meta: r(at + 8), outputs_meta: r(at + 0xC), shader_hash: u64_at(data, at + 0x10).unwrap(), slots }
    }).collect();
    let compute = (0..nc).map(|i| {
        let at = cs_at + 0x18 * i;
        ComputePso { hash: u64_at(data, at).unwrap(), state_meta: r(at + 8), meta: r(at + 0xC), shader: r(at + 0x10), shader_meta: r(at + 0x14) }
    }).collect();
    let ranges = |at: usize, n: usize, base: usize| (0..n).map(|i| {
        let start = base + r(at + 8 * i) as usize;
        start..start + r(at + 8 * i + 4) as usize
    }).collect::<Vec<_>>();
    let metas = ranges(mi_at, nm, marea);
    let shaders = ranges(si_at, ns, carea);
    if let Some(m) = metas.iter().find(|m| m.end > carea) { bail!("metadata entry {m:?} runs past the metadata area"); }
    Ok(Cache { header, graphics, compute, metas, shaders })
}

impl Cache {
    /// A shader's code block; `None` when the blob is cut short.
    pub fn code<'a>(&self, data: &'a [u8], shader: u32) -> Option<&'a [u8]> {
        data.get(self.shaders.get(shader as usize)?.clone())
    }

    pub fn meta<'a>(&self, data: &'a [u8], index: u32) -> Option<&'a [u8]> {
        data.get(self.metas.get(index as usize)?.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records() {
        let mut d = vec![0u8; 0x80];
        d[..4].copy_from_slice(b"PS\x00\x09");
        d[0x24..0x2F].copy_from_slice(b"930.048.020");
        for (o, v) in [(0x50, 1u32), (0x54, 1), (0x58, 3), (0x5C, 2), (0x60, 12), (0x64, 0x200)] { d[o..o + 4].copy_from_slice(&v.to_le_bytes()); }
        let mut g = vec![0xFFu8; 0x70];
        g[..0x18].fill(0);
        g[..8].copy_from_slice(&0xAABBu64.to_le_bytes());
        for (o, v) in [(0x38, 1u32), (0x3C, 0), (0x48, 0), (0x4C, 1)] { g[o..o + 4].copy_from_slice(&v.to_le_bytes()); }
        d.extend_from_slice(&g);
        for v in [0xCCu64, 0x1_0000_0000, 0x5_0000_0004] { d.extend_from_slice(&v.to_le_bytes()); }
        for (off, size) in [(0u32, 4u32), (4, 4), (8, 4), (0, 0x100), (0x100, 0x100)] { d.extend_from_slice(&off.to_le_bytes()); d.extend_from_slice(&size.to_le_bytes()); }
        d.resize(d.len() + 12 + 0x200, 0);

        let c = parse(&d).unwrap();
        assert_eq!((c.header.driver.as_str(), c.graphics.len(), c.compute.len()), ("930.048.020", 1, 1));
        let p = &c.graphics[0];
        assert_eq!((p.hash, p.shader_count(), p.is_mesh()), (0xAABB, 2, true));
        assert_eq!(p.slots[SLOT_PIXEL].unwrap().shader, 1);
        assert_eq!((p.slots[SLOT_MESH].unwrap().shader, p.slots[SLOT_MESH].unwrap().meta), (0, 1));
        assert_eq!((c.compute[0].shader, c.compute[0].shader_meta), (4, 5));
        assert_eq!(c.code(&d, 1).unwrap().len(), 0x100);
        assert_eq!(c.meta(&d, 2).unwrap().len(), 4);
    }
}
