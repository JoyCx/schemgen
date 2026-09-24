//! The web UI at `/`: built into the binary, or from a folder on disk.

use std::path::{Path, PathBuf};

use actix_web::http::{header, Method};
use actix_web::{web, HttpRequest, HttpResponse};

mod embedded {
    // Written by build.rs: `pub static FILES: &[(&str, &[u8])]`.
    include!(concat!(env!("OUT_DIR"), "/ui.rs"));
}

/// A file table: `/`-separated paths and their contents.
pub(crate) type Files = &'static [(&'static str, &'static [u8])];

/// Where the web UI comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ui {
    /// The UI built into this binary (see `build.rs`).
    Embedded,
    /// A built UI in a folder.
    Dir(PathBuf),
    /// None: `/` says how to get one. The API works all the same.
    None,
}

impl Ui {
    /// Whether this binary was built with the web UI inside.
    pub fn is_embedded_available() -> bool {
        !embedded::FILES.is_empty()
    }

    /// The UI to serve when none was asked for: the folder `SCHEMGEN_UI_DIR`
    /// names, else the one built into the binary, else a built
    /// `frontend/dist` found from the working directory, the binary's
    /// location or the source tree.
    pub fn find() -> Ui {
        if let Some(dir) = std::env::var_os("SCHEMGEN_UI_DIR").filter(|d| !d.is_empty()) {
            return Ui::Dir(PathBuf::from(dir));
        }
        if Self::is_embedded_available() {
            return Ui::Embedded;
        }
        find_on_disk().map_or(Ui::None, Ui::Dir)
    }
}

/// A built `frontend/dist` near the working directory, the binary or the
/// source tree — for builds without the UI inside.
fn find_on_disk() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_deref().and_then(Path::parent);
    let cwd = std::env::current_dir().ok();
    [
        cwd.as_ref().map(|d| d.join("frontend/dist")),
        cwd.as_ref().map(|d| d.join("../frontend/dist")),
        exe_dir.map(|d| d.join("ui")),
        // backend/target/release/schemgen2 → frontend/dist
        exe_dir.map(|d| d.join("../../../frontend/dist")),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../frontend/dist")),
    ]
    .into_iter()
    .flatten()
    .find(|d| d.join("index.html").is_file())
}

/// The built-in UI's files.
pub(crate) fn embedded_files() -> Files {
    embedded::FILES
}

/// Serve `files` for every request no route took: `/` is `index.html`,
/// Vite's content-hashed `assets/` are cached for good, the rest revalidated.
pub(crate) fn service(files: Files) -> actix_web::Route {
    web::to(move |req: HttpRequest| async move { respond(files, &req) })
}

fn respond(files: Files, req: &HttpRequest) -> HttpResponse {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return HttpResponse::MethodNotAllowed()
            .insert_header((header::ALLOW, "GET, HEAD"))
            .finish();
    }
    let path = req.path().trim_start_matches('/');
    let name = if path.is_empty() || path.ends_with('/') {
        format!("{path}index.html")
    } else {
        path.to_string()
    };
    let Some(&(name, body)) = files.iter().find(|(n, _)| *n == name) else {
        return HttpResponse::NotFound()
            .content_type("text/plain; charset=utf-8")
            .body("Not found");
    };
    let cache = if name.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    HttpResponse::Ok()
        .content_type(content_type(name))
        .insert_header((header::CACHE_CONTROL, cache))
        .insert_header((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .body(body)
}

/// The media type for a file of the built UI.
fn content_type(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    match ext.to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{body::to_bytes, test, App};

    fn header_of(res: &actix_web::dev::ServiceResponse, name: &str) -> String {
        res.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    static FILES: Files = &[
        ("assets/main-1a2b.js", b"console.log(1)"),
        ("index.html", b"<!doctype html><div id=\"root\"></div>"),
        ("shader-check.html", b"<!doctype html>"),
    ];

    #[actix_web::test]
    async fn serves_the_files_with_their_types_and_caching() {
        let app = test::init_service(App::new().default_service(service(FILES))).await;

        let res = test::call_service(&app, test::TestRequest::get().uri("/").to_request()).await;
        assert_eq!(res.status(), 200);
        assert_eq!(header_of(&res, "content-type"), "text/html; charset=utf-8");
        assert_eq!(header_of(&res, "cache-control"), "no-cache");
        let body = to_bytes(res.into_body()).await.unwrap();
        assert!(body.starts_with(b"<!doctype html>"));

        let req = test::TestRequest::get()
            .uri("/assets/main-1a2b.js")
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), 200);
        assert_eq!(
            header_of(&res, "content-type"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            header_of(&res, "cache-control"),
            "public, max-age=31536000, immutable"
        );

        let req = test::TestRequest::default()
            .method(Method::HEAD)
            .uri("/shader-check.html")
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), 200);
    }

    #[actix_web::test]
    async fn anything_else_is_not_found_or_not_allowed() {
        let app = test::init_service(App::new().default_service(service(FILES))).await;
        for uri in ["/missing.js", "/assets/", "/../Cargo.toml", "/index.html/x"] {
            let res =
                test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(res.status(), 404, "{uri}");
        }
        let req = test::TestRequest::post().uri("/").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), 405);
        assert_eq!(header_of(&res, "allow"), "GET, HEAD");
    }

    #[::core::prelude::v1::test]
    fn content_types_follow_the_extension() {
        assert_eq!(content_type("a/b.CSS"), "text/css; charset=utf-8");
        assert_eq!(content_type("favicon.svg"), "image/svg+xml");
        assert_eq!(content_type("LICENSE"), "application/octet-stream");
    }
}
