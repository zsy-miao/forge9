//! TOC entry parsing from decrypted table regions.

pub const RECORD: usize = 24;

#[derive(Debug, Clone)]
pub struct Entry {
    pub index: usize,
    pub offset: u64,
    /// Stored bytes: the payload as it sits in the archive, raw or as a zlib chain.
    pub size: u64,
    /// Declared content length (24-byte records only): what the payload inflates to, or its
    /// unpadded length when raw.
    pub decoded_size: Option<u64>,
    /// Compact records only: `+0x00` and `+0x10`, meanings not established.
    pub size_hint: u32,
    pub aux: u32,
    pub flags: u32,
    /// joaat of the entry's name, from the TOC's name hash column (see `name_hashes`).
    pub name_hash: Option<u32>,
    pub raw: [u8; RECORD],
}

impl Entry {
    pub fn is_zlib(&self, data: &[u8]) -> bool {
        data.get(self.offset as usize..).is_some_and(zlib_head)
    }

    /// Content length of a payload that is not a zlib chain: the decoded size when the stored
    /// bytes are exactly that padded to 16, else the whole stored span (an unidentified codec).
    pub fn raw_len(&self) -> u64 {
        self.decoded_size.filter(|d| d.next_multiple_of(16) == self.size).unwrap_or(self.size)
    }

    pub fn payload_magic(&self, data: &[u8]) -> Option<[u8; 4]> {
        let off = self.offset as usize;
        data.get(off..off + 4).map(|s| [s[0], s[1], s[2], s[3]])
    }
}

/// True if `data` starts with a zlib stream header. Every zlib part in the archives opens with
/// CMF `0x78`; payloads of the unidentified codec can pass the bare header checksum.
pub fn zlib_head(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x78 && u16::from_be_bytes([data[0], data[1]]) % 31 == 0
}

const GRAPH_MAGIC: [u8; 16] = [
    0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x89, 0x09, 0xc5, 0xf2, 0xfa, 0x0d, 0xf7, 0x33,
];

pub fn has_graph_magic(plain: &[u8]) -> bool {
    plain.windows(GRAPH_MAGIC.len()).any(|w| w == GRAPH_MAGIC)
}

/// Decodes a 24-byte record `{u32 offset/16, u32 flags, u64 stored size/16, u64 decoded size}`
/// into `(byte offset, flags, stored bytes, decoded bytes)`. The decoded size is the exact
/// content length: the inflated length of a zlib chain, or the unpadded length of a raw payload.
fn decode(bytes: &[u8]) -> Option<(u64, u32, u64, u64)> {
    if bytes.len() < RECORD { return None; }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let quad = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let (units, stored) = (word(0), quad(8));
    (units != 0 && stored != 0).then(|| (u64::from(units) * 16, word(4), stored.saturating_mul(16), quad(16)))
}

/// Rewrites a record's location and sizes in place, keeping its flags.
pub fn encode_location(record: &mut [u8], offset: u64, stored: u64, decoded: u64) {
    record[0..4].copy_from_slice(&((offset / 16) as u32).to_le_bytes());
    record[8..16].copy_from_slice(&stored.div_ceil(16).to_le_bytes());
    record[16..24].copy_from_slice(&decoded.to_le_bytes());
}

