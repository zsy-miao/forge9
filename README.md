# forge9

General-purpose toolkit for RPF9 archives. Read, decrypt, inspect, extract and analyze.

RPF9 is a container format used by a well-known retail game engine. This tool handles decryption, TOC parsing, entry listing, content extraction, and format detection: a foundation for research, interoperability work and asset tooling.

**Format reference:** [docs/rpf9_format.md](docs/rpf9_format.md), the RPF9 container spec (header, encryption, TOC columns, record layouts, names, compression), with what is known and what is still unknown.

## Scope and provenance

forge9 is an independent, interoperability-oriented implementation of the RPF9
container format. It is documentation and code about a file format, nothing more:

- **No copyrighted content.** The repository contains no game code, assets,
  archives, executables, cipher tables or keys, and none may be added. It
  distributes nothing that belongs to any game or its publisher.
- **You supply your own inputs.** The commands that open an archive need
  `tables.bin` and `keys.bin`, which you extract from your own legally obtained
  copy of the software. forge9 ships neither and cannot produce them.
- **Original fixtures only.** Every test builds its bytes in memory (see
  `CONTRIBUTING.md`); no sample is taken from any game.
- **Not affiliated** with or endorsed by any game publisher. "RPF9" and format
  field names are used descriptively to identify the format under study.

The purpose is to understand and interoperate with a file format, not to
reproduce, distribute or enable access to any protected work.

## Features

