//! SchemGen2's HTTP server: the API both the web UI and the Minecraft mod
//! are clients of, and the web UI itself.
//!
//! See `docs/api.md` for the routes. Two versions are mounted side by side:
//! v2 (`/api/schema`, `/api/jobs…`) and, for one more release, v1
//! (`/api/convert`, `/api/progress/…`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use tokio::sync::Semaphore;

use schemgen_core::{PaletteSet, Settings};

mod error;
mod guard;
mod jobs;
mod multipart;
mod preview;
mod request;
pub mod savedir;
mod shared;
mod sse;
mod state;
mod sweep;
mod textures;
mod v1;
mod v2;

pub use guard::host_name;
pub use state::AppState;

/// Default upload limit per model file.
pub const DEFAULT_MAX_UPLOAD: u64 = 1024 * 1024 * 1024;

/// How the server is started.
pub struct ServerConfig {
    /// Address to bind. Loopback unless deliberately exposed.
    pub host: String,
    /// Port to bind; 0 lets the OS choose (the chosen one is printed).
    pub port: u16,
    pub palettes: PaletteSet,
    /// Settings requests start from — notably the default target.
    pub defaults: Settings,
    /// Uploads and outputs live in `work_dir/uploads` and `work_dir/outputs`.
    pub work_dir: PathBuf,
    /// Built web UI to serve at `/`.
    pub ui_dir: Option<PathBuf>,
    pub token: Option<String>,
    /// Host names accepted besides the loopback ones.
    pub allowed_hosts: Vec<String>,
    /// Keep finished jobs this long; `None` keeps them until restart.
    pub job_ttl: Option<Duration>,
    /// Conversions running at once, across all requests.
    pub max_jobs: usize,
    pub max_upload: u64,
    /// Stop when standard input closes — how a parent process that dies
    /// without cleaning up still takes its server with it.
    pub exit_with_stdin: bool,
    /// Write the process id here while running.
    pub pid_file: Option<PathBuf>,
    /// Block textures for the preview: a client jar, resource pack or folder.
    /// `None` looks for a launcher's client jar.
    pub textures: Option<PathBuf>,
}

impl ServerConfig {
    pub fn new(palettes: PaletteSet) -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3001,
            palettes,
            defaults: Settings::default(),
            work_dir: default_work_dir(),
            ui_dir: None,
            token: None,
            allowed_hosts: Vec::new(),
            job_ttl: Some(Duration::from_secs(24 * 3600)),
            max_jobs: std::thread::available_parallelism().map_or(4, |n| n.get()),
            max_upload: DEFAULT_MAX_UPLOAD,
            exit_with_stdin: false,
            pid_file: None,
            textures: None,
        }
    }
}

/// Per-user cache folder for uploads and outputs: `%LOCALAPPDATA%\schemgen2`,
/// `~/Library/Caches/schemgen2` or `$XDG_CACHE_HOME/schemgen2`
/// (`~/.cache/schemgen2`), falling back to the temp folder.
pub fn default_work_dir() -> PathBuf {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        env("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Caches"))
    } else {
        env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("schemgen2")
}

/// The built web UI: `SCHEMGEN_UI_DIR`, else `frontend/dist` found from the
/// working directory, the binary's location or the source tree.
pub fn find_ui_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SCHEMGEN_UI_DIR").map(PathBuf::from) {
        return Some(dir);
    }
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

/// The shared state for `config`, with its work folders created.
pub fn build_state(config: &mut ServerConfig) -> std::io::Result<Arc<AppState>> {
    // An upload arrives alone: a .gltf naming a file beside it could only
    // reach other uploads.
    schemgen_core::mesh::allow_external_files(false);
    let uploads = config.work_dir.join("uploads");
    let outputs = config.work_dir.join("outputs");
    std::fs::create_dir_all(&uploads)?;
    std::fs::create_dir_all(&outputs)?;
    let palettes = std::mem::replace(&mut config.palettes, PaletteSet::builtin());
    Ok(Arc::new(AppState {
        palettes: Arc::new(palettes),
        jobs: jobs::JobStore::default(),
        uploads,
        outputs,
        defaults: config.defaults.clone(),
        token: config.token.clone(),
        allowed_hosts: config.allowed_hosts.clone(),
        job_ttl: config.job_ttl.filter(|d| !d.is_zero()),
        slots: Arc::new(Semaphore::new(config.max_jobs.max(1))),
        max_upload: config.max_upload,
        textures: std::sync::OnceLock::new(),
        textures_path: config.textures.clone(),
    }))
}

