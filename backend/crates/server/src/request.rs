//! Turning a conversion request into settings and jobs.
//!
//! API v2 sends one `settings` JSON object whose fields are all optional and
//! land on the server's defaults. API v1 sends every setting as its own text
//! field; its parsing is kept exactly as lenient as it always was, so existing
//! clients see no difference.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value};

use schemgen_core::schema::{schema, Scope};
use schemgen_core::Settings;

use crate::error::{ApiError, ApiResult};
use crate::jobs::{Job, NewJob};
use crate::multipart::{Form, Upload};
use crate::savedir;
use crate::state::AppState;

/// Parallel conversions per request when the request does not say.
pub const DEFAULT_THREADS: usize = 4;

/// Everything a request asks for besides the model files.
pub struct Request {
    pub settings: Settings,
    /// Folder to copy finished schematics into, already resolved and created.
    pub deliver_to: Option<PathBuf>,
    /// At most this many of the request's files convert at once.
    pub threads: usize,
    /// Settings keys the server did not recognize.
    pub ignored: Vec<String>,
}

/// v2: the `settings` JSON part, overlaid on `defaults`.
pub fn from_json(defaults: &Settings, raw: Option<&str>) -> ApiResult<Request> {
    let mut patch = match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Map::new(),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => map,
            Ok(_) => return Err(ApiError::bad_request("settings must be a JSON object")),
            Err(e) => {
                return Err(ApiError::bad_request(format!(
                    "settings is not valid JSON: {e}"
                )))
            }
        },
    };

    let output_dir = match patch.remove("output_dir") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(s),
        Some(_) => {
            return Err(ApiError::bad_request(
                "settings.output_dir must be a string",
            ))
        }
    };
    let threads = match patch.remove("threads") {
        None | Some(Value::Null) => DEFAULT_THREADS,
        Some(v) => v
            .as_u64()
            .map(|n| n.clamp(1, 64) as usize)
            .ok_or_else(|| ApiError::bad_request("settings.threads must be a positive integer"))?,
    };

    let known: HashSet<&str> = schema().keys(Scope::Conversion).collect();
    let mut ignored: Vec<String> = patch
        .keys()
        .filter(|k| !known.contains(k.as_str()))
        .cloned()
        .collect();
    ignored.sort();
    patch.retain(|k, _| known.contains(k.as_str()));

    let base = match serde_json::to_value(defaults) {
        Ok(Value::Object(map)) => map,
        _ => unreachable!("settings serialize to an object"),
    };
    // Checked one field at a time so an error can name the field.
    for (key, value) in &patch {
        let mut single = base.clone();
        single.insert(key.clone(), value.clone());
        if let Err(e) = serde_json::from_value::<Settings>(Value::Object(single)) {
            return Err(ApiError::bad_request(format!("settings.{key}: {e}")));
        }
    }
    let mut merged = base;
    merged.extend(patch);
    let settings = serde_json::from_value::<Settings>(Value::Object(merged))
        .map_err(|e| ApiError::bad_request(format!("settings: {e}")))?
        .normalized()?;

    Ok(Request {
        settings,
        deliver_to: prepare_folder(output_dir.as_deref())?,
        threads,
        ignored,
    })
}

/// v1: one text field per setting, every one optional and forgiving — a
/// value that does not parse falls back to its default instead of failing.
pub fn from_v1_fields(defaults: &Settings, form: &Form) -> ApiResult<Request> {
    let d = defaults;
    let num = |key: &str, default: f32| -> f32 {
        form.text(key)
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    };
    let flag = |key: &str, default: bool| match form.text(key) {
        None => default,
        Some(v) => v.eq_ignore_ascii_case("true"),
    };

    let light_dir = form
        .text("light_dir")
        .and_then(|raw| {
            let parts: Vec<f32> = raw
                .split(',')
                .filter_map(|p| p.trim().parse().ok())
                .collect();
            match parts[..] {
                [x, y, z] if x.is_finite() && y.is_finite() && z.is_finite() => Some([x, y, z]),
                _ => None,
            }
        })
        .filter(|[x, y, z]| x * x + y * y + z * z > 1e-12)
        .unwrap_or(d.light_dir);

    let settings = Settings {
        max_size: form
            .text("max_size")
            .and_then(|v| v.parse().ok())
            .unwrap_or(d.max_size),
        voxel_size: form
            .text("voxel_size")
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v > 0.0),
        ram_limit: num("ram_limit", d.ram_limit),
        dither: flag("dither", d.dither),
        color_sampling: flag("color_sampling", d.color_sampling),
        brightness: num("brightness", d.brightness),
        contrast: num("contrast", d.contrast),
        saturation: num("saturation", d.saturation),
        default_block: match form.text("no_color_block") {
            Some("netherrack") => "minecraft:netherrack".to_string(),
            _ => schemgen_core::settings::DEFAULT_BLOCK.to_string(),
        },
        light_dir,
        light_ambient: num("light_ambient", d.light_ambient),
        light_gloss: num("light_gloss", d.light_gloss),
        specular: num("specular", d.specular),
        highlight_rejection: num("highlight_rejection", d.highlight_rejection),
        highlight_recovery: num("highlight_recovery", d.highlight_recovery),
        delight: num("delight", d.delight),
        target: d.target.clone(),
        format: d.format.clone(),
        schematic_name: form.text("schematic_name").unwrap_or_default().to_string(),
    }
    .normalized()?;

    // Auto-save is on when a folder was given and the toggle is not
    // explicitly off.
    let folder = form.text("output_dir").filter(|_| {
        form.text("auto_save")
            .is_none_or(|v| v.eq_ignore_ascii_case("true"))
    });

    Ok(Request {
        settings,
        deliver_to: prepare_folder(folder)?,
        threads: form
            .text("threads")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 64),
        ignored: Vec::new(),
    })
}

