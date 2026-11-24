//! RDNA2 (GFX10.3) disassembly: LLVM-style text for instructions sized by [`decode`].
//!
//! Each opcode's operand list (field, kind, bits) comes from the spec-derived
//! [`OPERANDS`] table; this module knows the encodings' field layouts and the
//! per-family modifiers. Syntax follows `llvm-mc -mcpu=gfx1030` closely enough to
//! diff against it, except that branch targets are absolute byte offsets. Fields
//! whose meaning is not decoded print raw (`sdwa:0x...`, `format:N`, `src0=0xf9`).

use super::rdna2::{analyze_marks, decode, opcode_label, opcode_name, Family, Inst, Stats};
use super::rdna2_operands::OPERANDS;
use Family::*;

/// Operand descriptor of `op` in `family` (see [`super::rdna2_operands`]).
pub fn operands(family: Family, op: u16) -> Option<&'static str> {
    OPERANDS.binary_search_by_key(&(family, op), |&(f, o, _)| (f, o)).ok().map(|i| OPERANDS[i].2)
}

/// One line of a program listing.
pub struct Line {
    pub addr: usize,
    pub words: Vec<u32>,
    /// `None` for an unreached (data) dword.
    pub inst: Option<Inst>,
    pub text: String,
}

/// Every instruction [`analyze`](super::rdna2::analyze) reaches, in address order,
/// with unreached dwords (embedded constants, alignment `s_nop`s) as `.long`; trailing
/// `s_code_end` padding is dropped.
pub fn disassemble(code: &[u8], wave32: bool) -> (Stats, Vec<Line>) {
    let (stats, mark) = analyze_marks(code);
    let word = |d: usize| u32::from_le_bytes(code[d * 4..d * 4 + 4].try_into().unwrap());
    let end = (0..mark.len()).rposition(|d| mark[d] != 0 || word(d) != S_CODE_END).map_or(0, |d| d + 1);
    let mut lines = Vec::new();
    let mut d = 0;
    while d < end {
        let at = d * 4;
        match (mark[d], decode(&code[at..])) {
            (1, Some(i)) => {
                lines.push(Line { addr: at, words: (d..d + i.len / 4).map(word).collect(), inst: Some(i), text: format(code, at, &i, wave32) });
                d += i.len / 4;
            }
            _ => {
                lines.push(Line { addr: at, words: vec![word(d)], inst: None, text: format!(".long {:#010x} ; unreached", word(d)) });
                d += 1;
            }
        }
    }
    (stats, lines)
}

const S_CODE_END: u32 = 0xBF9F_0000;
const LITERAL: u32 = 255;
const NULL: u32 = 125;

/// (bit offset in the first two dwords, width, register scale) of a named field.
fn field(f: Family, name: &str) -> Option<(u32, u32, u32)> {
    Some(match (f, name) {
        (Sop2 | Sop1 | Sopk, "SDST") => (16, 7, 1),
        (Sop2 | Sop1 | Sopc, "SSRC0") => (0, 8, 1),
        (Sop2 | Sopc, "SSRC1") => (8, 8, 1),
        (Sopk | Sopp, "SIMM16") => (0, 16, 1),
        (Smem, "SBASE") => (0, 6, 2),
        (Smem, "SDATA") => (6, 7, 1),
        (Smem, "SOFFSET") => (57, 7, 1),
        (Vop1 | Vop2 | Vopc, "SRC0") => (0, 9, 1),
        (Vop2 | Vopc, "VSRC1") => (9, 8, 1),
        (Vop1 | Vop2, "VDST") => (17, 8, 1),
        (Vop3 | Vop3p, "VDST") => (0, 8, 1),
        (Vop3, "SDST") => (8, 7, 1),
        (Vop3 | Vop3p, "SRC0") => (32, 9, 1),
        (Vop3 | Vop3p, "SRC1") => (41, 9, 1),
        (Vop3 | Vop3p, "SRC2") => (50, 9, 1),
        (Vintrp, "VSRC") => (0, 8, 1),
        (Vintrp, "VDST") => (18, 8, 1),
        (Ds | Flat | Global | Scratch, "ADDR") => (32, 8, 1),
        (Ds, "DATA0") | (Flat | Global | Scratch, "DATA") => (40, 8, 1),
        (Ds, "DATA1") => (48, 8, 1),
        (Ds | Flat | Global | Scratch, "VDST") => (56, 8, 1),
        (Flat | Global | Scratch, "SADDR") => (48, 7, 1),
        (Mubuf | Mtbuf | Mimg, "VADDR") => (32, 8, 1),
        (Mubuf | Mtbuf | Mimg, "VDATA") => (40, 8, 1),
        (Mubuf | Mtbuf | Mimg, "SRSRC") => (48, 5, 4),
        (Mimg, "SSAMP") => (53, 5, 4),
        (Mubuf | Mtbuf, "SOFFSET") => (56, 8, 1),
        (Exp, "VSRC0") => (32, 8, 1),
        (Exp, "VSRC1") => (40, 8, 1),
        (Exp, "VSRC2") => (48, 8, 1),
        (Exp, "VSRC3") => (56, 8, 1),
        _ => return None,
    })
}

