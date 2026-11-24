# BCPack BC5 / BC7 stream structure

Structural measurements of BCPack chunks of the 16-byte-block codecs, taken
from one retail texture store with `forge9 bcpack-analyze`. This document has
no fitted tables, dictionaries or codebook values. Every number below is a
count or statistic from our own sample. "Confirmed" means the
entropy decoder reproduces it exactly. Anything that needs block semantics is
labeled as observed or unknown.

The entropy stage is the MDU rANS lane decoder from bcpack-rs
(`bcpack::rans::decode_lanes`, a port of Microsoft's MIT `silicon-rans`
Verilog plus the feeder and lockstep rules). forge9 adds a byte source that
records which lane takes each stream byte. Each lane's own bytes can then be
split into fragments by an independent header parser and checked against the
decoder.

## Sample

`ttdb_graphics.ttpf`: 1,549 chunks, 912 textures.

| Codec id | Chunks | Distinct payloads | Textures they belong to | Blocks / chunk | Stored bytes | Bytes / block |
|---|---|---|---|---|---|---|
| `0x14` BC5 | 100 | 67 (one payload appears 19 times) | mip 0 slices of 4 `BC5_UNORM` (DXGI 83) 256x256 arrays (x8, x32, x48, x64) | 4,096 (64 KiB virtual) | 6,110,310 | 14.92 (93.2% of raw BC5) |
| `0x1C` BC7 | 27 | 27 | `BC7_UNORM` (DXGI 98) textures, 32x32 to 512x512 | 1,024-16,384 (median 4,096) | 3,300,338 | 13.89 (86.8% of raw BC7) |
| `0x0C` BC3 | 1 | 1 | one DXGI 77 texture | 2,048 | 16,189 | 7.90 (49.4%) |
| `0x18` BC6H | 0 | | | | | not tested |

Codec ids map to DXGI formats as expected. Every `0x14` chunk overlaps only
DXGI 83 textures, and every `0x1C` chunk overlaps only DXGI 98 textures.
BC5 statistics below are weighted over all 100 chunks, duplicates included.

## 1. Symbols per lane: two vectors per 16-byte block (corrected)

Each lane emits **`virtual_length / 8` symbols**, the same 8-byte unit as
BC1. A 16-byte BCn block therefore takes **two symbol vectors**.

| Symbols per lane | BC5 converging | BC7 converging | BC3 converging |
|---|---|---|---|
| `virtual_length / 16` (one per block) | 0/100 | 0/27 | 0/1 |
| `virtual_length / 8` | **100/100** | **27/27** | **1/1** |
| `virtual_length / 8` +- 1 | 0/100, 0/100 | 0/27, 0/27 | 0/1, 0/1 |

With one vector per block, decoding stalls partway through the chunk: after
27-50% of the stored bytes for BC5 and 50-51% for BC7. That failure is the
reason the lane widths can look wrong when tested alone.

The two vectors of block i are vectors 2i and 2i+1, not 2i+1 and 2i+2. On
every BC7 lane and on BC5 lanes 0 and 4-11, a lane's symbol in vector 2i
shares more information with vector 2i+1 than with vector 2i+2. The BC5
endpoint-residual lanes 2-3 show no preference (0.075-0.078 against 0.082
bits).

| | MI(2i, 2i+1) | MI(2i+1, 2i+2) |
|---|---|---|
| BC5 lanes 4-7 | 0.370-0.409 bits | 0.111-0.120 |
| BC5 lanes 8-11 | 0.540-0.565 | 0.152-0.158 |
| BC7 lane 15 | 1.724 | 0.067 |
| BC7 lanes 5, 9, 10 | 0.262-0.348 | 0.051-0.076 |

## 2. Lane widths (confirmed)

The widths are symbol widths. The Verilog `alphabet_bits` value is width - 1,
as for BC1.

| Codec | Lanes | Widths |
|---|---|---|
| BC5 | 12 | `4,4,8,8,6,6,6,6,6,6,6,6` |
| BC7 | 16 | `4,4,8,8,8,8,8,7,7,5,6,8,8,8,8,5` |
| BC3 | 12 | `4,4,8,6,6,6,6,5,6,5,8,8` |
| BC6H | 16 | `4,4,8,8,8,5,5,5,6,6,6,8,8,8,8,5` (no chunks in sample, untested) |

