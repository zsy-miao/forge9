# forge9

General-purpose toolkit for RPF9 archives. Read, decrypt, inspect, extract and analyze.

RPF9 is a container format used by a well-known retail game engine. This tool handles decryption, TOC parsing, entry listing, content extraction, and format detection — a foundation for research, interoperability work and asset tooling.

## Features

- **Decrypt** archive table-of-contents using a 9-key T-table cipher with CBC mode
- **Inspect** headers, entry counts, key IDs, version info and flags
- **List** all entries with offset, size, format detection (zlib, XML, PSIN, nested RPF9, etc.)
- **Extract** entries to disk with automatic zlib chain inflation
- **Tree / search** nested RPF9 archives by magic, extension, size or content
- **Replace / repack** entries in place or at any size, re-encrypting the TOC
- **Decode payloads**: AWC audio streams, SGD5 shader databases, Bink 2 video headers, PSIN game data, paged resources
- **Textures**: list RDS5 (`.ttpf`) stores and decode BCPack BC1 to DDS or PNG (fitted endpoint tables; output is approximate, see [bcpack-rs#4](https://github.com/Ahmet-Dedeler/bcpack-rs/issues/4)), and BCPack BC3/BC5/BC7 to DDS (DX10) or PNG, including array slices (bcpack-rs `bc5-bc7` branch; checked against the stored mip pyramid on all 129 BC3/BC5/BC7 chunks we have)
- **Models**: export a drawable's LOD geometry (positions, indices, UVs when present) to glTF 2.0 (`.glb`, or `.gltf` + `.bin`) or OBJ. Checked only on a synthetic drawable so far: no local archive holds a drawable yet
- **Mod packs**: a TOML manifest of entry replacements, verified and applied in one command
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

# Texture stores (.ttpf files sit next to the archives, not inside them)
forge9 texture store.ttpf                       # list: size, format, mips, tile mode, hash
forge9 texture store.ttpf --extract 12 -o a.dds # BC1/BC3/BC5/BC7 mip 0 as DDS
forge9 texture store.ttpf --decode 12 -o a.png  # mip 0 as PNG (BC5 as red/green)
forge9 texture store.ttpf --decode 300 --slice 5 # one slice of an array texture
forge9 bcpack-analyze store.ttpf --codec bc7     # BC3/BC5/BC7 entropy stage: fragments, lane stats
forge9 bcpack-analyze store.ttpf 119 -o l.bin    # one chunk; lane symbol streams as u16 LE

# joaat hash of a name
forge9 hash player_one
```

### Mod packs

A mod is a `mod.toml` listing entry replacements. Paths are relative to the manifest.

```toml
[mod]
name = "example-mod"
version = "0.1.0"
description = "Example mod"
target_archive = "common.rpf"

[[replace]]
entry = 6
file = "mods/my_config.xml"

[[replace]]
entry = 12
file = "mods/modified_data.bin"
```

```bash
forge9 mod init                          # write a template mod.toml
forge9 mod verify mod.toml               # files exist, indices in range, sizes sane
forge9 mod apply mod.toml -o modded.rpf  # repack the target with every replacement
```

`verify` and `apply` take `--archive path.rpf` to override `target_archive`. Replacement bytes are stored as
given (uncompressed); readers detect zlib per entry, so plain payloads read back as-is.

## Supported formats

| Format | Magic | Command | Status |
|---|---|---|---|
| RPF9 archive | `9FPR` | `info`, `list`, `tree`, `extract`, `repack` | Read and write; TOC decrypt/encrypt |
| zlib chains | `78 xx` | (automatic) | Inflated on read |
| PSIN / PSO game data | `PSIN` | `psin` | Block table, struct schemas |
| AWC audio | `ADAT` | `awc` | Stream table, chunks, formats (Opus, XMA2, PCM); no decode |
| SGD5 shader database | `SGD5` | `shaders` | Record names, stages, tags, sizes; DXBC payload offsets |
| Xbox shader program | `XBCH` → `FSSM` | `fssm` | XDAT section 0x0D inflated; stages, root layout, used resources |
| RDNA2 machine code | (FSSM code range) | `isa`, `disasm` | Length decode by control flow, encoding families, opcodes, wave64-sensitive ops, EXEC-uniformity (DXIL-fold headroom); full disassembly with operands (matches llvm-mc on a 3.47M-instruction sample); see [docs/wave64_instruction_usage.md](docs/wave64_instruction_usage.md), [docs/rdna2_opcode_coverage.md](docs/rdna2_opcode_coverage.md), [docs/exec_uniformity.md](docs/exec_uniformity.md) |
| Pipeline cache | `PS 00 09` | `pipelines` | Header, graphics/compute PSO records, shader slots, metadata and code ranges |
| Bink 2 video | `KB2?` | `bink` | Header: size, frames, dimensions, frame rate |
| Paged resource | `0DRP` | `resource` | Kind detection, virtual/physical pointer resolution |
| Drawable (model) | (paged, no magic) | `model` | Standalone drawables: LOD groups → models → geometries; positions in formats 66 (quantized) and 99 (float3), float2/snorm16 UVs, clustered meshlet indices; to glTF/OBJ. Normals, skinning, materials, dictionaries and fragments not yet decoded. Checked only on a synthetic drawable |
| RDS5 texture store | `RDS5` | `texture` | Chunk table, ETPF index; mip 0 to DDS/PNG: BCPack BC1 (fitted tables — endpoints approximate), BCPack BC3/BC5/BC7 and plain BC3/BC5/BC7 (tile modes 0-3) |
| XML, DDS, PNG, GFX, RBF | various | `list` | Detected only |

## Architecture

```
src/
  lib.rs            library root (`forge9` crate)
  main.rs           CLI: one subcommand per cmd_* function
  rpf9/             the archive container
    header.rs       fixed header
    crypto.rs       T-table cipher, key schedules, CBC
    entry.rs        TOC records (indexed and stored layouts)
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
    resource.rs     0DRP paged resources
    texture.rs      RDS5 texture stores (via bcpack-rs)
  modding/          mod packs
    manifest.rs     mod.toml format
    apply.rs        verify and apply against an archive
```

Adding a format is one file in `src/formats/` plus a subcommand; see [CONTRIBUTING.md](CONTRIBUTING.md).

## Crypto data

The decryption tables and key schedules must be provided externally in a `data/` directory (or set via `--data-dir` / `FORGE9_DATA` env var):

- `tables.bin` — 20,480 bytes: 16 permuted S-boxes + 16 T-tables
- `keys.bin` — 39,060 bytes: 9 expanded key schedules

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
- [x] Model export (glTF/OBJ) from standalone drawables (needs checking on real models)
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