fn range(prefix: &str, n: u32, bits: u32) -> String {
    match bits.div_ceil(32) {
        0 | 1 => format!("{prefix}{n}"),
        k => format!("{prefix}[{n}:{}]", n + k - 1),
    }
}

/// A scalar operand code (0..255) or 9-bit source (256+ are VGPRs); `None` for reserved codes.
fn src(code: u32, bits: u32, literal: Option<u32>) -> Option<String> {
    let wide = bits > 32;
    Some(match code {
        0..=105 => range("s", code, bits),
        106 => (if wide { "vcc" } else { "vcc_lo" }).into(),
        107 => "vcc_hi".into(),
        108..=123 => range("ttmp", code - 108, bits),
        124 => "m0".into(),
        NULL => "null".into(),
        126 => (if wide { "exec" } else { "exec_lo" }).into(),
        127 => "exec_hi".into(),
        128..=192 => (code - 128).to_string(),
        193..=208 => format!("-{}", code - 192),
        235 => "src_shared_base".into(),
        236 => "src_shared_limit".into(),
        237 => "src_private_base".into(),
        238 => "src_private_limit".into(),
        239 => "src_pops_exiting_wave_id".into(),
        240..=247 => ["0.5", "-0.5", "1.0", "-1.0", "2.0", "-2.0", "4.0", "-4.0"][code as usize - 240].into(),
        248 => "0.15915494".into(),
        251 => "src_vccz".into(),
        252 => "src_execz".into(),
        253 => "src_scc".into(),
        254 => "src_lds_direct".into(),
        LITERAL => format!("{:#x}", literal?),
        256..=511 => range("v", code - 256, bits),
        _ => return None,
    })
}

/// VOP3 / VOP2 / VOP1 / VINTRP twin of a VOP1 / VOP2 / VOPC / VINTRP / VOP3 opcode, for the `_e32` / `_e64` suffix.
fn twin(f: Family, op: u16) -> Option<(Family, u16)> {
    Some(match (f, op) {
        (Vopc, _) => (Vop3, op),
        (Vop2, _) => (Vop3, op + 0x100),
        (Vop1, _) => (Vop3, op + 0x180),
        (Vintrp, _) => (Vop3, op + 0x200),
        (Vop3, 0x000..=0x0FF) => (Vopc, op),
        (Vop3, 0x100..=0x13F) => (Vop2, op - 0x100),
        (Vop3, 0x180..=0x1FF) => (Vop1, op - 0x180),
        (Vop3, 0x200..=0x202) => (Vintrp, op - 0x200),
        _ => return None,
    })
}

/// VADDR dwords of a MIMG op, from its name, `dim` and `a16`.
fn mimg_addrs(name: &str, dim: usize, a16: bool) -> u32 {
    let coords: u32 = [1, 2, 3, 3, 2, 3, 3, 4][dim];
    if name.starts_with("image_bvh64") { return if a16 { 9 } else { 12 }; }
    if name.starts_with("image_bvh") { return if a16 { 8 } else { 11 }; }
    if name == "image_get_resinfo" { return 1; }
    let (mut extra, mut grads, mut lod, mut g16) = (0, 0, 0, a16);
    if name.starts_with("image_sample") || name.starts_with("image_gather4") {
        for t in name.split('_').skip(2) {
            match t {
                "o" | "c" | "b" => extra += 1,
                "d" | "cd" => grads = [2, 4, 6, 4, 2, 4, 4, 4][dim],
                "l" | "cl" => lod += 1,
                "g16" => g16 = true,
                _ => {}
            }
        }
    } else if name.contains("_mip") {
        lod = 1;
    }
    let grads = if g16 { grads / 2 } else { grads };
    extra + grads + if a16 { (coords + lod).div_ceil(2) } else { coords + lod }
}

const DIMS: [&str; 8] = ["1D", "2D", "3D", "CUBE", "1D_ARRAY", "2D_ARRAY", "2D_MSAA", "2D_MSAA_ARRAY"];

