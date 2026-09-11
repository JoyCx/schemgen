//! HTTP API layer — Actix-web routes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use actix_web::{web, HttpResponse, get, post};
use actix_multipart::form::{MultipartForm, tempfile::TempFile, text::Text};
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;

use crate::palette::Palette;
use crate::savedir;
use crate::types::{ConversionJob, ConversionOptions, ConversionResult, JobStatus, LightingOptions, ProgressEvent};

/// Global payload / multipart total limit. Raised well above the single-file
/// 300 MB so batch uploads of many GLBs fit in one request.
const MAX_PAYLOAD_BYTES: usize = 4usize * 1024 * 1024 * 1024; // 4 GiB

#[derive(Clone)]
pub struct AppState {
    pub jobs: Arc<Mutex<HashMap<String, ConversionJob>>>,
    pub palette: Palette,
    /// Unique block IDs behind `palette` — several palette entries can share one.
    pub block_count: usize,
    pub upload_dir: std::path::PathBuf,
    pub output_dir: std::path::PathBuf,
}

// ---- Upload forms -----------------------------------------------------------

#[derive(Debug, MultipartForm)]
pub struct ConvertForm {
    #[multipart(limit = "256MB")]
    pub file: TempFile,
    pub max_size: Text<String>,
    pub voxel_size: Text<String>,
    pub ram_limit: Text<String>,
    pub dither: Text<String>,
    pub color_sampling: Text<String>,
    pub brightness: Text<String>,
    pub contrast: Text<String>,
    pub saturation: Text<String>,
    pub no_color_block: Text<String>,
    pub schematic_name: Text<String>,
    pub output_dir: Option<Text<String>>,
    pub auto_save: Option<Text<String>>,
    pub light_dir: Option<Text<String>>,
    pub light_ambient: Option<Text<String>>,
    pub light_gloss: Option<Text<String>>,
    pub specular: Option<Text<String>>,
    pub highlight_rejection: Option<Text<String>>,
    pub highlight_recovery: Option<Text<String>>,
    pub delight: Option<Text<String>>,
}

/// Batch form: many files (multipart field name `files`), shared options, plus
/// `threads` (max concurrent conversions). No per-field limit is set on `files`
/// so each file is only bounded by the global payload / total limit.
#[derive(Debug, MultipartForm)]
pub struct BatchConvertForm {
    pub files: Vec<TempFile>,
    pub max_size: Text<String>,
    pub voxel_size: Text<String>,
    pub ram_limit: Text<String>,
    pub threads: Text<String>,
    pub dither: Text<String>,
    pub color_sampling: Text<String>,
    pub brightness: Text<String>,
    pub contrast: Text<String>,
    pub saturation: Text<String>,
    pub no_color_block: Text<String>,
    pub output_dir: Option<Text<String>>,
    pub auto_save: Option<Text<String>>,
    pub light_dir: Option<Text<String>>,
    pub light_ambient: Option<Text<String>>,
    pub light_gloss: Option<Text<String>>,
    pub specular: Option<Text<String>>,
    pub highlight_rejection: Option<Text<String>>,
    pub highlight_recovery: Option<Text<String>>,
    pub delight: Option<Text<String>>,
}

/// Where a finished schematic should be copied once the conversion succeeds.
#[derive(Debug, Clone)]
struct SaveTarget {
    dir: PathBuf,
    filename: String,
}

// ---- Field parsing helpers --------------------------------------------------

fn is_true(s: &str) -> bool { s.to_lowercase() == "true" }

fn text_or(s: &Text<String>, default: &str) -> String {
    if s.as_str().trim().is_empty() { default.to_string() } else { s.as_str().trim().to_string() }
}

fn text_bool(s: &Text<String>, default: bool) -> bool {
    if s.as_str().trim().is_empty() { default } else { is_true(s.as_str()) }
}

fn text_f32(s: &Text<String>, default: f32) -> f32 {
    s.as_str().trim().parse().unwrap_or(default)
}

fn text_u32(s: &Text<String>, default: u32) -> u32 {
    s.as_str().trim().parse().unwrap_or(default)
}

fn text_opt_f32(s: &Text<String>) -> Option<f32> {
    let v = s.as_str().trim();
    if v.is_empty() { None } else { v.parse().ok() }
}

