# EXEC Uniformity: can the compile bombs fold their lane guards?

A direct RDNA2→DXIL backend guards every vector instruction with its lane's
EXEC bit. That guard is what makes the emitted DXIL huge. A uniformity analysis
proves where EXEC covers every lane, so the guard can be dropped there. This
document measures, over the whole shader database, how much of that headroom is
*provable*. It asks the question that matters most: are the heaviest programs
(see [shader_compile_cost.md](shader_compile_cost.md)) foldable, or divergent?

```
forge9 isa shaders.rpf 0 --uniformity        # whole database + the 30 heaviest programs
forge9 isa shaders.rpf 0 --uniformity 60     # the 60 heaviest
```

## Answer

**The compile bombs are not foldable.** Every program over 20,000 instructions
except one is 0.0–0.5% provably uniform. Across all 57 of them, 1.7% of 957,277
vector instructions are uniform, and most of that comes from `ActivateRays`. The
cause is not fine-grained divergence. Each of these shaders opens a per-lane
condition in its first few hundred bytes (a bounds or validity early-out, then
usually one more `s_and_saveexec` region) and never provably gets back to a full
EXEC before its tail. So the whole body runs under a partial mask. Folding
guards only where EXEC is full cannot shrink their DXIL.

The one exception is `CS_ScreenProbes_ActivateRays`, which is 97.9% uniform. It
is scalar-dominated: only 13.5k of its 74.6k instructions are vector, and it has
just 34 EXEC writes.

