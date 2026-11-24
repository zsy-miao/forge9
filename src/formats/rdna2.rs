//! RDNA2 (GFX10.3) instruction-length decoder and classifier.
//!
//! Not a disassembler: it walks a program's dwords, sizes each instruction
//! (base encoding + literal / DPP / SDWA / MIMG NSA dwords) and tags it with
//! its encoding family and, where it has one, a wave64-sensitive op class.
//!
//! Encodings are picked by the top bits of the first dword:
//!
//! | Bits | Encoding | Dwords |
//! |---|---|---|
//! | `0` (31) | VOP2, except `0111111` VOP1 and `0111110` VOPC (31:25) | 1 |
//! | `10` (31:30) | SOP2, `1011` SOPK, `101111101` SOP1, `..110` SOPC, `..111` SOPP | 1 |
//! | `110010` / `110011` / `110101` (31:26) | VINTRP / VOP3P / VOP3 | 1 / 2 / 2 |
//! | `110110` / `110111` | DS / FLAT (bits 15:14 pick flat, scratch, global) | 2 |
//! | `111000` / `111010` / `111100` | MUBUF / MTBUF / MIMG (+ NSA dwords, bits 2:1) | 2 |
//! | `111101` / `111110` | SMEM / EXP | 2 |
//!
//! A source operand of 255 appends a 32-bit literal (VALU fetches it even
//! when the opcode ignores that source). VOP1/VOP2/VOPC `src0` of 0xF9 (SDWA),
//! 0xFA (DPP16) or 0xE9/0xEA (DPP8) appends one control dword. GFX10.3 has no
//! VOP3 DPP (that arrived in GFX11). The console GPU also keeps GFX10.1's
//! `v_madmk_f32` / `v_madak_f32` (VOP2 0x20/0x21), which carry a literal.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Family {
    Sop2, Sopk, Sop1, Sopc, Sopp, Smem,
    Vop2, Vop1, Vopc, Vop3, Vop3p, Vintrp,
    Ds, Mubuf, Mtbuf, Mimg, Flat, Global, Scratch, Exp,
    Unknown,
}

pub const FAMILIES: [Family; 21] = {
    use Family::*;
    [Sop2, Sopk, Sop1, Sopc, Sopp, Smem, Vop2, Vop1, Vopc, Vop3, Vop3p, Vintrp, Ds, Mubuf, Mtbuf, Mimg, Flat, Global, Scratch, Exp, Unknown]
};

impl Family {
    pub fn name(self) -> &'static str {
        use Family::*;
        match self {
            Sop2 => "SOP2", Sopk => "SOPK", Sop1 => "SOP1", Sopc => "SOPC", Sopp => "SOPP", Smem => "SMEM",
            Vop2 => "VOP2", Vop1 => "VOP1", Vopc => "VOPC", Vop3 => "VOP3", Vop3p => "VOP3P", Vintrp => "VINTRP",
            Ds => "DS", Mubuf => "MUBUF", Mtbuf => "MTBUF", Mimg => "MIMG", Flat => "FLAT", Global => "GLOBAL",
            Scratch => "SCRATCH", Exp => "EXP", Unknown => "unknown",
        }
    }
}