- **Decrypt** archive table-of-contents using a 9-key T-table cipher with CBC mode
- **Inspect** headers, entry counts, key IDs, version info and flags
- **List** all entries with offset, size, format detection (zlib, XML, PSIN, nested RPF9, etc.)
- **Extract** entries to disk with automatic zlib chain inflation
- **Tree / search** nested RPF9 archives by magic, extension, size or content
- **Replace / repack** entries in place or at any size, re-encrypting the TOC
- **Decode payloads**: AWC audio streams, SGD5 shader databases, Bink 2 video headers, PSIN game data, paged resources
- **Textures**: list RDS5 (`.ttpf`) stores and decode BCPack BC1 to DDS or PNG (fitted endpoint tables; output is approximate, see [bcpack-rs#4](https://github.com/Ahmet-Dedeler/bcpack-rs/issues/4)), and BCPack BC3/BC4/BC5/BC7 to DDS (DX10) or PNG, including array slices (bcpack-rs `bc5-bc7` branch; checked against the stored mip pyramid on all 129 BC3/BC5/BC7 chunks we have)
- **Models**: export a drawable's LOD geometry (positions, indices, normals, up to two UV sets and the RGBA8 vertex attribute when present, plus one material per shader with its diffuse, normal and emissive textures embedded when they resolve in the `.ttpf` stores, directly or through the `EDUP` dedup table of `textures.rpf`, read lazily so multi-GB and partially downloaded stores work; several entries, e.g. the parts of a character, merge into one scene) to glTF 2.0 (`.glb`, or `.gltf` + `.bin`) or OBJ. Standalone drawables and drawable dictionaries; checked on a real drawable dictionary (see [docs/drawable_format.md](docs/drawable_format.md))
- **Geometry checks**: dump a drawable's stored vertex, index and meshlet buffers with a reference JSON (layout, quantization, decoded positions, meshlets, AABBs), and check a position buffer captured from a run against it: NaN/Inf, `w <= 0`, outlying `x/w`, `y/w`, AABB and reference mismatches, on-screen triangle coverage (see [docs/geometry_check.md](docs/geometry_check.md))
- **Mod packs**: TOML manifests of hash-guarded patches (text, XML, binary) to the user's own files, verified and applied in one command
- **Hexdump** individual entries for analysis
- **JSON output** for integration with other tools
- **Library crate**: every decoder is usable from Rust as `forge9::formats::*`

## Install

Prebuilt binaries for Windows (x86_64), Linux (x86_64) and macOS (arm64, x86_64) are on the [Releases page](https://github.com/nineforge0/forge9/releases): download `forge9-<target>` (`.exe` on Windows) and put it on your `PATH`.

Or build from source with a stable Rust toolchain:

```bash
cargo install --git https://github.com/nineforge0/forge9
```

## Usage

```bash
# Show archive info
forge9 info archive.rpf

# List entries
forge9 list archive.rpf
forge9 list archive.rpf --json
forge9 list archive.rpf --recursive         # nested RPF9 entries too, as 5/3 paths

# Summary: entries per kind, stored and inflated bytes, nested archives, largest entries
forge9 stats archive.rpf --top 10
forge9 stats archive.rpf --json

# Compare two archives by inflated payload: added, removed, changed (with size delta) and
# moved entries; exit code 0 if identical, 1 if they differ, 2 on error
forge9 diff old.rpf new.rpf
forge9 diff original.rpf repacked.rpf --json

# Nested archives and searching
forge9 tree archive.rpf --depth 2
forge9 search archive.rpf --magic ADAT

# Extract all entries
forge9 extract archive.rpf -o output_dir

# Extract specific entries
forge9 extract archive.rpf --select 0,1,5

# Hex dump entry #0
forge9 hexdump archive.rpf 0 --limit 256

# Rebuild with replaced entries (any size)
forge9 repack archive.rpf --patch 6=new.xml -o repacked.rpf

# Same, then reopen the output and check it: every replacement reads back, every other entry is
# byte-identical (offsets may move); exits nonzero on any unexpected difference
forge9 repack archive.rpf --patch 6=new.xml -o repacked.rpf --verify
```

### Format decoders

Every decoder takes an archive and an entry index. Use `a/b` to reach an entry inside a nested RPF9
(e.g. `5/3` is entry 3 of the archive stored as entry 5).

```bash
# Audio streams: id, codec, channels, sample rate, sample count, chunks
forge9 awc audio.rpf 12/0

# Shader records: name, stage, record tag, compressed/decompressed size
forge9 shaders shaders.rpf 2
forge9 shaders shaders.rpf 0 --summary     # counts per stage and tag

# Root-signature layout of each native (FSSM) program: one line per program,
# e.g. "MS <name>: 6 root params [Table(261 entries, s8, p0), ..., ResourceHeap(s15)]"
forge9 fssm shaders.rpf 0 --roots
forge9 fssm shaders.rpf 2 --shader 2       # full layout + used resources of one record
forge9 fssm shaders.rpf 0                  # programs per stage

# RDNA2 machine code of each FSSM program: instruction families and
# wave64-sensitive ops (DPP, permlane, readlane, 64-bit EXEC/VCC masks, ...)
forge9 isa shaders.rpf 2
forge9 isa shaders.rpf 0 --summary         # programs using each op, by stage
forge9 isa shaders.rpf 0 --shader 594      # full family breakdown of one record
forge9 isa shaders.rpf 0 --opcodes         # opcode census (add --stage MS for one stage)
forge9 isa shaders.rpf 0 --uniformity      # EXEC-uniform (guard-foldable) share, per stage and for the 30 heaviest
forge9 isa shaders.rpf 0 --rt-census       # BVH intersect sites, traversal-loop shapes and loop share (--json too)
forge9 isa shaders.rpf 0 --shader 594 --dump out  # raw code → out.<n>.<stage>.bin

# Full disassembly of one record's programs, LLVM (llvm-mc -mcpu=gfx1030) syntax;
# branch targets are byte offsets, unreached dwords print as `.long ... ; unreached`
forge9 disasm shaders.rpf 2 --shader 1
forge9 disasm shaders.rpf 0 --shader 594 --raw   # with each instruction's dwords

# Driver pipeline cache (PS09): graphics/compute PSOs and their shader slots
forge9 pipelines shaders.rpf 1
forge9 pipelines shaders.rpf 1 --mesh      # only PSOs with a mesh (NGG) shader

# Bink 2 video: dimensions, frame count, frame rate, duration
forge9 bink archive.rpf 3

# PSIN game data and paged resources
forge9 psin script.rpf 0 --structs
forge9 resource archive.rpf 7

# Drawable (model) geometry: LOD 0 by default; unhandled vertex formats are skipped and named
forge9 model props.rpf 12 -o prop.glb          # binary glTF
forge9 model props.rpf 12 -o prop.gltf         # glTF JSON + prop.bin
forge9 model props.rpf 12 --obj prop.obj --lod 1
forge9 model props.rpf 12 --textures ttdb_props.ttpf   # texture stores (default: every .ttpf next to the archive)
forge9 model packs.rpf 102/33 102/30 119/75 -o char.glb # several entries (character parts) in one scene
forge9 model packs.rpf 102/33 --dedup textures.rpf      # EDUP table for deduplicated texture keys (default: textures.rpf next to the archive)
forge9 search props.rpf --drawable              # every entry `model` can export, with mesh counts per LOD

# Reference data for emulator testing: stored vertex/index/meshlet buffers + JSON per geometry, and a check of a
# captured position buffer (stored, float3 or float4 clip) against it; see docs/geometry_check.md
forge9 model props.rpf 12 --dump-buffers dump
forge9 check-positions dump/g0.json capture.bin --layout clip --stride 16

# Texture stores (.ttpf files sit next to the archives, not inside them)
forge9 texture store.ttpf                       # list: size, format, mips, tile mode, hash
forge9 texture store.ttpf --extract 12 -o a.dds # BC1/BC3/BC4/BC5/BC7 mip 0 as DDS
forge9 texture store.ttpf --decode 12 -o a.png  # mip 0 as PNG (BC5 as red/green)
forge9 texture store.ttpf --decode 300 --slice 5 # one slice of an array texture
forge9 bcpack-analyze store.ttpf --codec bc7     # BC3/BC5/BC7 entropy stage: fragments, lane stats
forge9 bcpack-analyze store.ttpf 119 -o l.bin    # one chunk; lane symbol streams as u16 LE

# joaat hash of a name
forge9 hash player_one

# Grow the hash dictionary: name unknown PSIN schema hashes from strings found in the archives
forge9 harvest script.rpf common.rpf > names.txt
forge9 --dict names.txt psin script.rpf 1 --structs

# Entry catalog across archives (nested RPF9s included): one row per entry with its id
# (`archive:path`, plus `@hash` when the TOC has a name hash for it), kind, stored and inflated size,
# and its name: from the dictionary, else the embedded label of a drawable or drawable dictionary,
# else candidate names from archive strings whose joaat matches (a 32-bit match can be a
# coincidence), else unresolved. The name hash is the joaat of the entry name, read from
# the TOC's u32 name hash column, a column beside the records rather than a record field; some
# archives have no such column, or one shorter than their records.
forge9 --dict names.txt catalog common.rpf script.rpf
forge9 --dict names.txt catalog common.rpf --write-dict names.txt  # accumulate confirmed candidates into a sorted, deduped dict
forge9 catalog common.rpf --json             # array of {id, archive, path, hash, kind, stored, inflated, status, name, evidence, candidates}
```

### Mod packs

Published mods live in [forge9-mods](https://github.com/nineforge0/forge9-mods).

A mod is a `mod.toml` of **patches** applied to the user's own archive, so a published mod never
contains game files. Each patch targets an entry by path (as `list --recursive` prints it), is
guarded by the SHA-256 of the original inflated payload, and edits it with operations that check
their own preconditions: exact text find/replace with a match count, XML element text or attribute
set by path, and binary patch at an offset with the expected original bytes. Whole-file `file`
replacement is for content the mod author created. Full spec: [docs/mod_format.md](docs/mod_format.md);
example: [examples/xml-tweak](examples/xml-tweak).

```toml
[mod]
name = "example-mod"
version = "0.1.0"
target_archive = "common.rpf"

[[patch]]
path = "9"
base_sha256 = "951e1174c3b64db0e0f40c02f4e6e1f9a3313580a510ac11bd5701d07033e789"

[[patch.op]]
type = "xml"
path = "CVehiclePoints/fNavMaxHeightChange"
attr = "value"
value = "0.400000"
```

```bash
forge9 mod init                          # write a template mod.toml
forge9 mod verify mod.toml               # every guard and op checked, nothing written
forge9 mod apply mod.toml -o modded.rpf  # repack the target with every patch
forge9 mod apply mod.toml -o modded.rpf --verify  # then reopen it: patches present, nothing else changed
forge9 diff common.rpf modded.rpf        # what changed
forge9 mod make --path 9 original.xml edited.xml >> mod.toml   # ops from an edited extracted payload
```

`verify` and `apply` take `--archive path.rpf` to override `target_archive`, and refuse an archive whose
entry does not match `base_sha256` (another game version, or already modded). v1 manifests
(`[[replace]] entry = 6, file = ...`) still load as unguarded whole-file patches.

## Supported formats

Container spec: [docs/rpf9_format.md](docs/rpf9_format.md). Payload formats link their own documents below.

| Format | Magic | Command | Status |
|---|---|---|---|
| RPF9 archive | `9FPR` | `info`, `list`, `tree`, `stats`, `diff`, `catalog`, `extract`, `repack` | Read and write; TOC decrypt/encrypt; spec: [docs/rpf9_format.md](docs/rpf9_format.md) |
| zlib chains | `78 xx` | (automatic) | Inflated on read |
| PSIN / PSO game data | `PSIN` | `psin`, `harvest` | Block table, struct schemas; schema hash names from archive strings |
| AWC audio | `ADAT` | `awc` | Stream table, chunks, formats (Opus, XMA2, PCM); no decode |
| SGD5 shader database | `SGD5` | `shaders` | Record names, stages, tags, sizes; DXBC payload offsets |
| Xbox shader program | `XBCH` → `FSSM` | `fssm` | XDAT section 0x0D inflated; stages, root layout, used resources |
| RDNA2 machine code | (FSSM code range) | `isa`, `disasm` | Length decode by control flow, encoding families, opcodes, wave64-sensitive ops, EXEC-uniformity (DXIL-fold headroom), ray-traversal census; full disassembly with operands (matches llvm-mc on a 3.47M-instruction sample); see [docs/wave64_instruction_usage.md](docs/wave64_instruction_usage.md), [docs/rdna2_opcode_coverage.md](docs/rdna2_opcode_coverage.md), [docs/exec_uniformity.md](docs/exec_uniformity.md), [docs/rt_traversal_census.md](docs/rt_traversal_census.md) |
| Pipeline cache | `PS 00 09` | `pipelines` | Header, graphics/compute PSO records, shader slots, metadata and code ranges |
| Bink 2 video | `KB2?` | `bink` | Header: size, frames, dimensions, frame rate |
| Paged resource | `0DRP` | `resource` | Kind detection, virtual/physical pointer resolution |
| Drawable (model) | (paged, no magic) | `model` | Standalone drawables and drawable dictionaries: LOD groups → models → geometries; positions in formats 66 (quantized) and 99 (float3), octahedral 10:10 normals, two float2/snorm16 UV sets, clustered meshlet indices; to glTF/OBJ; shader group texture references resolved against `.ttpf` stores into glTF materials (key = store texture hash, confirmed on 30901 references). Tangents, skinning and fragments not yet decoded. Checked on a real drawable dictionary (format 66 path); the float3 path only on a synthetic drawable |
| RDS5 texture store | `RDS5` | `texture` | Chunk table, ETPF index; mip 0 to DDS/PNG: BCPack BC1 (fitted tables, endpoints approximate), BCPack BC3/BC4/BC5/BC7 and plain BC3/BC4/BC5/BC7 (tile modes 0-3) |
| XML, DDS, PNG, GFX, RBF | various | `list` | Detected only |

## Architecture

```
src/
  lib.rs            library root (`forge9` crate)
  main.rs           CLI: one subcommand per cmd_* function
  rpf9/             the archive container
    header.rs       fixed header
    crypto.rs       T-table cipher, key schedules, CBC
    entry.rs        TOC records (24-byte and compact layouts), name hash column
    reader.rs       Archive::open with key discovery, entry reads, format sniffing
    writer.rs       in-place replace and full repack
    psin.rs         PSIN / PSO data
    hashes.rs       joaat and name dictionaries
  formats/          payload decoders (one file per format)
    awc.rs          AWC audio containers
    sgd5.rs         SGD5 shader databases
    xbch.rs         DXBC → XDAT → XBCH unwrap (zlib → FSSM)
    fssm.rs         FSSM programs: stages, root layout, used resources
    rdna2.rs        RDNA2 (GFX10.3) instruction-length decoder and classifier
    ps09.rs         PS09 driver pipeline cache
    bink.rs         Bink 2 video headers
    drawable.rs     drawable geometry, glTF/OBJ writers
    geometry_check.rs  reference buffer dumps, captured position checks
    resource.rs     0DRP paged resources
    texture.rs      RDS5 texture stores (via bcpack-rs)
  modding/          mod packs
    manifest.rs     mod.toml format
    patch.rs        patch ops, XML value locator, `mod make`
    apply.rs        verify and apply against an archive
```

Adding a format is one file in `src/formats/` plus a subcommand; see [CONTRIBUTING.md](CONTRIBUTING.md).

## Crypto data

The decryption tables and key schedules must be provided externally in a `data/` directory (or set via `--data-dir` / `FORGE9_DATA` env var):

- `tables.bin` (20,480 bytes): 16 permuted S-boxes + 16 T-tables
- `keys.bin` (39,060 bytes): 9 expanded key schedules

These files are **not included** in this repository. They must be extracted from the target runtime by the user from their own legally obtained copy.

## Legal

This tool does not include, distribute or link to any game files, encryption keys, executables or copyrighted content. All crypto material and archive files must come from the user's own legally purchased copy of the game. This project exists for interoperability research and modding purposes, in the same spirit as tools like Wine, DXVK and Proton.

## Building

```bash
cargo build --release
```

## Roadmap

- [x] Archive repacking (encrypt + rebuild TOC)
- [x] Nested RPF9 recursive inspection
- [x] Audio, shader and video container parsing
- [x] Mod pack manifests
- [ ] Name recovery from larger hash dictionaries
- [ ] PSIN binary-XML editor (read-modify-write values)
- [x] BCPack BC1 texture decoding
- [x] BCPack BC3/BC5/BC7 texture decoding (mip 0, array slices)
- [ ] Full mip chains and cube maps (tile modes 9/10)
- [x] Model export (glTF/OBJ) from standalone drawables and drawable dictionaries (checked on a real dictionary)
- [ ] Pipeline cache (PS09) parsing
- [ ] Streaming world analysis tools

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and the
[good first issues](https://github.com/nineforge0/forge9/labels/good%20first%20issue).

## Credits

BCPack decoding (BC1, and BC3/BC5/BC7 from its `bc5-bc7` branch, pinned by commit), RDS5 parsing
and BC1 Gfx10 detiling come from
[bcpack-rs](https://github.com/Ahmet-Dedeler/bcpack-rs) by Ahmet Dedeler (MIT OR Apache-2.0), whose
rANS tables derive from Microsoft's MIT-licensed [silicon-rans](https://github.com/microsoft/silicon-rans).
BC3/BC5/BC7 pixels for PNG output come from [bcdec_rs](https://crates.io/crates/bcdec_rs).

## License

MIT
