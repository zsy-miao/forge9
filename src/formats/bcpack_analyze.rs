//! Structural analysis of BCPack chunks of the 16-byte-block codecs
//! (BC3 0x0C, BC5 0x14, BC6H 0x18, BC7 0x1C): fragment headers, the rANS entropy stage
//! and per-lane symbol statistics. No block-level (BCn) reconstruction.
//!
//! The entropy stage is bcpack-rs's `rans::decode_lanes` (MDU lane decoder,
//! feeder and lockstep). The byte source here records which lane every stream
//! byte went to, so each lane's own byte sequence can be split back into
//! fragments and checked against the decoder's byte and fragment counts.
//!
//! Fragment header (per lane, in band):
//!
//! | Byte | Bits | Meaning |
//! |---|---|---|
//! | 0 | 3:0 | model q |
//! | 0 | 4 | flush |
//! | 0 | 5 | three-byte header |
//! | 0 | 7:6 | z, alphabet narrowing |
//! | 1 | | payload length - 1, or its low byte when three-byte |
//! | 2 | | (three-byte only) high byte; length = (b2 << 8 \| b1) + 257 |

use bcpack::rans::{self, ByteSource, DecodeError};

/// Symbol widths per lane (Verilog `alphabet_bits` is width - 1).
pub fn lane_widths(codec: u8) -> Option<&'static [u8]> {
    Some(match codec {
        0x0C => &[4, 4, 8, 6, 6, 6, 6, 5, 6, 5, 8, 8],
        0x14 => &[4, 4, 8, 8, 6, 6, 6, 6, 6, 6, 6, 6],
        0x18 => &[4, 4, 8, 8, 8, 5, 5, 5, 6, 6, 6, 8, 8, 8, 8, 5],
        0x1C => &[4, 4, 8, 8, 8, 8, 8, 7, 7, 5, 6, 8, 8, 8, 8, 5],
        _ => return None,
    })
}

/// Virtual bytes per lane symbol. Every lane emits `virtual_length / 8`
/// symbols, as for BC1, so a 16-byte BCn block takes two symbol vectors
/// (one per block, `virtual_length / 16`, never converges on retail chunks).
pub const SYMBOL_BYTES: u64 = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    pub q: u8,
    pub flush: bool,
    pub three_byte: bool,
    pub z: u8,
    /// Payload bytes (state bytes included), excluding the header.
    pub payload: u32,
}

/// Split one lane's byte sequence into fragments. Errors if the last
/// fragment's header or payload runs past the end.
pub fn parse_fragments(lane: &[u8]) -> Result<Vec<Fragment>, String> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < lane.len() {
        let b0 = lane[p];
        let three_byte = b0 & 0x20 != 0;
        let hdr = if three_byte { 3 } else { 2 };
        let h = lane.get(p..p + hdr).ok_or_else(|| format!("header at lane byte {p} truncated"))?;
        let payload = if three_byte { ((h[2] as u32) << 8 | h[1] as u32) + 257 } else { h[1] as u32 + 1 };
        p += hdr + payload as usize;
        if p > lane.len() {
            return Err(format!("fragment {} payload runs {} bytes past the lane end", out.len(), p - lane.len()));
        }
        out.push(Fragment { q: b0 & 15, flush: b0 & 0x10 != 0, three_byte, z: b0 >> 6, payload });
    }
    Ok(out)
}

/// Byte source that remembers which lane took each byte.
struct Recorder<'a> {
    data: &'a [u8],
    owner: Vec<u8>,
}

