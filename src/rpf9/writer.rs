//! RPF9 archive writer: in-place replacement and full repack.

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::Path;
use crate::rpf9::crypto::{CryptoContext, TAG_IV};
use crate::rpf9::header::HEADER_SIZE;
use crate::rpf9::reader::Archive;

/// Replace a single entry's payload in-place (same size only).
/// Writes a new archive file with the payload bytes swapped.
/// The encrypted TOC stays untouched: no encrypt needed.
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
    let archive = Archive::open(original, crypto)?;
    let entry = archive.entries().get(entry_index)
        .ok_or_else(|| anyhow::anyhow!("entry index {entry_index} out of range"))?;

    if !entry.is_zlib(original) {
        bail!("entry #{entry_index} is not zlib: use replace_inplace for raw entries");
    }

    let start = entry.offset as usize;
    let old_size = entry.size as usize;
    // The TOC is left as is, so the record's decoded size must stay true.
    if let Some(d) = entry.decoded_size.filter(|&d| d != new_data.len() as u64) {
        bail!("entry #{entry_index} declares {d} decoded bytes, replacement is {}: use `repack` to change the length", new_data.len());
    }

    let mut compressed = zlib_chain(new_data)?;
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

/// `data` as a zlib chain matching RPF9 layout: 64KB chunks, each padded to 16 bytes.
pub fn zlib_chain(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut compressed = Vec::new();
    for chunk in data.chunks(0x10000) {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(chunk)?;
        compressed.extend_from_slice(&encoder.finish()?);
        compressed.resize(compressed.len().next_multiple_of(16), 0);
    }
    Ok(compressed)
}

/// Full repack: rebuild archive with replaced entries at any size and write it to `output`.
pub fn repack(
    original: &[u8],
    crypto: &CryptoContext,
    replacements: &HashMap<usize, Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let out = repack_bytes(original, crypto, replacements)?;
    std::fs::write(output, &out).context("writing repacked archive")?;
    println!(
        "repacked {} entries ({} replaced) → {} ({} bytes)",
        Archive::open(original, crypto)?.entries().len(), replacements.len(), output.display(), out.len()
    );
    Ok(())
}

/// Decrypts TOC, lays out payloads, rebuilds entry records, re-encrypts TOC.
pub fn repack_bytes(
    original: &[u8],
    crypto: &CryptoContext,
    replacements: &HashMap<usize, Vec<u8>>,
) -> Result<Vec<u8>> {
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
            bail!("replacement for entry #{idx} is empty (0 bytes): this would produce an unreadable record");
        }
    }

    // Lay payloads out in their original physical order, so an unchanged archive repacks
    // byte-identically. An unchanged entry brings its whole original slot, up to the next entry,
    // so gaps and unreferenced bytes survive; bytes before the first entry are kept too.
    // `data_region` holds the bytes from `toc_end` on.
    let entries = archive.entries();
    let archive_end = original.len() - header.trailer_size as usize;
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&i| entries[i].offset);
    let first = order.first().map_or(archive_end, |&i| entries[i].offset as usize);
    let mut data_region = original[toc_end..first].to_vec();
    let mut new_offsets = vec![0u64; entries.len()];
    let mut new_sizes = vec![0u64; entries.len()];

    for (k, &i) in order.iter().enumerate() {
        let entry = &entries[i];
        // ponytail: alignment inferred from the original offset, capped at 4 KiB; the real rule is unknown
        let align = 1usize << entry.offset.trailing_zeros().min(12);
        let pos = (toc_end + data_region.len()).next_multiple_of(align);
        data_region.resize(pos - toc_end, 0);
        new_offsets[i] = pos as u64;
        if let Some(new_data) = replacements.get(&entry.index) {
            new_sizes[i] = new_data.len() as u64;
            data_region.extend_from_slice(new_data);
        } else {
            let start = entry.offset as usize;
            let next = order[k + 1..].iter().map(|&j| entries[j].offset as usize).find(|&o| o > start).unwrap_or(archive_end);
            new_sizes[i] = entry.size;
            data_region.extend_from_slice(&original[start..next.max(start + entry.size as usize)]);
        }
        let pad = (16 - (data_region.len() % 16)) % 16;
        data_region.resize(data_region.len() + pad, 0);
    }

    // Rebuild 24-byte entry records in plaintext TOC; an unchanged entry keeps its decoded size.
    let Some((entry_start, count)) = crate::rpf9::entry::detect(
        &toc_plain, toc_end as u64, archive_end as u64, header.entry_count as usize,
    ).filter(|&(_, n)| n == entries.len()) else {
        bail!("TOC layout has no 24-byte entry records to rewrite (compact content archive?): repack unsupported");
    };
    for (i, entry) in entries.iter().enumerate().take(count) {
        let decoded = match replacements.get(&entry.index) {
            Some(data) => crate::rpf9::reader::decode(data)?.len() as u64,
            None => entry.decoded_size.unwrap_or(new_sizes[i]),
        };
        let record_at = entry_start + i * crate::rpf9::entry::RECORD;
        crate::rpf9::entry::encode_location(
            &mut toc_plain[record_at..record_at + crate::rpf9::entry::RECORD],
            new_offsets[i],
            new_sizes[i],
            decoded,
        );
    }

    let mut toc_cipher = toc_plain;
    schedule.encrypt_cbc(&mut toc_cipher, &TAG_IV);

    let trailer = &original[archive_end..];

    let mut out = Vec::with_capacity(toc_end + data_region.len() + trailer.len());
    out.extend_from_slice(&original[..HEADER_SIZE]);
    out.extend_from_slice(&toc_cipher);
    out.extend_from_slice(&data_region);
    out.extend_from_slice(trailer);
    Ok(out)
}