At `virtual_length / 8` symbols, changing any single lane's width by -1 or +1
makes **every** chunk fail. No chunk converges under any of the 24 BC5, 32
BC7 or 24 BC3 single-lane perturbations (0/100, 0/27 and 0/1). Reading the
widths as `alphabet_bits` directly (every lane one bit wider) fails on all
chunks too: 0/100 BC5 and 0/27 BC7.

## 3. Entropy-stage self-consistency

| Check | BC5 | BC7 | BC3 |
|---|---|---|---|
| Chunks where every lane emits exactly the target count | 100/100 | 27/27 | 1/1 |
| Lanes ending in a legal state (`L`, `L>>8`, `L>>16`) | 1,200/1,200 | 432/432 | 12/12 |
| End state `L` / `L>>8` / `L>>16` | 1,163 / 29 / 8 | 409 / 17 / 6 | 11 / 1 / 0 |
| Bytes consumed == stored length - pad | 100/100 | 27/27 | 1/1 |
| Remaining bytes all zero | 100/100 | 27/27 | 1/1 |

The **end-state validity rate is 100%** (1,644/1,644 lanes over 128 chunks).
No chunk stalls. All three codecs reach the entropy level cleanly.

## 4. Fragment headers (confirmed)

Each lane's byte sequence, rebuilt from the feeder's byte-to-lane assignment,
splits into back-to-back `header + payload` fragments:

```
byte 0: bits 3:0 q (model), bit 4 flush, bit 5 three-byte header, bits 7:6 z (alphabet narrowing)
byte 1: payload length - 1                        (two-byte header)
        low byte of (payload length - 257)        (three-byte header)
byte 2: high byte of (payload length - 257)       (three-byte header only)
```

The payload length counts every byte the lane reads in the fragment,
including its four initial-state bytes. The sum of header and payload bytes
equals the bytes the decoder fed that lane in **1,200/1,200** BC5 lanes,
**432/432** BC7 lanes and 12/12 BC3 lanes. The fragment count equals the
decoder FSM's count in every one of those lanes.

| | BC5 | BC7 | BC3 |
|---|---|---|---|
| Fragments | 1,519 | 3,144 | 126 |
| Fragments per lane | 1-15 | 1-263 | 1-28 |
| Flush bit set | **1,519 (all)** | **3,144 (all)** | **126 (all)** |
| Three-byte headers | 1,201 | 1,151 | 7 |
| Payload bytes min / median / max | 4 / 5,875 / 6,148 | 4 / 71 / 20,948 | 4 / 18 / 3,839 |
| z = 0 / 1 / 2 / 3 | 1,177 / 92 / 81 / 169 | 907 / 537 / 1,295 / 405 | 76 / 13 / 17 / 20 |
| Models q used | all 16 | q0-q14 (no q15) | 14 of 16 |

Every fragment in the sample is flushed. A 4-byte payload is the minimum: it
holds only the initial state. BC5 lane 1 is constant in every chunk and
always codes its 8,192 zero symbols the same way: one fragment, q = 11,
z = 3 (a 1-bit alphabet), 6 bytes in total (100/100 chunks).

## 5. Lane statistics

Below, "even" and "odd" mean the first and second vector of each block. MI is
mutual information between the symbols of two lanes in the same vector.

### BC5

| Lane | Width | Coded bits / symbol | Entropy | Zero % even / odd | Distinct |
|---|---|---|---|---|---|
| 0 | 4 | 3.98 | 3.99 | 8.6 / 6.6 | 16 |
| 1 | 4 | 0.006 | 0 | 100 / 100 (constant in 100/100 chunks) | 1 |
| 2 | 8 | 4.43 | 4.20 | 32.9 / 32.2 | 256 |
| 3 | 8 | 4.43 | 4.21 | 32.9 / 32.7 | 256 |
| 4-7 | 6 | 5.88-5.90 | 5.93 | 3.0-3.8 | 64 |
| 8-11 | 6 | 5.79-5.84 | 5.66-5.69 | 4.6-6.3 | 64 |

