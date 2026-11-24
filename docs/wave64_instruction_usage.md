# Wave64 Instruction Usage in the Shader Database

Which wave64-sensitive RDNA2 (GFX10.3) instructions the game's native shader
programs actually execute, counted per program and per stage. Extracted with
`forge9 isa` from the decrypted `shaders.rpf`. Code comes from each record's
SGD5 → DXBC → XBCH → FSSM program, at the FSSM stage's code offset and size.

```
forge9 isa shaders.rpf 0 --summary      # aggregate (this document)
forge9 isa shaders.rpf 0                # one line per program
forge9 isa shaders.rpf 0 --shader N     # full family breakdown of one record
```

## Coverage and validation

| Entry | Records | Programs | Instructions | Decoded cleanly | Unknown dwords |
|---|---|---|---|---|---|
| 0 (271 MB SGD5) | 24,804 | 25,194 | 44,251,337 | 25,194 | 0 |
| 2 (3 MB SGD5) | 1,699 | 1,699 | 141,396 | 1,699 | 0 |

- **Decoding:** each program is decoded by following control flow from its
  first instruction through `s_branch`, `s_cbranch_*` and `s_call_b64`. A path
  ends at `s_endpgm*`, `s_setpc_b64` or `s_code_end`. A program is *clean*
  when every path decodes, no branch lands outside the code or inside another
  instruction, and at least one path exits. A linear sweep is not enough:
  many programs keep constants (or a second, unreachable `s_endpgm`) after the
  last exit and before the `s_code_end` padding.
- **Cross-check:** at all 44,251,337 reachable instruction offsets in entry 0
  (and all 141,396 in entry 2), forge9's hand-written bit-pattern decoder
  agrees with the xcl project's decoder on instruction length, encoding family
  and wave-op class. xcl's decoder is generated from AMD's machine-readable
  RDNA2 ISA XML and includes the console-only GFX10.1 opcodes. The only
  differences are naming: xcl calls VOP3B (`v_add_co_u32` and similar)
  `VOP3_SDST_ENC`.
- **Sanity check:** none of the 2,662 wave32 programs uses a 64-bit EXEC/VCC
  op, a 64-bit mask scan, `v_mbcnt_hi` or a constant lane ≥ 32. Those classes
  only show up where the FSSM wave64 flag is set.
- **ES programs** (195) have no `s_endpgm`. They are the ES half of merged
  ES+GS programs and end with `s_setpc_b64` into the GS half. That counts as
  an exit.

## Which ops actually cross the two 32-lane halves

To run a wave64 program as two wave32 halves (NVIDIA warps, or RDNA in
wave32 mode), a translator needs to know which instructions mix lanes from
both halves. On GFX10 the answer is short, because the GFX9 wave-wide DPP
forms (`wave_shl`, `row_bcast15/31` and others) were removed:

| Op class | Lane reach | Crosses halves? |
|---|---|---|
| DPP8 | within each 8 lanes | no |
| DPP16 (`quad_perm`, `row_shl/shr/ror`, `row_mirror`, `row_share/xmask`) | within each 16-lane row | no |
| `v_permlane16_b32` / `v_permlanex16_b32` | within a row / between the two rows of a 32-lane half | no |
| `ds_swizzle_b32` (quad and bitmask modes) | within 32 lanes | no |
| `v_readlane` / `v_writelane`, constant lane 32..63 | names an upper-half lane | **yes** |
| `v_readlane` / `v_writelane`, lane in an SGPR | any lane | **maybe** (dynamic) |
| `v_readfirstlane_b32` | first active lane of 64 | **yes** |
| `v_mbcnt_hi_u32_b32` | lane index / prefix count for lanes 32..63 | **yes** |
| 64-bit EXEC/VCC SALU (`s_and_saveexec_b64`, `s_and_b64 exec`, …) | whole-wave mask | **yes** |
| `s_bcnt1_i32_b64`, `s_ff1_i32_b64`, `s_flbit_i32_b64` | whole-wave ballot | **yes** |
| `s_wqm_b64`, `s_quadmask_b64` | whole-wave mask, per-quad | yes (mask width only) |
| `ds_permute_b32` / `ds_bpermute_b32` | any of 64 lanes | **yes** |

## Results (entry 0)

