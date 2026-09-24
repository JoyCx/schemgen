//! The one error type the pipeline, the palette and the writers return.

use std::path::PathBuf;

/// Everything that can stop a conversion.
///
/// The messages are written for the person who started it: the server sends
/// them to the UI as they are and the CLI prints them after `error:`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a glTF model: {0} (expected .glb or .gltf)")]
    UnsupportedInput(String),

    #[error("could not read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not load the model: {0}")]
    Mesh(String),

    #[error("the model has no triangle geometry")]
    NoGeometry,

    #[error("voxelization produced no voxels — try a larger max size or a smaller voxel size")]
    NoVoxels,

    #[error("voxelization failed: {0}")]
    Voxelizer(String),

    #[error("invalid {field}: {reason}")]
    InvalidSetting { field: &'static str, reason: String },

    #[error("the color table is empty")]
    EmptyPalette,

    #[error(
        "no usable block textures in {} — point it at assets/minecraft/textures/block",
        .0.display()
    )]
    NoTextures(PathBuf),

    #[error("conversion cancelled")]
    Cancelled,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Error::InvalidSetting {
            field,
            reason: reason.into(),
        }
    }

    /// Whether the error is the caller's doing (a bad setting or input) rather
    /// than something going wrong while converting — an HTTP 400, not a 500.
    pub fn is_user_error(&self) -> bool {
        matches!(
            self,
            Error::UnsupportedInput(_) | Error::InvalidSetting { .. }
        )
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
