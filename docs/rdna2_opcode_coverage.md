# RDNA2 Opcode Coverage of the Shader Database

Every distinct RDNA2 (GFX10.3) instruction the game's native shader programs
execute, with how often and in how many programs. This is the complete target
set for a direct RDNA2→DXIL (or RDNA2→HLSL) shader backend: implement these
opcodes and every shipped program translates; implement the 99% core first and
almost every instruction executed is covered.

Extracted with `forge9 isa` from the decrypted `shaders.rpf`, using the same
control-flow decoder as [wave64_instruction_usage.md](wave64_instruction_usage.md)
(only reachable code is counted; embedded constants are not).

```
forge9 isa shaders.rpf 0 --opcodes             # census across all programs (this document)
forge9 isa shaders.rpf 0 --opcodes --stage MS  # one hardware stage
forge9 isa shaders.rpf 0 --shader N --dump out # raw machine code of record N → out.<n>.<stage>.bin
```

## Summary

| Entry | Programs | Instructions | Distinct opcodes | Cover 90% | Cover 99% | Cover 99.9% | Unnamed |
|---|---|---|---|---|---|---|---|
| 0 (271 MB SGD5) | 25,194 | 44,251,337 | 470 | 85 | 194 | 282 | 0 |
| 2 (3 MB SGD5) | 1,699 | 141,396 | 182 | 35 | 87 | 136 | 0 |

- **470 distinct opcodes** across all 25,194 programs of entry 0.
  Entry 2 uses 182, none of them outside entry 0's set.
- **194 opcodes cover 99%** of the 44,251,337 instructions: the
  "must implement" core. 85 cover 90%, 282 cover 99.9%.
- **Long tail:** 66 opcodes appear in 10 programs or fewer
  (1,106 instructions in total), but a program
  only translates if *every* opcode in it does, so the tail still matters for
  full coverage. 76 opcodes appear in at least half of all programs.
