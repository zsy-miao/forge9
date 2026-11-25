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
        /// Also list the entries of nested RPF9s, as `a/b` paths
        #[arg(long)]
        recursive: bool,
    },
    /// Summarize an archive and its nested RPF9s: entries per kind, sizes, largest entries
    Stats {
        archive: PathBuf,
        /// Number of largest entries to show
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long)]
        json: bool,
    },
    /// Compare two archives (with nested RPF9s) by inflated payload; exits 1 if they differ
    Diff {
        old: PathBuf,
        new: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Print `hash name` lines (for --dict) naming unknown PSIN schema hashes from strings in the archives
    Harvest {
        #[arg(required = true)]
        archives: Vec<PathBuf>,
    },
    /// Catalog every entry (with nested RPF9s) of the archives: path, name hash, name or candidates, kind, sizes
    Catalog {
        #[arg(required = true)]
        archives: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
        /// Write confirmed candidates as `hexhash name` lines (deduped, sorted), merging into an existing file
        #[arg(long, value_name = "PATH")]
        write_dict: Option<PathBuf>,
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
        /// Reopen the output and check every entry against the source; fails on any unexpected difference
        #[arg(long)]
        verify: bool,
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
        /// Only entries that `model` can export, with their mesh counts per LOD (slow)
        #[arg(long)]
        drawable: bool,
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
        /// Rank the N heaviest programs by instruction count (a compile-cost proxy)
        #[arg(long, conflicts_with_all = ["summary", "shader"], value_name = "N")]
        heavy: Option<usize>,
        /// Opcode census: each distinct opcode's occurrences and programs using it
        #[arg(long, conflicts_with_all = ["summary", "shader", "heavy"])]
        opcodes: bool,
        /// EXEC-uniformity: share of vector instructions provably under a full EXEC (guard foldable), overall, per stage and for the N heaviest programs (default 30)
        #[arg(long, conflicts_with_all = ["summary", "shader", "heavy", "opcodes"], value_name = "N", num_args = 0..=1, default_missing_value = "30")]
        uniformity: Option<usize>,
        /// Ray-traversal census: BVH intersect sites, their traversal-loop template and loop coverage (see docs/rt_traversal_census.md)
        #[arg(long, conflicts_with_all = ["summary", "shader", "heavy", "opcodes", "uniformity"])]
        rt_census: bool,
        /// With --rt-census: JSON output
        #[arg(long, requires = "rt_census")]
        json: bool,
        /// With --opcodes: only programs of this hardware stage (LS HS ES GS VS PS CS MS)
        #[arg(long, requires = "opcodes")]
        stage: Option<String>,
        /// With --shader: write each program's raw machine code to PREFIX.<n>.<stage>.bin
        #[arg(long, requires = "shader", value_name = "PREFIX")]
        dump: Option<PathBuf>,
    },
    /// Disassemble the RDNA2 machine code of one shader record (LLVM-style syntax)
    Disasm {
        archive: PathBuf,
        /// Entry index; `5/3` descends into nested RPF9s
        index: String,
        /// Shader record to disassemble
        #[arg(long)]
        shader: usize,
        /// Also print each instruction's dwords in hex
        #[arg(long)]
        raw: bool,
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
    /// Export the LOD geometry of one or more drawable entries as one glTF (.glb, or .gltf + .bin) or OBJ scene
    Model {
        archive: PathBuf,
        /// Entry index; `5/3` descends into nested RPF9s. Several are merged into one scene
        #[arg(required = true, num_args = 1..)]
        index: Vec<String>,
        /// glTF output: binary unless the name ends in .gltf (default: model.glb)
        #[arg(short, long, conflicts_with = "obj")]
        output: Option<PathBuf>,
        /// Write Wavefront OBJ here instead
        #[arg(long)]
        obj: Option<PathBuf>,
        /// LOD to export, 0 = highest detail
        #[arg(long, default_value_t = 0)]
        lod: usize,
        /// Texture stores to resolve material textures in (default: every .ttpf next to the archive)
        #[arg(long, num_args = 1..)]
        textures: Vec<PathBuf>,
        /// Archive whose EDUP tables map deduplicated texture keys (default: textures.rpf next to the archive)
        #[arg(long)]
        dedup: Option<PathBuf>,
        /// Write each geometry's stored vertex, index and meshlet buffers and a reference JSON here
        /// (no model is written unless -o or --obj is also given)
        #[arg(long)]
        dump_buffers: Option<PathBuf>,
    },
    /// Check a position buffer captured from a run against a `model --dump-buffers` reference JSON
    CheckPositions {
        /// Reference `g<i>.json`
        reference: PathBuf,
        /// Captured buffer (raw bytes)
        capture: PathBuf,
        /// stored (vertex buffer as stored), float3 (model space) or clip (f32x4 clip positions)
        #[arg(long, default_value = "clip")]
        layout: String,
        /// Bytes between elements (default 12 for float3, 16 for clip)
        #[arg(long)]
        stride: Option<usize>,
        /// Byte offset of the first element (or of the vertex buffer, for stored)
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Elements to check (default: one per vertex, or per meshlet slot)
        #[arg(long)]
        count: Option<usize>,
        /// Elements are in meshlet order (meshlet 0's vertices, then meshlet 1's, ...), not vertex order
        #[arg(long)]
        meshlet_order: bool,
        /// With --meshlet-order: each meshlet occupies this many elements (e.g. 128)
        #[arg(long, requires = "meshlet_order")]
        meshlet_slots: Option<usize>,
        /// Model-space tolerance for the reference and AABB checks
        #[arg(long, default_value_t = 1e-3)]
        tolerance: f64,
        /// Clip-space tolerance for the fit residual, in units of |w|
        #[arg(long, default_value_t = 1e-3)]
        clip_tolerance: f64,
        /// |x/w| or |y/w| above this counts as outlying
        #[arg(long, default_value_t = 16.0)]
        ndc_limit: f64,
        /// Worst offenders to list (0: counts only)
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// List textures in an RDS5 (.ttpf) store, or export one BC1/BC3/BC4/BC5/BC7 texture's mip 0
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
    /// Check every guard and patch precondition against the archive, without writing
    Verify {
        manifest: PathBuf,
        /// Archive to check against (default: the manifest's target_archive)
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Apply the manifest's patches, writing a repacked archive
    Apply {
        manifest: PathBuf,
        #[arg(short, long, default_value = "modded.rpf")]
        output: PathBuf,
        /// Archive to patch (default: the manifest's target_archive)
        #[arg(long)]
        archive: Option<PathBuf>,
        /// Reopen the output and check every entry against the source; fails on any unexpected difference
        #[arg(long)]
        verify: bool,
    },
    /// Print a [[patch]] table that turns an extracted payload into an edited copy of it
    Make {
        /// Entry path the payload was extracted from, as `list --recursive` prints it
        #[arg(long)]
        path: String,
        original: PathBuf,
        edited: PathBuf,
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
        println!("ok: {} patch(es)", m.patch.len());
        return Ok(());
    }
    if let Command::Mod(ModCommand::Make { path, original, edited }) = &cli.command {
        let old = std::fs::read(original).with_context(|| format!("reading {}", original.display()))?;
        let new = std::fs::read(edited).with_context(|| format!("reading {}", edited.display()))?;
        let ops = modding::patch::make(&old, &new)?;
        let carried: usize = ops.iter().map(|op| match op {
            modding::manifest::Op::Text { find, .. } => find.len(),
            modding::manifest::Op::Xml { expect, .. } => expect.as_ref().map_or(0, String::len),
            modding::manifest::Op::Bytes { expect, .. } => expect.len() / 2,
        }).sum();
        eprintln!("{} op(s) carrying {carried} of the original's {} bytes", ops.len(), old.len());
        let p = modding::manifest::Patch { path: path.clone(), base_sha256: Some(modding::patch::sha256_hex(&old)), file: None, ops };
        print!("{}", toml::to_string(&std::collections::BTreeMap::from([("patch", [p])]))?);
        return Ok(());
    }
    if let Command::CheckPositions { reference, capture, layout, stride, offset, count, meshlet_order, meshlet_slots, tolerance, clip_tolerance, ndc_limit, top } = &cli.command {
        use formats::geometry_check as gc;
        let r: gc::Reference = serde_json::from_slice(&std::fs::read(reference).with_context(|| format!("reading {}", reference.display()))?)
            .with_context(|| format!("parsing {}", reference.display()))?;
        let data = std::fs::read(capture).with_context(|| format!("reading {}", capture.display()))?;
        let opts = gc::Options { layout: gc::parse_layout(layout)?, stride: *stride, offset: *offset, count: *count, meshlet_order: *meshlet_order,
            meshlet_slots: *meshlet_slots, tolerance: *tolerance, clip_tolerance: *clip_tolerance, ndc_limit: *ndc_limit, top: *top };
        let stored = reference.parent().map(|d| d.join(&r.files.vertex)).and_then(|p| std::fs::read(p).ok());
        let report = gc::check(&r, &data, stored.as_deref(), &opts)?;
        print!("{}", report.text);
        std::process::exit(if report.bad > 0 { 1 } else { 0 });
    }
    if let Command::Texture { store, extract, decode, output, slice } = &cli.command {
        return cmd_texture(store, *extract, *decode, output.clone(), *slice);
    }
    if let Command::BcpackAnalyze { store, chunk, codec, output, widths } = &cli.command {
        return cmd_bcpack_analyze(store, *chunk, codec.as_deref(), output.as_deref(), widths.as_deref());
    }
    let crypto = rpf9::crypto::CryptoContext::load(&cli.data_dir)
        .context("loading crypto data: set FORGE9_DATA or --data-dir")?;

    match cli.command {
        Command::Info { archive } => cmd_info(&archive, &crypto),
        Command::List { archive, limit, json, recursive } => cmd_list(&archive, &crypto, limit, json, recursive),
        Command::Stats { archive, top, json } => cmd_stats(&archive, &crypto, top, json),
        Command::Diff { old, new, json } => match cmd_diff(&old, &new, &crypto, json) {
            Ok(differ) => std::process::exit(differ as i32),
            Err(e) => { eprintln!("Error: {e:?}"); std::process::exit(2) }
        },
        Command::Harvest { archives } => cmd_harvest(&archives, &crypto, &dict),
        Command::Catalog { archives, json, write_dict } => cmd_catalog(&archives, &crypto, &dict, json, write_dict.as_deref()),
        Command::Extract { archive, output, select } => cmd_extract(&archive, &crypto, &output, select),
        Command::Hexdump { archive, index, limit } => cmd_hexdump(&archive, &crypto, index, limit),
        Command::Replace { archive, index, file, output } => cmd_replace(&archive, &crypto, index, &file, &output),
        Command::Psin { archive, index, structs, limit } => cmd_psin(&archive, &crypto, index, structs, limit, &dict),
        Command::Tree { archive, depth } => cmd_tree(&archive, &crypto, depth),
        Command::Repack { archive, patch, output, verify } => cmd_repack(&archive, &crypto, &patch, &output, verify),
        Command::Search { archive, magic, ext, min_size, grep, drawable } => {
            let filter = SearchFilter { magic: magic.map(String::into_bytes), ext, min_size, grep: grep.map(String::into_bytes), drawable };
            let raw = std::fs::read(&archive)?;
            let hits = search(&raw, &archive.display().to_string(), &crypto, &filter)?;
            println!("{hits} matches");
            Ok(())
        }
        Command::Awc { archive, index } => cmd_awc(&load_entry(&archive, &crypto, &index)?),
        Command::Shaders { archive, index, summary } => cmd_shaders(&load_entry(&archive, &crypto, &index)?, summary),
        Command::Pipelines { archive, index, mesh } => cmd_pipelines(&load_entry(&archive, &crypto, &index)?, mesh),
        Command::Fssm { archive, index, shader, roots } => cmd_fssm(&load_entry(&archive, &crypto, &index)?, shader, roots),
        Command::Isa { archive, index, summary, shader, heavy, opcodes, uniformity, rt_census, json, stage, dump } => {
            let bytes = load_entry(&archive, &crypto, &index)?;
            if rt_census { cmd_isa_rt_census(&bytes, json) } else if let Some(n) = uniformity { cmd_isa_uniformity(&bytes, n) } else if opcodes { cmd_isa_opcodes(&bytes, stage.as_deref()) } else { cmd_isa(&bytes, summary, shader, heavy, dump.as_deref()) }
        }
        Command::Disasm { archive, index, shader, raw } => cmd_disasm(&load_entry(&archive, &crypto, &index)?, shader, raw),
        Command::Bink { archive, index } => cmd_bink(&load_entry(&archive, &crypto, &index)?),
        Command::Resource { archive, index } => cmd_resource(&load_entry(&archive, &crypto, &index)?),
        Command::Model { archive, index, output, obj, lod, textures, dedup, dump_buffers } => {
            let raw = std::fs::read(&archive)?;
            let parts = index.iter().map(|i| Ok((i.as_str(), entry_blocks(&raw, &crypto, i)?))).collect::<Result<Vec<_>>>()?;
            drop(raw);
            if let Some(out) = &dump_buffers {
                for (i, blocks) in &parts {
                    let sub = if parts.len() == 1 { out.clone() } else { out.join(i.replace('/', "_")) };
                    cmd_dump_buffers(&formats::drawable::parse_blocks(blocks, lod)?, &sub)?;
                }
                if output.is_none() && obj.is_none() { return Ok(()); }
            }
            let dir = archive.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
            let dedup = match dedup {
                Some(path) => load_dedup(&path, &crypto)?,
                None => load_dedup(&dir.join("textures.rpf"), &crypto).unwrap_or_default(),
            };
            let textures = if !textures.is_empty() { textures } else {
                let mut found: Vec<PathBuf> = std::fs::read_dir(dir)?.filter_map(|e| Some(e.ok()?.path()))
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ttpf"))).collect();
                found.sort();
                found
            };
            cmd_model(&parts, output, obj, lod, &textures, &dedup, &dict)
        }
        Command::Mod(ModCommand::Apply { manifest, output, archive, verify }) => {
            let m = modding::manifest::Manifest::load(&manifest)?;
            let changes = modding::apply::apply(&m, archive.as_deref(), &crypto, &output)?;
            if !verify { return Ok(()); }
            let changes = changes.into_iter().map(|(k, v)| (k.iter().map(usize::to_string).collect::<Vec<_>>().join("/"), v)).collect();
            let src = std::fs::read(archive.as_ref().unwrap_or(&m.info.target_archive))?;
            let v = verify_written(&src, &std::fs::read(&output)?, &crypto, &changes)?;
            anyhow::ensure!(v.problems.is_empty(), "verify failed: {} problem(s)", v.problems.len());
            Ok(())
        }
        Command::Hash { .. } | Command::Texture { .. } | Command::BcpackAnalyze { .. } | Command::Mod(_) | Command::CheckPositions { .. } => unreachable!(),
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

fn cmd_list(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, limit: usize, json: bool, recursive: bool) -> Result<()> {
    let raw = std::fs::read(path)?;
    let archive = rpf9::reader::Archive::open(&raw, crypto)?;
    let mut rows = Vec::new();
    if recursive {
        walk(&archive, "", crypto, &mut |p, a, e, _| rows.push((p.to_string(), e.clone(), a.sniff(e), e.is_zlib(a.data()))))?;
    } else {
        rows.extend(archive.entries().iter().map(|e| (e.index.to_string(), e.clone(), archive.sniff(e), e.is_zlib(&raw))));
    }
    if json {
        println!("[");
        let total = rows.len().min(limit);
        for (i, (p, entry, ext, _)) in rows.iter().take(limit).enumerate() {
            let comma = if i + 1 >= total { "" } else { "," };
            let decoded = entry.decoded_size.map_or("null".into(), |d| d.to_string());
            println!("  {{\"path\":\"{p}\",\"index\":{},\"offset\":{},\"size\":{},\"decoded\":{decoded},\"flags\":{},\"ext\":\"{ext}\"}}{comma}", entry.index, entry.offset, entry.size, entry.flags);
        }
        println!("]");
    } else {
        for (p, entry, ext, zlib) in rows.iter().take(limit) {
            let zlib = if *zlib { " zlib" } else { "" };
            println!("  #{p:<4} {:#010x}  {:>10} bytes  .{ext}{zlib}", entry.offset, entry.size);
        }
        if rows.len() > limit {
            println!("  … {} more", rows.len() - limit);
        }
    }
    Ok(())
}

/// Calls `f(path, archive, entry, nested)` for every entry, descending into nested RPF9s; `path` is
/// `prefix` + `a/b` as `load_entry` takes it, and `nested` is true for an entry that opened as an RPF9.
fn walk(archive: &rpf9::reader::Archive, prefix: &str, crypto: &rpf9::crypto::CryptoContext,
    f: &mut dyn FnMut(&str, &rpf9::reader::Archive, &rpf9::entry::Entry, bool)) -> Result<()> {
    for entry in archive.entries() {
        let path = format!("{prefix}{}", entry.index);
        let bytes = if archive.sniff(entry) == "rpf" { archive.read_entry(entry).ok() } else { None };
        let nested = match bytes.as_deref().map(|b| rpf9::reader::Archive::open(b, crypto)) {
            Some(Ok(n)) => Some(n),
            Some(Err(e)) => { eprintln!("  {path}: nested open failed: {e}"); None }
            None => None,
        };
        f(&path, archive, entry, nested.is_some());
        if let Some(n) = &nested { walk(n, &format!("{path}/"), crypto, f)?; }
    }
    Ok(())
}

/// Inflated entry content; an entry that does not read whole falls back to its readable head.
fn content(archive: &rpf9::reader::Archive, entry: &rpf9::entry::Entry) -> Vec<u8> {
    archive.read_entry(entry).unwrap_or_else(|_| archive.head(entry, usize::MAX))
}

#[derive(Default)]
struct Stats {
    /// Entries that are not nested archives (those count in `nested`, their entries here).
    entries: usize,
    nested: usize,
    zlib: usize,
    stored: u64,
    inflated: u64,
    /// ext -> (entries, stored, inflated)
    kinds: std::collections::BTreeMap<&'static str, (usize, u64, u64)>,
    /// (inflated, stored, path, ext), largest first
    largest: Vec<(u64, u64, String, &'static str)>,
}

fn stats(archive: &rpf9::reader::Archive, crypto: &rpf9::crypto::CryptoContext) -> Result<Stats> {
    let mut s = Stats::default();
    walk(archive, "", crypto, &mut |path, a, e, nested| {
        if nested { s.nested += 1; return; }
        let (ext, zlib) = (a.sniff(e), e.is_zlib(a.data()));
        let inflated = if zlib { content(a, e).len() as u64 } else { e.raw_len() };
        s.entries += 1;
        s.zlib += zlib as usize;
        s.stored += e.size;
        s.inflated += inflated;
        let k = s.kinds.entry(ext).or_default();
        k.0 += 1;
        k.1 += e.size;
        k.2 += inflated;
        s.largest.push((inflated, e.size, path.to_string(), ext));
    })?;
    s.largest.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    Ok(s)
}

fn cmd_stats(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, top: usize, json: bool) -> Result<()> {
    let raw = std::fs::read(path)?;
    let s = stats(&rpf9::reader::Archive::open(&raw, crypto)?, crypto)?;
    let largest = &s.largest[..top.min(s.largest.len())];
    if json {
        let kinds: Vec<String> = s.kinds.iter().map(|(ext, (n, st, inf))| format!("\"{ext}\":{{\"entries\":{n},\"stored\":{st},\"inflated\":{inf}}}")).collect();
        let largest: Vec<String> = largest.iter().map(|(inf, st, p, ext)| format!("{{\"path\":\"{p}\",\"ext\":\"{ext}\",\"stored\":{st},\"inflated\":{inf}}}")).collect();
        println!("{{\"entries\":{},\"nested_archives\":{},\"zlib\":{},\"stored\":{},\"inflated\":{},", s.entries, s.nested, s.zlib, s.stored, s.inflated);
        println!(" \"kinds\":{{{}}},", kinds.join(","));
        println!(" \"largest\":[{}]}}", largest.join(","));
        return Ok(());
    }
    println!("{} ({} bytes): {} entries ({} zlib), {} nested archives", path.display(), raw.len(), s.entries, s.zlib, s.nested);
    println!("  {} bytes stored, {} inflated", s.stored, s.inflated);
    println!("  {:<8} {:>8} {:>14} {:>14}", "kind", "entries", "stored", "inflated");
    for (ext, (n, st, inf)) in &s.kinds { println!("  .{ext:<7} {n:>8} {st:>14} {inf:>14}"); }
    println!("  {} largest (inflated bytes):", largest.len());
    for (inf, st, p, ext) in largest { println!("  #{p:<10} {inf:>12} bytes ({st} stored) .{ext}"); }
    Ok(())
}

/// A non-archive entry and its inflated payload's digest.
#[derive(Debug, Clone)]
struct Leaf { path: String, digest: u64, size: u64, ext: &'static str, offset: u64 }

/// Every entry except nested archives, which are compared through their entries.
fn leaves(archive: &rpf9::reader::Archive, crypto: &rpf9::crypto::CryptoContext) -> Result<Vec<Leaf>> {
    use std::hash::{Hash, Hasher};
    let mut out = Vec::new();
    walk(archive, "", crypto, &mut |path, a, e, nested| {
        if nested { return; }
        let bytes = content(a, e);
        let mut h = std::hash::DefaultHasher::new();
        bytes.hash(&mut h);
        out.push(Leaf { path: path.to_string(), digest: h.finish(), size: bytes.len() as u64, ext: a.sniff(e), offset: e.offset });
    })?;
    Ok(out)
}

#[derive(Default)]
struct Diff { added: Vec<Leaf>, removed: Vec<Leaf>, changed: Vec<(Leaf, Leaf)>, moved: Vec<(Leaf, Leaf)>, same: usize }

impl Diff {
    fn differ(&self) -> bool { !(self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty() && self.moved.is_empty()) }
}

/// Pairs entries with the same payload (same path: identical, else moved), then the rest by path (changed).
fn diff(old: Vec<Leaf>, new: Vec<Leaf>) -> Diff {
    let mut d = Diff::default();
    let at: std::collections::HashMap<String, usize> = new.iter().enumerate().map(|(i, l)| (l.path.clone(), i)).collect();
    let mut new: Vec<Option<Leaf>> = new.into_iter().map(Some).collect();
    let mut rest = Vec::new();
    for o in old {
        match at.get(&o.path) {
            Some(&i) if new[i].as_ref().is_some_and(|n| n.digest == o.digest) => { new[i] = None; d.same += 1; }
            _ => rest.push(o),
        }
    }
    // Popped from the back, so the first remaining copy of a payload pairs first.
    let mut by_digest: std::collections::HashMap<u64, Vec<usize>> = Default::default();
    for (i, n) in new.iter().enumerate().rev() {
        if let Some(n) = n { by_digest.entry(n.digest).or_default().push(i); }
    }
    let mut unpaired = Vec::new();
    for o in rest {
        match by_digest.get_mut(&o.digest).and_then(Vec::pop) {
            Some(i) => d.moved.push((o, new[i].take().unwrap())),
            None => unpaired.push(o),
        }
    }
    for o in unpaired {
        match at.get(&o.path).and_then(|&i| new[i].take()) {
            Some(n) => d.changed.push((o, n)),
            None => d.removed.push(o),
        }
    }
    d.added = new.into_iter().flatten().collect();
    d
}

/// Prints the differences; true if the archives differ.
fn cmd_diff(old: &PathBuf, new: &PathBuf, crypto: &rpf9::crypto::CryptoContext, json: bool) -> Result<bool> {
    let leaves_of = |path: &PathBuf| -> Result<Vec<Leaf>> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        leaves(&rpf9::reader::Archive::open(&raw, crypto)?, crypto)
    };
    let d = diff(leaves_of(old)?, leaves_of(new)?);
    if json {
        let one = |l: &Leaf| format!("{{\"path\":\"{}\",\"size\":{},\"ext\":\"{}\"}}", l.path, l.size, l.ext);
        let list = |v: &[Leaf]| v.iter().map(one).collect::<Vec<_>>().join(",");
        let changed: Vec<String> = d.changed.iter().map(|(o, n)| format!("{{\"path\":\"{}\",\"old_size\":{},\"new_size\":{},\"ext\":\"{}\"}}", o.path, o.size, n.size, n.ext)).collect();
        let moved: Vec<String> = d.moved.iter().map(|(o, n)| format!("{{\"old_path\":\"{}\",\"new_path\":\"{}\",\"size\":{},\"ext\":\"{}\"}}", o.path, n.path, n.size, n.ext)).collect();
        println!("{{\"identical\":{},\"added\":[{}],\"removed\":[{}],", d.same, list(&d.added), list(&d.removed));
        println!(" \"changed\":[{}],\"moved\":[{}]}}", changed.join(","), moved.join(","));
        return Ok(d.differ());
    }
    for l in &d.removed { println!("- {:<16} {:>12} bytes .{}", l.path, l.size, l.ext); }
    for l in &d.added { println!("+ {:<16} {:>12} bytes .{}", l.path, l.size, l.ext); }
    for (o, n) in &d.changed {
        println!("~ {:<16} {:>12} -> {} bytes ({:+}) .{}", o.path, o.size, n.size, n.size as i64 - o.size as i64, n.ext);
    }
    for (o, n) in &d.moved { println!("> {:<16} -> {} ({} bytes) .{}", o.path, n.path, n.size, n.ext); }
    println!("{} added, {} removed, {} changed, {} moved, {} identical", d.added.len(), d.removed.len(), d.changed.len(), d.moved.len(), d.same);
    Ok(d.differ())
}

/// What `--verify` found re-reading a written archive against its source.
#[derive(Debug, Default)]
struct Verified { checked: usize, intended: usize, confirmed: usize, moved: usize, padded: usize, problems: Vec<String> }

/// Re-reads `out` against `src` and prints a summary: each `changes` path (`a/b`; anything under it may
/// differ) reads back as its content, and every other entry as before, wherever its offset moved.
/// A raw payload whose record stores no size may read back with extra zero padding: counted, not a problem.
fn verify_written(src: &[u8], out: &[u8], crypto: &rpf9::crypto::CryptoContext, changes: &std::collections::BTreeMap<String, Vec<u8>>) -> Result<Verified> {
    let old = leaves(&rpf9::reader::Archive::open(src, crypto)?, crypto)?;
    let new = leaves(&rpf9::reader::Archive::open(out, crypto)?, crypto).context("reopening the written archive")?;
    let covered = |p: &str| changes.keys().any(|c| p == c || p.strip_prefix(c.as_str()).is_some_and(|r| r.starts_with('/')));
    let read = |raw: &[u8], p: &str| entry_blocks(raw, crypto, p).map(|b| b.concat());
    let padded = |a: &[u8], b: &[u8]| a.starts_with(b) && a[b.len()..].iter().all(|&x| x == 0);
    let first = |a: &[u8], b: &[u8]| a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let mut v = Verified { checked: old.len(), intended: changes.len(), ..Default::default() };
    let was: std::collections::HashMap<&str, (u64, u64)> = old.iter().map(|l| (l.path.as_str(), (l.digest, l.offset))).collect();
    v.moved = new.iter().filter(|n| was.get(n.path.as_str()).is_some_and(|&(d, o)| d == n.digest && o != n.offset)).count();
    for (p, want) in changes {
        match read(out, p) {
            Ok(back) if padded(&back, want) => { v.confirmed += 1; v.padded += (back.len() > want.len()) as usize; }
            Ok(back) => v.problems.push(format!("{p}: intended change reads back as {} bytes, not {}; first difference at {:#x}", back.len(), want.len(), first(&back, want))),
            Err(e) => v.problems.push(format!("{p}: intended change missing: {e:#}")),
        }
    }
    let d = diff(old, new);
    for (o, n) in d.changed.iter().filter(|(o, _)| !covered(&o.path)) {
        let (a, b) = (read(src, &o.path).unwrap_or_default(), read(out, &o.path).unwrap_or_default());
        if padded(&a, &b) || padded(&b, &a) { v.padded += 1; continue; }
        v.problems.push(format!("{}: unexpected change, {} -> {} bytes; first difference at {:#x}", o.path, o.size, n.size, first(&a, &b)));
    }
    for (o, n) in d.moved.iter().filter(|(o, n)| !(covered(&o.path) && covered(&n.path))) {
        v.problems.push(format!("{}: unexpected change, content now at {} ({} bytes)", o.path, n.path, n.size));
    }
    for l in d.removed.iter().filter(|l| !covered(&l.path)) { v.problems.push(format!("{}: missing from output ({} bytes)", l.path, l.size)); }
    for l in d.added.iter().filter(|l| !covered(&l.path)) { v.problems.push(format!("{}: unexpected new entry ({} bytes)", l.path, l.size)); }
    for p in &v.problems { println!("  ✗ {p}"); }
    println!("verify: {} entries checked, {}/{} intended changes confirmed, {} moved with identical bytes, {} zero padded, {} unexpected difference(s)",
        v.checked, v.confirmed, v.intended, v.moved, v.padded, v.problems.len() - (v.intended - v.confirmed));
    Ok(v)
}

/// Names for the PSIN schema hashes in `archives` that `dict` does not know: every identifier-like
/// string (`[A-Za-z0-9_]{3,64}`, as found and lowercased) in any of their entries whose joaat matches one.
/// Returns (unknown hashes, names found).
fn harvest(archives: &[rpf9::reader::Archive], crypto: &rpf9::crypto::CryptoContext, dict: &rpf9::hashes::Dict)
    -> Result<(usize, std::collections::BTreeMap<u32, String>)> {
    let mut targets = std::collections::HashSet::new();
    for archive in archives { walk(archive, "", crypto, &mut |_, a, e, nested| {
        if nested || a.head(e, 4) != b"PSIN" { return; }
        let Ok(c) = rpf9::psin::PsinContainer::parse(&content(a, e)) else { return };
        for s in &c.structs {
            targets.insert(s.name_hash);
            for m in &s.members { targets.extend([m.name_hash, m.extra]); }
        }
        for en in &c.enums {
            targets.insert(en.name_hash);
            targets.extend(en.values.iter().filter_map(|v| u32::try_from(v.1).ok()));
        }
    })?; }
    // 0 and 0x100 (anonymous array item) are placeholders, not names.
    targets.retain(|&h| h > 0x100 && dict.name(h).is_none());
    let mut found = std::collections::BTreeMap::new();
    for archive in archives { scan_words(archive, crypto, &mut |_, w, h| {
        if targets.contains(&h) { found.entry(h).or_insert_with(|| w.to_string()); }
    })?; }
    Ok((targets.len(), found))
}

/// Calls `f(path, word, joaat)` for every identifier-like string (`[A-Za-z0-9_]{3,64}`) and dotted
/// identifier run (`[A-Za-z0-9_]+(\.[A-Za-z0-9_]+)+`, 3–64 bytes), as found and lowercased, in the
/// entries that are not nested archives.
fn scan_words(archive: &rpf9::reader::Archive, crypto: &rpf9::crypto::CryptoContext, f: &mut dyn FnMut(&str, &str, u32)) -> Result<()> {
    let mut emit = |path: &str, bytes: &[u8]| {
        if !(3..=64).contains(&bytes.len()) { return; }
        let word = std::str::from_utf8(bytes).unwrap();
        f(path, word, rpf9::hashes::joaat(word));
        let lower = word.to_ascii_lowercase();
        if lower != word { f(path, &lower, rpf9::hashes::joaat(&lower)); }
    };
    walk(archive, "", crypto, &mut |path, a, e, nested| {
        if nested { return; }
        let bytes = content(a, e);
        for run in bytes.split(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.')) {
            let mut i = 0;
            while i < run.len() {
                while i < run.len() && run[i] == b'.' { i += 1; }
                let start = i;
                while i < run.len() && (run[i] != b'.' || (i + 1 < run.len() && run[i + 1] != b'.')) { i += 1; }
                let dotted = &run[start..i];
                if dotted.is_empty() { continue; }
                if dotted.contains(&b'.') {
                    emit(path, dotted);
                    for part in dotted.split(|&b| b == b'.') { emit(path, part); }
                } else {
                    emit(path, dotted);
                }
            }
        }
    })
}

/// One entry of `catalog`: `name` from the dictionary or the embedded resource label (`evidence`
/// says which), else `candidates` as (name, evidence).
#[derive(Debug, Clone)]
struct CatalogRow { archive: String, path: String, hash: Option<u32>, ext: &'static str, stored: u64, inflated: u64,
    name: Option<String>, evidence: &'static str, candidates: Vec<(String, String)> }

impl CatalogRow {
    /// What an author targets: `archive:path`, plus `@hash` when the record carries one.
    fn id(&self) -> String {
        let hash = self.hash.map(|h| format!("@{h:#010x}")).unwrap_or_default();
        format!("{}:{}{hash}", self.archive, self.path)
    }
    fn status(&self) -> &'static str {
        if self.name.is_some() { "resolved" } else if self.candidates.is_empty() { "unresolved" } else { "candidate" }
    }
}

/// Every entry of the (file name, archive) pairs, nested archives and their entries included, named
/// only by the embedded label of drawables and drawable dictionaries.
fn catalog(archives: &[(String, rpf9::reader::Archive)], crypto: &rpf9::crypto::CryptoContext) -> Result<Vec<CatalogRow>> {
    let mut rows = Vec::new();
    for (name, archive) in archives { walk(archive, "", crypto, &mut |path, a, e, nested| {
        let zlib = e.is_zlib(a.data());
        let bytes = if nested && !zlib { Vec::new() } else { content(a, e) };
        let inflated = if zlib { bytes.len() as u64 } else { e.raw_len() };
        let labels = if nested { Vec::new() } else { formats::drawable::labels(&bytes) };
        rows.push(CatalogRow { archive: name.clone(), path: path.to_string(), hash: e.name_hash, ext: a.sniff(e), stored: e.size,
            inflated, name: (!labels.is_empty()).then(|| labels.join(", ")), evidence: "embedded resource label", candidates: Vec::new() });
    })?; }
    Ok(rows)
}

/// Names each row's hash from `dict` (over an embedded label), else, for a row still unnamed, lists as
/// candidates the strings in `archives` whose joaat matches it. A 32-bit match can be a coincidence,
/// so candidates stay candidates.
fn name_rows(rows: &mut [CatalogRow], archives: &[(String, rpf9::reader::Archive)], crypto: &rpf9::crypto::CryptoContext, dict: &rpf9::hashes::Dict) -> Result<()> {
    for r in rows.iter_mut() {
        if let Some(n) = r.hash.and_then(|h| dict.name(h)) { (r.name, r.evidence) = (Some(n.to_string()), "dictionary"); }
    }
    let targets: std::collections::HashSet<u32> = rows.iter().filter(|r| r.name.is_none()).filter_map(|r| r.hash).collect();
    if targets.is_empty() { return Ok(()); }
    let mut found: std::collections::HashMap<u32, Vec<(String, String)>> = Default::default();
    for (name, archive) in archives { scan_words(archive, crypto, &mut |path, w, h| {
        if !targets.contains(&h) { return; }
        let c = found.entry(h).or_default();
        if !c.iter().any(|(n, _)| n == w) { c.push((w.to_string(), format!("string in entry {name}:{path}"))); }
    })?; }
    for r in rows.iter_mut().filter(|r| r.name.is_none()) {
        if let Some(c) = r.hash.and_then(|h| found.get(&h)) { r.candidates = c.clone(); }
    }
    Ok(())
}

/// Confirmed `(hash, name)` pairs from `catalog` rows: unambiguous single-candidate joaat matches
/// and embedded resource labels whose name (or stem before `.#`) hashes to the row's hash.
/// Returns `(entries, skipped_ambiguous)`.
fn confirmed_catalog_names(rows: &[CatalogRow]) -> (Vec<(u32, String)>, usize) {
    let mut out = std::collections::BTreeMap::new();
    let mut ambiguous = std::collections::HashSet::new();
    for r in rows {
        let Some(h) = r.hash else { continue };
        match (&r.name, r.evidence, r.candidates.as_slice()) {
            (Some(labels), "embedded resource label", _) => {
                for label in labels.split(", ") {
                    let stem = label.split_once(".#").map_or(label, |(s, _)| s);
                    if let Some(m) = [label, stem].into_iter().find(|s| rpf9::hashes::joaat(s) == h) {
                        out.entry(h).or_insert_with(|| m.to_string());
                        break;
                    }
                }
            }
            (None, _, [(name, _)]) if rpf9::hashes::joaat(name) == h => {
                out.entry(h).or_insert_with(|| name.clone());
            }
            (None, _, c) if c.len() > 1 => { ambiguous.insert(h); }
            _ => {}
        }
    }
    (out.into_iter().collect(), ambiguous.len())
}

fn cmd_catalog(paths: &[PathBuf], crypto: &rpf9::crypto::CryptoContext, dict: &rpf9::hashes::Dict, json: bool, write_dict: Option<&std::path::Path>) -> Result<()> {
    let raws = paths.iter().map(|p| std::fs::read(p).with_context(|| format!("reading {}", p.display()))).collect::<Result<Vec<_>>>()?;
    let archives = paths.iter().zip(&raws).map(|(p, r)| Ok((p.file_name().unwrap_or_default().to_string_lossy().into_owned(), rpf9::reader::Archive::open(r, crypto)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut rows = catalog(&archives, crypto)?;
    name_rows(&mut rows, &archives, crypto, dict)?;
    if let Some(out_path) = write_dict {
        let (entries, skipped) = confirmed_catalog_names(&rows);
        let (added, total) = rpf9::hashes::write_dict(out_path, entries)?;
        if skipped > 0 {
            eprintln!("wrote {total} dict entries ({added} new, {skipped} ambiguous skipped) to {}", out_path.display());
        } else {
            eprintln!("wrote {total} dict entries ({added} new) to {}", out_path.display());
        }
    }
    if json {
        let records: Vec<_> = rows.iter().map(|r| serde_json::json!({
            "id": r.id(), "archive": r.archive, "path": r.path, "hash": r.hash.map(|h| format!("{h:#010x}")),
            "kind": r.ext, "stored": r.stored, "inflated": r.inflated, "status": r.status(), "name": r.name, "evidence": r.name.as_ref().map(|_| r.evidence),
            "candidates": r.candidates.iter().map(|(n, ev)| serde_json::json!({"name": n, "evidence": ev, "confidence": "exact hash match"})).collect::<Vec<_>>(),
        })).collect();
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }
    println!("  {:<28} {:<10} {:<6} {:>12} {:>12}  name", "id", "hash", "kind", "stored", "inflated");
    for r in &rows {
        let name = match (&r.name, r.candidates.as_slice()) {
            (Some(n), _) => format!("{n} ({})", r.evidence),
            (None, []) => "(unresolved)".into(),
            (None, c) => c.iter().map(|(n, ev)| format!("{n}? ({ev}, exact hash match)")).collect::<Vec<_>>().join(", "),
        };
        let hash = r.hash.map_or("-".into(), |h| format!("{h:#010x}"));
        println!("  {:<28} {hash:<10} .{:<5} {:>12} {:>12}  {name}", r.id(), r.ext, r.stored, r.inflated);
    }
    let count = |s| rows.iter().filter(|r| r.status() == s).count();
    println!("{} entries: {} resolved, {} candidate, {} unresolved ({} without a name hash)",
        rows.len(), count("resolved"), count("candidate"), count("unresolved"), rows.iter().filter(|r| r.hash.is_none()).count());
    Ok(())
}

fn cmd_harvest(paths: &[PathBuf], crypto: &rpf9::crypto::CryptoContext, dict: &rpf9::hashes::Dict) -> Result<()> {
    let raws = paths.iter().map(|p| std::fs::read(p).with_context(|| format!("reading {}", p.display()))).collect::<Result<Vec<_>>>()?;
    let archives = raws.iter().map(|r| rpf9::reader::Archive::open(r, crypto)).collect::<Result<Vec<_>>>()?;
    let (unknown, found) = harvest(&archives, crypto, dict)?;
    for (h, name) in &found { println!("{h:#010x} {name}"); }
    eprintln!("named {} of {unknown} unknown PSIN schema hashes", found.len());
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

fn cmd_repack(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, patches: &[String], output: &PathBuf, verify: bool) -> Result<()> {
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
    rpf9::writer::repack(&raw, crypto, &replacements, output)?;
    if !verify { return Ok(()); }
    let changes = replacements.iter().map(|(i, d)| Ok((i.to_string(), rpf9::reader::decode(d)?))).collect::<Result<_>>()?;
    let v = verify_written(&raw, &std::fs::read(output)?, crypto, &changes)?;
    anyhow::ensure!(v.problems.is_empty(), "verify failed: {} problem(s)", v.problems.len());
    Ok(())
}

fn cmd_psin(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, index: usize, show_structs: bool, limit: usize, dict: &rpf9::hashes::Dict) -> Result<()> {
    let raw = std::fs::read(path)?;
    let archive = rpf9::reader::Archive::open(&raw, crypto)?;
    let entry = archive.entries().get(index)
        .ok_or_else(|| anyhow::anyhow!("entry index {index} out of range"))?;
    let bytes = archive.read_entry(entry)?;

    // Check if it's a nested RPF9 containing PSIN
    if bytes.starts_with(b"9FPR") {
        println!("entry #{index} is a nested RPF9: scanning for PSIN inside...");
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
    drawable: bool,
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
        // Content is only inflated when needed: for grep or drawable, or to recurse.
        let blocks = if (matches && (f.grep.is_some() || f.drawable)) || ext == "rpf" { archive.read_entry_blocks(entry).ok() } else { None };
        let bytes = blocks.as_deref().map(<[Vec<u8>]>::concat);
        let grep_ok = match (&f.grep, &bytes) {
            (None, _) => true,
            (Some(g), Some(b)) => !g.is_empty() && b.windows(g.len()).any(|w| w == g.as_slice()),
            (Some(_), None) => false,
        };
        let lods: Vec<String> = match (&blocks, f.drawable && ext != "rpf") {
            (Some(b), true) => (0..formats::drawable::LOD_COUNT)
                .filter_map(|lod| formats::drawable::parse_blocks(b, lod).ok().map(|d| format!(" LOD{lod}: {} meshes", d.meshes.len())))
                .collect(),
            _ => Vec::new(),
        };
        if matches && grep_ok && (!f.drawable || !lods.is_empty()) {
            hits += 1;
            println!("  {path:<24} {:#010x} {:>10} bytes .{ext}{}", entry.offset, entry.size, lods.concat());
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
    Ok(load_entry_blocks(path, crypto, index)?.concat())
}

/// Like `load_entry`, keeping the final entry's inflated zlib blocks apart.
fn load_entry_blocks(path: &PathBuf, crypto: &rpf9::crypto::CryptoContext, index: &str) -> Result<Vec<Vec<u8>>> {
    entry_blocks(&std::fs::read(path)?, crypto, index)
}

/// `load_entry_blocks` on archive bytes already in memory.
fn entry_blocks(root: &[u8], crypto: &rpf9::crypto::CryptoContext, index: &str) -> Result<Vec<Vec<u8>>> {
    let mut blocks: Option<Vec<Vec<u8>>> = None;
    for part in index.split(['/', '#']).filter(|p| !p.is_empty()) {
        let i: usize = part.parse().with_context(|| format!("bad entry index {part:?}"))?;
        let nested = blocks.take().map(|b| b.concat());
        let archive = rpf9::reader::Archive::open(nested.as_deref().unwrap_or(root), crypto)?;
        let entry = archive.entries().get(i).ok_or_else(|| anyhow::anyhow!("entry index {i} out of range"))?;
        blocks = Some(archive.read_entry_blocks(entry)?);
    }
    Ok(blocks.unwrap_or_else(|| vec![root.to_vec()]))
}

fn cmd_awc(bytes: &[u8]) -> Result<()> {
    let awc = formats::awc::parse(bytes)?;
    println!("AWC v{} flags {:#06x} header {:#x}: {} streams", awc.version, awc.flags, awc.header_size, awc.streams.len());
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
    use formats::drawable;
    let r = formats::resource::parse(bytes)?;
    let magic = r.magic_at.map_or("none".to_string(), |m| format!("0DRP at {m:#x}"));
    let version = r.version.map_or(String::new(), |v| format!(" version {v:#x}"));
    println!("{:?} resource, {} bytes, magic {magic}{version}", r.kind, bytes.len());
    println!("  pointer-like words: {} virtual, {} physical", r.virtual_ptrs, r.physical_ptrs);
    if let Some((s, g)) = drawable::page_counts(bytes) { println!("  page map: {s} system, {g} graphics pages"); }
    if let Some(members) = drawable::dictionary(bytes) {
        println!("  drawable dictionary, {} drawables (export with `model`)", members.len());
    } else if drawable::looks_like(bytes) {
        println!("  standalone drawable (export with `model`)");
    }
    Ok(())
}

/// Key -> stored key from every EDUP entry at the top level of the archive at `path`.
fn load_dedup(path: &std::path::Path, crypto: &rpf9::crypto::CryptoContext) -> Result<std::collections::HashMap<u64, u64>> {
    let raw = std::fs::read(path)?;
    let archive = rpf9::reader::Archive::open(&raw, crypto)?;
    let mut map = std::collections::HashMap::new();
    for e in archive.entries().iter().filter(|e| archive.head(e, 4) == b"EDUP") {
        map.extend(formats::texture::edup(&archive.read_entry(e)?).with_context(|| format!("entry {}: bad EDUP table", e.index))?);
    }
    Ok(map)
}

fn cmd_dump_buffers(d: &formats::drawable::Drawable, dir: &std::path::Path) -> Result<()> {
    for s in &d.skipped { eprintln!("  skipped {s}"); }
    for (prefix, r) in formats::geometry_check::dump(d, dir)? {
        let attrs: Vec<String> = r.attributes.iter().map(|a| format!("{}:{}", a.semantic, a.name)).collect();
        println!("  {prefix} {}: {} vertices, {} triangles, {} meshlets, position format {}, vertex buffer {} bytes, index buffer {} bytes, semantics {}",
            r.geometry, r.vertex_count, r.triangle_count, r.meshlets.len(), r.position.format, r.vertex_buffer.bytes, r.index_buffer.bytes, attrs.join(" "));
    }
    println!("{} LOD{}: {} geometries → {}", d.name, d.lod, d.meshes.len(), dir.display());
    Ok(())
}

/// Exports the drawables of `parts` (entry index, blocks) as one scene.
fn cmd_model(parts: &[(&str, Vec<Vec<u8>>)], output: Option<PathBuf>, obj: Option<PathBuf>, lod: usize, stores: &[PathBuf],
    dedup: &std::collections::HashMap<u64, u64>, dict: &rpf9::hashes::Dict) -> Result<()> {
    use formats::{drawable, texture};
    let mut ds = parts.iter().map(|(i, b)| drawable::parse_blocks(b, lod).with_context(|| format!("entry {i}"))).collect::<Result<Vec<_>>>()?;
    let mut d = if ds.len() == 1 { ds.pop().unwrap() } else { drawable::merge(format!("{} entries", ds.len()), ds) };
    for s in &d.skipped { eprintln!("  skipped {s}"); }
    let mut opened = Vec::new();
    for path in stores {
        match texture::LazyStore::open(path) {
            Ok(s) => opened.push((path.file_name().unwrap_or_default().to_string_lossy().into_owned(), s)),
            Err(e) => eprintln!("  skipping store {}: {e:#}", path.display()),
        }
    }
    let total: usize = opened.iter().map(|(_, s)| s.textures.len()).sum();
    if !dedup.is_empty() { println!("  {} deduplicated texture keys", dedup.len()); }
    let mut resolved = 0;
    for m in &mut d.materials {
        println!("  material {} (shader {:#010x}): {} textures", m.name, m.shader, m.textures.len());
        for t in &m.textures {
            // A deduplicated key's pixels are stored under the key EDUP maps it to.
            let direct = opened.iter().any(|(_, s)| s.textures.iter().any(|x| x.hash == t.key));
            let stored = if direct { None } else { dedup.get(&t.key).copied() };
            let key = stored.unwrap_or(t.key);
            let hit = opened.iter_mut().find_map(|(name, s)| s.textures.iter().position(|x| x.hash == key).map(|i| (&*name, s, i)));
            let what = match hit {
                None => format!("not in {} stores ({total} textures)", opened.len()),
                Some((name, s, i)) => {
                    resolved += 1;
                    let tex = s.textures[i].clone();
                    let slot = match t.param {
                        drawable::DIFFUSE_PARAM => Some(&mut m.base_color),
                        drawable::BUMP_PARAM => Some(&mut m.normal),
                        drawable::EMISSIVE_PARAM => Some(&mut m.emissive),
                        _ => None,
                    };
                    let used = match slot.map(|slot| s.load(&tex).and_then(|()| texture::material_png(&s.store, &tex)).map(|png| *slot = Some(png))) {
                        None => String::new(),
                        Some(Ok(())) => ", embedded".into(),
                        Some(Err(e)) => format!(", not decoded: {e:#}"),
                    };
                    let via = stored.map_or(String::new(), |k| format!(" (dedup {k:016x})"));
                    format!("{name} #{} {}x{} {}{via}{used}", tex.index, tex.width, tex.height, texture::dxgi_name(tex.dxgi))
                }
            };
            println!("    {} name {:#010x} key {:016x}: {what}", dict.fmt(t.param), t.name_hash, t.key);
        }
    }
    let refs: usize = d.materials.iter().map(|m| m.textures.len()).sum();
    if refs > 0 { println!("  {resolved} of {refs} texture references resolved"); }
    let out = match obj {
        Some(path) => { std::fs::write(&path, drawable::to_obj(&d))?; path }
        None => {
            let path = output.unwrap_or_else(|| PathBuf::from("model.glb"));
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gltf")) {
                let bin = path.with_extension("bin");
                let (json, data) = drawable::to_gltf(&d, Some(&bin.file_name().unwrap().to_string_lossy()));
                std::fs::write(&path, json)?;
                std::fs::write(&bin, data)?;
            } else {
                std::fs::write(&path, drawable::to_glb(&d))?;
            }
            path
        }
    };
    let verts: usize = d.meshes.iter().map(|m| m.positions.len()).sum();
    let tris: usize = d.meshes.iter().map(|m| m.indices.len() / 3).sum();
    let uv = d.meshes.iter().filter(|m| m.uvs.is_some()).count();
    let normals = d.meshes.iter().filter(|m| m.normals.is_some()).count();
    let colors = d.meshes.iter().filter(|m| m.colors.is_some()).count();
    println!("{} LOD{}: {} meshes ({normals} with normals, {uv} with UVs, {colors} with colors), {} materials, {verts} vertices, {tris} triangles, {} skipped → {}",
        d.name, d.lod, d.meshes.len(), d.materials.len(), d.skipped.len(), out.display());
    for m in &d.meshes {
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &m.positions { for k in 0..3 { min[k] = min[k].min(p[k]); max[k] = max[k].max(p[k]); } }
        println!("  {}: {} vertices, {} triangles, AABB {min:?}..{max:?}", m.name, m.positions.len(), m.indices.len() / 3);
    }
    Ok(())
}

fn cmd_texture(path: &PathBuf, extract: Option<usize>, decode: Option<usize>, output: Option<PathBuf>, slice: u32) -> Result<()> {
    use formats::texture;
    let mut lazy = texture::LazyStore::open(path).with_context(|| format!("opening {}", path.display()))?;
    if let Some(i) = extract.or(decode) {
        let tex = lazy.textures.get(i).cloned().ok_or_else(|| anyhow::anyhow!("texture {i} out of range ({} textures)", lazy.textures.len()))?;
        lazy.load(&tex)?;
        let (store, tex) = (&lazy.store, &tex);
        let png = decode.is_some();
        let out = output.unwrap_or_else(|| PathBuf::from(format!("texture_{i}.{}", if png { "png" } else { "dds" })));
        let (bytes, unfitted) = texture::export(store, tex, png, slice)?;
        std::fs::write(&out, bytes)?;
        let sl = if tex.array_size > 1 { format!(" slice {slice}") } else { String::new() };
        println!("texture {i}: {} {}x{}{sl} mip 0 → {}", texture::dxgi_name(tex.dxgi), tex.width, tex.height, out.display());
        if unfitted > 0 { println!("  warning: {unfitted} blocks depend on table values bcpack-rs marks undetermined and may be off"); }
        return Ok(());
    }
    let (store, texs) = (&lazy.store, &lazy.textures);
    let mut codecs = std::collections::BTreeMap::new();
    for c in &store.chunks { *codecs.entry(texture::codec_name(c.codec)).or_insert(0usize) += 1; }
    let codecs: Vec<String> = codecs.iter().map(|(k, n)| format!("{n} {k}")).collect();
    println!("{}: {} chunks ({}), {} textures", path.display(), store.chunks.len(), codecs.join(", "), texs.len());
    for t in texs {
        let array = if t.array_size > 1 { format!(" x{}", t.array_size) } else { String::new() };
        println!("  #{:<5} {:>5}x{:<5}{array} {:<5} (dxgi {:>2}) mips {:>2} tile {}  hash {:016x}", t.index, t.width, t.height, texture::dxgi_name(t.dxgi), t.dxgi, t.mips, t.tile_mode, t.hash);
    }
    Ok(())
}

/// Each FSSM stage of one SGD5 record with its raw machine code.
fn record_code(sgd: &[u8], rec: &formats::sgd5::Shader) -> Result<Vec<(formats::fssm::Stage, Vec<u8>)>> {
    let mut out = Vec::new();
    for x in formats::xbch::sections(&rec.dxbc(sgd)?)? {
        let blob = formats::xbch::decompress(x)?;
        for s in formats::fssm::parse(&blob)?.stages {
            let code = blob.get(s.code_offset as usize..(s.code_offset + s.code_size) as usize)
                .with_context(|| format!("code {:#x}+{:#x} past the {:#x}-byte FSSM", s.code_offset, s.code_size, blob.len()))?;
            out.push((s, code.to_vec()));
        }
    }
    Ok(out)
}

/// Each FSSM stage of one SGD5 record with its classified machine code.
fn record_isa(sgd: &[u8], rec: &formats::sgd5::Shader) -> Result<Vec<(formats::fssm::Stage, formats::rdna2::Stats)>> {
    Ok(record_code(sgd, rec)?.into_iter().map(|(s, c)| { let st = formats::rdna2::analyze(&c); (s, st) }).collect())
}

/// Every distinct opcode across the entry's programs, most executed first.
fn cmd_isa_opcodes(bytes: &[u8], stage: Option<&str>) -> Result<()> {
    use formats::rdna2::{opcode_label, opcode_name, Family};
    use std::collections::{BTreeMap, BTreeSet};
    let records = formats::sgd5::parse(bytes)?;
    // (family, op) -> (occurrences, programs); stage -> (programs, distinct opcodes).
    let mut ops: BTreeMap<(Family, u16), (u64, u64)> = BTreeMap::new();
    let mut by_stage: BTreeMap<&str, (u64, BTreeSet<(Family, u16)>)> = BTreeMap::new();
    let (mut programs, mut failed) = (0u64, 0);
    for rec in &records {
        let Ok(progs) = record_isa(bytes, rec) else { failed += 1; continue };
        for (s, st) in progs.iter().filter(|(s, _)| stage.is_none_or(|w| s.name().eq_ignore_ascii_case(w))) {
            programs += 1;
            let e = by_stage.entry(s.name()).or_default();
            e.0 += 1;
            for (&k, &n) in &st.opcodes {
                let o = ops.entry(k).or_default();
                o.0 += n as u64;
                o.1 += 1;
                if k.0 != Family::Unknown { e.1.insert(k); }
            }
        }
    }
    let unknown = ops.remove(&(Family::Unknown, 0)).unwrap_or_default();
    let mut rows: Vec<_> = ops.into_iter().collect();
    rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
    let total: u64 = rows.iter().map(|r| r.1.0).sum();
    let unnamed = rows.iter().filter(|((f, o), _)| opcode_name(*f, *o).is_none()).count();
    let cover = |pct: f64| {
        let mut cum = 0;
        rows.iter().position(|r| { cum += r.1.0; cum as f64 >= pct / 100.0 * total as f64 }).map_or(0, |i| i + 1)
    };
    println!("{} records ({failed} without a readable FSSM program), {programs} programs{}", records.len(), stage.map(|s| format!(" of stage {s}")).unwrap_or_default());
    println!("{} distinct opcodes, {total} instructions; {unnamed} opcodes unassigned in GFX10.3 (synthetic names); {} undecodable dwords in {} programs",
        rows.len(), unknown.0, unknown.1);
    println!("{} opcodes cover 90% of instructions, {} cover 99%, {} cover 99.9%", cover(90.0), cover(99.0), cover(99.9));
    println!("distinct opcodes by stage:{}", by_stage.iter().map(|(s, (n, set))| format!(" {s}={} ({n} programs)", set.len())).collect::<String>());
    println!("  rank  occurrences  share    cum%  programs  family  opcode");
    let mut cum = 0;
    for (i, ((f, o), (n, p))) in rows.iter().enumerate() {
        cum += n;
        println!("  {:>4} {n:>12} {:>5.2}% {:>6.2}% {p:>9}  {:<7} {}", i + 1, 100.0 * *n as f64 / total as f64, 100.0 * cum as f64 / total as f64, f.name(), opcode_label(*f, *o));
    }
    Ok(())
}

/// EXEC-uniform vs guarded vector instructions: overall, by stage, by size band and for the `top` heaviest programs.
fn cmd_isa_uniformity(bytes: &[u8], top: usize) -> Result<()> {
    use formats::uniformity::{analyze, Uniformity};
    use std::collections::BTreeMap;
    let records = formats::sgd5::parse(bytes)?;
    let mut all: Vec<(u32, &str, bool, Uniformity, &str)> = Vec::new();
    let mut failed = 0;
    for rec in &records {
        let Ok(progs) = record_code(bytes, rec) else { failed += 1; continue };
        for (s, code) in progs {
            all.push((formats::rdna2::analyze(&code).insts, s.name(), s.wave32, analyze(&code, s.wave32), &rec.name));
        }
    }
    let pct = |u: &Uniformity| 100.0 * u.uniform as f64 / u.vector.max(1) as f64;
    let per_run = |u: &Uniformity| u.guarded() as f64 / u.guard_runs.max(1) as f64;
    let add = |a: &mut Uniformity, b: &Uniformity| { a.vector += b.vector; a.uniform += b.uniform; a.guard_runs += b.guard_runs; };
    let line = |label: &str, n: u64, u: &Uniformity| println!("  {label:<10} {n:>8} {:>12} {:>12} {:>7.1}% {:>7.1}% {:>12} {:>9.1}",
        u.vector, u.uniform, pct(u), 100.0 - pct(u), u.guard_runs, per_run(u));
    let mut total = Uniformity::default();
    let mut stages: BTreeMap<&str, (u64, Uniformity)> = BTreeMap::new();
    const BANDS: [(u32, &str); 5] = [(0, "<2k"), (2_000, "2k-8k"), (8_000, "8k-20k"), (20_000, "20k-50k"), (50_000, ">=50k")];
    let mut bands = [(0u64, Uniformity::default()); BANDS.len()];
    for (insts, stage, _, u, _) in &all {
        add(&mut total, u);
        let e = stages.entry(stage).or_default();
        e.0 += 1;
        add(&mut e.1, u);
        let b = &mut bands[BANDS.iter().rposition(|(lo, _)| insts >= lo).unwrap()];
        b.0 += 1;
        add(&mut b.1, u);
    }
    println!("{} records ({failed} without a readable FSSM program), {} programs", records.len(), all.len());
    println!("uniform = vector instructions provably under a full EXEC (guard foldable); guard runs = guards left if one covers each straight-line run under one EXEC");
    println!("  {:<10} {:>8} {:>12} {:>12} {:>8} {:>8} {:>12} {:>9}", "", "programs", "vector", "uniform", "uniform", "guarded", "guard runs", "insts/run");
    line("all", all.len() as u64, &total);
    println!("by stage:");
    for (s, (n, u)) in &stages { line(s, *n, u); }
    println!("by program size (instructions):");
    for ((_, label), (n, u)) in BANDS.iter().zip(&bands) { line(label, *n, u); }
    all.sort_by(|a, b| b.0.cmp(&a.0));
    println!("{} heaviest programs:", top.min(all.len()));
    println!("  {:>7} {:>7} {:>8} {:>8} {:>10} {:>9}  stage  shader", "insts", "vector", "uniform", "guarded", "guard runs", "insts/run");
    for (insts, stage, w32, u, name) in all.iter().take(top) {
        println!("  {insts:>7} {:>7} {:>7.1}% {:>7.1}% {:>10} {:>9.1}  {stage} {}  {name}",
            u.vector, pct(u), 100.0 - pct(u), u.guard_runs, per_run(u), if *w32 { "w32" } else { "w64" });
    }
    Ok(())
}

/// BVH intersect sites per program, their traversal template and innermost-loop coverage.
fn cmd_isa_rt_census(bytes: &[u8], json: bool) -> Result<()> {
    use formats::rdna2::Family;
    use formats::rt_census::{census, Census, PUSH_WINDOW, SORT_WINDOW};
    use std::collections::BTreeMap;
    let records = formats::sgd5::parse(bytes)?;
    // (insts, stage, name, census of programs with a site)
    let mut all: Vec<(u32, &str, &str, Option<Census>)> = Vec::new();
    let mut stages: BTreeMap<&str, u64> = BTreeMap::new();
    let mut failed = 0;
    for rec in &records {
        let Ok(progs) = record_code(bytes, rec) else { failed += 1; continue };
        for (s, code) in progs {
            let st = formats::rdna2::analyze(&code);
            *stages.entry(s.name()).or_default() += 1;
            let bvh = st.opcodes.contains_key(&(Family::Mimg, 0xe6)) || st.opcodes.contains_key(&(Family::Mimg, 0xe7));
            all.push((st.insts, s.name(), &rec.name, bvh.then(|| census(&code, s.wave32))));
        }
    }
    let hits: Vec<_> = all.iter().filter_map(|(n, st, name, c)| c.as_ref().map(|c| (*n, *st, *name, c))).collect();
    let count = |f: &dyn Fn(&formats::rt_census::Site) -> bool| hits.iter().map(|h| h.3.sites.iter().filter(|s| f(s)).count()).sum::<usize>();
    let progs = |b64: bool| hits.iter().filter(|h| h.3.sites.iter().any(|s| s.bvh64 == b64)).count();
    let sites = count(&|_| true);
    let insts: u64 = hits.iter().map(|h| h.0 as u64).sum();
    let in_loop: u64 = hits.iter().map(|h| h.3.in_loop as u64).sum();
    let template = |s: &formats::rt_census::Site| s.sort && s.push.is_some() && s.ring.is_some() && s.node_type && s.loop_span.is_some();
    let mut checks: Vec<(String, usize)> = vec![
        ("sort".into(), count(&|s| s.sort)), ("push".into(), count(&|s| s.push.is_some())),
        ("push_ds_write_b32".into(), count(&|s| s.push == Some("ds_write_b32"))), ("push_ds_write2_b32".into(), count(&|s| s.push == Some("ds_write2_b32"))),
        ("ring".into(), count(&|s| s.ring.is_some())),
    ];
    let mut masks: BTreeMap<u32, usize> = BTreeMap::new();
    for h in &hits { for m in h.3.sites.iter().filter_map(|s| s.ring) { *masks.entry(m).or_default() += 1; } }
    checks.extend(masks.iter().map(|(m, n)| (format!("ring_{m:#x}"), *n)));
    checks.extend([
        ("node_type".into(), count(&|s| s.node_type)), ("cap".into(), count(&|s| s.cap)), ("loop".into(), count(&|s| s.loop_span.is_some())),
        ("template".into(), count(&template)), ("template_programs".into(), hits.iter().filter(|h| h.3.sites.iter().all(template)).count()),
    ]);
    let pct = |a: u64, b: u64| 100.0 * a as f64 / b.max(1) as f64;
    let mut by_size: Vec<_> = all.iter().collect();
    by_size.sort_by(|a, b| b.0.cmp(&a.0));
    let site_count = |c: &Option<Census>| c.as_ref().map_or(0, |c| c.sites.len());
    if json {
        let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let stages: Vec<String> = stages.iter().map(|(s, n)| format!("\"{s}\":{n}")).collect();
        let checks: Vec<String> = checks.iter().map(|(k, n)| format!("\"{k}\":{n}")).collect();
        let rows: Vec<String> = hits.iter().map(|(n, st, name, c)| {
            let b64 = c.sites.iter().filter(|s| s.bvh64).count();
            format!("{{\"name\":\"{}\",\"stage\":\"{st}\",\"insts\":{n},\"bvh\":{},\"bvh64\":{b64},\"in_loop\":{},\"template\":{},\"cap\":{}}}",
                q(name), c.sites.len() - b64, c.in_loop, c.sites.iter().filter(|s| template(s)).count(), c.sites.iter().filter(|s| s.cap).count())
        }).collect();
        let heavy: Vec<String> = by_size.iter().take(10).map(|(n, st, name, c)| format!("{{\"name\":\"{}\",\"stage\":\"{st}\",\"insts\":{n},\"sites\":{}}}", q(name), site_count(c))).collect();
        println!("{{\"records\":{},\"unreadable\":{failed},\"programs\":{},\"stages\":{{{}}},", records.len(), all.len(), stages.join(","));
        println!(" \"sites\":{sites},\"bvh_sites\":{},\"bvh_programs\":{},\"bvh64_sites\":{},\"bvh64_programs\":{},\"programs_with_sites\":{},",
            count(&|s| !s.bvh64), progs(false), count(&|s| s.bvh64), progs(true), hits.len());
        println!(" \"insts\":{insts},\"in_loop\":{in_loop},\"windows\":{{\"sort\":{SORT_WINDOW},\"push\":{PUSH_WINDOW}}},\"checks\":{{{}}},", checks.join(","));
        println!(" \"by_program\":[{}],", rows.join(","));
        println!(" \"heaviest\":[{}]}}", heavy.join(","));
        return Ok(());
    }
    println!("{} records ({failed} without a readable FSSM program), {} programs ({})", records.len(), all.len(),
        stages.iter().map(|(s, n)| format!("{s} {n}")).collect::<Vec<_>>().join(", "));
    println!("{sites} BVH intersect sites in {} programs: image_bvh_intersect_ray {} in {} programs, image_bvh64_intersect_ray {} in {} programs",
        hits.len(), count(&|s| !s.bvh64), progs(false), count(&|s| s.bvh64), progs(true));
    println!("per site: sort = v_min3/v_med3/v_max3_u32 within {SORT_WINDOW} instructions after; push = ds_write_b32 or ds_write2_b32 and ring = v_and_b32 with mask 4*(2^k-1) within {PUSH_WINDOW} after;");
    println!("  loop = innermost backward branch around it; node_type = v_and_b32 .., 7 in that loop; cap = s_cmpk_lt_u32 sN, 0xffff in that loop; template = sort+push+ring+node_type+loop");
    let (last, per_site) = checks.split_last().unwrap();
    for (k, n) in per_site { println!("  {k:<18} {n:>6} of {sites} sites ({:.1}%)", pct(*n as u64, sites as u64)); }
    println!("  {} of {} programs have every site on the template", last.1, hits.len());
    println!("{insts} instructions in these programs, {in_loop} ({:.1}%) in an innermost loop around a site", pct(in_loop, insts));
    println!("  {:>7} {:>5} {:>5} {:>7} {:>8} {:>4}  stage  shader", "insts", "bvh", "bvh64", "in loop", "template", "cap");
    let mut rows = hits.clone();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    for (n, st, name, c) in rows {
        let b64 = c.sites.iter().filter(|s| s.bvh64).count();
        println!("  {n:>7} {:>5} {b64:>5} {:>6.1}% {:>8} {:>4}  {st}  {name}", c.sites.len() - b64, pct(c.in_loop as u64, n as u64),
            c.sites.iter().filter(|s| template(s)).count(), c.sites.iter().filter(|s| s.cap).count());
    }
    println!("10 heaviest programs:");
    for (n, st, name, c) in by_size.iter().take(10) { println!("  {n:>7} {:>4} sites  {st}  {name}", site_count(c)); }
    Ok(())
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

fn cmd_isa(bytes: &[u8], summary: bool, shader: Option<usize>, heavy: Option<usize>, dump: Option<&std::path::Path>) -> Result<()> {
    use formats::rdna2::{FAMILIES, WAVE_OPS, WaveOp};
    use std::collections::BTreeMap;
    let records = formats::sgd5::parse(bytes)?;
    if let Some(n) = heavy {
        // Rank programs by instruction count: the heaviest drive DXC/driver compile cost.
        let mut all: Vec<(u32, &str, bool, String)> = Vec::new();
        for rec in &records {
            if let Ok(progs) = record_isa(bytes, rec) {
                for (s, st) in progs {
                    all.push((st.insts, s.name(), s.wave32, rec.name.clone()));
                }
            }
        }
        all.sort_by(|a, b| b.0.cmp(&a.0));
        println!("{} programs; {} heaviest by instruction count (compile-cost proxy):", all.len(), n.min(all.len()));
        for (insts, stage, w32, name) in all.iter().take(n) {
            println!("  {:>7}  {}  {}  {}", insts, stage, if *w32 { "w32" } else { "w64" }, name);
        }
        return Ok(());
    }
    if let Some(i) = shader {
        let rec = records.get(i).ok_or_else(|| anyhow::anyhow!("shader {i} out of range ({} records)", records.len()))?;
        println!("#{i} {} (tag {:#04x})", rec.name, rec.tag);
        if let Some(prefix) = dump {
            for (n, (s, code)) in record_code(bytes, rec)?.iter().enumerate() {
                let out = format!("{}.{n}.{}.bin", prefix.display(), s.name());
                std::fs::write(&out, code)?;
                println!("  program {n}: {} {} {} bytes → {out}", s.name(), wave_name(s), code.len());
            }
            return Ok(());
        }
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

fn cmd_disasm(bytes: &[u8], shader: usize, raw: bool) -> Result<()> {
    use std::io::Write;
    let records = formats::sgd5::parse(bytes)?;
    let rec = records.get(shader).ok_or_else(|| anyhow::anyhow!("shader {shader} out of range ({} records)", records.len()))?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "#{shader} {} (tag {:#04x})", rec.name, rec.tag)?;
    for (n, (s, code)) in record_code(bytes, rec)?.iter().enumerate() {
        let (st, lines) = formats::rdna2_disasm::disassemble(code, s.wave32);
        writeln!(out, "program {n}: {} {} code {:#x}+{:#x}: {} instructions, {} data dwords, {}",
            s.name(), wave_name(s), s.code_offset, s.code_size, st.insts, st.data_dwords, isa_status(&st))?;
        for l in &lines {
            if raw {
                let words: Vec<String> = l.words.iter().map(|w| format!("{w:08x}")).collect();
                writeln!(out, "  {:#06x}  {:<26}  {}", l.addr, words.join(" "), l.text)?;
            } else {
                writeln!(out, "  {:#06x}  {}", l.addr, l.text)?;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use forge9::rpf9::{crypto::{CryptoContext, TAG_IV}, header::HEADER_SIZE, hashes::joaat, reader::Archive};

    /// Stand-in crypto material: identity S-boxes and T-tables that put each input byte in its
    /// own lane of the output word (`table % 4` differs within every round word).
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

    /// An archive of `payloads` in the stored record layout `{offset/16, ceil(size/16), size}`.
    fn archive(crypto: &CryptoContext, payloads: &[Vec<u8>]) -> Vec<u8> { named_archive(crypto, payloads, &[]) }

    /// `archive`, with a TOC column directory before the records placing `names` as the name hash
    /// column after them.
    fn named_archive(crypto: &CryptoContext, payloads: &[Vec<u8>], names: &[u32]) -> Vec<u8> {
        let records = payloads.len() * 24;
        let mut toc = Vec::new();
        if !names.is_empty() {
            for (id, count, off, stride) in [(0x33f7_0dfa_f2c5_0989u64, payloads.len(), 0, 24u64), (0xb707_6b62_45d3_5fb5, names.len(), records, 4)] {
                toc.extend([id.to_le_bytes(), ((count as u64) | (off as u64) << 32).to_le_bytes(), stride.to_le_bytes()].concat());
            }
        }
        let base = toc.len();
        toc.resize(base + records, 0);
        toc.extend(names.iter().flat_map(|h| h.to_le_bytes()));
        toc.resize((toc.len() + 16).next_multiple_of(16), 0);
        let mut data = Vec::new();
        for (i, p) in payloads.iter().enumerate() {
            let at = (HEADER_SIZE + toc.len() + data.len()) as u64;
            for (k, v) in [at / 16, (p.len() as u64).div_ceil(16), rpf9::reader::decode(p).unwrap().len() as u64].iter().enumerate() {
                toc[base + i * 24 + k * 8..base + i * 24 + k * 8 + 8].copy_from_slice(&v.to_le_bytes());
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

    fn zlib(data: &[u8], level: u32) -> Vec<u8> {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(level));
        z.write_all(data).unwrap();
        z.finish().unwrap()
    }

    /// `[xml, rpf[a, b], zlib(c)]`
    fn sample(crypto: &CryptoContext, b: &[u8], level: u32) -> Vec<u8> {
        let nested = archive(crypto, &[b"alpha".to_vec(), b.to_vec()]);
        archive(crypto, &[b"<?xml version='1.0'?><a/>".to_vec(), nested, zlib(&[b'c'; 1000], level)])
    }

    #[test]
    fn two_part_entry_reads_both_parts_from_its_own_span() {
        // 0x10100 bytes: a 64 KiB zlib part and a 0x100 part, both in entry 0's stored span,
        // then a neighbour that is not a continuation.
        let crypto = crypto();
        let content: Vec<u8> = (0..0x10100u32).map(|i| (i * 7 % 251) as u8).collect();
        let chain = rpf9::writer::zlib_chain(&content).unwrap();
        let raw = archive(&crypto, &[chain.clone(), b"next".to_vec()]);
        let a = Archive::open(&raw, &crypto).unwrap();
        let e = &a.entries()[0];
        assert_eq!((e.size, e.decoded_size), (chain.len() as u64, Some(0x10100)));
        let parts = a.read_entry_blocks(e).unwrap();
        assert_eq!(parts.iter().map(Vec::len).collect::<Vec<_>>(), [0x10000, 0x100]);
        assert_eq!(parts.concat(), content);
        assert_eq!(a.read_entry(&a.entries()[1]).unwrap(), b"next");
        // A record declaring only the first part's length is reported, not silently cut.
        let mut bad = raw.clone();
        let toc = &mut bad[HEADER_SIZE..HEADER_SIZE + 64];
        crypto.schedule(0).unwrap().decrypt_cbc(toc, &TAG_IV);
        toc[16..24].copy_from_slice(&0x10000u64.to_le_bytes());
        crypto.schedule(0).unwrap().encrypt_cbc(toc, &TAG_IV);
        let a = Archive::open(&bad, &crypto).unwrap();
        let err = a.read_entry(&a.entries()[0]).unwrap_err().to_string();
        assert_eq!(err, "entry #0 inflates to 0x10100 bytes in 2 parts, but its record declares 0x10000");
    }

    #[test]
    fn walk_lists_nested_entries_with_paths() {
        let crypto = crypto();
        let raw = sample(&crypto, b"beta", 6);
        let mut rows = Vec::new();
        walk(&Archive::open(&raw, &crypto).unwrap(), "", &crypto, &mut |p, a, e, nested| rows.push((p.to_string(), a.sniff(e), nested))).unwrap();
        let expect = [("0", "xml", false), ("1", "rpf", true), ("1/0", "bin", false), ("1/1", "bin", false), ("2", "bin", false)];
        assert_eq!(rows, expect.map(|(p, e, n)| (p.to_string(), e, n)));
    }

    #[test]
    fn stats_count_leaves_and_inflated_sizes() {
        let crypto = crypto();
        let raw = sample(&crypto, b"beta", 6);
        let s = stats(&Archive::open(&raw, &crypto).unwrap(), &crypto).unwrap();
        assert_eq!((s.entries, s.nested, s.zlib), (4, 1, 1));
        assert_eq!(s.inflated, 25 + 5 + 4 + 1000);
        assert!(s.stored < s.inflated);
        assert_eq!(s.kinds["bin"].0, 3);
        assert_eq!((s.largest[0].0, s.largest[0].2.as_str()), (1000, "2"));
    }

    #[test]
    fn diff_compares_inflated_payloads() {
        let crypto = crypto();
        let leaves_of = |raw: &[u8]| leaves(&Archive::open(raw, &crypto).unwrap(), &crypto).unwrap();
        let old = leaves_of(&sample(&crypto, b"beta", 6));
        // Recompressed but equal content is identical.
        let d = diff(old.clone(), leaves_of(&sample(&crypto, b"beta", 9)));
        assert!(!d.differ());
        assert_eq!(d.same, 4);
        let d = diff(old.clone(), leaves_of(&sample(&crypto, b"gamma", 6)));
        assert_eq!(d.changed.iter().map(|(o, n)| (o.path.as_str(), o.size, n.size)).collect::<Vec<_>>(), [("1/1", 4, 5)]);
        assert!(d.added.is_empty() && d.removed.is_empty() && d.moved.is_empty());
        // An inserted entry moves the rest; a new nested payload is added, the old one removed.
        let shifted = &old[1..].iter().map(|l| Leaf { path: format!("{}x", l.path), ..l.clone() }).collect::<Vec<_>>();
        let d = diff(old.clone(), shifted.iter().cloned().chain([Leaf { path: "9".into(), digest: 1, size: 1, ext: "bin", offset: 0 }]).collect());
        assert_eq!((d.moved.len(), d.added.len(), d.removed.len(), d.same), (3, 1, 1, 0));
        assert_eq!(d.removed[0].path, "0");
    }

    #[test]
    fn verify_confirms_intended_changes_and_flags_the_rest() {
        let c = crypto();
        let src = sample(&c, b"beta", 6);
        let repack = |raw: &[u8], i: usize, d: &[u8]| rpf9::writer::repack_bytes(raw, &c, &[(i, d.to_vec())].into()).unwrap();
        // (entries checked, intended confirmed, moved with identical bytes, problems)
        let check = |out: &[u8], changes: &[(&str, &[u8])]| {
            let v = verify_written(&src, out, &c, &changes.iter().map(|(p, d)| (p.to_string(), d.to_vec())).collect()).unwrap();
            ((v.checked, v.confirmed, v.moved, v.problems.len()), v.problems)
        };
        let same = rpf9::writer::repack_bytes(&src, &c, &Default::default()).unwrap();
        assert_eq!(check(&same, &[]).0, (4, 0, 0, 0));
        let xml: &[u8] = b"<?xml version='1.0'?><b/>";
        assert_eq!(check(&repack(&src, 0, xml), &[("0", xml)]).0, (4, 1, 0, 0));
        // A larger #0 moves #2; nested entries keep their offsets inside #1.
        let big = [b'x'; 100];
        let mut out = repack(&src, 0, &big);
        assert_eq!(check(&out, &[("0", &big)]).0, (4, 1, 1, 0));
        let gamma: &[u8] = b"a much longer gamma payload";
        let inner = repack(&entry_blocks(&src, &c, "1").unwrap().concat(), 1, gamma);
        assert_eq!(check(&repack(&src, 1, &inner), &[("1/1", gamma)]).0, (4, 1, 1, 0));
        // Corruption in an untouched nested entry, and an intended change that is not there.
        let at = out.windows(5).position(|w| w == b"alpha").unwrap();
        out[at + 2] ^= 1;
        let (counts, problems) = check(&out, &[("0", &big)]);
        assert_eq!(counts, (4, 1, 1, 1));
        assert_eq!(problems, ["1/0: unexpected change, 5 -> 5 bytes; first difference at 0x2"]);
        assert_eq!(check(&same, &[("0", &big)]).0, (4, 0, 0, 1));
    }

    #[test]
    fn harvest_names_unknown_psin_hashes() {
        let crypto = crypto();
        let (known, s, m) = (0xde396fe2u32, joaat("my_struct"), joaat("health_max"));
        let mut lsch = vec![0u8; 32];
        lsch[0..4].copy_from_slice(&s.to_le_bytes());
        lsch[16..24].copy_from_slice(&[0xAA; 8]);
        lsch[30..32].copy_from_slice(&2u16.to_le_bytes());
        for name in [m, known] { lsch.extend([name.to_le_bytes(), [0; 4], [0x15, 0, 0, 0], [0; 4]].concat()); }
        let psin = [b"PSIN".as_slice(), &8u32.to_be_bytes(), b"LSCH", &(8 + lsch.len() as u32).to_be_bytes(), &lsch].concat();
        let text = b"<?xml?><x a=\"My_Struct\" b=\"health_max\" c=\"root_document\" d=\"nothing\"/>".to_vec();
        // Schema in one archive, names in a nested entry of another.
        let (a, b) = (archive(&crypto, &[psin]), archive(&crypto, &[archive(&crypto, &[zlib(&text, 6)])]));
        let dict = rpf9::hashes::Dict::load(None).unwrap();
        let archives = [Archive::open(&a, &crypto).unwrap(), Archive::open(&b, &crypto).unwrap()];
        let (unknown, found) = harvest(&archives, &crypto, &dict).unwrap();
        assert_eq!(unknown, 2);
        assert_eq!(found, [(s, "my_struct".to_string()), (m, "health_max".to_string())].into_iter().collect());
    }

    #[test]
    fn catalog_lists_every_entry_and_names_hashes() {
        let crypto = crypto();
        // A standalone drawable header (pointers at +0x08/+0x10/+0x50 and the name at +0xA8, zero bounds).
        let mut drawable = vec![0u8; 176];
        for at in [8, 16, 80, 168] { drawable[at..at + 8].copy_from_slice(&(forge9::formats::resource::VIRTUAL_BASE + 176).to_le_bytes()); }
        drawable.extend(b"prop_crate_01.#dr\0");
        let a = sample(&crypto, b"beta", 6);
        let b = named_archive(&crypto, &[b"<x n=\"Health_Max\"/>".to_vec(), b"x".to_vec(), b"y".to_vec(), drawable, b"z".to_vec()],
            &[0xde396fe2, joaat("health_max"), joaat("no_such_name"), joaat("prop_crate_01"), 0]);
        let archives = [("a.rpf".to_string(), Archive::open(&a, &crypto).unwrap()), ("b.rpf".to_string(), Archive::open(&b, &crypto).unwrap())];
        let mut rows = catalog(&archives, &crypto).unwrap();
        let ids: Vec<_> = rows.iter().map(|r| (r.id(), r.ext, r.inflated)).collect();
        let h = |n: &str| format!("{:#010x}", joaat(n));
        assert_eq!(ids, [("a.rpf:0".into(), "xml", 25), ("a.rpf:1".into(), "rpf", rows[1].stored), ("a.rpf:1/0".into(), "bin", 5), ("a.rpf:1/1".into(), "bin", 4),
            ("a.rpf:2".into(), "bin", 1000), ("b.rpf:0@0xde396fe2".into(), "bin", 19), (format!("b.rpf:1@{}", h("health_max")), "bin", 1),
            (format!("b.rpf:2@{}", h("no_such_name")), "bin", 1), (format!("b.rpf:3@{}", h("prop_crate_01")), "bin", 194), ("b.rpf:4".into(), "bin", 1)]);
        name_rows(&mut rows, &archives, &crypto, &rpf9::hashes::Dict::load(None).unwrap()).unwrap();
        let status: Vec<_> = rows[5..].iter().map(|r| (r.status(), r.name.as_deref(), r.evidence)).collect();
        let label = "embedded resource label";
        assert_eq!(status, [("resolved", Some("root_document"), "dictionary"), ("candidate", None, label), ("unresolved", None, label),
            ("resolved", Some("prop_crate_01.#dr"), label), ("unresolved", None, label)]);
        assert!(rows[..5].iter().all(|r| r.hash.is_none() && r.status() == "unresolved"));
        assert_eq!(rows[6].candidates, [("health_max".to_string(), "string in entry b.rpf:0".to_string())]);
    }

    #[test]
    fn catalog_resolves_dotted_candidate_names() {
        let crypto = crypto();
        let text = b"<Item>platform:/data/Content.XML</Item> ..setup2.meta.xml. foo..bar".to_vec();
        let raw = named_archive(
            &crypto,
            &[text, b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec(), b"e".to_vec()],
            &[0, joaat("content.xml"), joaat("content"), joaat("setup2.meta.xml"), joaat("foo..bar"), joaat("bar")],
        );
        let archives = [("pack.rpf".to_string(), Archive::open(&raw, &crypto).unwrap())];
        let mut rows = catalog(&archives, &crypto).unwrap();
        name_rows(&mut rows, &archives, &crypto, &rpf9::hashes::Dict::load(None).unwrap()).unwrap();
        let ev = "string in entry pack.rpf:0".to_string();
        assert_eq!(rows[1].candidates, [("content.xml".to_string(), ev.clone())]);
        assert_eq!(rows[2].candidates, [("content".to_string(), ev.clone())]);
        assert_eq!(rows[3].candidates, [("setup2.meta.xml".to_string(), ev.clone())]);
        assert_eq!(rows[4].status(), "unresolved");
        assert_eq!(rows[5].candidates, [("bar".to_string(), ev)]);
    }

    #[test]
    fn catalog_write_dict_accumulates_sorted_deduped_entries() {
        let crypto = crypto();
        let mut drawable = vec![0u8; 176];
        for at in [8, 16, 80, 168] { drawable[at..at + 8].copy_from_slice(&(forge9::formats::resource::VIRTUAL_BASE + 176).to_le_bytes()); }
        drawable.extend(b"prop_crate_01.#dr\0");

        let dict_path = std::env::temp_dir().join(format!("forge9-catalog-dict-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&dict_path);

        // Run 1: archive a.rpf yields a single candidate ("health_max") and an embedded drawable label ("prop_crate_01").
        let a = named_archive(&crypto, &[b"<x n=\"Health_Max\"/>".to_vec(), b"x".to_vec(), drawable, b"y".to_vec()],
            &[0xde396fe2, joaat("health_max"), joaat("prop_crate_01"), joaat("no_such_name")]);
        let archives_a = [("a.rpf".to_string(), Archive::open(&a, &crypto).unwrap())];
        let mut rows_a = catalog(&archives_a, &crypto).unwrap();
        name_rows(&mut rows_a, &archives_a, &crypto, &rpf9::hashes::Dict::load(None).unwrap()).unwrap();
        // Also ensure an ambiguous row with 2 candidates is skipped rather than written.
        rows_a.push(CatalogRow { archive: "a.rpf".into(), path: "99".into(), hash: Some(0x12345678), ext: "bin", stored: 1, inflated: 1,
            name: None, evidence: "embedded resource label", candidates: vec![("first".into(), "ev".into()), ("second".into(), "ev".into())] });
        let (entries_a, skipped) = confirmed_catalog_names(&rows_a);
        assert_eq!(skipped, 1);
        assert_eq!(rpf9::hashes::write_dict(&dict_path, entries_a).unwrap(), (2, 2));

        // Run 2: reload the written dict and catalog b.rpf (contains an already-recovered hash + a new candidate).
        let dict = rpf9::hashes::Dict::load(Some(&dict_path)).unwrap();
        let b = named_archive(&crypto, &[b"<x n=\"spawn_point\"/>".to_vec(), b"x".to_vec(), b"y".to_vec()],
            &[0, joaat("health_max"), joaat("spawn_point")]);
        let archives_b = [("b.rpf".to_string(), Archive::open(&b, &crypto).unwrap())];
        let mut rows_b = catalog(&archives_b, &crypto).unwrap();
        name_rows(&mut rows_b, &archives_b, &crypto, &dict).unwrap();
        assert_eq!((rows_b[1].status(), rows_b[1].name.as_deref(), rows_b[1].evidence), ("resolved", Some("health_max"), "dictionary"));
        let (entries_b, skipped_b) = confirmed_catalog_names(&rows_b);
        assert_eq!(skipped_b, 0);
        assert_eq!(rpf9::hashes::write_dict(&dict_path, entries_b).unwrap(), (1, 3));

        // Verify the accumulated file is deduplicated, sorted by hash, and reloads cleanly.
        let text = std::fs::read_to_string(&dict_path).unwrap();
        let mut expected = [
            (joaat("health_max"), "health_max"),
            (joaat("prop_crate_01"), "prop_crate_01"),
            (joaat("spawn_point"), "spawn_point"),
        ];
        expected.sort_by_key(|(h, _)| *h);
        let want: String = expected.iter().map(|(h, n)| format!("{h:#010x} {n}\n")).collect();
        assert_eq!(text, want);
        let reloaded = rpf9::hashes::Dict::load(Some(&dict_path)).unwrap();
        for (h, n) in expected { assert_eq!(reloaded.name(h), Some(n)); }
        let _ = std::fs::remove_file(&dict_path);
    }
}