- Even and odd vectors have the same statistics on every lane. The two vectors
  of a BC5 block look like the same layout used twice. That fits the BC5
  structure of two independent BC4-style channel halves, but it is not
  proven.
- Bit budget per vector: 8 + 8 + 8 x 6 = 64 bits in lanes 2-11, the exact
  size of a BC4 half-block (two 8-bit endpoints and sixteen 3-bit indices).
  Lane 0 adds 4 bits and lane 1 is unused. This is a capacity match, not a
  verified mapping.
- Lanes 2 and 3 are distributed like zigzag residuals. Zero is the most
  common value (about 33%), and the per-segment mean frequency falls steadily
  (1, 0.38, 0.21, 0.10, 0.04 ...). This is the same shape as the BC1
  endpoint-residual lanes.
- Lane 0 is almost uniform over 16 values (3.99 bits) and shares 0.02 bits or
  less with every other lane. If it were a BC1-style back-reference, it would
  strongly gate whether the residual is zero. It does not: P(lane 2 = 0) is
  0.37-0.39 when lane 0 is 0 or 1, and 0.31-0.33 for every other value.
- Lanes 4-11 are nearly incompressible, and they form two dependency groups:
  - Lanes 4, 5, 6 and 7 form a chain. MI between neighbours is 0.61-0.64
    bits, and MI between non-neighbours is 0.11-0.16 bits.
  - Lanes 8-11 form a 2x2 lattice. MI is 0.20-0.22 bits for the pairs 8↔10
    and 9↔11, and 0.15-0.16 bits for 8↔9 and 10↔11.

  Inside one symbol, the two 3-bit halves share little: 0.03 bits in lanes
  4-7 and 0.15-0.16 bits in lanes 8-11. Two adjacent pixels' raw 3-bit indices
  would share far more than that, since whole symbols of neighbouring lanes
  share 0.6 bits. So each symbol is probably an index into a pattern table,
  not raw index bits.

### BC7

| Lane | Width | Coded bits / symbol | Zero % even / odd | Distinct even / odd | Max seen |
|---|---|---|---|---|---|
| 0, 1 | 4 | 4.00 | 7-9 | 16 / 16 | 15 |
| 2, 3, 4, 6 | 8 | 2.88-3.59 | 34-44 | 94-140 | 195-233 |
| 5 | 8 | 1.30 | 76.9 / 76.5 | 32 / 32 | **31** |
| 7, 8 | 7 | 3.04-3.61 | 34-43 | 67-121 | 81-127 |
| 9 | 5 | 1.33 | 81.2 / 80.8 | 32 / 32 | 31 |
| 10 | 6 | 2.33 | 66.0 / 65.9 | 32 / 32 | **31** |
| 11-14 | 8 | 4.43-4.57 | 33-39 | 226-256 | 255 |
| 15 | 5 | 4.58 | **0.0 / 8.9** | **31 / 16** | 31 |

- Lanes 0 and 1 look like raw bits. They are uniform (3.99-4.00 bits) and
  share 0.01 bits or less with every other lane, as BC5 lane 0 does.
- Lanes 5 and 10 never exceed 31, so they use only a 5-bit range of their 8-
  and 6-bit alphabets. Together with lane 9, they form a group that is mostly
  zero (66-81%). MI is 0.12-0.17 bits between them, 0.07-0.09 bits with lane
  15, and 0.04 bits or less with every other lane.
- Lanes 11, 12, 13 and 14 form a chain: MI between neighbours is 0.17-0.19
  bits and 0.09-0.10 bits otherwise. Lanes 2-8 have weak pairings at a stride
  of 4 (2↔6, 3↔7, 4↔8: 0.02-0.04 bits) and weak neighbour pairings in 6-8
  (0.05 bits).
- **Lane 15 is the only lane whose statistics depend on the vector.** In the
  even vector it takes values 1-31 and is never 0. In the odd vector it takes
  0-15. The pair is strongly dependent: MI is 1.72 bits, and
  H(odd | even) = 2.03 bits against H(odd) = 3.75 bits. Only 219 of the 496
  possible (even, odd) pairs occur, and the odd range moves down as the even
  value rises: 10-15 when even = 1, 1-10 when even = 10, 0-4 when even = 20,
  and 0-2 when even = 31.
