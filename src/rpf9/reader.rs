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

        let mut best: Option<(usize, usize, Vec<u8>)> = None;
        let mut best_key = 0;
        for index in 0..crypto.key_count() {
            let schedule = crypto.schedule(index)?;
            let mut plain = ciphertext.to_vec();
            schedule.decrypt_cbc(&mut plain, &TAG_IV);
            let (start, run) = entry::detect(&plain, data.len() as u64, declared);
            let replace = match &best {
                None => true,
                Some((best_run, ..)) => run > *best_run,
            };
            if replace {
                best = Some((run, start, plain));
                best_key = index;
            }
        }
        let (run, start, plain) = best.expect("at least one key exists");
        if run == 0 && declared > 0 {
            bail!("no table records recovered with any key");
        }
        let skip = if declared > 0 && run == declared + 1 { 1 } else { 0 };
        let count = if declared == 0 { run } else { declared.min(run) };
        let trailer = header.trailer_size as u64;
        let archive_end = (data.len() as u64).saturating_sub(trailer);
        let entries = entry::parse(
            &plain,
            start + skip * entry::RECORD,
            run.saturating_sub(skip),
            count,
            archive_end,
        );

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
        if entry.is_zlib(self.data) {
            inflate_zlib_chain(raw)
        } else {
            Ok(vec![raw.to_vec()])
        }
    }
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