/// `<mnemonic> <operands> <modifiers>` of instruction `i` at byte offset `at` of `code`.
pub fn format(code: &[u8], at: usize, i: &Inst, wave32: bool) -> String {
    let w: Vec<u32> = code[at..at + i.len].chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
    let f = i.family;
    if f == Unknown { return format!(".long {:#010x}", w[0]); }
    let q = w[0] as u64 | (w.get(1).copied().unwrap_or(0) as u64) << 32;
    let bit = |o: u32| (q >> o) & 1 == 1;
    let get = |o: u32, n: u32| ((q >> o) & ((1 << n) - 1)) as u32;
    let base = match f { Sop2 | Sopk | Sop1 | Sopc | Sopp | Vop1 | Vop2 | Vopc | Vintrp => 1, _ => 2 };
    // VOP1/VOP2/VOPC src0 0xF9 SDWA, 0xFA DPP16, 0xE9/0xEA DPP8: real src0 in the extension dword.
    let ext = match (f, w[0] & 0x1FF) {
        (Vop1 | Vop2 | Vopc, s @ (0xF9 | 0xFA | 0xE9 | 0xEA)) => Some((s, w[1])),
        _ => None,
    };
    let literal = (ext.is_none() && f != Mimg).then(|| w.get(base).copied()).flatten();
    let name = opcode_label(f, i.op);
    let mut m = name.clone();
    match ext {
        Some((0xF9, _)) => m += "_sdwa",
        Some(_) => m += "_dpp",
        // LLVM leaves the e32 form of v_readfirstlane_b32 unsuffixed.
        None if (f, i.op) == (Vop1, 0x02) || operands(f, i.op) == Some("") => {}
        None if twin(f, i.op).is_some_and(|(t, o)| opcode_name(t, o).is_some()) => m += if f == Vop3 { "_e64" } else { "_e32" },
        None => {}
    }
    if f == Vop3 && operands(f, i.op) == Some("") && w[1..] != [0] {
        // Fields the op ignores (console shaders hide a hash in v_nop's literal).
        let raw: Vec<String> = w[1..].iter().map(|x| format!("{x:#010x}")).collect();
        return format!("{m} ; ignored {}", raw.join(" "));
    }
    let Some(desc) = operands(f, i.op) else {
        let raw: Vec<String> = w.iter().map(|x| format!("{x:#010x}")).collect();
        return format!("{m} {}", raw.join(" "));
    };

    let mut ops: Vec<String> = Vec::new();
    let mut mods: Vec<String> = Vec::new();
    let mut nsrc = 0;
    let mask_bits = if wave32 { 32 } else { 64 };
    let (idxen, offen) = (bit(13), bit(12));
    let saddr_off = matches!(get(48, 7), NULL | 0x7F);
    let dmask = get(8, 4);
    // VOP3b: SDST sits where VOP3a keeps abs and op_sel.
    let vop3b = f == Vop3 && desc.contains("SDST:");
    // v_fma_mix*: VOP3P encoding whose neg_lo / neg_hi bits are per-source neg / abs.
    let mix = f == Vop3p && name.starts_with("v_fma_mix");
    for tok in desc.split_whitespace() {
        let (fname, ks) = tok.split_once(':').unwrap();
        let (kind, bits) = (ks.as_bytes()[0], ks[1..].parse::<u32>().unwrap());
        let val = match fname {
            "vcc" => 106,
            _ => field(f, fname).map_or(0, |(o, n, scale)| get(o, n) * scale),
        };
        let raw = || format!("{}={val:#x}", fname.to_ascii_lowercase());
        let text = match (f, fname) {
            (Vopc, "vcc") if matches!(ext, Some((0xF9, e)) if e >> 15 & 1 == 1) => {
                src(ext.unwrap().1 >> 8 & 0x7F, mask_bits, None).unwrap_or_else(raw)
            }
            (Vop1 | Vop2 | Vopc, "SRC0") if ext.is_some() => {
                let (kind, e) = ext.unwrap();
                nsrc += 1;
                let r = if kind == 0xF9 && e >> 23 & 1 == 1 { src(e & 0xFF, bits, None).unwrap_or_else(raw) } else { range("v", e & 0xFF, bits) };
                match kind {
                    0xFA => neg_abs(r, e >> 20 & 1 == 1, e >> 21 & 1 == 1),
                    0xF9 => sdwa_src(r, e >> 16),
                    _ => r,
                }
            }
            (Vop2 | Vopc, "VSRC1") if matches!(ext, Some((0xFA, _))) => {
                let e = ext.unwrap().1;
                neg_abs(range("v", val, bits), e >> 22 & 1 == 1, e >> 23 & 1 == 1)
            }
            (Vop2 | Vopc, "VSRC1") if matches!(ext, Some((0xF9, _))) => {
                let e = ext.unwrap().1;
                let r = if e >> 31 == 1 { src(val, bits, None).unwrap_or_else(raw) } else { range("v", val, bits) };
                sdwa_src(r, e >> 24)
            }
            (Mubuf | Mtbuf, "VADDR") => match idxen as u32 + offen as u32 {
                0 => "off".into(),
                n => range("v", val, 32 * n),
            },
            (Mubuf | Mtbuf, "VDATA") => range("v", val, bits + 32 * bit(55) as u32),
            (Mimg, "VDATA") => {
                let mut n = if name.starts_with("image_gather4") { 4 } else { dmask.count_ones().max(1) };
                if bit(63) { n = n.div_ceil(2); }
                range("v", val, 32 * (n + (bit(16) || bit(17)) as u32))
            }
            (Mimg, "VADDR") => {
                let n = mimg_addrs(&name, get(3, 3) as usize, bit(62));
                let nsa = i.len / 4 - 2;
                if nsa == 0 {
                    range("v", val, 32 * match n { 0..=12 => n, _ => 16 })
                } else {
                    let slots = 1 + 4 * nsa as u32;
                    let regs = std::iter::once(val).chain(w[2..].iter().flat_map(|x| x.to_le_bytes()).map(u32::from));
                    let regs: Vec<String> = regs.take(n.clamp(slots - 3, slots) as usize).map(|r| format!("v{r}")).collect();
                    format!("[{}]", regs.join(", "))
                }
            }
            (Mimg, "SRSRC") => range("s", val, if bit(15) { 128 } else { bits }),
            (Global, "SADDR") | (Scratch, "SADDR") if saddr_off => "off".into(),
            (Global, "ADDR") if !saddr_off => range("v", val, 32),
            (Scratch, "ADDR") if !saddr_off => "off".into(),
            (Smem, "SOFFSET") => {
                let off = (get(32, 21) << 11) as i32 >> 11;
                let off = if off < 0 { format!("-{:#x}", -off) } else { format!("{off:#x}") };
                match val {
                    NULL if get(32, 21) == 0 => "null".into(),
                    NULL => off,
                    _ if get(32, 21) == 0 => src(val, 32, None).unwrap_or_else(raw),
                    _ => format!("{} offset:{off}", src(val, 32, None).unwrap_or_else(raw)),
                }
            }
            (Vintrp, "ATTR") => attr(get(10, 6), get(8, 2)),
            (Vop3, "SRC0") if kind == b'a' => attr(val & 0x3F, val >> 6 & 3),
            (Exp, "TGT") => exp_target(get(4, 6)),
            (Exp, _) => {
                let k = fname.as_bytes()[4] - b'0';
                let (en, compr) = (get(0, 4), bit(10));
                let reg = if compr { get(32 + 8 * (k as u32 / 2), 8) } else { val };
                if en >> k & 1 == 1 { format!("v{reg}") } else { "off".into() }
            }
            _ => {
                let r = match kind {
                    b'v' => range("v", val, bits),
                    b's' => src(val, bits, literal).unwrap_or_else(raw),
                    b'm' => src(val, mask_bits, literal).unwrap_or_else(raw),
                    b'k' => literal.map_or_else(raw, |l| format!("{l:#x}")),
                    b'l' => format!("{:#x}", at as i64 + i.len as i64 + 4 * (val as u16 as i16 as i64)),
                    b'w' => waitcnt(val),
                    b'h' => hwreg(val),
                    b'n' => sendmsg(val),
                    b'p' => ["p10", "p20", "p0"].get(val as usize).map_or_else(raw, |s| s.to_string()),
                    _ => format!("{val:#x}"),
                };
                if (f == Vop3 || mix) && fname.starts_with("SRC") {
                    let k = fname.as_bytes()[3] - b'0';
                    nsrc = nsrc.max(k + 1);
                    neg_abs(r, bit(61 + k as u32), !vop3b && bit(8 + k as u32))
                } else {
                    if f == Vop3p && fname.starts_with("SRC") { nsrc = nsrc.max(fname.as_bytes()[3] - b'0' + 1); }
                    r
                }
            }
        };
        ops.push(text);
    }

    let list = |bits: [bool; 4], n: u8| format!("[{}]", (0..n as usize).map(|k| (bits[k] as u8).to_string()).collect::<Vec<_>>().join(","));
    match f {
        Smem => {
            if bit(16) { mods.push("glc".into()); }
            if bit(14) { mods.push("dlc".into()); }
        }
        Vop3 => {
            let sel = if vop3b { 0 } else { get(11, 4) };
            // VOP3 interp: bit 8 of the attribute source selects the high f16 half.
            if desc.contains("SRC0:a") && get(40, 1) == 1 { mods.push("high".into()); }
            if bit(15) { mods.push("clamp".into()); }
            if let Some(o) = [None, Some("mul:2"), Some("mul:4"), Some("div:2")][get(59, 2) as usize] { mods.push(o.into()); }
            if name.starts_with("v_permlane") {
                // op_sel[0] is fetch-inactive, op_sel[1] bound_ctrl.
                if sel & 3 != 0 { mods.push(format!("op_sel:[{},{}]", sel & 1, sel >> 1 & 1)); }
            } else if sel != 0 {
                let b = [sel & 1 != 0, sel & 2 != 0, sel & 4 != 0, sel & 8 != 0];
                let mut v: Vec<bool> = b[..nsrc as usize].to_vec();
                v.push(b[3]);
                mods.push(format!("op_sel:[{}]", v.iter().map(|&x| (x as u8).to_string()).collect::<Vec<_>>().join(",")));
            }
        }
        Vop3p => {
            let sel = get(11, 3);
            let hi = get(59, 2) | (bit(14) as u32) << 2;
            let (nlo, nhi) = (get(61, 3), get(8, 3));
            let b = |v: u32| [v & 1 != 0, v & 2 != 0, v & 4 != 0, false];
            // op_sel_hi defaults to all ones for packed math, to zero (f32 sources) for v_fma_mix*.
            let all = (1 << nsrc) - 1;
            let default = if mix { 0 } else { all };
            if sel != 0 { mods.push(format!("op_sel:{}", list(b(sel), nsrc))); }
            if hi & all != default { mods.push(format!("op_sel_hi:{}", list(b(hi), nsrc))); }
            if nlo != 0 && !mix { mods.push(format!("neg_lo:{}", list(b(nlo), nsrc))); }
            if nhi != 0 && !mix { mods.push(format!("neg_hi:{}", list(b(nhi), nsrc))); }
            if bit(15) { mods.push("clamp".into()); }
        }
        Ds => {
            let (o0, o1) = (get(0, 8), get(8, 8));
            if name == "ds_swizzle_b32" {
                mods.push(format!("offset:{}", swizzle(o0 | o1 << 8)));
            } else if ["read2", "write2", "wrxchg2"].iter().any(|s| name.contains(s)) {
                if o0 != 0 { mods.push(format!("offset0:{o0}")); }
                if o1 != 0 { mods.push(format!("offset1:{o1}")); }
            } else if o0 | o1 << 8 != 0 {
                mods.push(format!("offset:{}", o0 | o1 << 8));
            }
            if bit(17) { mods.push("gds".into()); }
        }
        Mubuf | Mtbuf => {
            if f == Mtbuf && get(19, 7) != 1 { mods.push(format!("format:{}", buf_format(get(19, 7)))); }
            if idxen { mods.push("idxen".into()); }
            if offen { mods.push("offen".into()); }
            if get(0, 12) != 0 { mods.push(format!("offset:{}", get(0, 12))); }
            for (b, s) in [(14, "glc"), (54, "slc"), (15, "dlc")] { if bit(b) { mods.push(s.into()); } }
            if f == Mubuf && bit(16) { mods.push("lds".into()); }
            if bit(55) { mods.push("tfe".into()); }
        }
        Mimg if name.starts_with("image_bvh") => {
            if bit(62) { mods.push("a16".into()); }
        }
        Mimg => {
            mods.push(format!("dmask:{dmask:#x}"));
            mods.push(format!("dim:SQ_RSRC_IMG_{}", DIMS[get(3, 3) as usize]));
            for (b, s) in [(12, "unorm"), (13, "glc"), (25, "slc"), (7, "dlc"), (15, "r128"), (62, "a16"), (16, "tfe"), (17, "lwe"), (63, "d16")] {
                if bit(b) { mods.push(s.into()); }
            }
        }
        Flat | Global | Scratch => {
            let off = get(0, 12);
            let off = if f == Flat { off as i32 } else { (off << 20) as i32 >> 20 };
            if off != 0 { mods.push(format!("offset:{off}")); }
            for (b, s) in [(16, "glc"), (17, "slc"), (12, "dlc"), (13, "lds")] { if bit(b) { mods.push(s.into()); } }
        }
        Exp => {
            for (b, s) in [(11, "done"), (10, "compr"), (12, "vm")] { if bit(b) { mods.push(s.into()); } }
        }
        Vop1 | Vop2 | Vopc => match ext {
            Some((0xFA, e)) => {
                mods.push(dpp_ctrl(e >> 8 & 0x1FF));
                mods.push(format!("row_mask:{:#x}", e >> 28));
                mods.push(format!("bank_mask:{:#x}", e >> 24 & 0xF));
                if e >> 19 & 1 == 1 { mods.push("bound_ctrl:1".into()); }
                if e >> 18 & 1 == 1 { mods.push("fi:1".into()); }
            }
            Some((0xF9, e)) => {
                const SEL: [&str; 7] = ["BYTE_0", "BYTE_1", "BYTE_2", "BYTE_3", "WORD_0", "WORD_1", "DWORD"];
                let sel = |v: u32| SEL.get(v as usize & 7).map_or_else(|| format!("{:#x}", v & 7), |s| s.to_string());
                if f != Vopc {
                    if e >> 13 & 1 == 1 { mods.push("clamp".into()); }
                    if let Some(o) = [None, Some("mul:2"), Some("mul:4"), Some("div:2")][(e >> 14 & 3) as usize] { mods.push(o.into()); }
                    mods.push(format!("dst_sel:{}", sel(e >> 8)));
                    mods.push(format!("dst_unused:{}", ["UNUSED_PAD", "UNUSED_SEXT", "UNUSED_PRESERVE", "0x3"][(e >> 11 & 3) as usize]));
                }
                mods.push(format!("src0_sel:{}", sel(e >> 16)));
                if f != Vop1 { mods.push(format!("src1_sel:{}", sel(e >> 24))); }
            }
            Some((s, e)) => {
                mods.push(format!("dpp8:[{}]", (0..8).map(|k| (e >> (8 + 3 * k) & 7).to_string()).collect::<Vec<_>>().join(",")));
                if s == 0xEA { mods.push("fi:1".into()); }
            }
            None => {}
        },
        _ => {}
    }
    let mut s = m;
    if !ops.is_empty() { s += " "; s += &ops.join(", "); }
    for x in mods { s += " "; s += &x; }
    s
}

