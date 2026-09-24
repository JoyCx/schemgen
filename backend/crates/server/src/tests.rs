//! The HTTP surface, exercised in-process.
//!
//! Tests that run a real conversion need the voxelizer; while it is the
//! Python helper they skip, loudly, when no interpreter with trimesh is found
//! (`SCHEMGEN_PYTHON` picks one).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use actix_web::http::StatusCode;
use actix_web::{test, web, App};
use serde_json::{json, Value};

use schemgen_core::{PaletteSet, Settings};

use super::*;
use crate::jobs::Status;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn voxelizer_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let ok = std::process::Command::new(schemgen_core::voxelizer::python_interpreter())
            .args(["-c", "import trimesh, scipy, PIL"])
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            eprintln!("SKIPPING conversion tests: no Python with trimesh (set SCHEMGEN_PYTHON)");
        }
        ok
    })
}

fn config(tag: &str) -> ServerConfig {
    let mut c = ServerConfig::new(PaletteSet::builtin());
    c.work_dir = std::env::temp_dir().join(format!("schemgen_srv_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&c.work_dir);
    c
}

/// The API as `run` mounts it, minus the static UI.
macro_rules! service {
    ($state:expr) => {
        test::init_service(
            App::new()
                .app_data(web::Data::new($state))
                .wrap(middleware::from_fn(guard::guard))
                .configure(configure_api),
        )
        .await
    };
}

/// A multipart body of files (field name, file name, bytes) and text fields.
fn multipart(files: &[(&str, &str, Vec<u8>)], fields: &[(&str, &str)]) -> (String, Vec<u8>) {
    let boundary = "----schemgen-test-boundary";
    let mut body = Vec::new();
    for (field, file_name, bytes) in files {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{file_name}\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn post_multipart(
    uri: &str,
    files: &[(&str, &str, Vec<u8>)],
    fields: &[(&str, &str)],
) -> test::TestRequest {
    let (content_type, body) = multipart(files, fields);
    test::TestRequest::post()
        .uri(uri)
        .insert_header(("Content-Type", content_type))
        .set_payload(body)
}

fn model() -> Vec<u8> {
    std::fs::read(fixture("textured.glb")).expect("fixture textured.glb")
}

async fn wait_finished(state: &AppState, id: &str) -> jobs::JobState {
    for _ in 0..600 {
        let s = state.jobs.get(id).expect("job exists").state();
        if s.status.is_finished() {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("job {id} did not finish within a minute");
}

#[actix_web::test]
async fn health_reports_version_and_target() {
    let app = service!(build_state(&mut config("health")).unwrap());
    let body: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::get().uri("/api/health").to_request(),
    )
    .await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], schemgen_core::VERSION);
    assert_eq!(body["api"], 2);
    assert_eq!(body["target"], "1.21.8");
    assert_eq!(body["data_version"], 4440);
    assert_eq!(body["auth"], false);
}

#[actix_web::test]
async fn schema_describes_fields_and_targets() {
    let mut c = config("schema");
    c.defaults = Settings {
        target: "1.20.4".into(),
        ..Settings::default()
    };
    let app = service!(build_state(&mut c).unwrap());
    let body: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::get().uri("/api/schema").to_request(),
    )
    .await;
    let field = |key: &str| {
        body["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["key"] == key)
            .cloned()
            .unwrap_or_else(|| panic!("field {key}"))
    };
    assert_eq!(field("max_size")["default"], 128);
    assert_eq!(field("max_size")["type"], "int");
    assert_eq!(
        field("target")["default"],
        "1.20.4",
        "defaults are the server's"
    );
    assert_eq!(body["default_target"], "1.20.4");
    assert!(body["targets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == "26.3"));
    assert!(body["groups"].as_array().unwrap().len() >= 5);
}

#[actix_web::test]
async fn foreign_hosts_and_origins_are_refused() {
    let app = service!(build_state(&mut config("guard")).unwrap());
    let req = |host: &str, origin: Option<&str>| {
        let mut r = test::TestRequest::get()
            .uri("/api/health")
            .insert_header(("Host", host));
        if let Some(o) = origin {
            r = r.insert_header(("Origin", o));
        }
        r.to_request()
    };
    let status = |r| async { test::call_service(&app, r).await.status() };
    assert_eq!(status(req("localhost:3001", None)).await, StatusCode::OK);
    assert_eq!(
        status(req("127.0.0.1:3001", Some("http://localhost:5173"))).await,
        StatusCode::OK
    );
    assert_eq!(
        status(req("rebind.example:3001", None)).await,
        StatusCode::MISDIRECTED_REQUEST
    );
    assert_eq!(
        status(req("localhost:3001", Some("https://evil.example"))).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(req("localhost:3001", Some("null"))).await,
        StatusCode::FORBIDDEN
    );
}

#[actix_web::test]
async fn token_is_required_everywhere_but_health() {
    let mut c = config("token");
    c.token = Some("s3cret".into());
    let app = service!(build_state(&mut c).unwrap());
    let get = |uri: &str| test::TestRequest::get().uri(uri);
    assert_eq!(
        test::call_service(&app, get("/api/health").to_request())
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        test::call_service(&app, get("/api/schema").to_request())
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let bearer = get("/api/schema")
        .insert_header(("Authorization", "Bearer s3cret"))
        .to_request();
    assert_eq!(
        test::call_service(&app, bearer).await.status(),
        StatusCode::OK
    );
    let wrong = get("/api/schema")
        .insert_header(("Authorization", "Bearer nope"))
        .to_request();
    assert_eq!(
        test::call_service(&app, wrong).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        test::call_service(&app, get("/api/jobs?token=s3cret").to_request())
            .await
            .status(),
        StatusCode::OK
    );
}

#[actix_web::test]
async fn bad_requests_say_what_is_wrong() {
    let app = service!(build_state(&mut config("bad")).unwrap());

    let resp = test::call_service(&app, post_multipart("/api/jobs", &[], &[]).to_request()).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let resp = test::call_service(
        &app,
        post_multipart(
            "/api/jobs",
            &[("file", "model.obj", b"o cube".to_vec())],
            &[],
        )
        .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = test::read_body_json(resp).await;
    assert!(
        body["error"].as_str().unwrap().contains("model.obj"),
        "{body}"
    );

    let resp = test::call_service(
        &app,
        post_multipart(
            "/api/jobs",
            &[("file", "m.glb", model())],
            &[("settings", r#"{"max_size": "big"}"#)],
        )
        .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = test::read_body_json(resp).await;
    assert!(
        body["error"].as_str().unwrap().contains("max_size"),
        "{body}"
    );

    let resp = test::call_service(
        &app,
        test::TestRequest::get().uri("/api/jobs/nope").to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/no-such-route")
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: Value = test::read_body_json(resp).await;
    assert!(body["error"].is_string());
}

#[actix_web::test]
async fn queued_jobs_cancel_immediately() {
    let mut c = config("cancel");
    c.max_jobs = 1;
    let state = build_state(&mut c).unwrap();
    // Hold the only slot so everything submitted stays queued.
    let slot = Arc::clone(&state.slots).acquire_owned().await.unwrap();
    let app = service!(Arc::clone(&state));
    let body: Value = test::call_and_read_body_json(
        &app,
        post_multipart("/api/jobs", &[("file", "m.glb", model())], &[]).to_request(),
    )
    .await;
    let id = body["job_id"].as_str().unwrap().to_string();
    assert_eq!(state.jobs.get(&id).unwrap().state().status, Status::Queued);

    let resp = test::call_service(
        &app,
        test::TestRequest::delete()
            .uri(&format!("/api/jobs/{id}"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    assert_eq!(
        state.jobs.get(&id).unwrap().state().status,
        Status::Cancelled
    );
    drop(slot);
    let finished = wait_finished(&state, &id).await;
    assert_eq!(finished.status, Status::Cancelled);
    assert!(
        !state.jobs.get(&id).unwrap().upload.exists(),
        "upload removed"
    );

    // A finished job is forgotten on DELETE.
    let resp = test::call_service(
        &app,
        test::TestRequest::delete()
            .uri(&format!("/api/jobs/{id}"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(state.jobs.get(&id).is_none());
}

#[actix_web::test]
async fn v2_job_runs_to_a_valid_schematic() {
    if !voxelizer_available() {
        return;
    }
    let state = build_state(&mut config("v2")).unwrap();
    let app = service!(Arc::clone(&state));
    let resp = test::call_service(
        &app,
        post_multipart(
            "/api/jobs",
            &[("file", "castle.glb", model())],
            &[(
                "settings",
                r#"{"max_size": 24, "target": "1.20.4", "colour": 1}"#,
            )],
        )
        .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["ignored"], json!(["colour"]));
    let id = body["job_id"].as_str().unwrap().to_string();

    let state_now = wait_finished(&state, &id).await;
    assert_eq!(state_now.status, Status::Done, "{:?}", state_now.error);

    let view: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/jobs/{id}"))
            .to_request(),
    )
    .await;
    assert_eq!(view["status"], "done");
    assert_eq!(view["progress"], 100.0);
    assert_eq!(view["download_name"], "castle.litematic");
    assert_eq!(view["result"]["target"], "1.20.4");
    let dims = view["result"]["dims"].as_array().unwrap();
    assert_eq!(dims.iter().map(|d| d.as_u64().unwrap()).max(), Some(24));
    let materials = view["result"]["materials"].as_array().unwrap();
    let placed: u64 = materials.iter().map(|m| m["count"].as_u64().unwrap()).sum();
    assert_eq!(placed, view["result"]["blocks"].as_u64().unwrap());

    let list: Value =
        test::call_and_read_body_json(&app, test::TestRequest::get().uri("/api/jobs").to_request())
            .await;
    assert_eq!(list["jobs"][0]["id"], id.as_str());

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/jobs/{id}/download"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let disposition = resp
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(disposition.contains("castle.litematic"), "{disposition}");
    let bytes = test::read_body(resp).await;
    let (_, root) = schemgen_core::formats::nbt::read_gzip(&bytes[..]).unwrap();
    assert_eq!(
        root.get("MinecraftDataVersion"),
        Some(&schemgen_core::formats::nbt::Tag::Int(3700))
    );
    assert_eq!(
        root.get("Version"),
        Some(&schemgen_core::formats::nbt::Tag::Int(6))
    );

    let png = test::call_and_read_body(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/jobs/{id}/thumbnail.png"))
            .to_request(),
    )
    .await;
    assert_eq!(&png[..4], b"\x89PNG");

    let events = test::call_and_read_body(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/jobs/{id}/events"))
            .to_request(),
    )
    .await;
    let text = String::from_utf8_lossy(&events);
    assert!(text.starts_with("retry: "), "{text}");
    assert!(text.contains("event: done\ndata: {"), "{text}");
}

#[actix_web::test]
async fn v1_routes_still_work() {
    if !voxelizer_available() {
        return;
    }
    let state = build_state(&mut config("v1")).unwrap();
    let app = service!(Arc::clone(&state));
    let body: Value = test::call_and_read_body_json(
        &app,
        post_multipart(
            "/api/convert",
            &[("file", "model.glb", model())],
            &[
                ("max_size", "20"),
                ("voxel_size", ""),
                ("ram_limit", "4"),
                ("dither", "true"),
                ("color_sampling", "true"),
                ("brightness", "0"),
                ("contrast", "1"),
                ("saturation", "1"),
                ("no_color_block", "white"),
                ("schematic_name", "legacy"),
            ],
        )
        .to_request(),
    )
    .await;
    let id = body["job_id"].as_str().unwrap().to_string();
    wait_finished(&state, &id).await;
    let progress: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/progress/{id}"))
            .to_request(),
    )
    .await;
    assert_eq!(progress["status"], "done", "{progress}");
    assert_eq!(progress["download_name"], "legacy.litematic");
    assert!(progress["message"].as_str().unwrap().starts_with("Done! "));
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/download/{id}"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[actix_web::test]
async fn previews_come_packed_for_v2_and_as_objects_for_v1() {
    if !voxelizer_available() {
        return;
    }
    let app = service!(build_state(&mut config("preview")).unwrap());

    let v2: Value = test::call_and_read_body_json(
        &app,
        post_multipart(
            "/api/preview",
            &[("file", "m.glb", model())],
            &[("settings", r#"{"max_size": 16}"#)],
        )
        .to_request(),
    )
    .await;
    let count = v2["count"].as_u64().unwrap() as usize;
    use base64::Engine;
    let packed = base64::engine::general_purpose::STANDARD
        .decode(v2["blocks"].as_str().unwrap())
        .unwrap();
    assert_eq!(packed.len(), count * 16);
    let palette_len = v2["palette"].as_array().unwrap().len() as i32;
    let ints: Vec<i32> = packed
        .chunks(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    assert!(ints.chunks(4).all(|q| (0..palette_len).contains(&q[3])));
    assert!(v2["pitch"].as_f64().unwrap() > 0.0);

    let v1: Value = test::call_and_read_body_json(
        &app,
        post_multipart(
            "/api/preview",
            &[("file", "m.glb", model())],
            &[("max_size", "16")],
        )
        .to_request(),
    )
    .await;
    assert_eq!(v1["blocks"].as_array().unwrap().len(), count);
    assert!(v1["blocks"][0]["name"]
        .as_str()
        .unwrap()
        .starts_with("minecraft:"));
}

#[actix_web::test]
async fn sweep_forgets_expired_jobs_and_stray_files() {
    let mut c = config("sweep");
    let state = build_state(&mut c).unwrap();
    let job = jobs::Job::new(
        &state.outputs,
        jobs::NewJob {
            input_name: "old.glb".into(),
            upload: state.uploads.join("old.glb"),
            settings: Settings::default(),
            name: "old".into(),
            file_name: "old.litematic".into(),
            deliver_to: None,
        },
    );
    std::fs::write(&job.output, b"x").unwrap();
    job.update(|s| {
        s.status = Status::Done;
        s.finished_ms = Some(schemgen_core::formats::now_ms() - 3_600_000);
    });
    state.jobs.insert(Arc::clone(&job));
    let stray = state.uploads.join("stray.glb");
    std::fs::write(&stray, b"x").unwrap();

    // Younger than the TTL: kept.
    assert_eq!(sweep::sweep(&state, Duration::from_secs(7200)), 0);
    // Older: gone, with its file — and the stray file is younger than a
    // minute, so it stays until it too is old enough.
    assert_eq!(sweep::sweep(&state, Duration::from_secs(60)), 1);
    assert!(state.jobs.get(&job.id).is_none());
    assert!(!job.output.exists());
    assert!(stray.exists());
    let _ = std::fs::remove_dir_all(&c.work_dir);
}

#[actix_web::test]
async fn palettes_follow_the_target() {
    let app = service!(build_state(&mut config("palette")).unwrap());
    let old: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::get()
            .uri("/api/palette?target=1.16.5")
            .to_request(),
    )
    .await;
    let new: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::get().uri("/api/palette").to_request(),
    )
    .await;
    let (old, new) = (old.as_object().unwrap(), new.as_object().unwrap());
    assert!(old.len() < new.len(), "{} vs {}", old.len(), new.len());
    assert!(!old.contains_key("deepslate") && new.contains_key("deepslate"));
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/palette?target=1.8")
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let schema: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::get().uri("/api/schema").to_request(),
    )
    .await;
    let blocks_for = |id: &str| {
        schema["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == id)
            .unwrap()["blocks"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(blocks_for("1.16.5") as usize, old.len());
    assert!(blocks_for("26.3") >= blocks_for("1.21.1"));
    let formats: Vec<&str> = schema["formats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(formats, ["litematic", "schem", "schem-v3", "nbt"]);
}

#[actix_web::test]
async fn old_targets_never_get_newer_blocks() {
    if !voxelizer_available() {
        return;
    }
    let state = build_state(&mut config("gating")).unwrap();
    let app = service!(Arc::clone(&state));
    let body: Value = test::call_and_read_body_json(
        &app,
        post_multipart(
            "/api/jobs",
            &[(
                "file",
                "m.glb",
                std::fs::read(fixture("metal_transforms.glb")).unwrap(),
            )],
            &[(
                "settings",
                r#"{"max_size": 40, "target": "1.16.5", "format": "schem"}"#,
            )],
        )
        .to_request(),
    )
    .await;
    let id = body["job_id"].as_str().unwrap().to_string();
    let finished = wait_finished(&state, &id).await;
    assert_eq!(finished.status, Status::Done, "{:?}", finished.error);

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/jobs/{id}/download"))
            .to_request(),
    )
    .await;
    let disposition = resp
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(disposition.contains("m.schem"), "{disposition}");
    let bytes = test::read_body(resp).await;
    let (_, root) = schemgen_core::formats::nbt::read_gzip(&bytes[..]).unwrap();
    use schemgen_core::formats::nbt::Tag;
    assert_eq!(root.get("Version"), Some(&Tag::Int(2)));
    assert_eq!(root.get("DataVersion"), Some(&Tag::Int(2586)));
    let Some(Tag::Compound(palette)) = root.get("Palette") else {
        panic!("Palette")
    };
    let versions = schemgen_core::BlockVersions::builtin();
    let floor = schemgen_core::targets::FLOOR;
    for (name, _) in palette {
        assert!(
            versions.exists_in(name, &floor),
            "{name} does not exist in 1.16.5"
        );
    }
    assert!(palette.len() > 2, "some color blocks were used");
}
