//! Conservative EXEC-uniformity analysis of RDNA2 programs.
//!
//! For every reachable vector instruction (VALU, VMEM, DS, EXP) decide whether
//! EXEC *provably* covers every live lane when it runs. Those are UNIFORM: a
//! translator can drop their EXEC guard. Everything else is GUARDED. Scalar
//! instructions (SALU, SMEM, SOPP) are not per-lane and never counted.
//!
//! Forward dataflow over the CFG [`rdna2::decode`] walks (plus the
//! `s_getpc`/`s_add_u32`/`s_setpc` long jump). Every SGPR (s0..s105, VCC, EXEC, ...)
//! holds an abstract lane mask relative to the entry mask `E` (the live lanes) and
//! `W ⊇ E`, the lanes that exist for a translator: `E` plus, in a pixel shader, the
//! helper lanes of its quads. Lanes outside `W` do not exist; helper lanes have no
//! side effects, so running them is invisible.
//!
//! | value | meaning |
//! |---|---|
//! | `Full` | `E ⊆ mask ⊆ W`: every live lane |
//! | `Sub` | `mask ⊆ W` (`0` included) |
//! | `Top` | unknown |
//!
//! Rules (join at CFG merges is the pointwise max, `Full < Sub < Top`):
//!
//! 1. At entry EXEC is `Full`, every other register `Top`. The constant `-1` is
//!    `Full` (`s_mov_b64 exec, -1` opens every lane of `W`), `0` and `null` are `Sub`,
//!    other constants `Top`. `s_wqm` keeps `Full`/`Sub` (it only adds helper lanes).
//! 2. Mask ops of the wave's mask width (`_b64` in wave64, `_b32` in wave32)
//!    are evaluated: `s_mov`, `s_cmov` (join of old and new), `s_and/or/xor/andn2`
//!    and the `s_*_saveexec` / `s_*_wrexec` forms. `and` with anything `⊆ W` is
//!    `Sub` (`Full & Full` is `Full`); `or` with `Top` is `Top`, else `Full` if
//!    either side is; `xor` of two `⊆ W` is `Sub`; `a & ~b` is `Sub` when `a ⊆ W`.
//!    `orn2`, `nand`, `nor`, `xnor`, `not` and `cselect` give `Top`.
//! 3. So `s_and_saveexec s[n], vcc` makes EXEC `Sub` and saves `Full` in `s[n]`;
//!    `s_mov_b64 exec, s[n]` or `s_or_b64 exec, exec, s[n]` restores `Full` only
//!    if `s[n]` still provably holds a `Full` mask on every path.
//! 4. `v_cmpx_*` (VOPC or VOP3) leaves EXEC `⊆` its old value: `Sub` at best.
//! 5. Any other SGPR write makes its registers `Top`: SALU `sdst` (two registers
//!    for 64-bit opcodes), SMEM `sdata` (the load width), VOP3 `v_cmp` / carry-out /
//!    `v_div_scale` / `v_mad_*64` `sdst` (one mask), `v_readlane` / `v_readfirstlane`,
//!    VCC from VOPC/VOP2. A write that lands on EXEC this way makes EXEC `Top`.
//! 6. `s_call_b64`, `s_swappc_b64`, `s_movreld_*`, `s_subvector_loop_*` and any
//!    SALU/VOP3 opcode without a GFX10.3 name make every register `Top`.
//! 7. Branches (`s_cbranch_execz/execnz`, `vccz`, `scc*`, `s_branch`) change no
//!    mask; the region they skip is already `Sub` from rule 3/4, and merges join.
//!
//! Every rule only ever moves a value toward `Top`, so UNIFORM is a provable
//! subset: real foldability is at least this high. Known gaps (all toward GUARDED):
//! no relational reasoning (a loop's break mask OR'd back into EXEC stays `Sub`),
//! VCC from `v_cmp` is `Top` though it is `⊆ EXEC`, literal masks other than
//! `0`/`-1` are `Top`, and an early-out (lanes dropped and never re-enabled) keeps
//! the rest of the program `Sub` even though a translator could end those lanes.
//!
//! `guard_runs` counts the guards left if one guard covers each straight-line run
//! of GUARDED vector instructions (`s_waitcnt` / `s_nop` between them allowed)
//! that no EXEC write splits: EXEC is the same mask across a run.

