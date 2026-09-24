//! API v2: settings as one JSON object, jobs as a resource, progress as
//! server-sent events.

use std::sync::Arc;

use actix_files::NamedFile;
use actix_multipart::Multipart;
use actix_web::{delete, get, post, web, HttpResponse};
use serde_json::json;

use schemgen_core::schema::schema;
use schemgen_core::targets::TARGETS;

use crate::error::{ApiError, ApiResult};
use crate::jobs::{self, Status};
use crate::multipart;
use crate::request;
use crate::shared::{self, App, FolderRequest};
use crate::sse;

/// Everything a client needs to render a settings form: fields with types,
/// ranges, defaults, groups, labels and help; the Minecraft versions it can
/// target; and the palette's size.
#[get("/api/schema")]
async fn get_schema(app: App) -> HttpResponse {
    let mut schema = schema();
    // Defaults are this server's, which may target another version than the
    // built-in default.
    let defaults = serde_json::to_value(&app.defaults).unwrap_or_default();
    for field in &mut schema.fields {
        if let Some(v) = defaults.get(field.key) {
            field.default = v.clone();
        }
    }
    let targets: Vec<_> = TARGETS
        .iter()
        .rev()
        .map(|t| {
            json!({
                "id": t.id,
                "data_version": t.data_version,
                "schematic_version": t.schematic_version,
            })
        })
        .collect();
    HttpResponse::Ok().json(json!({
        "version": schemgen_core::VERSION,
        "groups": schema.groups,
        "fields": schema.fields,
        "targets": targets,
        "default_target": app.defaults.target().key(),
        "formats": ["litematic"],
        "palette": {
            "entries": app.palette.len(),
            "blocks": app.palette.block_count(),
        },
        "limits": {
            "max_upload_bytes": app.max_upload,
        },
    }))
}

/// Start converting one or more uploaded models: `file` / `files` parts plus
/// an optional `settings` JSON part.
#[post("/api/jobs")]
async fn create(app: App, payload: Multipart) -> ApiResult<HttpResponse> {
    let form = multipart::collect(payload, &app.uploads, app.max_upload).await?;
    let request = request::from_json(&app.defaults, form.text("settings"))?;
    if form.files.is_empty() {
        return Err(ApiError::bad_request(if form.skipped.is_empty() {
            "Upload at least one .glb or .gltf model".to_string()
        } else {
            format!(
                "Only .glb / .gltf models are supported (got {})",
                form.skipped.join(", ")
            )
        }));
    }
    let skipped = form.skipped.clone();
    let created = request::create_jobs(&app, form.files, &request);
    for job in &created {
        app.jobs.insert(Arc::clone(job));
    }
    let listed: Vec<_> = created
        .iter()
        .map(|j| json!({ "job_id": j.id, "filename": j.input_name, "name": j.name }))
        .collect();
    let first = created.first().map(|j| j.id.clone());
    jobs::start(Arc::clone(&app), created, request.threads);

    Ok(HttpResponse::Accepted().json(json!({
        "job_id": first,
        "jobs": listed,
        "skipped": skipped,
        "ignored": request.ignored,
        "output_dir": request.deliver_to.map(|d| d.display().to_string()),
    })))
}

/// Every job the server remembers, newest first.
#[get("/api/jobs")]
async fn list(app: App) -> HttpResponse {
    let jobs: Vec<_> = app.jobs.list().iter().map(|j| j.view()).collect();
    HttpResponse::Ok().json(json!({ "jobs": jobs }))
}

#[get("/api/jobs/{id}")]
async fn get(app: App, id: web::Path<String>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(shared::find_job(&app, &id)?.view()))
}

/// Cancel a job that has not finished, or forget a finished one and delete
/// its files.
#[delete("/api/jobs/{id}")]
async fn remove(app: App, id: web::Path<String>) -> ApiResult<HttpResponse> {
    let job = shared::find_job(&app, &id)?;
    if job.state().status.is_finished() {
        app.jobs.remove(&job.id);
        job.remove_files();
        Ok(HttpResponse::NoContent().finish())
    } else {
        job.request_cancel();
        Ok(HttpResponse::Accepted().json(job.view()))
    }
}

#[get("/api/jobs/{id}/events")]
async fn events(app: App, id: web::Path<String>) -> ApiResult<HttpResponse> {
    Ok(sse::events(shared::find_job(&app, &id)?))
}

#[get("/api/jobs/{id}/download")]
async fn download(app: App, id: web::Path<String>) -> ApiResult<NamedFile> {
    let job = shared::find_job(&app, &id)?;
    shared::download(&job).await
}

/// A 256 × 256 isometric render of the finished schematic.
#[get("/api/jobs/{id}/thumbnail.png")]
async fn thumbnail(app: App, id: web::Path<String>) -> ApiResult<NamedFile> {
    let job = shared::find_job(&app, &id)?;
    if job.state().status != Status::Done {
        return Err(ApiError::conflict("Conversion is not finished yet"));
    }
    NamedFile::open_async(&job.thumbnail)
        .await
        .map_err(|_| ApiError::not_found("No thumbnail for this job"))
}

#[post("/api/jobs/{id}/save")]
async fn save(
    app: App,
    id: web::Path<String>,
    body: web::Json<FolderRequest>,
) -> ApiResult<HttpResponse> {
    shared::save(shared::find_job(&app, &id)?, &body.path).await
}

#[post("/api/jobs/{id}/reveal")]
async fn reveal(app: App, id: web::Path<String>) -> ApiResult<HttpResponse> {
    let job = shared::find_job(&app, &id)?;
    shared::reveal(&job)
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(get_schema)
        .service(create)
        .service(list)
        .service(get)
        .service(remove)
        .service(events)
        .service(download)
        .service(thumbnail)
        .service(save)
        .service(reveal);
}