fn opt_f32(field: &Option<Text<String>>, default: f32) -> f32 {
    field.as_ref()
        .and_then(|t| t.as_str().trim().parse().ok())
        .unwrap_or(default)
}

/// Parse an "x,y,z" light direction. A malformed value is not worth a 400 —
/// the sampler's own default direction is a perfectly good answer.
fn parse_direction(field: &Option<Text<String>>) -> Option<[f32; 3]> {
    let raw = field.as_ref()?;
    let parts: Vec<f32> = raw.as_str().split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    match parts[..] {
        [x, y, z] => Some([x, y, z]),
        _ => None,
    }
}

/// The lighting fields, carried identically by both upload forms.
///
/// They are all optional: a client that predates them — or the batch form
/// posting a subset — falls through to `LightingOptions::default()` rather
/// than failing the upload.
trait LightingFields {
    fn lighting(&self) -> LightingOptions;
}

macro_rules! impl_lighting_fields {
    ($form:ty) => {
        impl LightingFields for $form {
            fn lighting(&self) -> LightingOptions {
                let d = LightingOptions::default();
                LightingOptions {
                    light_dir: parse_direction(&self.light_dir).unwrap_or(d.light_dir),
                    ambient: opt_f32(&self.light_ambient, d.ambient).clamp(0.0, 1.0),
                    gloss: opt_f32(&self.light_gloss, d.gloss).clamp(0.0, 1.0),
                    specular: opt_f32(&self.specular, d.specular).clamp(0.0, 4.0),
                    rejection: opt_f32(&self.highlight_rejection, d.rejection).clamp(0.0, 1.0),
                    recovery: opt_f32(&self.highlight_recovery, d.recovery).clamp(0.0, 1.0),
                    delight: opt_f32(&self.delight, d.delight).clamp(0.0, 1.0),
                }
            }
        }
    };
}

impl_lighting_fields!(ConvertForm);
impl_lighting_fields!(BatchConvertForm);

fn opt_text(field: &Option<Text<String>>) -> String {
    field.as_ref().map(|t| t.as_str().trim().to_string()).unwrap_or_default()
}

/// Auto-save is on when a folder was given and the toggle is not explicitly off.
fn wants_auto_save(auto_save: &Option<Text<String>>, raw_dir: &str) -> bool {
    if raw_dir.is_empty() { return false; }
    match auto_save {
        Some(t) if !t.as_str().trim().is_empty() => is_true(t.as_str()),
        _ => true,
    }
}

fn is_glb_or_gltf(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".glb") || lower.ends_with(".gltf")
}

fn default_block_name(no_color_block: &str) -> String {
    match no_color_block {
        "netherrack" => "minecraft:netherrack".to_string(),
        _ => "minecraft:white_concrete".to_string(),
    }
}

/// Build shared conversion options from the common multipart text fields.
fn options_from_fields(
    max_size: &Text<String>,
    voxel_size: &Text<String>,
    ram_limit: &Text<String>,
    dither: &Text<String>,
    color_sampling: &Text<String>,
    brightness: &Text<String>,
    contrast: &Text<String>,
    saturation: &Text<String>,
    no_color_block: &Text<String>,
    lighting: LightingOptions,
) -> ConversionOptions {
    ConversionOptions {
        max_size: text_u32(max_size, 128),
        voxel_size: text_opt_f32(voxel_size),
        ram_limit: text_f32(ram_limit, 4.0).max(0.5),
        use_dithering: text_bool(dither, true),
        use_color_sampling: text_bool(color_sampling, true),
        brightness: text_f32(brightness, 0.0).clamp(-1.0, 1.0),
        contrast: text_f32(contrast, 1.0).clamp(0.0, 3.0),
        saturation: text_f32(saturation, 1.0).clamp(0.0, 3.0),
        default_block_name: default_block_name(text_or(no_color_block, "white").as_str()),
        schematic_name: String::new(),
        lighting,
    }
}