use super::rdna2::{decode, opcode_name, Family, Flow, Inst};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum M { Full, Sub, Top }

type Regs = [M; 128];
const EXEC: u32 = 126;
const VCC: u32 = 106;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Uniformity {
    /// Reachable vector instructions (VALU, VMEM, DS, EXP).
    pub vector: u32,
    /// Vector instructions provably under a full EXEC (guard foldable).
    pub uniform: u32,
    /// Straight-line runs of guarded vector instructions under one EXEC value.
    pub guard_runs: u32,
}

impl Uniformity {
    pub fn guarded(&self) -> u32 { self.vector - self.uniform }
}

fn is_vector(f: Family) -> bool {
    use Family::*;
    matches!(f, Vop1 | Vop2 | Vopc | Vop3 | Vop3p | Vintrp | Ds | Mubuf | Mtbuf | Mimg | Flat | Global | Scratch | Exp)
}

fn logic(op: &str, a: M, b: M) -> M {
    use M::*;
    let sub = |x: M| x <= Sub;
    match op {
        "and" if a == Full && b == Full => Full,
        "and" if sub(a) || sub(b) => Sub,
        "or" if a == Top || b == Top => Top,
        "or" if a == Full || b == Full => Full,
        "or" => Sub,
        "xor" if sub(a) && sub(b) => Sub,
        "andn2" if sub(a) => Sub,
        _ => Top,
    }
}

