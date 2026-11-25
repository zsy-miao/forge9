//! High-level RPF9 archive reader with automatic key discovery.

use anyhow::{bail, Result};
use crate::rpf9::crypto::{CryptoContext, TAG_IV};
use crate::rpf9::entry::{self, Entry};
use crate::rpf9::header::{Header, HEADER_SIZE};

pub struct Archive<'a> {
    data: &'a [u8],
    header: Header,
    entries: Vec<Entry>,
    key_index: usize,
}

impl<'a> Archive<'a> {
    pub fn open(data: &'a [u8], crypto: &CryptoContext) -> Result<Self> {
        let header = Header::parse(data)?;
        let end = header.payload_start as usize;
        if end > data.len() || end <= HEADER_SIZE {
            bail!("payload_start {end:#x} out of bounds");
        }
        let ciphertext = &data[HEADER_SIZE..end];
        let declared = header.entry_count as usize;
        let archive_end = (data.len() as u64).saturating_sub(u64::from(header.trailer_size));

        // The key whose plaintext holds the record table (24-byte, else compact) with the most entries.
        let mut best: Option<(usize, usize, Vec<u8>, bool)> = None;
        let mut best_key = 0;
        for index in 0..crypto.key_count() {
            let schedule = crypto.schedule(index)?;
            let mut plain = ciphertext.to_vec();
            schedule.decrypt_cbc(&mut plain, &TAG_IV);
            let found = entry::detect(&plain, end as u64, archive_end, declared).map(|(s, n)| (s, n, false))
                .or_else(|| entry::detect_compact(&plain, data, end as u64).map(|(s, n)| (s, n, true)));
            let Some((start, run, compact)) = found else { continue };
            if best.as_ref().is_none_or(|(best_run, ..)| run > *best_run) {
                best = Some((run, start, plain, compact));
                best_key = index;
            }
        }
        let Some((run, start, plain, compact)) = best else {
            if declared > 0 { bail!("no table records recovered with any key"); }
            return Ok(Self { data, header, entries: Vec::new(), key_index: 0 });
        };
        let mut entries = if compact { entry::parse_compact(&plain, start, run) } else { entry::parse(&plain, start, run) };
        name(&mut entries, &entry::name_hashes(&plain, start, compact));
        Ok(Self { data, header, entries, key_index: best_key })
    }

    pub fn header(&self) -> &Header { &self.header }
    pub fn entries(&self) -> &[Entry] { &self.entries }
    pub fn key_index(&self) -> usize { self.key_index }
    pub fn data(&self) -> &[u8] { self.data }

    /// Extension guessed from the entry's content; zlib heads are inflated first.
    pub fn sniff(&self, entry: &Entry) -> &'static str {
        sniff_ext(&self.head(entry, 16))
    }

    /// First `n` content bytes of an entry, inflating zlib heads.
    pub fn head(&self, entry: &Entry, n: usize) -> Vec<u8> {
        use std::io::Read;
        let start = (entry.offset as usize).min(self.data.len());
        let end = (start + entry.size as usize).min(self.data.len());
        let raw = &self.data[start..end];
        if !entry.is_zlib(self.data) { return raw[..raw.len().min(n)].to_vec(); }
        let mut head = Vec::new();
        let _ = flate2::read::ZlibDecoder::new(raw).take(n as u64).read_to_end(&mut head);
        head
    }

    pub fn read_entry(&self, entry: &Entry) -> Result<Vec<u8>> {
        Ok(self.read_entry_blocks(entry)?.concat())
    }

    /// Entry content as its separately inflated zlib blocks (one block if stored raw).
    /// Paged resources keep their system and graphics segments on block boundaries.
    pub fn read_entry_blocks(&self, entry: &Entry) -> Result<Vec<Vec<u8>>> {
        let start = entry.offset as usize;
        let end = start + entry.size as usize;
        if end > self.data.len() {
            bail!("entry #{} overflows archive ({end:#x} > {:#x})", entry.index, self.data.len());
        }
        let raw = &self.data[start..end];
        if !entry.is_zlib(self.data) {
            return Ok(vec![raw[..entry.raw_len() as usize].to_vec()]);
        }
        // A resource of more than 64 KiB is one zlib stream per 64 KiB, all inside this entry's
        // stored span; the declared decoded size counts every part.
        let blocks = inflate_zlib_chain(raw)?;
        let total: usize = blocks.iter().map(Vec::len).sum();
        if let Some(d) = entry.decoded_size.filter(|&d| d != total as u64) {
            bail!("entry #{} inflates to {total:#x} bytes in {} parts, but its record declares {d:#x}", entry.index, blocks.len());
        }
        Ok(blocks)
    }
}

/// Sets each entry's name hash from the column; 0 is a placeholder (joaat of ""), not a name.
fn name(entries: &mut [Entry], hashes: &[u32]) {
    for e in entries { e.name_hash = hashes.get(e.index).copied().filter(|&h| h != 0); }
}

/// A payload stored as `raw`, as `read_entry` returns it.
pub fn decode(raw: &[u8]) -> Result<Vec<u8>> {
    Ok(if entry::zlib_head(raw) { inflate_zlib_chain(raw)?.concat() } else { raw.to_vec() })
}

fn inflate_zlib_chain(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    use std::io::Read;
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        if data[pos] != 0x78 {
            break;
        }
        let mut decoder = flate2::read::ZlibDecoder::new(&data[pos..]);
        let mut chunk = Vec::new();
        decoder.read_to_end(&mut chunk)?;
        let consumed = decoder.total_in() as usize;
        out.push(chunk);
        pos += consumed;
        let align = (16 - (pos % 16)) % 16;
        pos += align;
    }
    Ok(out)
}

pub fn sniff_ext(data: &[u8]) -> &'static str {
    if data.len() < 4 { return "bin"; }
    match &data[..4] {
        b"9FPR" => "rpf",
        b"PSIN" => "psin",
        b"RBF0" => "rbf",
        b"ADAT" => "awc",
        b"SGD5" => "sgd5",
        b"RDS5" => "rds5",
        b"GFX\x00" | b"CFX\x00" => "gfx",
        [0x3C, 0x3F, 0x78, 0x6D] => "xml",   // <?xm
        [0xEF, 0xBB, 0xBF, 0x3C] => "xml",   // BOM + <
        _ if data.starts_with(b"0DRP") => "ydr",
        _ if data.starts_with(b"KB2") => "bk2",
        _ if data.starts_with(b"DDS ") => "dds",
        _ if data.starts_with(b"\x89PNG") => "png",
        _ => "bin",
    }
}
