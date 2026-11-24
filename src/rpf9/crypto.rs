//! RPF9 T-table block cipher — decrypt and encrypt.
//!
//! The cipher uses permuted S-boxes and T-tables extracted from the
//! runtime of a well-known retail title. It runs 13 rounds (14 round
//! keys including the initial whitening) in CBC mode with a fixed IV
//! for the first table region.
//!
//! Tables and keys are loaded at runtime from external binary blobs
//! so that no key material is compiled into this crate.

use anyhow::{bail, Context, Result};
use std::path::Path;

const STATE_SIZE: usize = 0x10F0;
const KEY_BLOB_SIZE: usize = 0x10F4;
const TABLES_SIZE: usize = 0x5000;
const KEY_MAGIC: [u8; 4] = [0x8E, 0x9B, 0x3B, 0x4E];

pub const TAG_IV: [u8; 16] = [
    0xC1, 0x7A, 0xAD, 0x4C, 0x53, 0xD6, 0x0E, 0xFA,
    0xD6, 0x01, 0xD6, 0xAE, 0xBA, 0x5F, 0xC6, 0x0E,
];

/// For each round output word: (T-table, input word, byte shift) of its four lookups.
const ROUND_TAPS: [[(usize, usize, u32); 4]; 4] = [
    [(10, 2, 8), (13, 3, 16), (0, 0, 24), (7, 1, 0)],
    [(1, 0, 16), (14, 3, 8), (4, 1, 24), (11, 2, 0)],
    [(5, 1, 16), (2, 0, 8), (8, 2, 24), (15, 3, 0)],
    [(9, 2, 16), (6, 1, 8), (12, 3, 24), (3, 0, 0)],
];

pub struct CryptoContext {
    tables: Vec<u8>,
    keys: Vec<u8>,
    inverse: InverseRound,
}

/// Inverse of one T-table round. Each T-table is an affine GF(2) map of an
/// unknown byte bijection, and the four tables feeding one output word span
/// all 32 bits, so a word splits back into its four input bytes by solving
/// a 32x32 linear system (precomputed as per-byte lookup tables).
struct InverseRound {
    /// XOR of T[t][0] over the word's four tables (the affine offset).
    offset: [u32; 4],
    /// solve[word][byte_pos][v]: coordinates of `v << 8*byte_pos`, one byte per tap.
    solve: Box<[[[u32; 256]; 4]; 4]>,
    /// coord_to_x[word][tap][coordinate byte] = T-table input byte.
    coord_to_x: [[[u8; 256]; 4]; 4],
    /// Inverses of the 16 initial S-boxes.
    sboxes: [[u8; 256]; 16],
}

impl InverseRound {
    fn build(tables: &[u8]) -> Result<Self> {
        let t = |table: usize, x: usize| {
            let at = 0x1000 + table * 0x400 + x * 4;
            u32::from_le_bytes(tables[at..at + 4].try_into().unwrap())
        };
        let mut inverse = Self {
            offset: [0; 4],
            solve: Box::new([[[0; 256]; 4]; 4]),
            coord_to_x: [[[0; 256]; 4]; 4],
            sboxes: std::array::from_fn(|i| invert_sbox(&tables[i * 0x100..(i + 1) * 0x100])),
        };
        for (word, taps) in ROUND_TAPS.iter().enumerate() {
            // XOR basis keyed by highest bit; `combo` tracks which of the
            // 32 chosen basis vectors (8 per tap) the reduced vector is made of.
            let mut pivots: [Option<(u32, u32)>; 32] = [None; 32];
            let mut found = 0;
            for (tap, &(table, ..)) in taps.iter().enumerate() {
                inverse.offset[word] ^= t(table, 0);
                let mut picked = 0;
                for x in 1..256 {
                    if picked == 8 {
                        break;
                    }
                    let (mut v, mut combo) = (t(table, x) ^ t(table, 0), 1u32 << (tap * 8 + picked));
                    for bit in (0..32).rev() {
                        if v >> bit & 1 == 0 {
                            continue;
                        }
                        match pivots[bit] {
                            Some((pv, pc)) => { v ^= pv; combo ^= pc; }
                            None => { pivots[bit] = Some((v, combo)); picked += 1; found += 1; break; }
                        }
                    }
                }
            }
            if found != 32 {
                bail!("T-tables for word {word} are not invertible (rank {found})");
            }
            let solve = |mut v: u32| {
                let mut coords = 0;
                for bit in (0..32).rev() {
                    if v >> bit & 1 == 1 {
                        let (pv, pc) = pivots[bit].unwrap();
                        v ^= pv;
                        coords ^= pc;
                    }
                }
                coords
            };
            for pos in 0..4 {
                for v in 0..256u32 {
                    inverse.solve[word][pos][v as usize] = solve(v << (pos * 8));
                }
            }
            for (tap, &(table, ..)) in taps.iter().enumerate() {
                let mut seen = [false; 256];
                for x in 0..256 {
                    let coords = solve(t(table, x) ^ t(table, 0));
                    let c = (coords >> (tap * 8)) as u8;
                    if coords != (c as u32) << (tap * 8) || seen[c as usize] {
                        bail!("T-table {table} is not an affine bijection");
                    }
                    seen[c as usize] = true;
                    inverse.coord_to_x[word][tap][c as usize] = x as u8;
                }
            }
        }
        Ok(inverse)
    }

