# Repack Validation: an independent reader accepts forge9's output

What forge9's `repack` output has been checked against, and what is still
unproven. Short version: an RPF9 reader that forge9 did not write parses
forge9's repacked archives and reads every entry back correctly. Unchanged
archives repack **byte-identically**. **No repacked archive has been shown to
load in the game.** One published analysis says the game RSA-verifies RPF9
headers, and forge9 does not re-sign them (see Limits).

## Reader used

- **`zeozeozeo/rpf9` v0.2.0** (commit `a201c47`), used through its public API
  (`Rpf::parse`, `entries`, `read`) by a small external harness. It has a
  separate parser and decrypt path, and it inflates zlib entries and stitches
  64 KiB pages across physically adjacent entries, which forge9's reader does
  not do. It has no encryptor, so a TOC it accepts was encrypted by forge9 and
  decrypted by different code.
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
  content.
- In `script.rpf`, nested child #1 moved from `0xc4000` to `0xc3b90`, which
  lost its 4 KiB alignment.

The writer now keeps the original physical order and each entry's original
alignment (capped at 4 KiB), and it copies the original header bytes.

## Limits: what this does not show

- **Game acceptance.** The game's loader is another RPF9 reader, but its rules
  are stricter than anything tested here.
- **Header signatures.** `rpf9-explorer`'s format notes
  (`docs/rpf9-format-findings.md`) report RSA-2048 PKCS#1 v1.5 signatures,
  using double SHA-256 over selected header ranges after the first transform,
  on all 22 standalone and 4,351 child headers. They say that changing a
  covered byte fails verification. forge9 writes new TOC ciphertext whenever
  an offset changes and never re-signs. If those ranges cover the changed
  bytes and the game enforces the check, a size-changing repack will be
  rejected. We have not checked this claim. An identity repack is unaffected
  because it is byte-identical.
- **Index fields that forge9 does not model.** The same notes describe a
  three-stage header transform, six record tables, per-chunk codec and length
  records, declared decoded lengths, and counter-mode encrypted members.
  forge9 rewrites only the 24-byte record's offset word. Declared lengths and
  codec flags are left at their original values. Changing an entry's decoded
  length (for example, appending an XML comment) or storing a zlib entry raw
  may therefore contradict the game's index even though both heuristic
  readers accept it. Replacements should keep the original codec: compress
  the replacement for a zlib entry.
- **Raw entries have no explicit length** in the indexed record layout. A raw
  replacement reads back with zero padding up to the next 16-byte boundary.

## Conclusion

The evidence supports this claim: *forge9's repacked archives are
structurally valid RPF9 files that a separate RPF9 implementation accepts and
reads back correctly, and unchanged archives round-trip byte-identically.* It
does **not** support removing the disclaimer "repacked archives are not shown
running in the game". The open header-signature question alone is a reason to
keep it until a repacked archive is shown to load in the game.