fn options_for_preview(form: &ConvertForm, filename: &str) -> ConversionOptions {
    ConversionOptions {
        max_size: text_u32(&form.max_size, 128),
        voxel_size: text_opt_f32(&form.voxel_size),
        ram_limit: text_f32(&form.ram_limit, 4.0).max(0.5),
        use_dithering: text_bool(&form.dither, true),
        use_color_sampling: text_bool(&form.color_sampling, true),
        brightness: text_f32(&form.brightness, 0.0).clamp(-1.0, 1.0),
        contrast: text_f32(&form.contrast, 1.0).clamp(0.0, 3.0),
        saturation: text_f32(&form.saturation, 1.0).clamp(0.0, 3.0),
        default_block_name: "minecraft:white_concrete".to_string(),
        schematic_name: Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or("preview").to_string(),
        lighting: form.lighting(),
    }
}

// ---- Job lifecycle helpers --------------------------------------------------

/// Record a finished conversion in shared state, copying the schematic into the
/// user-chosen folder first (done before taking the lock — the copy is I/O).
async fn finish_job(
    jobs: &Arc<Mutex<HashMap<String, ConversionJob>>>,
    job_id: &str,
    result: Result<ConversionResult, String>,
    save: Option<SaveTarget>,
    output_path: &Path,
) {
    // Deliver to the chosen folder before reporting Done, so a UI that reacts to
    // "done" always sees the file already in place.
    let delivered = match (&result, &save) {
        (Ok(_), Some(target)) => Some(savedir::deliver(output_path, &target.dir, &target.filename)),
        _ => None,
    };

    let mut map = jobs.lock().await;
    if let Some(job) = map.get_mut(job_id) {
        match result {
            Ok(res) => {
                job.status = JobStatus::Done;
                job.progress = 100.0;
                job.saved_path = None;
                job.save_error = None;
                let mut message = format!(
                    "Done! {} blocks, {} unique, {:.1}s",
                    res.voxel_count, res.unique_blocks, res.elapsed
                );
                match delivered {
                    Some(Ok(dest)) => {
                        message.push_str(&format!(" → saved to {}", dest.display()));
                        job.saved_path = Some(dest.display().to_string());
                    }
                    Some(Err(e)) => {
                        log::error!("Job {job_id}: save to output folder failed: {e}");
                        message.push_str(&format!(" — but saving to your folder failed: {e}"));
                        job.save_error = Some(e);
                    }
                    None => {}
                }
                job.message = message;
            }
            Err(e) => {
                job.status = JobStatus::Error;
                job.message = e;
            }
        }
    }
}

/// Mirror pipeline events into the shared job, so `/api/progress` reports the
/// real stage instead of sitting at "Starting..." until the job is finished.
/// The web UI's progress panel polls that route.
///
/// The percentage stops at 99 while the job runs: `finish_job` owns the step to
/// 100, and only takes it once the file is written and delivered. Events that
/// arrive after that — the pipeline's own "Done!" — find the job no longer
/// `Running` and end the loop instead of overwriting the final message.
fn stream_progress_into_job(
    jobs: Arc<Mutex<HashMap<String, ConversionJob>>>,
    job_id: String,
) -> tokio::sync::mpsc::UnboundedSender<ProgressEvent> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ProgressEvent>();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let mut map = jobs.lock().await;
            match map.get_mut(&job_id) {
                Some(job) if job.status == JobStatus::Running => {
                    job.progress = (event.pct * 100.0).clamp(0.0, 99.0);
                    job.message = event.msg;
                }
                _ => break,
            }
        }
    });
    tx
}

/// Run one conversion in a dedicated blocking thread (own runtime), then record
/// the outcome in shared state.
fn spawn_convert_job(
    jobs: Arc<Mutex<HashMap<String, ConversionJob>>>,
    palette: Arc<Palette>,
    job_id: String,
    input: String,
    output: String,
    options: ConversionOptions,
    save: Option<SaveTarget>,
) {
    let jid = job_id.clone();
    let progress = stream_progress_into_job(Arc::clone(&jobs), job_id);
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let result = crate::converter::convert(&input, &output, &options, &palette, progress).await;
            finish_job(&jobs, &jid, result, save, Path::new(&output)).await;
        });
    });
}

// ---- Handlers ---------------------------------------------------------------