- **Unnamed: 0.** Every opcode decoded maps to a mnemonic (see
  [Naming and unknowns](#naming-and-unknowns)); no instruction fell into the
  synthetic `FAMILY_op0x…` bucket and no dword was undecodable.

Distinct opcodes by encoding family (entry 0): VOP3 131, VOPC 58, VOP1 42, VOP2 38, SOP2 32, MUBUF 25, SOP1 24, MIMG 24, DS 23, SOPP 15, SOPK 14, SOPC 12, VOP3P 10, SMEM 9, GLOBAL 5, MTBUF 4, VINTRP 3, EXP 1.
VOP3 counts VOP1/VOP2/VOPC ops promoted to the 64-bit encoding separately from
their 32-bit forms (e.g. `v_mul_legacy_f32` appears as both VOP2 and VOP3): a
backend implements the operation once, but must decode both encodings.

## Top opcodes by frequency (entry 0)

| # | Occurrences | Share | Cum. | Programs | Family | Opcode |
|---|---|---|---|---|---|---|
| 1 | 5,482,918 | 12.39% | 12.39% | 23,091 | VOP2 | `v_mul_legacy_f32` |
| 2 | 3,791,660 | 8.57% | 20.96% | 22,362 | VOP2 | `v_fmac_f32` |
| 3 | 2,471,052 | 5.58% | 26.54% | 24,734 | SOPP | `s_waitcnt` |
| 4 | 2,380,761 | 5.38% | 31.92% | 21,619 | VOP3 | `v_fma_f32` |
| 5 | 2,145,958 | 4.85% | 36.77% | 24,315 | VOP1 | `v_mov_b32` |
| 6 | 1,394,751 | 3.15% | 39.92% | 24,518 | SOP1 | `s_mov_b32` |
| 7 | 1,173,155 | 2.65% | 42.58% | 15,820 | VOP2 | `v_sub_f32` |
| 8 | 1,121,617 | 2.53% | 45.11% | 20,926 | VOP2 | `v_add_f32` |
| 9 | 1,090,326 | 2.46% | 47.57% | 22,008 | VOP3 | `v_cndmask_b32` |
| 10 | 927,782 | 2.10% | 49.67% | 24,377 | SMEM | `s_load_dwordx4` |
| 11 | 644,744 | 1.46% | 51.13% | 21,917 | SOPP | `s_nop` |
| 12 | 642,730 | 1.45% | 52.58% | 21,252 | VOP2 | `v_and_b32` |
| 13 | 627,566 | 1.42% | 54.00% | 22,742 | SMEM | `s_buffer_load_dword` |
| 14 | 621,093 | 1.40% | 55.40% | 20,770 | SOPP | `s_cbranch_execz` |
| 15 | 614,958 | 1.39% | 56.79% | 20,425 | SOP1 | `s_mov_b64` |
| 16 | 581,399 | 1.31% | 58.11% | 21,400 | SOP2 | `s_and_b32` |
| 17 | 538,523 | 1.22% | 59.32% | 21,395 | SOP2 | `s_add_i32` |
| 18 | 487,658 | 1.10% | 60.42% | 9,620 | VOP3P | `v_fma_mix_f32` |
| 19 | 468,616 | 1.06% | 61.48% | 16,162 | VOP3 | `v_mul_legacy_f32` |
| 20 | 447,621 | 1.01% | 62.50% | 21,816 | SMEM | `s_buffer_load_dwordx4` |
| 21 | 401,864 | 0.91% | 63.40% | 14,699 | VOP3 | `v_fmac_f32` |
| 22 | 398,072 | 0.90% | 64.30% | 20,268 | VOP2 | `v_cndmask_b32` |
| 23 | 386,280 | 0.87% | 65.18% | 17,076 | VOP1 | `v_rcp_f32` |
| 24 | 370,298 | 0.84% | 66.01% | 19,080 | VOP2 | `v_add_nc_u32` |
| 25 | 369,746 | 0.84% | 66.85% | 7,488 | VOP2 | `v_mul_f32` |
| 26 | 334,571 | 0.76% | 67.60% | 16,424 | SMEM | `s_load_dwordx8` |
| 27 | 294,834 | 0.67% | 68.27% | 18,188 | VOP1 | `v_cvt_f32_u32` |
| 28 | 290,343 | 0.66% | 68.93% | 17,850 | VOP2 | `v_lshrrev_b32` |
| 29 | 284,611 | 0.64% | 69.57% | 17,379 | MUBUF | `buffer_load_dword` |
| 30 | 281,027 | 0.64% | 70.20% | 21,273 | SOP2 | `s_lshl_b32` |
| 31 | 272,301 | 0.62% | 70.82% | 19,003 | SOP2 | `s_lshr_b32` |
| 32 | 271,301 | 0.61% | 71.43% | 20,329 | SOPC | `s_cmp_eq_u32` |
| 33 | 269,083 | 0.61% | 72.04% | 17,199 | VOP1 | `v_rsq_f32` |
| 34 | 262,114 | 0.59% | 72.63% | 11,540 | VOP2 | `v_max_f32` |
| 35 | 251,584 | 0.57% | 73.20% | 17,519 | VOP3 | `v_sub_f32` |
| 36 | 248,522 | 0.56% | 73.76% | 11,853 | VOP2 | `v_subrev_f32` |
| 37 | 239,034 | 0.54% | 74.30% | 5,091 | VOP2 | `v_mac_f32` |
| 38 | 237,883 | 0.54% | 74.84% | 17,669 | VOP1 | `v_cvt_f32_ubyte0` |
| 39 | 221,937 | 0.50% | 75.34% | 16,937 | VOP1 | `v_readfirstlane_b32` |
| 40 | 221,744 | 0.50% | 75.84% | 17,707 | SOP2 | `s_or_b32` |

Notes for a backend author:

- `v_mul_legacy_f32` (D3D9-style multiply: `0 × anything = 0`) is the single
  most executed opcode, ahead of `v_fmac_f32`. Treating it as a plain `fmul`
  changes results where one operand is 0 and the other is Inf/NaN.
- The console keeps GFX10.1's **non-fused** `v_mac_f32`, `v_mad_f32`,
  `v_madmk_f32`, `v_madak_f32` (absent from retail GFX10.3); they must be
  translated as separate multiply and add, not `fma` (counts below).
- `v_fma_mix_f32` (VOP3P, mixed f16/f32 FMA) is in the top 20.

## Per stage (entry 0)

| Stage | Programs | Instructions | Distinct opcodes | Cover 90% | Cover 99% | Cover 99.9% |
|---|---|---|---|---|---|---|
| CS | 2,647 | 15,571,480 | 437 | 76 | 177 | 260 |
| MS | 9,282 | 8,821,623 | 249 | 67 | 148 | 194 |
| PS | 10,944 | 18,847,938 | 358 | 78 | 168 | 238 |
| VS | 1,931 | 971,781 | 269 | 65 | 141 | 197 |
| GS | 195 | 28,245 | 43 | 29 | 42 | 43 |
| ES | 195 | 10,270 | 46 | 27 | 42 | 44 |

- **CS** uses 437 of the 470 opcodes; 81 appear in no other stage
  (listed below). A backend that handles CS covers almost the whole set.
- **MS** uses 249 opcodes, 247 of which CS also uses
  (MS-only: `exp`, `s_sendmsg`).
  20 MS opcodes never appear in PS.
- **ES/GS** (merged legacy geometry, 195 programs each) need only 65 opcodes between them.

### CS-only opcodes (in no other stage), by family, most used first

- **VOP3** (21): `v_mul_i32_i24`, `v_add_f64`, `v_mul_u32_u24`, `v_cvt_f32_u32`, `v_cvt_f16_f32`, `v_mad_i32_i24`, `v_sub_co_ci_u32`, `v_alignbyte_b32`, `v_cmp_eq_i32`, `v_sin_f32`, `v_cmpx_ngt_f32`, `v_cmp_gt_f16`, `v_cmpx_neq_f32`, `v_cmpx_eq_f32`, `v_cmp_nlt_f16`, `v_cvt_u32_f32`, `v_max_f16`, `v_add_nc_u16`, `v_or_b32`, `v_add_nc_u32`, `v_cos_f32`
- **DS** (14): `ds_wrxchg_rtn_b32`, `ds_add_u32`, `ds_add_rtn_u32`, `ds_max_u32`, `ds_min_u32`, `ds_read_u16`, `ds_bpermute_b32`, `ds_or_b32`, `ds_write_b16`, `ds_read_u16_d16_hi`, `ds_write_b16_d16_hi`, `ds_cmpst_rtn_b32`, `ds_max_i32`, `ds_and_b32`
- **MUBUF** (10): `buffer_store_dwordx3`, `buffer_load_format_d16_x`, `buffer_store_dwordx4`, `buffer_store_dwordx2`, `buffer_store_format_d16_xyzw`, `buffer_atomic_umin`, `buffer_load_format_d16_xyzw`, `buffer_atomic_cmpswap`, `buffer_atomic_and`, `buffer_atomic_swap`
- **VOP1** (8): `v_swap_b32`, `v_cvt_f32_f64`, `v_sqrt_f16`, `v_exp_f16`, `v_ffbh_i32`, `v_trunc_f16`, `v_cos_f16`, `v_sin_f16`
- **VOPC** (6): `v_cmpx_eq_i32`, `v_cmpx_ne_u64`, `v_cmpx_lt_u64`, `v_cmp_le_f16`, `v_cmpx_ne_u16`, `v_cmp_eq_u16`
- **MIMG** (6): `image_bvh_intersect_ray`, `image_gather4_l_o`, `image_bvh64_intersect_ray`, `image_gather4_l`, `image_gather4_lz_o`, `image_atomic_cmpswap`
- **VOP2** (4): `v_mul_i32_i24`, `v_max_f16`, `v_min_f16`, `v_xnor_b32`
- **SOP2** (4): `s_lshr_b64`, `s_sub_u32`, `s_subb_u32`, `s_orn2_b32`
- **VOP3P** (4): `v_pk_max_f16`, `v_pk_min_f16`, `v_pk_lshlrev_b16`, `v_pk_lshrrev_b16`
- **SOPK** (2): `s_cmpk_lt_i32`, `s_cmpk_gt_i32`
- **SOP1** (1): `s_abs_i32`
- **GLOBAL** (1): `global_load_ushort`

### Full MS opcode set, by family, most used first

- **VOP3** (63): `v_fma_f32`, `v_cndmask_b32`, `v_fmac_f32`, `v_mad_u32_u24`, `v_bfe_u32`, `v_mad_f32`, `v_sub_f32`, `v_mul_legacy_f32`, `v_lshl_add_u32`, `v_pack_b32_f16`, `v_max_f32`, `v_nop`, `v_cmp_le_u32`, `v_mul_lo_u32`, `v_cmp_eq_u32`, `v_cmp_lt_f32`, `v_cmp_le_f32`, `v_perm_b32`, `v_or3_b32`, `v_mbcnt_lo_u32_b32`, `v_mbcnt_hi_u32_b32`, `v_cmp_gt_f32`, `v_lshlrev_b32`, `v_add_f32`, `v_mac_f32`, `v_lshrrev_b32`, `v_mul_f32`, `v_mul_hi_u32`, `v_max3_f32`, `v_add3_u32`, `v_add_lshl_u32`, `v_subrev_nc_u32`, `v_bcnt_u32_b32`, `v_and_or_b32`, `v_add_co_ci_u32`, `v_add_co_u32`, `v_cmp_eq_f32`, `v_cmp_eq_u64`, `v_cmp_lg_f32`, `v_cmp_ge_u32`, `v_cmp_ge_f32`, `v_fma_legacy_f32`, `v_med3_f32`, `v_cmp_ne_u32`, `v_fract_f32`, `v_log_f32`, `v_rcp_f32`, `v_lshl_or_b32`, `v_cmp_nle_f32`, `v_cmp_class_f32`, `v_cmp_neq_f32`, `v_bfi_b32`, `v_xad_u32`, `v_readlane_b32`, `v_cmpx_gt_f32`, `v_subrev_f32`, `v_cmp_gt_u32`, `v_writelane_b32`, `v_min3_f32`, `v_cmp_gt_i32`, `v_subrev_co_ci_u32`, `v_permlanex16_b32`, `v_cmp_lt_f16`
- **VOPC** (34): `v_cmp_le_f32`, `v_cmp_eq_u32`, `v_cmp_gt_u32`, `v_cmpx_gt_u32`, `v_cmp_le_u32`, `v_cmp_gt_f32`, `v_cmpx_eq_u32`, `v_cmpx_lg_f32`, `v_cmp_lt_f32`, `v_cmpx_gt_i32`, `v_cmp_ne_u32`, `v_cmpx_ne_u32`, `v_cmpx_lt_f32`, `v_cmpx_lt_u32`, `v_cmp_eq_u64`, `v_cmpx_gt_f32`, `v_cmp_ge_u32`, `v_cmpx_lt_i32`, `v_cmp_lg_f32`, `v_cmp_gt_i32`, `v_cmpx_eq_f32`, `v_cmp_nle_f32`, `v_cmp_ge_f32`, `v_cmp_eq_f32`, `v_cmpx_le_f32`, `v_cmpx_neq_f32`, `v_cmpx_ge_f32`, `v_cmp_neq_f32`, `v_cmpx_ngt_f32`, `v_cmp_nge_f32`, `v_cmp_lt_f16`, `v_cmpx_ge_u32`, `v_cmp_ngt_f32`, `v_cmp_lt_u32`
- **VOP1** (31): `v_mov_b32`, `v_readfirstlane_b32`, `v_cvt_f32_u32`, `v_rsq_f32`, `v_cvt_f16_f32`, `v_cvt_u32_f32`, `v_cvt_f32_ubyte0`, `v_nop`, `v_rcp_f32`, `v_sqrt_f32`, `v_trunc_f32`, `v_cvt_f32_i32`, `v_bfrev_b32`, `v_cvt_f32_f16`, `v_fract_f32`, `v_cvt_f32_ubyte1`, `v_sin_f32`, `v_cos_f32`, `v_rcp_iflag_f32`, `v_cvt_f32_ubyte2`, `v_exp_f32`, `v_cvt_i32_f32`, `v_cvt_f32_ubyte3`, `v_floor_f32`, `v_log_f32`, `v_rndne_f32`, `v_ceil_f32`, `v_ffbh_u32`, `v_not_b32`, `v_ffbl_b32`, `v_movrels_b32`
- **VOP2** (25): `v_fmac_f32`, `v_mul_legacy_f32`, `v_add_f32`, `v_and_b32`, `v_mul_f32`, `v_mac_f32`, `v_add_nc_u32`, `v_sub_f32`, `v_lshrrev_b32`, `v_cndmask_b32`, `v_subrev_f32`, `v_lshlrev_b32`, `v_max_f32`, `v_xor_b32`, `v_sub_nc_u32`, `v_subrev_nc_u32`, `v_min_f32`, `v_or_b32`, `v_ashrrev_i32`, `v_fmaak_f32`, `v_min_u32`, `v_madak_f32`, `v_subrev_co_ci_u32`, `v_add_co_ci_u32`, `v_max_u32`
- **SOP2** (21): `s_and_b32`, `s_lshr_b32`, `s_add_i32`, `s_lshl_b32`, `s_cselect_b64`, `s_and_b64`, `s_or_b32`, `s_or_b64`, `s_mul_i32`, `s_bfe_u32`, `s_andn2_b64`, `s_xor_b64`, `s_cselect_b32`, `s_sub_i32`, `s_mul_hi_u32`, `s_bfe_i32`, `s_add_u32`, `s_addc_u32`, `s_min_u32`, `s_xor_b32`, `s_ashr_i32`
- **SOPP** (13): `s_waitcnt`, `s_cbranch_execz`, `s_cbranch_scc1`, `s_branch`, `s_nop`, `s_cbranch_vccnz`, `s_barrier`, `s_cbranch_scc0`, `s_inst_prefetch`, `s_endpgm`, `s_sendmsg`, `s_cbranch_execnz`, `s_cbranch_vccz`
- **SOP1** (11): `s_mov_b32`, `s_mov_b64`, `s_and_saveexec_b64`, `s_brev_b32`, `s_getpc_b64`, `s_not_b32`, `s_bitset1_b32`, `s_andn2_wrexec_b64`, `s_bcnt1_i32_b32`, `s_bcnt1_i32_b64`, `s_or_saveexec_b64`
- **SOPC** (9): `s_cmp_eq_u32`, `s_cmp_lg_u32`, `s_cmp_lt_u32`, `s_cmp_gt_u32`, `s_cmp_le_u32`, `s_cmp_ge_u32`, `s_cmp_lt_i32`, `s_cmp_gt_i32`, `s_cmp_ge_i32`
- **SMEM** (8): `s_load_dwordx4`, `s_buffer_load_dwordx4`, `s_buffer_load_dword`, `s_buffer_load_dwordx2`, `s_load_dwordx8`, `s_buffer_load_dwordx16`, `s_buffer_load_dwordx8`, `s_load_dword`
- **MUBUF** (8): `buffer_load_dwordx4`, `buffer_load_dword`, `buffer_load_dwordx2`, `buffer_load_format_x`, `buffer_load_dwordx3`, `buffer_load_format_xyz`, `buffer_load_format_xyzw`, `buffer_load_format_xy`
- **DS** (7): `ds_swizzle_b32`, `ds_read2_b32`, `ds_write2_b32`, `ds_read_b32`, `ds_read2st64_b32`, `ds_write2st64_b32`, `ds_write_b32`
- **SOPK** (6): `s_setreg_imm32_b32`, `s_movk_i32`, `s_addk_i32`, `s_mulk_i32`, `s_cmpk_gt_u32`, `s_cmpk_eq_i32`
- **MTBUF** (4): `tbuffer_load_format_xyzw`, `tbuffer_load_format_xy`, `tbuffer_load_format_xyz`, `tbuffer_load_format_x`
- **MIMG** (4): `image_sample_lz`, `image_load`, `image_gather4_lz`, `image_sample_l`
- **VOP3P** (3): `v_fma_mix_f32`, `v_fma_mixlo_f16`, `v_fma_mixhi_f16`
- **EXP** (1): `exp`
- **GLOBAL** (1): `global_load_dword`

### MS top 25

| # | Occurrences | Share | Cum. | Programs | Family | Opcode |
|---|---|---|---|---|---|---|
| 1 | 1,154,843 | 13.09% | 13.09% | 9,000 | VOP2 | `v_fmac_f32` |
| 2 | 974,199 | 11.04% | 24.13% | 9,050 | VOP2 | `v_mul_legacy_f32` |
| 3 | 491,634 | 5.57% | 29.71% | 9,282 | SOPP | `s_waitcnt` |
| 4 | 335,944 | 3.81% | 33.52% | 8,977 | VOP3 | `v_fma_f32` |
| 5 | 310,379 | 3.52% | 37.03% | 9,280 | VOP1 | `v_mov_b32` |
| 6 | 238,253 | 2.70% | 39.73% | 9,282 | SOP2 | `s_and_b32` |
| 7 | 233,203 | 2.64% | 42.38% | 9,282 | VOP2 | `v_add_f32` |
| 8 | 225,327 | 2.55% | 44.93% | 9,282 | VOP3 | `v_cndmask_b32` |
| 9 | 172,305 | 1.95% | 46.89% | 9,282 | VOP2 | `v_and_b32` |
| 10 | 171,587 | 1.95% | 48.83% | 9,282 | SOP1 | `s_mov_b32` |
| 11 | 159,735 | 1.81% | 50.64% | 9,282 | SOP1 | `s_mov_b64` |
| 12 | 155,529 | 1.76% | 52.40% | 9,282 | SOP2 | `s_lshr_b32` |
| 13 | 151,079 | 1.71% | 54.12% | 2,053 | VOP2 | `v_mul_f32` |
| 14 | 144,393 | 1.64% | 55.75% | 883 | VOP2 | `v_mac_f32` |
| 15 | 138,331 | 1.57% | 57.32% | 9,282 | SMEM | `s_load_dwordx4` |
| 16 | 132,156 | 1.50% | 58.82% | 9,282 | SOP2 | `s_add_i32` |
| 17 | 121,156 | 1.37% | 60.19% | 7,729 | MUBUF | `buffer_load_dwordx4` |
| 18 | 119,341 | 1.35% | 61.55% | 9,192 | SMEM | `s_buffer_load_dwordx4` |
| 19 | 114,898 | 1.30% | 62.85% | 9,282 | VOP2 | `v_add_nc_u32` |
| 20 | 114,085 | 1.29% | 64.14% | 4,199 | VOP2 | `v_sub_f32` |
| 21 | 107,213 | 1.22% | 65.36% | 9,282 | SMEM | `s_buffer_load_dword` |
| 22 | 101,918 | 1.16% | 66.51% | 9,282 | SOPP | `s_cbranch_execz` |
| 23 | 89,042 | 1.01% | 67.52% | 9,282 | EXP | `exp` |
| 24 | 88,905 | 1.01% | 68.53% | 9,282 | SOP2 | `s_lshl_b32` |
| 25 | 79,764 | 0.90% | 69.43% | 9,282 | SOPC | `s_cmp_eq_u32` |

### CS top 25

| # | Occurrences | Share | Cum. | Programs | Family | Opcode |
|---|---|---|---|---|---|---|
| 1 | 1,944,450 | 12.49% | 12.49% | 2,310 | VOP2 | `v_mul_legacy_f32` |
| 2 | 994,166 | 6.38% | 18.87% | 2,125 | VOP2 | `v_fmac_f32` |
| 3 | 924,731 | 5.94% | 24.81% | 2,642 | SOPP | `s_waitcnt` |
| 4 | 855,076 | 5.49% | 30.30% | 2,127 | VOP3 | `v_fma_f32` |
| 5 | 832,114 | 5.34% | 35.65% | 2,590 | VOP1 | `v_mov_b32` |
| 6 | 678,274 | 4.36% | 40.00% | 2,642 | SOP1 | `s_mov_b32` |
| 7 | 408,351 | 2.62% | 42.62% | 2,582 | SMEM | `s_load_dwordx4` |
| 8 | 403,394 | 2.59% | 45.21% | 2,091 | VOP3 | `v_cndmask_b32` |
| 9 | 396,339 | 2.55% | 47.76% | 2,089 | VOP2 | `v_add_f32` |
| 10 | 363,151 | 2.33% | 50.09% | 1,955 | VOP2 | `v_sub_f32` |
| 11 | 328,672 | 2.11% | 52.20% | 2,083 | SOPP | `s_nop` |
| 12 | 256,027 | 1.64% | 53.85% | 2,421 | SOPP | `s_cbranch_execz` |
| 13 | 244,876 | 1.57% | 55.42% | 2,069 | VOP2 | `v_and_b32` |
| 14 | 208,570 | 1.34% | 56.76% | 1,958 | SOP2 | `s_and_b32` |
| 15 | 198,307 | 1.27% | 58.03% | 2,231 | SMEM | `s_buffer_load_dword` |
| 16 | 182,451 | 1.17% | 59.20% | 1,416 | VOP3P | `v_fma_mix_f32` |
| 17 | 173,680 | 1.12% | 60.32% | 2,030 | VOP1 | `v_rcp_f32` |
| 18 | 169,088 | 1.09% | 61.41% | 1,911 | VOP3 | `v_mul_legacy_f32` |
| 19 | 151,797 | 0.97% | 62.38% | 2,225 | SOP2 | `s_add_i32` |
| 20 | 150,553 | 0.97% | 63.35% | 1,991 | VOP2 | `v_cndmask_b32` |
| 21 | 138,311 | 0.89% | 64.24% | 2,225 | VOP2 | `v_add_nc_u32` |
| 22 | 137,956 | 0.89% | 65.12% | 2,163 | VOP1 | `v_cvt_f32_u32` |
| 23 | 137,600 | 0.88% | 66.00% | 2,126 | SMEM | `s_buffer_load_dwordx4` |
| 24 | 125,185 | 0.80% | 66.81% | 1,722 | VOP3 | `v_fmac_f32` |
| 25 | 121,386 | 0.78% | 67.59% | 1,452 | SOP1 | `s_and_saveexec_b32` |

## Naming and unknowns

Opcode names come from AMD's machine-readable RDNA 2 ISA specification
(per-encoding opcode number → mnemonic), plus four GFX10.1 ops the console
GPU still executes in opcode slots GFX10.3 leaves unassigned:

| Opcode | Encoding | Use in entry 0 |
|---|---|---|
| `v_mac_f32` | VOP2 0x1F / VOP3 0x11F | 239,034 in 5,091 programs / 10,811 in 1,249 programs |
| `v_madmk_f32` | VOP2 0x20 | not used |
| `v_madak_f32` | VOP2 0x21 | 8,442 in 3,679 programs |
| `v_mad_f32` | VOP3 0x141 | 139,282 in 7,293 programs |

**Synthetic names:** none needed. Every one of the 470 opcodes in
entry 0 (and the 182 in entry 2) has a name. The decoder would label an
unassigned opcode `VOP3_op0x1a7`-style rather than drop it.

**One real ambiguity — VOP3 0x140** (24,717 in 6,058 programs): GFX10.3 names
it `v_fma_legacy_f32` (fused), GFX10.1 `v_mad_legacy_f32` (not fused). Since the
console GPU demonstrably keeps the other GFX10.1 non-fused multiply-adds, the
non-fused GFX10.1 meaning is the likelier one; forge9 prints the GFX10.3 name.
A backend should treat it as `mul_legacy` + `add` unless hardware testing shows
otherwise. (The matching VOP2 0x06 `v_fmac_legacy_f32`/`v_mac_legacy_f32` does
not occur.)

**Cross-check** against an independent hand-written GFX10 table (G6Recomp's
`rdna2_names.cpp`, from LLVM/Mesa ACO): of the 470 opcodes, 457 have
identical names, 12 are MIMG sample/gather ops (`image_sample*`,
`image_gather4*`) that table does not list, and 1 differs: VOP3 0x140 above.
forge9's decoder itself was cross-checked against xcl's at all 44,251,337
instruction offsets for length, family and wave-op class
([wave64_instruction_usage.md](wave64_instruction_usage.md)).

## Appendix: complete opcode set (entry 0), by family, most used first

Occurrence counts in parentheses. Implementing every opcode here covers every
shipped program.

- **VOP2** (38 opcodes, 15,056,499 instructions): `v_mul_legacy_f32` (5,482,918), `v_fmac_f32` (3,791,660), `v_sub_f32` (1,173,155), `v_add_f32` (1,121,617), `v_and_b32` (642,730), `v_cndmask_b32` (398,072), `v_add_nc_u32` (370,298), `v_mul_f32` (369,746), `v_lshrrev_b32` (290,343), `v_max_f32` (262,114), `v_subrev_f32` (248,522), `v_mac_f32` (239,034), `v_lshlrev_b32` (133,877), `v_xor_b32` (108,224), `v_or_b32` (92,483), `v_min_f32` (88,392), `v_cvt_pkrtz_f16_f32` (51,595), `v_fmaak_f32` (44,812), `v_sub_nc_u32` (37,042), `v_subrev_nc_u32` (26,212), `v_min_u32` (21,598), `v_mul_u32_u24` (15,557), `v_ashrrev_i32` (12,166), `v_madak_f32` (8,442), `v_add_co_ci_u32` (7,047), `v_max_u32` (6,479), `v_max_i32` (5,377), `v_min_i32` (3,692), `v_mul_i32_i24` (948), `v_add_f16` (799), `v_mul_f16` (655), `v_fmac_f16` (654), `v_sub_f16` (154), `v_subrev_co_ci_u32` (54), `v_max_f16` (12), `v_min_f16` (9), `v_xnor_b32` (7), `v_sub_co_ci_u32` (3)
- **VOP3** (131 opcodes, 7,251,572 instructions): `v_fma_f32` (2,380,761), `v_cndmask_b32` (1,090,326), `v_mul_legacy_f32` (468,616), `v_fmac_f32` (401,864), `v_sub_f32` (251,584), `v_mad_u32_u24` (214,716), `v_add_f32` (187,051), `v_mul_lo_u32` (166,991), `v_bfe_u32` (146,908), `v_mad_f32` (139,282), `v_cmp_lt_f32` (133,887), `v_max_f32` (131,717), `v_cmp_eq_u32` (113,647), `v_cmp_gt_f32` (104,987), `v_readlane_b32` (77,701), `v_lshl_add_u32` (65,154), `v_interp_p1ll_f16` (59,367), `v_interp_p2_f16` (59,367), `v_med3_f32` (56,023), `v_nop` (50,583), `v_cmp_le_f32` (50,205), `v_max3_f32` (47,244), `v_cmp_eq_f32` (42,498), `v_cmp_ge_f32` (37,970), `v_cmp_ne_u32` (37,323), `v_cmp_class_f32` (37,026), `v_cmp_lg_f32` (36,973), `v_rcp_f32` (36,247), `v_or3_b32` (35,576), `v_pack_b32_f16` (35,328), `v_cmp_le_u32` (29,997), `v_writelane_b32` (29,990), `v_add3_u32` (27,241), `v_fma_legacy_f32` (24,717), `v_med3_i32` (21,545), `v_cubesc_f32` (20,853), `v_cubetc_f32` (20,853), `v_cubema_f32` (20,853), `v_cubeid_f32` (20,744), `v_lshl_or_b32` (20,118), `v_mul_hi_u32` (18,629), `v_min_f32` (18,216), `v_lshlrev_b32` (17,956), `v_log_f32` (16,578), `v_and_or_b32` (16,319), `v_lshrrev_b32` (15,785), `v_sqrt_f32` (14,754), `v_mul_f32` (13,414), `v_subrev_f32` (11,999), `v_perm_b32` (11,967), `v_mbcnt_lo_u32_b32` (11,410), `v_bcnt_u32_b32` (10,834), `v_mac_f32` (10,811), `v_mbcnt_hi_u32_b32` (10,713), `v_bfi_b32` (10,676), `v_add_co_u32` (10,520), `v_permlanex16_b32` (9,021), `v_exp_f32` (8,488), `v_min3_f32` (8,001), `v_lshlrev_b64` (6,888), `v_add_co_ci_u32` (5,228), `v_cmp_le_i32` (5,123), `v_subrev_nc_u32` (3,995), `v_add_lshl_u32` (3,894), `v_cmp_neq_f32` (3,795), `v_cmp_gt_i32` (2,978), `v_xad_u32` (2,841), `v_rcp_iflag_f32` (2,633), `v_bfe_i32` (2,612), `v_cvt_pk_u8_f32` (2,606), `v_cmp_eq_u64` (2,448), `v_cmp_ge_u32` (2,264), `v_cmpx_lt_f32` (2,218), `v_med3_u32` (2,002), `v_min3_u32` (1,890), `v_max3_u32` (1,870), `v_cmp_lt_u32` (1,802), `v_cmp_lt_i32` (1,685), `v_fract_f32` (1,237), `v_cmp_nle_f32` (1,177), `v_cmp_gt_u32` (1,151), `v_alignbit_b32` (1,104), `v_cmp_nlt_f32` (1,053), `v_cmpx_gt_f32` (1,011), `v_cmp_ngt_f32` (738), `v_bfm_b32` (690), `v_cvt_f32_ubyte0` (657), `v_fma_f16` (578), `v_cmpx_le_f32` (470), `v_lshrrev_b16` (363), `v_cvt_f32_i32` (335), `v_mul_i32_i24` (319), `v_cmp_nge_f32` (262), `v_cvt_f32_f16` (252), `v_cmp_ne_u64` (220), `v_add_f64` (216), `v_rsq_f32` (194), `v_cmp_lt_f16` (139), `v_cmpx_ge_f32` (112), `v_mul_u32_u24` (77), `v_cvt_f32_u32` (74), `v_and_b32` (72), `v_subrev_co_ci_u32` (57), `v_add_f16` (48), `v_mul_hi_i32` (46), `v_cmp_ge_i32` (34), `v_floor_f32` (26), `v_cmp_neq_f16` (21), `v_med3_f16` (21), `v_cvt_f16_f32` (18), `v_mul_f16` (14), `v_mad_i32_i24` (14), `v_sub_co_ci_u32` (12), `v_cmp_eq_i32` (9), `v_alignbyte_b32` (9), `v_trunc_f32` (8), `v_sub_f16` (6), `v_sin_f32` (6), `v_cmpx_ngt_f32` (5), `v_cmp_gt_f16` (4), `v_cmpx_neq_f32` (3), `v_cmpx_eq_f32` (2), `v_cmp_nlt_f16` (2), `v_xor_b32` (2), `v_cvt_i32_f32` (2), `v_or_b32` (1), `v_add_nc_u32` (1), `v_max_f16` (1), `v_cvt_u32_f32` (1), `v_cos_f32` (1), `v_add_nc_u16` (1)
- **VOP1** (42 opcodes, 5,170,445 instructions): `v_mov_b32` (2,145,958), `v_rcp_f32` (386,280), `v_cvt_f32_u32` (294,834), `v_rsq_f32` (269,083), `v_cvt_f32_ubyte0` (237,883), `v_readfirstlane_b32` (221,937), `v_sqrt_f32` (186,613), `v_cvt_u32_f32` (183,163), `v_exp_f32` (139,903), `v_cvt_f32_ubyte1` (131,785), `v_nop` (127,169), `v_cvt_f32_ubyte2` (120,234), `v_cvt_f32_f16` (94,963), `v_fract_f32` (83,552), `v_cvt_f32_i32` (78,262), `v_log_f32` (73,017), `v_cvt_f32_ubyte3` (68,881), `v_cvt_i32_f32` (66,417), `v_trunc_f32` (49,645), `v_cvt_f16_f32` (44,470), `v_floor_f32` (40,969), `v_sin_f32` (32,556), `v_rcp_iflag_f32` (32,195), `v_cos_f32` (24,726), `v_rndne_f32` (14,915), `v_bfrev_b32` (11,745), `v_ceil_f32` (4,036), `v_ffbl_b32` (3,805), `v_not_b32` (385), `v_movrels_b32` (318), `v_ffbh_u32` (231), `v_rsq_f16` (215), `v_rcp_f16` (104), `v_movreld_b32` (80), `v_swap_b32` (44), `v_cvt_f32_f64` (36), `v_sqrt_f16` (13), `v_exp_f16` (12), `v_ffbh_i32` (4), `v_trunc_f16` (3), `v_sin_f16` (2), `v_cos_f16` (2)
- **SOPP** (15 opcodes, 4,549,159 instructions): `s_waitcnt` (2,471,052), `s_nop` (644,744), `s_cbranch_execz` (621,093), `s_cbranch_scc1` (180,922), `s_branch` (179,910), `s_cbranch_vccnz` (162,198), `s_cbranch_scc0` (92,859), `s_cbranch_execnz` (75,940), `s_cbranch_vccz` (33,467), `s_endpgm` (31,937), `s_inst_prefetch` (25,194), `s_barrier` (19,371), `s_sendmsg` (10,267), `s_setprio` (195), `s_sleep` (10)
- **SOP2** (32 opcodes, 3,125,660 instructions): `s_and_b32` (581,399), `s_add_i32` (538,523), `s_lshl_b32` (281,027), `s_lshr_b32` (272,301), `s_or_b32` (221,744), `s_and_b64` (197,603), `s_cselect_b32` (153,425), `s_andn2_b64` (132,835), `s_cselect_b64` (123,068), `s_or_b64` (114,563), `s_andn2_b32` (104,692), `s_mul_i32` (100,113), `s_bfe_u32` (90,351), `s_xor_b32` (59,370), `s_add_u32` (54,974), `s_xor_b64` (49,551), `s_sub_i32` (14,240), `s_mul_hi_u32` (11,567), `s_addc_u32` (8,130), `s_bfe_i32` (6,235), `s_lshl_b64` (2,392), `s_bfm_b32` (1,861), `s_min_u32` (1,793), `s_ashr_i32` (1,458), `s_max_i32` (1,316), `s_bfm_b64` (390), `s_lshr_b64` (332), `s_sub_u32` (137), `s_subb_u32` (137), `s_max_u32` (83), `s_min_i32` (49), `s_orn2_b32` (1)
- **SMEM** (9 opcodes, 2,661,354 instructions): `s_load_dwordx4` (927,782), `s_buffer_load_dword` (627,566), `s_buffer_load_dwordx4` (447,621), `s_load_dwordx8` (334,571), `s_buffer_load_dwordx2` (190,288), `s_buffer_load_dwordx8` (76,149), `s_buffer_load_dwordx16` (54,804), `s_load_dword` (2,474), `s_load_dwordx2` (99)
- **SOP1** (24 opcodes, 2,464,924 instructions): `s_mov_b32` (1,394,751), `s_mov_b64` (614,958), `s_and_saveexec_b32` (152,228), `s_and_saveexec_b64` (141,848), `s_andn2_wrexec_b32` (51,487), `s_brev_b32` (23,339), `s_andn2_wrexec_b64` (21,513), `s_wqm_b64` (18,554), `s_ff1_i32_b32` (9,830), `s_or_saveexec_b32` (9,269), `s_bcnt1_i32_b32` (6,297), `s_getpc_b64` (5,556), `s_wqm_b32` (4,098), `s_not_b32` (3,399), `s_or_saveexec_b64` (2,247), `s_setpc_b64` (1,626), `s_bcnt1_i32_b64` (1,044), `s_ff1_i32_b64` (973), `s_bitset1_b32` (885), `s_swappc_b64` (472), `s_bitset0_b32` (375), `s_flbit_i32_b32` (100), `s_not_b64` (49), `s_abs_i32` (26)
- **VOPC** (58 opcodes, 1,125,453 instructions): `v_cmp_eq_u32` (162,438), `v_cmpx_eq_u32` (151,403), `v_cmp_le_f32` (113,275), `v_cmp_gt_f32` (104,381), `v_cmp_lt_f32` (96,881), `v_cmpx_lt_f32` (72,750), `v_cmpx_ne_u32` (56,842), `v_cmp_ne_u32` (51,866), `v_cmpx_gt_f32` (48,250), `v_cmpx_lg_f32` (36,071), `v_cmp_le_u32` (33,251), `v_cmp_gt_u32` (28,730), `v_cmpx_gt_u32` (28,437), `v_cmpx_gt_i32` (16,807), `v_cmp_eq_f32` (16,723), `v_cmp_lg_f32` (16,480), `v_cmp_ge_f32` (16,069), `v_cmpx_lt_i32` (14,320), `v_cmpx_lt_u32` (7,399), `v_cmpx_eq_f32` (6,779), `v_cmpx_le_f32` (6,140), `v_cmpx_ge_f32` (6,055), `v_cmp_gt_i32` (6,015), `v_cmp_nlt_f32` (4,158), `v_cmp_ge_u32` (3,488), `v_cmp_lt_u32` (3,323), `v_cmp_eq_u64` (2,425), `v_cmp_le_i32` (2,366), `v_cmp_neq_f32` (2,149), `v_cmpx_neq_f32` (1,940), `v_cmp_nle_f32` (1,676), `v_cmp_lt_i32` (1,440), `v_cmpx_eq_i32` (681), `v_cmpx_nge_f32` (673), `v_cmpx_nle_f32` (607), `v_cmpx_ngt_f32` (416), `v_cmp_lt_f16` (352), `v_cmp_ngt_f32` (328), `v_cmpx_ge_i32` (290), `v_cmp_gt_f16` (280), `v_cmpx_le_i32` (257), `v_cmpx_lt_f16` (241), `v_cmp_ge_i32` (184), `v_cmpx_le_u32` (154), `v_cmp_nge_f32` (150), `v_cmpx_ge_u32` (143), `v_cmp_neq_f16` (113), `v_cmpx_gt_f16` (72), `v_cmpx_ge_f16` (60), `v_cmp_eq_f16` (59), `v_cmpx_nlt_f32` (28), `v_cmpx_eq_f16` (21), `v_cmpx_ne_u64` (10), `v_cmp_le_f16` (2), `v_cmpx_lt_u64` (2), `v_cmp_eq_u16` (1), `v_cmpx_ne_u16` (1), `v_cmpx_eq_u64` (1)
- **MUBUF** (25 opcodes, 664,769 instructions): `buffer_load_dword` (284,611), `buffer_load_dwordx4` (167,257), `buffer_load_format_x` (69,507), `buffer_load_dwordx2` (55,479), `buffer_load_dwordx3` (52,649), `buffer_load_format_xyz` (10,213), `buffer_store_dword` (7,199), `buffer_store_format_xyzw` (4,967), `buffer_atomic_add` (3,229), `buffer_load_format_xy` (2,689), `buffer_load_format_xyzw` (1,735), `buffer_atomic_or` (1,283), `buffer_store_dwordx3` (879), `buffer_load_format_d16_x` (616), `buffer_store_dwordx4` (605), `buffer_store_dwordx2` (465), `buffer_store_format_d16_xyzw` (441), `buffer_atomic_umin` (404), `buffer_atomic_umax` (344), `buffer_load_format_d16_xyzw` (99), `buffer_atomic_cmpswap` (46), `buffer_atomic_smax` (28), `buffer_atomic_smin` (22), `buffer_atomic_swap` (1), `buffer_atomic_and` (1)
- **VOP3P** (10 opcodes, 524,989 instructions): `v_fma_mix_f32` (487,658), `v_fma_mixlo_f16` (33,896), `v_fma_mixhi_f16` (2,133), `v_pk_fma_f16` (480), `v_pk_mul_f16` (461), `v_pk_add_f16` (291), `v_pk_max_f16` (36), `v_pk_min_f16` (32), `v_pk_lshlrev_b16` (1), `v_pk_lshrrev_b16` (1)
- **SOPC** (12 opcodes, 474,599 instructions): `s_cmp_eq_u32` (271,301), `s_cmp_lg_u32` (119,510), `s_cmp_lt_u32` (29,784), `s_cmp_gt_u32` (23,110), `s_cmp_ge_u32` (8,349), `s_cmp_lt_i32` (8,080), `s_cmp_gt_i32` (6,971), `s_cmp_le_u32` (6,062), `s_cmp_eq_u64` (1,196), `s_cmp_ge_i32` (198), `s_cmp_le_i32` (32), `s_cmp_lg_u64` (6)
- **MIMG** (24 opcodes, 436,490 instructions): `image_sample_lz` (182,292), `image_sample` (91,252), `image_load` (58,506), `image_sample_l` (43,069), `image_sample_d` (18,664), `image_sample_c_lz` (15,155), `image_gather4_lz` (11,015), `image_store` (7,501), `image_atomic_umax` (2,485), `image_sample_b` (1,202), `image_get_resinfo` (1,148), `image_load_mip` (726), `image_atomic_or` (725), `image_get_lod` (718), `image_atomic_umin` (713), `image_bvh_intersect_ray` (638), `image_sample_d_cl` (273), `image_gather4_l_o` (174), `image_bvh64_intersect_ray` (78), `image_gather4_l` (58), `image_gather4_lz_o` (55), `image_sample_o` (31), `image_atomic_add` (11), `image_atomic_cmpswap` (1)
- **VINTRP** (3 opcodes, 222,740 instructions): `v_interp_p1_f32` (107,907), `v_interp_p2_f32` (107,907), `v_interp_mov_f32` (6,926)
- **SOPK** (14 opcodes, 194,567 instructions): `s_movk_i32` (126,000), `s_setreg_imm32_b32` (34,879), `s_addk_i32` (19,223), `s_mulk_i32` (6,100), `s_cmpk_eq_i32` (4,379), `s_cmpk_eq_u32` (1,616), `s_cmpk_lg_i32` (963), `s_cmpk_lt_i32` (768), `s_cmpk_gt_i32` (245), `s_cmpk_gt_u32` (210), `s_waitcnt_vscnt` (133), `s_cmpk_lt_u32` (36), `s_setreg_b32` (10), `s_cmpk_lg_u32` (5)
- **EXP** (1 opcodes, 135,659 instructions): `exp` (135,659)
- **DS** (23 opcodes, 128,989 instructions): `ds_swizzle_b32` (105,226), `ds_write2_b32` (9,607), `ds_read2_b32` (7,232), `ds_read_b32` (2,516), `ds_write_b32` (2,238), `ds_wrxchg_rtn_b32` (397), `ds_read2st64_b32` (282), `ds_write2st64_b32` (259), `ds_write2_b64` (195), `ds_write_b64` (190), `ds_add_u32` (185), `ds_add_rtn_u32` (176), `ds_max_u32` (147), `ds_min_u32` (78), `ds_read_u16` (75), `ds_bpermute_b32` (74), `ds_or_b32` (71), `ds_write_b16` (27), `ds_write_b16_d16_hi` (4), `ds_read_u16_d16_hi` (4), `ds_max_i32` (2), `ds_and_b32` (2), `ds_cmpst_rtn_b32` (2)
- **MTBUF** (4 opcodes, 52,927 instructions): `tbuffer_load_format_xy` (27,095), `tbuffer_load_format_xyzw` (22,845), `tbuffer_load_format_xyz` (2,719), `tbuffer_load_format_x` (268)
- **GLOBAL** (5 opcodes, 10,542 instructions): `global_load_dword` (9,536), `global_load_dwordx2` (633), `global_load_dwordx3` (193), `global_load_dwordx4` (174), `global_load_ushort` (6)
