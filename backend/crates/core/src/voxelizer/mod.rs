//! Surface voxelization and per-voxel color sampling.

use std::path::Path;

use crate::error::Result;
use crate::pipeline::Progress;
use crate::settings::Settings;

mod python;

pub use python::{python_interpreter, set_python};

/// The voxel shell of a model and, when asked for, its surface colors.
#[derive(Debug, Clone)]
pub struct Voxels {
    /// Voxel positions, non-negative, with the model's minimum corner at 0.
    pub coords: Vec<[i32; 3]>,
    /// sRGB, 0–255 per channel, one per voxel — present when color sampling ran.
    pub colors: Option<Vec<[f32; 3]>>,
    /// Model units per voxel.
    pub pitch: f32,
    /// Model-space position of voxel (0, 0, 0)'s minimum corner.
    pub origin: [f32; 3],
}

/// Voxelize the model at `input`, sampling colors when
/// `settings.color_sampling` is on.
pub fn voxelize(input: &Path, settings: &Settings, progress: &mut dyn Progress) -> Result<Voxels> {
    python::voxelize(input, settings, progress)
}
