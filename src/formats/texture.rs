//! RDS5 texture stores (`.ttpf`, standalone files next to the archives).
//!
//! Parsing, BCPack decoding (MDU rANS; BC1, and BC3/BC5/BC7 from the
//! `bc5-bc7` branch), BC1 Gfx10 detiling and DDS/PNG output come from
//! bcpack-rs by Ahmet Dedeler (https://github.com/Ahmet-Dedeler/bcpack-rs,
//! MIT OR Apache-2.0). BC3/BC4/BC5/BC7 pixels come from bcdec_rs.

use anyhow::{Context, Result};
use std::collections::HashMap;
use bcpack::{bc7, bcn, format::Mode};
pub use bcpack::ttpf::{Store, Texture, CODEC_BC1, CODEC_STORED, CODEC_ZLIB};

/// A store with only its header, chunk table and ETPF index read; `load` reads
/// one texture's chunks on demand, so multi-GB and partially downloaded stores
/// cost no more than their index. Unread bytes are zero pages of one allocation.
pub struct LazyStore {
    file: std::fs::File,
    pub store: Store,
    pub textures: Vec<Texture>,
}

impl LazyStore {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut data = vec![0u8; usize::try_from(file.metadata()?.len())?];
        let word = |d: &[u8], at: usize| d.get(at..at + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap())) as usize;
        read_into(&mut file, &mut data, 0, 24)?;
        anyhow::ensure!(&data[..4] == b"RDS5", "no RDS5 header (missing or not downloaded yet)");
        let n = word(&data, 4);
        read_into(&mut file, &mut data, 24, n * 8)?;
        let etpf = word(&data, 16) << 8;
        read_into(&mut file, &mut data, etpf, 16)?;
        let t = word(&data, etpf + 8);
        read_into(&mut file, &mut data, etpf + 16, t * 24)?;
        let store = Store::from_bytes(data)?;
        let textures = store.textures()?;
        Ok(Self { file, store, textures })
    }

    /// Reads the chunks from `tex`'s start up to the next texture's; fails when one
    /// of them is all zeros, as in a partially downloaded store.
    pub fn load(&mut self, tex: &Texture) -> Result<()> {
        let end = self.textures.iter().map(|t| t.vstart).filter(|&v| v > tex.vstart).min().unwrap_or(self.store.vsize);
        for (ci, _, _) in self.store.locate(tex.vstart, end)? {
            let c = &self.store.chunks[ci];
            read_into(&mut self.file, &mut self.store.data, c.file_off as usize, c.file_len as usize)?;
            anyhow::ensure!(self.store.chunk_bytes(ci, true)?.iter().any(|&b| b != 0), "texture {}: chunk {ci} is all zeros (not downloaded yet?)", tex.index);
        }
        Ok(())
    }
}

fn read_into(file: &mut std::fs::File, data: &mut [u8], at: usize, len: usize) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let buf = data.get_mut(at..at.saturating_add(len)).with_context(|| format!("range {at:#x}+{len:#x} past end of file"))?;
    file.seek(SeekFrom::Start(at as u64))?;
    Ok(file.read_exact(buf)?)
}

/// An `EDUP` table (an entry of `textures.rpf`): texture keys whose pixels are stored
/// under another key. `EDUP`, u32 count, then count `{u64 key, u64 stored key}` sorted by key.
pub fn edup(d: &[u8]) -> Option<HashMap<u64, u64>> {
    if d.get(..4)? != b"EDUP" { return None; }
    let n = super::u32_at(d, 4)? as usize;
    let recs = d.get(8..8 + n.checked_mul(16)?)?;
    Some(recs.chunks_exact(16).map(|r| (super::u64_at(r, 0).unwrap(), super::u64_at(r, 8).unwrap())).collect())
}

