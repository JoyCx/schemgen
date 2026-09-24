//! API v1, kept for one release for clients written against it.
//!
//! The routes, fields and response shapes are the old ones; underneath they
//! run on the same jobs as v2. New clients should use v2 — see `docs/api.md`.

use std::sync::Arc;

use actix_files::NamedFile;
use actix_multipart::Multipart;
use actix_web::{get, post, web, HttpResponse};
use serde_json::json;

use crate::error::{ApiError, ApiResult};
use crate::jobs::{self, Status};
use crate::multipart;
use crate::request;
use crate::shared::{self, App, FolderRequest};

/// One model, settings as separate text fields.
#[post("/api/convert")]
async fn convert(app: App, payload: Multipart) -> ApiResult<HttpResponse> {
    let form = multipart::collect(payload, &app.uploads, app.max_upload).await?;
    let request = request::from_v1_fields(&app.defaults, &form)?;
    let Some(upload) = form.files.into_iter().next() else {
        return Err(ApiError::bad_request(
            "Only .glb / .gltf files are supported",
        ));
    };
    let created = request::create_jobs(&app, vec![upload], &request);
    let id = created[0].id.clone();
    app.jobs.insert(Arc::clone(&created[0]));
    jobs::start(Arc::clone(&app), created, 1);
    Ok(HttpResponse::Ok().json(json!({
        "job_id": id,
        "output_dir": request.deliver_to.map(|d| d.display().to_string()),
    })))
}

/// Many models (field `files`), shared settings, `threads` at a time.
#[post("/api/convert-batch")]
async fn convert_batch(app: App, payload: Multipart) -> ApiResult<HttpResponse> {
    let form = multipart::collect(payload, &app.uploads, app.max_upload).await?;
    let request = request::from_v1_fields(&app.defaults, &form)?;
    if form.files.is_empty() {
        return Err(ApiError::bad_request(if form.skipped.is_empty() {
            "No files uploaded"
        } else {
            "No valid .glb / .gltf files found"
        }));
    }
    let created = request::create_jobs(&app, form.files, &request);
    let listed: Vec<_> = created
        .iter()
        .map(|j| json!({ "job_id": j.id, "filename": j.input_name }))
        .collect();
    for job in &created {
        app.jobs.insert(Arc::clone(job));
    }
    jobs::start(Arc::clone(&app), created, request.threads);
    Ok(HttpResponse::Ok().json(json!({
        "jobs": listed,
        "threads": request.threads,
        "output_dir": request.deliver_to.map(|d| d.display().to_string()),
    })))
}

/// v1's three states: a queued job reads as running, a cancelled one as an
/// error.
#[get("/api/progress/{id}")]
async fn progress(app: App, id: web::Path<String>) -> ApiResult<HttpResponse> {
    let job = shared::find_job(&app, &id)?;
    let s = job.state();
    let status = match s.status {
        Status::Queued | Status::Running => "running",
        Status::Done => "done",
        Status::Error | Status::Cancelled => "error",
    };
    Ok(HttpResponse::Ok().json(json!({
        "status": status,
        "progress": s.progress,
        "message": s.message,
        "download_name": job.file_name,
        "saved_path": s.saved_path,
        "save_error": s.save_error,
    })))
}

#[get("/api/download/{id}")]
async fn download(app: App, id: web::Path<String>) -> ApiResult<NamedFile> {
    let job = shared::find_job(&app, &id)?;
    shared::download(&job).await
}

#[post("/api/save/{id}")]
async fn save(
    app: App,
    id: web::Path<String>,
    body: web::Json<FolderRequest>,
) -> ApiResult<HttpResponse> {
    shared::save(shared::find_job(&app, &id)?, &body.path).await
}

#[post("/api/reveal/{id}")]
async fn reveal(app: App, id: web::Path<String>) -> ApiResult<HttpResponse> {
    let job = shared::find_job(&app, &id)?;
    shared::reveal(&job)
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(convert)
        .service(convert_batch)
        .service(progress)
        .service(download)
        .service(save)
        .service(reveal);
}
