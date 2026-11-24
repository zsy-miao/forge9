# Shader Compile Cost: the heaviest programs

Which native shader programs are largest, as a proxy for how long a PC
translator's DXC/driver pipeline build takes on them. The game's own GPU
watchdog kills the title after ~30 s without a present, so a few shaders that
take minutes to compile can block world load on their own (see xcl #505,
#1269). Extracted with `forge9 isa` from the decrypted `shaders.rpf`.

```
forge9 isa shaders.rpf 0 --heavy 40     # the 40 heaviest programs
```

## Distribution (25,194 programs, entry 0)

| metric | instructions |
|---|---|
| median | 669 |
| p90 | 4,627 |
| p99 | 17,018 |
| max | 88,862 |

- 5,591 programs exceed 2,000 instructions, 2,079 exceed 5,000, **871 exceed 8,000**.
- **The top 50 heaviest are all compute shaders.** The largest pixel shader is ~10,500.

## The compile bombs are ray-tracing / global-illumination compute

Of the 871 programs over 8,000 instructions, **709 are ray-tracing or
global-illumination compute shaders** (inlined DXR lowered into a single
compute program, which is why the instruction counts are enormous).

Top 10 by instruction count:

| insts | stage | shader |
|---|---|---|
| 88,862 | CS | `indirect_diffuse_lighting:CS_ScreenProbes_ActivateProbes` |
| 88,444 | CS | `indirect_diffuse_lighting:CS_ScreenProbes_ActivateProbesTarget` |
| 74,570 | CS | `indirect_diffuse_lighting:CS_ScreenProbes_ActivateRays` |
| 46,642 | CS | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_OneOver2RPP_Tiled` |
| 45,303 | CS | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_Tiled` |
| 44,340 | CS | `rtdirectionalshadows:CS_RTShadowTrace32HQ_Tiled` |
| 31,049 | CS | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_OneOver8RPP_Tiled` |
| 29,157 | CS | `cascadeshadows_rendering:CS_ShadowRenderTiledCascadesAlphaCBPPPC` |
| 26,738 | CS | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_OneOver2RPP_TiledFarOnly` |
| 26,438 | CS | `rtdirectionalshadows:CS_RTShadowTrace32TemporalHQ_OneOver4RPP_Tiled` |

Three shader families account for nearly all of the >20k-instruction programs:

- `indirect_diffuse_lighting:CS_ScreenProbes_*` — the screen-probe GI trace/activate passes (the 70k–90k ones).
- `rtdirectionalshadows:CS_RTShadowTrace32*` — RT directional shadows, ~14 HQ/temporal/RPP variants from 10k to 47k instructions each.
- `*:CS_RaytracingResolve` and `shadowmask:CS_TraceShadowRays_*` — material RT resolve and shadow-ray passes.

## Why this helps the watchdog problem

The title compiles pipelines lazily as the world streams in. A translator that
hits one of these shaders mid-frame pays the full DXC + driver build inside the
30-second watchdog window, and xcl #1269 measured `ActivateRays` alone at
842 s of DXC for 10.66 MB of DXIL, with the driver build killed past 20 GB.

Knowing the full list up front (not discovered one crash at a time) lets a
project:

1. **Precompile these offline** with a long time and memory budget, before the
   watchdog is armed (as in xcl's offline pipeline pass).
2. **Bring the world up first with ray tracing disabled**, since the heaviest
   programs are all RT/GI, then enable it once their builds are cached.
3. **Budget memory** for the known-large builds instead of OOMing on discovery.

Reproduce the full ranking with `forge9 isa shaders.rpf 0 --heavy <N>`.