Wave64 programs per stage: CS 1,112, ES 195, GS 195, MS 9,282, PS 9,817,
VS 1,931 (22,532 in total). Every MS, VS, ES and GS program is wave64. The
next table counts **wave64 programs that use each op at least once**. The
`w32` column counts wave32 programs that use the op, for comparison.

| Op | wave64 programs | CS | ES | GS | MS | PS | VS | w32 | Occurrences |
|---|---|---|---|---|---|---|---|---|---|
| exec_vcc_b64 | 20,704 | 866 | 195 | 195 | **9,282** | 8,994 | 1,172 | 0 | 1,254,043 |
| readfirstlane | 14,518 | 504 | 0 | 0 | **9,282** | 4,180 | 552 | 2,419 | 221,937 |
| mbcnt_hi | 10,034 | 176 | 195 | 195 | **9,282** | 186 | 0 | 0 | 10,713 |
| quad_wqm_b64 | 7,738 | 46 | 0 | 0 | 0 | 7,692 | 0 | 0 | 18,554 |
| ds_swizzle | 5,497 | 53 | 0 | 0 | 42 | 5,402 | 0 | 774 | 105,226 |
| dpp_quad_perm | 2,096 | 17 | 0 | 0 | 0 | 2,079 | 0 | 362 | 30,179 |
| readlane (lane < 32 or SGPR) | 1,131 | 143 | 0 | 0 | 73 | 914 | 1 | 1,881 | 74,880 |
| mask_scan_b64 | 631 | 159 | 0 | 0 | 40 | 432 | 0 | 0 | 2,017 |
| lane_32_63 (read/writelane) | 611 | 147 | 0 | 0 | 17 | 446 | 1 | 0 | 3,391 |
| permlane | 561 | 108 | 0 | 0 | 17 | 435 | 1 | 1,732 | 9,021 |
| dpp_row_mirror | 560 | 107 | 0 | 0 | 17 | 435 | 1 | 1,731 | 8,659 |
| dpp8 | 560 | 107 | 0 | 0 | 17 | 435 | 1 | 1,731 | 25,977 |
| writelane (lane < 32 or SGPR) | 381 | 38 | 0 | 0 | 56 | 287 | 0 | 792 | 29,420 |
| dpp_row_shift | 37 | 37 | 0 | 0 | 0 | 0 | 0 | 10 | 1,448 |
| ds_permute / ds_bpermute | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 | 74 |
| dpp_row_share / dpp_row_xmask | 0 | | | | | | | 0 | 0 |
| s_movrel* | 0 | | | | | | | 0 | 0 |

Occurrence totals cover wave32 and wave64 programs together.

Grouped (wave64 programs):

| Group | Programs | CS | ES | GS | MS | PS | VS |
|---|---|---|---|---|---|---|---|
| Crosses the 32-lane halves (ds_permute, lane 32..63, readfirstlane, mbcnt_hi, 64-bit masks) | 20,810 | 905 | 195 | 195 | 9,282 | 8,997 | 1,236 |
| Any 64-bit mask op (exec/vcc b64, mask scan, mbcnt_hi, quad/wqm) | 20,705 | 867 | 195 | 195 | 9,282 | 8,994 | 1,172 |
| Any lane read/write (readlane, writelane, readfirstlane) | 14,626 | 545 | 0 | 0 | 9,282 | 4,247 | 552 |
| Any permute (ds_[b]permute, permlane, swizzle) | 5,816 | 157 | 0 | 0 | 59 | 5,599 | 1 |
| Any DPP16/DPP8 | 2,495 | 113 | 0 | 0 | 17 | 2,364 | 1 |

Instruction mix, all programs: VOP2 34.0%, VOP3 16.4%, VOP1 11.7%, SOPP
10.3%, SOP2 7.1%, SMEM 6.0%, SOP1 5.6%, VOPC 2.5%, MUBUF 1.5%, VOP3P 1.2%,
SOPC 1.1%, MIMG 1.0%, VINTRP 0.5%, SOPK 0.4%, DS 0.3%, EXP 0.3%, MTBUF 0.1%,
GLOBAL < 0.1%. No FLAT or SCRATCH instructions.

Entry 2 (1,699 programs, all wave64: CS 42, PS 1,157, VS 500) is much
simpler: exec_vcc_b64 in 1,044 programs, quad_wqm_b64 in 1,015,
readfirstlane in 26, ds_swizzle in 25, readlane with lane 32..63 in 8 (CS).
It has no DPP, permlane, ds_permute, mbcnt_hi or mask scans.

