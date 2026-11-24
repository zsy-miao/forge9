//! XBCH: the zlib-wrapped native program inside an Xbox DXBC container.
//!
//! Path: DXBC chunk `DXIL` → program header (0x18 bytes) → `XDAT` table of
//! `u32 id, u32 offset, u32 size` (offsets from the program header) → section
//! 0x0D, one per hardware-stage variant: `"XBCH", u32 version (3), u64
//! compressed size, u64 raw size`, then a zlib stream that inflates to FSSM.

use anyhow::{bail, Context, Result};
use std::io::Read;
use super::{u32_at, u64_at};

pub const XDAT_XBCH: u32 = 0x0D;

/// Inflates one XBCH blob.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    if !data.starts_with(b"XBCH") { bail!("not an XBCH blob"); }
    let csize = u64_at(data, 8).context("XBCH header truncated")? as usize;
    let raw = u64_at(data, 0x10).context("XBCH header truncated")? as usize;
    let stream = data.get(0x18..0x18 + csize).context("XBCH stream truncated")?;
    let mut out = Vec::with_capacity(raw);
    flate2::read::ZlibDecoder::new(stream).read_to_end(&mut out)?;
    if out.len() != raw { bail!("XBCH inflated to {} bytes, header says {raw}", out.len()); }
    Ok(out)
}

/// The XBCH blobs (section 0x0D) of a DXBC container, in XDAT order.
pub fn sections(dxbc: &[u8]) -> Result<Vec<&[u8]>> {
    if !dxbc.starts_with(b"DXBC") { bail!("not a DXBC container"); }
    let count = u32_at(dxbc, 0x1C).context("DXBC header truncated")? as usize;
    let prog = (0..count)
        .filter_map(|i| u32_at(dxbc, 0x20 + 4 * i))
        .map(|o| o as usize)
        .find(|&o| dxbc.get(o..o + 4) == Some(b"DXIL"))
        .map(|o| o + 8)
        .context("no DXIL chunk")?;
    let xdat = prog + 0x18;
    if dxbc.get(xdat..xdat + 4) != Some(b"XDAT") { bail!("DXIL chunk has no XDAT table"); }
    let n = u32_at(dxbc, xdat + 4).context("XDAT truncated")? as usize;
    let mut out = Vec::new();
    for i in 0..n {
        let at = xdat + 12 + 12 * i;
        let (id, off, size) = (u32_at(dxbc, at), u32_at(dxbc, at + 4), u32_at(dxbc, at + 8));
        let (Some(id), Some(off), Some(size)) = (id, off, size) else { bail!("XDAT table truncated") };
        if id != XDAT_XBCH { continue; }
        let start = prog + off as usize;
        out.push(dxbc.get(start..start + size as usize).context("XBCH section out of bounds")?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn unwraps_container() {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(b"FSSM-payload").unwrap();
        let z = z.finish().unwrap();
        let mut xbch = b"XBCH\x03\0\0\0".to_vec();
        xbch.extend_from_slice(&(z.len() as u64).to_le_bytes());
        xbch.extend_from_slice(&12u64.to_le_bytes());
        xbch.extend_from_slice(&z);

        // DXBC with one chunk at 0x24: "DXIL" + size, program header, XDAT with one 0x0D section.
        let mut d = b"DXBC".to_vec();
        d.resize(0x1C, 0);
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&0x24u32.to_le_bytes());
        d.extend_from_slice(b"DXIL\0\0\0\0");
        d.resize(0x2C + 0x18, 0);
        for v in [u32::from_le_bytes(*b"XDAT"), 1, 0x18, XDAT_XBCH, 0x30, xbch.len() as u32] { d.extend_from_slice(&v.to_le_bytes()); }
        d.extend_from_slice(&xbch);

        let secs = sections(&d).unwrap();
        assert_eq!(secs.len(), 1);
        assert_eq!(decompress(secs[0]).unwrap(), b"FSSM-payload");
    }
}
