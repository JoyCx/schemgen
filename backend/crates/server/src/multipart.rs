//! Reading `multipart/form-data` uploads: model files stream to disk, text
//! fields are kept in memory. Both API versions post the same shape — files
//! plus text — so one reader serves them all.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use actix_multipart::Multipart;
use actix_web::http::StatusCode;
use futures_util::TryStreamExt;
use tokio::io::AsyncWriteExt;

use crate::error::{ApiError, ApiResult};

/// Longest text field accepted — settings JSON is a few hundred bytes.
const MAX_TEXT_FIELD: usize = 1 << 20;

/// A model file saved from the request. Deleted when dropped unless it was
/// handed to a job with [`Upload::keep`], so a request that fails halfway
/// leaves nothing behind.
pub struct Upload {
    /// Name the client gave the file.
    pub file_name: String,
    pub path: PathBuf,
    kept: bool,
}

impl Upload {
    /// Take ownership of the file; it is no longer deleted on drop.
    pub fn keep(mut self) -> PathBuf {
        self.kept = true;
        self.path.clone()
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        if !self.kept {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[derive(Default)]
pub struct Form {
    pub files: Vec<Upload>,
    pub fields: HashMap<String, String>,
    /// Files that were not glTF, by name — skipped rather than failing a batch.
    pub skipped: Vec<String>,
}

impl Form {
    /// A trimmed text field, `None` when absent or empty.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.fields
            .get(key)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
    }
}

pub fn is_gltf(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".glb") || lower.ends_with(".gltf")
}

fn malformed(e: impl std::fmt::Display) -> ApiError {
    ApiError::bad_request(format!("Malformed upload: {e}"))
}

fn too_large(what: &str, limit: u64) -> ApiError {
    ApiError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        format!(
            "{what} is larger than the {} MB limit",
            limit / (1024 * 1024)
        ),
    )
}

/// Read every field of `payload`. Files go into `dir`, each at most
/// `max_file` bytes.
pub async fn collect(mut payload: Multipart, dir: &Path, max_file: u64) -> ApiResult<Form> {
    let mut form = Form::default();
    while let Some(mut field) = payload.try_next().await.map_err(malformed)? {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field
            .content_disposition()
            .and_then(|cd| cd.get_filename())
            .map(str::to_string)
            .filter(|f| !f.is_empty());

        match file_name {
            // Settings are text even when a client attaches them as a file.
            Some(file_name) if name != "settings" => {
                if !is_gltf(&file_name) {
                    while field.try_next().await.map_err(malformed)?.is_some() {}
                    form.skipped.push(file_name);
                    continue;
                }
                let ext = if file_name.to_ascii_lowercase().ends_with(".gltf") {
                    "gltf"
                } else {
                    "glb"
                };
                let upload = Upload {
                    path: dir.join(format!("{}.{ext}", uuid::Uuid::new_v4())),
                    file_name,
                    kept: false,
                };
                let mut file = tokio::fs::File::create(&upload.path).await?;
                let mut size = 0u64;
                while let Some(chunk) = field.try_next().await.map_err(malformed)? {
                    size += chunk.len() as u64;
                    if size > max_file {
                        return Err(too_large(&upload.file_name, max_file));
                    }
                    file.write_all(&chunk).await?;
                }
                file.flush().await?;
                form.files.push(upload);
            }
            _ => {
                let mut buf = Vec::new();
                while let Some(chunk) = field.try_next().await.map_err(malformed)? {
                    if buf.len() + chunk.len() > MAX_TEXT_FIELD {
                        return Err(too_large(
                            &format!("Field \"{name}\""),
                            MAX_TEXT_FIELD as u64,
                        ));
                    }
                    buf.extend_from_slice(&chunk);
                }
                form.fields
                    .insert(name, String::from_utf8_lossy(&buf).into_owned());
            }
        }
    }
    Ok(form)
}