pub fn dxgi_name(dxgi: u32) -> &'static str {
    match dxgi {
        28 | 29 => "RGBA8",
        70..=72 => "BC1",
        73..=75 => "BC2",
        76..=78 => "BC3",
        79..=81 => "BC4",
        82..=84 => "BC5",
        87 | 91 => "BGRA8",
        94..=96 => "BC6H",
        97..=99 => "BC7",
        _ => "?",
    }
}

/// Tightly packed payload bytes after dropping 0, 1, 2 or 3 of the largest mips.
/// The smallest declared mip is always retained. Includes array slices for
/// tile modes 0–3, and excludes padding or extra copies made by a driver.
pub fn mip_payload_budget(tex: &Texture) -> Option<[u64; 4]> {
    let (block_width, bytes_per_block) = match tex.dxgi {
        28 | 29 | 87 | 91 => (1, 4),
        70..=72 | 79..=81 => (4, 8),
        73..=78 | 82..=84 | 94..=99 => (4, 16),
        _ => return None,
    };
    if tex.width == 0 || tex.height == 0 || tex.array_size == 0 || tex.tile_mode > 3
        || tex.mips == 0 || tex.mips > tex.width.max(tex.height).ilog2() + 1
    {
        return None;
    }

    let mut removable = [0; 3];
    let mut full: u64 = 0;
    for level in 0..tex.mips {
        let wide = (tex.width >> level).max(1).div_ceil(block_width);
        let high = (tex.height >> level).max(1).div_ceil(block_width);
        let bytes = (u64::from(wide) * u64::from(high))
            .checked_mul(bytes_per_block)?
            .checked_mul(tex.array_size.into())?;
        full = full.checked_add(bytes)?;
        if level + 1 < tex.mips {
            if let Some(slot) = removable.get_mut(level as usize) {
                *slot = bytes;
            }
        }
    }
    let [first, second, third] = removable;
    Some([full, full - first, full - first - second, full - first - second - third])
}

pub fn codec_name(codec: u8) -> &'static str {
    match codec {
        CODEC_STORED => "stored",
        CODEC_ZLIB => "zlib",
        CODEC_BC1 => "bcpack-bc1",
        0x0C => "bcpack-bc3",
        0x10 => "bcpack-bc4",
        0x14 => "bcpack-bc5",
        0x18 => "bcpack-bc6h",
        0x1C => "bcpack-bc7",
        _ => "?",
    }
}

/// Mip 0 (of array slice `slice`) of a BC1/BC3/BC4/BC5/BC7 texture as DDS bytes,
/// or PNG when `png`; also returns the number of blocks bcpack-rs flags as
/// depending on undetermined table values (0 = exact).
pub fn export(store: &Store, tex: &Texture, png: bool, slice: u32) -> Result<(Vec<u8>, usize)> {
    let (blocks, unfitted) = mip0_blocks(store, tex, slice)?;
    let bytes = if png { bcpack::ttpf::png_rgba(&to_rgba(&blocks, tex), tex.width, tex.height)? }
        else if is_16b(tex) || is_bc4(tex) { dds_dx10(&blocks, tex.width, tex.height, tex.dxgi) }
        else { bcpack::ttpf::dds_bc1(&blocks, tex.width, tex.height) };
    Ok((bytes, unfitted))
}

/// Mip 0 of slice 0 as PNG for a glTF material; BC5 (two-channel normal map) gets Z rebuilt into blue.
pub fn material_png(store: &Store, tex: &Texture) -> Result<Vec<u8>> {
    let mut px = to_rgba(&mip0_blocks(store, tex, 0)?.0, tex);
    if matches!(tex.dxgi, 82..=84) { rebuild_z(&mut px); }
    Ok(bcpack::ttpf::png_rgba(&px, tex.width, tex.height)?)
}

fn rebuild_z(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let [x, y] = [p[0], p[1]].map(|c| f32::from(c) / 127.5 - 1.0);
        p[2] = (((1.0 - x * x - y * y).max(0.0).sqrt() + 1.0) * 127.5).round() as u8;
    }
}