/// `ds_swizzle_b32` offset: quad permute (bit 15) or 32-lane and/or/xor bitmask.
fn swizzle(o: u32) -> String {
    if o & 0x8000 != 0 {
        return format!("swizzle(QUAD_PERM,{},{},{},{})", o & 3, o >> 2 & 3, o >> 4 & 3, o >> 6 & 3);
    }
    let (and, or, xor) = (o & 0x1F, o >> 5 & 0x1F, o >> 10 & 0x1F);
    let group = (and ^ 0x1F) + 1;
    match (and, or, xor) {
        (0x1F, 0, x) if x.is_power_of_two() => return format!("swizzle(SWAP,{x})"),
        (0x1F, 0, x) if x > 0 && (x + 1).is_power_of_two() => return format!("swizzle(REVERSE,{})", x + 1),
        (_, lane, 0) if group >= 2 && group.is_power_of_two() && lane < group => return format!("swizzle(BROADCAST,{group},{lane})"),
        _ => {}
    }
    let mut s = String::new();
    for b in (0..5).rev() {
        s.push(match (and >> b & 1, or >> b & 1, xor >> b & 1) {
            (0, 0, 0) => '0',
            (0, 1, 0) => '1',
            (1, 0, 0) => 'p',
            (1, 0, 1) => 'i',
            _ => return format!("{o:#x}"),
        });
    }
    format!("swizzle(BITMASK_PERM,\"{s}\")")
}