- The even lane-15 value predicts the zero rates of the other lanes in its
  vector. For example, when even = 20, lanes 9 and 10 are zero in 99% of
  blocks and lanes 6-8 in about 82%. When even = 1, lane 9 is always zero and
  lanes 11-14 are zero in 31-38% of blocks. Within even vectors, lane 15
  shares 0.11-0.16 bits with lanes 5 and 9-14. So the lane-15 pair behaves like a **per-block control
  symbol** that selects which fields are present (a mode or partition-like
  class). Each chunk uses 28-31 of the 31 even values.

## 6. The gap: lane symbols to BCn block bytes

The entropy layer is exact for BC3, BC5 and BC7. What remains is the mapping
from each block's two symbol vectors to the 16 bytes of the BCn block. This
mapping is a fixed transform in the decoder hardware. It is not carried in
the stream: every chunk has only fragment headers (a model q, a narrowing z,
a length and a flush bit) and rANS payload. That is why the mapping has to be
fitted against reference data, such as plain (zlib) mips of the same texture,
which this store has next to the BCPack mip 0 slices.

**BC5.** The unknowns, from most to least constrained:

1. **Vector to channel.** Which vector of the pair is red and which is green.
   The statistics are symmetric, so the stream does not show it.
2. **Endpoint prediction.** Lanes 2 and 3 are residuals, but the predictor is
   not in the stream. It could be a previous vector, the other channel of the
   same block, or a fixed preset as in BC1's window for references before the
   chunk. The rule for wrapping or clamping 8-bit endpoints is also unknown.
3. **Lane 0.** A 4-bit, near-uniform, near-independent field of unknown
   meaning. It could be low endpoint bits, a selector, or something else.
   Lane 1 is unused, as in BC1.
4. **Index lanes 4-11.** Two things are missing. One is which of the 16
   pixels each lane covers. The MI topology (a chain of four and a 2x2
   lattice of four) constrains this but does not settle it. The other is the
   table from each 6-bit symbol to two 3-bit BC4 indices, or to some other
   pattern. The symbols do not look like raw index bits. A 64-entry
   symbol-to-pattern table per lane group is the natural analogue of BC1's
   256-entry quadrant codebook. Its values are exactly what fitting has to
   supply.
5. **Block order.** Gfx10 tiled order with 16-byte elements, by analogy with
   BC1. This is not verified for BC5.

**BC7.** The same kinds of unknowns, plus mode handling:

1. **Control symbol.** The table from the lane-15 pair (219 observed
   combinations) to the BC7 mode, partition, rotation and index-selection
   bits. It also determines which other lanes carry meaning in each class.
2. **Field mapping.** Per class, how lanes 2-14 map to endpoint fields,
   p-bits and index groups, including any prediction for the residual-shaped
   lanes (2-8 and 11-14, with zero as the most common value).
3. **Lanes 0 and 1.** Two 4-bit near-uniform fields of unknown meaning.
4. **Vector split.** How the two 110-bit-capacity vectors divide one 128-bit
   block. Only lane 15 differs between the two, so the rest of the layout is
   shared.

BC6H has the same lane-count and vector structure on paper. It is untested
here because the sample has no BC6H chunks.

## Reproducing

```bash
forge9 bcpack-analyze ttdb_graphics.ttpf --codec bc5    # sections 1-5, aggregate
forge9 bcpack-analyze ttdb_graphics.ttpf --codec bc7
forge9 bcpack-analyze ttdb_graphics.ttpf 119            # one chunk: per-lane fragments and top symbols
forge9 bcpack-analyze ttdb_graphics.ttpf 119 -o lanes.bin   # lane-major u16 LE symbol streams
forge9 bcpack-analyze ttdb_graphics.ttpf --codec bc5 --widths 5,5,9,9,7,7,7,7,7,7,7,7   # test other widths
```

The aggregate run prints convergence, end states, header consistency, the
per-lane table with the even/odd split and pairing MI, the lane-to-lane MI
matrix, and the perturbation results. The lane-15 joint table and the
conditional zero rates in section 5 come from the `-o` dumps.
