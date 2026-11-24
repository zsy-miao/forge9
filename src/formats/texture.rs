//! RDS5 texture stores (`.ttpf`, standalone files next to the archives).
//!
//! Parsing, BCPack decoding (MDU rANS; BC1, and BC3/BC5/BC7 from the
//! `bc5-bc7` branch), BC1 Gfx10 detiling and DDS/PNG output come from
//! bcpack-rs by Ahmet Dedeler (https://github.com/Ahmet-Dedeler/bcpack-rs,
//! MIT OR Apache-2.0). 16-byte-block pixels come from bcdec_rs.

use anyhow::{Context, Result};
use bcpack::{bc7, bcn, format::Mode};
pub use bcpack::ttpf::{Store, Texture, CODEC_BC1, CODEC_STORED, CODEC_ZLIB};

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

/// Mip 0 (of array slice `slice`) of a BC1/BC3/BC5/BC7 texture as DDS bytes,
/// or PNG when `png`; also returns the number of blocks bcpack-rs flags as
/// depending on undetermined table values (0 = exact).
pub fn export(store: &Store, tex: &Texture, png: bool, slice: u32) -> Result<(Vec<u8>, usize)> {
    if matches!(tex.dxgi, 76..=78 | 82..=84 | 97..=99) {
        return export_16b(store, tex, png, slice);
    }
    anyhow::ensure!(slice == 0, "texture {}: slices are only supported for BC3/BC5/BC7", tex.index);
    let window = bcpack::bc1::Window::default_table();
    let quads = bcpack::bc1::QuadTable::default_table();
    let m = bcpack::texture_mip0(store, tex, &window, &quads).with_context(|| format!("texture {}", tex.index))?;
    let bytes = if png {
        let rgba = bcpack::ttpf::bc1_to_rgba(&m.blocks, m.bw, m.bh, m.width as usize, m.height as usize);
        bcpack::ttpf::png_rgba(&rgba, m.width, m.height)?
    } else {
        bcpack::ttpf::dds_bc1(&m.blocks, m.width, m.height)
    };
    Ok((bytes, m.unfitted_blocks))
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

fn export_16b(store: &Store, tex: &Texture, png: bool, slice: u32) -> Result<(Vec<u8>, usize)> {
    anyhow::ensure!(tex.tile_mode <= 3, "texture {}: tile mode {} not supported", tex.index, tex.tile_mode);
    anyhow::ensure!(slice < tex.array_size, "texture {}: slice {slice} out of range ({} slices)", tex.index, tex.array_size);
    let (off, len, rw, rh, ox, oy) = mip0_place_16b(tex.tile_mode, tex.width, tex.height, tex.mips, tex.array_size, slice);
    let a = tex.vstart + off;
    let mut tiled = Vec::with_capacity(len as usize);
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
            (_, Some(m @ (Mode::Bc3 | Mode::Bc5))) => {
                let r = bcn::decode(store.chunk_bytes(ci, true)?, c.vlen as usize, bcn::Tables::default_for(m).unwrap()).map_err(|e| anyhow::anyhow!("chunk {ci}: {e:?}"))?;
                unfitted += r.unfitted_blocks;
                r.blocks
            }
            (x, _) => anyhow::bail!("chunk {ci}: codec {} in a 16-byte-block texture", codec_name(x)),
        };
        tiled.extend_from_slice(&bytes[f as usize..e as usize]);
    }
    let (rw, rh) = (rw as usize, rh as usize);
    let mut lin = vec![0u8; rw * rh * 16];
    for (i, d) in bcpack::ttpf::untile_order(tiled.len() / 16, rw, rh, pattern_16b(tex.tile_mode)).into_iter().enumerate() {
        if let Some(d) = d {
            lin[d * 16..d * 16 + 16].copy_from_slice(&tiled[i * 16..i * 16 + 16]);
        }
    }
    let (bw, bh) = (tex.width.div_ceil(4) as usize, tex.height.div_ceil(4) as usize);
    let blocks: Vec<u8> = (0..bh).flat_map(|y| lin[((y + oy as usize) * rw + ox as usize) * 16..][..bw * 16].iter().copied()).collect();
    let bytes = if png {
        let rgba = bc16_to_rgba(&blocks, tex.dxgi, bw, tex.width as usize, tex.height as usize);
        bcpack::ttpf::png_rgba(&rgba, tex.width, tex.height)?
    } else {
        dds_dx10(&blocks, tex.width, tex.height, tex.dxgi)
    };
    Ok((bytes, unfitted))
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
    }

    #[test]
    fn mip0_placement_16b() {
        // 256x256 BC5, 9 mips, 64 KiB tiles, 8 slices: tail group first, then mip 0 of each slice.
        assert_eq!(mip0_place_16b(0, 256, 256, 9, 8, 3), (8 * 0x10000 + 3 * 0x10000, 0x10000, 64, 64, 0, 0));
        // 4x4 with 2 mips: mip 0 in the 4 KiB tail tile, slot 7 -> 8 elements right.
        assert_eq!(mip0_place_16b(1, 4, 4, 2, 1, 0), (0, 4096, 16, 16, 8, 0));
    }
}
