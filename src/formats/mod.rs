//! Decoders for payload formats found inside RPF9 archives.

pub mod awc;
pub mod bcpack_analyze;
pub mod bink;
pub mod fssm;
pub mod ps09;
pub mod rdna2;
mod rdna2_ops;
pub mod resource;
pub mod sgd5;
pub mod texture;
pub mod uniformity;
pub mod xbch;

pub(crate) fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

pub(crate) fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

pub(crate) fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}