/// Resolve, create and probe an output folder up front — failing now beats
/// converting for minutes and only then finding the folder unusable.
fn prepare_folder(raw: Option<&str>) -> ApiResult<Option<PathBuf>> {
    match raw {
        None => Ok(None),
        Some(raw) => savedir::prepare(raw)
            .map(Some)
            .map_err(ApiError::bad_request),
    }
}

/// One job per uploaded model. With a single model the schematic takes the
/// requested name; in a batch every model keeps its own file name, and
/// same-named models become `name`, `name-2`, …
pub fn create_jobs(app: &AppState, files: Vec<Upload>, request: &Request) -> Vec<Arc<Job>> {
    let single = files.len() == 1;
    let mut taken = HashSet::new();
    files
        .into_iter()
        .map(|upload| {
            let stem = Path::new(&upload.file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("model")
                .to_string();
            let name = if single {
                request.settings.name_or(&stem).to_string()
            } else {
                stem
            };
            let extension = request.settings.format().extension();
            let file_name =
                savedir::dedupe_filename(&savedir::sanitize_filename(&name, extension), &mut taken);
            let settings = Settings {
                schematic_name: name.clone(),
                ..request.settings.clone()
            };
            let input_name = upload.file_name.clone();
            Job::new(
                &app.outputs,
                NewJob {
                    input_name,
                    upload: upload.keep(),
                    settings,
                    name,
                    file_name,
                    deliver_to: request.deliver_to.clone(),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(fields: &[(&str, &str)]) -> Form {
        Form {
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Form::default()
        }
    }

    #[test]
    fn empty_json_is_the_defaults() {
        let d = Settings::default();
        let r = from_json(&d, None).unwrap();
        assert_eq!(r.settings, d);
        assert_eq!(r.threads, DEFAULT_THREADS);
        assert!(r.deliver_to.is_none());
        assert_eq!(from_json(&d, Some("{}")).unwrap().settings, d);
    }

    #[test]
    fn json_overlays_the_server_defaults() {
        let d = Settings {
            target: "1.20.4".into(),
            ..Settings::default()
        };
        let r = from_json(
            &d,
            Some(r#"{"max_size": 64, "dither": false, "threads": 2}"#),
        )
        .unwrap();
        assert_eq!(r.settings.max_size, 64);
        assert!(!r.settings.dither);
        assert_eq!(
            r.settings.target, "1.20.4",
            "unspecified fields keep the server default"
        );
        assert_eq!(r.threads, 2);
    }

    #[test]
    fn json_errors_name_the_field() {
        let d = Settings::default();
        let e = from_json(&d, Some(r#"{"max_size": "big"}"#)).err().unwrap();
        assert!(e.message.contains("max_size"), "{}", e.message);
        let e = from_json(&d, Some(r#"{"target": "1.7.10"}"#))
            .err()
            .unwrap();
        assert!(e.message.contains("target"), "{}", e.message);
        assert!(from_json(&d, Some("[1, 2]")).is_err());
        assert!(from_json(&d, Some("{not json")).is_err());
    }

    #[test]
    fn unknown_json_keys_are_reported_not_fatal() {
        let r = from_json(&Settings::default(), Some(r#"{"max_sise": 1, "zz": true}"#)).unwrap();
        assert_eq!(r.ignored, ["max_sise", "zz"]);
    }

    #[test]
    fn v1_fields_parse_leniently() {
        let d = Settings::default();
        let r = from_v1_fields(
            &d,
            &form(&[
                ("max_size", "96"),
                ("voxel_size", ""),
                ("dither", "false"),
                ("brightness", "not a number"),
                ("saturation", "9"),
                ("no_color_block", "netherrack"),
                ("light_dir", "0,0,0"),
                ("threads", "3"),
            ]),
        )
        .unwrap();
        assert_eq!(r.settings.max_size, 96);
        assert_eq!(r.settings.voxel_size, None);
        assert!(!r.settings.dither);
        assert_eq!(r.settings.brightness, d.brightness);
        assert_eq!(r.settings.saturation, 3.0);
        assert_eq!(r.settings.default_block, "minecraft:netherrack");
        assert_eq!(
            r.settings.light_dir, d.light_dir,
            "a zero direction falls back"
        );
        assert_eq!(r.threads, 3);
    }

    #[test]
    fn v1_auto_save_needs_a_folder_and_no_explicit_off() {
        let d = Settings::default();
        let dir = std::env::temp_dir().join("schemgen_v1_autosave_test");
        let dir_s = dir.display().to_string();
        let on = from_v1_fields(&d, &form(&[("output_dir", &dir_s)])).unwrap();
        assert_eq!(on.deliver_to.as_deref(), Some(dir.as_path()));
        let off =
            from_v1_fields(&d, &form(&[("output_dir", &dir_s), ("auto_save", "false")])).unwrap();
        assert!(off.deliver_to.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