/// Applies `i` (first dwords `w`, `w1`) to `r`. Returns `true` if it clobbers everything.
fn step(r: &mut Regs, i: &Inst, w: u32, w1: u32, wave32: bool) -> bool {
    let n = if wave32 { 1 } else { 2 };
    let name = opcode_name(i.family, i.op).unwrap_or("");
    let read = |r: &Regs, c: u32| match c {
        125 | 128 => M::Sub, // null, inline 0
        193 => M::Full,      // inline -1
        255 if w1 == 0 => M::Sub,
        255 if w1 == u32::MAX && n == 1 => M::Full, // a 32-bit literal may not reach lanes 32..63
        0..=127 => (c..c + n).map(|k| r.get(k as usize).copied().unwrap_or(M::Top)).max().unwrap(),
        _ => M::Top,
    };
    let set = |r: &mut Regs, d: u32, count: u32, v: M| for k in d..(d + count).min(128) { if k != 125 { r[k as usize] = v } };
    // `s_and_saveexec_b64` → ("and_saveexec", 2)
    let width = if name.ends_with("_b64") { 2 } else if name.ends_with("_b32") { 1 } else { 0 };
    // Registers an opcode we do not evaluate may write.
    let kill = if name.contains("64") { 2 } else { 1 };
    let base = name.strip_prefix("s_").unwrap_or(name).trim_end_matches("_b64").trim_end_matches("_b32");
    let sdst = (w >> 16) & 0x7F;
    let (s0, s1) = (w & 0xFF, (w >> 8) & 0xFF);
    match i.family {
        Family::Sop1 => {
            if name.is_empty() || name.starts_with("s_movreld") || name == "s_swappc_b64" { return true; }
            let exec_op = base.strip_suffix("_saveexec").map(|b| (b, true)).or(base.strip_suffix("_wrexec").map(|b| (b, false)));
            match exec_op {
                Some((op, save)) if width == n => {
                    let (e, s) = (read(r, EXEC), read(r, s0));
                    let new = match op { "andn1" => logic("andn2", e, s), _ => logic(op, s, e) };
                    set(r, sdst, n, if save { e } else { new });
                    set(r, EXEC, n, new);
                }
                Some(_) => { set(r, sdst, 2, M::Top); set(r, EXEC, 2, M::Top); }
                // The whole-quad closure of the live lanes adds only helper lanes: see rule 1.
                None if (base == "mov" || base == "wqm") && width == n => { let v = read(r, s0); set(r, sdst, n, v); }
                None if base == "cmov" && width == n => { let v = read(r, s0).max(read(r, sdst)); set(r, sdst, n, v); }
                None if name == "s_setpc_b64" || name == "s_rfe_b64" => {} // no destination
                None => set(r, sdst, kill, M::Top),
            }
        }
        Family::Sop2 => {
            if name.is_empty() { return true; }
            let v = if width == n { logic(base, read(r, s0), read(r, s1)) } else { M::Top };
            set(r, sdst, if width == n { n } else { kill }, v);
        }
        Family::Sopk => {
            if name.is_empty() || name == "s_call_b64" || name.starts_with("s_subvector_loop") { return true; }
            if !["s_cmpk", "s_waitcnt", "s_setreg"].iter().any(|p| name.starts_with(p)) { set(r, sdst, kill, M::Top); }
        }
        Family::Smem => {
            let count = match name.rsplit_once('x').and_then(|(_, n)| n.parse().ok()) {
                Some(n) => n,
                None if name.ends_with("dword") => 1,
                None if name.starts_with("s_mem") => 2,
                None if name.ends_with("_inv") => 0,
                None => 16,
            };
            set(r, (w >> 6) & 0x7F, count, M::Top);
        }
        Family::Vop1 if name == "v_readfirstlane_b32" => set(r, (w >> 17) & 0xFF, 1, M::Top),
        Family::Vop2 => set(r, VCC, 2, M::Top),
        Family::Vopc => {
            set(r, VCC, 2, M::Top);
            if w & 0x1FF == 0xF9 { set(r, (w1 >> 8) & 0x7F, n, M::Top); } // SDWA sdst
            if name.starts_with("v_cmpx") || name.is_empty() { let e = logic("and", read(r, EXEC), M::Top); set(r, EXEC, n, e); }
        }
        Family::Vop3 => {
            if name.is_empty() { return true; }
            if name.starts_with("v_cmp") {
                let e = logic("and", read(r, EXEC), M::Top);
                set(r, w & 0xFF, n, M::Top);
                if name.starts_with("v_cmpx") { set(r, EXEC, n, e); }
            } else if name.contains("_co_") || name.starts_with("v_div_scale") || name.starts_with("v_mad_u64") || name.starts_with("v_mad_i64") {
                set(r, (w >> 8) & 0x7F, n, M::Top);
            } else if name == "v_readlane_b32" || name == "v_readfirstlane_b32" {
                set(r, w & 0xFF, 1, M::Top);
            }
        }
        _ => {}
    }
    false
}

struct Node { at: usize, i: Inst, succ: [Option<u32>; 2] }

fn exec_of(r: &Regs, wave32: bool) -> M {
    if wave32 { r[EXEC as usize] } else { r[EXEC as usize].max(r[EXEC as usize + 1]) }
}

/// Target of `s_getpc_b64 s[a:b]; s_add_u32 sa, sa, lit; s_addc_u32 sb, sb, 0; s_setpc_b64 s[a:b]`
/// ending at `at`: the compiler's branch for offsets past the 16-bit `simm` of `s_branch`.
fn long_jump(code: &[u8], at: usize) -> Option<usize> {
    let dw = |at: usize| code.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let op = |at: usize| decode(code.get(at..)?).and_then(|i| opcode_name(i.family, i.op));
    let (setpc, getpc, add) = (dw(at)?, dw(at.checked_sub(16)?)?, dw(at - 12)?);
    let reg = setpc & 0xFF;
    let ok = op(at) == Some("s_setpc_b64") && op(at - 16) == Some("s_getpc_b64") && (getpc >> 16) & 0x7F == reg
        && op(at - 12) == Some("s_add_u32") && add & 0xFF == reg && (add >> 8) & 0xFF == 255 && (add >> 16) & 0x7F == reg
        && op(at - 4) == Some("s_addc_u32");
    // s_getpc_b64 returns the address of the instruction after it.
    ok.then(|| (at - 12).checked_add_signed(dw(at - 8)? as i32 as isize)).flatten()
}

