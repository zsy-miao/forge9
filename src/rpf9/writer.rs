//! RPF9 archive writer — in-place replacement and full repack.

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::Path;
use crate::rpf9::crypto::{CryptoContext, TAG_IV};
use crate::rpf9::header::HEADER_SIZE;
use crate::rpf9::reader::Archive;

/// Replace a single entry's payload in-place (same size only).
/// Writes a new archive file with the payload bytes swapped.
/// The encrypted TOC stays untouched — no encrypt needed.
pub fn replace_inplace(
    original: &[u8],
    crypto: &CryptoContext,
    entry_index: usize,
    new_payload: &[u8],
    output: &Path,
) -> Result<()> {
    let archive = Archive::open(original, crypto)?;
    let entry = archive.entries().get(entry_index)
        .ok_or_else(|| anyhow::anyhow!("entry index {entry_index} out of range"))?;

    let start = entry.offset as usize;
    let end = start + entry.size as usize;
    let old_size = end - start;

    if new_payload.len() != old_size {
        bail!(
            "in-place replacement requires same size: entry is {} bytes, replacement is {} bytes. \
             Use `repack` for different-sized replacements.",
            old_size, new_payload.len()
        );
    }
    if end > original.len() {
        bail!("entry overflows archive bounds");
    }

    let mut out = original.to_vec();
    out[start..end].copy_from_slice(new_payload);
    std::fs::write(output, &out).context("writing output archive")?;
    println!(
        "replaced entry #{entry_index} ({old_size} bytes) at offset {start:#x} → {}",
        output.display()
    );
    Ok(())
}

/// Replace a payload with zlib-compressed content (same compressed size).
/// Compresses `new_data` into 64KB zlib chunks, verifies the total
/// compressed size matches, then swaps in-place.
pub fn replace_zlib_inplace(
    original: &[u8],
    crypto: &CryptoContext,
    entry_index: usize,
    new_data: &[u8],
    output: &Path,
) -> Result<()> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let archive = Archive::open(original, crypto)?;
    let entry = archive.entries().get(entry_index)
        .ok_or_else(|| anyhow::anyhow!("entry index {entry_index} out of range"))?;

    if !entry.is_zlib(original) {
        bail!("entry #{entry_index} is not zlib — use replace_inplace for raw entries");
    }

    let start = entry.offset as usize;
    let old_size = entry.size as usize;

    // Compress into 64KB chunks with 16-byte alignment, matching RPF9 layout
    let mut compressed = Vec::new();
    for chunk in new_data.chunks(0x10000) {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(chunk)?;
        let zchunk = encoder.finish()?;
        compressed.extend_from_slice(&zchunk);
        let pad = (16 - (compressed.len() % 16)) % 16;
        compressed.resize(compressed.len() + pad, 0);
    }

    if compressed.len() > old_size {
        bail!(
            "compressed replacement ({} bytes) exceeds original slot ({} bytes). \
             Try higher compression or smaller data. Full repack needed for size changes.",
            compressed.len(), old_size
        );
    }

    // Pad to exact original size (zero-fill remainder)
    compressed.resize(old_size, 0);

    let mut out = original.to_vec();
    out[start..start + old_size].copy_from_slice(&compressed);
    std::fs::write(output, &out).context("writing output archive")?;
    println!(
        "replaced entry #{entry_index} zlib payload ({old_size} bytes) → {}",
        output.display()
    );
    Ok(())
}

/// Full repack: rebuild archive with replaced entries at any size.
/// Decrypts TOC, lays out payloads, rebuilds entry records,
/// re-encrypts TOC, writes the new archive.
pub fn repack(
    original: &[u8],
    crypto: &CryptoContext,
    replacements: &HashMap<usize, Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let archive = Archive::open(original, crypto)?;
    let header = archive.header().clone();
    let key_index = archive.key_index();
    let schedule = crypto.schedule(key_index)?;

    let toc_end = header.payload_start as usize;
    let original_toc_cipher = &original[HEADER_SIZE..toc_end];
    let mut toc_plain = original_toc_cipher.to_vec();
    schedule.decrypt_cbc(&mut toc_plain, &TAG_IV);

    for (&idx, data) in replacements {
        if data.is_empty() {
            bail!("replacement for entry #{idx} is empty (0 bytes) — this would produce an unreadable record");
        }
    }

    let entries = archive.entries();
    let mut payloads: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    for entry in entries {
        if let Some(new_data) = replacements.get(&entry.index) {
            payloads.push(new_data.clone());
        } else {
            let start = entry.offset as usize;
            let end = (start + entry.size as usize).min(original.len());
            payloads.push(original[start..end].to_vec());
        }
    }

    // Lay payloads out in their original physical order, so multi-page zlib
    // streams stay adjacent and an unchanged archive repacks byte-identically.
    // `data_region` holds the bytes from `toc_end` on.
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&i| entries[i].offset);
    let mut data_region = Vec::new();
    let mut new_offsets = vec![0u64; entries.len()];
    let mut new_sizes = vec![0u64; entries.len()];

    for &i in &order {
        // ponytail: alignment inferred from the original offset, capped at 4 KiB; the real rule is unknown
        let align = 1usize << entries[i].offset.trailing_zeros().min(12);
        let pos = (toc_end + data_region.len()).next_multiple_of(align);
        data_region.resize(pos - toc_end, 0);
        new_offsets[i] = pos as u64;
        new_sizes[i] = payloads[i].len() as u64;
        data_region.extend_from_slice(&payloads[i]);
        let pad = (16 - (data_region.len() % 16)) % 16;
        data_region.resize(data_region.len() + pad, 0);
    }

    // Rebuild 24-byte entry records in plaintext TOC
    let archive_total = toc_end as u64 + data_region.len() as u64 + header.trailer_size as u64;
    let (entry_start, run) = crate::rpf9::entry::detect(
        &toc_plain, archive_total, header.entry_count as usize,
    );
    let skip = if header.entry_count > 0 && run == header.entry_count as usize + 1 { 1 } else { 0 };

    for (i, _entry) in entries.iter().enumerate() {
        let record_at = entry_start + (i + skip) * crate::rpf9::entry::RECORD;
        if record_at + crate::rpf9::entry::RECORD > toc_plain.len() { break; }
        crate::rpf9::entry::encode_location(
            &mut toc_plain[record_at..record_at + crate::rpf9::entry::RECORD],
            new_offsets[i],
            new_sizes[i],
        );
    }

    let mut toc_cipher = toc_plain;
    schedule.encrypt_cbc(&mut toc_cipher, &TAG_IV);

    let trailer_start = original.len() - header.trailer_size as usize;
    let trailer = &original[trailer_start..];

    let mut out = Vec::with_capacity(toc_end + data_region.len() + trailer.len());
    out.extend_from_slice(&original[..HEADER_SIZE]);
    out.extend_from_slice(&toc_cipher);
    out.extend_from_slice(&data_region);
    out.extend_from_slice(trailer);

    std::fs::write(output, &out).context("writing repacked archive")?;
    println!(
        "repacked {} entries ({} replaced) → {} ({} bytes)",
        entries.len(), replacements.len(), output.display(), out.len()
    );
    Ok(())
}