fn is_16b(tex: &Texture) -> bool { matches!(tex.dxgi, 76..=78 | 82..=84 | 97..=99) }
fn is_bc4(tex: &Texture) -> bool { matches!(tex.dxgi, 79..=81) }

fn to_rgba(blocks: &[u8], tex: &Texture) -> Vec<u8> {
    let (bw, bh, w, h) = (tex.width.div_ceil(4) as usize, tex.height.div_ceil(4) as usize, tex.width as usize, tex.height as usize);
    if is_16b(tex) { bc16_to_rgba(blocks, tex.dxgi, bw, w, h) }
    else if is_bc4(tex) { bc4_to_rgba(blocks, bw, w, h) }
    else { bcpack::ttpf::bc1_to_rgba(blocks, bw, bh, w, h) }
}

/// Row-major mip 0 blocks of slice `slice` and bcpack-rs's unfitted block count.
fn mip0_blocks(store: &Store, tex: &Texture, slice: u32) -> Result<(Vec<u8>, usize)> {
    if is_16b(tex) {
        return blocks_16b(store, tex, slice);
    }
    anyhow::ensure!(slice == 0, "texture {}: slices are only supported for BC3/BC5/BC7", tex.index);
    if is_bc4(tex) {
        anyhow::ensure!(tex.array_size == 1 && tex.tile_mode <= 3, "texture {}: BC4 array or tile mode {} not supported", tex.index, tex.tile_mode);
        // Same 8-byte-element layout as BC1.
        let (levels, _) = bcpack::ttpf::mip_chain_bc1(tex.tile_mode, tex.width, tex.height, tex.mips).context("unsupported mip layout")?;
        let (tiled, unfitted) = gather(store, tex.vstart + levels[0].off, levels[0].len)?;
        return Ok((bcpack::ttpf::detile_bc1(&tiled, &levels[0], tex.tile_mode).0, unfitted));
    }
    let window = bcpack::bc1::Window::default_table();
    let quads = bcpack::bc1::QuadTable::default_table();
    let m = bcpack::texture_mip0(store, tex, &window, &quads).with_context(|| format!("texture {}", tex.index))?;
    Ok((m.blocks, m.unfitted_blocks))
}

/// Gfx10 swizzle of 16-byte elements, lowest address bit first: a 4x4-element
/// 256-byte micro tile, then alternating y/x up to the 64 KiB or 4 KiB tile.
fn pattern_16b(tile_mode: u32) -> &'static str {
    if tile_mode == 0 { "xyxyyxyxyxyx" } else { "yyxxyxyx" }
}

/// Mip 0 of slice `slice`: (byte offset in the texture, length, region width
/// and height in elements, origin x/y in elements). Same chain rules as
/// bcpack's `mip_chain_bc1`; array slices are stored level-major, smallest
/// level (the packed mip tail) first. Checked against the mip pyramids of real
/// BC3/BC5/BC7 stores for tile modes 0-3.
fn mip0_place_16b(tile_mode: u32, w: u32, h: u32, mips: u32, slices: u32, slice: u32) -> (u64, u64, u32, u32, u32, u32) {
    let pat = pattern_16b(tile_mode);
    let xs = pat.bytes().filter(|&c| c == b'x').count() as u32;
    let (bw, bh) = (1u32 << xs, 1u32 << (pat.len() as u32 - xs));
    let tile = bw as u64 * bh as u64 * 16;
    let max_in_tail = if tile_mode == 0 { 12 } else { 8 };
    let el = |i: u32| ((w >> i).max(1).div_ceil(4), (h >> i).max(1).div_ceil(4));
    let tail = (0..mips).find(|&i| mips > 1 && el(i).0 <= bw / 2 && el(i).1 <= bh && mips - i <= max_in_tail).unwrap_or(mips);
    let (n, s) = (slices as u64, slice as u64);
    if tail == 0 {
        // Mip 0 sits in the tail tile, at the tail's first slot.
        let morton = (16u32 << (max_in_tail - 1)) >> 8;
        let (mut mx, mut my) = (0, 0);
        for bit in 0..8 {
            my |= ((morton >> (2 * bit)) & 1) << bit;
            mx |= ((morton >> (2 * bit + 1)) & 1) << bit;
        }
        return (s * tile, tile, bw, bh, mx * 4, my * 4);
    }
    let len = |i: u32| el(i).0.div_ceil(bw) as u64 * el(i).1.div_ceil(bh) as u64 * tile;
    let before: u64 = (1..tail).map(len).sum::<u64>() + if tail < mips { tile } else { 0 };
    let (ew, eh) = el(0);
    (n * before + s * len(0), len(0), ew.div_ceil(bw) * bw, eh.div_ceil(bh) * bh, 0, 0)
}

