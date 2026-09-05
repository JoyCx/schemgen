//! Command-line interface — the same pipeline the HTTP server runs, headless.
//!
//! `schemgen2 convert model.glb --out-dir ~/.minecraft/schematics` produces the
//! same file as dropping that model into the web UI with the same settings:
//! both call [`crate::converter::convert`] with a [`ConversionOptions`] built
//! from identical defaults and clamps.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::palette::Palette;
use crate::savedir;
use crate::types::{ConversionOptions, ConversionResult, LightingOptions};

pub const HELP: &str = "\
SchemGen2 — GLB/glTF → Litematica converter

USAGE:
    schemgen2 <COMMAND> [OPTIONS]

COMMANDS:
    serve                     Start the HTTP API and web UI (default with no command)
    convert <FILE>...         Convert models to .litematic without a server
    palette                   Print the block palette
    build-table <DIR> <OUT>   Rebuild the color table from a texture pack
    help, version

Run `schemgen2 help <COMMAND>` for the options of one command.
";

pub const HELP_SERVE: &str = "\
schemgen2 serve — HTTP API + web UI on one port

USAGE:
    schemgen2 serve [--port <N>]

OPTIONS:
    --port <N>    Port to bind (default 3001, or the PORT env var)

The web UI is served from ../frontend/dist when that folder exists; the API is
available either way. See docs/api.md for the routes.
";

pub const HELP_CONVERT: &str = "\
schemgen2 convert — convert models to .litematic without a server

USAGE:
    schemgen2 convert <FILE>... [OPTIONS]

OUTPUT:
    -o, --output <FILE>       Write to this exact path (single input only)
    -d, --out-dir <DIR>       Write into this folder; ~ and %APPDATA% expand and
                              the folder is created if missing
    -n, --name <NAME>         Schematic name (default: the input file stem)
    With neither -o nor -d, the file lands next to its input.

GEOMETRY:
    --max-size <N>            Longest axis in blocks (default 128)
    --voxel-size <F>          Explicit voxel pitch in model units; overrides --max-size
    --ram-limit <GB>          Color-sampler memory budget (default 4)

COLOR:
    --no-dither               Skip 8x8 Bayer ordered dithering
    --no-color                One block everywhere instead of matching colors
    --block <ID>              Block for --no-color: white, netherrack, or a full ID
    --brightness <F>          -1..1  (default 0)
    --contrast <F>            0..3   (default 1)
    --saturation <F>          0..3   (default 1)

LIGHTING SEPARATION (see docs/pipeline.md):
    --light-dir <x,y,z>       Key light direction in model space (default 0.35,0.85,0.4)
    --light-ambient <F>       0..1   (default 0.32)
    --light-gloss <F>         0..1   (default 0.5)
    --specular <F>            0..4   (default 1.1)
    --highlight-rejection <F> 0..1   (default 0.75)
    --highlight-recovery <F>  0..1   (default 1)
    --delight <F>             0..1   (default 0 = off)

OUTPUT FORMAT:
    --data-version <N>        MinecraftDataVersion to stamp (default 4440 = 1.21.8;
                              4671 = 1.21.11). Also settable with SCHEMGEN_DATA_VERSION.

RUN:
    --threads <N>             Convert N files at once (default 1)
    -q, --quiet               No progress lines
    -j, --json                Print a JSON result object on stdout

EXIT CODES:
    0 success   1 conversion failed   2 bad usage
";

/// Long options that consume the next argument.
const VALUE_OPTS: &[&str] = &[
    "output", "out-dir", "name", "max-size", "voxel-size", "ram-limit", "threads",
    "block", "brightness", "contrast", "saturation", "light-dir", "light-ambient",
    "light-gloss", "specular", "highlight-rejection", "highlight-recovery",
    "delight", "data-version", "port",
];

/// One-letter aliases. The bool marks the ones that take a value.
const SHORT_OPTS: &[(char, &str, bool)] = &[
    ('o', "output", true),
    ('d', "out-dir", true),
    ('n', "name", true),
    ('p', "port", true),
    ('q', "quiet", false),
    ('j', "json", false),
    ('h', "help", false),
    ('V', "version", false),
];

#[derive(Debug, Default)]
pub struct ParsedArgs {
    pub command: String,
    pub positional: Vec<String>,
    pub opts: HashMap<String, String>,
    pub flags: HashSet<String>,
}