/// GFX10 unified buffer format names (`BUF_FMT_*`), numbered from 1 in this group order.
fn buf_format(n: u32) -> String {
    const N6: &[&str] = &["UNORM", "SNORM", "USCALED", "SSCALED", "UINT", "SINT"];
    const N7: &[&str] = &["UNORM", "SNORM", "USCALED", "SSCALED", "UINT", "SINT", "FLOAT"];
    const I3: &[&str] = &["UINT", "SINT", "FLOAT"];
    const GROUPS: [(&str, &[&str]); 14] = [("8", N6), ("16", N7), ("8_8", N6), ("32", I3), ("16_16", N7), ("10_11_11", N7), ("11_11_10", N7),
        ("10_10_10_2", N6), ("2_10_10_10", N6), ("8_8_8_8", N6), ("32_32", I3), ("16_16_16_16", N7), ("32_32_32", I3), ("32_32_32_32", I3)];
    if n == 0 { return "[BUF_FMT_INVALID]".into(); }
    let mut k = n as usize - 1;
    for (bits, types) in GROUPS {
        if k < types.len() { return format!("[BUF_FMT_{bits}_{}]", types[k]); }
        k -= types.len();
    }
    n.to_string()
}

/// SDWA source with its sext / neg / abs bits (`m` = the source's byte of the extension dword).
fn sdwa_src(r: String, m: u32) -> String {
    let r = if m >> 3 & 1 == 1 { format!("sext({r})") } else { r };
    neg_abs(r, m >> 4 & 1 == 1, m >> 5 & 1 == 1)
}

