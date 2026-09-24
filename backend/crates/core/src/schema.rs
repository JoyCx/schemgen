//! The settings schema: every field a front end can show, with its type,
//! range, default, group, label and help text.
//!
//! `GET /api/schema` serves this, and both the web UI and the mod render
//! their settings forms from it, so adding a setting means adding it to
//! [`Settings`] and describing it here — not editing every client. Defaults
//! are read from [`Settings::default`] rather than restated, and a test fails
//! when a settings field has no description or a description no field.

use serde::Serialize;
use serde_json::{json, Value};

use crate::formats::Format;
use crate::settings::{limits, FloatRange, Settings};
use crate::targets::TARGETS;

/// Settings sections, in the order both UIs present them.
#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub key: &'static str,
    pub label: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<&'static str>,
}

/// How a field's value is typed and edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Whole number; `min`/`max`/`step` apply.
    Int,
    /// Number; `min`/`max`/`step` apply.
    Float,
    Bool,
    Text,
    /// A block ID such as `minecraft:stone`; `choices` are suggestions.
    Block,
    /// A unit vector `[x, y, z]` in model space (Y up). UIs usually show it as
    /// azimuth and elevation.
    Direction,
    /// One of `choices`.
    Choice,
    /// An absolute folder on the machine running the server.
    Folder,
}

/// What a field affects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Changes the blocks the conversion produces.
    Conversion,
    /// Changes only how the job runs or where its file goes.
    Job,
}

#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    pub value: Value,
    pub label: &'static str,
}

/// A field is shown or enabled only while another field has this value.
#[derive(Debug, Clone, Serialize)]
pub struct Condition {
    pub field: &'static str,
    pub equals: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub key: &'static str,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub group: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub default: Value,
    pub scope: Scope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    /// Range a slider should span when it is narrower than `min`..`max`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slider: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<Choice>,
    /// `null` is a valid value (for example "derive the voxel size").
    #[serde(skip_serializing_if = "is_false")]
    pub nullable: bool,
    /// Tucked away by default in both UIs.
    #[serde(skip_serializing_if = "is_false")]
    pub advanced: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<Condition>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize)]
pub struct Schema {
    pub groups: Vec<Group>,
    pub fields: Vec<Field>,
}

impl Schema {
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.key == key)
    }

    /// Keys of the fields that belong to `scope`.
    pub fn keys(&self, scope: Scope) -> impl Iterator<Item = &'static str> + '_ {
        self.fields
            .iter()
            .filter(move |f| f.scope == scope)
            .map(|f| f.key)
    }
}

/// A field with everything optional left empty; the builders below fill in
/// what each kind needs.
fn field(
    key: &'static str,
    kind: Kind,
    group: &'static str,
    label: &'static str,
    help: &'static str,
) -> Field {
    let default = serde_json::to_value(Settings::default())
        .ok()
        .and_then(|v| v.get(key).cloned())
        .unwrap_or(Value::Null);
    Field {
        key,
        kind,
        group,
        label,
        help,
        default,
        scope: Scope::Conversion,
        min: None,
        max: None,
        step: None,
        slider: None,
        unit: None,
        placeholder: None,
        choices: Vec::new(),
        nullable: false,
        advanced: false,
        when: None,
    }
}

impl Field {
    fn range(mut self, range: FloatRange, step: f64) -> Self {
        self.min = Some(range.min as f64);
        self.max = Some(range.max as f64);
        self.step = Some(step);
        self
    }
    fn slider(mut self, lo: f64, hi: f64) -> Self {
        self.slider = Some([lo, hi]);
        self
    }
    fn unit(mut self, unit: &'static str) -> Self {
        self.unit = Some(unit);
        self
    }
    fn advanced(mut self) -> Self {
        self.advanced = true;
        self
    }
    fn when(mut self, field: &'static str, equals: Value) -> Self {
        self.when = Some(Condition { field, equals });
        self
    }
    fn job(mut self, default: Value) -> Self {
        self.scope = Scope::Job;
        self.default = default;
        self
    }
}

/// Sections in display order.
pub fn groups() -> Vec<Group> {
    vec![
        Group {
            key: "size",
            label: "Size & shape",
            help: None,
        },
        Group {
            key: "color",
            label: "Color",
            help: None,
        },
        Group {
            key: "lighting",
            label: "Lighting",
            help: Some(
                "Separates lighting baked into the model's textures from its real colors. \
                 Leave it alone unless the model looks shiny or shaded in its texture.",
            ),
        },
        Group {
            key: "target",
            label: "Target version",
            help: Some(
                "The Minecraft version the schematic is for. Only blocks that exist in \
                 that version are used.",
            ),
        },
        Group {
            key: "output",
            label: "Output",
            help: None,
        },
        Group {
            key: "advanced",
            label: "Advanced",
            help: None,
        },
    ]
}