/// Every API route. The static UI, when there is one, is mounted after.
pub fn configure_api(cfg: &mut web::ServiceConfig) {
    cfg.configure(shared::configure)
        .configure(textures::configure)
        .configure(v2::configure)
        .configure(v1::configure)
        .service(preview::preview)
        .service(web::scope("/api").default_service(web::to(|| async {
            HttpResponse::NotFound().json(serde_json::json!({ "error": "No such API route" }))
        })));
}

/// Page served at `/` when no built UI was found.
const NO_UI_PAGE: &str = "<!doctype html><meta charset=utf-8><title>SchemGen2</title>\
<body style=\"font:15px system-ui;max-width:40em;margin:3em auto\">\
<h1>SchemGen2 is running</h1><p>The API is available under <code>/api</code>, but no web UI \
build was found. Build it with <code>cd frontend &amp;&amp; npm run build</code> and restart, or \
point <code>SCHEMGEN_UI_DIR</code> at a build.</p>";

/// Run the server until it is stopped (Ctrl+C, or standard input closing
/// with `exit_with_stdin`).
pub async fn run(mut config: ServerConfig) -> std::io::Result<()> {
    let state = build_state(&mut config)?;
    sweep::spawn(Arc::clone(&state));

    let ui_dir = config.ui_dir.clone();
    match &ui_dir {
        Some(dir) => log::info!("Serving the web UI from {}", dir.display()),
        None => log::warn!("No web UI build found — run `cd frontend && npm run build`"),
    }
    let loopback = guard::LOOPBACK_HOSTS.contains(&config.host.as_str());
    if !loopback && config.token.is_none() {
        log::warn!(
            "Listening on {} without a token: anyone who can reach this address can convert \
             files and write into folders on this machine. Pass --token.",
            config.host
        );
    }

    let app_state = Arc::clone(&state);
    let server = HttpServer::new(move || {
        let mut app = App::new()
            .app_data(web::Data::new(Arc::clone(&app_state)))
            .app_data(web::JsonConfig::default().limit(64 * 1024))
            .wrap(middleware::from_fn(guard::guard))
            .configure(configure_api);
        app = match &ui_dir {
            Some(dir) => app.service(
                actix_files::Files::new("/", dir)
                    .index_file("index.html")
                    .prefer_utf8(true),
            ),
            None => app.route(
                "/",
                web::get().to(|| async {
                    HttpResponse::Ok()
                        .content_type("text/html; charset=utf-8")
                        .body(NO_UI_PAGE)
                }),
            ),
        };
        app
    })
    .bind((config.host.as_str(), config.port))?;

    let addrs = server.addrs();
    let server = server.run();
    let handle = server.handle();

    if let Some(addr) = addrs.first() {
        let shown_host = if addr.ip().is_unspecified() {
            "127.0.0.1".to_string()
        } else if addr.is_ipv6() {
            format!("[{}]", addr.ip())
        } else {
            addr.ip().to_string()
        };
        let url = format!("http://{shown_host}:{}", addr.port());
        log::info!("SchemGen2 {} listening on {url}", schemgen_core::VERSION);
        if let Some(token) = &config.token {
            log::info!("Open {url}/?token={token} to use the web UI");
        }
        // The one line a launching process waits for, e.g. with --port 0.
        let mut out = std::io::stdout();
        let _ = writeln!(out, "listening {url}");
        let _ = out.flush();
    }
    if let Some(pid_file) = &config.pid_file {
        std::fs::write(pid_file, std::process::id().to_string())?;
    }
    if config.exit_with_stdin {
        // A plain thread, not the runtime's: a blocking read on the runtime
        // would hold up its shutdown for as long as stdin stays open.
        let (closed_tx, closed_rx) = tokio::sync::oneshot::channel::<()>();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 256];
            while matches!(stdin.read(&mut buf), Ok(n) if n > 0) {}
            let _ = closed_tx.send(());
        });
        let handle = handle.clone();
        tokio::spawn(async move {
            if closed_rx.await.is_ok() {
                log::info!("Standard input closed — shutting down");
                handle.stop(true).await;
            }
        });
    }

    let result = server.await;

    // Stop whatever is still converting, so no voxelizer outlives the server.
    for job in state.jobs.list() {
        job.request_cancel();
    }
    if let Some(pid_file) = &config.pid_file {
        let _ = std::fs::remove_file(pid_file);
    }
    result
}

/// [`run`] on a runtime of its own, for a plain `main`.
pub fn run_blocking(config: ServerConfig) -> std::io::Result<()> {
    actix_web::rt::System::new().block_on(run(config))
}

/// A fresh random token: 256 bits as hex.
pub fn generate_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Write a secret readable only by its owner (on Unix; Windows profiles are
/// per-user already).
pub fn write_private_file(path: &Path, contents: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests;