impl ParsedArgs {
    pub fn has(&self, flag: &str) -> bool { self.flags.contains(flag) }
    pub fn get(&self, key: &str) -> Option<&str> { self.opts.get(key).map(String::as_str) }

    fn number<T: std::str::FromStr>(&self, key: &str, default: T) -> Result<T, String> {
        match self.get(key) {
            None => Ok(default),
            Some(raw) => raw.trim().parse::<T>()
                .map_err(|_| format!("--{key}: '{raw}' is not a number")),
        }
    }

    fn clamped(&self, key: &str, default: f32, lo: f32, hi: f32) -> Result<f32, String> {
        Ok(self.number::<f32>(key, default)?.clamp(lo, hi))
    }
}

/// Split argv (without the program name) into a command, options and operands.
///
/// `--key value`, `--key=value`, `-o value`, clustered short flags (`-qj`) and
/// a bare `--` terminator all work. An unknown long option is a usage error
/// rather than a silently ignored typo.
pub fn parse(argv: &[String]) -> Result<ParsedArgs, String> {
    let mut out = ParsedArgs::default();
    let mut rest_are_positional = false;
    let mut i = 0;

    while i < argv.len() {
        let arg = argv[i].clone();
        i += 1;

        if rest_are_positional {
            out.positional.push(arg);
            continue;
        }
        if arg == "--" {
            rest_are_positional = true;
            continue;
        }

        if let Some(long) = arg.strip_prefix("--") {
            let (key, inline) = match long.split_once('=') {
                Some((k, v)) => (k.to_string(), Some(v.to_string())),
                None => (long.to_string(), None),
            };
            if VALUE_OPTS.contains(&key.as_str()) {
                let value = match inline {
                    Some(v) => v,
                    None => {
                        let v = argv.get(i).cloned()
                            .ok_or_else(|| format!("--{key} needs a value"))?;
                        i += 1;
                        v
                    }
                };
                out.opts.insert(key, value);
            } else if inline.is_some() {
                return Err(format!("--{key} does not take a value"));
            } else if is_known_flag(&key) {
                out.flags.insert(key);
            } else {
                return Err(format!("unknown option --{key}"));
            }
            continue;
        }

        if arg.len() > 1 && arg.starts_with('-') {
            let chars: Vec<char> = arg.chars().skip(1).collect();
            for (pos, c) in chars.iter().enumerate() {
                let (_, name, takes_value) = SHORT_OPTS.iter()
                    .find(|(short, _, _)| short == c)
                    .ok_or_else(|| format!("unknown option -{c}"))?;
                if *takes_value {
                    // The rest of the cluster is the value, else the next argv.
                    let tail: String = chars[pos + 1..].iter().collect();
                    let value = if tail.is_empty() {
                        let v = argv.get(i).cloned()
                            .ok_or_else(|| format!("-{c} needs a value"))?;
                        i += 1;
                        v
                    } else {
                        tail
                    };
                    out.opts.insert((*name).to_string(), value);
                    break;
                }
                out.flags.insert((*name).to_string());
            }
            continue;
        }

        if out.command.is_empty() && out.positional.is_empty() {
            out.command = arg;
        } else {
            out.positional.push(arg);
        }
    }

    Ok(out)
}

fn is_known_flag(name: &str) -> bool {
    matches!(name, "no-dither" | "no-color" | "quiet" | "json" | "help" | "version")
}

// ---- convert ---------------------------------------------------------------

/// Per-file outcome, also one entry of `--json` output.
struct JobOutcome {
    input: String,
    output: String,
    name: String,
    result: Result<ConversionResult, String>,
}