/// The full schema: every [`Settings`] field plus the job-level fields the
/// server reads from the same settings object.
pub fn schema() -> Schema {
    use limits::*;
    let sampling = json!(true);

    let mut max_size = field(
        "max_size",
        Kind::Int,
        "size",
        "Max size",
        "Length of the longest side of the result, in blocks.",
    )
    .slider(8.0, 512.0)
    .unit("blocks");
    max_size.min = Some(MAX_SIZE.min as f64);
    max_size.max = Some(MAX_SIZE.max as f64);
    max_size.step = Some(1.0);

    let mut voxel_size = field(
        "voxel_size",
        Kind::Float,
        "size",
        "Voxel size",
        "Size of one block in the model's own units. Overrides max size when set.",
    )
    .advanced();
    voxel_size.nullable = true;
    voxel_size.min = Some(0.0);
    voxel_size.placeholder = Some("Auto");

    let mut default_block = field(
        "default_block",
        Kind::Block,
        "color",
        "Block",
        "The block used everywhere when color sampling is off.",
    )
    .when("color_sampling", json!(false));
    default_block.choices = vec![
        Choice {
            value: json!("minecraft:white_concrete"),
            label: "White concrete",
        },
        Choice {
            value: json!("minecraft:netherrack"),
            label: "Netherrack",
        },
    ];

    let mut target = field(
        "target",
        Kind::Choice,
        "target",
        "Minecraft version",
        "The game version the schematic is for: it is stamped into the file, and only \
         blocks that exist in that version are used.",
    );
    target.choices = TARGETS
        .iter()
        .rev()
        .map(|t| Choice {
            value: json!(t.id),
            label: t.id,
        })
        .collect();

    let mut format = field(
        "format",
        Kind::Choice,
        "output",
        "File format",
        "Litematica's .litematic; .schem for WorldEdit and FAWE (v2 reads everywhere, v3 needs \
         WorldEdit 7.3+); .nbt for structure blocks and /place template.",
    );
    format.choices = Format::ALL
        .iter()
        .map(|f| Choice {
            value: json!(f.id()),
            label: f.label(),
        })
        .collect();

    let mut schematic_name = field(
        "schematic_name",
        Kind::Text,
        "output",
        "Schematic name",
        "Name stored in the schematic and used for its file. Empty uses the model's file name.",
    );
    schematic_name.placeholder = Some("Model file name");

    let mut output_dir = field(
        "output_dir",
        Kind::Folder,
        "output",
        "Save into folder",
        "Also copy each finished schematic into this folder, typically a schematics folder. \
         ~ and %APPDATA% expand; the folder is created if missing.",
    )
    .job(Value::Null);
    output_dir.nullable = true;
    output_dir.placeholder = Some("Only keep it on the server");

    let mut threads = field(
        "threads",
        Kind::Int,
        "advanced",
        "Parallel conversions",
        "How many files of one batch convert at the same time.",
    )
    .job(json!(4))
    .advanced();
    threads.min = Some(1.0);
    threads.max = Some(64.0);
    threads.step = Some(1.0);

    let fields = vec![
        max_size,
        voxel_size,
        field(
            "color_sampling",
            Kind::Bool,
            "color",
            "Color sampling",
            "Match every block to the model's surface color. Off places one block everywhere.",
        ),
        field(
            "dither",
            Kind::Bool,
            "color",
            "Dithering",
            "8×8 ordered dithering, so large flat areas do not band into a single block.",
        )
        .when("color_sampling", sampling.clone()),
        field(
            "brightness",
            Kind::Float,
            "color",
            "Brightness",
            "Added to every color before matching.",
        )
        .range(BRIGHTNESS, 0.01)
        .slider(-0.5, 0.5)
        .when("color_sampling", sampling.clone()),
        field(
            "contrast",
            Kind::Float,
            "color",
            "Contrast",
            "Spreads colors away from mid-grey before matching.",
        )
        .range(CONTRAST, 0.01)
        .slider(0.5, 2.0)
        .when("color_sampling", sampling.clone()),
        field(
            "saturation",
            Kind::Float,
            "color",
            "Saturation",
            "0 matches in greyscale; above 1 exaggerates color.",
        )
        .range(SATURATION, 0.01)
        .slider(0.0, 2.0)
        .when("color_sampling", sampling.clone()),
        default_block,
        field(
            "light_dir",
            Kind::Direction,
            "lighting",
            "Key light",
            "Where the light baked into the texture came from, in model space.",
        )
        .when("color_sampling", sampling.clone()),
        field(
            "delight",
            Kind::Float,
            "lighting",
            "De-light",
            "Removes the assumed lighting from the texture: the highlight is subtracted, \
             the shading divided out. 0 is off.",
        )
        .range(UNIT, 0.01)
        .when("color_sampling", sampling.clone()),
        field(
            "light_gloss",
            Kind::Float,
            "lighting",
            "Assumed gloss",
            "How sharp a highlight to assume was baked in. 0 assumes none.",
        )
        .range(UNIT, 0.01)
        .when("color_sampling", sampling.clone()),
        field(
            "light_ambient",
            Kind::Float,
            "lighting",
            "Ambient",
            "Light still reaching surfaces that face away from the key light.",
        )
        .range(UNIT, 0.01)
        .when("color_sampling", sampling.clone()),
        field(
            "specular",
            Kind::Float,
            "lighting",
            "Specular gain",
            "Strength of the assumed highlight.",
        )
        .range(SPECULAR, 0.01)
        .slider(0.0, 2.0)
        .when("color_sampling", sampling.clone()),
        field(
            "highlight_rejection",
            Kind::Float,
            "lighting",
            "Highlight rejection",
            "Discounts samples inside the highlight when averaging a block, so a shiny \
             surface keeps its own color.",
        )
        .range(UNIT, 0.01)
        .when("color_sampling", sampling.clone()),
        field(
            "highlight_recovery",
            Kind::Float,
            "lighting",
            "Blown-voxel recovery",
            "Rebuilds blocks that were entirely highlight from the rest of their material.",
        )
        .range(UNIT, 0.01)
        .when("color_sampling", sampling),
        target,
        format,
        schematic_name,
        output_dir,
        threads,
        field(
            "ram_limit",
            Kind::Float,
            "advanced",
            "Memory budget",
            "Upper bound on the memory color sampling may use.",
        )
        .range(RAM_LIMIT, 0.5)
        .slider(0.5, 32.0)
        .unit("GB")
        .advanced(),
    ];

    Schema {
        groups: groups(),
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_setting_is_described_and_nothing_else() {
        let settings: BTreeSet<String> = match serde_json::to_value(Settings::default()) {
            Ok(Value::Object(map)) => map.keys().cloned().collect(),
            other => panic!("settings must serialize to an object, got {other:?}"),
        };
        let s = schema();
        let described: BTreeSet<String> = s.keys(Scope::Conversion).map(str::to_string).collect();
        assert_eq!(settings, described);
    }

    #[test]
    fn defaults_come_from_settings() {
        let s = schema();
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        for f in s.fields.iter().filter(|f| f.scope == Scope::Conversion) {
            assert_eq!(f.default, defaults[f.key], "{}", f.key);
        }
        assert_eq!(s.field("max_size").unwrap().default, json!(128));
        assert_eq!(s.field("voxel_size").unwrap().default, Value::Null);
    }

    #[test]
    fn fields_are_well_formed() {
        let s = schema();
        let group_keys: BTreeSet<&str> = s.groups.iter().map(|g| g.key).collect();
        let mut seen = BTreeSet::new();
        for f in &s.fields {
            assert!(seen.insert(f.key), "duplicate field {}", f.key);
            assert!(group_keys.contains(f.group), "{} has unknown group", f.key);
            assert!(!f.label.is_empty() && !f.help.is_empty(), "{}", f.key);
            if let (Some(lo), Some(hi)) = (f.min, f.max) {
                assert!(lo <= hi, "{}", f.key);
                if let Some([a, b]) = f.slider {
                    assert!(
                        lo <= a && a < b && b <= hi,
                        "{} slider outside range",
                        f.key
                    );
                }
            }
            if let Some(c) = &f.when {
                assert!(
                    s.field(c.field).is_some(),
                    "{} depends on unknown field",
                    f.key
                );
            }
        }
    }

    #[test]
    fn default_values_sit_inside_their_ranges() {
        for f in schema().fields {
            if let (Some(v), Some(lo), Some(hi)) = (f.default.as_f64(), f.min, f.max) {
                assert!(
                    (lo..=hi).contains(&v),
                    "{} default {v} outside {lo}..{hi}",
                    f.key
                );
            }
        }
    }
}
