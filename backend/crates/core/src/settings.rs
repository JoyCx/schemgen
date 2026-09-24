//! Conversion settings — the one definition the command line, the HTTP API,
//! the web UI and the mod all share.
//!
//! A front end fills in whichever fields it exposes, leaves the rest at
//! [`Settings::default`], and calls [`Settings::normalized`]. The defaults and
//! ranges below are also what [`crate::schema`] publishes, so a UI rendered
//! from the schema cannot drift from what the pipeline accepts.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::formats::Format;
use crate::targets::{Target, DEFAULT_TARGET};
use crate::types::LightingOptions;

/// Default key light, model space: above, in front and a little to the right.
/// Mirrors `DEFAULT_LIGHT_DIR` in `scripts/sample_colors.py`.
pub const DEFAULT_LIGHT_DIR: [f32; 3] = [0.35, 0.85, 0.40];

/// Block placed everywhere when color sampling is off.
pub const DEFAULT_BLOCK: &str = "minecraft:white_concrete";

/// Inclusive range of an integer setting.
#[derive(Debug, Clone, Copy)]
pub struct IntRange {
    pub min: u32,
    pub max: u32,
}

/// Inclusive range of a numeric setting.
#[derive(Debug, Clone, Copy)]
pub struct FloatRange {
    pub min: f32,
    pub max: f32,
}

impl FloatRange {
    fn clamp(self, field: &'static str, value: f32) -> Result<f32> {
        if !value.is_finite() {
            return Err(Error::invalid(field, "must be a finite number"));
        }
        Ok(value.clamp(self.min, self.max))
    }
}

/// The accepted range of every bounded setting. Values outside are clamped
/// rather than rejected, as they always have been by both front ends.
pub mod limits {
    use super::{FloatRange, IntRange};

    pub const MAX_SIZE: IntRange = IntRange { min: 1, max: 2048 };
    pub const BRIGHTNESS: FloatRange = FloatRange {
        min: -1.0,
        max: 1.0,
    };
    pub const CONTRAST: FloatRange = FloatRange { min: 0.0, max: 3.0 };
    pub const SATURATION: FloatRange = FloatRange { min: 0.0, max: 3.0 };
    pub const UNIT: FloatRange = FloatRange { min: 0.0, max: 1.0 };
    pub const SPECULAR: FloatRange = FloatRange { min: 0.0, max: 4.0 };
    pub const RAM_LIMIT: FloatRange = FloatRange {
        min: 0.5,
        max: 256.0,
    };
    /// Longest schematic name kept; NBT strings are length-prefixed.
    pub const NAME_CHARS: usize = 200;
}

/// Every setting that changes the blocks a conversion produces.
///
/// Serialized field names are the API's: `POST /api/jobs` takes this object
/// as JSON, and `GET /api/schema` describes each field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Longest axis of the result, in blocks.
    pub max_size: u32,
    /// Explicit voxel pitch in model units; overrides `max_size` when set.
    pub voxel_size: Option<f32>,
    /// Match each voxel's sampled color; off places `default_block` everywhere.
    pub color_sampling: bool,
    /// 8×8 Bayer ordered dithering before matching.
    pub dither: bool,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// Block used everywhere when `color_sampling` is off.
    pub default_block: String,
    /// Key light direction, model space (glTF / three.js Y-up).
    pub light_dir: [f32; 3],
    pub light_ambient: f32,
    pub light_gloss: f32,
    pub specular: f32,
    pub highlight_rejection: f32,
    pub highlight_recovery: f32,
    pub delight: f32,
    /// Minecraft version to write for: a game version such as `1.21.8`, or a
    /// bare data version. Decides the stamp and which blocks may be used.
    pub target: String,
    /// File format: `litematic`, `schem`, `schem-v3` or `nbt`.
    pub format: String,
    /// Name stored in the schematic; empty means the input file's stem.
    pub schematic_name: String,
    /// Memory budget for color sampling, in GB.
    pub ram_limit: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_size: 128,
            voxel_size: None,
            color_sampling: true,
            dither: true,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            default_block: DEFAULT_BLOCK.to_string(),
            light_dir: DEFAULT_LIGHT_DIR,
            light_ambient: 0.32,
            light_gloss: 0.5,
            specular: 1.1,
            highlight_rejection: 0.75,
            highlight_recovery: 1.0,
            delight: 0.0,
            target: DEFAULT_TARGET.to_string(),
            format: Format::Litematic.id().to_string(),
            schematic_name: String::new(),
            ram_limit: 4.0,
        }
    }
}

