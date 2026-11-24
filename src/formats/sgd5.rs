//! SGD5 shader databases (shaders.rpf).
//!
//! Records are found by their constant marker 0x000DDCE7. Layout around it:
//! `u8 name_len (incl. NUL), name, NUL, u8 tag, u32 csize, u32 0x000DDCE7,
//! u32 csize, u32 dsize`, then `csize` bytes of raw deflate holding a DXBC
//! container.

use anyhow::{bail, Result};
use super::u32_at;

pub const MARKER: [u8; 4] = 0x000D_DCE7u32.to_le_bytes();

#[derive(Debug, Clone)]
pub struct Shader {
    /// Offset of the record's length byte.
    pub offset: usize,
    pub tag: u8,
    pub name: String,
    pub csize: u32,
    pub dsize: u32,
}

impl Shader {
    /// Byte range of the raw-deflate DXBC payload.
    pub fn payload(&self) -> std::ops::Range<usize> {
        let start = self.offset + 1 + self.name.len() + 1 + 1 + 16;
        start..start + self.csize as usize
    }

    /// Inflates the record's DXBC container from the SGD5 table it was parsed from.
    pub fn dxbc(&self, data: &[u8]) -> Result<Vec<u8>> {
        use std::io::Read;
        let raw = data.get(self.payload()).ok_or_else(|| anyhow::anyhow!("record {} runs past the table", self.name))?;
        let mut out = Vec::with_capacity(self.dsize as usize);
        flate2::read::DeflateDecoder::new(raw).read_to_end(&mut out)?;
        Ok(out)
    }

    /// Pipeline stage named in the record (`PS`, `VS`, `CS`, `MS`, ...): the first
    /// `_`-separated token that is a stage, else a stage prefix, else `?`.
    pub fn stage(&self) -> &'static str {
        const STAGES: [&str; 8] = ["PS", "VS", "CS", "MS", "AS", "GS", "HS", "DS"];
        let base = self.name.rsplit(':').next().unwrap_or(&self.name).to_ascii_uppercase();
        base.split('_').find_map(|t| STAGES.into_iter().find(|s| t == *s))
            .or_else(|| STAGES.into_iter().find(|s| base.starts_with(s)))
            .unwrap_or("?")
    }
}

pub fn tag_name(tag: u8) -> &'static str {
    match tag {
        0x32 => "shader",
        0x3C => "compute",
        0x41 => "mesh/material",
        _ => "other",
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<Shader>> {
    if !data.starts_with(b"SGD5") { bail!("not an SGD5 database (no SGD5 magic)"); }
    let mut shaders = Vec::new();
    let mut at = 0;
    while let Some(found) = data[at..].windows(4).position(|w| w == MARKER) {
        let k = at + found;
        at = k + 4;
        if let Some(s) = record_at(data, k) { shaders.push(s); }
    }
    Ok(shaders)
}

fn record_at(data: &[u8], marker: usize) -> Option<Shader> {
    let nul = marker.checked_sub(6)?;
    let tag = data[marker - 5];
    let csize = u32_at(data, marker - 4)?;
    if data[nul] != 0 || u32_at(data, marker + 4)? != csize || csize == 0 { return None; }
    let dsize = u32_at(data, marker + 8)?;
    // Walk back over printable name bytes; the length byte is the one that
    // equals its distance to the NUL. Keep the furthest such byte.
    let mut len_at = None;
    for j in (nul.saturating_sub(255)..nul).rev() {
        if data[j] as usize == nul - j && nul - j >= 4 { len_at = Some(j); }
        if !(0x20..0x7f).contains(&data[j]) { break; }
    }
    let j = len_at?;
    let name = std::str::from_utf8(&data[j + 1..nul]).ok()?.to_string();
    Some(Shader { offset: j, tag, name, csize, dsize })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_records() {
        let mut d = b"SGD5\0\0\0\0".to_vec();
        let name = b"PS_Blit_8b8e4721_Wrapped";
        d.push(name.len() as u8 + 1);
        d.extend_from_slice(name);
        d.extend_from_slice(&[0, 0x32]);
        for v in [3u32, 0x000DDCE7, 3, 10] { d.extend_from_slice(&v.to_le_bytes()); }
        d.extend_from_slice(&[1, 2, 3]);
        let s = parse(&d).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].name.as_str(), s[0].stage(), s[0].tag, s[0].dsize), ("PS_Blit_8b8e4721_Wrapped", "PS", 0x32, 10));
        assert_eq!(&d[s[0].payload()], &[1, 2, 3]);
        let stage = |n: &str| Shader { name: n.into(), ..s[0].clone() }.stage();
        assert_eq!((stage("MSR_VS_Cascade_1_Wrapped"), stage("effect:MS_VS_Bar"), stage("VSSpotLight_1")), ("VS", "MS", "VS"));
    }
}
