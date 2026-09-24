//! Surface voxelization and per-voxel color sampling.
//!
//! The native implementation ([`crate::mesh`], [`crate::voxel`],
//! [`crate::sample`]) is the default. Builds with the `python-voxelizer`
//! feature can still run SchemGen2 2.0's Python/trimesh helper instead, for
//! one release, selected with [`set_backend`].

use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::error::Result;
use crate::pipeline::Progress;
use crate::sample::Materials;
use crate::settings::Settings;

mod native;
#[cfg(feature = "python-voxelizer")]
pub mod python;

#[cfg(feature = "python-voxelizer")]
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

/// Which implementation voxelizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    /// Built in (Rust).
    #[default]
    Native,
    /// The Python/trimesh helper, `scripts/voxelize.py`.
    #[cfg(feature = "python-voxelizer")]
    Python,
}

impl Backend {
    /// `rust` (or `native`), or `python` in builds that have it.
    pub fn parse(name: &str) -> std::result::Result<Self, String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "rust" | "native" => Ok(Backend::Native),
            #[cfg(feature = "python-voxelizer")]
            "python" => Ok(Backend::Python),
            #[cfg(not(feature = "python-voxelizer"))]
            "python" => Err(
                "this build has no Python voxelizer (it is only in builds with \
                 the python-voxelizer feature)"
                    .into(),
            ),
            other => Err(format!(
                "unknown voxelizer {other:?}: expected rust or python"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Backend::Native => "rust",
            #[cfg(feature = "python-voxelizer")]
            Backend::Python => "python",
        }
    }
}

static BACKEND: AtomicU8 = AtomicU8::new(0);

/// Use this implementation for every conversion in this process.
pub fn set_backend(backend: Backend) {
    BACKEND.store(
        match backend {
            Backend::Native => 0,
            #[cfg(feature = "python-voxelizer")]
            Backend::Python => 1,
        },
        Ordering::Relaxed,
    );
}

pub fn backend() -> Backend {
    match BACKEND.load(Ordering::Relaxed) {
        #[cfg(feature = "python-voxelizer")]
        1 => Backend::Python,
        _ => Backend::Native,
    }
}

/// Voxelize the model at `input`, sampling colors when
/// `settings.color_sampling` is on.
pub fn voxelize(input: &Path, settings: &Settings, progress: &mut dyn Progress) -> Result<Voxels> {
    match backend() {
        Backend::Native => native::voxelize(input, settings, Materials::Gltf, progress),
        #[cfg(feature = "python-voxelizer")]
        Backend::Python => python::voxelize(input, settings, progress),
    }
}

/// The native voxelizer, reading materials as `materials` says — for the
/// parity harness, which compares against the Python helper like for like.
pub fn voxelize_native(
    input: &Path,
    settings: &Settings,
    materials: Materials,
    progress: &mut dyn Progress,
) -> Result<Voxels> {
    native::voxelize(input, settings, materials, progress)
}
