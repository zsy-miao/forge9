//! FSSM: the native RDNA program of one shader, plus the root-signature
//! layout it was compiled against.
//!
//! Stored offset *values* are relative to file offset 0x10; unused fields hold
//! 0xDEADBEEF. Stage blocks start at 0xC0 with stride 0x88 (a GS adds its VS
//! copy shader as a second block):
//!
//! | Block | Field |
//! |---|---|
//! | +0x00, +0x08 | u64 code offset, code size |
//! | +0x14 | flags: bit 5 wave32, bits 16..23 user SGPR count |
//! | +0x28, +0x30 | u64 root layout table, used resource table |
//! | +0x38, +0x3C | u32 layout count, used count |
//! | +0x40 | stage descriptor `0x000000C0_0002_AAHH` (HH hardware, AA API stage) |
//!
//! Both tables hold 20-byte entries. The layout is the whole root signature
//! (one entry per root constant run, root descriptor, table entry, heap and
//! static sampler); the used table is the subset this program reads.

use anyhow::{bail, Context, Result};
use super::{u32_at, u64_at};

const DEAD: u64 = 0xDEAD_BEEF_DEAD_BEEF;
const BLOCK: usize = 0xC0;
const STRIDE: usize = 0x88;

pub const ROOT_CONSTANTS: u32 = 0;
pub const ROOT_DESCRIPTOR: u32 = 1;
pub const ROOT_TABLE: u32 = 3;
pub const ROOT_STATIC_SAMPLER: u32 = 4;
pub const ROOT_RESOURCE_HEAP: u32 = 5;
pub const ROOT_SAMPLER_HEAP: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutEntry {
    pub root_type: u32,
    /// Offset in the descriptor table, in dwords (tables only).
    pub table_offset: u32,
    /// User SGPRs taken: root constants their dword count, descriptors 2, tables and heaps 1.
    pub user_sgprs: u32,
    /// Shader register (b500 for constants, t/s/u for the rest).
    pub register: u32,
    /// First user SGPR holding the parameter (0 for static samplers).
    pub first_user_data: u8,
    /// Root parameter index; for static samplers the static sampler index.
    pub root_slot: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsedEntry {
    /// 0 SRV, 1 sampler, 2 CBV, 3 UAV.
    pub resource_type: u8,
    pub root_type: u8,
    pub table_offset: u32,
    pub lower_bound: u32,
    pub stride: u32,
    pub descriptor_dwords: u8,
    pub first_user_data: u8,
    pub root_slot: u8,
}

#[derive(Debug, Clone)]
pub struct Stage {
    /// PAL hardware stage: 0 LS, 1 HS, 2 ES, 3 GS, 4 VS, 5 PS, 6 CS, 7 NGG (mesh).
    pub hw: u8,
    pub api: u8,
    pub code_offset: u64,
    pub code_size: u64,
    pub wave32: bool,
    pub user_sgpr_count: u8,
    pub layout: Vec<LayoutEntry>,
    pub used: Vec<UsedEntry>,
}

#[derive(Debug, Clone)]
pub struct Fssm {
    pub version: u32,
    pub stages: Vec<Stage>,
}

/// One root parameter as the program sees it: a run of user SGPRs.
#[derive(Debug, Clone, PartialEq)]
pub struct RootParam {
    pub root_type: u32,
    pub sgpr: u8,
    pub sgprs: u32,
    pub slot: u8,
    pub register: u32,
    /// Table entries (tables only).
    pub entries: usize,
}

impl std::fmt::Display for RootParam {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let (s, p) = (self.sgpr, self.slot);
        match self.root_type {
            ROOT_CONSTANTS => write!(f, "Constants({}dw b{}, s{s}, p{p})", self.sgprs, self.register),
            ROOT_DESCRIPTOR => write!(f, "Descriptor(reg {}, s{s}, p{p})", self.register),
            ROOT_TABLE => write!(f, "Table({} entries, s{s}, p{p})", self.entries),
            ROOT_RESOURCE_HEAP => write!(f, "ResourceHeap(s{s})"),
            ROOT_SAMPLER_HEAP => write!(f, "SamplerHeap(s{s})"),
            t => write!(f, "Type{t}(s{s}, p{p})"),
        }
    }
}

impl Stage {
    pub fn name(&self) -> &'static str { hw_stage_name(self.hw) }

    /// Root parameters in user-SGPR order; static samplers are not parameters.
    pub fn root_params(&self) -> Vec<RootParam> {
        let mut params: Vec<RootParam> = Vec::new();
        for e in self.layout.iter().filter(|e| e.root_type != ROOT_STATIC_SAMPLER) {
            match params.iter_mut().find(|p| p.sgpr == e.first_user_data) {
                Some(p) => p.entries += 1,
                None => params.push(RootParam { root_type: e.root_type, sgpr: e.first_user_data, sgprs: e.user_sgprs, slot: e.root_slot, register: e.register, entries: 1 }),
            }
        }
        params.sort_by_key(|p| p.sgpr);
        params
    }

    pub fn static_samplers(&self) -> usize {
        self.layout.iter().filter(|e| e.root_type == ROOT_STATIC_SAMPLER).count()
    }
}

pub fn hw_stage_name(hw: u8) -> &'static str {
    ["LS", "HS", "ES", "GS", "VS", "PS", "CS", "MS"].get(hw as usize).copied().unwrap_or("?")
}

pub fn resource_type_name(t: u8) -> &'static str {
    ["SRV", "Sampler", "CBV", "UAV"].get(t as usize).copied().unwrap_or("?")
}

