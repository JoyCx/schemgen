//! The conversion pipeline — one function, shared by every front end.
//!
//! ```text
//! voxelize + sample colors → brightness/contrast/saturation → dither
//!     → CIEDE2000 match → BlockGrid
//! ```
//!
//! A conversion is [`run`] followed by one of the [`crate::formats`] writers;
//! a preview is [`run`] at a lower resolution with no file at all.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;

use crate::dither;
use crate::error::{Error, Result};
use crate::grid::BlockGrid;
use crate::palette::Palette;
use crate::settings::Settings;
use crate::voxelizer::{self, Voxels};

/// Dither amplitude in RGB units — about 11% of the range.
const DITHER_STRENGTH: f32 = 28.0;

/// KD-tree candidates re-ranked by CIEDE2000 per color.
const MATCH_CANDIDATES: usize = 7;

/// Where a running conversion is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Reading the mesh, building the voxel shell and sampling its colors.
    Voxelize,
    /// Brightness, contrast and saturation.
    Adjust,
    Dither,
    /// CIEDE2000 block matching.
    Match,
    Done,
}

impl Stage {
    /// Stable lowercase name, as the API reports it.
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Voxelize => "voxelize",
            Stage::Adjust => "adjust",
            Stage::Dither => "dither",
            Stage::Match => "match",
            Stage::Done => "done",
        }
    }
}

/// Receives progress from [`run`], and can ask it to stop.
pub trait Progress: Send {
    /// `fraction` is the whole run's progress, 0 to 1.
    fn update(&mut self, stage: Stage, fraction: f32, message: &str);

    /// Polled between stages and while the voxelizer runs; returning `true`
    /// makes [`run`] give up with [`Error::Cancelled`].
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// A [`Progress`] that ignores everything and never cancels.
pub struct NoProgress;

impl Progress for NoProgress {
    fn update(&mut self, _: Stage, _: f32, _: &str) {}
}

/// A cancellation flag that can be shared between whoever may cancel a
/// conversion and the [`Progress`] it runs with.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Convert the model at `input` to blocks.
///
/// `settings` should already be [`Settings::normalized`]. The result carries
/// no file: pass it to a [`crate::formats`] writer to produce one.
pub fn run(
    input: &Path,
    settings: &Settings,
    palette: &Palette,
    progress: &mut dyn Progress,
) -> Result<BlockGrid> {
    let started = Instant::now();
    check_input(input)?;
    progress.update(
        Stage::Voxelize,
        0.02,
        if settings.color_sampling {
            "Voxelizing mesh and sampling surface colors…"
        } else {
            "Voxelizing mesh…"
        },
    );
    let voxels = voxelizer::voxelize(input, settings, progress)?;
    let grid = blocks(voxels, settings, palette, progress)?;

    let [x, y, z] = grid.size;
    progress.update(
        Stage::Done,
        1.0,
        &format!(
            "{} blocks, {} kinds, {x}×{y}×{z}, {:.1}s",
            grid.len(),
            grid.names.len(),
            started.elapsed().as_secs_f32()
        ),
    );
    Ok(grid)
}

/// The blocks for voxels a voxelizer produced: adjust, dither and match their
/// colors, or fill them with the default block when color sampling is off.
pub fn blocks(
    voxels: Voxels,
    settings: &Settings,
    palette: &Palette,
    progress: &mut dyn Progress,
) -> Result<BlockGrid> {
    let checkpoint = |progress: &mut dyn Progress| {
        if progress.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    };
    let n = voxels.coords.len();
    if n == 0 {
        return Err(Error::NoVoxels);
    }
    progress.update(
        Stage::Voxelize,
        0.85,
        &format!("Voxelized: {n} surface blocks"),
    );
    checkpoint(progress)?;

    let grid = if settings.color_sampling {
        let mut colors = voxels
            .colors
            .unwrap_or_else(|| vec![[255.0, 255.0, 255.0]; n]);

        progress.update(Stage::Adjust, 0.86, "Adjusting colors…");
        adjust_colors(
            &mut colors,
            settings.brightness,
            settings.contrast,
            settings.saturation,
        );

        if settings.dither {
            progress.update(Stage::Dither, 0.88, "Applying 3D dithering…");
            colors = dither::apply_dithering(&voxels.coords, &colors, DITHER_STRENGTH);
        }
        checkpoint(progress)?;

        progress.update(Stage::Match, 0.90, "Matching blocks (CIEDE2000)…");
        let indices = palette.match_colors(&colors, MATCH_CANDIDATES);
        BlockGrid::from_indices(
            voxels.coords,
            &indices,
            |i| palette.name_of(i),
            voxels.origin,
            voxels.pitch,
        )
    } else {
        let block = settings.default_block.as_str();
        BlockGrid::from_names(
            voxels.coords,
            std::iter::repeat_n(block, n),
            voxels.origin,
            voxels.pitch,
        )
    };
    Ok(grid)
}

/// Only glTF goes in; say so before starting a voxelizer on anything else.
fn check_input(input: &Path) -> Result<()> {
    let lower = input.to_string_lossy().to_ascii_lowercase();
    if !(lower.ends_with(".glb") || lower.ends_with(".gltf")) {
        return Err(Error::UnsupportedInput(input.display().to_string()));
    }
    if !input.is_file() {
        return Err(Error::Read {
            path: input.to_path_buf(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        });
    }
    Ok(())
}

/// Brightness, contrast and saturation, applied in sRGB space around mid-grey.
pub fn adjust_colors(colors: &mut [[f32; 3]], brightness: f32, contrast: f32, saturation: f32) {
    if brightness == 0.0 && contrast == 1.0 && saturation == 1.0 {
        return;
    }
    colors.par_iter_mut().for_each(|color| {
        let mut rgb = color.map(|channel| channel / 255.0);
        rgb = rgb.map(|channel| (channel - 0.5) * contrast + 0.5 + brightness);
        let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        rgb = rgb.map(|channel| luma + (channel - luma) * saturation);
        *color = rgb.map(|channel| channel.clamp(0.0, 1.0) * 255.0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_adjustment_is_a_no_op() {
        let mut colors = vec![[12.5, 200.25, 255.0], [0.0, 0.0, 0.0]];
        let before = colors.clone();
        adjust_colors(&mut colors, 0.0, 1.0, 1.0);
        assert_eq!(colors, before);
    }

    #[test]
    fn zero_saturation_is_grey() {
        let mut colors = vec![[200.0, 50.0, 10.0]];
        adjust_colors(&mut colors, 0.0, 1.0, 0.0);
        let [r, g, b] = colors[0];
        assert!(
            (r - g).abs() < 1e-3 && (g - b).abs() < 1e-3,
            "{:?}",
            colors[0]
        );
    }

    #[test]
    fn non_gltf_input_is_rejected_up_front() {
        let err = run(
            Path::new("model.obj"),
            &Settings::default(),
            &crate::palette::tests::two_block_palette(),
            &mut NoProgress,
        )
        .unwrap_err();
        assert!(matches!(err, Error::UnsupportedInput(_)), "{err}");
        assert!(err.is_user_error());
    }

    #[test]
    fn cancel_flag_is_shared() {
        let cancel = Cancel::new();
        let clone = cancel.clone();
        assert!(!clone.is_cancelled());
        cancel.cancel();
        assert!(clone.is_cancelled());
    }
}
