use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// CIE L*a*b* color (D65 illuminant, 2° observer).
/// L: 0–100, a: approximately −128…+128, b: approximately −128…+128
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

/// sRGB color, 0–255.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// One color entry for a block (from color_table_safe.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockColorEntry {
    pub lab: [f32; 3],
    pub rgb: [f32; 3],
    pub weight: f32,
}

/// The full color table: block_name -> list of dominant colors
pub type ColorTable = HashMap<String, Vec<BlockColorEntry>>;

/// How the color sampler separates baked lighting from albedo.
///
/// `light_dir` is in model space — glTF's Y-up right-handed frame, which is
/// also three.js's — so the direction the preview draws is the one the sampler
/// removes, with no conversion in between.
///
/// Built from [`crate::Settings`], whose defaults mirror `DEFAULT_LIGHTING` in
/// `scripts/sample_colors.py`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LightingOptions {
    /// Key light direction, model space. Normalized by the sampler.
    pub light_dir: [f32; 3],
    /// Fraction of full illumination still reaching surfaces facing away.
    pub ambient: f32,
    /// How sharp a highlight to assume the texture was baked with.
    pub gloss: f32,
    /// Gain on the highlight lobe.
    pub specular: f32,
    /// How hard to discount samples sitting inside that lobe.
    pub rejection: f32,
    /// How far a blown-out voxel is rebuilt from its material's albedo.
    pub recovery: f32,
    /// Strength of taking the assumed lighting back out of the albedo.
    pub delight: f32,
}