impl Settings {
    /// Clamp every value into its range and canonicalize names.
    ///
    /// Only values with no sensible nearest valid value are errors: a
    /// non-finite number, a negative voxel size, a light direction of zero
    /// length, a block ID that cannot be one.
    pub fn normalized(mut self) -> Result<Self> {
        use limits::*;

        self.max_size = self.max_size.clamp(MAX_SIZE.min, MAX_SIZE.max);
        self.voxel_size = match self.voxel_size {
            None => None,
            // 0 is how a form says "derive it from max_size".
            Some(0.0) => None,
            Some(v) if v.is_finite() && v > 0.0 => Some(v),
            Some(_) => {
                return Err(Error::invalid(
                    "voxel_size",
                    "must be a positive number, or empty to derive it from max_size",
                ))
            }
        };

        self.brightness = BRIGHTNESS.clamp("brightness", self.brightness)?;
        self.contrast = CONTRAST.clamp("contrast", self.contrast)?;
        self.saturation = SATURATION.clamp("saturation", self.saturation)?;
        self.default_block = block_id(&self.default_block)?;

        let [x, y, z] = self.light_dir;
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return Err(Error::invalid("light_dir", "must be three finite numbers"));
        }
        if x * x + y * y + z * z < 1e-12 {
            return Err(Error::invalid(
                "light_dir",
                "the zero vector has no direction",
            ));
        }
        self.light_ambient = UNIT.clamp("light_ambient", self.light_ambient)?;
        self.light_gloss = UNIT.clamp("light_gloss", self.light_gloss)?;
        self.specular = SPECULAR.clamp("specular", self.specular)?;
        self.highlight_rejection = UNIT.clamp("highlight_rejection", self.highlight_rejection)?;
        self.highlight_recovery = UNIT.clamp("highlight_recovery", self.highlight_recovery)?;
        self.delight = UNIT.clamp("delight", self.delight)?;

        self.target = Target::parse(&self.target)?.key();
        self.format = Format::parse(&self.format)?.id().to_string();
        self.schematic_name = self
            .schematic_name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(NAME_CHARS)
            .collect();
        self.ram_limit = RAM_LIMIT.clamp("ram_limit", self.ram_limit)?;
        Ok(self)
    }

    /// The lighting-separation half of the settings, as the sampler wants it.
    pub fn lighting(&self) -> LightingOptions {
        LightingOptions {
            light_dir: self.light_dir,
            ambient: self.light_ambient,
            gloss: self.light_gloss,
            specular: self.specular,
            rejection: self.highlight_rejection,
            recovery: self.highlight_recovery,
            delight: self.delight,
        }
    }

    /// The target Minecraft version. Settings that were not normalized fall
    /// back to the default target when theirs does not parse.
    pub fn target(&self) -> Target {
        Target::parse(&self.target).unwrap_or_default()
    }

    /// The file format. Settings that were not normalized fall back to
    /// `.litematic` when theirs does not parse.
    pub fn format(&self) -> Format {
        Format::parse(&self.format).unwrap_or(Format::Litematic)
    }

    /// The name to store in the schematic: the explicit one, else `fallback`
    /// (normally the input file's stem).
    pub fn name_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.schematic_name.is_empty() {
            fallback
        } else {
            &self.schematic_name
        }
    }
}