#[post("/api/preview")]
async fn preview_handler(state: web::Data<Arc<AppState>>, MultipartForm(form): MultipartForm<ConvertForm>) -> HttpResponse {
    let fname = form.file.file_name.clone().unwrap_or_else(|| "preview.glb".to_string());
    if !is_glb_or_gltf(&fname) {
        return HttpResponse::BadRequest().json(serde_json::json!({"error": "Only .glb / .gltf files are supported"}));
    }
    let input = form.file.file.path().to_path_buf();
    // TempFile uses a .tmp path; voxelize.py needs a real GLB/GLTF extension.
    let preview_ext = if fname.to_lowercase().ends_with(".gltf") { "gltf" } else { "glb" };
    let preview_input = std::env::temp_dir().join(format!("schemgen_preview_{}.{}", Uuid::new_v4(), preview_ext));
    if let Err(e) = std::fs::copy(&input, &preview_input) {
        return HttpResponse::InternalServerError().json(serde_json::json!({"error": format!("Could not prepare preview input: {e}")}));
    }
    let output = std::env::temp_dir().join(format!("schemgen_preview_{}.litematic", Uuid::new_v4()));
    let options = options_for_preview(&form, &fname);
    let palette = state.palette.clone();
    let input_s = preview_input.to_string_lossy().to_string();
    let output_s = output.to_string_lossy().to_string();
    let result = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        rt.block_on(crate::converter::preview_litematic(&input_s, &output_s, &options, &palette))
    }).await;
    let preview = match result {
        Ok(Ok(preview)) => preview,
        Ok(Err(e)) => {
            let _ = std::fs::remove_file(&output);
            let _ = std::fs::remove_file(&preview_input);
            return HttpResponse::InternalServerError().json(serde_json::json!({"error": format!("Preview failed: {e}")}));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&output);
            let _ = std::fs::remove_file(&preview_input);
            return HttpResponse::InternalServerError().json(serde_json::json!({"error": format!("Preview task failed: {e}")}));
        }
    };
    let litematic_verified = output.exists();
    let _ = std::fs::remove_file(output);
    let _ = std::fs::remove_file(preview_input);
    HttpResponse::Ok().json(serde_json::json!({
        "litematic_verified": litematic_verified,
        "grid": preview.grid,
        "blocks": preview.blocks,
    }))
}

/// Move an uploaded temp file to `dest`.
///
/// `TempFile::persist` renames, and a rename fails with `EXDEV` when the
/// multipart temp directory and the upload directory are on different
/// filesystems. That is the normal case on Linux, where `/tmp` is usually a
/// tmpfs while the upload directory is not, so fall back to copying the bytes
/// rather than letting the host's temp layout decide whether uploads work.
fn persist_upload(upload: TempFile, dest: &Path) -> std::io::Result<()> {
    match upload.file.persist(dest) {
        Ok(_) => Ok(()),
        Err(e) => {
            std::fs::copy(e.file.path(), dest).map_err(|_| e.error)?;
            Ok(())
        }
    }
}

#[post("/api/convert")]
async fn convert_handler(
    state: web::Data<Arc<AppState>>,
    MultipartForm(mut form): MultipartForm<ConvertForm>,
) -> HttpResponse {
    let fname = form.file.file_name.clone().unwrap_or_else(|| "unknown".to_string());
    if !is_glb_or_gltf(&fname) {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "Only .glb / .gltf files are supported"}));
    }

    let mut options = options_from_fields(
        &form.max_size, &form.voxel_size, &form.ram_limit, &form.dither, &form.color_sampling,
        &form.brightness, &form.contrast, &form.saturation, &form.no_color_block,
        form.lighting(),
    );
    let base = Path::new(&fname).file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    options.schematic_name = text_or(&form.schematic_name, base);

    let job_id = Uuid::new_v4().to_string();
    let glb_path = state.upload_dir.join(format!("{job_id}.glb"));
    let out_path = state.output_dir.join(format!("{job_id}.litematic"));

    let mut download_name = options.schematic_name.clone();
    if !download_name.ends_with(".litematic") { download_name.push_str(".litematic"); }

    // Optional: write the finished schematic straight into a folder chosen in
    // the UI (e.g. .minecraft/schematics). Fail loudly here rather than convert
    // for minutes and only then discover the folder is unusable.
    let raw_dir = opt_text(&form.output_dir);
    let save = if wants_auto_save(&form.auto_save, &raw_dir) {
        match savedir::prepare(&raw_dir) {
            Ok(dir) => Some(SaveTarget { dir, filename: savedir::sanitize_filename(&download_name) }),
            Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
        }
    } else {
        None
    };

    if let Err(e) = persist_upload(form.file, &glb_path) {
        log::error!("Failed to save upload: {e}");
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": "Failed to save uploaded file"}));
    }

    state.jobs.lock().await.insert(job_id.clone(), ConversionJob {
        status: JobStatus::Running,
        progress: 0.0,
        message: "Starting...".to_string(),
        download_name: download_name.clone(),
        output_path: out_path.to_str().unwrap().to_string(),
        saved_path: None,
        save_error: None,
    });

    spawn_convert_job(
        Arc::clone(&state.jobs),
        Arc::new(state.palette.clone()),
        job_id.clone(),
        glb_path.display().to_string(),
        out_path.display().to_string(),
        options,
        save.clone(),
    );

    HttpResponse::Ok().json(serde_json::json!({
        "job_id": job_id,
        "output_dir": save.map(|t| t.dir.display().to_string()),
    }))
}