fn blocks_16b(store: &Store, tex: &Texture, slice: u32) -> Result<(Vec<u8>, usize)> {
    anyhow::ensure!(tex.tile_mode <= 3, "texture {}: tile mode {} not supported", tex.index, tex.tile_mode);
    anyhow::ensure!(slice < tex.array_size, "texture {}: slice {slice} out of range ({} slices)", tex.index, tex.array_size);
    let (off, len, rw, rh, ox, oy) = mip0_place_16b(tex.tile_mode, tex.width, tex.height, tex.mips, tex.array_size, slice);
    let (tiled, unfitted) = gather(store, tex.vstart + off, len)?;
    let (rw, rh) = (rw as usize, rh as usize);
    let mut lin = vec![0u8; rw * rh * 16];
    for (i, d) in bcpack::ttpf::untile_order(tiled.len() / 16, rw, rh, pattern_16b(tex.tile_mode)).into_iter().enumerate() {
        if let Some(d) = d {
            lin[d * 16..d * 16 + 16].copy_from_slice(&tiled[i * 16..i * 16 + 16]);
        }
    }
    let (bw, bh) = (tex.width.div_ceil(4) as usize, tex.height.div_ceil(4) as usize);
    let blocks: Vec<u8> = (0..bh).flat_map(|y| lin[((y + oy as usize) * rw + ox as usize) * 16..][..bw * 16].iter().copied()).collect();
    Ok((blocks, unfitted))
}

/// Stored bytes of virtual range `a..a + len` with BCPack BC3/BC4/BC5/BC7 chunks
/// decoded, and the number of blocks bcpack-rs flags as unfitted.
fn gather(store: &Store, a: u64, len: u64) -> Result<(Vec<u8>, usize)> {
    let mut out = Vec::with_capacity(len as usize);
    let mut unfitted = 0;
    for (ci, f, e) in store.locate(a, a + len)? {
        let c = &store.chunks[ci];
        let bytes = match (c.codec, Mode::from_codec(c.codec)) {
            (CODEC_ZLIB | CODEC_STORED, _) => store.plain_chunk(ci)?,
            (_, Some(Mode::Bc7)) => {
                let r = bc7::decode(store.chunk_bytes(ci, true)?, c.vlen as usize, bc7::Tables::default_table()).map_err(|e| anyhow::anyhow!("chunk {ci}: {e:?}"))?;
                unfitted += r.unfitted_blocks;
                r.blocks
            }
            (_, Some(m @ (Mode::Bc3 | Mode::Bc4 | Mode::Bc5))) => {
                let r = bcn::decode(store.chunk_bytes(ci, true)?, c.vlen as usize, bcn::Tables::default_for(m).unwrap()).map_err(|e| anyhow::anyhow!("chunk {ci}: {e:?}"))?;
                unfitted += r.unfitted_blocks;
                r.blocks
            }
            (x, _) => anyhow::bail!("chunk {ci}: codec {} not supported here", codec_name(x)),
        };
        out.extend_from_slice(&bytes[f as usize..e as usize]);
    }
    Ok((out, unfitted))
}

