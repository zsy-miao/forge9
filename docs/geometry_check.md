# Reference geometry dumps and position checks

Two commands for telling a mesh-shader translation bug from a bad upstream
writer: `model --dump-buffers` writes what a drawable's geometry is on disk,
and `check-positions` compares a position buffer captured from a run with it.
Code: `src/formats/geometry_check.rs`; drawable layout:
[drawable_format.md](drawable_format.md).

## `forge9 model <archive> <index> --dump-buffers <dir>`

For each geometry `g<i>` of the LOD (`--lod`, default 0), in the order
`model` prints them:

| File | Content |
|------|---------|
| `g<i>.vertex.bin` | the stored vertex buffer, byte for byte (`elements * element_stride` bytes) |
| `g<i>.index.bin` | the stored index buffer, byte for byte: 3 header words, meshlet records, triangle words, vertex refs |
| `g<i>.meshlets.bin` | the meshlet records alone (`index.bin` bytes `12 .. 12 + 16 * meshlets`) |
| `g<i>.json` | the reference below |

No model is written unless `-o` or `--obj` is also given. These files are
game data: keep them local.

`g<i>.json` (`"format": "forge9-geometry-v1"`):

| Key | Meaning |
|-----|---------|
| `drawable`, `lod`, `geometry` | names as `model` prints them |
| `files` | the three `.bin` names, relative to the JSON |
| `vertex_count`, `triangle_count`, `aabb` | `aabb` is `{min, max}` of the decoded positions |
| `vertex_buffer` | `bytes`, `elements`, `element_stride`, `descriptors` (the 16 u32 at +0) |
| `attributes` | one per semantic with a nonzero layout size: `semantic`, `name` (position, normal, color, texcoord0/1, or unknown), `offset`, `size`, `dxgi_format`, `descriptor` |
| `position` | `format` (66 quantized, 99 float3), `offset`, `stride` (8 or 12), `quantization` |
| `position.quantization` | `bias`, `axis_order`, `scale`, `lane_bits` [24, 24, 16] (format 66 only) |
| `texcoord_scale` | the f32 at vertex data +0x7C, when texcoords are decoded |
| `index_buffer` | `bytes`, `meshlet_records_offset` (12), `triangles_offset`, `refs_offset`, `ref_size` (2 or 4) |
| `meshlets` | `index`, `vertices`, `triangles`, `first_ref`, `first_triangle`, `record` (the four stored u32), `aabb` |
| `meshlet_refs` | meshlet-local vertex to mesh vertex, concatenated over meshlets |
| `indices` | triangle list in mesh vertex indices, meshlet by meshlet |
| `positions` | decoded model-space positions, mesh vertex order |

The vertex buffer is structure of arrays: semantic `s` is `vertex_count`
elements of `size` bytes at `offset`. Format 66 decodes as
`bias + scale * lane`, lanes `X = bits 8..31`, `Y = bits 0..7 | bits 32..47 << 8`,
`Z = bits 48..63` of the u64, then reordered by `axis_order` (0: `(Z, Y, X)`,
1: `(Y, Z, X)`, 2: `(X, Y, Z)`). Meshlet record word 0: `vertices - 1` (bits
0..6), `triangles - 1` (7..13), bit 16 of the ref start (14) and of the
triangle start (15); word 1: ref start (low 16) and triangle start (high
16); words 2 and 3 are not decoded. Triangle word: three 7-bit
meshlet-local corners, meshlet index from bit 21.

## `forge9 check-positions <g.json> <capture> [options]`

The capture is raw bytes, in one of three layouts (`--layout`):

| Layout | Element | Default stride | Compared with |
|--------|---------|---------------:|---------------|
| `stored` | the vertex buffer as stored (same bytes as `g<i>.vertex.bin`), from `--offset` | (layout) | positions decoded with the capture's own descriptor and quantization header; bytes compared with `g<i>.vertex.bin` when it sits next to the JSON |
| `float3` | model-space f32x3 | 12 | `positions` |
| `clip` (default) | post-transform f32x4 clip position (x, y, z, w) | 16 | `positions` through a fitted `clip = M * (x, y, z, 1)` |

Element order: mesh vertex order (default), or `--meshlet-order`
(element `k` is vertex `meshlet_refs[k]`), or `--meshlet-order
--meshlet-slots N` (element `m * N + j` is local vertex `j` of meshlet `m`,
the other slots ignored). `--stride` and `--offset` place the elements in a
larger buffer; `--count` limits them; a short capture checks what it holds.

The clip check fits one 4x4 matrix by least squares to the plausible
elements (finite, `w > 0`, not outlying), then refits three times without
those above 8x the median residual. That recovers the draw's MVP for a
rigid mesh with no transform supplied; a skinned mesh does not fit one
matrix. It reports how many elements the fit used, and warns when it is
under half.

Counts (summary first, then the worst `--top` elements, default 20; `--top 0`
prints counts only):