#[post("/api/convert-batch")]
async fn convert_batch_handler(
    state: web::Data<Arc<AppState>>,
    MultipartForm(form): MultipartForm<BatchConvertForm>,
) -> HttpResponse {
    let threads = text_u32(&form.threads, 1).clamp(1, 64);

    if form.files.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({"error": "No files uploaded"}));
    }

    let base_options = options_from_fields(
        &form.max_size, &form.voxel_size, &form.ram_limit, &form.dither, &form.color_sampling,
        &form.brightness, &form.contrast, &form.saturation, &form.no_color_block,
        form.lighting(),
    );

    // Same UI-chosen output folder for the whole batch.
    let raw_dir = opt_text(&form.output_dir);
    let save_dir = if wants_auto_save(&form.auto_save, &raw_dir) {
        match savedir::prepare(&raw_dir) {
            Ok(dir) => Some(dir),
            Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
        }
    } else {
        None
    };

    // Persist each file and register its job.
    let mut pending: Vec<(String, String, String, ConversionOptions, Option<SaveTarget>)> = Vec::new();
    let mut response_jobs: Vec<serde_json::Value> = Vec::new();
    // Two identically named models in one batch must not overwrite each other.
    let mut used_names: HashSet<String> = HashSet::new();

    for file in form.files {
        let fname = file.file_name.clone().unwrap_or_else(|| "model.glb".to_string());
        if !is_glb_or_gltf(&fname) {
            continue;
        }
        let job_id = Uuid::new_v4().to_string();
        let glb_path = state.upload_dir.join(format!("{job_id}.glb"));
        let out_path = state.output_dir.join(format!("{job_id}.litematic"));

        let stem = Path::new(&fname)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output")
            .to_string();

        let mut options = base_options.clone();
        options.schematic_name = stem.clone();

        let mut download_name = stem;
        if !download_name.ends_with(".litematic") { download_name.push_str(".litematic"); }

        let save = save_dir.as_ref().map(|dir| SaveTarget {
            dir: dir.clone(),
            filename: savedir::dedupe_filename(&savedir::sanitize_filename(&download_name), &mut used_names),
        });

        if let Err(e) = persist_upload(file, &glb_path) {
            log::error!("Failed to save upload {fname}: {e}");
            continue;
        }

        state.jobs.lock().await.insert(job_id.clone(), ConversionJob {
            status: JobStatus::Running,
            progress: 0.0,
            message: "Queued...".to_string(),
            download_name,
            output_path: out_path.to_str().unwrap().to_string(),
            saved_path: None,
            save_error: None,
        });

        pending.push((job_id.clone(), glb_path.display().to_string(), out_path.display().to_string(), options, save));
        response_jobs.push(serde_json::json!({ "job_id": job_id, "filename": fname }));
    }

    if pending.is_empty() {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "No valid .glb / .gltf files found"}));
    }

    let jobs_arc = Arc::clone(&state.jobs);
    let palette = Arc::new(state.palette.clone());

    // Coordinator: run conversions, `threads` at a time. Each conversion runs in
    // its own blocking thread (own runtime) so it can saturate a Python voxelize
    // subprocess and Rayon without blocking the async runtime.
    tokio::spawn(async move {
        let sem = Arc::new(Semaphore::new(threads as usize));
        let mut set = tokio::task::JoinSet::new();

        for (job_id, input, output, options, save) in pending {
            let sem = Arc::clone(&sem);
            let jobs = Arc::clone(&jobs_arc);
            let palette = Arc::clone(&palette);
            let jid = job_id.clone();
            let out_path = output.clone();

            set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore closed");
                let progress = stream_progress_into_job(Arc::clone(&jobs), jid.clone());
                let result = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async move {
                        crate::converter::convert(&input, &output, &options, &palette, progress).await
                    })
                }).await;

                let converted = match result {
                    Ok(r) => r,
                    Err(e) => Err(format!("Conversion task failed: {e}")),
                };
                finish_job(&jobs, &jid, converted, save, Path::new(&out_path)).await;
            });
        }

        while set.join_next().await.is_some() {}
    });

    HttpResponse::Ok().json(serde_json::json!({
        "jobs": response_jobs,
        "threads": threads,
        "output_dir": save_dir.map(|d| d.display().to_string()),
    }))
}