/// Row-major BC4 blocks -> grey RGBA8.
fn bc4_to_rgba(blocks: &[u8], bw: usize, w: usize, h: usize) -> Vec<u8> {
    let mut rgba = vec![255u8; w * h * 4];
    for (k, b) in blocks.chunks_exact(8).enumerate() {
        let mut px = [0u8; 16];
        bcdec_rs::bc4(b, &mut px, 4, false);
        let (bx, by) = (k % bw * 4, k / bw * 4);
        for y in 0..4.min(h - by) {
            for x in 0..4.min(w - bx) {
                rgba[((by + y) * w + bx + x) * 4..][..3].fill(px[y * 4 + x]);
            }
        }
    }
    rgba
}

/// Row-major BC3/BC5/BC7 blocks -> RGBA8 (BC5 as R, G, 0, 255).
fn bc16_to_rgba(blocks: &[u8], dxgi: u32, bw: usize, w: usize, h: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; w * h * 4];
    for (k, b) in blocks.chunks_exact(16).enumerate() {
        let mut px = [0u8; 64];
        match dxgi {
            82..=84 => {
                let mut rg = [0u8; 32];
                bcdec_rs::bc5(b, &mut rg, 8, false);
                for p in 0..16 {
                    px[4 * p..4 * p + 4].copy_from_slice(&[rg[2 * p], rg[2 * p + 1], 0, 255]);
                }
            }
            76..=78 => bcdec_rs::bc3(b, &mut px, 16),
            _ => bcdec_rs::bc7(b, &mut px, 16),
        }
        let (bx, by) = (k % bw * 4, k / bw * 4);
        for y in 0..4.min(h - by) {
            let n = 4.min(w - bx) * 4;
            rgba[((by + y) * w + bx) * 4..][..n].copy_from_slice(&px[y * 16..][..n]);
        }
    }
    rgba
}