/// Run `schemgen2 convert`. Returns the process exit code.
pub async fn convert(args: &ParsedArgs, palette: Arc<Palette>) -> Result<i32, String> {
    let quiet = args.has("quiet") || args.has("json");
    let as_json = args.has("json");

    let inputs = collect_inputs(&args.positional)?;
    let base_options = options_from_args(args)?;

    if let Some(raw) = args.get("data-version") {
        let version: i32 = raw.trim().parse()
            .map_err(|_| format!("--data-version: '{raw}' is not a number"))?;
        crate::litematic::set_data_version(version);
    }

    let explicit_output = args.get("output").map(PathBuf::from);
    if explicit_output.is_some() && inputs.len() > 1 {
        return Err("--output takes a single input file; use --out-dir for several".to_string());
    }
    if args.get("name").is_some() && inputs.len() > 1 {
        return Err("--name takes a single input file; with several, each keeps its own name".to_string());
    }

    let out_dir = match args.get("out-dir") {
        Some(raw) => Some(savedir::prepare(raw)?),
        None => None,
    };

    // Plan every output path up front, so an unusable folder fails before the
    // first multi-minute voxelization and two identically named models in one
    // run become `name.litematic` and `name-2.litematic`.
    let mut taken: HashSet<String> = HashSet::new();
    let mut plan: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for input in &inputs {
        let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        let name = args.get("name").unwrap_or(stem).to_string();
        let filename = savedir::dedupe_filename(
            &savedir::sanitize_filename(&format!("{name}.litematic")), &mut taken);
        let output = match (&explicit_output, &out_dir) {
            (Some(path), _) => path.clone(),
            (None, Some(dir)) => dir.join(&filename),
            (None, None) => input.parent().unwrap_or(Path::new(".")).join(&filename),
        };
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
        }
        plan.push((input.clone(), output, name));
    }

    let threads = args.number::<usize>("threads", 1)?.max(1);
    let show_prefix = plan.len() > 1;

    if !quiet {
        eprintln!("SchemGen2 {} — {} file(s), {} palette entries",
            env!("CARGO_PKG_VERSION"), plan.len(), palette.len());
    }

    let mut outcomes: Vec<JobOutcome> = Vec::with_capacity(plan.len());
    for chunk in plan.chunks(threads) {
        let mut handles = Vec::with_capacity(chunk.len());
        for (input, output, name) in chunk {
            let mut options = base_options.clone();
            options.schematic_name = name.clone();
            let palette = Arc::clone(&palette);
            let input_s = input.display().to_string();
            let output_s = output.display().to_string();
            let label = if show_prefix {
                format!("{} ", input.file_name().and_then(|s| s.to_str()).unwrap_or(""))
            } else {
                String::new()
            };

            handles.push((
                input.clone(), output.clone(), name.clone(),
                tokio::task::spawn_blocking(move || {
                    run_blocking(&input_s, &output_s, options, palette, quiet, label)
                }),
            ));
        }
        for (input, output, name, handle) in handles {
            let result = handle.await
                .unwrap_or_else(|e| Err(format!("conversion task panicked: {e}")));
            outcomes.push(JobOutcome {
                input: input.display().to_string(),
                output: output.display().to_string(),
                name,
                result,
            });
        }
    }

    report(&outcomes, as_json, quiet);
    Ok(i32::from(outcomes.iter().any(|o| o.result.is_err())))
}

/// One conversion on its own runtime, mirroring how the server runs jobs, with
/// progress lines streamed to stderr so stdout stays parseable.
fn run_blocking(
    input: &str,
    output: &str,
    options: ConversionOptions,
    palette: Arc<Palette>,
    quiet: bool,
    label: String,
) -> Result<ConversionResult, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not start runtime: {e}"))?;

    runtime.block_on(async move {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<crate::types::ProgressEvent>();
        let printer = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if !quiet {
                    eprintln!("{label}[{:>3.0}%] {}", event.pct * 100.0, event.msg);
                }
            }
        });
        let result = crate::converter::convert(input, output, &options, &palette, tx).await;
        let _ = printer.await;
        result
    })
}

/// Human output goes to stderr and the finished paths to stdout, so
/// `schemgen2 convert a.glb | xargs -I{} cp {} ...` works.
fn report(outcomes: &[JobOutcome], as_json: bool, quiet: bool) {
    if as_json {
        let files: Vec<serde_json::Value> = outcomes.iter().map(|o| match &o.result {
            Ok(r) => serde_json::json!({
                "ok": true,
                "input": o.input,
                "output": o.output,
                "name": o.name,
                "voxels": r.voxel_count,
                "unique_blocks": r.unique_blocks,
                "grid": [r.grid.0, r.grid.1, r.grid.2],
                "seconds": r.elapsed,
            }),
            Err(e) => serde_json::json!({
                "ok": false, "input": o.input, "output": o.output,
                "name": o.name, "error": e,
            }),
        }).collect();
        let ok = outcomes.iter().all(|o| o.result.is_ok());
        println!("{}", serde_json::json!({
            "ok": ok,
            "data_version": crate::litematic::data_version(),
            "files": files,
        }));
        return;
    }

    for outcome in outcomes {
        match &outcome.result {
            Ok(r) => {
                if !quiet {
                    eprintln!("{} — {} blocks, {} unique, {}×{}×{}, {:.1}s",
                        outcome.name, r.voxel_count, r.unique_blocks,
                        r.grid.0, r.grid.1, r.grid.2, r.elapsed);
                }
                println!("{}", outcome.output);
            }
            Err(e) => eprintln!("error: {}: {e}", outcome.input),
        }
    }
}

