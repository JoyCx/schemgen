//! Voxelizer — calls the Python trimesh helper via subprocess for hollow voxelization.

use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use crate::types::LightingOptions;

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
    let source_tree = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts");
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
    grid_dims: [i32; 3],
    /// Present when the helper was asked to sample colors in the same pass.
    colors: Option<Vec<[f32; 3]>>,
}

/// Result of one combined voxelize (+ optional color sampling) pass.
pub struct VoxelizeResult {
    pub coords: Vec<[i32; 3]>,
    pub grid_dims: (u32, u32, u32),
    pub colors: Option<Vec<[f32; 3]>>,
}

/// Voxelize a GLB file using the Python trimesh helper, optionally sampling
/// per-voxel surface colors in the same process — the GLB is parsed once and
/// only one interpreter is started.
pub fn voxelize(
    glb_path: &str,
    max_size: u32,
    voxel_size: Option<f32>,
    sample_colors: Option<f32>, // Some(ram_limit_gb) to sample colors too
    lighting: &LightingOptions,
) -> Result<VoxelizeResult, String> {
    let python_path = python_interpreter();
    let script_path = voxelize_script();
    log::info!(
        "Voxelizing: {glb_path} (max_size={max_size}, colors={})",
        sample_colors.is_some()
    );

    let mut cmd = Command::new(&python_path);
    cmd.arg(&script_path)
        .arg("--input")
        .arg(glb_path)
        .arg("--max-size")
        .arg(max_size.to_string());

    if let Some(vs) = voxel_size {
        cmd.arg("--voxel-size").arg(vs.to_string());
    }
    if let Some(ram_limit) = sample_colors {
        let [lx, ly, lz] = lighting.light_dir;
        cmd.arg("--sample-colors")
            .arg(format!("--ram-limit={ram_limit}"))
            .arg(format!("--light-dir={lx},{ly},{lz}"))
            .arg(format!("--light-ambient={}", lighting.ambient))
            .arg(format!("--light-gloss={}", lighting.gloss))
            .arg(format!("--specular={}", lighting.specular))
            .arg(format!("--highlight-rejection={}", lighting.rejection))
            .arg(format!("--highlight-recovery={}", lighting.recovery))
            .arg(format!("--delight={}", lighting.delight));
    }

    cmd.arg("--hollow");

    // Use output() to capture stdout and stderr concurrently (avoids pipe deadlock)
    let result = cmd.output().map_err(|e| {
        format!(
            "Failed to run Python ({python_path}): {e}. Point --python or SCHEMGEN_PYTHON \
                 at an interpreter that has trimesh installed"
        )
    })?;

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);

    // Log stderr for diagnostics
    for line in stderr.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            log::info!("[voxelize.py] {trimmed}");
        }
    }

    if !result.status.success() {
        let code = result.status.code().unwrap_or(-1);
        // Try to extract a clean error from stderr
        let err_msg: String = stderr
            .lines()
            .filter_map(|l| {
                let t = l.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.strip_prefix("ERROR: ").unwrap_or(t).to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("; ");

        let detail = if err_msg.is_empty() {
            format!("exit code {code}")
        } else {
            err_msg
        };
        return Err(format!("Voxelization failed: {detail}"));
    }

    let trimmed_stdout = stdout.trim();
    if trimmed_stdout.is_empty() {
        return Err("Voxelizer produced no output".to_string());
    }

    let output: VoxelOutput = serde_json::from_str(trimmed_stdout).map_err(|e| {
        format!(
            "Failed to parse voxel output: {e}. Got: {}",
            &trimmed_stdout[..trimmed_stdout.len().min(200)]
        )
    })?;

    let (sx, sy, sz) = (
        output.grid_dims[0] as u32,
        output.grid_dims[1] as u32,
        output.grid_dims[2] as u32,
    );

    log::info!(
        "Voxelization complete: {} voxels, grid={}×{}×{}, pitch={}",
        output.coords.len(),
        sx,
        sy,
        sz,
        output.pitch
    );

    if let Some(colors) = &output.colors {
        if colors.len() != output.coords.len() {
            return Err(format!(
                "Color count mismatch: {} colors for {} voxels",
                colors.len(),
                output.coords.len()
            ));
        }
    }

    Ok(VoxelizeResult {
        coords: output.coords,
        grid_dims: (sx, sy, sz),
        colors: output.colors,
    })
}