/// A DDS file with a DX10 header holding one level of `dxgi` blocks.
fn dds_dx10(blocks: &[u8], w: u32, h: u32, dxgi: u32) -> Vec<u8> {
    let mut d = bcpack::ttpf::dds_bc1(&[], w, h); // same legacy header
    d[20..24].copy_from_slice(&(blocks.len() as u32).to_le_bytes()); // linear size
    d[84..88].copy_from_slice(b"DX10");
    for v in [dxgi, 3, 0, 1, 0] {
        d.extend_from_slice(&v.to_le_bytes()); // format, TEXTURE2D, misc, array size, misc2
    }
    d.extend_from_slice(blocks);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(d: &mut [u8], at: usize, v: u32) {
        d[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// One 4 KiB BCPack BC1 chunk (512 blocks) and one 4x4 BC1 texture over it.
    fn synthetic_store() -> (Vec<u8>, Vec<u8>) {
        let (chunk, _) = bcpack::encode::synthetic_bc1(7, 512, 64);
        (store_over(&chunk, CODEC_BC1, 71), chunk)
    }

    /// A store with one 4 KiB chunk and one 4x4 texture of `dxgi` over it.
    fn store_over(chunk: &[u8], codec: u8, dxgi: u32) -> Vec<u8> {
        let etpf = (0x100 + chunk.len()).next_multiple_of(0x100);
        let mut d = vec![0u8; etpf + 16 + 16 + 8];
        d[..4].copy_from_slice(b"RDS5");
        put(&mut d, 4, 2); // chunk 0 + end sentinel
        put(&mut d, 8, 4096); // virtual size
        put(&mut d, 16, (etpf >> 8) as u32);
        put(&mut d, 0x18, 1 | (u32::from(codec) << 24)); // file 0x100, virtual 0
        put(&mut d, 0x20, (etpf >> 8) as u32 | (1 << 30)); // end: virtual 0x1000
        d[0x100..0x100 + chunk.len()].copy_from_slice(&chunk);
        d[etpf..etpf + 4].copy_from_slice(b"ETPF");
        put(&mut d, etpf + 8, 1);
        put(&mut d, etpf + 16, 3 | (3 << 14)); // 4x4, 1 mip
        put(&mut d, etpf + 20, (dxgi << 3) | (1 << 24)); // tile mode 1
        put(&mut d, etpf + 32, 0xDEAD_BEEF);
        d
    }

    #[test]
    fn bc1_block_to_rgba_both_orderings() {
        // c0 = white > c1 = black: 4 colours. Pixel k uses index k % 4.
        let block = [0xFF, 0xFF, 0x00, 0x00, 0xE4, 0xE4, 0xE4, 0xE4];
        let px = bcpack::ttpf::bc1_to_rgba(&block, 1, 1, 4, 4);
        assert_eq!(&px[..16], &[255, 255, 255, 255, 0, 0, 0, 255, 170, 170, 170, 255, 85, 85, 85, 255]);
        // c0 = black <= c1 = white: 3 colours + transparent black, clipped to 3x1.
        let block = [0x00, 0x00, 0xFF, 0xFF, 0xE4, 0, 0, 0];
        let px = bcpack::ttpf::bc1_to_rgba(&block, 1, 1, 3, 1);
        assert_eq!(px, [0, 0, 0, 255, 255, 255, 255, 255, 127, 127, 127, 255]);
        assert_eq!(&bcpack::ttpf::bc1_to_rgba(&block, 1, 1, 4, 1)[12..], &[0, 0, 0, 0]);
    }

    #[test]
    fn estimates_payload_after_largest_mip_drops() {
        let tex = Texture {
            index: 0,
            hash: 0,
            width: 9,
            height: 5,
            mips: 3,
            dxgi: 71,
            array_size: 3,
            tile_mode: 3,
            vstart: 0,
        };
        assert_eq!(mip_payload_budget(&tex), Some([192, 48, 24, 24]));
        assert_eq!(mip_payload_budget(&Texture { dxgi: 98, ..tex }), Some([384, 96, 48, 48]));
        assert_eq!(mip_payload_budget(&Texture { dxgi: 87, ..tex }), Some([660, 120, 24, 24]));
        assert_eq!(mip_payload_budget(&Texture { mips: 1, ..tex }), Some([144; 4]));
        assert_eq!(mip_payload_budget(&Texture { mips: 8, ..tex }), None);
        assert_eq!(mip_payload_budget(&Texture { tile_mode: 9, ..tex }), None);
        assert_eq!(mip_payload_budget(&Texture { dxgi: 0, ..tex }), None);
    }

    #[test]
    fn exports_bcpack_bc1_texture() {
        let (bytes, chunk) = synthetic_store();
        let store = Store::from_bytes(bytes).unwrap();
        assert_eq!(store.chunks.len(), 1);
        assert_eq!(store.chunks[0].codec, CODEC_BC1);
        let texs = store.textures().unwrap();
        let t = &texs[0];
        assert_eq!((t.width, t.height, t.mips, dxgi_name(t.dxgi), t.hash), (4, 4, 1, "BC1", 0xDEAD_BEEF));

        let blocks = bcpack::bc1::decode(&chunk, 512, &bcpack::bc1::Window::default_table(), &bcpack::bc1::QuadTable::default_table()).unwrap().blocks;
        let (dds, _) = export(&store, t, false, 0).unwrap();
        assert_eq!(&dds[..4], b"DDS ");
        assert_eq!(&dds[128..], &blocks[..8]);
        let (png, _) = export(&store, t, true, 0).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn exports_bc5_texture_dx10_and_rg_png() {
        // Stored 4 KiB tile, element 0 = one BC5 block: red flat 255, green flat 0.
        let mut chunk = vec![0u8; 4096];
        chunk[..2].copy_from_slice(&[255, 255]);
        let store = Store::from_bytes(store_over(&chunk, CODEC_STORED, 83)).unwrap();
        let t = &store.textures().unwrap()[0];
        let (dds, unfitted) = export(&store, t, false, 0).unwrap();
        assert_eq!((&dds[84..88], &dds[128..132], unfitted), (&b"DX10"[..], &83u32.to_le_bytes()[..], 0));
        assert_eq!(&dds[148..], &chunk[..16]);
        let rgba = bc16_to_rgba(&chunk[..16], 83, 1, 3, 2);
        assert_eq!(rgba, [255, 0, 0, 255].repeat(6));
        assert!(export(&store, t, false, 1).is_err());
        let mut px = [255, 128, 0, 255, 128, 128, 0, 255];
        rebuild_z(&mut px);
        assert_eq!(px, [255, 128, 128, 255, 128, 128, 255, 255]);
        assert_eq!(&material_png(&store, t).unwrap()[1..4], b"PNG");
    }

    #[test]
    fn lazy_store_reads_chunks_on_demand_and_rejects_zeros() {
        let (bytes, _) = synthetic_store();
        let path = std::env::temp_dir().join(format!("forge9-lazy-{}.ttpf", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let mut s = LazyStore::open(&path).unwrap();
        assert!(s.store.data[0x100..0x200].iter().all(|&b| b == 0));
        let t = s.textures[0].clone();
        s.load(&t).unwrap();
        assert_eq!(export(&s.store, &t, false, 0).unwrap(), export(&Store::from_bytes(bytes.clone()).unwrap(), &t, false, 0).unwrap());
        let (mut holed, etpf) = (bytes, s.store.etpf_off as usize);
        holed[0x100..etpf].fill(0);
        std::fs::write(&path, &holed).unwrap();
        let err = LazyStore::open(&path).and_then(|mut s| s.load(&t)).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(format!("{err}").contains("all zeros"));
    }

    #[test]
    fn exports_bc4_texture_dx10_and_grey_png() {
        // Stored 4 KiB tile, element 0 = one BC4 block: flat 200.
        let mut chunk = vec![0u8; 4096];
        chunk[..2].copy_from_slice(&[200, 200]);
        let store = Store::from_bytes(store_over(&chunk, CODEC_STORED, 80)).unwrap();
        let t = &store.textures().unwrap()[0];
        let (dds, _) = export(&store, t, false, 0).unwrap();
        assert_eq!((&dds[84..88], &dds[128..132]), (&b"DX10"[..], &80u32.to_le_bytes()[..]));
        assert_eq!(&dds[148..], &chunk[..8]);
        assert_eq!(bc4_to_rgba(&chunk[..8], 1, 3, 2), [200, 200, 200, 255].repeat(6));
        assert_eq!(&material_png(&store, t).unwrap()[1..4], b"PNG");
    }

    #[test]
    fn parses_edup_table() {
        let mut d = b"EDUP".to_vec();
        d.extend(2u32.to_le_bytes());
        for v in [1u64, 10, 5, 50, 99] { d.extend(v.to_le_bytes()); } // 2 records, then trailing data
        assert_eq!(edup(&d).unwrap(), HashMap::from([(1, 10), (5, 50)]));
        assert!(edup(&d[..20]).is_none());
        assert!(edup(b"PUDE\0\0\0\0").is_none());
    }

    #[test]
    fn mip0_placement_16b() {
        // 256x256 BC5, 9 mips, 64 KiB tiles, 8 slices: tail group first, then mip 0 of each slice.
        assert_eq!(mip0_place_16b(0, 256, 256, 9, 8, 3), (8 * 0x10000 + 3 * 0x10000, 0x10000, 64, 64, 0, 0));
        // 4x4 with 2 mips: mip 0 in the 4 KiB tail tile, slot 7 -> 8 elements right.
        assert_eq!(mip0_place_16b(1, 4, 4, 2, 1, 0), (0, 4096, 16, 16, 8, 0));
    }
}