## What this means for translators

1. **Mesh shaders need only a small wave64 core.** All 9,282 MS programs use
   64-bit EXEC/VCC masks, `v_readfirstlane_b32` and `v_mbcnt_hi_u32_b32`. In
   the sampled program, `v_mbcnt_lo/hi(-1)` builds the 0..63 lane index. Only
   17 MS programs use DPP or permlane, 42 use `ds_swizzle`, 73 use readlane,
   56 use writelane and 40 use a 64-bit ballot scan. To get MS working, a
   correct 64-bit EXEC/VCC model, lane index, first-active-lane and
   divergence handling come first. Cross-lane shuffles barely matter.
2. **No shader in this database uses `ds_permute_b32` / `ds_bpermute_b32` in
   wave64**, the one fully arbitrary 64-lane shuffle. Five wave32 programs
   use it. `s_movrel*` and the GFX10-specific DPP `row_share` / `row_xmask`
   never appear.
3. **The DPP / permlane ops belong to one subgroup-reduction sequence.**
   dpp8, dpp_row_mirror and permlane show up in nearly the same programs
   (560/560/561 wave64). In the sampled mesh shader `MS_VS_TransformFire`
   (record #594) a `WaveActiveMin` lowers to 3× `v_min_u32_dpp8` (within 8
   lanes), `v_min_u32_dpp` `row_mirror` (within 16),
   `v_permlanex16_b32` (within 32), then `v_readlane_b32 s, v, 32` plus
   `v_readlane_b32 s, v, 0` to combine the halves. Only the final readlane of
   lane 32 crosses halves. That is why the 611 `lane_32_63` programs closely
   track the reduction users.
4. **Pixel shaders** add whole-quad mode (`s_wqm_b64` in 7,692 wave64 PS) for
   derivatives, plus `ds_swizzle` (5,402) and DPP `quad_perm` (2,079), which
   stay inside a quad or a 32-lane half. 1,127 PS programs also ship as wave32.
5. **Compute shaders** use the widest mix for their count: all 37
   `row_shl/shr/ror` users, and of the 1,112 wave64 CS, 147 read lanes
   32..63 and 159 use a 64-bit mask scan. 1,535 of 2,647 CS programs are
   wave32.

## Accuracy notes and limits

- **What is classified:** this is a length decoder and classifier, not a
  disassembler. Opcode-level classification covers only the ops listed
  above. Everything else is counted by encoding family alone.
- **Wave-dependent ops not counted:** VOPC / VOP3 compares writing VCC or an
  SGPR pair, `v_cndmask_b32`, carry-in/out ops (`v_add_co_ci_u32` etc.) and
  `s_cbranch_execz/vccz` depend on wave size too: they read or write 64-bit
  masks in wave64. They are per-lane and appear in nearly every program, so
  they are not tallied.
- **`exec_vcc_b64` rule:** counts every `s_*_saveexec_b64` /
  `s_*_wrexec_b64`, plus 64-bit `s_mov/cmov/not/bitset` and SOP2 64-bit logic
  ops / `s_cselect_b64` when a source or destination is EXEC or VCC. A
  64-bit mask copied through other SGPRs first is not counted.
- **Lane select:** `readlane` / `writelane` with the lane in an SGPR are
  counted as `readlane` / `writelane`. The lane is not known statically.
- **DPP16 sub-classes** come from forge9's own reading of the `dpp_ctrl`
  field. The xcl cross-check confirms DPP16 vs DPP8 vs SDWA, but not the
  `quad_perm` / row sub-class.
- **`ds_swizzle`** is not split by mode. Both modes stay within 32 lanes.
- **Not in GFX10.3:** VOP3 DPP is a GFX11 feature and cannot appear here.
  The decoder sizes VOP3 without a DPP dword.
- **Missed code:** code reachable only through an indirect jump
  (`s_setpc_b64` / `s_swappc_b64` to a computed address) would not be
  decoded. `s_swappc_b64` is treated as a call that returns. No program
  ended unclean, so no reachable path ran into data.
- **Console opcodes:** the GFX10.1 opcodes the console keeps
  (`v_mac_f32`, `v_madmk_f32`, `v_madak_f32`, `v_mad_f32`) are sized
  correctly. VOP2 0x20 and 0x21 carry a literal that the GFX10.3 tables
  do not list.