/// The reachable instructions (the walk of `rdna2::analyze`, plus long jumps) with successor indices.
fn graph(code: &[u8]) -> Vec<Node> {
    let mut index = vec![u32::MAX; code.len() / 4];
    let mut nodes: Vec<(usize, Inst, [Option<usize>; 2])> = Vec::new();
    let mut todo = vec![0usize];
    while let Some(at) = todo.pop() {
        if index.get(at / 4).is_none_or(|&k| k != u32::MAX) { continue; }
        let Some(i) = decode(&code[at..]).filter(|i| !i.code_end) else { continue };
        let next = at + i.len;
        let target = |d: i16| (next / 4).checked_add_signed(d as isize).map(|t| t * 4);
        let succ = match i.flow {
            _ if i.family == Family::Unknown => [None, None],
            Flow::Next => [Some(next), None],
            Flow::Jump(d) => [target(d), None],
            Flow::Branch(d) => [target(d), Some(next)],
            Flow::End => [long_jump(code, at), None],
        };
        index[at / 4] = nodes.len() as u32;
        nodes.push((at, i, succ));
        todo.extend(succ.iter().flatten());
    }
    let idx = |t: Option<usize>| t.and_then(|t| index.get(t / 4).copied()).filter(|&j| j != u32::MAX);
    nodes.into_iter().map(|(at, i, s)| Node { at, i, succ: [idx(s[0]), idx(s[1])] }).collect()
}

