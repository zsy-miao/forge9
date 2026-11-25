# Repack Validation: an independent reader accepts forge9's output

What forge9's `repack` output has been checked against, and what is still
unproven. Short version: an RPF9 reader that forge9 did not write parses
forge9's repacked archives and reads every entry back correctly. Unchanged
archives repack **byte-identically**. **No repacked archive has been shown to
load in the game.** The headers carry RSA signatures, but a static read of the
game executable finds no code that enforces them (or any hash) on the RPF9 load
path (see Game loader), which weakens the main structural objection but does not
prove a live load.

## Reader used

- **`zeozeozeo/rpf9` v0.2.0** (commit `a201c47`), used through its public API
  (`Rpf::parse`, `entries`, `read`) by a small external harness. It has a
  separate parser and decrypt path. It inflates the first zlib stream of an
  entry and, when that stream decodes to a multiple of 64 KiB, appends the
  physically next entry ("page stitching"). It has no encryptor, so a TOC it
  accepts was encrypted by forge9 and decrypted by different code.
- **Its multi-part reads are wrong.** The 24-byte record declares each entry's
  stored and decoded size, and a resource over 64 KiB keeps all its zlib parts
  inside its own stored span ([rpf9_format.md](rpf9_format.md), "Multi-part
  resources"). Stitching therefore returns the first part followed by an
  unrelated neighbour for each of the 84 multi-part entries of `common.rpf`.
  The comparisons below still hold for the other entries, and for those 84
  they show only that both sides read the same bytes the same wrong way.
- It is **not fully independent**. forge9's cipher and TOC record heuristic
  were ported from this crate, and both use the same `keys.bin`/`tables.bin`.
  Agreement shows that forge9's writer and this reader's model of the format
  are consistent with each other. It does not show that the model is complete.
- Other readers checked: `rpf9-explorer` (C#) hands index decoding to an
  external bridge and has no built-in TOC reader. The `xcl` and `rage9-re`
  readers scan for records without a TOC, and `tombox-rpf` only parses
  headers. None of them could test the TOC independently.

## Results

Harness checks for each repacked archive: the raw 64 header bytes match, the
key and the graph magic are recovered again, the entry count matches, offsets
are 16-aligned, stay in bounds and do not overlap, only the offset word of each
record changes, and every entry decodes to the original bytes or to the
replacement's bytes.

| Test | Archive | Result |
|---|---|---|
| Identity repack | `common.rpf` (418 entries) | **Byte-identical**; 418/418 entries read the same |
| Identity repack | `appdata0.rpf` (2) | Byte-identical |
| Identity repack | `script.rpf` (2 nested RPF9s) | Byte-identical |
| Identity repack | `shaders.rpf` (3 entries, 411 MB) | Byte-identical |
| XML #6 + comment, zlib-compressed | `common.rpf` | Upstream reader inflates the modified XML exactly; 417 others identical |
| XML #6 + comment, stored raw | `common.rpf` | Read back as the XML plus zero padding to 16 bytes; 417 others identical |
| Size change: #1 8192→20000, #5 1904→26, #417 → 20000 | `common.rpf` | All 418 entries correct at their new offsets |
| Size change: #0 → 20000 | `appdata0.rpf` | Both entries correct |
| Replace nested child #1 | `script.rpf` | Child #0 is byte-identical and parses as RPF9 in the upstream reader |

The byte-identical identity repacks show that forge9's TOC encryption exactly
inverts the decryption. It reproduces the original ciphertext with key slot 8
on `common.rpf`, slot 4 on `appdata0.rpf` and `script.rpf`, and slot 5 on
`shaders.rpf`. forge9's writer copies the header unchanged.

## Writer bug found and fixed

Before the fix, `repack` laid payloads out in TOC-index order, aligned the
data start to 4 KiB, and packed entries at 16-byte alignment. As a result:

- An identity repack of `common.rpf` was not byte-identical (6,644,880 vs
  6,642,192 bytes).
- For **60 multi-page zlib entries**, the upstream reader's page stitching
  followed a different neighbour, so those resources decoded to different
  content. (Stitching is itself wrong, see above; the old order was still a
  real bug, because it broke byte identity and alignment.)
- In `script.rpf`, nested child #1 moved from `0xc4000` to `0xc3b90`, which
  lost its 4 KiB alignment.

The writer now keeps the original physical order and each entry's original
alignment (capped at 4 KiB), and it copies the original header bytes.

## Game loader: no signature enforcement found

The RPF9 headers do carry signatures: `rpf9-explorer`'s format notes report
RSA-2048 PKCS#1 v1.5 signatures over selected header ranges, verified offline
across 22 standalone and 4,351 child headers. The open question for modding is
whether the game *enforces* them at load. A static analysis of the game
executable (PE x86-64, ImageBase `0x140000000`; the game was not run) traced the
code that reads the RPF9 magic and header. The loader validates only the 4-byte
magic `9FPR` and then allocates and stream-reads the header and TOC. No
signature or hash check was found on that path. Evidence:

- The only `bcrypt` imports are `BCryptGenRandom/Open/Close` (an RNG). There is
  no `BCryptCreateHash`/`HashData`/`FinishHash`/`VerifySignature`, no NCrypt and
  no CryptoAPI, and none of those names appear as strings (so they are not
  resolved at runtime either).
- SHA-256, SHA-512, MD5 and CRC32 constant tables exist in the binary, but every
  xref to them lands in the TLS/X.509 certificate library and the online
  (Xbox Live / libHttpClient) stack, far from the loader. The loader and its
  constructor reach only allocator, stream and device routines.
- The `RSA`, `Signature` and `Verify` strings belong to a PEM/X.509 library and
  to web-token verification, not to archive loading. No archive-integrity string
  exists.

So the header signatures are real, but on this executable no code was found that
checks them when an archive loads: the only gate observed is the TOC block
cipher, which forge9 re-encrypts exactly. The result is a static negative on a
vtable-heavy loader: not every virtual call in the mount path was resolved, so it
is strong evidence that the signatures are not enforced here, not a proof. A
runtime test (a resigned-or-not repacked archive loading in a running build) is
still needed to confirm it.

## Self-check: `--verify`

`repack --verify` and `mod apply --verify` reopen the written archive with
forge9's reader and compare it with the source by inflated payload, nested
archives included. Each replacement or patch must read back as intended and
every other entry must be byte-identical, wherever its offset moved. A raw
entry whose record stores no size may read back with extra zero padding: that
is counted, not flagged. Any other
difference is listed with its path, old and new size and first differing
offset, and the command exits nonzero. This catches a bad rebuild before a slow
in-game boot. It checks forge9 against itself, so it is no proof that the game
loads the archive.

## Limits: what this does not show

- **Game acceptance.** The game's loader is another RPF9 reader. No integrity or
  signature check was found in it (see Game loader), but a live load has still
  not been shown, and the format-correctness points below can still make an
  archive the running game rejects.
- **Index fields that forge9 does not model.** `rpf9-explorer`'s format notes
  (`docs/rpf9-format-findings.md`) describe a
  three-stage header transform, six record tables, per-chunk codec and length
  records, declared decoded lengths, and counter-mode encrypted members.
  forge9 rewrites the 24-byte record's offset, stored size and decoded size
  (the record layout is in [rpf9_format.md](rpf9_format.md), "24-byte
  record"); the flags and the other columns are left at their original
  values. Storing a zlib entry raw may therefore still contradict the game's
  index. Replacements should keep the original codec: compress the
  replacement for a zlib entry.
- Before the record fix forge9 rewrote only the offset, so the size-change
  tests in the table above left stale stored and decoded sizes in the records.
  Both readers sized entries by the span to the next entry and accepted them;
  the game's loader may not.

## Conclusion

The evidence supports this claim: *forge9's repacked archives are
structurally valid RPF9 files that a separate RPF9 implementation accepts and
reads back correctly, and unchanged archives round-trip byte-identically.* The
static read of the game loader weakens the header-signature objection for this
build: the headers are signed, but no code enforcing a signature or hash was
found on the load path. It does **not** support
removing the disclaimer "repacked archives are not shown running in the game":
format-correctness fields forge9 does not model, and a live load, are still
untested.
