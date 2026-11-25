# Mod Pack Format v2

A mod is a `mod.toml` manifest of **patches** that `forge9 mod apply` runs
against the user's own archive. A published mod must not contain game files or
game bytes. It carries entry paths, hashes of the original payloads, and edit
operations. Whole-file replacement is allowed only for content the mod author
created.

## Manifest

```toml
[mod]
name = "example-mod"
version = "0.1.0"
description = "Example mod"
target_archive = "common.rpf"   # relative to this manifest; `--archive` overrides it

[[patch]]
path = "5/3"
base_sha256 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"

[[patch.op]]
type = "xml"
path = "Root/Items/Item[2]/value"
value = "64"
```

### `[[patch]]`

| Key | Required | Meaning |
|---|---|---|
| `path` | yes | Entry path as `forge9 list --recursive` prints it: `5` is top-level entry 5, `5/3` is entry 3 of the nested archive in entry 5. A leading `#` is accepted. |
| `base_sha256` | no, strongly recommended | SHA-256 (hex) of the entry's original **inflated** payload, as `forge9 extract` writes it (`shasum -a 256 file`). Apply refuses on mismatch: the archive is a different game version, or already modded. |
| `file` | no | Whole new payload, relative to the manifest. Only for content the mod author created. The ops, if any, apply to it. |
| `op` | no | Operations, applied in order, each to the result of the previous one. |

A patch with only `path` and `base_sha256` is a pure version guard. Each entry
may be patched once per manifest. Patching an archive entry and an entry inside
it in the same manifest is refused.

### Operations

Every operation states what it expects to find and fails when that does not hold, so
a mod either applies exactly as written or not at all.

**text**: exact find and replace on the payload bytes (UTF-8 in the manifest).

```toml
[[patch.op]]
type = "text"
find = "<Enabled>false</Enabled>"
replace = "<Enabled>true</Enabled>"
count = 1          # required number of non-overlapping matches (default 1); all are replaced
```

**xml**: set an element's text or one attribute's value, leaving every other byte as it is.

```toml
[[patch.op]]
type = "xml"
path = "CVehiclePoints/fNavMaxHeightChange"   # element names from the root down
attr = "value"     # omit to set the element's text
value = "0.400000"
expect = "0.350000"   # optional: required current value
```

- A step `Item[2]` selects the third `Item` child (0-based, counting same-name
  siblings only). A step without `[i]` matches any of them. The whole path must
  match exactly one element, or the op fails.
- Text can be set only on an element with an end tag and no child elements.
  Attributes must already exist. Ops do not add or remove elements.
- `value` and `expect` are raw XML text: write `&amp;` for `&`, `&lt;` for `<`.
- The parser is a small scanner (comments, CDATA, processing instructions,
  quoted `>` in attributes), not a validating XML parser.

**bytes**: binary patch at a fixed offset of the inflated payload.

```toml
[[patch.op]]
type = "bytes"
offset = 0x40            # TOML integer; hex allowed
expect = "0000803f"      # original bytes (hex, whitespace ignored); must match
data = "00000040"        # replacement; may differ in length
```

### v1 manifests

v1 `[[replace]]` tables still load: `entry = 6, file = "f.xml"` is read as
`[[patch]] path = "6", file = "f.xml"` without a guard. New mods should use
`[[patch]]` with `base_sha256`.

## Commands

```bash
forge9 mod init                                  # template mod.toml
forge9 mod verify mod.toml --archive common.rpf  # every guard and op, nothing written
forge9 mod apply mod.toml --archive common.rpf -o modded.rpf
forge9 diff common.rpf modded.rpf                # which entries changed
forge9 mod make --path 9 original.xml edited.xml >> mod.toml
```

`verify` runs every patch in memory, prints each entry's size before and after with
one line per op, and repacks without writing. Exit status is non-zero if any check fails.

`make` writes a `[[patch]]` table that turns an extracted payload into an
edited copy of it, with `base_sha256` set from the original. When only element
text and attribute values differ, it emits one `xml` op per changed value (with
`expect`). Otherwise it emits a single `text` op (or `bytes` for binary payloads)
over the changed span, widened by whole lines until `find` is unique. It prints
how many original bytes the ops carry to stderr. Check that number before publishing:
far-apart edits in one non-XML file become one large span.

## Storage

A patched payload that replaces a zlib entry is stored as zlib (64 KiB chunks).
Other payloads are stored raw. Nested archives are repacked from the inside
out. Each record gets the new stored and decoded size, so a raw payload reads
back at its exact length. `verify` and `apply` warn when a patched entry does
not read back exactly.
Paged resources are not re-split on their original block boundaries.

## Example

[`examples/xml-tweak`](../examples/xml-tweak) changes one attribute value in
one XML entry. Checked on a real `common.rpf`: `verify` passes, `apply` writes a
same-size archive, `forge9 diff` reports exactly that entry as changed, the new
value reads back, and verifying or applying again against the modded archive
fails on `base_sha256`.