/// Canonical block ID for what a user typed: the web UI's short names
/// (`white`, `netherrack`), a bare ID (`deepslate`) or a full one.
pub fn block_id(raw: &str) -> Result<String> {
    let raw = raw.trim();
    let id = match raw {
        "" | "white" => DEFAULT_BLOCK.to_string(),
        "netherrack" => "minecraft:netherrack".to_string(),
        other if other.contains(':') => other.to_ascii_lowercase(),
        other => format!("minecraft:{}", other.to_ascii_lowercase()),
    };
    let valid = id.split_once(':').is_some_and(|(ns, path)| {
        !ns.is_empty()
            && !path.is_empty()
            && ns
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
            && path
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-./".contains(c))
    });
    if valid {
        Ok(id)
    } else {
        Err(Error::invalid(
            "default_block",
            format!("\"{raw}\" is not a block ID"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_already_normal() {
        let d = Settings::default();
        assert_eq!(d.clone().normalized().unwrap(), d);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let s = Settings {
            max_size: 0,
            saturation: 9.0,
            brightness: -4.0,
            specular: 10.0,
            ram_limit: 0.0,
            ..Settings::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(s.max_size, 1);
        assert_eq!(s.saturation, 3.0);
        assert_eq!(s.brightness, -1.0);
        assert_eq!(s.specular, 4.0);
        assert_eq!(s.ram_limit, 0.5);
    }

    #[test]
    fn meaningless_values_are_errors() {
        let bad = |s: Settings| s.normalized().is_err();
        assert!(bad(Settings {
            light_dir: [0.0, 0.0, 0.0],
            ..Settings::default()
        }));
        assert!(bad(Settings {
            contrast: f32::NAN,
            ..Settings::default()
        }));
        assert!(bad(Settings {
            voxel_size: Some(-1.0),
            ..Settings::default()
        }));
        assert!(bad(Settings {
            default_block: "not a block!".into(),
            ..Settings::default()
        }));
        assert!(bad(Settings {
            target: "1.12.2".into(),
            ..Settings::default()
        }));
        assert!(bad(Settings {
            format: "schematic".into(),
            ..Settings::default()
        }));
    }

    #[test]
    fn targets_are_canonicalized() {
        let t = |raw: &str| {
            Settings {
                target: raw.into(),
                ..Settings::default()
            }
            .normalized()
            .unwrap()
            .target
        };
        assert_eq!(t(" 1.20.4 "), "1.20.4");
        assert_eq!(t("3700"), "1.20.4");
        assert_eq!(t("4435"), "4435");
        assert_eq!(Settings::default().target().data_version, 4440);
    }

    #[test]
    fn zero_voxel_size_means_auto() {
        let s = Settings {
            voxel_size: Some(0.0),
            ..Settings::default()
        };
        assert_eq!(s.normalized().unwrap().voxel_size, None);
    }

    #[test]
    fn block_ids_accept_short_and_full_names() {
        assert_eq!(block_id("white").unwrap(), "minecraft:white_concrete");
        assert_eq!(block_id("").unwrap(), "minecraft:white_concrete");
        assert_eq!(block_id("netherrack").unwrap(), "minecraft:netherrack");
        assert_eq!(block_id("minecraft:stone").unwrap(), "minecraft:stone");
        assert_eq!(block_id("Deepslate").unwrap(), "minecraft:deepslate");
        assert!(block_id("a:").is_err());
        assert!(block_id("bad id").is_err());
    }

    #[test]
    fn json_fields_are_optional() {
        let s: Settings = serde_json::from_str(r#"{"max_size": 64, "delight": 0.5}"#).unwrap();
        assert_eq!(s.max_size, 64);
        assert_eq!(s.delight, 0.5);
        assert_eq!(s.contrast, Settings::default().contrast);
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        let s = Settings {
            schematic_name: format!("  castle\u{7}{}", "x".repeat(500)),
            ..Settings::default()
        }
        .normalized()
        .unwrap();
        assert!(s.schematic_name.starts_with("castlex"));
        assert_eq!(s.schematic_name.chars().count(), limits::NAME_CHARS);
        assert_eq!(s.name_or("model"), s.schematic_name);
        assert_eq!(Settings::default().name_or("model"), "model");
    }
}
