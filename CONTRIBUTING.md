# Contributing to forge9

Thanks for helping. forge9 grows one format at a time, and small focused PRs are the fastest way in. Issues
labelled [`good first issue`](https://github.com/nineforge0/forge9/labels/good%20first%20issue) are scoped for a
first contribution.

## Building

```bash
cargo build --release
./target/release/forge9 --help
```

Rust edition 2024 (stable toolchain). No system dependencies.

## Crypto material and game files

Nothing copyrighted lives in this repository, and nothing copyrighted may be added to it: no archives, no
extracted payloads, no executables, no key tables. Commands that open an archive need `tables.bin` and `keys.bin`
in `data/` (or `--data-dir` / `FORGE9_DATA`), taken by you from your own legally obtained copy. `.gitignore`
already excludes `bin/`, `data/*.bin`, `*.rpf`, `*.awc`, `*.bk2`; double-check `git status` before you commit.

## Running tests

```bash
cargo test --release
```

Unit tests must not need game files. Build the bytes you need inside the test (see `src/formats/awc.rs` or
`src/rpf9/entry.rs` for the pattern). A test that needs real data or crypto files must skip cleanly when they are
missing, as `src/rpf9/crypto.rs` does.

## Layout

```
src/
  lib.rs           library root: everything below is usable as the `forge9` crate
  main.rs          CLI (clap); one cmd_* function per subcommand
  rpf9/            the archive itself: header, TOC crypto, entries, reader, writer, PSIN, hashes
  formats/         decoders for payloads stored inside archives
  modding/         mod.toml manifests and applying them
```

## Adding a format decoder

1. Create `src/formats/<name>.rs` and add `pub mod <name>;` to `src/formats/mod.rs`.
2. Start the file with a `//!` comment describing the layout you parse: magic, field offsets, sizes. That comment
   is the format's documentation, so be precise and note what is still unknown.
3. Expose plain structs with `pub` fields and a `pub fn parse(data: &[u8]) -> anyhow::Result<T>`. Check the magic
   first and return an error (never panic) on truncated or implausible input. The `u16_at` / `u32_at` / `u64_at`
   helpers in `formats/mod.rs` return `None` instead of panicking on short input.
4. Add one `#[cfg(test)]` test that builds a minimal valid blob in memory, parses it, and checks that a broken
   one is refused.
5. Wire a subcommand in `src/main.rs`: add a variant to `Command`, a match arm that calls
   `load_entry(&archive, &crypto, &index)` (which also handles nested paths like `5/3`) and a small `cmd_<name>`
   that prints the result.
6. If the format has a recognisable magic, add it to `sniff_ext` in `src/rpf9/reader.rs` so `list`, `tree` and
   `search` show it.
7. Add the format to the "Supported formats" table in `README.md`.

## Code style

- Match the surrounding code: naming, comment density, compact `if … { return …; }` guards, `anyhow` errors.
- Run `cargo build --release` and keep it warning-free. `cargo fmt` is not enforced; don't reformat code you
  aren't changing.
- No unrelated changes in a PR: no drive-by renames, reformatting or dependency bumps.
- Prefer the standard library and the crates already in `Cargo.toml`. New dependencies need a reason in the PR.
- Document reverse-engineered facts where they're used, with how you confirmed them (e.g. "all 1,699 records in
  shaders.rpf #2 inflate to DXBC").

## Pull requests

- One topic per PR. Link the issue it closes.
- Describe what you tested: which command, against which kind of file, and what it printed. Don't attach or
  paste game data; describe it (sizes, counts, field values).
- `cargo build --release` and `cargo test --release` must pass.
- Keep commits focused with messages that say what changed and why.

## Reporting format findings

Not ready to write code? Open an issue with the layout you've worked out: offsets, types, sample values and how
many files you checked it against. That is often the hardest part of a decoder.

For a graphics regression, report one reproducible draw or a short scene. Include
the runtime commit, adapter and driver, cache state, expected image, and first
observed failure. A small synthetic fixture, timing summary, or a description of
the failed binding gives others a way to reproduce it. Keep original archives,
shader binaries, captured buffers, and logs with private paths local.
