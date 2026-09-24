//! `schemgen2 convert`, plus the small read-only commands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use schemgen_core::formats::Metadata;
use schemgen_core::{pipeline, Palette, PaletteSet, Progress, Settings, Stage, Target};
use schemgen_server::savedir;

use crate::args::{settings_from_args, ParsedArgs};

/// What one input came to — also one entry of `--json` output.
struct Outcome {
    input: PathBuf,
    output: PathBuf,
    name: String,
    result: Result<Converted, String>,
}

struct Converted {
    blocks: usize,
    unique_blocks: usize,
    dims: [u32; 3],
    seconds: f32,
}

/// Progress lines on stderr, so stdout stays for results.
struct StderrProgress {
    label: String,
    quiet: bool,
}

impl Progress for StderrProgress {
    fn update(&mut self, _stage: Stage, fraction: f32, message: &str) {
        if !self.quiet {
            eprintln!("{}[{:>3.0}%] {message}", self.label, fraction * 100.0);
        }
    }
}

/// Run `schemgen2 convert`; returns the process exit code.
pub fn convert(args: &ParsedArgs, palettes: &PaletteSet) -> Result<i32, String> {
    let quiet = args.has("quiet") || args.has("json");
    let inputs = collect_inputs(&args.positional)?;
    let base = settings_from_args(args)?;
    let target = base.target();
    let palette = palettes.for_target(&target).map_err(|e| e.to_string())?;
    let palette = palette.as_ref();
    let extension = base.format().extension();

    let explicit_output = args.get("output").map(PathBuf::from);
    if explicit_output.is_some() && inputs.len() > 1 {
        return Err("--output takes a single input file; use --out-dir for several".to_string());
    }
    if args.get("name").is_some() && inputs.len() > 1 {
        return Err(
            "--name takes a single input file; with several, each keeps its own name".to_string(),
        );
    }
    let out_dir = args.get("out-dir").map(savedir::prepare).transpose()?;

    // Plan every output up front, so an unusable folder fails before the first
    // multi-minute voxelization, and two same-named models in one run become
    // `name.litematic` and `name-2.litematic`.
    let mut taken = HashSet::new();
    let mut plan: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for input in &inputs {
        let stem = input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let name = args.get("name").unwrap_or(stem).to_string();
        let filename =
            savedir::dedupe_filename(&savedir::sanitize_filename(&name, extension), &mut taken);
        let output = match (&explicit_output, &out_dir) {
            (Some(path), _) => path.clone(),
            (None, Some(dir)) => dir.join(&filename),
            (None, None) => input.parent().unwrap_or(Path::new(".")).join(&filename),
        };
        if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        plan.push((input.clone(), output, name));
    }

    let threads = args
        .number::<usize>("threads", 1)?
        .clamp(1, plan.len().max(1));
    if !quiet {
        eprintln!(
            "SchemGen2 {} — {} file(s) to .{extension}, target {} (data version {}), {} blocks in palette",schemgen_core::VERSION,
            plan.len(),
            target.key(),
            target.data_version,
            palette.block_count()
        );
    }

    let label_files = plan.len() > 1;
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Outcome>>> = Mutex::new((0..plan.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((input, output, name)) = plan.get(i) else {
                    break;
                };
                let label = if label_files {
                    format!(
                        "{} ",
                        input.file_name().and_then(|s| s.to_str()).unwrap_or("")
                    )
                } else {
                    String::new()
                };
                let settings = Settings {
                    schematic_name: name.clone(),
                    ..base.clone()
                };
                let result = convert_one(
                    input,
                    output,
                    &settings,
                    palette,
                    StderrProgress { label, quiet },
                );
                results.lock().expect("results lock")[i] = Some(Outcome {
                    input: input.clone(),
                    output: output.clone(),
                    name: name.clone(),
                    result,
                });
            });
        }
    });

    let outcomes: Vec<Outcome> = results
        .into_inner()
        .expect("results lock")
        .into_iter()
        .flatten()
        .collect();
    report(&outcomes, &target, args.has("json"), quiet);
    Ok(i32::from(outcomes.iter().any(|o| o.result.is_err())))
}

fn convert_one(
    input: &Path,
    output: &Path,
    settings: &Settings,
    palette: &Palette,
    mut progress: StderrProgress,
) -> Result<Converted, String> {
    let started = Instant::now();
    let grid = pipeline::run(input, settings, palette, &mut progress).map_err(|e| e.to_string())?;
    let meta = Metadata::new(&settings.schematic_name);
    settings
        .format()
        .write(output, &grid, &meta, &settings.target())
        .map_err(|e| e.to_string())?;
    Ok(Converted {
        blocks: grid.len(),
        unique_blocks: grid.names.len(),
        dims: grid.size,
        seconds: started.elapsed().as_secs_f32(),
    })
}