    /// Recover the round input words from round output words (round key already removed).
    fn apply(&self, out: [u32; 4]) -> [u32; 4] {
        let mut state = [0u32; 4];
        for (word, taps) in ROUND_TAPS.iter().enumerate() {
            let v = out[word] ^ self.offset[word];
            let s = &self.solve[word];
            let coords = s[0][(v & 0xff) as usize]
                ^ s[1][(v >> 8 & 0xff) as usize]
                ^ s[2][(v >> 16 & 0xff) as usize]
                ^ s[3][(v >> 24) as usize];
            for (tap, &(_, input, shift)) in taps.iter().enumerate() {
                let x = self.coord_to_x[word][tap][(coords >> (tap * 8) & 0xff) as usize];
                state[input] |= (x as u32) << shift;
            }
        }
        state
    }
}

fn invert_sbox(sbox: &[u8]) -> [u8; 256] {
    let mut inv = [0u8; 256];
    for (i, &v) in sbox.iter().enumerate() {
        inv[v as usize] = i as u8;
    }
    inv
}

impl CryptoContext {
    pub fn load(data_dir: &Path) -> Result<Self> {
        let tables = std::fs::read(data_dir.join("tables.bin"))
            .context("reading tables.bin")?;
        let keys = std::fs::read(data_dir.join("keys.bin"))
            .context("reading keys.bin")?;
        if tables.len() != TABLES_SIZE {
            bail!("tables.bin: expected {TABLES_SIZE} bytes, got {}", tables.len());
        }
        if keys.is_empty() || keys.len() % KEY_BLOB_SIZE != 0 {
            bail!("keys.bin: expected a non-empty multiple of {KEY_BLOB_SIZE} bytes, got {}", keys.len());
        }
        if let Some(index) = keys.chunks(KEY_BLOB_SIZE).position(|blob| blob[..4] != KEY_MAGIC) {
            bail!("keys.bin: key {index} is not in the {KEY_MAGIC:02x?} family");
        }
        let inverse = InverseRound::build(&tables)?;
        Ok(Self { tables, keys, inverse })
    }

    /// Number of key blobs in keys.bin.
    pub fn key_count(&self) -> usize {
        self.keys.len() / KEY_BLOB_SIZE
    }

    pub fn schedule(&self, index: usize) -> Result<KeySchedule<'_>> {
        let count = self.key_count();
        if index >= count {
            bail!("key index {index} out of range (0..{count})");
        }
        let at = index * KEY_BLOB_SIZE;
        let blob = &self.keys[at..at + KEY_BLOB_SIZE];
        let mut round_keys = [0u32; 60];
        for (i, key) in round_keys.iter_mut().enumerate() {
            let o = 4 + i * 4;
            *key = u32::from_le_bytes([blob[o], blob[o + 1], blob[o + 2], blob[o + 3]]);
        }
        let mut sboxes = [0u8; STATE_SIZE - 0xF0];
        sboxes.copy_from_slice(&blob[4 + 0xF0..4 + STATE_SIZE]);
        Ok(KeySchedule {
            round_keys,
            inv_sboxes: std::array::from_fn(|i| invert_sbox(&sboxes[i * 0x100..(i + 1) * 0x100])),
            sboxes,
            tables: &self.tables,
            inverse: &self.inverse,
        })
    }
}

pub struct KeySchedule<'a> {
    round_keys: [u32; 60],
    sboxes: [u8; STATE_SIZE - 0xF0],
    inv_sboxes: [[u8; 256]; 16],
    tables: &'a [u8],
    inverse: &'a InverseRound,
}

impl<'a> KeySchedule<'a> {
    #[inline]
    fn sbox(&self, table: usize, value: u8) -> u8 {
        self.tables[table * 0x100 + value as usize]
    }