| Count | Layouts | Meaning |
|-------|---------|---------|
| non-finite | all | any NaN or Inf component |
| `w <= 0` | clip | |
| outlying | clip | `\|x/w\|` or `\|y/w\|` above `--ndc-limit` (16) |
| outside the clip volume | clip | `\|x\| > w`, `\|y\| > w`, `z < 0` or `z > w`; informational, not counted as bad |
| differ | all | model space: max component distance above `--tolerance` (1e-3); clip: max component residual from the fit above `--clip-tolerance` (1e-3) times `\|w\|` |
| outside the AABB | all | position (clip: unprojected through `M^-1`) outside the geometry AABB by more than `--tolerance` |
| bad | all | any of the above but the informational one |
| triangles with a bad corner | all | over `indices`, corners mapped to elements the same way |
| corner at `w <= 0` | clip | triangles not measured on screen |
| covering over 1/4 of the screen, screens covered | clip | area of each triangle's NDC projection inside [-1, 1]^2, in screens; no culling or depth, so a sum over 1 means overlap |

Bad elements are also counted by meshlet-local vertex index (0-31, 32-63,
64-95, 96-127) and by meshlet, which separates per-lane or per-wave errors
(for example only locals 32 and up) from a buffer that is wrong as a whole.
Exit code 0 when nothing is bad, 1 when something is.

What the results point to:

- `stored` differs: the buffer the title reads is not the on-disk buffer
  (an upload, copy or streaming problem), before any shader runs.
- `stored` identical, `clip` bad: the vertex/mesh program or its
  translation produces the bad positions.
- `clip` bad only for some meshlet-local ranges or meshlets: lane or wave
  mapping in the mesh stage, not the data.
- `clip` fit uses under half the elements: the elements are not this
  geometry (wrong buffer, stale data, another order), or the mesh is skinned.

## Synthetic smoke test

From a source checkout, run the position checks on original, in-memory meshes:

```bash
cargo test --release --locked formats::geometry_check::tests
```

The tests cover model-space and clip-space captures, meshlet order, and
corrupted positions. For a runtime comparison, start with one rigid draw and
record the build SHA, GPU and driver, geometry and LOD, capture layout,
stride, offset, ordering, and the tool's summary. Keep the captured buffers
local. If the stored bytes match but the clip-space check fails, compare
the draw's resource bindings and transforms before changing the position
decoder.

## Worked example

`cvar_am_python_head_01a` (entry `1/28` of `core_restofworld.rpf`):

```
$ forge9 model core_restofworld.rpf 1/28 --dump-buffers dump
  g0 model_0_geometry_0: 3168 vertices, 4849 triangles, 38 meshlets, position format 66, vertex buffer 88832 bytes, index buffer 27744 bytes, semantics 0:position 4:normal 16:unknown 20:unknown 24:color 28:texcoord0
cvar_am_python_head_01a.#dr LOD0: 1 geometries → dump
```

The stored buffer against itself, then with the top byte of 8 positions
flipped:

```
$ forge9 check-positions dump/g0.json dump/g0.vertex.bin --layout stored
  bytes: identical to the stored buffer over 88832 bytes
  bad (any check above)                                 0
$ forge9 check-positions dump/g0.json vertex_bad.bin --layout stored --top 0
  bytes: 8 of 88832 differ from the stored buffer, first at +0x1c7 (semantic 0 position), last at +0x1ff (semantic 0 position)
  differ from the reference by > 0.001                  8
  bad (any check above)                                 8
  with a bad corner                                    30
bad elements by meshlet-local vertex: 0-31 0, 32-63 8, 64-95 0, 96-127 0
meshlets with a bad element: 1 of 38
```

Clip positions made from the reference with a perspective camera, then 3
components set to NaN, 2 w negated, 4 vertices set to (1e6, -3e5, 0.5,
0.01) and 6 moved by 0.01 w in y:

```
$ forge9 check-positions dump/g0.json clip_bad.bin --top 0
capture: clip-space f32x4, stride 16, offset 0, 3168 elements in vertex order
fit: clip = M * (x, y, z, 1) from 3153 of 3168 elements
summary (3168 elements checked):
  non-finite (NaN/Inf)                                  3
  w <= 0                                                2
  |x/w| or |y/w| > 16                                   4
  outside the clip volume (not counted as bad)          6
  differ from the fit by > 0.001 |w|                   12
  outside the AABB by > 0.001                           6
  bad (any check above)                                15
triangles (4849 with every corner captured):
  with a bad corner                                    59
  with a corner at w <= 0 (not measured)                6
  covering over 1/4 of the screen                       0
  screens covered (sum of on-screen areas)          0.089
bad elements by meshlet-local vertex: 0-31 3, 32-63 12, 64-95 0, 96-127 0
meshlets with a bad element: 5 of 38
```

The uncorrupted clip capture gives 0 in every row (fit from 3168 of 3168),
and the same positions written in meshlet order with 128 slots per meshlet
and a 32-byte stride (`--meshlet-order --meshlet-slots 128 --stride 32`)
check 3857 elements (one per meshlet ref) with 0 bad. The three gore
drawables of `story000_restofworld.rpf` `19/0` dump as 15, 4 and 3
meshlets, and each stored buffer checks identical.
