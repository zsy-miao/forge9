//! RDS5 texture stores (`.ttpf`, standalone files next to the archives).
//!
//! Parsing, BCPack BC1 decoding (MDU rANS, codec 0x04), Gfx10 detiling and
//! DDS/PNG output come from bcpack-rs by Ahmet Dedeler
//! (https://github.com/Ahmet-Dedeler/bcpack-rs, MIT OR Apache-2.0).

use anyhow::{Context, Result};
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
        // Named but not yet decodable; see BCPack codec IDs.
        0x0C => "bcpack-bc3",
        0x10 => "bcpack-bc4",
        0x14 => "bcpack-bc5",
        0x18 => "bcpack-bc6h",
        0x1C => "bcpack-bc7",
        _ => "?",
    }
}

/// Mip 0 of a BC1 texture as DDS bytes, or PNG when `png`; also returns the
/// number of blocks that depend on unfitted window slots (0 = exact).
pub fn export(store: &Store, tex: &Texture, png: bool) -> Result<(Vec<u8>, usize)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn put(d: &mut [u8], at: usize, v: u32) {
        d[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// One 4 KiB BCPack BC1 chunk (512 blocks) and one 4x4 BC1 texture over it.
    fn synthetic_store() -> (Vec<u8>, Vec<u8>) {
        let (chunk, _) = bcpack::encode::synthetic_bc1(7, 512, 64);
        let etpf = (0x100 + chunk.len()).next_multiple_of(0x100);
        let mut d = vec![0u8; etpf + 16 + 16 + 8];
        d[..4].copy_from_slice(b"RDS5");
        put(&mut d, 4, 2); // chunk 0 + end sentinel
        put(&mut d, 8, 4096); // virtual size
        put(&mut d, 16, (etpf >> 8) as u32);
        put(&mut d, 0x18, 1 | (u32::from(CODEC_BC1) << 24)); // file 0x100, virtual 0
        put(&mut d, 0x20, (etpf >> 8) as u32 | (1 << 30)); // end: virtual 0x1000
        d[0x100..0x100 + chunk.len()].copy_from_slice(&chunk);
        d[etpf..etpf + 4].copy_from_slice(b"ETPF");
        put(&mut d, etpf + 8, 1);
        put(&mut d, etpf + 16, 3 | (3 << 14)); // 4x4, 1 mip
        put(&mut d, etpf + 20, (71 << 3) | (1 << 24)); // BC1_UNORM, tile mode 1
        put(&mut d, etpf + 32, 0xDEAD_BEEF);
        (d, chunk)
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
        let (dds, _) = export(&store, t, false).unwrap();
        assert_eq!(&dds[..4], b"DDS ");
        assert_eq!(&dds[128..], &blocks[..8]);
        let (png, _) = export(&store, t, true).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
