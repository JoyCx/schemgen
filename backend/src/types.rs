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

/// One color entry for a block (from color_table.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockColorEntry {
    pub lab: [f32; 3],
    pub rgb: [f32; 3],
    pub weight: f32,
}

/// The full color table: block_name -> list of dominant colors
pub type ColorTable = HashMap<String, Vec<BlockColorEntry>>;

/// Conversion job status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Done,
    Error,
}

/// How the color sampler separates baked lighting from albedo.
///
/// `light_dir` is in model space — glTF's Y-up right-handed frame, which is
/// also three.js's — so the direction the preview draws is the one the sampler
/// removes, with no conversion in between.
///
/// These defaults mirror `DEFAULT_LIGHTING` in `scripts/sample_colors.py`,
/// which stays authoritative for anyone driving the script directly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

impl Default for LightingOptions {
    fn default() -> Self {
        Self {
            light_dir: [0.35, 0.85, 0.40],
            ambient: 0.32,
            gloss: 0.5,
            specular: 1.1,
            rejection: 0.75,
            recovery: 1.0,
            delight: 0.0,
        }
    }
}

/// Conversion options sent by the frontend.
#[derive(Debug, Clone, Deserialize)]
pub struct ConversionOptions {
    #[serde(default = "default_max_size")]
    pub max_size: u32,
    pub voxel_size: Option<f32>,
    #[serde(default = "default_ram_limit")]
    pub ram_limit: f32,
    #[serde(default = "default_true")]
    pub use_dithering: bool,
    #[serde(default = "default_true")]
    pub use_color_sampling: bool,
    #[serde(default = "default_brightness")]
    pub brightness: f32,
    #[serde(default = "default_contrast")]
    pub contrast: f32,
    #[serde(default = "default_saturation")]
    pub saturation: f32,
    #[serde(default = "default_block")]
    pub default_block_name: String,
    #[serde(default)]
    pub schematic_name: String,
    #[serde(default)]
    pub lighting: LightingOptions,
}

fn default_max_size() -> u32 { 128 }
fn default_ram_limit() -> f32 { 4.0 }
fn default_true() -> bool { true }
fn default_brightness() -> f32 { 0.0 }
fn default_contrast() -> f32 { 1.0 }
fn default_saturation() -> f32 { 1.0 }
fn default_block() -> String { "minecraft:white_concrete".to_string() }

/// Progress event sent via SSE.
#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    pub pct: f32,
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ConversionResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_name: Option<String>,
}

/// Final conversion result.
#[derive(Debug, Clone, Serialize)]
pub struct ConversionResult {
    pub voxel_count: usize,
    pub unique_blocks: usize,
    pub grid: (u32, u32, u32),
    pub elapsed: f32,
}

/// A conversion job stored in shared state.
#[derive(Debug, Clone)]
pub struct ConversionJob {
    pub status: JobStatus,
    pub progress: f32,
    pub message: String,
    pub download_name: String,
    pub output_path: String,
    /// Where the finished schematic was copied, when a UI output folder is set.
    pub saved_path: Option<String>,
    /// Why that copy failed, if it did (the conversion itself still succeeded).
    pub save_error: Option<String>,
}
