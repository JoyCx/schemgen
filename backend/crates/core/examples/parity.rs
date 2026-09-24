//! Parity harness: the Rust voxelizer against SchemGen2 2.0's Python helper.
//!
//! ```text
//! cargo run --release -p schemgen-core --features python-voxelizer --example parity -- \
//!     [--max-size N] [--python PATH] [--target PCT] [MODEL.glb ...]
//! ```
//!
//! Both implementations convert each model (default: `backend/fixtures/*.glb`)
//! with the default settings. For each, the harness reports
//!
//! * voxels only one side produced;
//! * the color difference (CIEDE2000) over the voxels both produced;
//! * blocks that differ once both go through the same adjust → dither → match
//!   stages, out of every voxel either produced — the number the acceptance
//!   target (below 0.5 %) is about.
//!
//! `trimesh` rows read materials the way the Python helper could, so they
//! measure the port itself. `gltf` rows are what ships; where they differ,
//! it is the material fixes `docs/pipeline.md` lists.
//!
//! Exits 1 when a `trimesh` row misses the target. Needs a Python with the
//! helper's requirements (`backend/scripts/requirements.txt`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use schemgen_core::palette::{ciede2000, rgb_to_lab, Palette};
use schemgen_core::pipeline;
use schemgen_core::sample::Materials;
use schemgen_core::voxelizer::{self, Voxels};
use schemgen_core::{NoProgress, PaletteSet, Settings};

struct Row {
    model: String,
    reading: &'static str,
    voxels: usize,
    only_python: usize,
    only_rust: usize,
    mean_de: f64,
    max_de: f32,
    colors_over_1: usize,
    blocks_differ: usize,
    union: usize,
    seconds: [f32; 2],
}

impl Row {
    fn block_mismatch(&self) -> f64 {
        100.0 * self.blocks_differ as f64 / self.union.max(1) as f64
    }
}

fn block_map(voxels: &Voxels, settings: &Settings, palette: &Palette) -> HashMap<[i32; 3], String> {
    let grid = pipeline::blocks(voxels.clone(), settings, palette, &mut NoProgress)
        .expect("matching blocks");
    grid.coords
        .iter()
        .zip(&grid.blocks)
        .map(|(c, &b)| (*c, grid.names[b as usize].clone()))
        .collect()
}

fn compare(
    model: &str,
    reading: &'static str,
    python: &Voxels,
    rust: &Voxels,
    seconds: [f32; 2],
    settings: &Settings,
    palette: &Palette,
) -> Row {
    let rust_index: HashMap<[i32; 3], usize> = rust
        .coords
        .iter()
        .enumerate()
        .map(|(i, c)| (*c, i))
        .collect();
    let (py_colors, rs_colors) = (python.colors.as_ref(), rust.colors.as_ref());

    let mut deltas = Vec::new();
    let mut only_python = 0;
    for (i, c) in python.coords.iter().enumerate() {
        match rust_index.get(c) {
            Some(&j) => {
                if let (Some(a), Some(b)) = (py_colors, rs_colors) {
                    let (a, b) = (a[i], b[j]);
                    let squared =
                        ciede2000(&rgb_to_lab(a[0], a[1], a[2]), &rgb_to_lab(b[0], b[1], b[2]));
                    deltas.push(squared.max(0.0).sqrt());
                }
            }
            None => only_python += 1,
        }
    }
    let common = python.coords.len() - only_python;
    let only_rust = rust.coords.len() - common;

    let py_blocks = block_map(python, settings, palette);
    let rs_blocks = block_map(rust, settings, palette);
    let differ_on_common = py_blocks
        .iter()
        .filter(|(c, name)| rs_blocks.get(*c).is_some_and(|other| other != *name))
        .count();

    Row {
        model: model.to_string(),
        reading,
        voxels: python.coords.len(),
        only_python,
        only_rust,
        mean_de: deltas.iter().map(|&d| f64::from(d)).sum::<f64>() / deltas.len().max(1) as f64,
        max_de: deltas.iter().copied().fold(0.0, f32::max),
        colors_over_1: deltas.iter().filter(|&&d| d > 1.0).count(),
        blocks_differ: differ_on_common + only_python + only_rust,
        union: common + only_python + only_rust,
        seconds,
    }
}

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut models: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    models.retain(|p| p.extension().is_some_and(|e| e == "glb" || e == "gltf"));
    models.sort();
    models
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut max_size = 64u32;
    let mut target = 0.5f64;
    let mut models = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--max-size" => {
                max_size = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--max-size N")
            }
            "--target" => {
                target = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--target PCT")
            }
            "--python" => voxelizer::set_python(&args.next().expect("--python PATH")),
            other => models.push(PathBuf::from(other)),
        }
    }
    if models.is_empty() {
        models = fixtures();
    }

    let settings = Settings {
        max_size,
        ..Settings::default()
    }
    .normalized()
    .expect("default settings are valid");
    let palette = PaletteSet::builtin()
        .for_target(&settings.target())
        .expect("the built-in palette");

    let mut rows = Vec::new();
    let mut skipped = Vec::new();
    for path in &models {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let started = Instant::now();
        let python = voxelizer::python::voxelize(path, &settings, &mut NoProgress);
        let python_seconds = started.elapsed().as_secs_f32();
        for (reading, materials) in [("trimesh", Materials::Trimesh), ("gltf", Materials::Gltf)] {
            let started = Instant::now();
            let rust = voxelizer::voxelize_native(path, &settings, materials, &mut NoProgress);
            let seconds = [python_seconds, started.elapsed().as_secs_f32()];
            match (&python, rust) {
                (Ok(python), Ok(rust)) => rows.push(compare(
                    &name, reading, python, &rust, seconds, &settings, &palette,
                )),
                (python, rust) => {
                    // A model one side cannot read is reported, not compared.
                    let show = |r: Result<(), String>| r.err().unwrap_or_else(|| "ok".into());
                    skipped.push(format!(
                        "{name} ({reading}): python {}; rust {}",
                        show(python.as_ref().map(|_| ()).map_err(|e| e.to_string())),
                        show(rust.map(|_| ()).map_err(|e| e.to_string()))
                    ));
                }
            }
        }
    }

    println!("max_size {max_size}, default settings; ΔE is CIEDE2000 over voxels both produced\n");
    println!(
        "{:<22} {:<8} {:>7} {:>6} {:>6} {:>8} {:>7} {:>6} {:>8} {:>13}",
        "model",
        "reading",
        "voxels",
        "−rust",
        "+rust",
        "mean ΔE",
        "max ΔE",
        "ΔE>1",
        "blocks≠",
        "python/rust s"
    );
    let mut failed = false;
    for r in &rows {
        let pct = r.block_mismatch();
        let miss = r.reading == "trimesh" && pct >= target;
        failed |= miss;
        println!(
            "{:<22} {:<8} {:>7} {:>6} {:>6} {:>8.4} {:>7.3} {:>6} {:>7.3}% {:>6.2}/{:<6.2}{}",
            r.model,
            r.reading,
            r.voxels,
            r.only_python,
            r.only_rust,
            r.mean_de,
            r.max_de,
            r.colors_over_1,
            pct,
            r.seconds[0],
            r.seconds[1],
            if miss { "  ← over target" } else { "" }
        );
    }
    for line in &skipped {
        println!("not compared: {line}");
    }
    println!("\ntarget: trimesh rows below {target}% of blocks different");
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
