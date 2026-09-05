//! Conversion pipeline orchestrator.
//!
//! Stages:
//!   1. Voxelize + sample surface colors (one Python trimesh subprocess —
//!      the GLB is parsed once for both)
//!   2. Apply optional dithering
//!   3. Match blocks via CIEDE2000 palette
//!   4. Write .litematic

use std::path::Path;
use rayon::prelude::*;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::dithering;
use crate::palette::Palette;
use crate::types::{ConversionResult, ConversionOptions, Lab, ProgressEvent};
use crate::voxelizer;

/// Run the full conversion pipeline.
/// Reports progress via the sender channel.
pub async fn convert(
    input_path: &str,
    output_path: &str,
    options: &ConversionOptions,
    palette: &Palette,
    progress_tx: mpsc::UnboundedSender<ProgressEvent>,
) -> Result<ConversionResult, String> {
    let t0 = std::time::Instant::now();
    let send = |pct: f32, msg: &str| {
        let _ = progress_tx.send(ProgressEvent {
            pct, msg: msg.to_string(), speed: None, done: None,
            error: None, detail: None, result: None, download_name: None,
        });
    };

    // Determine Python path
    let python_path = if cfg!(windows) { "python" } else { "python3" };

    // Determine script paths relative to backend root
    let script_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts");
    let voxelize_script = script_dir.join("voxelize.py");

    send(0.05, if options.use_color_sampling {
        "Voxelizing mesh and sampling surface colors..."
    } else {
        "Voxelizing mesh (hollow mode)..."
    });

    // 1. Voxelize (+ color sampling in the same subprocess)
    let vox = voxelizer::voxelize(
        input_path,
        options.max_size,
        options.voxel_size,
        options.use_color_sampling.then_some(options.ram_limit),
        &options.lighting,
        python_path,
        voxelize_script.to_str().unwrap_or("scripts/voxelize.py"),
    ).map_err(|e| format!("Voxelization failed: {e}"))?;

    let n_voxels = vox.coords.len();
    let (sx, sy, sz) = vox.grid_dims;
    send(0.50, &format!("Voxelized: {n_voxels} surface voxels ({sx}×{sy}×{sz})"));

    let mut colors = match vox.colors {
        Some(colors) => colors,
        None => vec![[255.0, 255.0, 255.0]; n_voxels],
    };
    if options.use_color_sampling {
        apply_color_adjustments(&mut colors, options.brightness, options.contrast, options.saturation);
    }

    // 2. Dithering
    let dithered = if options.use_dithering {
        send(0.55, "Applying 3D dithering...");
        dithering::apply_dithering(&vox.coords, &colors, 28.0)
    } else {
        colors
    };

    // 3. Block matching (borrowed names — no per-voxel String allocation)
    let (block_names, unique): (Vec<&str>, usize) = if options.use_color_sampling {
        send(0.60, "Matching blocks (CIEDE2000)...");
        let lab_queries: Vec<Lab> = dithered.par_iter()
            .map(|c| crate::palette::rgb_to_lab_vec(*c))
            .collect();

        let indices = palette.match_indices_batch(&lab_queries, 7);
        let names: Vec<&str> = indices.iter().map(|&i| palette.name_of(i)).collect();
        let unique = count_unique(&names);
        send(0.65, &format!("Block matching complete: {unique} unique blocks"));
        (names, unique)
    } else {
        (vec![options.default_block_name.as_str(); n_voxels], 1)
    };

    // 4. Write litematic
    send(0.70, "Writing .litematic...");
    crate::litematic::write_litematic(
        output_path,
        &vox.coords,
        &block_names,
        &options.schematic_name,
        "SchemGen2",
        "Converted from GLB",
    ).map_err(|e| format!("Litematic write failed: {e}"))?;

    let elapsed = t0.elapsed().as_secs_f32();
    let result = ConversionResult {
        voxel_count: n_voxels,
        unique_blocks: unique,
        grid: vox.grid_dims,
        elapsed,
    };

    send(1.0, &format!("Done! {n_voxels} blocks in {elapsed:.1}s"));

    Ok(result)
}

#[derive(Serialize)]
pub struct PreviewBlock {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub name: String,
}

#[derive(Serialize)]
pub struct PreviewSchematic {
    pub grid: (u32, u32, u32),
    pub blocks: Vec<PreviewBlock>,
}

/// Build the same schematic used by conversion at a small preview resolution.
pub async fn preview_litematic(input_path: &str, output_path: &str, options: &ConversionOptions, palette: &Palette) -> Result<PreviewSchematic, String> {
    let python_path = if cfg!(windows) { "python" } else { "python3" };
    let script_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts");
    let vox = voxelizer::voxelize(
        input_path, options.max_size, options.voxel_size,
        options.use_color_sampling.then_some(options.ram_limit),
        &options.lighting,
        python_path,
        script_dir.join("voxelize.py").to_str().unwrap_or("scripts/voxelize.py"),
    ).map_err(|e| format!("Voxelization failed: {e}"))?;
    let coords = vox.coords;
    let mut colors = match vox.colors {
        Some(colors) => colors,
        None => vec![[255.0, 255.0, 255.0]; coords.len()],
    };
    apply_color_adjustments(&mut colors, options.brightness, options.contrast, options.saturation);
    let colors = if options.use_dithering { dithering::apply_dithering(&coords, &colors, 28.0) } else { colors };
    let names: Vec<&str> = if options.use_color_sampling {
        let queries: Vec<Lab> = colors.par_iter().map(|c| crate::palette::rgb_to_lab_vec(*c)).collect();
        palette.match_indices_batch(&queries, 7).iter()
            .map(|&i| palette.name_of(i)).collect()
    } else { vec![options.default_block_name.as_str(); coords.len()] };
    crate::litematic::write_litematic(output_path, &coords, &names, "preview", "SchemGen2", "Preview of converted GLB")
        .map_err(|e| format!("Litematic write failed: {e}"))?;
    Ok(PreviewSchematic {
        grid: vox.grid_dims,
        blocks: coords.into_iter().zip(names).map(|(coord, name)| PreviewBlock {
            x: coord[0], y: coord[1], z: coord[2], name: name.to_string(),
        }).collect(),
    })
}
/// Number of distinct names, without building per-voxel Strings.
fn count_unique(names: &[&str]) -> usize {
    let seen: std::collections::HashSet<&str> = names.iter().copied().collect();
    seen.len()
}

fn apply_color_adjustments(colors: &mut [[f32; 3]], brightness: f32, contrast: f32, saturation: f32) {
    colors.par_iter_mut().for_each(|color| {
        let mut rgb = color.map(|channel| channel / 255.0);
        rgb = rgb.map(|channel| (channel - 0.5) * contrast + 0.5 + brightness);
        let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        rgb = rgb.map(|channel| luma + (channel - luma) * saturation);
        *color = rgb.map(|channel| channel.clamp(0.0, 1.0) * 255.0);
    });
}