| insts | vector | uniform | guard runs | guarded/run | shader |
|---:|---:|---:|---:|---:|---|
| 88,862 | 53,548 | **0.0%** | 18,943 | 2.8 | `indirect_diffuse_lighting:CS_ScreenProbes_ActivateProbes` |
| 88,444 | 53,311 | **0.0%** | 18,891 | 2.8 | `indirect_diffuse_lighting:CS_ScreenProbes_ActivateProbesTarget` |
| 74,570 | 13,544 | **97.9%** | 48 | 5.9 | `indirect_diffuse_lighting:CS_ScreenProbes_ActivateRays` |
| 46,642 | 27,195 | 0.3% | 7,341 | 3.7 | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_OneOver2RPP_Tiled` |
| 45,303 | 26,134 | 0.3% | 7,267 | 3.6 | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_Tiled` |
| 44,340 | 25,333 | 0.3% | 7,228 | 3.5 | `rtdirectionalshadows:CS_RTShadowTrace32HQ_Tiled` |
| 31,049 | 19,747 | 0.4% | 4,519 | 4.4 | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_OneOver8RPP_Tiled` |
| 29,157 | 20,145 | 0.1% | 3,410 | 5.9 | `cascadeshadows_rendering:CS_ShadowRenderTiledCascadesAlphaCBPPPC` |
| 26,738 | 16,642 | 0.4% | 3,992 | 4.2 | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_OneOver2RPP_TiledFarOnly` |
| 26,438 | 16,164 | 0.4% | 4,042 | 4.0 | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_OneOver4RPP_Tiled` |
| 25,442 | 15,589 | 0.4% | 3,876 | 4.0 | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_TiledFarOnly` |
| 25,105 | 16,624 | 0.3% | 3,288 | 5.0 | `cascadeshadows_rendering:CS_ShadowRenderTiledCascadesCBPPPC` |
| 24,496 | 14,794 | 0.5% | 3,832 | 3.8 | `rtdirectionalshadows:CS_RTShadowTrace32HQ_TiledFarOnly` |
| 21,245 | 15,264 | 0.4% | 1,922 | 7.9 | `water_micro_geom_renderer:CS_SurfacePrimitives` |
| 21,065 | 14,564 | 0.5% | 2,773 | 5.2 | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_OneOver8RPP_TiledFarOnly` |

The remaining >20k programs are 42 `*:CS_RaytracingResolve` material variants
(20.0k–21.9k instructions, about 14.5k vector each). All of them are 0.2–0.5%
uniform, with about 6.2 guarded instructions per guard run.

By family (all programs over 20,000 instructions):

| family | programs | vector | uniform | guarded/run |
|---|---:|---:|---:|---:|
| `CS_ScreenProbes_ActivateProbes` / `…Target` | 2 | 106,859 | 0.0% | 2.8 |
| `CS_ScreenProbes_ActivateRays` | 1 | 13,544 | 97.9% | 5.9 |
| `CS_RTShadowTrace32*` | 9 | 176,162 | 0.4% | 3.9 |
| `CS_ShadowRenderTiledCascades*` | 2 | 36,769 | 0.2% | 5.5 |
| `CS_RaytracingResolve` | 42 | 608,679 | 0.4% | 6.2 |
| `CS_SurfacePrimitives` | 1 | 15,264 | 0.4% | 7.9 |
| **all >20k** | **57** | **957,277** | **1.7%** | **5.0** |

### Where the mask goes partial

These are traces of the strict analysis, with byte offsets into each program's code:

- `ActivateProbes` (0x77600 bytes): `v_cmp_lt_u32` ×2 → `s_or_b64` → `s_xor_b64 …, -1`
  → `s_and_saveexec_b64` at 0x68. If no lane survives, a `s_getpc`/`s_setpc` long
  jump goes to `s_endpgm`. EXEC is partial from 0x6c to the end. A second
  `s_and_saveexec_b64` at 0x1b18 opens a region whose saved mask is only
  restored at 0x773d4, 0x22c bytes before the end. Inside, it is ordinary
  structured code: if-regions saved/restored with `s_mov_b64 exec, s[n]`, and
  loops with break masks.
- `RTShadowTrace32HQ_Tiled` (0x37300 bytes): a `v_cmpx_eq_i32` at 0x88 is undone at
  0x11c. Then `s_and_saveexec_b32` + long jump at 0x180 leaves EXEC partial for
  the rest of the program. Only a handful of ~70-byte islands get back to a full mask.
- `ShadowRenderTiledCascadesCBPPPC` (0x22740 bytes): the same shape, partial from
  0x168 onward.

## Whole database

25,194 programs from 24,804 records (entry 0 of `shaders.rpf`), counting
reachable vector instructions only:

| | programs | vector insts | uniform | guarded | guard runs | guarded/run |
|---|---:|---:|---:|---:|---:|---:|
| **all** | 25,194 | 30,781,089 | **9.5%** | **90.5%** | 3,831,535 | 7.3 |
| CS | 2,647 | 10,605,285 | 1.6% | 98.4% | 1,617,159 | 6.5 |
| MS | 9,282 | 5,950,601 | 4.7% | 95.3% | 633,533 | 9.0 |
| PS | 10,944 | 13,483,999 | 15.4% | 84.6% | 1,550,655 | 7.4 |
| VS | 1,931 | 714,063 | 55.2% | 44.8% | 27,623 | 11.6 |
| ES | 195 | 4,161 | 9.4% | 90.6% | 810 | 4.7 |
| GS | 195 | 22,980 | 1.7% | 98.3% | 1,755 | 12.9 |

The uniform share falls with program size:

| program size | programs | vector insts | uniform | guarded/run |
|---|---:|---:|---:|---:|
| < 2k | 19,599 | 7,197,642 | 22.3% | 7.3 |
| 2k–8k | 4,724 | 14,259,587 | 8.7% | 7.9 |
| 8k–20k | 814 | 8,366,583 | 0.7% | 6.8 |
| 20k–50k | 54 | 836,874 | 0.4% | 5.5 |
| ≥ 50k | 3 | 120,403 | 11.0% (`ActivateRays`) | 2.8 |

Small shaders fold well. A typical ~1,000-instruction decoration pixel shader
was 86% uniform. The mass of guarded instructions sits in the large
compute and pixel programs.

**Compared with gefydev's `recomp-lane-masks` (37.1% of vector instructions
still guarded):** this analysis keeps 90.5% guarded. That is consistent with it
being a conservative lower bound (it should never report more uniform than a
precise analysis). The gap is large, though, and I have not checked whether
their "uniform" means the same thing. A uniformity that is relative to an
enclosing region (a structurizing backend), or that treats early-outs as lane
termination, would count far more instructions than "EXEC equals the entry
mask", which is what this tool proves. So treat the two numbers as different
measures, not as a calibration of one against the other.

## What this means for the backend

1. **Per-instruction folding of full-EXEC regions will not fix the compile
   bombs.** Their foldable share is about 0%. Where folding works (VS, small
   PS/MS), the programs were never the compile-time problem.
2. **Guard coalescing is safe today and helps everywhere.** Within a
   straight-line run of guarded vector instructions, no instruction writes EXEC,
   so one `if (lane active)` can cover the whole run. The `guard runs` column
   counts these guards. It is about 5× fewer guards than instructions for the
   >20k programs, 7.3× across the database, and 2.8× for `ActivateProbes`,
   where scalar instructions are densely interleaved and break the runs. (A
   run is also split by any scalar instruction, since scalar code must run for
   every lane.)
3. **The real lever for the bombs is structural.** Their body is one or two
   per-lane regions deep. A backend that turns the outermost early-out and region
   into real DXIL control flow (`if (cond) return;` / `if (cond) { … }`) can then
   treat the body as uniform relative to that region. This analysis does not
   prove that relative uniformity.

   A prototype of the early-out half was tried. It treated never-undone
   narrowings as lane termination, guarded by checks that no lane is re-enabled,
   no `s_barrier`, and no `v_readlane`/`ds_[b]permute` afterwards. It moved the
   database from 9.5% to 10.9% uniform (CS 1.6→5.0%, MS 4.7→5.8%) but left the
   bombs at ≤0.6%, because of the second nested region. So it was not kept.
   Region-relative folding needs a structurizer, not a better mask lattice.

## The analysis (`src/formats/uniformity.rs`)

It is a forward dataflow over the control flow that `forge9`'s RDNA2 decoder
walks. It also follows the compiler's long jump (`s_getpc_b64` / `s_add_u32 lit`
/ `s_addc_u32` / `s_setpc_b64`), which the heavy shaders use for every far
branch. Every SGPR, including VCC and EXEC, carries an abstract mask:

| value | meaning |
|---|---|
| `Full` | covers every live lane: `E ⊆ mask ⊆ W` |
| `Sub` | a subset of `W` (`0` included) |
| `Top` | unknown |

`E` is the entry EXEC (the live lanes). `W` is `E` plus the helper lanes of a
pixel shader's quads. Lanes outside `W` do not exist for a translator, and
helper lanes have no side effects, so running them is invisible. Merges take the
pointwise max (`Full < Sub < Top`). A vector instruction is **UNIFORM** only if
EXEC is `Full` on every path reaching it. Scalar instructions are never
counted.

Rules:

1. At entry EXEC is `Full` and everything else is `Top`. The constant `-1` is
   `Full` (`s_mov_b64 exec, -1` opens every lane of `W`; mesh shaders start
   this way). `0` and `null` are `Sub`; other constants are `Top`. `s_wqm` keeps
   `Full`/`Sub`, since it only adds helper lanes (most pixel shaders start with
   `s_wqm_b64 exec, exec`).
2. Mask ops at the wave's mask width (`_b64` in wave64, `_b32` in wave32) are
   evaluated: `s_mov`, `s_cmov` (join), `s_and/or/xor/andn2`, and the
   `s_*_saveexec` / `s_*_wrexec` forms. `and` with anything `⊆ W` is `Sub`
   (`Full & Full = Full`). `or` with `Top` is `Top`, otherwise it is `Full` if
   either side is `Full`. `xor` of two `⊆ W` values is `Sub`. `a & ~b` is `Sub`
   when `a ⊆ W`. `orn2`, `nand`, `nor`, `xnor`, `not`, `cselect` and
   wrong-width ops give `Top`.
3. So `s_and_saveexec s[n], vcc` makes EXEC `Sub` and saves `Full` in `s[n]`.
   `s_mov_b64 exec, s[n]` or `s_or_b64 exec, exec, s[n]` restores `Full` only
   if `s[n]` provably still holds a full mask on every path.
4. `v_cmpx_*` (VOPC or VOP3) leaves EXEC a subset of its old value: `Sub` at
   best.
5. Any other SGPR write makes its registers `Top`. That covers SALU `sdst` (two
   registers for 64-bit opcodes), SMEM `sdata` (the load width), the `sdst` of
   VOP3 `v_cmp` / carry-out / `v_div_scale` / `v_mad_*64` (one mask),
   `v_readlane` / `v_readfirstlane`, and VCC from VOPC/VOP2. A write that lands
   on EXEC this way makes EXEC `Top`.
6. `s_call_b64`, `s_swappc_b64`, `s_movreld_*`, `s_subvector_loop_*`, and any
   SALU/VOP3 opcode with no GFX10.3 name make every register `Top`.
7. Branches (`s_cbranch_execz/execnz`, `vccz`, `scc*`, `s_branch`) change no
   mask. The region they guard is already `Sub` from rules 3–4, and merges
   join.

**Conservatism.** Every rule moves values only toward `Top`, so UNIFORM is a
provable subset, and real foldability is at least this high. Known gaps, all of
which err toward GUARDED:
- No relational reasoning: a loop's break mask OR'd back into EXEC stays `Sub`.
- VCC from `v_cmp` is `Top`, even though it is a subset of EXEC.
- Literal masks other than `0` (and `-1` in wave32) are `Top`.
- Early-outs keep the rest of the program `Sub`.
- Uniformity is only ever relative to the entry mask.

**Assumptions** (where the result is a model, not a proof about the hardware):
- Lanes outside `W` and helper lanes are treated as invisible.
- The CFG is the decoder's static walk plus the long-jump idiom. An unmatched
  `s_setpc_b64` (the ES half of a merged ES+GS program tail-jumps to its GS
  half) ends the path.
- Calls clobber everything.

**Test**: `saveexec_region_is_guarded` covers these cases:
- A `s_and_saveexec` / `s_cbranch_execz` region restored by `s_mov_b64` and by
  `s_or_b64`.
- A clobbered saved mask (no restore proven).
- `v_cmpx` splitting runs.
- `s_mov_b64 exec, -1` reopening lanes.
- A break-mask loop restored after the loop.
- The long-jump edge.