/// Finds the 24-byte record table: the first 8-aligned start where `count` records in a row
/// decode with payloads inside `data_start..end` that do not overlap and a decoded size under
/// 4 GiB. `count` is the records
/// column's own count when the column directory has it, else `declared`. Column data follows the
/// directory, so the table starts no earlier than the last known descriptor's end plus the records
/// column's offset from the shared base.
pub fn detect(plain: &[u8], data_start: u64, end: u64, declared: usize) -> Option<(usize, usize)> {
    let (count, rec_off) = column(plain, RECORDS).map_or((declared, 0), |(n, off, _)| (n, off));
    if count == 0 { return None; }
    let dir_end = COLUMN_IDS.iter().filter_map(|id| plain.windows(8).position(|w| w == id.to_le_bytes())).max().map_or(0, |at| at + 24);
    let valid = |s: usize| {
        let mut spans = Vec::with_capacity(count);
        for i in 0..count {
            let (offset, _, stored, decoded) = decode(&plain[s + i * RECORD..])?;
            // ponytail: a decoded size of 4 GiB or more is taken as a misaligned start (an
            // `offset | flags` word read as a size); lift it if an archive holds such an entry.
            if offset < data_start || offset.saturating_add(stored) > end || decoded >> 32 != 0 { return None; }
            spans.push((offset, stored));
        }
        spans.sort_unstable();
        spans.windows(2).all(|w| w[0] == w[1] || w[0].0 + w[0].1 <= w[1].0).then_some(())
    };
    ((dir_end + rec_off).next_multiple_of(8)..).step_by(8).take_while(|&s| s + count * RECORD <= plain.len()).find(|&s| valid(s).is_some()).map(|s| (s, count))
}

/// Directory hash of the 20-byte entry section that content archives (e.g. the
/// nested RPF9s of story/level packs) carry instead of the 24-byte one:
/// `{u32 size_hint, u32 flags, u32 size/16, u32 offset/16, u32 aux}`.
/// Each payload starts with 16 zero bytes, which are not part of the entry.
const COMPACT_SECTION: [u8; 8] = 0xba18_0afb_232d_1b4bu64.to_le_bytes();
const COMPACT_RECORD: usize = 20;
const COMPACT_PREFIX: u64 = 16;

/// Finds the compact entry table: `(start, count)` of the first 8-aligned run
/// of `count` records after the section's directory descriptor whose payloads
/// lie in `data` past `data_start` behind the zero prefix.
pub fn detect_compact(plain: &[u8], data: &[u8], data_start: u64) -> Option<(usize, usize)> {
    let dir = plain.windows(8).position(|w| w == COMPACT_SECTION)?;
    let count = u32::from_le_bytes(plain.get(dir + 8..dir + 12)?.try_into().unwrap()) as usize;
    let valid = |at: usize| {
        let (_, blocks, units) = compact_fields(&plain[at..at + COMPACT_RECORD]);
        let (start, end) = (units * 16, (units + blocks) * 16);
        blocks > 1 && start >= data_start && end <= data.len() as u64
            && data[start as usize..(start + COMPACT_PREFIX) as usize].iter().all(|&b| b == 0)
    };
    ((dir + 24).next_multiple_of(8)..)
        .step_by(8)
        .take_while(|&s| s + count * COMPACT_RECORD <= plain.len())
        .find(|&s| (0..count).all(|i| valid(s + i * COMPACT_RECORD)))
        .map(|s| (s, count))
}

/// Column ids in the TOC's column directory, whose descriptors are `{u64 id, u32 count, u32 offset,
/// u64 stride}` with offsets from one shared base: the 24-byte records, the 20-byte records
/// (`COMPACT_SECTION`) and the u32 name hashes, one per record index. A u64 column (id
/// 0x73bcb72dce74f11f) holds a further per-record value whose hash function is not identified.
const RECORDS: u64 = 0x33f7_0dfa_f2c5_0989;
const NAME_HASHES: u64 = 0xb707_6b62_45d3_5fb5;
/// Every column id seen in a directory; the last one found bounds where the directory ends.
const COLUMN_IDS: [u64; 8] = [
    RECORDS, NAME_HASHES, 0xba18_0afb_232d_1b4b, 0x73bc_b72d_ce74_f11f,
    0xe0c1_2d7a_5d48_84c5, 0xb6f5_bdb2_c66d_a2e9, 0x2860_30a3_58a7_89fb, 0x9045_6c0f_bc4d_5746,
];