/// Classifies every reachable vector instruction of one program.
pub fn analyze(code: &[u8], wave32: bool) -> Uniformity {
    let dw = |at: usize| code.get(at..at + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    let g = graph(code);
    let mut state: Vec<Option<Regs>> = vec![None; g.len()];
    let mut entry = [M::Top; 128];
    entry[EXEC as usize] = M::Full;
    entry[EXEC as usize + 1] = M::Full;
    let mut work = Vec::new();
    if !g.is_empty() { state[0] = Some(entry); work.push(0); }
    while let Some(k) = work.pop() {
        let n = &g[k];
        let mut r = state[k].unwrap();
        if step(&mut r, &n.i, dw(n.at), dw(n.at + 4), wave32) { r = [M::Top; 128]; }
        for &j in n.succ.iter().flatten() {
            let j = j as usize;
            let merged = state[j].map_or(r, |o| std::array::from_fn(|q| o[q].max(r[q])));
            if state[j] != Some(merged) { state[j] = Some(merged); work.push(j); }
        }
    }

    let mut preds = vec![0u32; g.len()];
    for n in &g { for &j in n.succ.iter().flatten() { preds[j as usize] += 1; } }
    let mut order: Vec<usize> = (0..g.len()).filter(|&k| state[k].is_some()).collect();
    order.sort_by_key(|&k| g[k].at);
    let mut u = Uniformity::default();
    // Whether a guarded run is open after the previous instruction in address order.
    let (mut open, mut prev): (bool, Option<usize>) = (false, None);
    for k in order {
        let n = &g[k];
        let falls_in = prev.is_some_and(|p| g[p].at + g[p].i.len == n.at && g[p].succ == [Some(k as u32), None]) && preds[k] == 1;
        open &= falls_in;
        prev = Some(k);
        let name = opcode_name(n.i.family, n.i.op).unwrap_or("");
        if n.i.family == Family::Sopp && n.i.flow == Flow::Next && (name.starts_with("s_waitcnt") || name == "s_nop") { continue; }
        if !is_vector(n.i.family) { open = false; continue; }
        u.vector += 1;
        if exec_of(&state[k].unwrap(), wave32) == M::Full { u.uniform += 1; open = false; continue; }
        u.guard_runs += !open as u32;
        // A run ends at an EXEC write (v_cmpx, or an SGPR destination that is EXEC).
        let mut r = entry;
        open = !step(&mut r, &n.i, dw(n.at), dw(n.at + 4), wave32) && exec_of(&r, wave32) == M::Full;
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(words: &[u32]) -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() }

    const V_MOV: u32 = 0x7E00_0280; // v_mov_b32 v0, 0
    const V_CMP: u32 = 0x7C08_0080; // v_cmp_gt_f32 vcc, 0, v0  (VOPC 0x04)
    const SAVEEXEC: u32 = 0xBE80_246A; // s_and_saveexec_b64 s[0:1], vcc
    const EXECZ_2: u32 = 0xBF88_0002; // s_cbranch_execz +2
    const RESTORE: u32 = 0xBEFE_0400; // s_mov_b64 exec, s[0:1]
    const OR_RESTORE: u32 = 0x88FE_007E; // s_or_b64 exec, exec, s[0:1]
    const ENDPGM: u32 = 0xBF81_0000;

    fn run(prog: &[u32]) -> (u32, u32, u32) {
        let u = analyze(&bytes(prog), false);
        (u.vector, u.uniform, u.guard_runs)
    }

    #[test]
    fn saveexec_region_is_guarded() {
        // U U | saveexec ; execz ; G G (one run) | restore ; U ; end
        assert_eq!(run(&[V_MOV, V_CMP, SAVEEXEC, EXECZ_2, V_MOV, V_MOV, RESTORE, V_MOV, ENDPGM]), (5, 3, 1));
        // The s_or_b64 restore form works too.
        assert_eq!(run(&[V_MOV, V_CMP, SAVEEXEC, EXECZ_2, V_MOV, V_MOV, OR_RESTORE, V_MOV, ENDPGM]), (5, 3, 1));
        // Clobbering the saved mask (s_mov_b32 s0, 0) inside the region: no proof of restore.
        assert_eq!(run(&[V_MOV, V_CMP, SAVEEXEC, EXECZ_2, 0xBE80_0380, V_MOV, RESTORE, V_MOV, ENDPGM]), (4, 2, 2));
        // v_cmpx_gt_f32 (VOPC 0x14) writes EXEC: everything after is guarded, and it splits runs.
        assert_eq!(run(&[V_MOV, 0x7C28_0080, V_MOV, 0x7C28_0080, V_MOV, ENDPGM]), (5, 2, 2));
        // s_mov_b64 exec, -1 reopens every lane.
        assert_eq!(run(&[0x7C28_0080, V_MOV, 0xBEFE_04C1, V_MOV, ENDPGM]), (3, 2, 1));
        // A loop: s_mov_b64 s[2:3], exec ; L: v_mov ; s_andn2_b64 exec, exec, vcc ; s_cbranch_execnz L ; s_mov_b64 exec, s[2:3] ; v_mov
        assert_eq!(run(&[0xBE82_047E, V_MOV, 0x8AFE_6A7E, 0xBF89_FFFD, 0xBEFE_0402, V_MOV, ENDPGM]), (2, 1, 1));
        // Long jump: s_getpc s[4:5] ; s_add_u32 s4, s4, lit ; s_addc_u32 s5, s5, 0 ; s_setpc s[4:5]
        let jump = |lit: u32| [0xBE84_1F00, 0x8004_FF04, lit, 0x8205_8005, 0xBE80_2004];
        assert_eq!(run(&[&jump(16)[..], &[V_MOV, ENDPGM]].concat()), (1, 1, 0));
        // ... over a guarded v_mov to the restore (s_cbranch_execnz +5 skips the jump).
        assert_eq!(run(&[&[V_CMP, SAVEEXEC, 0xBF89_0005][..], &jump(20), &[V_MOV, RESTORE, V_MOV, ENDPGM]].concat()), (3, 2, 1));
    }
}
