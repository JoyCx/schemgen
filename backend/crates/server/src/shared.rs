//! Routes that belong to neither API version: health, host information, the
//! palette and the output-folder helpers — plus the job actions both versions
//! expose under their own paths.

use std::path::PathBuf;
use std::sync::Arc;

use actix_files::NamedFile;
use actix_web::http::header::{
    Charset, ContentDisposition, DispositionParam, DispositionType, ExtendedValue,
};
use actix_web::{get, post, web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::error::{ApiError, ApiResult};
use crate::jobs::{Job, Status};
use crate::savedir;
use crate::state::AppState;

pub type App = web::Data<Arc<AppState>>;

/// Cheap liveness probe. A client pings this before offering to convert, so a
/// stopped server is reported up front instead of as a failed upload later.
#[get("/api/health")]
async fn health(app: App) -> ApiResult<HttpResponse> {
    let target = app.defaults.target();
    let current = app.palettes.for_target(&target)?;
    Ok(HttpResponse::Ok().json(json!({
        "status": "ok",
        "name": "schemgen2",
        "version": schemgen_core::VERSION,
        "api": 2,
        "palette_entries": current.len(),
        "palette_blocks": current.block_count(),
        "target": target.key(),
        "data_version": target.data_version,
        "schematic_version": target.schematic_version,
        "os": std::env::consts::OS,
        "auth": app.token.is_some(),
        "voxelizer": schemgen_core::voxelizer::backend().name(),
    })))
}

/// What this machine calls its file manager, so a UI can label the button.
#[get("/api/system")]
async fn system() -> HttpResponse {
    HttpResponse::Ok().json(json!({
        "os": std::env::consts::OS,
        "file_manager": savedir::file_manager_name(),
    }))
}

#[derive(Deserialize)]
struct PaletteQuery {
    target: Option<String>,
}

/// Every block a conversion may choose, `{short_id: [r, g, b]}` — for the
/// server's default target, or `?target=1.20.4`.
#[get("/api/palette")]
async fn palette(app: App, query: web::Query<PaletteQuery>) -> ApiResult<HttpResponse> {
    let target = match query.target.as_deref().filter(|t| !t.trim().is_empty()) {
        Some(raw) => schemgen_core::Target::parse(raw)?,
        None => app.defaults.target(),
    };
    Ok(HttpResponse::Ok().json(app.palettes.for_target(&target)?.to_palette_json()))
}

#[derive(Deserialize)]
pub struct FolderRequest {
    pub path: String,
}

/// Validate a folder typed in a UI without creating it. Always 200 — `ok` is
/// the verdict.
#[post("/api/output-dir/check")]
async fn check_output_dir(body: web::Json<FolderRequest>) -> HttpResponse {
    match savedir::check(&body.path) {
        Ok(c) => HttpResponse::Ok().json(json!({
            "ok": true, "path": c.path.display().to_string(), "exists": c.exists
        })),
        Err(e) => HttpResponse::Ok().json(json!({ "ok": false, "error": e })),
    }
}

/// Likely Litematica schematic folders on this machine. A folder inside a
/// launcher instance says which instance, which Minecraft version it runs and
/// the target that suits it, so a UI can offer to switch.
#[get("/api/output-dir/suggestions")]
async fn output_dir_suggestions() -> HttpResponse {
    let items: Vec<_> = web::block(savedir::suggestions)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|s| {
            let instance = s.instance.map(|i| {
                let target = i
                    .mc_version
                    .as_deref()
                    .and_then(schemgen_core::Target::for_game_version)
                    .map(|t| t.id);
                json!({
                    "name": i.name,
                    "launcher": i.launcher,
                    "mc_version": i.mc_version,
                    "target": target,
                })
            });
            json!({
                "path": s.path.display().to_string(),
                "exists": s.exists,
                "instance": instance,
            })
        })
        .collect();
    HttpResponse::Ok().json(json!({ "suggestions": items }))
}
/// Open a folder in the host's file manager.
#[post("/api/reveal-folder")]
async fn reveal_folder(body: web::Json<FolderRequest>) -> ApiResult<HttpResponse> {
    let dir = savedir::resolve(&body.path).map_err(ApiError::bad_request)?;
    savedir::reveal_dir(&dir).map_err(ApiError::internal)?;
    Ok(HttpResponse::Ok().json(json!({ "revealed": dir.display().to_string() })))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(health)
        .service(system)
        .service(palette)
        .service(check_output_dir)
        .service(output_dir_suggestions)
        .service(reveal_folder);
}

// ── Job actions shared by v1 and v2 routes ──────────────────────────────

pub fn find_job(app: &AppState, id: &str) -> ApiResult<Arc<Job>> {
    app.jobs.get(id).ok_or_else(ApiError::unknown_job)
}

fn finished(job: &Job) -> ApiResult<()> {
    match job.state().status {
        Status::Done => Ok(()),
        Status::Error | Status::Cancelled => {
            Err(ApiError::conflict("The conversion did not finish"))
        }
        Status::Queued | Status::Running => {
            Err(ApiError::conflict("Conversion is not finished yet"))
        }
    }
}

/// The finished schematic, as an attachment named after it.
pub async fn download(job: &Job) -> ApiResult<NamedFile> {
    finished(job)?;
    let file = NamedFile::open_async(&job.output)
        .await
        .map_err(|_| ApiError::not_found("File not found"))?;
    let name = job.file_name.clone();
    Ok(file.set_content_disposition(ContentDisposition {
        disposition: DispositionType::Attachment,
        parameters: vec![
            DispositionParam::Filename(name.clone()),
            DispositionParam::FilenameExt(ExtendedValue {
                charset: Charset::Ext("UTF-8".to_string()),
                language_tag: None,
                value: name.into_bytes(),
            }),
        ],
    }))
}

/// Copy a finished schematic into a folder — no re-conversion.
pub async fn save(job: Arc<Job>, raw_dir: &str) -> ApiResult<HttpResponse> {
    finished(&job)?;
    let dir = savedir::prepare(raw_dir).map_err(ApiError::bad_request)?;
    let (src, name) = (job.output.clone(), job.file_name.clone());
    let delivered = web::block(move || savedir::deliver(&src, &dir, &name))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    match delivered {
        Ok(dest) => {
            let dest = dest.display().to_string();
            job.update(|s| {
                s.saved_path = Some(dest.clone());
                s.save_error = None;
            });
            Ok(HttpResponse::Ok().json(json!({ "saved_path": dest })))
        }
        Err(e) => {
            job.update(|s| s.save_error = Some(e.clone()));
            Err(ApiError::internal(e))
        }
    }
}

/// Show the finished schematic in the host's file manager — the copy in the
/// user's folder when there is one.
pub fn reveal(job: &Job) -> ApiResult<HttpResponse> {
    finished(job)?;
    let state = job.state();
    let target = state
        .saved_path
        .map(PathBuf::from)
        .unwrap_or_else(|| job.output.clone());
    savedir::reveal(&target).map_err(ApiError::internal)?;
    Ok(HttpResponse::Ok().json(json!({ "revealed": target.display().to_string() })))
}