    #[inline]
    fn ttable(&self, table: usize, value: usize) -> u32 {
        let at = 0x1000 + table * 0x400 + value * 4;
        u32::from_le_bytes([
            self.tables[at],
            self.tables[at + 1],
            self.tables[at + 2],
            self.tables[at + 3],
        ])
    }

    pub fn decrypt_block(&self, input: &[u8; 16]) -> [u8; 16] {
        let rk = &self.round_keys;
        let mut w5 = u32::from_be_bytes([
            self.sbox(0, input[0]), self.sbox(1, input[1]),
            self.sbox(2, input[2]), self.sbox(3, input[3]),
        ]) ^ rk[0];
        let mut w6 = u32::from_be_bytes([
            self.sbox(4, input[4]), self.sbox(5, input[5]),
            self.sbox(6, input[6]), self.sbox(7, input[7]),
        ]) ^ rk[1];
        let mut w13 = u32::from_be_bytes([
            self.sbox(8, input[8]), self.sbox(9, input[9]),
            self.sbox(10, input[10]), self.sbox(11, input[11]),
        ]) ^ rk[2];
        let mut w4 = u32::from_be_bytes([
            self.sbox(12, input[12]), self.sbox(13, input[13]),
            self.sbox(14, input[14]), self.sbox(15, input[15]),
        ]) ^ rk[3];

        let mut rk_at = 5;
        let mut round = 0u32;
        let (mut w7, mut w9, mut w8, mut w10);
        let (mut b1, mut b2, mut b3);
        loop {
            w7 = self.ttable(10, ((w13 >> 8) & 0xff) as usize)
                ^ self.ttable(13, ((w4 >> 16) & 0xff) as usize)
                ^ self.ttable(0, (w5 >> 24) as usize)
                ^ self.ttable(7, (w6 & 0xff) as usize)
                ^ rk[rk_at - 1];
            w9 = self.ttable(1, ((w5 >> 16) & 0xff) as usize)
                ^ self.ttable(14, ((w4 >> 8) & 0xff) as usize)
                ^ self.ttable(4, (w6 >> 24) as usize)
                ^ self.ttable(11, (w13 & 0xff) as usize)
                ^ rk[rk_at];
            w8 = self.ttable(5, ((w6 >> 16) & 0xff) as usize)
                ^ self.ttable(2, ((w5 >> 8) & 0xff) as usize)
                ^ self.ttable(8, (w13 >> 24) as usize)
                ^ self.ttable(15, (w4 & 0xff) as usize)
                ^ rk[rk_at + 1];
            w10 = self.ttable(9, ((w13 >> 16) & 0xff) as usize)
                ^ self.ttable(6, ((w6 >> 8) & 0xff) as usize)
                ^ self.ttable(12, (w4 >> 24) as usize)
                ^ self.ttable(3, (w5 & 0xff) as usize)
                ^ rk[rk_at + 2];
            b1 = ((w7 >> 16) & 0xff) as u8;
            b2 = ((w10 >> 8) & 0xff) as u8;
            b3 = ((w9 >> 16) & 0xff) as u8;
            if round == 0x0C {
                break;
            }
            round += 2;
            w5 = self.ttable(10, ((w8 >> 8) & 0xff) as usize)
                ^ self.ttable(13, ((w10 >> 16) & 0xff) as usize)
                ^ self.ttable(0, (w7 >> 24) as usize)
                ^ self.ttable(7, (w9 & 0xff) as usize)
                ^ rk[rk_at + 3];
            w6 = self.ttable(1, b1 as usize)
                ^ self.ttable(14, b2 as usize)
                ^ self.ttable(4, (w9 >> 24) as usize)
                ^ self.ttable(11, (w8 & 0xff) as usize)
                ^ rk[rk_at + 4];
            w13 = self.ttable(2, ((w7 >> 8) & 0xff) as usize)
                ^ self.ttable(5, b3 as usize)
                ^ self.ttable(8, (w8 >> 24) as usize)
                ^ self.ttable(15, (w10 & 0xff) as usize)
                ^ rk[rk_at + 5];
            w4 = self.ttable(6, ((w9 >> 8) & 0xff) as usize)
                ^ self.ttable(9, ((w8 >> 16) & 0xff) as usize)
                ^ self.ttable(12, (w10 >> 24) as usize)
                ^ self.ttable(3, (w7 & 0xff) as usize)
                ^ rk[rk_at + 6];
            rk_at += 8;
        }

        let indexes = [
            (w7 >> 24) as u8, ((w10 >> 16) & 0xff) as u8,
            ((w8 >> 8) & 0xff) as u8, w9 as u8,
            (w9 >> 24) as u8, b1, b2, w8 as u8,
            (w8 >> 24) as u8, b3, ((w7 >> 8) & 0xff) as u8, w10 as u8,
            (w10 >> 24) as u8, ((w8 >> 16) & 0xff) as u8,
            ((w9 >> 8) & 0xff) as u8, w7 as u8,
        ];
        let mut out = [0u8; 16];
        for (i, index) in indexes.into_iter().enumerate() {
            out[i] = self.sboxes[i * 0x100 + index as usize];
        }
        out
    }