#[get("/api/palette")]
async fn palette_handler(state: web::Data<Arc<AppState>>) -> HttpResponse {
    HttpResponse::Ok().json(state.palette.to_palette_json())
}

#[get("/api/download/{job_id}")]
async fn download_handler(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let job_id = path.into_inner();
    let out_path = state.output_dir.join(format!("{job_id}.litematic"));
    if !out_path.exists() { return HttpResponse::NotFound().body("File not found"); }

    let dn = state.jobs.lock().await.get(&job_id)
        .map(|j| j.download_name.clone())
        .unwrap_or_else(|| "output.litematic".to_string());

    match tokio::fs::read(&out_path).await {
        Ok(data) => HttpResponse::Ok()
            .insert_header(("Content-Type", "application/octet-stream"))
            .insert_header(("Content-Disposition", format!("attachment; filename=\"{dn}\"")))
            .body(data),
        Err(e) => { log::error!("Download error: {e}"); HttpResponse::InternalServerError().body("Read error") }
    }
}

#[get("/api/progress/{job_id}")]
async fn progress_handler(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let job_id = path.into_inner();
    let jobs = state.jobs.lock().await;
    match jobs.get(&job_id) {
        Some(job) => {
            let status = match job.status { JobStatus::Done => "done", JobStatus::Running => "running", JobStatus::Error => "error" };
            HttpResponse::Ok().json(serde_json::json!({
                "status": status,
                "progress": job.progress,
                "message": job.message,
                "download_name": job.download_name,
                "saved_path": job.saved_path,
                "save_error": job.save_error,
            }))
        }
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "Unknown job"}))
    }
}

// ---- Output folder endpoints ------------------------------------------------

#[derive(serde::Deserialize)]
pub struct OutputDirRequest {
    pub path: String,
}

/// Validate a folder typed in the UI (without creating it) so the user gets
/// feedback before starting a long conversion. Always 200 — `ok` is the verdict.
#[post("/api/output-dir/check")]
async fn check_output_dir_handler(body: web::Json<OutputDirRequest>) -> HttpResponse {
    match savedir::check(&body.path) {
        Ok(c) => HttpResponse::Ok().json(serde_json::json!({
            "ok": true, "path": c.path.display().to_string(), "exists": c.exists
        })),
        Err(e) => HttpResponse::Ok().json(serde_json::json!({ "ok": false, "error": e })),
    }
}

/// Likely Litematica schematic folders on this machine, for one-click picking.
#[get("/api/output-dir/suggestions")]
async fn output_dir_suggestions_handler() -> HttpResponse {
    let items: Vec<serde_json::Value> = savedir::suggestions().into_iter()
        .map(|(path, exists)| serde_json::json!({
            "path": path.display().to_string(), "exists": exists
        }))
        .collect();
    HttpResponse::Ok().json(serde_json::json!({ "suggestions": items }))
}