/// Ops whose behaviour depends on the 64-lane wave, roughly hardest-to-emulate first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WaveOp {
    /// `ds_permute_b32` / `ds_bpermute_b32`: any lane reads any of the 64.
    DsPermute,
    /// `v_readlane_b32` / `v_writelane_b32` with a constant lane 32..63: reads the upper half.
    LaneHi,
    /// `v_readlane_b32`: one lane to an SGPR (constant lane 0..31, or SGPR-selected).
    Readlane,
    /// `v_writelane_b32` (constant lane 0..31, or SGPR-selected).
    Writelane,
    /// `v_readfirstlane_b32`: first active lane of 64.
    Readfirstlane,
    /// `v_mbcnt_hi_u32_b32`: counts mask bits of lanes 32..63 (lane index / prefix count).
    MbcntHi,
    /// `s_bcnt*_b64`, `s_ff0/ff1_i32_b64`, `s_flbit*_b64`: ballot popcount / first-lane scans.
    MaskScan64,
    /// 64-bit SALU on EXEC or VCC, and every `s_*_saveexec_b64` / `s_*_wrexec_b64`.
    ExecMask64,
    /// `s_quadmask_b64`, `s_wqm_b64`.
    QuadMask64,
    /// `v_permlane16_b32` / `v_permlanex16_b32`: within / across 16-lane rows.
    Permlane,
    /// `ds_swizzle_b32` (quad or 32-lane bitmask mode).
    DsSwizzle,
    /// DPP16 by `dpp_ctrl` group (all stay inside a 16-lane row on GFX10).
    DppQuadPerm,
    DppRowShift,
    DppRowMirror,
    DppRowShare,
    DppRowXmask,
    /// DPP16 control value GFX10 does not define (GFX9 wave/broadcast forms).
    DppOther,
    /// DPP8: arbitrary permute inside each group of 8 lanes.
    Dpp8,
    /// `s_movrel{s,d}*`: scalar register indexing (wave-size independent, listed on request).
    Movrel,
}

pub const WAVE_OPS: [WaveOp; 19] = {
    use WaveOp::*;
    [DsPermute, LaneHi, Readlane, Writelane, Readfirstlane, MbcntHi, MaskScan64, ExecMask64, QuadMask64, Permlane, DsSwizzle,
     DppQuadPerm, DppRowShift, DppRowMirror, DppRowShare, DppRowXmask, DppOther, Dpp8, Movrel]
};

impl WaveOp {
    pub fn name(self) -> &'static str {
        use WaveOp::*;
        match self {
            DsPermute => "ds_permute", LaneHi => "lane_32_63", Readlane => "readlane", Writelane => "writelane", Readfirstlane => "readfirstlane",
            MbcntHi => "mbcnt_hi", MaskScan64 => "mask_scan_b64", ExecMask64 => "exec_vcc_b64", QuadMask64 => "quad_wqm_b64",
            Permlane => "permlane", DsSwizzle => "ds_swizzle", DppQuadPerm => "dpp_quad_perm", DppRowShift => "dpp_row_shift",
            DppRowMirror => "dpp_row_mirror", DppRowShare => "dpp_row_share", DppRowXmask => "dpp_row_xmask",
            DppOther => "dpp_other", Dpp8 => "dpp8", Movrel => "s_movrel",
        }
    }
}

/// How control leaves an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Next,
    /// `s_branch`: only to the target (dword offset from the next instruction).
    Jump(i16),
    /// Conditional branch or `s_call_b64`: the target and the next instruction.
    Branch(i16),
    /// `s_endpgm*`, `s_setpc_b64`, `s_rfe_b64`: the path ends here.
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inst {
    pub len: usize,
    pub family: Family,
    pub wave: Option<WaveOp>,
    /// `s_endpgm` and its `_saved` / `_ordered_ps_done` forms.
    pub endpgm: bool,
    /// `s_code_end`: padding after the program.
    pub code_end: bool,
    pub flow: Flow,
}

const LITERAL: u32 = 255;
const VCC_LO: u32 = 106;
const EXEC_LO: u32 = 126;

