//! `POST /api/preview`: run the pipeline without writing a file and return
//! the blocks, for a UI to draw before anything is committed.
//!
//! A v2 request (a `settings` part, or no settings at all) gets the packed
//! v2 response. A v1 request — recognizable because v1 always sent
//! `max_size` as its own field — gets the old one, `{grid, blocks:[{x,y,z,name}]}`.

use actix_multipart::Multipart;
use actix_web::{post, HttpResponse};
use base64::Engine;
use serde_json::json;

use schemgen_core::{pipeline, BlockGrid, Cancel, Progress, Stage};

use crate::error::{ApiError, ApiResult};
use crate::multipart;
use crate::request;
use crate::shared::App;

/// Previews are for looking, not building: past this size they stop being
/// quick, so larger requests are drawn at this size instead.
pub const MAX_PREVIEW_SIZE: u32 = 256;

/// Stops a preview's pipeline when its request goes away — the web UI drops
/// the previous preview whenever a setting changes.
struct CancelOnDrop(Cancel);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

struct CancelOnly(Cancel);

impl Progress for CancelOnly {
    fn update(&mut self, _: Stage, _: f32, _: &str) {}
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

#[post("/api/preview")]
async fn preview(app: App, payload: Multipart) -> ApiResult<HttpResponse> {
    let form = multipart::collect(payload, &app.uploads, app.max_upload).await?;
    let legacy = !form.fields.contains_key("settings") && form.fields.contains_key("max_size");
    let mut request = if legacy {
        request::from_v1_fields(&app.defaults, &form)?
    } else {
        request::from_json(&app.defaults, form.text("settings"))?
    };
    let capped =
        request.settings.max_size > MAX_PREVIEW_SIZE && request.settings.voxel_size.is_none();
    if capped {
        request.settings.max_size = MAX_PREVIEW_SIZE;
    }
    let Some(upload) = form.files.into_iter().next() else {
        return Err(ApiError::bad_request(
            "Only .glb / .gltf files are supported",
        ));
    };

    let cancel = Cancel::new();
    let _guard = CancelOnDrop(cancel.clone());
    let palette = app.palettes.for_target(&request.settings.target())?;
    let settings = request.settings.clone();
    let path = upload.path.clone();
    let started = std::time::Instant::now();
    let grid = actix_web::web::block(move || {
        pipeline::run(&path, &settings, &palette, &mut CancelOnly(cancel))
    })
    .await
    .map_err(|e| ApiError::internal(format!("Preview task failed: {e}")))??;
    drop(upload);

    if legacy {
        let blocks: Vec<_> = (0..grid.len())
            .map(|i| {
                let [x, y, z] = grid.coords[i];
                json!({ "x": x, "y": y, "z": z, "name": grid.name_at(i) })
            })
            .collect();
        return Ok(HttpResponse::Ok().json(json!({
            "litematic_verified": true,
            "grid": grid.size,
            "blocks": blocks,
        })));
    }

    Ok(HttpResponse::Ok().json(json!({
        "dims": grid.size,
        "origin": grid.origin,
        "pitch": grid.pitch,
        "palette": grid.names,
        "count": grid.len(),
        "blocks": pack(&grid),
        "materials": grid.materials(),
        "target": request.settings.target,
        "capped": capped,
        "seconds": started.elapsed().as_secs_f32(),
    })))
}

/// Blocks as base64 of little-endian i32 quadruples `x, y, z, palette index`
/// — a `new Int32Array(bytes.buffer)` away from usable in a browser, and about
/// a fifth of the size of one JSON object per block.
pub fn pack(grid: &BlockGrid) -> String {
    let mut bytes = Vec::with_capacity(grid.len() * 16);
    for (c, &b) in grid.coords.iter().zip(&grid.blocks) {
        for v in [c[0], c[1], c[2], b as i32] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_little_endian_quadruples() {
        let g = BlockGrid::from_names(
            vec![[1, 2, 3], [0, 0, 258]],
            ["minecraft:stone", "minecraft:dirt"],
            [0.0; 3],
            1.0,
        );
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(pack(&g))
            .unwrap();
        let ints: Vec<i32> = bytes
            .chunks(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        // dirt sorts before stone, so stone is palette entry 1.
        assert_eq!(ints, [1, 2, 3, 1, 0, 0, 258, 0]);
    }
}
