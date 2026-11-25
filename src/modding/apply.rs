//! Verifying and applying a mod manifest against its target archive.

use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use crate::modding::manifest::{Manifest, Patch};
use crate::modding::patch;
use crate::rpf9::crypto::CryptoContext;
use crate::rpf9::reader::Archive;

/// Rebuilds `archive` (default: the manifest's target) with every patch and writes `output`.
/// Returns each patched entry path and its new content.
pub fn apply(m: &Manifest, archive: Option<&Path>, crypto: &CryptoContext, output: &Path) -> Result<BTreeMap<Vec<usize>, Vec<u8>>> {
    let target = archive.unwrap_or(&m.info.target_archive);
    let raw = std::fs::read(target).with_context(|| format!("reading {}", target.display()))?;
    let mut changes = BTreeMap::new();
    for p in &m.patch {
        let (_, new) = run(p, &raw, crypto).with_context(|| format!("patch {}", p.path))?;
        ensure!(changes.insert(indices(&p.path)?, new).is_none(), "entry {} patched twice", p.path);
    }
    let out = rebuild_checked(&raw, crypto, &changes)?;
    std::fs::write(output, &out).with_context(|| format!("writing {}", output.display()))?;
    println!("applied {} patch(es) → {} ({} bytes)", changes.len(), output.display(), out.len());
    Ok(changes)
}

/// Runs every patch in memory, without writing, and prints what each changes.
/// Without the archive or crypto only replacement files are checked. Returns the number of problems.
pub fn verify(m: &Manifest, archive: Option<&Path>, crypto: Option<&CryptoContext>) -> Result<usize> {
    let target = archive.unwrap_or(&m.info.target_archive);
    println!("{} {} → {}", m.info.name, m.info.version, target.display());
    let raw = std::fs::read(target).ok();
    let (Some(raw), Some(crypto)) = (&raw, crypto) else {
        println!("  (archive or crypto unavailable: checking files only)");
        let missing: Vec<_> = m.patch.iter().filter_map(|p| p.file.as_ref().filter(|f| !f.is_file()).map(|f| (p, f))).collect();
        for (p, f) in &missing { println!("  ✗ {:<10} {}: file missing", p.path, f.display()); }
        return Ok(missing.len());
    };
    let mut problems = 0;
    let mut changes = BTreeMap::new();
    for p in &m.patch {
        let checked = indices(&p.path).and_then(|k| {
            ensure!(!changes.contains_key(&k), "entry patched twice");
            Ok((k, run(p, raw, crypto)?))
        });
        match checked {
            Ok((k, (old, new))) => {
                let guard = if p.base_sha256.is_some() { "base_sha256 ok" } else { "no base_sha256 guard" };
                let file = p.file.as_ref().map_or(String::new(), |f| format!(", file {}", f.display()));
                println!("  ✓ {:<10} {guard}, {} -> {} bytes{file}", p.path, old.len(), new.len());
                for op in &p.ops { println!("      {}", patch::describe(op)); }
                changes.insert(k, new);
            }
            Err(e) => { problems += 1; println!("  ✗ {:<10} {e:#}", p.path); }
        }
    }
    if problems == 0 { rebuild_checked(raw, crypto, &changes).context("repacking")?; }
    Ok(problems)
}

/// The entry's original payload and its patched replacement.
fn run(p: &Patch, raw: &[u8], crypto: &CryptoContext) -> Result<(Vec<u8>, Vec<u8>)> {
    let base = read_path(raw, crypto, &indices(&p.path)?)?;
    if let Some(want) = &p.base_sha256 {
        let have = patch::sha256_hex(&base);
        ensure!(have.eq_ignore_ascii_case(want.trim()), "base_sha256 mismatch: entry is {have}; wrong game version, or already modded");
    }
    let mut data = match &p.file {
        Some(f) => std::fs::read(f).with_context(|| format!("reading {}", f.display()))?,
        None => base.clone(),
    };
    for op in &p.ops { data = patch::apply(op, &data)?; }
    ensure!(!data.is_empty(), "patched payload is empty");
    Ok((base, data))
}

/// `5/3` (or `#5/3`) as entry indices.
fn indices(path: &str) -> Result<Vec<usize>> {
    let v = path.split(['/', '#']).filter(|p| !p.is_empty())
        .map(|p| p.parse().with_context(|| format!("bad entry index {p:?} in path {path:?}"))).collect::<Result<Vec<usize>>>()?;
    ensure!(!v.is_empty(), "empty entry path");
    Ok(v)
}