/// Validate the operands: they must exist and look like glTF.
fn collect_inputs(positional: &[String]) -> Result<Vec<PathBuf>, String> {
    if positional.is_empty() {
        return Err("convert needs at least one .glb or .gltf file".to_string());
    }
    let mut inputs = Vec::with_capacity(positional.len());
    for raw in positional {
        let path = PathBuf::from(raw);
        let lower = raw.to_lowercase();
        if !(lower.ends_with(".glb") || lower.ends_with(".gltf")) {
            return Err(format!("not a glTF file: {raw}"));
        }
        if !path.exists() {
            return Err(format!("no such file: {raw}"));
        }
        inputs.push(path);
    }
    Ok(inputs)
}

/// Build conversion options from the parsed flags, using the same defaults and
/// clamps as the multipart form in `api.rs`.
fn options_from_args(args: &ParsedArgs) -> Result<ConversionOptions, String> {
    let defaults = LightingOptions::default();
    let lighting = LightingOptions {
        light_dir: match args.get("light-dir") {
            Some(raw) => parse_direction(raw)?,
            None => defaults.light_dir,
        },
        ambient: args.clamped("light-ambient", defaults.ambient, 0.0, 1.0)?,
        gloss: args.clamped("light-gloss", defaults.gloss, 0.0, 1.0)?,
        specular: args.clamped("specular", defaults.specular, 0.0, 4.0)?,
        rejection: args.clamped("highlight-rejection", defaults.rejection, 0.0, 1.0)?,
        recovery: args.clamped("highlight-recovery", defaults.recovery, 0.0, 1.0)?,
        delight: args.clamped("delight", defaults.delight, 0.0, 1.0)?,
    };

    Ok(ConversionOptions {
        max_size: args.number::<u32>("max-size", 128)?.max(1),
        voxel_size: match args.get("voxel-size") {
            Some(raw) => Some(raw.trim().parse::<f32>()
                .map_err(|_| format!("--voxel-size: '{raw}' is not a number"))?),
            None => None,
        },
        ram_limit: args.number::<f32>("ram-limit", 4.0)?.max(0.5),
        use_dithering: !args.has("no-dither"),
        use_color_sampling: !args.has("no-color"),
        brightness: args.clamped("brightness", 0.0, -1.0, 1.0)?,
        contrast: args.clamped("contrast", 1.0, 0.0, 3.0)?,
        saturation: args.clamped("saturation", 1.0, 0.0, 3.0)?,
        default_block_name: block_id(args.get("block").unwrap_or("white")),
        schematic_name: String::new(),
        lighting,
    })
}

/// Accept the web UI's short names as well as a full block ID.
fn block_id(raw: &str) -> String {
    match raw.trim() {
        "netherrack" => "minecraft:netherrack".to_string(),
        "white" | "" => "minecraft:white_concrete".to_string(),
        other if other.contains(':') => other.to_string(),
        other => format!("minecraft:{other}"),
    }
}

fn parse_direction(raw: &str) -> Result<[f32; 3], String> {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(format!("--light-dir: expected x,y,z — got '{raw}'"));
    }
    let mut out = [0.0f32; 3];
    for (slot, part) in out.iter_mut().zip(parts) {
        *slot = part.parse().map_err(|_| format!("--light-dir: '{part}' is not a number"))?;
    }
    if out == [0.0, 0.0, 0.0] {
        return Err("--light-dir: the zero vector has no direction".to_string());
    }
    Ok(out)
}

// ---- palette ---------------------------------------------------------------

