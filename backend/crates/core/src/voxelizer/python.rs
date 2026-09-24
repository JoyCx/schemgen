//! Voxelization through the Python trimesh helper, `scripts/voxelize.py`.
//!
//! One subprocess voxelizes and samples colors: the model is parsed once and
//! only one interpreter starts. The child is polled rather than waited on, so
//! a cancelled conversion kills it instead of letting it run to completion.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use super::Voxels;
use crate::error::{Error, Result};
use crate::pipeline::{Progress, Stage};
use crate::settings::Settings;

/// Interpreter chosen with `--python`, which beats `SCHEMGEN_PYTHON`.
static PYTHON_OVERRIDE: OnceLock<String> = OnceLock::new();

/// Use this Python interpreter for every conversion in this process. Called
/// once, at startup, from the `--python` flag.
pub fn set_python(path: &str) {
    let path = path.trim();
    if !path.is_empty() {
        let _ = PYTHON_OVERRIDE.set(path.to_string());
    }
}

/// The interpreter that must have `trimesh` installed: `--python`, else
/// `SCHEMGEN_PYTHON` (typically a virtualenv's), else whatever `python3`
/// (`python` on Windows) resolves to on `PATH`.
pub fn python_interpreter() -> String {
    if let Some(path) = PYTHON_OVERRIDE.get() {
        return path.clone();
    }
    match std::env::var("SCHEMGEN_PYTHON") {
        Ok(path) if !path.trim().is_empty() => path.trim().to_string(),
        _ => if cfg!(windows) { "python" } else { "python3" }.to_string(),
    }
}

/// `voxelize.py`: `SCHEMGEN_SCRIPTS_DIR` wins, then a `scripts` folder next to
/// the binary, then the source tree the binary was built from.
fn voxelize_script() -> PathBuf {
    let from_env = std::env::var("SCHEMGEN_SCRIPTS_DIR")
        .ok()
        .map(PathBuf::from);
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("scripts")));
    let source_tree = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts");
    [from_env, beside_exe, Some(source_tree.clone())]
        .into_iter()
        .flatten()
        .map(|dir| dir.join("voxelize.py"))
        .find(|script| script.is_file())
        .unwrap_or_else(|| source_tree.join("voxelize.py"))
}

#[derive(Debug, Deserialize)]
struct VoxelOutput {
    coords: Vec<[i32; 3]>,
    pitch: f32,
    voxel_world_origin: [f32; 3],
    /// Present when the helper was asked to sample colors in the same pass.
    colors: Option<Vec<[f32; 3]>>,
}

/// How far through the run each of the helper's own progress lines means it
/// is — it reports stages, not percentages.
const MILESTONES: &[(&str, f32)] = &[
    ("Sampling ", 0.30),
    ("Scattered ", 0.60),
    ("Rebuilding ", 0.65),
    ("Falling back", 0.70),
    ("Done sampling", 0.80),
];

pub(super) fn voxelize(
    input: &Path,
    settings: &Settings,
    progress: &mut dyn Progress,
) -> Result<Voxels> {
    let python = python_interpreter();
    let script = voxelize_script();
    log::info!(
        "Voxelizing {} (max_size={}, colors={}) with {python}",
        input.display(),
        settings.max_size,
        settings.color_sampling
    );

    let mut cmd = Command::new(&python);
    cmd.arg(&script)
        .arg("--input")
        .arg(input)
        .arg("--max-size")
        .arg(settings.max_size.to_string());
    if let Some(vs) = settings.voxel_size {
        cmd.arg("--voxel-size").arg(vs.to_string());
    }
    if settings.color_sampling {
        let light = settings.lighting();
        let [lx, ly, lz] = light.light_dir;
        cmd.arg("--sample-colors")
            .arg(format!("--ram-limit={}", settings.ram_limit))
            .arg(format!("--light-dir={lx},{ly},{lz}"))
            .arg(format!("--light-ambient={}", light.ambient))
            .arg(format!("--light-gloss={}", light.gloss))
            .arg(format!("--specular={}", light.specular))
            .arg(format!("--highlight-rejection={}", light.rejection))
            .arg(format!("--highlight-recovery={}", light.recovery))
            .arg(format!("--delight={}", light.delight));
    }
    cmd.arg("--hollow")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        Error::Voxelizer(format!(
            "could not start Python ({python}): {e}. Point --python or SCHEMGEN_PYTHON \
             at an interpreter that has trimesh installed"
        ))
    })?;

    // Both pipes are drained on their own threads so neither can fill up and
    // stall the child while this thread polls for cancellation.
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        stdout.read_to_end(&mut out).map(|_| out)
    });
    let stderr = child.stderr.take().expect("stderr is piped");
    let (line_tx, line_rx) = mpsc::channel::<String>();
    let logger = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            let line = line.trim().to_string();
            if !line.is_empty() && line_tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut errors: Vec<String> = Vec::new();
    let mut fraction = 0.05f32;
    let mut forward = |line: String, progress: &mut dyn Progress, errors: &mut Vec<String>| {
        log::info!("[voxelize.py] {line}");
        if let Some(&(_, f)) = MILESTONES.iter().find(|(m, _)| line.starts_with(m)) {
            fraction = fraction.max(f);
            progress.update(Stage::Voxelize, fraction, &line);
        }
        errors.push(line.strip_prefix("ERROR: ").unwrap_or(&line).to_string());
    };

    let status = loop {
        while let Ok(line) = line_rx.try_recv() {
            forward(line, progress, &mut errors);
        }
        if progress.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Cancelled);
        }
        match child.try_wait()? {
            Some(status) => break status,
            None => std::thread::sleep(Duration::from_millis(40)),
        }
    };

    let _ = logger.join();
    for line in line_rx.try_iter() {
        forward(line, progress, &mut errors);
    }
    let stdout = reader
        .join()
        .map_err(|_| Error::Voxelizer("output reader panicked".into()))??;

    if !status.success() {
        // The script's own "ERROR: ..." line is the useful part; tracebacks
        // and progress chatter only add noise.
        let detail = errors
            .iter()
            .rev()
            .find(|l| !l.starts_with("Traceback") && !l.starts_with("File "))
            .cloned()
            .unwrap_or_else(|| format!("exit status {status}"));
        return Err(Error::Voxelizer(detail));
    }

    let text = String::from_utf8_lossy(&stdout);
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::Voxelizer("the voxelizer produced no output".into()));
    }
    let output: VoxelOutput = serde_json::from_str(text).map_err(|e| {
        Error::Voxelizer(format!(
            "unreadable voxelizer output ({e}): {}",
            &text[..text.len().min(200)]
        ))
    })?;

    if let Some(colors) = &output.colors {
        if colors.len() != output.coords.len() {
            return Err(Error::Voxelizer(format!(
                "{} colors for {} voxels",
                colors.len(),
                output.coords.len()
            )));
        }
    }
    log::info!(
        "Voxelization complete: {} voxels, pitch={}",
        output.coords.len(),
        output.pitch
    );

    Ok(Voxels {
        coords: output.coords,
        colors: output.colors,
        pitch: output.pitch,
        origin: output.voxel_world_origin,
    })
}