fn read_path(raw: &[u8], crypto: &CryptoContext, path: &[usize]) -> Result<Vec<u8>> {
    let a = Archive::open(raw, crypto).context("opening archive")?;
    let e = a.entries().get(path[0]).with_context(|| format!("entry {} out of range (archive has {})", path[0], a.entries().len()))?;
    let bytes = a.read_entry(e)?;
    if path.len() == 1 { Ok(bytes) } else { read_path(&bytes, crypto, &path[1..]) }
}

/// `rebuild`, warning about every change that does not read back exactly.
fn rebuild_checked(raw: &[u8], crypto: &CryptoContext, changes: &BTreeMap<Vec<usize>, Vec<u8>>) -> Result<Vec<u8>> {
    let out = rebuild(raw, crypto, changes)?;
    for (path, data) in changes {
        let back = read_path(&out, crypto, path)?;
        if back != *data {
            let path = path.iter().map(usize::to_string).collect::<Vec<_>>().join("/");
            println!("  ⚠ {path} reads back as {} bytes, not {}: its record stores no size, so a raw payload is zero padded", back.len(), data.len());
        }
    }
    Ok(out)
}

/// `raw` with each `path -> payload` replaced, repacking nested archives on the way up.
/// A payload replacing a zlib entry is stored as zlib, which also keeps its length exact.
fn rebuild(raw: &[u8], crypto: &CryptoContext, changes: &BTreeMap<Vec<usize>, Vec<u8>>) -> Result<Vec<u8>> {
    let a = Archive::open(raw, crypto)?;
    let mut direct = HashMap::new();
    let mut deeper: BTreeMap<usize, BTreeMap<Vec<usize>, Vec<u8>>> = BTreeMap::new();
    for (path, data) in changes {
        match path[..] {
            [i] => { direct.insert(i, data.clone()); }
            _ => { deeper.entry(path[0]).or_default().insert(path[1..].to_vec(), data.clone()); }
        }
    }
    for (i, sub) in deeper {
        ensure!(!direct.contains_key(&i), "entry {i} is replaced and also patched inside");
        let nested = read_path(raw, crypto, &[i])?;
        direct.insert(i, rebuild(&nested, crypto, &sub)?);
    }
    for (i, data) in direct.iter_mut() {
        if a.entries().get(*i).is_some_and(|e| e.is_zlib(raw)) { *data = crate::rpf9::writer::zlib_chain(data)?; }
    }
    crate::rpf9::writer::repack_bytes(raw, crypto, &direct)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modding::manifest::Op;
    use crate::rpf9::{crypto::TAG_IV, header::HEADER_SIZE};

    /// Same stand-in crypto and archive layout as the tests in main.rs.
    fn crypto() -> CryptoContext {
        let mut tables: Vec<u8> = (0..16).flat_map(|_| 0..=255u8).collect();
        for t in 0..16u32 {
            for x in 0..256u32 { tables.extend((x << (8 * (t % 4))).to_le_bytes()); }
        }
        let mut keys = vec![0x8E, 0x9B, 0x3B, 0x4E];
        keys.extend((0..0xF0u32).map(|i| (i * 37) as u8));
        keys.extend((0..16).flat_map(|_| 0..=255u8));
        CryptoContext::from_bytes(tables, keys).unwrap()
    }

    fn archive(crypto: &CryptoContext, payloads: &[Vec<u8>]) -> Vec<u8> {
        let mut toc = vec![0u8; (payloads.len() * 24 + 16).next_multiple_of(16)];
        let mut data = Vec::new();
        for (i, p) in payloads.iter().enumerate() {
            let at = (HEADER_SIZE + toc.len() + data.len()) as u64;
            for (k, v) in [at / 16, (p.len() as u64).div_ceil(16), crate::rpf9::reader::decode(p).unwrap().len() as u64].iter().enumerate() {
                toc[i * 24 + k * 8..i * 24 + k * 8 + 8].copy_from_slice(&v.to_le_bytes());
            }
            data.extend(p);
            data.resize(data.len().next_multiple_of(16), 0);
        }
        let mut header = vec![0u8; HEADER_SIZE];
        header[0..4].copy_from_slice(b"9FPR");
        header[4..8].copy_from_slice(&((HEADER_SIZE + toc.len()) as u32).to_le_bytes());
        header[0x24..0x28].copy_from_slice(&(payloads.len() as u32).to_le_bytes());
        crypto.schedule(0).unwrap().encrypt_cbc(&mut toc, &TAG_IV);
        [header, toc, data].concat()
    }

    const XML: &[u8] = b"<?xml version=\"1.0\"?>\n<Cfg>\n  <Speed value=\"1.0\" />\n  <Name>old</Name>\n</Cfg>\n";

    /// `[zlib(xml), rpf[bin, xml]]`, archive written to a temp file.
    fn setup(name: &str) -> (CryptoContext, std::path::PathBuf) {
        let c = crypto();
        let raw = archive(&c, &[crate::rpf9::writer::zlib_chain(XML).unwrap(), archive(&c, &[vec![1, 2, 3, 4], XML.to_vec()])]);
        let path = std::env::temp_dir().join(format!("forge9-{name}-{}.rpf", std::process::id()));
        std::fs::write(&path, raw).unwrap();
        (c, path)
    }

    fn manifest(target: &Path, patches: &str) -> Manifest {
        Manifest::parse(&format!("[mod]\nname = \"t\"\nversion = \"1\"\ntarget_archive = {:?}\n{patches}", target.display().to_string())).unwrap()
    }

    #[test]
    fn applies_path_patches_at_top_level_and_nested() {
        let (c, target) = setup("apply");
        let sha = patch::sha256_hex(XML);
        let m = manifest(&target, &format!(r##"
[[patch]]
path = "0"
base_sha256 = "{sha}"
[[patch.op]]
type = "xml"
path = "Cfg/Speed"
attr = "value"
value = "2.5"
expect = "1.0"

[[patch]]
path = "1/1"
base_sha256 = "{sha}"
op = [{{ type = "text", find = "<Name>old</Name>", replace = "<Name>a longer new name</Name>" }}]

[[patch]]
path = "#1/0"
op = [{{ type = "bytes", offset = 1, expect = "0203", data = "ff" }}]
"##));
        assert_eq!(verify(&m, None, Some(&c)).unwrap(), 0);
        let out = target.with_extension("out.rpf");
        apply(&m, None, &c, &out).unwrap();
        let raw = std::fs::read(&out).unwrap();
        let read = |p: &[usize]| read_path(&raw, &c, p).unwrap();
        assert_eq!(read(&[0]), String::from_utf8(XML.to_vec()).unwrap().replace("value=\"1.0\"", "value=\"2.5\"").as_bytes());
        assert!(Archive::open(&raw, &c).unwrap().entries()[0].is_zlib(&raw));
        assert_eq!(read(&[1, 0]), [1, 0xff, 4]);
        assert!(String::from_utf8(read(&[1, 1])).unwrap().contains("<Name>a longer new name</Name>"));
        // The modded archive no longer matches the guards.
        let again = verify(&m, Some(&out), Some(&c)).unwrap();
        assert_eq!(again, 3);
        let err = apply(&m, Some(&out), &c, &out).unwrap_err();
        assert!(format!("{err:#}").contains("base_sha256 mismatch"), "{err:#}");
        for p in [target, out] { std::fs::remove_file(p).unwrap(); }
    }

    #[test]
    fn refuses_conflicts_and_bad_paths() {
        let (c, target) = setup("refuse");
        let one = |path: &str| format!("[[patch]]\npath = \"{path}\"\nop = [{{ type = \"text\", find = \"Cfg\", replace = \"Cfg\", count = 2 }}]\n");
        let problems = |patches: &str| verify(&manifest(&target, patches), None, Some(&c));
        assert_eq!(problems(&format!("{}{}", one("0"), one("0"))).unwrap(), 1);
        assert_eq!(problems(&one("5")).unwrap(), 1);
        assert_eq!(problems(&one("0/1")).unwrap(), 1);
        assert_eq!(problems("[[patch]]\npath = \"0\"\nbase_sha256 = \"00\"\n").unwrap(), 1);
        // Replacing an archive and patching inside it conflicts at repack.
        assert!(problems(&format!("[[patch]]\npath = \"1\"\nfile = {:?}\n{}", target.display().to_string(), one("1/1"))).is_err());
        let ok = manifest(&target, &one("1/1"));
        assert!(matches!(&ok.patch[0].ops[0], Op::Text { count: 2, .. }));
        assert_eq!(verify(&ok, None, Some(&c)).unwrap(), 0);
        std::fs::remove_file(target).unwrap();
    }
}