pub fn root_type_name(t: u32) -> &'static str {
    match t {
        ROOT_CONSTANTS => "Constants",
        ROOT_DESCRIPTOR => "Descriptor",
        ROOT_TABLE => "Table",
        ROOT_STATIC_SAMPLER => "StaticSampler",
        ROOT_RESOURCE_HEAP => "ResourceHeap",
        ROOT_SAMPLER_HEAP => "SamplerHeap",
        _ => "?",
    }
}

pub fn parse(data: &[u8]) -> Result<Fssm> {
    if !data.starts_with(b"FSSM") { bail!("not an FSSM program"); }
    let version = u32_at(data, 4).context("FSSM header truncated")?;
    let mut stages = vec![stage_at(data, BLOCK)?.context("FSSM has no stage descriptor at 0x100")?];
    // Only geometry pipelines chain blocks (GS + VS copy shader); elsewhere
    // the bytes past the first block are stage-specific data.
    if stages[0].hw <= 3 {
        let mut at = BLOCK + STRIDE;
        while let Some(s) = stage_at(data, at)? {
            stages.push(s);
            at += STRIDE;
        }
    }
    Ok(Fssm { version, stages })
}

fn stage_at(data: &[u8], at: usize) -> Result<Option<Stage>> {
    let Some(desc) = u64_at(data, at + 0x40) else { return Ok(None) };
    if desc >> 16 != 0xC0_0002 { return Ok(None); }
    let q = |o: usize| u64_at(data, at + o).context("FSSM stage block truncated");
    let d = |o: usize| u32_at(data, at + o).context("FSSM stage block truncated");
    let flags = d(0x14)?;
    let layout = table(data, q(0x28)?, d(0x38)?, |e| LayoutEntry {
        root_type: u32_at(e, 0).unwrap(),
        table_offset: u32_at(e, 4).unwrap(),
        user_sgprs: u32_at(e, 8).unwrap(),
        register: u32_at(e, 12).unwrap(),
        first_user_data: e[16],
        root_slot: e[17],
    })?;
    let used = table(data, q(0x30)?, d(0x3C)?, |e| UsedEntry {
        resource_type: e[0],
        root_type: e[1],
        table_offset: u32_at(e, 4).unwrap(),
        lower_bound: u32_at(e, 8).unwrap(),
        stride: u32_at(e, 12).unwrap(),
        descriptor_dwords: e[16],
        first_user_data: e[17],
        root_slot: e[18],
    })?;
    Ok(Some(Stage {
        hw: desc as u8,
        api: (desc >> 8) as u8,
        code_offset: q(0)? + 0x10,
        code_size: q(8)?,
        wave32: flags & 0x20 != 0,
        user_sgpr_count: (flags >> 16) as u8,
        layout,
        used,
    }))
}

fn table<T>(data: &[u8], ptr: u64, count: u32, f: impl Fn(&[u8]) -> T) -> Result<Vec<T>> {
    if ptr == DEAD || count == 0 { return Ok(Vec::new()); }
    let start = ptr as usize + 0x10;
    let bytes = data.get(start..start + 20 * count as usize)
        .with_context(|| format!("FSSM table at {start:#x} ({count} entries) out of bounds"))?;
    Ok(bytes.chunks_exact(20).map(f).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(v: [u32; 4], b: [u8; 4]) -> Vec<u8> {
        let mut e: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        e.extend_from_slice(&b);
        e
    }

    #[test]
    fn parses_root_layout() {
        let mut f = vec![0u8; 0x200];
        f[..8].copy_from_slice(b"FSSM\x01\x01\0\0");
        f[0xC0..0xC8].copy_from_slice(&0x1F0u64.to_le_bytes());
        f[0xD4..0xD8].copy_from_slice(&0x000E_0000u32.to_le_bytes());
        f[0xE8..0xF0].copy_from_slice(&0x100u64.to_le_bytes());
        f[0xF0..0xF8].copy_from_slice(&DEAD.to_le_bytes());
        f[0xF8..0xFC].copy_from_slice(&5u32.to_le_bytes());
        f[0x100..0x108].copy_from_slice(&0x0000_00C0_0002_0607u64.to_le_bytes());
        let rows = [
            entry([ROOT_STATIC_SAMPLER, 0, 0, 3], [0, 6, 0, 0]),
            entry([ROOT_TABLE, 0, 1, 34], [8, 0, 0, 0]),
            entry([ROOT_TABLE, 8, 1, 35], [8, 0, 0, 0]),
            entry([ROOT_CONSTANTS, 0, 2, 500], [9, 1, 0, 0]),
            entry([ROOT_RESOURCE_HEAP, 0, 1, 0], [11, 0, 0, 0]),
        ];
        let mut at = 0x110;
        for r in rows { f[at..at + 20].copy_from_slice(&r); at += 20; }

        let p = parse(&f).unwrap();
        assert_eq!(p.stages.len(), 1);
        let s = &p.stages[0];
        assert_eq!((s.name(), s.api, s.user_sgpr_count, s.code_offset, s.used.len()), ("MS", 6, 14, 0x200, 0));
        let roots: Vec<String> = s.root_params().iter().map(|r| r.to_string()).collect();
        assert_eq!(roots, ["Table(2 entries, s8, p0)", "Constants(2dw b500, s9, p1)", "ResourceHeap(s11)"]);
        assert_eq!(s.static_samplers(), 1);
    }
}
