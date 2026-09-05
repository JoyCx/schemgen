//! 8×8 Bayer matrix ordered dithering — direct port from Python.
//! Fully deterministic, zero dependencies, O(N).

use rayon::prelude::*;

/// 8×8 Bayer threshold matrix, values in [−0.5, +0.5)
const BAYER_8: [[f32; 8]; 8] = [
    [ 0.0, 32.0,  8.0, 40.0,  2.0, 34.0, 10.0, 42.0],
    [48.0, 16.0, 56.0, 24.0, 50.0, 18.0, 58.0, 26.0],
    [12.0, 44.0,  4.0, 36.0, 14.0, 46.0,  6.0, 38.0],
    [60.0, 28.0, 52.0, 20.0, 62.0, 30.0, 54.0, 22.0],
    [ 3.0, 35.0, 11.0, 43.0,  1.0, 33.0,  9.0, 41.0],
    [51.0, 19.0, 59.0, 27.0, 49.0, 17.0, 57.0, 25.0],
    [15.0, 47.0,  7.0, 39.0, 13.0, 45.0,  5.0, 37.0],
    [63.0, 31.0, 55.0, 23.0, 61.0, 29.0, 53.0, 21.0],
];

/// Apply ordered (Bayer) dithering to voxel colors.
///
/// `voxel_coords`: (N, 3) voxel grid positions (x, y, z) as i32
/// `colors`: (N, 3) sampled RGB colors [0, 255]
/// `strength`: dither amplitude in RGB units (~28 = 11% of range)
///
/// Returns perturbed colors clipped to [0, 255].
pub fn apply_dithering(
    voxel_coords: &[[i32; 3]],
    colors: &[[f32; 3]],
    strength: f32,
) -> Vec<[f32; 3]> {
    if voxel_coords.is_empty() {
        return colors.to_vec();
    }

    // Each voxel's offset depends only on its own coordinate, so this maps
    // in parallel while preserving input order.
    voxel_coords.par_iter()
        .zip(colors.par_iter())
        .map(|(&[x, y, z], color)| {
            let bx = (x & 7) as usize; // x % 8
            let bz = (z & 7) as usize;
            let by = (y & 7) as usize;
            let bx2 = ((z + 3) & 7) as usize; // phase-shifted

            let threshold = (BAYER_8[bz][bx] / 64.0 - 0.5) + (BAYER_8[by][bx2] / 64.0 - 0.5) * 0.35;
            let offset = threshold * strength;

            [
                (color[0] + offset).clamp(0.0, 255.0),
                (color[1] + offset).clamp(0.0, 255.0),
                (color[2] + offset).clamp(0.0, 255.0),
            ]
        })
        .collect()
}
