//! joaat hash dictionary: embedded known names plus optional external file.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const KNOWN: &[(u32, &str)] = &[
    // Script DOM
    (0xde396fe2, "root_document"), (0x85b4c631, "attribute"), (0xaeb623d8, "vector3"),
    (0x01393259, "tree_node"), (0x749dbb9e, "attributes"), (0x2260b8be, "dataNodes"),
    (0xe6e17f14, "name"), (0xd89231bd, "parentIndex"), (0x01296aed, "childIndex"),
    (0x881bf649, "siblingIndex"), (0x79661755, "attributeCount"),
    (0x817ed7a4, "valueHash"), (0x5c2411e5, "strings"), (0xd54b8545, "vectors"),
    (0x4880f687, "u64_hashes"), (0x649ecf8e, "attr_type"),
    // Bones
    (0x00000000, "mover"), (0x6686685c, "skel_root"), (0x8133de19, "skel_pelvis"),
    (0x9e0fb7af, "skel_spine_root"), (0x41e5d6a1, "skel_spine0"),
    (0xfb61fbb5, "skel_l_clavicle"), (0xb13045dc, "skel_r_clavicle"),
    (0x3314f242, "skel_l_thigh"), (0x364e7f26, "skel_r_thigh"),
    // Tracks
    (0x98e1750c, "bonerotation"), (0xf791a95f, "bonetranslation"),
    // World
    (0xd3593fa6, "CMapData"), (0xce501483, "CEntityDef"), (0x82d6fc83, "CBaseArchetypeDef"),
    // Packs
    (0xac9e7e35, "common"), (0x5eb5e655, "data"), (0x938b83b2, "shaders"),
    (0xa26a72dd, "textures"), (0xe7a8cdf1, "script"), (0xb80c6ae8, "audio"),
    (0x20ef298e, "movies"), (0x0e5d52e0, "packs"), (0x275b8a30, "levels"),
    (0x0a802776, "anim"), (0x2eb2ee6e, "navmeshes"),
    // Shader texture parameters
    (0xc8e8c282, "diffusetex"), (0x1e77bf4c, "bumptex"), (0xc6f46e79, "speculartex"),
    (0x739e3d0d, "speculartex2"), (0x2c8d7182, "tintpalettetex"), (0x8e0d63ee, "emissivetex"),
    (0x8adb8480, "materialid"), (0x5c74c225, "givemechecker"),
];

pub fn joaat(name: &str) -> u32 {
    let mut h: u32 = 0;
    for b in name.bytes() {
        h = h.wrapping_add(b as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h.wrapping_add(h << 15)
}

pub struct Dict(HashMap<u32, String>);

impl Dict {
    /// Embedded names, extended (and overridden) by `path` if given.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut map: HashMap<u32, String> = KNOWN.iter().map(|&(h, n)| (h, n.to_string())).collect();
        if let Some(path) = path {
            let text = std::fs::read_to_string(path).with_context(|| format!("reading dict {}", path.display()))?;
            map.extend(parse_lines(&text));
        }
        Ok(Self(map))
    }

    pub fn name(&self, hash: u32) -> Option<&str> {
        self.0.get(&hash).map(String::as_str)
    }

    /// `0xde396fe2 (root_document)`, or just the hex when unknown.
    pub fn fmt(&self, hash: u32) -> String {
        match self.name(hash) {
            Some(n) => format!("{hash:#010x} ({n})"),
            None => format!("{hash:#010x}"),
        }
    }
}

/// `hash name` per line, hash in hex (optional 0x); blank and `#` lines ignored.
fn parse_lines(text: &str) -> impl Iterator<Item = (u32, String)> + '_ {
    text.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { return None; }
        let (hash, name) = line.split_once(char::is_whitespace)?;
        let hash = u32::from_str_radix(hash.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()?;
        Some((hash, name.trim().to_string()))
    })
}

/// Merges `entries` into `existing` (`hexhash name` lines), keeping existing names on conflict,
/// deduplicating by hash and sorting ascending by hash. Returns `(formatted, added, total)`.
pub fn merge_dict(existing: &str, entries: impl IntoIterator<Item = (u32, String)>) -> (String, usize, usize) {
    let mut map: BTreeMap<u32, String> = parse_lines(existing).collect();
    let mut added = 0;
    for (hash, name) in entries {
        let name = name.trim();
        if name.is_empty() || name.contains(['\n', '\r']) { continue; }
        map.entry(hash).or_insert_with(|| { added += 1; name.to_string() });
    }
    let mut out = String::new();
    for (hash, name) in &map {
        use std::fmt::Write;
        let _ = writeln!(out, "{hash:#010x} {name}");
    }
    (out, added, map.len())
}

/// Merges `entries` into `path` (creating it if absent) as sorted, deduplicated `hexhash name`
/// lines. Returns `(added, total)`.
pub fn write_dict(path: &Path, entries: impl IntoIterator<Item = (u32, String)>) -> Result<(usize, usize)> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading dict {}", path.display())),
    };
    let (formatted, added, total) = merge_dict(&existing, entries);
    std::fs::write(path, formatted).with_context(|| format!("writing dict {}", path.display()))?;
    Ok((added, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joaat_and_dict_lines() {
        assert_eq!(joaat(""), 0);
        assert_eq!(joaat("skel_root"), 0x6686685c);
        assert_eq!(joaat("CMapData"), 0xd3593fa6);
        for &(h, n) in KNOWN.iter().skip_while(|(_, n)| *n != "diffusetex") { assert_eq!(joaat(n), h, "{n}"); }
        let parsed: Vec<_> = parse_lines("# c\n0xdeadbeef foo bar\nzz bad\n12 x\n").collect();
        assert_eq!(parsed, vec![(0xdeadbeef, "foo bar".to_string()), (0x12, "x".to_string())]);
        let d = Dict::load(None).unwrap();
        assert_eq!(d.fmt(0xde396fe2), "0xde396fe2 (root_document)");
        assert_eq!(d.fmt(0x12345678), "0x12345678");
    }

    #[test]
    fn merge_dict_dedups_sorts_and_preserves_existing() {
        let existing = "# saved\n0x00000020 kept_name\n";
        let (text, added, total) = merge_dict(existing, [
            (0x00000020, "override_attempt".to_string()),
            (0x00000030, "later".to_string()),
            (0x00000010, "earlier".to_string()),
            (0x00000010, "duplicate".to_string()),
        ]);
        assert_eq!((added, total), (2, 3));
        assert_eq!(text, "0x00000010 earlier\n0x00000020 kept_name\n0x00000030 later\n");
    }
}