/// Sizes and classifies the instruction at the start of `code`; `None` if it
/// runs past the end. Unrecognised encodings come back as one `Unknown` dword.
pub fn decode(code: &[u8]) -> Option<Inst> {
    let dw = |i: usize| code.get(i * 4..i * 4 + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let w = dw(0)?;
    let mut inst = Inst { len: 4, family: Family::Unknown, wave: None, endpgm: false, code_end: false, flow: Flow::Next };
    let mut words = 1;
    let lit = |srcs: &[u32]| srcs.contains(&LITERAL) as usize;

    if w >> 31 == 0 {
        let src0 = w & 0x1FF;
        inst.family = match w >> 25 {
            0x3F => Family::Vop1,
            0x3E => Family::Vopc,
            _ => Family::Vop2,
        };
        match src0 {
            0xF9 => words += 1,
            0xFA => {
                words += 1;
                inst.wave = Some(dpp16_class((dw(1)? >> 8) & 0x1FF));
            }
            0xE9 | 0xEA => { words += 1; inst.wave = Some(WaveOp::Dpp8); }
            _ => words += lit(&[src0]),
        }
        match inst.family {
            Family::Vop2 => if matches!((w >> 25) & 0x3F, 0x20 | 0x21 | 0x2C | 0x2D | 0x37 | 0x38) && src0 != LITERAL { words += 1 },
            Family::Vop1 if (w >> 9) & 0xFF == 0x02 => inst.wave = Some(WaveOp::Readfirstlane),
            _ => {}
        }
    } else if w >> 30 == 0b10 {
        let (s0, s1, sdst) = (w & 0xFF, (w >> 8) & 0xFF, (w >> 16) & 0x7F);
        match w >> 23 {
            0x17D => {
                inst.family = Family::Sop1;
                words += lit(&[s0]);
                inst.wave = sop1_class((w >> 8) & 0xFF, s0, sdst);
                if matches!((w >> 8) & 0xFF, 0x20 | 0x22) { inst.flow = Flow::End; }
            }
            0x17E => { inst.family = Family::Sopc; words += lit(&[s0, s1]); }
            0x17F => {
                inst.family = Family::Sopp;
                let op = (w >> 16) & 0x7F;
                inst.endpgm = matches!(op, 0x01 | 0x1B | 0x1E);
                inst.code_end = op == 0x1F;
                let simm = w as u16 as i16;
                inst.flow = match op {
                    0x02 => Flow::Jump(simm),
                    0x04..=0x09 | 0x17..=0x1A => Flow::Branch(simm),
                    _ if inst.endpgm => Flow::End,
                    _ => Flow::Next,
                };
            }
            _ if w >> 28 == 0xB => {
                inst.family = Family::Sopk;
                match (w >> 23) & 0x1F {
                    0x15 => words += 1, // s_setreg_imm32_b32
                    0x16 | 0x1B | 0x1C => inst.flow = Flow::Branch(w as u16 as i16), // s_call_b64, s_subvector_loop_*
                    _ => {}
                }
            }
            _ => {
                inst.family = Family::Sop2;
                words += lit(&[s0, s1]);
                // 64-bit logic ops and s_cselect_b64 on EXEC/VCC.
                if matches!((w >> 23) & 0x7F, 0x0B | 0x0F | 0x11 | 0x13 | 0x15 | 0x17 | 0x19 | 0x1B | 0x1D)
                    && [s0, s1, sdst].iter().any(|&r| r == VCC_LO || r == EXEC_LO) {
                    inst.wave = Some(WaveOp::ExecMask64);
                }
            }
        }
    } else {
        let w1 = dw(1);
        let vop3_srcs = |w1: u32| [w1 & 0x1FF, (w1 >> 9) & 0x1FF, (w1 >> 18) & 0x1FF];
        match w >> 26 {
            0x32 => inst.family = Family::Vintrp,
            0x33 => { inst.family = Family::Vop3p; words = 2 + lit(&vop3_srcs(w1?)); }
            0x35 => {
                inst.family = Family::Vop3;
                let srcs = vop3_srcs(w1?);
                words = 2 + lit(&srcs);
                // Lane select (src1): inline integers 128..192 are 0..64.
                let lane = match srcs[1] { LITERAL => dw(2).map(|l| l & 63), 128..=192 => Some(srcs[1] - 128), _ => None };
                let hi = lane.is_some_and(|l| l >= 32);
                inst.wave = match (w >> 16) & 0x3FF {
                    0x182 => Some(WaveOp::Readfirstlane),
                    0x360 | 0x361 if hi => Some(WaveOp::LaneHi),
                    0x360 => Some(WaveOp::Readlane),
                    0x361 => Some(WaveOp::Writelane),
                    0x366 => Some(WaveOp::MbcntHi),
                    0x377 | 0x378 => Some(WaveOp::Permlane),
                    _ => None,
                };
            }
            0x36 => {
                inst.family = Family::Ds;
                words = 2;
                inst.wave = match (w >> 18) & 0xFF {
                    0x35 => Some(WaveOp::DsSwizzle),
                    0xB2 | 0xB3 => Some(WaveOp::DsPermute),
                    _ => None,
                };
            }
            0x37 => {
                inst.family = [Family::Flat, Family::Scratch, Family::Global, Family::Unknown][((w >> 14) & 3) as usize];
                words = 2;
            }
            0x38 => { inst.family = Family::Mubuf; words = 2; }
            0x3A => { inst.family = Family::Mtbuf; words = 2; }
            0x3C => { inst.family = Family::Mimg; words = 2 + ((w >> 1) & 3) as usize; }
            0x3D => { inst.family = Family::Smem; words = 2; }
            0x3E => { inst.family = Family::Exp; words = 2; }
            _ => {}
        }
        if inst.family == Family::Unknown { words = 1; }
    }
    inst.len = words * 4;
    (code.len() >= inst.len).then_some(inst)
}

fn sop1_class(op: u32, s0: u32, sdst: u32) -> Option<WaveOp> {
    let on_mask = s0 == VCC_LO || s0 == EXEC_LO || sdst == VCC_LO || sdst == EXEC_LO;
    match op {
        0x0E | 0x10 | 0x12 | 0x14 | 0x16 | 0x18 => Some(WaveOp::MaskScan64),
        0x0A | 0x2D => Some(WaveOp::QuadMask64),
        0x24..=0x2B | 0x37..=0x3A => Some(WaveOp::ExecMask64),
        0x04 | 0x06 | 0x08 | 0x1C | 0x1E if on_mask => Some(WaveOp::ExecMask64),
        0x2E..=0x31 | 0x49 => Some(WaveOp::Movrel),
        _ => None,
    }
}

fn dpp16_class(ctrl: u32) -> WaveOp {
    match ctrl {
        0x000..=0x0FF => WaveOp::DppQuadPerm,
        0x101..=0x10F | 0x111..=0x11F | 0x121..=0x12F => WaveOp::DppRowShift,
        0x140 | 0x141 => WaveOp::DppRowMirror,
        0x150..=0x15F => WaveOp::DppRowShare,
        0x160..=0x16F => WaveOp::DppRowXmask,
        _ => WaveOp::DppOther,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub insts: u32,
    pub families: [u32; FAMILIES.len()],
    pub wave: [u32; WAVE_OPS.len()],
    pub endpgms: u32,
    /// Paths that left the program: `s_endpgm*`, or `s_setpc_b64` (the ES half
    /// of a merged ES+GS program tail-jumps to its GS half).
    pub exits: u32,
    /// Dwords no path reaches, other than `s_code_end` padding: embedded constants.
    pub data_dwords: u32,
    /// An instruction ran past the end of the code.
    pub truncated: bool,
    /// Branch targets outside the code or inside another instruction.
    pub bad_targets: u32,
}

impl Stats {
    pub fn family(&self, f: Family) -> u32 { self.families[f as usize] }
    pub fn op(&self, o: WaveOp) -> u32 { self.wave[o as usize] }
    pub fn unknown(&self) -> u32 { self.family(Family::Unknown) }
    /// Every path decoded without unknown dwords or bad targets and one left the program.
    pub fn clean(&self) -> bool { !self.truncated && self.unknown() == 0 && self.bad_targets == 0 && self.exits > 0 }
}

const S_CODE_END: u32 = 0xBF9F_0000;

/// Decodes every instruction reachable from offset 0, following branches;
/// paths end at `s_endpgm`, `s_branch`, `s_setpc_b64`, `s_code_end` or an
/// unknown encoding. Unreached dwords are data, not code.
pub fn analyze(code: &[u8]) -> Stats {
    // Per dword: 0 unvisited, 1 instruction start, 2 inside an instruction.
    let mut mark = vec![0u8; code.len() / 4];
    let mut s = Stats::default();
    let mut todo = vec![0usize];
    let target = |at: usize, len: usize, d: i16| (at / 4 + len / 4).checked_add_signed(d as isize);
    while let Some(mut at) = todo.pop() {
        loop {
            match mark.get(at / 4) {
                None => { s.bad_targets += 1; break; }
                Some(1) => break,
                Some(2) => { s.bad_targets += 1; break; }
                _ => {}
            }
            let Some(i) = decode(&code[at..]) else { s.truncated = true; break };
            if i.code_end { break; }
            if mark[at / 4 + 1..at / 4 + i.len / 4].iter().any(|&m| m != 0) { s.bad_targets += 1; break; }
            mark[at / 4] = 1;
            mark[at / 4 + 1..at / 4 + i.len / 4].fill(2);
            s.insts += 1;
            s.families[i.family as usize] += 1;
            if let Some(w) = i.wave { s.wave[w as usize] += 1; }
            s.endpgms += i.endpgm as u32;
            if i.family == Family::Unknown { break; }
            match i.flow {
                Flow::Next => {}
                Flow::End => { s.exits += 1; break; }
                Flow::Jump(d) | Flow::Branch(d) => {
                    match target(at, i.len, d) {
                        Some(t) => todo.push(t * 4),
                        None => s.bad_targets += 1,
                    }
                    if matches!(i.flow, Flow::Jump(_)) { break; }
                }
            }
            at += i.len;
        }
    }
    s.data_dwords = code.chunks_exact(4).zip(&mark)
        .filter(|(w, m)| **m == 0 && u32::from_le_bytes((*w).try_into().unwrap()) != S_CODE_END).count() as u32;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(words: &[u32]) -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() }

    #[test]
    fn sizes_and_classifies() {
        let one = |w: &[u32]| decode(&bytes(w)).unwrap();
        // s_mov_b32 s0, 0x3f800000 (literal)
        assert_eq!((one(&[0xBE80_03FF, 0x3F80_0000]).family, one(&[0xBE80_03FF, 0]).len), (Family::Sop1, 8));
        // s_and_saveexec_b64 s[0:1], vcc
        assert_eq!(one(&[0xBE80_246A]).wave, Some(WaveOp::ExecMask64));
        // s_and_saveexec_b32 is the wave32 form: not flagged
        assert_eq!(one(&[0xBE80_3C6A]).wave, None);
        // s_and_b64 exec, exec, s[2:3]
        assert_eq!(one(&[0x87FE_027E]).wave, Some(WaveOp::ExecMask64));
        // s_bcnt1_i32_b64 s0, vcc ; s_ff1_i32_b64
        assert_eq!(one(&[0xBE80_106A]).wave, Some(WaveOp::MaskScan64));
        assert_eq!(one(&[0xBE80_146A]).wave, Some(WaveOp::MaskScan64));
        // v_mov_b32 v0, v1 with DPP16 row_shr:1 (ctrl 0x111)
        let dpp = one(&[0x7E00_02FA, 0xFF01_1101]);
        assert_eq!((dpp.family, dpp.len, dpp.wave), (Family::Vop1, 8, Some(WaveOp::DppRowShift)));
        // DPP16 quad_perm, row_xmask, DPP8, SDWA
        assert_eq!(one(&[0x7E00_02FA, 0xFF00_1B01]).wave, Some(WaveOp::DppQuadPerm));
        assert_eq!(one(&[0x0200_00FA, 0xFF01_6101]).wave, Some(WaveOp::DppRowXmask));
        assert_eq!(one(&[0x7E00_02E9, 0]).wave, Some(WaveOp::Dpp8));
        assert_eq!((one(&[0x7E00_02F9, 0]).len, one(&[0x7E00_02F9, 0]).wave), (8, None));
        // v_readfirstlane_b32 s0, v1 (VOP1 op 2)
        assert_eq!(one(&[0x7E00_0501]).wave, Some(WaveOp::Readfirstlane));
        // v_fmamk_f32 always carries a literal; console v_madak_f32 too
        assert_eq!(one(&[0x5800_0300, 0x3F80_0000]).len, 8);
        assert_eq!(one(&[0x4200_0300, 0x3F80_0000]).len, 8);
        // v_readlane_b32 s0, v1, s2 (VOP3 0x360); with a literal lane select
        assert_eq!(one(&[0xD760_0000, 0x0000_0501]).wave, Some(WaveOp::Readlane));
        assert_eq!(one(&[0xD760_0000, 0x0001_FF01, 5]).len, 12);
        // v_readlane_b32 s0, v1, 63 / v_writelane_b32 v0, s1, 32 (literal) read the upper half
        assert_eq!(one(&[0xD760_0000, 0x0001_7F01]).wave, Some(WaveOp::LaneHi));
        assert_eq!(one(&[0xD761_0000, 0x0001_FE01, 32]).wave, Some(WaveOp::LaneHi));
        assert_eq!(one(&[0xD760_0000, 0x0001_3F01]).wave, Some(WaveOp::Readlane));
        // v_mbcnt_hi_u32_b32, v_permlanex16_b32
        assert_eq!(one(&[0xD766_0000, 0x0002_00FF, 7]).wave, Some(WaveOp::MbcntHi));
        assert_eq!(one(&[0xD778_0000, 0x0000_0101]).wave, Some(WaveOp::Permlane));
        // ds_bpermute_b32, ds_swizzle_b32
        assert_eq!(one(&[0xDACC_0000, 0]).wave, Some(WaveOp::DsPermute));
        assert_eq!(one(&[0xD8D4_801F, 0]).wave, Some(WaveOp::DsSwizzle));
        // global_load_dword, scratch, MIMG with 2 NSA dwords, SMEM, EXP
        assert_eq!(one(&[0xDC30_8000, 0]).family, Family::Global);
        assert_eq!(one(&[0xDC30_4000, 0]).family, Family::Scratch);
        assert_eq!(one(&[0xF080_0F04, 0, 0, 0]).len, 16);
        assert_eq!(one(&[0xF400_0000, 0]).family, Family::Smem);
        assert_eq!(one(&[0xF800_080F, 0]).family, Family::Exp);
        // unknown encoding: one dword
        assert_eq!((one(&[0xC000_0000]).family, one(&[0xC000_0000]).len), (Family::Unknown, 4));
        // truncated: VOP3 needs two dwords
        assert!(decode(&bytes(&[0xD760_0000])).is_none());
    }

    #[test]
    fn walks_to_code_end() {
        // s_mov_b64 exec, -1 ; v_mov_b32 v0, 1.0 (literal) ; s_endpgm ; s_code_end x2
        let s = analyze(&bytes(&[0xBEFE_04C1, 0x7E00_02FF, 0x3F80_0000, 0xBF81_0000, 0xBF9F_0000, 0xBF9F_0000]));
        assert_eq!((s.insts, s.endpgms, s.data_dwords, s.clean()), (3, 1, 0, true));
        assert_eq!((s.op(WaveOp::ExecMask64), s.family(Family::Vop1), s.family(Family::Sopp)), (1, 1, 1));
        // A literal cut off by the end of the code is truncation, not a clean end.
        let s = analyze(&bytes(&[0x7E00_02FF]));
        assert!(s.truncated && !s.clean());
    }

    #[test]
    fn follows_branches_and_skips_data() {
        // 0: s_cbranch_scc0 +2 -> 3 ; 1: s_endpgm ; 2: data ; 3: s_branch -3 -> 1 ; 4: data
        let s = analyze(&bytes(&[0xBF84_0002, 0xBF81_0000, 0xDEAD_BEEF, 0xBF82_FFFD, 0x1234_5678]));
        assert_eq!((s.insts, s.endpgms, s.data_dwords, s.bad_targets, s.clean()), (3, 1, 2, 0, true));
        // A branch into the middle of a VOP3 is a desync.
        let s = analyze(&bytes(&[0xBF84_0000, 0xD760_0000, 0x0000_0501, 0xBF81_0000]));
        assert_eq!(s.insts, 3);
        let s = analyze(&bytes(&[0xBF84_0001, 0xD760_0000, 0x0000_0501, 0xBF81_0000]));
        assert!(s.bad_targets == 1 && !s.clean());
        // s_setpc_b64 s[24:25] is an exit too.
        let s = analyze(&bytes(&[0xBE80_2018]));
        assert!(s.clean() && s.endpgms == 0);
    }
}
