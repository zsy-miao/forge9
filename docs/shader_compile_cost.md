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

- `indirect_diffuse_lighting:CS_ScreenProbes_*`: the screen-probe GI trace/activate passes (the 70k–90k ones).
- `rtdirectionalshadows:CS_RTShadowTrace32*`: RT directional shadows, ~14 HQ/temporal/RPP variants from 10k to 47k instructions each.
- `*:CS_RaytracingResolve` and `shadowmask:CS_TraceShadowRays_*`: material RT resolve and shadow-ray passes.

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

## Match a runtime report to a shader

```bash
forge9 fssm shaders.rpf 0 --json > shaders.json
forge9 fssm shaders.rpf 0 --shader 594 --json
```

The `forge9-fssm-v1` JSON lists every SGD5 record and its native programs
in XDAT order. Each readable FSSM program includes its stages, wave sizes,
and declared machine-code byte counts. A failed record or section includes
an `error` field while the remaining entries stay in the inventory.

| Field | Match against |
| --- | --- |
| `dxbc_fnv1a64` | XCL's `--record-hash` and `--measure-pso` IDs for an inflated DXBC record. |
| `programs[].fssm_sha256` | The SHA-256 of the complete inflated FSSM sidecar in a Leonida shader index. |
| `sgd5_marker_offset` and `name` | VIctory's `translate --list` entries, within the same SGD5 payload. The offset is a byte count from the entry's start. |

A record can contain several native programs. Keep the container ID and
FSSM IDs separate when comparing reports. Compare the source entry,
backend, GPU, driver, scene, and cache state before interpreting timings.
The inventory contains metadata. Keep source archives and captured buffers
local.

To summarize the presentation cadence of an individual run, pass a
[PresentMon CSV](https://github.com/GameTechDev/PresentMon/blob/main/README-CaptureApplication.md)
or a CSV with a `frame_ms` column to the standard-library script:

```bash
python3 scripts/frame_times.py capture.csv
```

Filter captures with multiple streams using `--process-id` and
`--swap-chain`. The report gives p50, p95 and p99 and summarizes available
CPU, GPU and display timing columns separately. Compare like-for-like runs
and verify that the expected content renders. Presentation frequency alone
can include repeated or incomplete frames.

On macOS, launch the renderer with `MTL_HUD_ENABLED=1` and
`MTL_HUD_LOG_ENABLED=1`. Export the
[Metal HUD log](https://developer.apple.com/documentation/xcode/monitoring-your-metal-apps-graphics-performance)
for only the rendering process and the interval you want to compare:

```bash
log show --last 10m --style compact --process "$renderer_pid" --info \
    --predicate 'eventMessage CONTAINS "metal-HUD:"' > metal-hud.log
python3 scripts/frame_times.py --metal-hud metal-hud.log
```

The same percentile summary applies to presentation intervals. Metal HUD
time covers whole command buffers by default and can include gaps between
encoders. The report labels it `metal_gpu_time_ms` to distinguish it from
PresentMon's GPU-busy time. Use a live Metal System Trace to separate copy
encoders, GPU execution and idle periods.
