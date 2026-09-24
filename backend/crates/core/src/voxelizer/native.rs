//! The built-in voxelizer: load the glTF, build the voxel shell, sample colors.

use std::path::Path;

use super::Voxels;
use crate::error::{Error, Result};
use crate::mesh::{self, LoadOptions};
use crate::pipeline::{Progress, Stage};
use crate::sample::{self, widen, Materials, SampleOptions, SAMPLES_PER_VOXEL};
use crate::settings::Settings;
use crate::voxel;

pub(super) fn voxelize(
    input: &Path,
    settings: &Settings,
    materials: Materials,
    progress: &mut dyn Progress,
) -> Result<Voxels> {
    progress.update(Stage::Voxelize, 0.03, "Reading the model…");
    let model = mesh::load(
        input,
        LoadOptions {
            textures: settings.color_sampling,
        },
    )?;
    if progress.is_cancelled() {
        return Err(Error::Cancelled);
    }
    log::info!(
        "Loaded {}: {} primitives, {} triangles",
        input.display(),
        model.primitives.len(),
        model.triangle_count()
    );

    let shell = voxel::voxelize(
        &model,
        settings.voxel_size.map(widen),
        settings.max_size,
        progress,
    )?;
    if shell.coords.is_empty() {
        return Err(Error::NoVoxels);
    }
    // The Python helper handed its origin over as single precision, and the
    // sampler bins against exactly that.
    let origin = shell.origin.map(|v| v as f32);

    let colors = if settings.color_sampling {
        let options = SampleOptions {
            materials,
            samples_per_voxel: SAMPLES_PER_VOXEL,
            ram_limit: settings.ram_limit,
        };
        Some(sample::sample_colors(
            &model,
            &shell.coords,
            shell.pitch,
            origin,
            &settings.lighting(),
            &options,
            progress,
        )?)
    } else {
        None
    };
    log::info!(
        "Voxelization complete: {} voxels, pitch={}",
        shell.coords.len(),
        shell.pitch
    );

    Ok(Voxels {
        coords: shell.coords,
        colors,
        pitch: shell.pitch as f32,
        origin,
    })
}