/// `(count, offset, stride)` of column `id`.
fn column(plain: &[u8], id: u64) -> Option<(usize, usize, u64)> {
    let at = plain.windows(8).position(|w| w == id.to_le_bytes())?;
    let d = plain.get(at + 8..at + 24)?;
    let word = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) as usize;
    Some((word(0), word(4), u64::from_le_bytes(d[8..16].try_into().unwrap())))
}

/// The name hash column, indexed like the records that start at `table_at`. Some archives
/// declare fewer hashes than records; the records past the column have none.
pub fn name_hashes(plain: &[u8], table_at: usize, compact: bool) -> Vec<u32> {
    let records = if compact { u64::from_le_bytes(COMPACT_SECTION) } else { RECORDS };
    let (Some((_, rec_off, _)), Some((count, off, 4))) = (column(plain, records), column(plain, NAME_HASHES)) else { return Vec::new() };
    let Some(at) = (table_at + off).checked_sub(rec_off) else { return Vec::new() };
    plain.get(at..at + count * 4).map_or(Vec::new(), |b| b.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect())
}

fn compact_fields(bytes: &[u8]) -> ([u32; 5], u64, u64) {
    let w: [u32; 5] = std::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
    (w, u64::from(w[2]), u64::from(w[3]))
}

pub fn parse_compact(plain: &[u8], table_at: usize, count: usize) -> Vec<Entry> {
    (0..count)
        .map(|index| {
            let at = table_at + index * COMPACT_RECORD;
            let (w, blocks, units) = compact_fields(&plain[at..at + COMPACT_RECORD]);
            let mut raw = [0u8; RECORD];
            raw[..COMPACT_RECORD].copy_from_slice(&plain[at..at + COMPACT_RECORD]);
            Entry {
                index,
                offset: units * 16 + COMPACT_PREFIX,
                size: blocks * 16 - COMPACT_PREFIX,
                decoded_size: None,
                size_hint: w[0],
                aux: w[4],
                flags: w[1],
                name_hash: None,
                raw,
            }
        })
        .collect()
}