fn neg_abs(r: String, neg: bool, abs: bool) -> String {
    let r = if abs { format!("|{r}|") } else { r };
    if neg { format!("-{r}") } else { r }
}

fn attr(n: u32, chan: u32) -> String { format!("attr{n}.{}", ["x", "y", "z", "w"][chan as usize]) }

fn exp_target(t: u32) -> String {
    match t {
        0..=7 => format!("mrt{t}"),
        8 => "mrtz".into(),
        9 => "null".into(),
        12..=16 => format!("pos{}", t - 12),
        20 => "prim".into(),
        32..=63 => format!("param{}", t - 32),
        _ => format!("tgt={t:#x}"),
    }
}

fn dpp_ctrl(c: u32) -> String {
    match c {
        0x000..=0x0FF => format!("quad_perm:[{},{},{},{}]", c & 3, c >> 2 & 3, c >> 4 & 3, c >> 6 & 3),
        0x101..=0x10F => format!("row_shl:{}", c & 0xF),
        0x111..=0x11F => format!("row_shr:{}", c & 0xF),
        0x121..=0x12F => format!("row_ror:{}", c & 0xF),
        0x140 => "row_mirror".into(),
        0x141 => "row_half_mirror".into(),
        0x150..=0x15F => format!("row_share:{}", c & 0xF),
        0x160..=0x16F => format!("row_xmask:{}", c & 0xF),
        _ => format!("dpp_ctrl:{c:#x}"),
    }
}

/// GFX10 `s_waitcnt`: counters not at their maximum (all of them when none is).
fn waitcnt(s: u32) -> String {
    let c = [("vmcnt", s & 0xF | (s >> 14 & 3) << 4, 63), ("expcnt", s >> 4 & 7, 7), ("lgkmcnt", s >> 8 & 0x3F, 63)];
    let set: Vec<String> = c.iter().filter(|x| x.1 != x.2).map(|(n, v, _)| format!("{n}({v})")).collect();
    if set.is_empty() { c.iter().map(|(n, v, _)| format!("{n}({v})")).collect::<Vec<_>>().join(" ") } else { set.join(" ") }
}

