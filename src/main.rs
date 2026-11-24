use forge9::{formats, modding, rpf9};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "forge9", version, about = "General-purpose RPF9 archive toolkit")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Path to directory containing tables.bin and keys.bin
    #[arg(long, env = "FORGE9_DATA", default_value = "data")]
    data_dir: PathBuf,

    /// Extra hash dictionary: one `hexhash name` per line
    #[arg(long, global = true, env = "FORGE9_DICT")]
    dict: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Show archive header and summary
    Info { archive: PathBuf },
    /// List all entries in the archive
    List {
        archive: PathBuf,
        #[arg(long, default_value_t = usize::MAX)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Extract entries to a directory
    Extract {
        archive: PathBuf,
        #[arg(short, long, default_value = "out")]
        output: PathBuf,
        #[arg(long)]
        select: Option<String>,
    },
    /// Hex dump a single entry
    Hexdump {
        archive: PathBuf,
        index: usize,
        #[arg(long, default_value_t = 512)]
        limit: usize,
    },
    /// Replace an entry's payload in-place (same size)
    Replace {
        archive: PathBuf,
        /// Entry index to replace
        index: usize,
        /// File with replacement bytes
        #[arg(short, long)]
        file: PathBuf,
        /// Output archive path
        #[arg(short, long, default_value = "out.rpf")]
        output: PathBuf,
    },
    /// Inspect PSIN game data in an entry
    Psin {
        archive: PathBuf,
        /// Entry index containing PSIN data
        index: usize,
        /// Show struct definitions
        #[arg(long)]
        structs: bool,
        /// Max structs to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Recursively list nested RPF9 archives
    Tree {
        archive: PathBuf,
        /// Max depth
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
    /// Repack archive with replaced entries (any size)
    Repack {
        archive: PathBuf,
        /// Replacements as index=file pairs (e.g. 0=new.bin,2=other.dat)
        #[arg(short, long, value_delimiter = ',')]
        patch: Vec<String>,
        /// Output archive path
        #[arg(short, long, default_value = "repacked.rpf")]
        output: PathBuf,
    },
    /// Find entries (recursing into nested RPF9s) matching all given filters
    Search {
        archive: PathBuf,
        /// Leading content bytes, e.g. PSIN
        #[arg(long)]
        magic: Option<String>,
        /// Detected extension, e.g. xml
        #[arg(long)]
        ext: Option<String>,
        #[arg(long)]
        min_size: Option<u64>,
        /// Byte string to find in inflated content (slow)
        #[arg(long)]
        grep: Option<String>,
    },
    /// Compute the joaat hash of a name
    Hash { name: String },
    /// List audio streams in an AWC entry
    Awc {
        archive: PathBuf,
        /// Entry index; `5/3` descends into nested RPF9s
        index: String,
    },
    /// List shader records in an SGD5 entry
    Shaders {
        archive: PathBuf,
        /// Entry index; `5/3` descends into nested RPF9s
        index: String,
        /// Only count records per stage and tag
        #[arg(long)]
        summary: bool,
    },
    /// List pipelines in a PS09 pipeline cache entry
    Pipelines {
        archive: PathBuf,
        /// Entry index; `5/3` descends into nested RPF9s
        index: String,
        /// Only graphics pipelines with a mesh (NGG) shader
        #[arg(long)]
        mesh: bool,
    },
    /// Dump FSSM root-signature layouts of the shaders in an SGD5 entry
    Fssm {
        archive: PathBuf,
        /// Entry index; `5/3` descends into nested RPF9s
        index: String,
        /// Full layout and used resources of one shader record
        #[arg(long, conflicts_with = "roots")]
        shader: Option<usize>,
        /// One root-parameter line per program
        #[arg(long)]
        roots: bool,
    },
    /// Classify the RDNA2 machine code of the shaders in an SGD5 entry, counting wave64-sensitive ops
    Isa {
        archive: PathBuf,
        /// Entry index; `5/3` descends into nested RPF9s
        index: String,
        /// Only aggregate counts per stage and op family
        #[arg(long, conflicts_with = "shader")]
        summary: bool,
        /// Full family breakdown of one shader record
        #[arg(long)]
        shader: Option<usize>,
    },
    /// Show Bink 2 video header info
    Bink {
        archive: PathBuf,
        /// Entry index; `5/3` descends into nested RPF9s
        index: String,
    },
    /// Show paged resource (0DRP) info
    Resource {
        archive: PathBuf,
        /// Entry index; `5/3` descends into nested RPF9s
        index: String,
    },
    /// List textures in an RDS5 (.ttpf) store, or export one BC1/BC3/BC5/BC7 texture's mip 0
    Texture {
        store: PathBuf,
        /// Write this texture as DDS
        #[arg(long, conflicts_with = "decode")]
        extract: Option<usize>,
        /// Decode this texture to PNG
        #[arg(long)]
        decode: Option<usize>,
        /// Output path (default: texture_<index>.dds / .png)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Array slice to export (BC3/BC5/BC7)
        #[arg(long, default_value_t = 0)]
        slice: u32,
    },
    /// Entropy-stage analysis of BCPack BC3/BC5/BC6H/BC7 chunks: fragment headers, lane symbol stats
    BcpackAnalyze {
        store: PathBuf,
        /// Chunk index (see `--codec` output for the chunks of a codec)
        #[arg(required_unless_present = "codec", conflicts_with = "codec")]
        chunk: Option<usize>,
        /// Aggregate over every chunk of this codec: bc3, bc5, bc6h, bc7
        #[arg(long)]
        codec: Option<String>,
        /// Write the chunk's lane symbol streams here (u16 LE, lane major)
        #[arg(short, long, requires = "chunk")]
        output: Option<PathBuf>,
        /// Override the lane symbol widths, e.g. `4,4,8,8,6,6,6,6,6,6,6,6`
        #[arg(long, value_delimiter = ',')]
        widths: Option<Vec<u8>>,
    },
    /// Create, verify and apply mod packs
    #[command(subcommand)]
    Mod(ModCommand),
}

#[derive(Subcommand)]
enum ModCommand {
    /// Write a template mod.toml
    Init {
        #[arg(default_value = "mod.toml")]
        path: PathBuf,
    },
    /// Check that replacement files exist and fit the target archive
    Verify {
        manifest: PathBuf,
        /// Archive to check against (default: the manifest's target_archive)
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Apply the manifest's replacements, writing a repacked archive
    Apply {
        manifest: PathBuf,
        #[arg(short, long, default_value = "modded.rpf")]
        output: PathBuf,
        /// Archive to patch (default: the manifest's target_archive)
        #[arg(long)]
        archive: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dict = rpf9::hashes::Dict::load(cli.dict.as_deref())?;
    if let Command::Hash { name } = &cli.command {
        let h = rpf9::hashes::joaat(name);
        println!("{h:#010x}  {name}");
        if let Some(known) = dict.name(h).filter(|k| k != name) { println!("  (dictionary: {known})"); }
        return Ok(());
    }
    if let Command::Mod(ModCommand::Init { path }) = &cli.command {
        if path.exists() { anyhow::bail!("{} already exists", path.display()); }
        std::fs::write(path, modding::manifest::TEMPLATE)?;
        println!("wrote {}", path.display());
        return Ok(());
    }
    if let Command::Mod(ModCommand::Verify { manifest, archive }) = &cli.command {
        let m = modding::manifest::Manifest::load(manifest)?;
        let crypto = rpf9::crypto::CryptoContext::load(&cli.data_dir).ok();
        let problems = modding::apply::verify(&m, archive.as_deref(), crypto.as_ref())?;
        if problems > 0 { anyhow::bail!("{problems} problem(s) found"); }
        println!("ok: {} replacement(s)", m.replace.len());
        return Ok(());
    }
    if let Command::Texture { store, extract, decode, output, slice } = &cli.command {
        return cmd_texture(store, *extract, *decode, output.clone(), *slice);
    }
    if let Command::BcpackAnalyze { store, chunk, codec, output, widths } = &cli.command {
        return cmd_bcpack_analyze(store, *chunk, codec.as_deref(), output.as_deref(), widths.as_deref());
    }
    let crypto = rpf9::crypto::CryptoContext::load(&cli.data_dir)
        .context("loading crypto data — set FORGE9_DATA or --data-dir")?;

    match cli.command {
        Command::Info { archive } => cmd_info(&archive, &crypto),
        Command::List { archive, limit, json } => cmd_list(&archive, &crypto, limit, json),
        Command::Extract { archive, output, select } => cmd_extract(&archive, &crypto, &output, select),
        Command::Hexdump { archive, index, limit } => cmd_hexdump(&archive, &crypto, index, limit),
        Command::Replace { archive, index, file, output } => cmd_replace(&archive, &crypto, index, &file, &output),
        Command::Psin { archive, index, structs, limit } => cmd_psin(&archive, &crypto, index, structs, limit, &dict),
        Command::Tree { archive, depth } => cmd_tree(&archive, &crypto, depth),
        Command::Repack { archive, patch, output } => cmd_repack(&archive, &crypto, &patch, &output),
        Command::Search { archive, magic, ext, min_size, grep } => {
            let filter = SearchFilter { magic: magic.map(String::into_bytes), ext, min_size, grep: grep.map(String::into_bytes) };
            let raw = std::fs::read(&archive)?;
            let hits = search(&raw, &archive.display().to_string(), &crypto, &filter)?;
            println!("{hits} matches");
            Ok(())
        }
        Command::Awc { archive, index } => cmd_awc(&load_entry(&archive, &crypto, &index)?),
        Command::Shaders { archive, index, summary } => cmd_shaders(&load_entry(&archive, &crypto, &index)?, summary),
        Command::Pipelines { archive, index, mesh } => cmd_pipelines(&load_entry(&archive, &crypto, &index)?, mesh),
        Command::Fssm { archive, index, shader, roots } => cmd_fssm(&load_entry(&archive, &crypto, &index)?, shader, roots),
        Command::Isa { archive, index, summary, shader } => cmd_isa(&load_entry(&archive, &crypto, &index)?, summary, shader),
        Command::Bink { archive, index } => cmd_bink(&load_entry(&archive, &crypto, &index)?),
        Command::Resource { archive, index } => cmd_resource(&load_entry(&archive, &crypto, &index)?),
        Command::Mod(ModCommand::Apply { manifest, output, archive }) => {
            let m = modding::manifest::Manifest::load(&manifest)?;
            modding::apply::apply(&m, archive.as_deref(), &crypto, &output)
        }
        Command::Hash { .. } | Command::Texture { .. } | Command::BcpackAnalyze { .. } | Command::Mod(_) => unreachable!(),
    }
}

fn cmd_info(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext) -> Result<()> {
    let raw = std::fs::read(path)?;
    let archive = rpf9::reader::Archive::open(&raw, crypto)?;
    let h = archive.header();
    println!("{} ({} bytes)", path.display(), raw.len());
    println!("  version {}.{} gen {:#x} key #{}", h.version_high, h.version_low, h.generation(), archive.key_index());
    println!("  entries {} payload {:#x} split {:#x} trailer {:#x}", h.entry_count, h.payload_start, h.split, h.trailer_size);
    println!("  key_id {:#x} flags {:#x} stride {}", h.key_id, h.flags, h.entry_stride());
    let zlib = archive.entries().iter().filter(|e| e.is_zlib(&raw)).count();
    println!("  {} entries ({} zlib, {} raw)", archive.entries().len(), zlib, archive.entries().len() - zlib);
    Ok(())
}

fn cmd_list(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, limit: usize, json: bool) -> Result<()> {
    let raw = std::fs::read(path)?;
    let archive = rpf9::reader::Archive::open(&raw, crypto)?;
    if json {
        println!("[");
        let total = archive.entries().len().min(limit);
        for (i, entry) in archive.entries().iter().take(limit).enumerate() {
            let comma = if i + 1 >= total { "" } else { "," };
            let ext = archive.sniff(entry);
            println!("  {{\"index\":{},\"offset\":{},\"size\":{},\"flags\":{},\"ext\":\"{ext}\"}}{comma}", entry.index, entry.offset, entry.size, entry.flags);
        }
        println!("]");
    } else {
        for entry in archive.entries().iter().take(limit) {
            let ext = archive.sniff(entry);
            let zlib = if entry.is_zlib(&raw) { " zlib" } else { "" };
            println!("  #{:<4} {:#010x}  {:>10} bytes  .{ext}{zlib}", entry.index, entry.offset, entry.size);
        }
        if archive.entries().len() > limit {
            println!("  … {} more", archive.entries().len() - limit);
        }
    }
    Ok(())
}

fn cmd_extract(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, output: &PathBuf, select: Option<String>) -> Result<()> {
    let raw = std::fs::read(path)?;
    let archive = rpf9::reader::Archive::open(&raw, crypto)?;
    std::fs::create_dir_all(output)?;
    let indices: Option<Vec<usize>> = select.map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect());
    let mut written = 0;
    for entry in archive.entries() {
        if let Some(ref idx) = indices {
            if !idx.contains(&entry.index) { continue; }
        }
        let bytes = archive.read_entry(entry)?;
        let ext = rpf9::reader::sniff_ext(&bytes);
        std::fs::write(output.join(format!("{:08x}.{ext}", entry.offset)), &bytes)?;
        written += 1;
    }
    println!("extracted {written} entries → {}", output.display());
    Ok(())
}

fn cmd_hexdump(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, index: usize, limit: usize) -> Result<()> {
    let raw = std::fs::read(path)?;
    let archive = rpf9::reader::Archive::open(&raw, crypto)?;
    let entry = archive.entries().get(index).ok_or_else(|| anyhow::anyhow!("index {index} out of range"))?;
    let bytes = archive.read_entry(entry)?;
    for (row, chunk) in bytes[..bytes.len().min(limit)].chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
        println!("{:08x}  {:<47}  |{ascii}|", row * 16, hex.join(" "));
    }
    if bytes.len() > limit { println!("… {} bytes total", bytes.len()); }
    Ok(())
}

fn cmd_replace(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, index: usize, file: &PathBuf, output: &PathBuf) -> Result<()> {
    let raw = std::fs::read(path)?;
    let replacement = std::fs::read(file)?;
    let archive = rpf9::reader::Archive::open(&raw, crypto)?;
    let entry = archive.entries().get(index)
        .ok_or_else(|| anyhow::anyhow!("entry index {index} out of range"))?;

    if entry.is_zlib(&raw) {
        rpf9::writer::replace_zlib_inplace(&raw, crypto, index, &replacement, output)
    } else {
        rpf9::writer::replace_inplace(&raw, crypto, index, &replacement, output)
    }
}

fn cmd_repack(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, patches: &[String], output: &PathBuf) -> Result<()> {
    let raw = std::fs::read(path)?;
    let mut replacements = std::collections::HashMap::new();
    for patch in patches {
        let (idx_str, file_str) = patch.split_once('=')
            .ok_or_else(|| anyhow::anyhow!("patch must be index=file, got: {patch}"))?;
        let idx: usize = idx_str.trim().parse()
            .context(format!("bad index in patch: {idx_str}"))?;
        let data = std::fs::read(file_str.trim())
            .context(format!("reading patch file: {file_str}"))?;
        replacements.insert(idx, data);
    }
    rpf9::writer::repack(&raw, crypto, &replacements, output)
}

fn cmd_psin(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, index: usize, show_structs: bool, limit: usize, dict: &rpf9::hashes::Dict) -> Result<()> {
    let raw = std::fs::read(path)?;
    let archive = rpf9::reader::Archive::open(&raw, crypto)?;
    let entry = archive.entries().get(index)
        .ok_or_else(|| anyhow::anyhow!("entry index {index} out of range"))?;
    let bytes = archive.read_entry(entry)?;

    // Check if it's a nested RPF9 containing PSIN
    if bytes.starts_with(b"9FPR") {
        println!("entry #{index} is a nested RPF9 — scanning for PSIN inside...");
        let nested = rpf9::reader::Archive::open(&bytes, crypto)?;
        let mut found = 0;
        for ne in nested.entries() {
            let nb = nested.read_entry(ne)?;
            if nb.starts_with(b"PSIN") {
                found += 1;
                println!("\n  nested entry #{} ({} bytes):", ne.index, nb.len());
                let container = rpf9::psin::PsinContainer::parse(&nb)?;
                rpf9::psin::print_summary(&container);
                if show_structs { rpf9::psin::print_structs(&container, limit, dict); }
            }
        }
        if found == 0 { println!("  no PSIN entries in nested archive"); }
        return Ok(());
    }

    if !bytes.starts_with(b"PSIN") {
        anyhow::bail!("entry #{index} is not PSIN (starts with {:02x?})", &bytes[..4.min(bytes.len())]);
    }

    let container = rpf9::psin::PsinContainer::parse(&bytes)?;
    rpf9::psin::print_summary(&container);
    if show_structs { rpf9::psin::print_structs(&container, limit, dict); }
    Ok(())
}

fn cmd_tree(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, max_depth: usize) -> Result<()> {
    let raw = std::fs::read(path)?;
    tree(&raw, &path.display().to_string(), crypto, max_depth, 0)
}

fn tree(raw: &[u8], label: &str, crypto: &rpf9::crypto::CryptoContext, max_depth: usize, depth: usize) -> Result<()> {
    let indent = "  ".repeat(depth);
    let archive = rpf9::reader::Archive::open(raw, crypto)?;
    let h = archive.header();
    println!("{indent}{label} (v{}.{} key#{} {} entries)",
        h.version_high, h.version_low, archive.key_index(), archive.entries().len());

    for entry in archive.entries() {
        let ext = archive.sniff(entry);
        let zlib = if entry.is_zlib(raw) { " zlib" } else { "" };
        println!("{indent}  #{:<4} {:#010x} {:>10} bytes .{ext}{zlib}",
            entry.index, entry.offset, entry.size);

        if ext == "rpf" && depth < max_depth {
            let bytes = archive.read_entry(entry)?;
            if let Err(e) = tree(&bytes, &format!("#{}", entry.index), crypto, max_depth, depth + 1) {
                println!("{indent}    (nested open failed: {e})");
            }
        }
    }
    Ok(())
}

struct SearchFilter {
    magic: Option<Vec<u8>>,
    ext: Option<String>,
    min_size: Option<u64>,
    grep: Option<Vec<u8>>,
}

/// Prints matching entries as `label#index`, descending into nested RPF9s; returns the match count.
fn search(raw: &[u8], label: &str, crypto: &rpf9::crypto::CryptoContext, f: &SearchFilter) -> Result<usize> {
    let archive = rpf9::reader::Archive::open(raw, crypto)?;
    let mut hits = 0;
    for entry in archive.entries() {
        let ext = archive.sniff(entry);
        let path = format!("{label}#{}", entry.index);
        let matches = f.min_size.is_none_or(|m| entry.size >= m)
            && f.ext.as_deref().is_none_or(|e| e.trim_start_matches('.') == ext)
            && f.magic.as_deref().is_none_or(|m| archive.head(entry, m.len()) == m);
        // Content is only inflated when needed: for grep, or to recurse.
        let bytes = if (matches && f.grep.is_some()) || ext == "rpf" { archive.read_entry(entry).ok() } else { None };
        let grep_ok = match (&f.grep, &bytes) {
            (None, _) => true,
            (Some(g), Some(b)) => !g.is_empty() && b.windows(g.len()).any(|w| w == g.as_slice()),
            (Some(_), None) => false,
        };
        if matches && grep_ok {
            hits += 1;
            println!("  {path:<24} {:#010x} {:>10} bytes .{ext}", entry.offset, entry.size);
        }
        if ext == "rpf" {
            if let Some(b) = &bytes {
                match search(b, &path, crypto, f) {
                    Ok(n) => hits += n,
                    Err(e) => eprintln!("  {path}: nested open failed: {e}"),
                }
            }
        }
    }
    Ok(hits)
}

/// Reads an entry by `index`, where `a/b/c` descends through nested RPF9s.
fn load_entry(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, index: &str) -> Result<Vec<u8>> {
    let mut bytes = std::fs::read(path)?;
    for part in index.split(['/', '#']).filter(|p| !p.is_empty()) {
        let i: usize = part.parse().with_context(|| format!("bad entry index {part:?}"))?;
        let archive = rpf9::reader::Archive::open(&bytes, crypto)?;
        let entry = archive.entries().get(i).ok_or_else(|| anyhow::anyhow!("entry index {i} out of range"))?;
        bytes = archive.read_entry(entry)?;
    }
    Ok(bytes)
}

fn cmd_awc(bytes: &[u8]) -> Result<()> {
    let awc = formats::awc::parse(bytes)?;
    println!("AWC v{} flags {:#06x} header {:#x} — {} streams", awc.version, awc.flags, awc.header_size, awc.streams.len());
    for s in &awc.streams {
        let fmt = match s.format {
            Some(f) => {
                let rate = if f.codec == 0x0B { "48000 Hz (Opus)".to_string() } else { format!("{} Hz", f.sample_rate) };
                format!("{:<7} {}ch {rate} {} samples", formats::awc::codec_name(f.codec), f.channels, f.samples)
            }
            None => "no format".to_string(),
        };
        let data = s.chunk(formats::awc::TAG_DATA).map_or(0, |c| c.size);
        let tags: Vec<&str> = s.chunks.iter().map(|c| formats::awc::tag_name(c.tag)).collect();
        println!("  {:#09x}  {fmt}  data {data} bytes  [{}]", s.id, tags.join(","));
    }
    Ok(())
}

fn cmd_shaders(bytes: &[u8], summary: bool) -> Result<()> {
    let shaders = formats::sgd5::parse(bytes)?;
    let mut counts = std::collections::BTreeMap::new();
    for s in &shaders {
        *counts.entry((s.stage(), s.tag)).or_insert(0usize) += 1;
        if !summary {
            println!("  {:#010x}  {:<3} {:#04x} {:<13} {:>8} → {:>8}  {}", s.offset, s.stage(), s.tag, formats::sgd5::tag_name(s.tag), s.csize, s.dsize, s.name);
        }
    }
    println!("{} shader records", shaders.len());
    for ((stage, tag), n) in counts {
        println!("  {stage:<3} {tag:#04x} {:<13} {n}", formats::sgd5::tag_name(tag));
    }
    Ok(())
}

fn cmd_pipelines(bytes: &[u8], mesh_only: bool) -> Result<()> {
    use formats::ps09;
    let c = ps09::parse(bytes)?;
    let h = &c.header;
    println!("PS09 {} driver {}: {} graphics, {} compute, {} metadata, {} shaders ({} code bytes)",
        h.id, h.driver, h.graphics, h.compute, h.metas, h.shaders, h.code_bytes);
    let mut patterns = std::collections::BTreeMap::new();
    for (i, p) in c.graphics.iter().enumerate() {
        let slots: Vec<String> = p.slots.iter().enumerate()
            .filter_map(|(s, slot)| slot.map(|x| format!("{}={}", ps09::SLOT_NAMES[s], x.shader))).collect();
        let pattern: Vec<&str> = slots.iter().map(|s| s.split('=').next().unwrap()).collect();
        *patterns.entry(pattern.join("+")).or_insert(0usize) += 1;
        if !mesh_only || p.is_mesh() {
            println!("  G#{i:<6} {:016x}  {} shaders  {}", p.hash, p.shader_count(), slots.join(" "));
        }
    }
    if !mesh_only {
        for (i, p) in c.compute.iter().enumerate() {
            println!("  C#{i:<6} {:016x}  1 shaders  CS={}", p.hash, p.shader);
        }
    }
    let truncated = c.shaders.iter().filter(|r| r.end > bytes.len()).count();
    if truncated > 0 { println!("warning: {truncated} code blocks run past the end of the blob"); }
    for (pattern, n) in patterns { println!("  {pattern:<10} {n} graphics pipelines"); }
    Ok(())
}

/// The FSSM programs of one SGD5 record (one per XBCH section).
fn record_programs(sgd: &[u8], rec: &formats::sgd5::Shader) -> Result<Vec<formats::fssm::Fssm>> {
    let dxbc = rec.dxbc(sgd)?;
    formats::xbch::sections(&dxbc)?.into_iter()
        .map(|x| formats::fssm::parse(&formats::xbch::decompress(x)?))
        .collect()
}

fn root_line(stage: &formats::fssm::Stage) -> String {
    let params: Vec<String> = stage.root_params().iter().map(|p| p.to_string()).collect();
    let samplers = match stage.static_samplers() { 0 => String::new(), n => format!(" + {n} static samplers") };
    format!("{} root params [{}]{samplers}", params.len(), params.join(", "))
}

fn cmd_fssm(bytes: &[u8], shader: Option<usize>, roots: bool) -> Result<()> {
    use formats::fssm;
    let records = formats::sgd5::parse(bytes)?;
    if let Some(i) = shader {
        let rec = records.get(i).ok_or_else(|| anyhow::anyhow!("shader {i} out of range ({} records)", records.len()))?;
        println!("#{i} {} (tag {:#04x})", rec.name, rec.tag);
        for (n, prog) in record_programs(bytes, rec)?.iter().enumerate() {
            for s in &prog.stages {
                println!("  program {n}: FSSM v{:#x} {} (api {}) code {:#x}+{:#x} {} user SGPRs {}",
                    prog.version, s.name(), s.api, s.code_offset, s.code_size, s.user_sgpr_count, if s.wave32 { "wave32" } else { "wave64" });
                println!("    {}", root_line(s));
                println!("    layout ({}):", s.layout.len());
                for e in &s.layout {
                    println!("      {:<13} reg {:<4} table+{:<4} sgprs {}  s{:<2} slot {}",
                        fssm::root_type_name(e.root_type), e.register, e.table_offset, e.user_sgprs, e.first_user_data, e.root_slot);
                }
                println!("    used ({}):", s.used.len());
                for u in &s.used {
                    println!("      {:<7} {:<13} reg {:<4} table+{:<4} stride {:<4} {}dw  s{:<2} slot {}",
                        fssm::resource_type_name(u.resource_type), fssm::root_type_name(u.root_type as u32),
                        u.lower_bound, u.table_offset, u.stride, u.descriptor_dwords, u.first_user_data, u.root_slot);
                }
            }
        }
        return Ok(());
    }
    // (stage, programs, with a root layout); failures counted separately.
    let mut counts: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    let mut failed = 0;
    for rec in &records {
        match record_programs(bytes, rec) {
            Ok(progs) => for s in progs.iter().flat_map(|p| &p.stages) {
                let c = counts.entry(s.name()).or_default();
                c.0 += 1;
                if !s.layout.is_empty() { c.1 += 1; }
                if roots {
                    let wave = if s.wave32 { "w32" } else { "w64" };
                    println!("{} {} {}: {}", s.name(), wave, rec.name, root_line(s));
                }
            },
            Err(e) => {
                failed += 1;
                if roots { eprintln!("? {}: {e}", rec.name); }
            }
        }
    }
    println!("{} records, {failed} without a readable FSSM program", records.len());
    for (stage, (n, with)) in counts { println!("  {stage:<3} {n:>6} programs, {with} with a root layout"); }
    Ok(())
}

fn cmd_bink(bytes: &[u8]) -> Result<()> {
    let b = formats::bink::parse(bytes)?;
    println!("Bink 2 rev '{}'  {}x{}  {} frames @ {:.3} fps ({}/{})  {:.2}s  file {} bytes  largest frame {} bytes",
        b.revision, b.width, b.height, b.frames, b.fps(), b.fps_num, b.fps_den, b.duration_secs(), b.file_size, b.largest_frame);
    Ok(())
}

fn cmd_resource(bytes: &[u8]) -> Result<()> {
    let r = formats::resource::parse(bytes)?;
    let magic = r.magic_at.map_or("none".to_string(), |m| format!("0DRP at {m:#x}"));
    let version = r.version.map_or(String::new(), |v| format!(" version {v:#x}"));
    println!("{:?} resource, {} bytes, magic {magic}{version}", r.kind, bytes.len());
    println!("  pointer-like words: {} virtual, {} physical", r.virtual_ptrs, r.physical_ptrs);
    Ok(())
}

fn cmd_texture(path: &PathBuf, extract: Option<usize>, decode: Option<usize>, output: Option<PathBuf>, slice: u32) -> Result<()> {
    use formats::texture;
    let store = texture::Store::open(path).with_context(|| format!("opening {}", path.display()))?;
    let texs = store.textures()?;
    if let Some(i) = extract.or(decode) {
        let tex = texs.get(i).ok_or_else(|| anyhow::anyhow!("texture {i} out of range ({} textures)", texs.len()))?;
        let png = decode.is_some();
        let out = output.unwrap_or_else(|| PathBuf::from(format!("texture_{i}.{}", if png { "png" } else { "dds" })));
        let (bytes, unfitted) = texture::export(&store, tex, png, slice)?;
        std::fs::write(&out, bytes)?;
        let sl = if tex.array_size > 1 { format!(" slice {slice}") } else { String::new() };
        println!("texture {i}: {} {}x{}{sl} mip 0 → {}", texture::dxgi_name(tex.dxgi), tex.width, tex.height, out.display());
        if unfitted > 0 { println!("  warning: {unfitted} blocks depend on table values bcpack-rs marks undetermined and may be off"); }
        return Ok(());
    }
    let mut codecs = std::collections::BTreeMap::new();
    for c in &store.chunks { *codecs.entry(texture::codec_name(c.codec)).or_insert(0usize) += 1; }
    let codecs: Vec<String> = codecs.iter().map(|(k, n)| format!("{n} {k}")).collect();
    println!("{}: {} chunks ({}), {} textures", path.display(), store.chunks.len(), codecs.join(", "), texs.len());
    for t in &texs {
        let array = if t.array_size > 1 { format!(" x{}", t.array_size) } else { String::new() };
        println!("  #{:<5} {:>5}x{:<5}{array} {:<5} (dxgi {:>2}) mips {:>2} tile {}  hash {:016x}", t.index, t.width, t.height, texture::dxgi_name(t.dxgi), t.dxgi, t.mips, t.tile_mode, t.hash);
    }
    Ok(())
}

/// Each FSSM stage of one SGD5 record with its classified machine code.
fn record_isa(sgd: &[u8], rec: &formats::sgd5::Shader) -> Result<Vec<(formats::fssm::Stage, formats::rdna2::Stats)>> {
    let mut out = Vec::new();
    for x in formats::xbch::sections(&rec.dxbc(sgd)?)? {
        let blob = formats::xbch::decompress(x)?;
        for s in formats::fssm::parse(&blob)?.stages {
            let code = blob.get(s.code_offset as usize..(s.code_offset + s.code_size) as usize)
                .with_context(|| format!("code {:#x}+{:#x} past the {:#x}-byte FSSM", s.code_offset, s.code_size, blob.len()))?;
            out.push((s, formats::rdna2::analyze(code)));
        }
    }
    Ok(out)
}

fn wave_name(s: &formats::fssm::Stage) -> &'static str { if s.wave32 { "w32" } else { "w64" } }

fn isa_status(st: &formats::rdna2::Stats) -> String {
    if st.clean() { return "ok".into(); }
    let mut why = Vec::new();
    if st.unknown() > 0 { why.push(format!("{} unknown", st.unknown())); }
    if st.truncated { why.push("truncated".into()); }
    if st.bad_targets > 0 { why.push(format!("{} bad branch targets", st.bad_targets)); }
    if st.exits == 0 { why.push("no exit".into()); }
    format!("DESYNC({})", why.join(", "))
}

fn cmd_isa(bytes: &[u8], summary: bool, shader: Option<usize>) -> Result<()> {
    use formats::rdna2::{FAMILIES, WAVE_OPS, WaveOp};
    use std::collections::BTreeMap;
    let records = formats::sgd5::parse(bytes)?;
    if let Some(i) = shader {
        let rec = records.get(i).ok_or_else(|| anyhow::anyhow!("shader {i} out of range ({} records)", records.len()))?;
        println!("#{i} {} (tag {:#04x})", rec.name, rec.tag);
        for (n, (s, st)) in record_isa(bytes, rec)?.iter().enumerate() {
            println!("  program {n}: {} {} code {:#x}+{:#x}: {} instructions, {} s_endpgm, {} data dwords, {}",
                s.name(), wave_name(s), s.code_offset, s.code_size, st.insts, st.endpgms, st.data_dwords, isa_status(st));
            println!("    families:");
            for f in FAMILIES.iter().filter(|&&f| st.family(f) > 0) { println!("      {:<8} {:>6}", f.name(), st.family(*f)); }
            println!("    wave64-sensitive:");
            for o in WAVE_OPS.iter().filter(|&&o| st.op(o) > 0) { println!("      {:<15} {:>6}", o.name(), st.op(*o)); }
        }
        return Ok(());
    }

    // Per stage: programs, wave64, instructions, clean, unknown dwords.
    let mut stages: BTreeMap<&str, [u64; 5]> = BTreeMap::new();
    let mut families = [0u64; FAMILIES.len()];
    // Per op: (stage -> wave64 programs using it), wave32 programs using it, total occurrences.
    let mut ops: Vec<(BTreeMap<&str, u64>, u64, u64)> = vec![Default::default(); WAVE_OPS.len()];
    let mut groups: BTreeMap<&str, BTreeMap<&str, u64>> = BTreeMap::new();
    let mut failed = 0;
    const GROUPS: [(&str, &[WaveOp]); 5] = [
        ("any DPP16/DPP8", &[WaveOp::DppQuadPerm, WaveOp::DppRowShift, WaveOp::DppRowMirror, WaveOp::DppRowShare, WaveOp::DppRowXmask, WaveOp::DppOther, WaveOp::Dpp8]),
        ("any permute (ds_[b]permute, permlane, swizzle)", &[WaveOp::DsPermute, WaveOp::Permlane, WaveOp::DsSwizzle]),
        ("any lane read/write (readlane, writelane, readfirstlane)", &[WaveOp::LaneHi, WaveOp::Readlane, WaveOp::Writelane, WaveOp::Readfirstlane]),
        ("crosses the 32-lane halves (ds_permute, lane_32_63, readfirstlane, mbcnt_hi, 64-bit masks)", &[WaveOp::DsPermute, WaveOp::LaneHi, WaveOp::Readfirstlane, WaveOp::MbcntHi, WaveOp::ExecMask64, WaveOp::MaskScan64, WaveOp::QuadMask64]),
        ("any 64-bit mask op (exec/vcc b64, mask scan, mbcnt_hi, quad/wqm)", &[WaveOp::ExecMask64, WaveOp::MaskScan64, WaveOp::MbcntHi, WaveOp::QuadMask64]),
    ];
    for (i, rec) in records.iter().enumerate() {
        let progs = match record_isa(bytes, rec) {
            Ok(p) => p,
            Err(e) => { failed += 1; if !summary { eprintln!("? #{i} {}: {e}", rec.name); } continue; }
        };
        for (s, st) in &progs {
            let c = stages.entry(s.name()).or_default();
            c[0] += 1;
            c[1] += !s.wave32 as u64;
            c[2] += st.insts as u64;
            c[3] += st.clean() as u64;
            c[4] += st.unknown() as u64;
            for (t, &n) in families.iter_mut().zip(&st.families) { *t += n as u64; }
            for (o, &n) in WAVE_OPS.iter().zip(&st.wave) {
                let e = &mut ops[*o as usize];
                e.2 += n as u64;
                if n == 0 { continue; }
                if s.wave32 { e.1 += 1; } else { *e.0.entry(s.name()).or_default() += 1; }
            }
            if !s.wave32 {
                for (g, members) in GROUPS {
                    if members.iter().any(|&o| st.op(o) > 0) { *groups.entry(g).or_default().entry(s.name()).or_default() += 1; }
                }
            }
            if !summary {
                let used: Vec<String> = WAVE_OPS.iter().filter(|&&o| st.op(o) > 0).map(|o| format!("{}={}", o.name(), st.op(*o))).collect();
                println!("#{i:<5} {} {} {:>6} insts  {}  [{}]  {}", s.name(), wave_name(s), st.insts, isa_status(st), used.join(" "), rec.name);
            }
        }
    }

    let total: [u64; 5] = stages.values().fold([0; 5], |a, c| std::array::from_fn(|k| a[k] + c[k]));
    println!("{} records, {failed} without a readable FSSM program; {} programs, {} instructions, {} decoded cleanly, {} unknown dwords",
        records.len(), total[0], total[2], total[3], total[4]);
    println!("  stage  programs  wave64  instructions  clean  unknown");
    for (stage, c) in &stages { println!("  {stage:<5} {:>9} {:>7} {:>13} {:>6} {:>8}", c[0], c[1], c[2], c[3], c[4]); }
    println!("instructions by family:");
    for (f, n) in FAMILIES.iter().zip(families).filter(|(_, n)| *n > 0) {
        println!("  {:<8} {n:>12}  {:>5.1}%", f.name(), 100.0 * n as f64 / total[2].max(1) as f64);
    }
    let names: Vec<&str> = stages.keys().copied().collect();
    let header: String = names.iter().map(|s| format!(" {s:>6}")).collect();
    println!("wave64 programs using each op, by stage (most widespread first):");
    println!("  {:<15} {:>7}{header} {:>6} {:>12}", "op", "w64", "w32", "occurrences");
    let mut order: Vec<usize> = (0..WAVE_OPS.len()).collect();
    order.sort_by_key(|&k| std::cmp::Reverse(ops[k].0.values().sum::<u64>()));
    for k in order {
        let (by, w32, occ) = &ops[k];
        let cols: String = names.iter().map(|s| format!(" {:>6}", by.get(s).copied().unwrap_or(0))).collect();
        println!("  {:<15} {:>7}{cols} {w32:>6} {occ:>12}", WAVE_OPS[k].name(), by.values().sum::<u64>());
    }
    println!("wave64 programs by op group:");
    for (g, _) in GROUPS {
        let by = groups.get(g).cloned().unwrap_or_default();
        let cols: String = names.iter().map(|s| format!(" {s}={}", by.get(s).copied().unwrap_or(0))).collect();
        println!("  {:>6}  {g}:{cols}", by.values().sum::<u64>());
    }
    Ok(())
}

fn cmd_bcpack_analyze(path: &PathBuf, chunk: Option<usize>, codec: Option<&str>, output: Option<&std::path::Path>, widths_override: Option<&[u8]>) -> Result<()> {
    use formats::{bcpack_analyze as ba, texture};
    let store = texture::Store::open(path).with_context(|| format!("opening {}", path.display()))?;
    let syms_of = |c: &texture::Store, i: usize| (c.chunks[i].vlen / ba::SYMBOL_BYTES) as usize;
    if let Some(i) = chunk {
        let c = store.chunks.get(i).ok_or_else(|| anyhow::anyhow!("chunk {i} out of range ({} chunks)", store.chunks.len()))?;
        let widths = widths_override.or(ba::lane_widths(c.codec)).ok_or_else(|| anyhow::anyhow!("chunk {i} is {}, not BC3/BC5/BC6H/BC7 BCPack", texture::codec_name(c.codec)))?;
        let data = store.chunk_bytes(i, false)?;
        let n = syms_of(&store, i);
        println!("chunk {i}: {}  file {:#x}+{:#x} (pad {})  virtual {:#x}+{:#x}  {} blocks, {n} symbols/lane, {:.2} bytes/block",
            texture::codec_name(c.codec), c.file_off, c.file_len, c.pad, c.vstart, c.vlen, n / 2, data.len() as f64 / (n / 2) as f64);
        for t in store.textures()?.iter().filter(|t| (c.vstart..c.vstart + c.vlen).contains(&t.vstart)) {
            println!("  texture #{} {}x{} {} mips {} starts here", t.index, t.width, t.height, texture::dxgi_name(t.dxgi), t.mips);
        }
        let a = ba::analyze(data, widths, n).map_err(|e| anyhow::anyhow!("entropy stage failed: {e}"))?;
        println!("  converged in {} cycles: {} of {} bytes consumed, tail {}", a.cycles, a.consumed, a.stream_len, if a.tail_zero { "zero" } else { "NOT zero" });
        println!("  lane width   bytes frags  end-state     hdr  bits/sym entropy  zero%  distinct  max  q-models          z   flush 3B  payload min..max  top symbols");
        for (k, l) in a.lanes.iter().enumerate() {
            let state = if l.final_state == bcpack::rans::L { "L" } else if l.final_state == bcpack::rans::L >> 8 { "L>>8" } else { "L>>16" };
            let mut qs: Vec<u8> = l.fragments.iter().map(|f| f.q).collect(); qs.sort(); qs.dedup();
            let mut zs: Vec<u8> = l.fragments.iter().map(|f| f.z).collect(); zs.sort(); zs.dedup();
            let pay = l.fragments.iter().map(|f| f.payload);
            let mut top: Vec<(usize, u64)> = l.hist.iter().copied().enumerate().filter(|x| x.1 > 0).collect();
            top.sort_by(|x, y| y.1.cmp(&x.1));
            let top: Vec<String> = top.iter().take(5).map(|(v, k)| format!("{v}:{k}")).collect();
            println!("  {k:>4} {:>5} {:>7} {:>5}  {state:<12}  {:<4} {:>7.3} {:>7.3} {:>6.1} {:>8} {:>4}  {:<17} {:<3} {:>4} {:>3}  {:>5}..{:<6}      {}",
                l.width, l.bytes, l.fragments.len(), if l.headers_consistent() { "ok" } else { "BAD" }, l.coded_bits(), l.entropy(),
                100.0 * l.hist[0] as f64 / n as f64, l.distinct(), l.max_symbol().unwrap_or(0), format!("{qs:?}"), format!("{zs:?}"),
                l.fragments.iter().filter(|f| f.flush).count(), l.fragments.iter().filter(|f| f.three_byte).count(),
                pay.clone().min().unwrap_or(0), pay.max().unwrap_or(0), top.join(" "));
        }
        if let Some(out) = output {
            let bytes: Vec<u8> = a.symbols.iter().flatten().flat_map(|s| s.to_le_bytes()).collect();
            std::fs::write(out, bytes)?;
            println!("  {} lanes x {n} symbols (u16 LE, lane major) → {}", a.lanes.len(), out.display());
        }
        return Ok(());
    }
    let code = match codec.unwrap_or_default().to_ascii_lowercase().as_str() {
        "bc3" => 0x0C, "bc5" => 0x14, "bc6h" => 0x18, "bc7" => 0x1C,
        other => anyhow::bail!("unknown codec {other:?} (bc3, bc5, bc6h, bc7)"),
    };
    let widths = widths_override.unwrap_or(ba::lane_widths(code).unwrap());
    if widths.is_empty() || widths.len() > 16 || widths.iter().any(|w| !(1..=9).contains(w)) { anyhow::bail!("need 1..=16 widths of 1..=9 bits"); }
    let idx: Vec<usize> = store.chunks.iter().filter(|c| c.codec == code).map(|c| c.index).collect();
    println!("{}: {} {} chunks", path.display(), idx.len(), texture::codec_name(code));
    if idx.is_empty() { return Ok(()); }
    let mut ok = Vec::new();
    for &i in &idx {
        match ba::analyze(store.chunk_bytes(i, false)?, widths, syms_of(&store, i)) {
            Ok(a) => ok.push((i, a)),
            Err(e) => println!("  chunk {i}: entropy stage failed ({} stored bytes): {e}", store.chunks[i].data_len()),
        }
    }
    let mut blocks: Vec<usize> = idx.iter().map(|&i| syms_of(&store, i) / 2).collect();
    blocks.sort();
    let stored: u64 = idx.iter().map(|&i| store.chunks[i].data_len()).sum();
    let total_blocks: usize = blocks.iter().sum();
    println!("  blocks per chunk {}..{} (median {}), {total_blocks} blocks, {stored} stored bytes = {:.2} bytes/block ({:.1}% of raw)",
        blocks[0], blocks[blocks.len() - 1], blocks[blocks.len() / 2], stored as f64 / total_blocks as f64, 100.0 * stored as f64 / (16 * total_blocks) as f64);
    let lane_count = widths.len();
    let all_lanes = || ok.iter().flat_map(|(_, a)| a.lanes.iter());
    let legal = all_lanes().filter(|l| bcpack::rans::is_legal_end_state(l.final_state)).count();
    let exact = ok.iter().filter(|(_, a)| a.consumed == a.stream_len).count();
    let tail = ok.iter().filter(|(_, a)| a.tail_zero).count();
    println!("  entropy stage: {}/{} chunks converge; {legal}/{} lanes end in a legal state; consumed == stored-pad in {exact}, zero tail in {tail}",
        ok.len(), idx.len(), ok.len() * lane_count);
    let ends = |s: u32| all_lanes().filter(|l| l.final_state == s).count();
    println!("    end states: L {}, L>>8 {}, L>>16 {}", ends(bcpack::rans::L), ends(bcpack::rans::L >> 8), ends(bcpack::rans::L >> 16));
    let frags: Vec<&ba::Fragment> = all_lanes().flat_map(|l| l.fragments.iter()).collect();
    let consistent = all_lanes().filter(|l| l.headers_consistent()).count();
    let mut pay: Vec<u32> = frags.iter().map(|f| f.payload).collect();
    pay.sort();
    let count = |f: &dyn Fn(&&ba::Fragment) -> bool| frags.iter().filter(|x| f(x)).count();
    println!("  fragment headers: {} fragments, header+payload == lane bytes and count == decoder count in {consistent}/{} lanes",
        frags.len(), ok.len() * lane_count);
    if !pay.is_empty() {
        println!("    three-byte {}, flush {}, payload {}..{} (median {})", count(&|f| f.three_byte), count(&|f| f.flush), pay[0], pay[pay.len() - 1], pay[pay.len() / 2]);
    }
    let qd: Vec<String> = (0..16).map(|q| count(&|f| f.q == q)).enumerate().filter(|x| x.1 > 0).map(|(q, n)| format!("q{q}:{n}")).collect();
    let zd: Vec<String> = (0..4).map(|z| count(&|f| f.z == z)).enumerate().filter(|x| x.1 > 0).map(|(z, n)| format!("z{z}:{n}")).collect();
    println!("    models {}  narrowing {}", qd.join(" "), zd.join(" "));
    println!("  lane width frags  bits/sym entropy  zero% even/odd  distinct even/odd  max  chunks-constant  MI(2i,2i+1) MI(2i+1,2i+2)");
    for n in 0..lane_count {
        let w = widths[n];
        let (mut he, mut ho) = (vec![0u64; 1 << w], vec![0u64; 1 << w]);
        let (mut bytes, mut nf, mut constant) = (0u64, 0usize, 0usize);
        let (mut pair, mut cross) = (ba::Joint::new(w, w), ba::Joint::new(w, w));
        for (_, a) in &ok {
            let l = &a.lanes[n];
            let (e, o) = ba::halves(&a.symbols[n]);
            for &x in &e { he[x as usize] += 1; }
            for &x in &o { ho[x as usize] += 1; }
            pair.add(&e, &o);
            cross.add(&o, &e[1..]);
            bytes += l.bytes as u64;
            nf += l.fragments.len();
            constant += (l.distinct() == 1) as usize;
        }
        let hist = he.iter().zip(&ho).map(|(x, y)| x + y).collect();
        let l = ba::Lane { width: w, bytes: bytes as u32, hist, ..Default::default() };
        let pct = |h: &[u64]| 100.0 * h[0] as f64 / h.iter().sum::<u64>().max(1) as f64;
        let distinct = |h: &[u64]| h.iter().filter(|&&x| x > 0).count();
        println!("  {n:>4} {w:>5} {nf:>5}  {:>7.3} {:>7.3}   {:>5.1}/{:<5.1}       {:>3}/{:<3}       {:>4}  {constant:>3}/{:<3}          {:>6.3}      {:>6.3}",
            l.coded_bits(), l.entropy(), pct(&he), pct(&ho), distinct(&he), distinct(&ho), l.max_symbol().unwrap_or(0), ok.len(), pair.mi(), cross.mi());
    }
    println!("  mutual information between lanes of the same symbol vector (bits):");
    println!("       {}", (0..lane_count).map(|b| format!("{b:>5}")).collect::<String>());
    for x in 0..lane_count {
        let row: String = (0..lane_count).map(|y| {
            if x == y { return "    -".into(); }
            let mut j = ba::Joint::new(widths[x], widths[y]);
            for (_, a) in &ok { j.add(&a.symbols[x], &a.symbols[y]); }
            format!("{:>5.2}", j.mi())
        }).collect();
        println!("  {x:>4} {row}");
    }
    // Every lane width and the block count are load-bearing: change one and the chunks stop converging.
    println!("  perturbation (chunks still converging, of {}):", ok.len());
    let conv = |w: &[u8], db: isize| ok.iter().filter(|(i, a)| ba::converges(store.chunk_bytes(*i, false).unwrap(), w, a.symbols_per_lane.saturating_add_signed(db))).count();
    let one_per_block = ok.iter().filter(|(i, a)| ba::converges(store.chunk_bytes(*i, false).unwrap(), widths, a.symbols_per_lane / 2)).count();
    println!("    symbols/lane -1: {}  +1: {}  one per block (virtual/16): {one_per_block}", conv(widths, -1), conv(widths, 1));
    let mut line = Vec::new();
    for n in 0..lane_count {
        let mut r = Vec::new();
        for d in [-1i8, 1] {
            let w = widths[n] as i8 + d;
            if !(1..=9).contains(&w) { r.push("-".to_string()); continue; }
            let mut ws = widths.to_vec();
            ws[n] = w as u8;
            r.push(conv(&ws, 0).to_string());
        }
        line.push(format!("lane{n} {}", r.join("/")));
    }
    println!("    width -1/+1: {}", line.join("  "));
    Ok(())
}