impl ByteSource for Recorder<'_> {
    fn next(&mut self, lane: usize) -> Option<u8> {
        let b = *self.data.get(self.owner.len())?;
        self.owner.push(lane as u8);
        Some(b)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Lane {
    pub width: u8,
    pub bytes: u32,
    pub final_state: u32,
    /// Fragments the decoder's header FSM counted.
    pub decoder_fragments: u32,
    /// Fragments split from the lane's bytes by [`parse_fragments`].
    pub fragments: Vec<Fragment>,
    pub parse_error: Option<String>,
    /// `hist[v]` = occurrences of symbol v (length 2^width).
    pub hist: Vec<u64>,
}

impl Lane {
    /// Header bytes + payload bytes equal the bytes the decoder fed this lane.
    pub fn headers_consistent(&self) -> bool {
        let sum: u32 = self.fragments.iter().map(|f| f.payload + if f.three_byte { 3 } else { 2 }).sum();
        self.parse_error.is_none() && sum == self.bytes && self.fragments.len() as u32 == self.decoder_fragments
    }
    pub fn symbols(&self) -> u64 { self.hist.iter().sum() }
    pub fn distinct(&self) -> usize { self.hist.iter().filter(|&&n| n > 0).count() }
    pub fn max_symbol(&self) -> Option<usize> { self.hist.iter().rposition(|&n| n > 0) }
    /// Order-0 Shannon entropy in bits per symbol.
    pub fn entropy(&self) -> f64 {
        let t = self.symbols() as f64;
        self.hist.iter().filter(|&&n| n > 0).map(|&n| { let p = n as f64 / t; -p * p.log2() }).sum::<f64>().max(0.0)
    }
    /// Coded bits per symbol (headers included).
    pub fn coded_bits(&self) -> f64 { 8.0 * self.bytes as f64 / self.symbols().max(1) as f64 }
}

#[derive(Debug)]
pub struct Analysis {
    /// Symbols per lane.
    pub symbols_per_lane: usize,
    pub stream_len: usize,
    pub consumed: usize,
    /// Bytes after `consumed` are all zero.
    pub tail_zero: bool,
    pub cycles: u64,
    pub lanes: Vec<Lane>,
    /// Per-lane symbol streams, lane major (`symbols[lane][block]`).
    pub symbols: Vec<Vec<u16>>,
}

/// Run the entropy stage over `data` (one chunk, pad optional) with the given
/// lane widths and `n` symbols per lane.
pub fn analyze(data: &[u8], widths: &[u8], n: usize) -> Result<Analysis, DecodeError> {
    let ab: Vec<u8> = widths.iter().map(|w| w - 1).collect();
    let mut src = Recorder { data, owner: Vec::with_capacity(data.len()) };
    let out = rans::decode_lanes(&mut src, &ab, n, data.len())?;
    let mut lane_bytes = vec![Vec::new(); widths.len()];
    for (&b, &l) in data.iter().zip(&src.owner) {
        lane_bytes[l as usize].push(b);
    }
    let lanes = widths.iter().enumerate().map(|(i, &w)| {
        let (fragments, parse_error) = match parse_fragments(&lane_bytes[i]) {
            Ok(f) => (f, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let mut hist = vec![0u64; 1 << w];
        for &s in out.lane(i) { hist[s as usize] += 1; }
        let st = &out.stats[i];
        Lane { width: w, bytes: st.bytes, final_state: st.final_state, decoder_fragments: st.fragments, fragments, parse_error, hist }
    }).collect();
    Ok(Analysis {
        symbols_per_lane: n,
        stream_len: data.len(),
        consumed: out.consumed,
        tail_zero: data[out.consumed..].iter().all(|&b| b == 0),
        cycles: out.cycles,
        lanes,
        symbols: (0..widths.len()).map(|i| out.lane(i).to_vec()).collect(),
    })
}

/// Joint histogram of two symbol streams, for mutual information.
pub struct Joint {
    wb: u8,
    counts: Vec<u64>,
}

impl Joint {
    pub fn new(wa: u8, wb: u8) -> Joint { Joint { wb, counts: vec![0; 1 << (wa + wb)] } }
    pub fn add(&mut self, a: &[u16], b: &[u16]) {
        for (&x, &y) in a.iter().zip(b) { self.counts[(x as usize) << self.wb | y as usize] += 1; }
    }
    /// I(A; B) in bits.
    pub fn mi(&self) -> f64 {
        let nb = 1usize << self.wb;
        let mut pa = vec![0u64; self.counts.len() / nb];
        let mut pb = vec![0u64; nb];
        for (k, &c) in self.counts.iter().enumerate() { pa[k / nb] += c; pb[k % nb] += c; }
        let n = pa.iter().sum::<u64>() as f64;
        self.counts.iter().enumerate().filter(|x| *x.1 > 0)
            .map(|(k, &c)| c as f64 / n * (c as f64 * n / (pa[k / nb] as f64 * pb[k % nb] as f64)).log2()).sum::<f64>().max(0.0)
    }
}

/// Even (`v[2i]`) and odd (`v[2i + 1]`) symbol vectors of a lane, i.e. the
/// first and second vector of each 16-byte block.
pub fn halves(s: &[u16]) -> (Vec<u16>, Vec<u16>) {
    (s.iter().step_by(2).copied().collect(), s.iter().skip(1).step_by(2).copied().collect())
}

/// Does the chunk converge (every lane legal, all bytes used up to zero pad)?
pub fn converges(data: &[u8], widths: &[u8], n: usize) -> bool {
    let ab: Vec<u8> = widths.iter().map(|w| w - 1).collect();
    rans::decode_lanes_fast(data, &ab, n).is_ok_and(|o| data[o.consumed..].iter().all(|&b| b == 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bcpack::encode::{encode_lanes, header_model, sample_symbol, Fragment as EncFrag, Rng};

    #[test]
    fn parses_two_and_three_byte_headers() {
        // q=5 flush z=1, 3-byte payload; then q=2 three-byte, payload 257 + 0x0102.
        let mut lane = vec![0x55, 2, 0xAA, 0xBB, 0xCC, 0x22, 0x02, 0x01];
        lane.extend(vec![0u8; 257 + 0x102]);
        let f = parse_fragments(&lane).unwrap();
        assert_eq!(f, vec![
            Fragment { q: 5, flush: true, three_byte: false, z: 1, payload: 3 },
            Fragment { q: 2, flush: false, three_byte: true, z: 0, payload: 515 },
        ]);
        assert!(parse_fragments(&lane[..lane.len() - 1]).is_err());
        assert!(parse_fragments(&[0x20, 0]).is_err());
    }

    #[test]
    fn recovers_synthetic_bc5_stream() {
        let widths = lane_widths(0x14).unwrap();
        let ab: Vec<u8> = widths.iter().map(|w| w - 1).collect();
        let (blocks, mut rng) = (300, Rng(0x9E37_79B9));
        let (mut lanes, mut syms) = (Vec::new(), Vec::new());
        for &a in &ab {
            let (mut frags, mut all, mut left) = (Vec::new(), Vec::<u16>::new(), blocks);
            while left > 0 {
                let n = left.min(1 + rng.below(120) as usize);
                let z = rng.below(2) as u8;
                let q = rng.below(16) as u8;
                let s: Vec<u16> = (0..n).map(|_| sample_symbol(&mut rng, header_model(a, z).unwrap(), q)).collect();
                all.extend(&s);
                frags.push(EncFrag { symbols: s, q, z });
                left -= n;
            }
            lanes.push(frags);
            syms.push(all);
        }
        let stream = encode_lanes(&ab, &lanes).unwrap();
        let a = analyze(&stream, widths, blocks).unwrap();
        assert_eq!(a.consumed, stream.len());
        assert_eq!(a.symbols, syms);
        for (l, enc) in a.lanes.iter().zip(&lanes) {
            assert!(l.headers_consistent());
            assert_eq!(l.fragments.iter().map(|f| (f.q, f.z)).collect::<Vec<_>>(), enc.iter().map(|f| (f.q, f.z)).collect::<Vec<_>>());
        }
        assert!(converges(&stream, widths, blocks));
        assert!(!converges(&stream, widths, blocks + 1));
    }
}