/// Copy an already-converted schematic into a folder — used when the user picks
/// or changes the folder after the conversion has finished, so nothing is
/// re-converted just to move a file.
#[post("/api/save/{job_id}")]
async fn save_to_folder_handler(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    body: web::Json<OutputDirRequest>,
) -> HttpResponse {
    let job_id = path.into_inner();

    let (out_path, download_name) = {
        let jobs = state.jobs.lock().await;
        match jobs.get(&job_id) {
            Some(job) if job.status == JobStatus::Done =>
                (PathBuf::from(job.output_path.clone()), job.download_name.clone()),
            Some(_) => return HttpResponse::Conflict()
                .json(serde_json::json!({"error": "Conversion is not finished yet"})),
            None => return HttpResponse::NotFound()
                .json(serde_json::json!({"error": "Unknown job"})),
        }
    };

    let dir = match savedir::prepare(&body.path) {
        Ok(dir) => dir,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
    };

    match savedir::deliver(&out_path, &dir, &savedir::sanitize_filename(&download_name)) {
        Ok(dest) => {
            let dest_s = dest.display().to_string();
            if let Some(job) = state.jobs.lock().await.get_mut(&job_id) {
                job.saved_path = Some(dest_s.clone());
                job.save_error = None;
            }
            HttpResponse::Ok().json(serde_json::json!({"saved_path": dest_s}))
        }
        Err(e) => {
            if let Some(job) = state.jobs.lock().await.get_mut(&job_id) {
                job.save_error = Some(e.clone());
            }
            HttpResponse::InternalServerError().json(serde_json::json!({"error": e}))
        }
    }
}

/// Reveal a finished schematic in the OS file manager, selected. The server and
/// the browser are on the same machine, so this beats downloading a second copy
/// of a file that already exists locally.
#[post("/api/reveal/{job_id}")]
async fn reveal_handler(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let job_id = path.into_inner();

    // Prefer the copy in the user's folder; fall back to the server's own output.
    let target = {
        let jobs = state.jobs.lock().await;
        match jobs.get(&job_id) {
            Some(job) if job.status == JobStatus::Done => PathBuf::from(
                job.saved_path.clone().unwrap_or_else(|| job.output_path.clone())
            ),
            Some(_) => return HttpResponse::Conflict()
                .json(serde_json::json!({"error": "Conversion is not finished yet"})),
            None => return HttpResponse::NotFound()
                .json(serde_json::json!({"error": "Unknown job"})),
        }
    };

    match savedir::reveal(&target) {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({
            "revealed": target.display().to_string()
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({"error": e})),
    }
}

/// Open the chosen output folder itself — the batch counterpart to revealing a
/// single file.
#[post("/api/reveal-folder")]
async fn reveal_folder_handler(body: web::Json<OutputDirRequest>) -> HttpResponse {
    let dir = match savedir::resolve(&body.path) {
        Ok(dir) => dir,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
    };
    match savedir::reveal_dir(&dir) {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({
            "revealed": dir.display().to_string()
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({"error": e})),
    }
}

/// Cheap liveness probe. A client pings this before offering to convert, so a
/// stopped server is reported up front instead of surfacing as a failed upload
/// minutes later.
#[get("/api/health")]
async fn health_handler(state: web::Data<Arc<AppState>>) -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "name": "schemgen2",
        "version": env!("CARGO_PKG_VERSION"),
        "palette_entries": state.palette.len(),
        "palette_blocks": state.block_count,
        "data_version": crate::litematic::data_version(),
        "schematic_version": 6,
        "os": std::env::consts::OS,
    }))
}

/// What this machine calls its file manager, so the UI can label the button.
#[get("/api/system")]
async fn system_handler() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "os": std::env::consts::OS,
        "file_manager": savedir::file_manager_name(),
    }))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    // Raise actix-multipart's total limit so batch uploads of many GLBs fit.
    let mf_config = actix_multipart::form::MultipartFormConfig::default()
        .total_limit(MAX_PAYLOAD_BYTES);
    cfg.app_data(mf_config)
        .service(preview_handler)
        .service(convert_handler)
        .service(convert_batch_handler)
        .service(palette_handler)
        .service(download_handler)
        .service(progress_handler)
        .service(check_output_dir_handler)
        .service(output_dir_suggestions_handler)
        .service(save_to_folder_handler)
        .service(reveal_handler)
        .service(reveal_folder_handler)
        .service(health_handler)
        .service(system_handler);
}