/// Human output goes to stderr and the finished paths to stdout, so
/// `schemgen2 convert a.glb | xargs -I{} cp {} ...` works.
fn report(outcomes: &[Outcome], target: &Target, as_json: bool, quiet: bool) {
    if as_json {
        let files: Vec<serde_json::Value> = outcomes
            .iter()
            .map(|o| match &o.result {
                Ok(r) => serde_json::json!({
                    "ok": true,
                    "input": o.input.display().to_string(),
                    "output": o.output.display().to_string(),
                    "name": o.name,
                    "format": o.output.extension().and_then(|e| e.to_str()).unwrap_or_default(),
                    "voxels": r.blocks,"unique_blocks": r.unique_blocks,
                    "grid": r.dims,
                    "seconds": r.seconds,
                }),
                Err(e) => serde_json::json!({
                    "ok": false,
                    "input": o.input.display().to_string(),
                    "output": o.output.display().to_string(),
                    "name": o.name,
                    "error": e,
                }),
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "ok": outcomes.iter().all(|o| o.result.is_ok()),
                "target": target.key(),
                "data_version": target.data_version,
                "files": files,
            })
        );
        return;
    }

    for o in outcomes {
        match &o.result {
            Ok(r) => {
                if !quiet {
                    let [x, y, z] = r.dims;
                    eprintln!(
                        "{} — {} blocks, {} unique, {x}×{y}×{z}, {:.1}s",
                        o.name, r.blocks, r.unique_blocks, r.seconds
                    );
                    let limit = schemgen_core::formats::structure::STRUCTURE_BLOCK_LIMIT;
                    let is_nbt = o.output.extension().is_some_and(|e| e == "nbt");
                    if is_nbt && r.dims.iter().any(|&d| d > limit) {
                        eprintln!(
                            "note: structure blocks load at most {limit}×{limit}×{limit}; \
                             place this one with /place template"
                        );
                    }
                }
                println!("{}", o.output.display());
            }
            Err(e) => eprintln!("error: {}: {e}", o.input.display()),
        }
    }
}

/// The operands must exist and look like glTF.
fn collect_inputs(positional: &[String]) -> Result<Vec<PathBuf>, String> {
    if positional.is_empty() {
        return Err("convert needs at least one .glb or .gltf file".to_string());
    }
    positional
        .iter()
        .map(|raw| {
            let lower = raw.to_lowercase();
            if !(lower.ends_with(".glb") || lower.ends_with(".gltf")) {
                return Err(format!("not a glTF file: {raw}"));
            }
            let path = PathBuf::from(raw);
            if !path.exists() {
                return Err(format!("no such file: {raw}"));
            }
            Ok(path)
        })
        .collect()
}

/// `schemgen2 palette`: the blocks a conversion for `--target` may choose.
pub fn palette(args: &ParsedArgs, palettes: &PaletteSet) -> Result<(), String> {
    let target = match args.get("target") {
        Some(raw) => Target::parse(raw).map_err(|e| e.to_string())?,
        None => Target::default(),
    };
    let palette = palettes.for_target(&target).map_err(|e| e.to_string())?;
    let table = palette.to_palette_json();
    if args.has("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&table).unwrap_or_default()
        );
        return Ok(());
    }
    let mut rows: Vec<(&String, &[f32; 3])> = table.iter().collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    for (name, rgb) in &rows {
        println!(
            "{:<44} #{:02X}{:02X}{:02X}",
            name, rgb[0] as u8, rgb[1] as u8, rgb[2] as u8
        );
    }
    eprintln!(
        "{} blocks, {} palette entries — Minecraft {} (data version {})",
        rows.len(),
        palette.len(),
        target.key(),
        target.data_version
    );
    Ok(())
}
/// `schemgen2 targets`: the Minecraft versions schematics can be made for.
pub fn targets(args: &ParsedArgs) {
    let default = Target::default();
    if args.has("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "default": default.key(),
                "targets": schemgen_core::targets::TARGETS,
            }))
            .unwrap_or_default()
        );
        return;
    }
    println!(
        "{:<10} {:>12} {:>10}",
        "VERSION", "DATA VERSION", "LITEMATIC"
    );
    for t in schemgen_core::targets::TARGETS.iter().rev() {
        let mark = if t.id == default.id {
            "  (default)"
        } else {
            ""
        };
        println!(
            "{:<10} {:>12} {:>10}{mark}",
            t.id, t.data_version, t.schematic_version
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_must_look_like_gltf_and_exist() {
        assert!(collect_inputs(&[]).is_err());
        assert!(collect_inputs(&["nope.obj".to_string()]).is_err());
        assert!(collect_inputs(&["missing.glb".to_string()]).is_err());
    }
}