/// Print the loaded palette — the blocks a conversion is allowed to choose.
pub fn palette(args: &ParsedArgs, palette: &Palette) {
    let table = palette.to_palette_json();
    if args.has("json") {
        println!("{}", serde_json::to_string_pretty(&table).unwrap_or_default());
        return;
    }
    let mut rows: Vec<(&String, &[f32; 3])> = table.iter().collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    for (name, rgb) in &rows {
        println!("{:<44} #{:02X}{:02X}{:02X}", name,
            rgb[0] as u8, rgb[1] as u8, rgb[2] as u8);
    }
    eprintln!("{} blocks, {} palette entries", rows.len(), palette.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parses_command_operands_and_values() {
        let a = parse(&argv(&["convert", "a.glb", "b.glb", "--max-size", "64"])).unwrap();
        assert_eq!(a.command, "convert");
        assert_eq!(a.positional, vec!["a.glb".to_string(), "b.glb".to_string()]);
        assert_eq!(a.get("max-size"), Some("64"));
    }

    #[test]
    fn accepts_inline_values_and_short_clusters() {
        let a = parse(&argv(&["convert", "m.glb", "--max-size=96", "-qj", "-o", "out.litematic"])).unwrap();
        assert_eq!(a.get("max-size"), Some("96"));
        assert!(a.has("quiet") && a.has("json"));
        assert_eq!(a.get("output"), Some("out.litematic"));
    }

    #[test]
    fn rejects_typos_instead_of_ignoring_them() {
        assert!(parse(&argv(&["convert", "--max-sixe", "64"])).is_err());
        assert!(parse(&argv(&["convert", "--max-size"])).is_err());
        assert!(parse(&argv(&["convert", "--no-dither=1"])).is_err());
        assert!(parse(&argv(&["convert", "-Z"])).is_err());
    }

    #[test]
    fn double_dash_stops_option_parsing() {
        let a = parse(&argv(&["convert", "--", "--weird-name.glb"])).unwrap();
        assert_eq!(a.positional, vec!["--weird-name.glb".to_string()]);
    }

    #[test]
    fn defaults_match_the_web_form() {
        let a = parse(&argv(&["convert", "m.glb"])).unwrap();
        let o = options_from_args(&a).unwrap();
        assert_eq!(o.max_size, 128);
        assert!(o.use_dithering && o.use_color_sampling);
        assert_eq!(o.default_block_name, "minecraft:white_concrete");
        assert_eq!(o.ram_limit, 4.0);
        assert_eq!(o.lighting.ambient, LightingOptions::default().ambient);
        assert_eq!(o.lighting.light_dir, LightingOptions::default().light_dir);
    }

    #[test]
    fn flags_override_and_clamp() {
        let a = parse(&argv(&[
            "convert", "m.glb", "--no-dither", "--no-color", "--block", "netherrack",
            "--saturation", "9", "--delight", "0.5", "--light-dir", "0,1,0",
        ])).unwrap();
        let o = options_from_args(&a).unwrap();
        assert!(!o.use_dithering && !o.use_color_sampling);
        assert_eq!(o.default_block_name, "minecraft:netherrack");
        assert_eq!(o.saturation, 3.0);
        assert_eq!(o.lighting.delight, 0.5);
        assert_eq!(o.lighting.light_dir, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn bad_light_dir_is_a_usage_error() {
        let bad = parse(&argv(&["convert", "m.glb", "--light-dir", "1,2"])).unwrap();
        assert!(options_from_args(&bad).is_err());
        let zero = parse(&argv(&["convert", "m.glb", "--light-dir", "0,0,0"])).unwrap();
        assert!(options_from_args(&zero).is_err());
    }

    #[test]
    fn non_numeric_values_are_usage_errors() {
        let a = parse(&argv(&["convert", "m.glb", "--max-size", "big"])).unwrap();
        assert!(options_from_args(&a).is_err());
    }

    #[test]
    fn block_ids_accept_short_and_full_names() {
        assert_eq!(block_id("white"), "minecraft:white_concrete");
        assert_eq!(block_id("netherrack"), "minecraft:netherrack");
        assert_eq!(block_id("minecraft:stone"), "minecraft:stone");
        assert_eq!(block_id("deepslate"), "minecraft:deepslate");
    }

    #[test]
    fn inputs_must_look_like_gltf() {
        assert!(collect_inputs(&[]).is_err());
        assert!(collect_inputs(&["nope.obj".to_string()]).is_err());
        assert!(collect_inputs(&["missing.glb".to_string()]).is_err());
    }
}
