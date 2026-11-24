# Mesh Shader Root Parameter Binding Model

Extracted from 9,282 mesh shader programs using forge9 FSSM parser.

## Universal pattern

ALL mesh shaders follow the same SGPR slot assignment:

| Slot | Type | Contents | SGPRs used |
|------|------|----------|------------|
| s8   | Table | Main descriptor table (72-261 entries) | 1 |
| s9   | Table | Secondary descriptor table (4-13 entries) | 1 |
| s10  | Table | Single-entry table | 1 |
| s11  | Descriptor | Root CBV/SRV (reg 0-9) | 2 |
| s13  | Constants | 2 dwords (95%) or 1 dword (5%), register b500/b501 | 1 |
| s15  | ResourceHeap | Bindless resource heap | 1 |
| s16  | SamplerHeap | Bindless sampler heap (present in 87% of shaders) | 1 |

## Key findings for PC port

1. **Tables take 1 SGPR (1 dword)** in root memory. This confirms AntonioEspana's assumption in xbox-emulator user_data_abi::pack.
2. **Descriptors take 2 SGPRs (2 dwords)** - a 64-bit GPU virtual address.
3. **Constants are always 2 dwords** (8 bytes) at register b500, except 522 shaders that use 1 dword.
4. **Static samplers**: 11-22 per shader (not in root memory, baked into the pipeline state).
5. **s8 always starts the root parameters** for graphics. The table size varies (72, 126, 197, or 261 entries) but the slot is always s8.

## Root memory layout (7 parameters, 8 SGPRs total)

```
s8:  [Table ptr]        1 dword   p0
s9:  [Table ptr]        1 dword   p1
s10: [Table ptr]        1 dword   p2
s11: [Descriptor lo]    2 dwords  p3
s13: [Constants]        1-2 dw    p4
s15: [ResourceHeap]     1 dword   p5
s16: [SamplerHeap]      1 dword   p6
```

Total: 8-9 SGPRs of user data per mesh shader.

## Pattern distribution

Top 3 patterns cover 10% of all mesh shaders:
- 261-entry + 8-entry + 1-entry + reg5 + 2dw + heaps + 13 samplers (371 shaders)
- Same with 11 samplers (297)
- 197-entry variant (261)

The binding model is highly uniform. A single rootmem.rs implementation handling these 7 parameter types would cover 100% of mesh shaders.

## Wave size requirements

Extracted from FSSM flags (bit 5 at +0x14) across all 25,194 shader programs.

**100% of mesh shaders require wave64.** No mesh shader uses wave32.

| Stage | Wave64 | Wave32 | % wave64 |
|-------|--------|--------|----------|
| MS    | 9,282  | 0      | 100%     |
| VS    | 1,931  | 0      | 100%     |
| PS    | 9,817  | 1,127  | 89.7%    |
| CS    | 1,112  | 1,535  | 42.0%    |
| ES/GS | 390    | 0      | 100%     |
| **Total** | **22,532** | **2,662** | **89.4%** |

Implications for PC port:
- AMD RDNA2/3 GPUs support wave64 natively (this is why XCL runs furthest on AMD)
- NVIDIA GPUs use warp32 natively and must emulate wave64 for all mesh/vertex/geometry shaders
- Only pixel shaders and compute shaders have wave32 variants available