    pub fn decrypt_cbc(&self, data: &mut [u8], iv: &[u8; 16]) {
        let mut previous = *iv;
        for block in data.chunks_exact_mut(16) {
            let input: [u8; 16] = block.try_into().unwrap();
            let mut plain = self.decrypt_block(&input);
            for (p, pv) in plain.iter_mut().zip(previous.iter()) {
                *p ^= *pv;
            }
            previous = input;
            block.copy_from_slice(&plain);
        }
    }

    /// Inverse of `decrypt_block`: `decrypt_block(&encrypt_block(p)) == p`.
    pub fn encrypt_block(&self, input: &[u8; 16]) -> [u8; 16] {
        let rk = &self.round_keys;
        // Undo the per-key output S-boxes; output byte 4c+r came from
        // byte (24 - 8r) of word (c - r) mod 4.
        let mut state = [0u32; 4];
        for (i, &byte) in input.iter().enumerate() {
            let (c, r) = (i / 4, i % 4);
            let x = self.inv_sboxes[i][byte as usize];
            state[(c + 4 - r) % 4] |= (x as u32) << (24 - 8 * r);
        }
        for round in (1..=13).rev() {
            for (w, k) in state.iter_mut().zip(&rk[round * 4..round * 4 + 4]) {
                *w ^= k;
            }
            state = self.inverse.apply(state);
        }
        let mut out = [0u8; 16];
        for (i, o) in out.iter_mut().enumerate() {
            let byte = ((state[i / 4] ^ rk[i / 4]) >> (24 - 8 * (i % 4))) as u8;
            *o = self.inverse.sboxes[i][byte as usize];
        }
        out
    }

    pub fn encrypt_cbc(&self, data: &mut [u8], iv: &[u8; 16]) {
        let mut previous = *iv;
        for block in data.chunks_exact_mut(16) {
            let mut input: [u8; 16] = block.try_into().unwrap();
            for (b, pv) in input.iter_mut().zip(previous.iter()) {
                *b ^= *pv;
            }
            previous = self.encrypt_block(&input);
            block.copy_from_slice(&previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Option<CryptoContext> {
        let ctx = CryptoContext::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("data")).ok();
        if ctx.is_none() { eprintln!("skipping: data/tables.bin or data/keys.bin not present"); }
        ctx
    }

    #[test]
    fn block_roundtrip_all_keys() {
        let Some(crypto) = context() else { return; };
        let mut seed = 0x1234_5678u32;
        for index in 0..crypto.key_count() {
            let schedule = crypto.schedule(index).unwrap();
            for _ in 0..64 {
                let mut plain = [0u8; 16];
                for b in plain.iter_mut() {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    *b = (seed >> 16) as u8;
                }
                let cipher = schedule.encrypt_block(&plain);
                assert_eq!(schedule.decrypt_block(&cipher), plain, "key {index}");
                assert_eq!(schedule.encrypt_block(&schedule.decrypt_block(&plain)), plain, "key {index}");
            }
        }
    }

    #[test]
    fn reencrypts_appdata0_toc() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/appdata0.rpf");
        let Ok(raw) = std::fs::read(&path) else {
            eprintln!("skipping: {} not present", path.display());
            return;
        };
        let Some(crypto) = context() else { return; };
        let archive = crate::rpf9::reader::Archive::open(&raw, &crypto).unwrap();
        assert_eq!(archive.key_index(), 4);
        let schedule = crypto.schedule(archive.key_index()).unwrap();
        let start = crate::rpf9::header::HEADER_SIZE;
        let original = &raw[start..archive.header().payload_start as usize];
        let mut data = original.to_vec();
        schedule.decrypt_cbc(&mut data, &TAG_IV);
        schedule.encrypt_cbc(&mut data, &TAG_IV);
        assert_eq!(data, original);
    }
}