pub fn parse(plain: &[u8], table_at: usize, count: usize) -> Vec<Entry> {
    (0..count)
        .filter_map(|index| {
            let raw: [u8; RECORD] = plain.get(table_at + index * RECORD..table_at + (index + 1) * RECORD)?.try_into().unwrap();
            let (offset, flags, size, decoded) = decode(&raw)?;
            Some(Entry { index, offset, size, decoded_size: Some(decoded), size_hint: 0, aux: 0, flags, name_hash: None, raw })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quads(q: [u64; 3]) -> Vec<u8> {
        q.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn records_carry_offset_stored_and_decoded_sizes() {
        // script.rpf: two raw nested RPF9s, flags 0, stored = decoded rounded up to 16
        let plain = [quads([0x100, 0xc2b9, 0xc2b90]), quads([0xc400, 0x3642b, 0x3642b0])].concat();
        assert_eq!(detect(&plain, 0x260, 0x4282b0, 2), Some((0, 2)));
        let entries = parse(&plain, 0, 2);
        assert_eq!((entries[0].offset, entries[0].size, entries[0].decoded_size), (0x1000, 0xc2b90, Some(0xc2b90)));
        assert_eq!((entries[1].offset, entries[1].size, entries[1].decoded_size), (0xc4000, 0x3642b0, Some(0x3642b0)));
    }

    fn words(w: &[u64]) -> Vec<u8> {
        w.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn two_part_record_found_behind_leading_words() {
        // Shape of a script.rpf child: a u64 before the table, then a one-part and a two-part
        // zlib entry. Flags 0xb00 share the first u64 with the offset; the two-part entry
        // declares 0x184d0 decoded bytes (a 64 KiB part and a 0x84d0 part) in 0x6390 stored.
        let flagged = |units: u64| units | 0xb00 << 32;
        let plain = words(&[0xd4, flagged(0x1200), 0x190, 0x2580, flagged(0x2300), 0x639, 0x184d0]);
        assert_eq!(detect(&plain, 0x260, 0x30000, 2), Some((8, 2)));
        let entries = parse(&plain, 8, 2);
        assert_eq!((entries[0].offset, entries[0].flags, entries[0].size, entries[0].decoded_size), (0x12000, 0xb00, 0x1900, Some(0x2580)));
        assert_eq!((entries[1].offset, entries[1].size, entries[1].decoded_size), (0x23000, 0x6390, Some(0x184d0)));
        assert_eq!(detect(&plain, 0x260, 0x22000, 2), None, "second payload past the end");
        // script.rpf/0: two words that decode as a record in bounds, but whose decoded size is
        // the real record's `offset | flags` word. The real table starts 16 bytes later.
        let plain = words(&[0x1000, 4, 0x5200 | 0x6000 << 32, 0x70b5, 0x293c30]);
        assert_eq!(detect(&plain, 0x260, 0xc2b90, 1), Some((16, 1)));
    }

    #[test]
    fn compact_layout_from_directory() {
        // story000_restofworld.rpf nested #19: directory descriptor, padding, one record
        let mut plain = vec![0u8; 0x60];
        plain[0..8].copy_from_slice(&COMPACT_SECTION);
        plain[8..12].copy_from_slice(&1u32.to_le_bytes());
        for (i, w) in [0x201bu32, 0xe601, 0xb5e, 0x100, 4].iter().enumerate() {
            plain[0x30 + i * 4..0x34 + i * 4].copy_from_slice(&w.to_le_bytes());
        }
        let mut data = vec![0u8; 0xd000];
        assert_eq!(detect_compact(&plain, &data, 0x2b0), Some((0x30, 1)));
        data[0x1008] = 1; // zero prefix broken
        assert_eq!(detect_compact(&plain, &data, 0x2b0), None);
        assert_eq!(detect_compact(&plain, &data[..0xc000], 0x2b0), None);
        let entries = parse_compact(&plain, 0x30, 1);
        assert_eq!((entries[0].offset, entries[0].size, entries[0].aux), (0x1010, 0xb5d0, 4));
    }

    #[test]
    fn name_hashes_follow_the_column_directory() {
        // story000_restofworld.rpf nested #1 shape: directory, then records and hashes at offsets
        // from a shared base (here 0x100, records at base + 0x40, hashes at base + 0x20).
        let mut plain = vec![0u8; 0x200];
        for (i, (id, count, off, stride)) in [(NAME_HASHES, 2u32, 0x20u32, 4u64), (u64::from_le_bytes(COMPACT_SECTION), 3, 0x40, 20)].into_iter().enumerate() {
            let d = 0x10 + i * 24;
            plain[d..d + 8].copy_from_slice(&id.to_le_bytes());
            plain[d + 8..d + 12].copy_from_slice(&count.to_le_bytes());
            plain[d + 12..d + 16].copy_from_slice(&off.to_le_bytes());
            plain[d + 16..d + 24].copy_from_slice(&stride.to_le_bytes());
        }
        plain[0x120..0x128].copy_from_slice(&[0x8b, 0x28, 0xf2, 0xd1, 0xcc, 0x4d, 0x93, 0x24]);
        assert_eq!(name_hashes(&plain, 0x140, true), [0xd1f2_288b, 0x2493_4dcc]);
        // No 24-byte records directory, so nothing for the indexed layout.
        assert!(name_hashes(&plain, 0x140, false).is_empty());
        plain[0x10..0x18].fill(0);
        assert!(name_hashes(&plain, 0x140, true).is_empty());
    }

    #[test]
    fn encode_location_keeps_flags() {
        let mut record = [quads([0x100 | 0xff00 << 32, 0x21, 0x6ed])].concat();
        encode_location(&mut record, 0x3000, 0x2a0, 0x900);
        assert_eq!(decode(&record), Some((0x3000, 0xff00, 0x2a0, 0x900)));
    }
}