fn hwreg(s: u32) -> String {
    let (id, off, size) = (s & 0x3F, s >> 6 & 0x1F, (s >> 11) + 1);
    let name = match id {
        1 => "HW_REG_MODE", 2 => "HW_REG_STATUS", 3 => "HW_REG_TRAPSTS", 4 => "HW_REG_HW_ID", 5 => "HW_REG_GPR_ALLOC",
        6 => "HW_REG_LDS_ALLOC", 7 => "HW_REG_IB_STS", 15 => "HW_REG_SH_MEM_BASES", 20 => "HW_REG_FLAT_SCR_LO",
        21 => "HW_REG_FLAT_SCR_HI", 22 => "HW_REG_XNACK_MASK", 23 => "HW_REG_HW_ID1", 24 => "HW_REG_HW_ID2",
        25 => "HW_REG_POPS_PACKER", 29 => "HW_REG_SHADER_CYCLES",
        _ => return format!("hwreg({id}, {off}, {size})"),
    };
    if (off, size) == (0, 32) { format!("hwreg({name})") } else { format!("hwreg({name}, {off}, {size})") }
}

fn sendmsg(s: u32) -> String {
    let (id, op, stream) = (s & 0xF, s >> 4 & 7, s >> 8 & 3);
    let name = match id {
        1 => "MSG_INTERRUPT", 2 => "MSG_GS", 3 => "MSG_GS_DONE", 4 => "MSG_SAVEWAVE", 5 => "MSG_STALL_WAVE_GEN",
        6 => "MSG_HALT_WAVES", 7 => "MSG_ORDERED_PS_DONE", 8 => "MSG_EARLY_PRIM_DEALLOC", 9 => "MSG_GS_ALLOC_REQ",
        10 => "MSG_GET_DOORBELL", 11 => "MSG_GET_DDID", 15 => "MSG_SYSMSG",
        _ => return format!("sendmsg({id}, {op}, {stream})"),
    };
    let gs = ["GS_OP_NOP", "GS_OP_CUT", "GS_OP_EMIT", "GS_OP_EMIT_CUT"];
    match id {
        2 if op < 4 => format!("sendmsg({name}, {}, {stream})", gs[op as usize]),
        3 if op < 4 && op != 0 => format!("sendmsg({name}, {}, {stream})", gs[op as usize]),
        3 if op == 0 && stream == 0 => format!("sendmsg({name}, GS_OP_NOP)"),
        _ if s >> 4 == 0 => format!("sendmsg({name})"),
        _ => format!("sendmsg({name}, {op}, {stream})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(words: &[u32]) -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() }
    fn one(words: &[u32], wave32: bool) -> String {
        let b = bytes(words);
        format(&b, 0, &decode(&b).unwrap(), wave32)
    }

    #[test]
    fn formats_operands() {
        // Cross-checked against llvm-mc -mcpu=gfx1030 (wave64 unless noted).
        let cases: &[(&[u32], &str)] = &[
            (&[0xBE80_03FF, 0x3F80_0000], "s_mov_b32 s0, 0x3f800000"),
            (&[0xBE80_246A], "s_and_saveexec_b64 s[0:1], vcc"),
            (&[0xBE84_2104], "s_swappc_b64 s[4:5], s[4:5]"),
            (&[0xBF8C_0070], "s_waitcnt vmcnt(0) lgkmcnt(0)"),
            (&[0xBF90_0003], "s_sendmsg sendmsg(MSG_GS_DONE, GS_OP_NOP)"),
            (&[0xB980_0801], "s_setreg_b32 hwreg(HW_REG_MODE, 0, 2), s0"),
            (&[0xBF84_0002], "s_cbranch_scc0 0xc"),
            (&[0xF408_0000, 0x0400_0010], "s_load_dwordx4 s[0:3], s[0:1], s2 offset:0x10"),
            (&[0xF408_0000, 0xFA00_0010], "s_load_dwordx4 s[0:3], s[0:1], 0x10"),
            (&[0x7E06_02F2], "v_mov_b32_e32 v3, 1.0"),
            (&[0x7E00_0501], "v_readfirstlane_b32 s0, v1"),
            (&[0x0200_0300], "v_cndmask_b32_e32 v0, v0, v1, vcc"),
            (&[0xD70F_0600, 0x0002_0300], "v_add_co_u32 v0, s[6:7], v0, v1"),
            (&[0xD402_0006, 0x0002_0300], "v_cmp_eq_f32_e64 s[6:7], v0, v1"),
            (&[0xD412_007E, 0x0002_0300], "v_cmpx_eq_f32_e64 v0, v1"),
            (&[0xD503_8100, 0x2802_0300], "v_add_f32_e64 v0, -|v0|, v1 clamp mul:2"),
            (&[0xCC0E_4800, 0x040A_0300], "v_pk_fma_f16 v0, v0, v1, v2 op_sel:[1,0,0] op_sel_hi:[0,0,1]"),
            (&[0x7E00_02FA, 0xFF01_1101], "v_mov_b32_dpp v0, v1 row_shr:1 row_mask:0xf bank_mask:0xf"),
            (&[0x7E02_14F9, 0x0006_1509], "v_cvt_f16_f32_sdwa v1, v9 dst_sel:WORD_1 dst_unused:UNUSED_PRESERVE src0_sel:DWORD"),
            (&[0xF408_0004, 0xFA00_0000], "s_load_dwordx4 s[0:3], s[8:9], null"),
            (&[0xCC21_0004, 0x0202_2507], "v_fma_mixlo_f16 v4, v7, v18, 0"),
            (&[0xCC20_4864, 0x8D89_E562], "v_fma_mix_f32 v100, v98, 1.0, -v98 op_sel:[1,0,0] op_sel_hi:[1,0,1]"),
            (&[0x7E00_0000], "v_nop"),
            (&[0xD8D4_8055, 0x1300_000B], "ds_swizzle_b32 v19, v11 offset:swizzle(QUAD_PERM,1,1,1,1)"),
            (&[0xD8D4_001F, 0x0400_000B], "ds_swizzle_b32 v4, v11 offset:swizzle(BITMASK_PERM,\"ppppp\")"),
            (&[0xD8D4_201F, 0x0200_0004], "ds_swizzle_b32 v2, v4 offset:swizzle(SWAP,8)"),
            (&[0xD8D4_3C1F, 0x0400_000B], "ds_swizzle_b32 v4, v11 offset:swizzle(REVERSE,16)"),
            (&[0xD8D4_0010, 0x0400_000B], "ds_swizzle_b32 v4, v11 offset:swizzle(BROADCAST,16,0)"),
            (&[0xE8C1_1000, 0x8002_0505], "tbuffer_load_format_xy v[5:6], v5, s[8:11], 0 format:[BUF_FMT_16_16_SNORM] offen"),
            (&[0xD742_0008, 0x0002_0181], "v_interp_p1ll_f16 v8, v0, attr1.z high"),
            (&[0xD778_4804, 0x0305_8303], "v_permlanex16_b32 v4, v3, -1, -1 op_sel:[1,0]"),
            (&[0xC800_0100], "v_interp_p1_f32_e32 v0, v0, attr0.y"),
            (&[0xC802_0002], "v_interp_mov_f32_e32 v0, p0, attr0.x"),
            (&[0xD8DC_0201, 0x0100_0002], "ds_read2_b32 v[1:2], v2 offset0:1 offset1:2"),
            (&[0xE030_3010, 0x8001_0100], "buffer_load_dword v1, v[0:1], s[4:7], 0 idxen offen offset:16"),
            (&[0xE030_0010, 0x8001_0100], "buffer_load_dword v1, off, s[4:7], 0 offset:16"),
            (&[0xF080_0F08, 0x0040_0004], "image_sample v[0:3], v[4:5], s[0:7], s[8:11] dmask:0xf dim:SQ_RSRC_IMG_2D"),
            (&[0xF080_0F0A, 0x0040_0004, 0x0000_0302], "image_sample v[0:3], [v4, v2], s[0:7], s[8:11] dmask:0xf dim:SQ_RSRC_IMG_2D"),
            (&[0xDC30_8000, 0x007D_0002], "global_load_dword v0, v[2:3], off"),
            (&[0xDC30_8010, 0x0002_0002], "global_load_dword v0, v2, s[2:3] offset:16"),
            (&[0xDC30_4010, 0x0002_0002], "scratch_load_dword v0, off, s2 offset:16"),
            (&[0xF800_0C05, 0x0302_0100], "exp mrt0, v0, off, v1, off done compr"),
            (&[0xF800_08CF, 0x0306_0504], "exp pos0, v4, v5, v6, v3 done"),
        ];
        for (w, want) in cases { assert_eq!(one(w, false), *want, "{w:x?}"); }
        // wave32 masks
        assert_eq!(one(&[0x0200_0300], true), "v_cndmask_b32_e32 v0, v0, v1, vcc_lo");
        assert_eq!(one(&[0xD70F_0600, 0x0002_0300], true), "v_add_co_u32 v0, s6, v0, v1");
        // reserved source code: raw field
        assert_eq!(one(&[0xD503_0000, 0x0002_02D1], false), "v_add_f32_e64 v0, src0=0xd1, v1");
    }

    #[test]
    fn operand_table_matches_opcodes() {
        assert!(OPERANDS.windows(2).all(|p| (p[0].0, p[0].1) < (p[1].0, p[1].1)));
        let ops = super::super::rdna2_ops::OPCODES;
        assert_eq!(ops.len(), OPERANDS.len());
        assert!(ops.iter().zip(OPERANDS).all(|(a, b)| (a.0, a.1) == (b.0, b.1)));
    }

    #[test]
    fn lists_program() {
        // s_mov_b64 exec, -1 ; s_branch +1 ; data ; v_mov_b32 v0, 1.0 ; s_endpgm ; s_code_end x2
        let code = bytes(&[0xBEFE_04C1, 0xBF82_0001, 0xDEAD_BEEF, 0x7E00_02F2, 0xBF81_0000, S_CODE_END, S_CODE_END]);
        let (st, lines) = disassemble(&code, false);
        let text: Vec<String> = lines.iter().map(|l| format!("{:#x} {}", l.addr, l.text)).collect();
        assert_eq!(text, ["0x0 s_mov_b64 exec, -1", "0x4 s_branch 0xc", "0x8 .long 0xdeadbeef ; unreached", "0xc v_mov_b32_e32 v0, 1.0", "0x10 s_endpgm"]);
        assert_eq!(st.insts as usize, lines.iter().filter(|l| l.inst.is_some()).count());
    }
}
