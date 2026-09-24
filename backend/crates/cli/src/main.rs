//! SchemGen2 — GLB → Litematica converter.
//!
//! One binary, two front doors onto the same pipeline
//! (voxelize → color sample → dither → CIEDE2000 block match → schematic):
//!
//! * `schemgen2 serve` — HTTP API + the web UI
//! * `schemgen2 convert` — headless conversion
//! * `schemgen2 palette` / `targets` / `schema` / `build-table` — inspection
//!   and rebuilding the color table
//!
//! With no command it serves, so existing shortcuts keep working.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use schemgen_core::{color_table, Palette, Settings};
use schemgen_server::ServerConfig;

mod args;
mod convert;

/// CLI commands want a quiet log; `serve` wants the request/pipeline log.
fn init_logging(default_level: &str) {
    let _ =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_level))
            .try_init();
}

fn usage_error(message: &str, help: &str) -> ExitCode {
    eprintln!("error: {message}\n\n{help}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match args::parse(&argv) {
        Ok(args) => args,
        Err(e) => return usage_error(&e, args::HELP),
    };

    // `--help` / `--version` with no command are the same as the commands.
    let command = match args.command.as_str() {
        "" if args.has("help") => "help",
        "" if args.has("version") => "version",
        other => other,
    };
    match command {
        "help" | "" | "serve" => {}
        _ => init_logging("warn"),
    }
    if let Some(python) = args.get("python") {
        schemgen_core::voxelizer::set_python(python);
    }

    match command {
        "help" => {
            print!(
                "{}",
                match args.positional.first().map(String::as_str) {
                    Some("convert") => args::HELP_CONVERT,
                    Some("serve") => args::HELP_SERVE,
                    _ => args::HELP,
                }
            );
            ExitCode::SUCCESS
        }

        "version" => {
            let t = Settings::default().target();
            println!(
                "schemgen2 {} (default target Minecraft {}: data version {}, litematic v{})",
                schemgen_core::VERSION,
                t.id,
                t.data_version,
                t.schematic_version
            );
            ExitCode::SUCCESS
        }

        "convert" => {
            if args.has("help") {
                print!("{}", args::HELP_CONVERT);
                return ExitCode::SUCCESS;
            }
            let palette = match load_palette(&args) {
                Ok(p) => p,
                Err(e) => return usage_error(&e, args::HELP_CONVERT),
            };
            match convert::convert(&args, &palette) {
                Ok(code) => ExitCode::from(code as u8),
                Err(e) => {
                    eprintln!("error: {e}\n\nRun `schemgen2 help convert` for the options.");
                    ExitCode::from(2)
                }
            }
        }

        "palette" => match load_palette(&args) {
            Ok(palette) => {
                convert::palette(&args, &palette);
                ExitCode::SUCCESS
            }
            Err(e) => usage_error(&e, args::HELP),
        },

        "targets" => {
            convert::targets(&args);
            ExitCode::SUCCESS
        }

        "schema" => {
            let schema = schemgen_core::schema::schema();
            println!(
                "{}",
                serde_json::to_string_pretty(&schema).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }

        // `schemgen2 build-table <texture_dir> [output.json]` regenerates the
        // curated color table.
        "build-table" => {
            init_logging("info");
            let Some(pack) = args.positional.first().map(PathBuf::from) else {
                return usage_error(
                    "build-table needs the folder of block textures \
                     (assets/minecraft/textures/block of a client jar or resource pack)",
                    args::HELP,
                );
            };
            let out = args
                .positional
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("color_table_safe.json"));
            match color_table::build_table(&pack, &out, true) {
                Ok(table) => {
                    eprintln!("{} blocks → {}", table.len(), out.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }

        "" | "serve" => serve(&args),

        other => usage_error(&format!("unknown command '{other}'"), args::HELP),
    }
}

/// The built-in palette, unless `--palette`, `SCHEMGEN_PALETTE` or a
/// `SCHEMGEN_DATA_DIR` holding `color_table_safe.json` names another.
fn load_palette(args: &args::ParsedArgs) -> Result<Palette, String> {
    let custom = args
        .get("palette")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("SCHEMGEN_PALETTE").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("SCHEMGEN_DATA_DIR")
                .map(|d| Path::new(&d).join("color_table_safe.json"))
        });
    let table = match &custom {
        Some(path) => {
            let table = color_table::load(path).map_err(|e| e.to_string())?;
            log::info!(
                "Color table: {} blocks from {}",
                table.len(),
                path.display()
            );
            table
        }
        None => color_table::builtin(),
    };
    Palette::from_table(&table).map_err(|e| match &custom {
        Some(path) => format!("{}: {e}", path.display()),
        None => e.to_string(),
    })
}

/// A token from `--token`, `SCHEMGEN_TOKEN` or `--token-file` — which is
/// created with a fresh random token when it does not exist yet.
fn resolve_token(args: &args::ParsedArgs) -> Result<Option<String>, String> {
    if let Some(t) = args.get("token").map(str::trim).filter(|t| !t.is_empty()) {
        return Ok(Some(t.to_string()));
    }
    if let Some(t) = std::env::var("SCHEMGEN_TOKEN")
        .ok()
        .map(|t| t.trim().to_string())
    {
        if !t.is_empty() {
            return Ok(Some(t));
        }
    }
    let Some(path) = args.get("token-file").map(PathBuf::from) else {
        return Ok(None);
    };
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim();
        if !existing.is_empty() {
            return Ok(Some(existing.to_string()));
        }
    }
    let token = schemgen_server::generate_token();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("--token-file: {e}"))?;
    }
    schemgen_server::write_private_file(&path, &token).map_err(|e| format!("--token-file: {e}"))?;
    Ok(Some(token))
}

fn serve(args: &args::ParsedArgs) -> ExitCode {
    init_logging("info");
    if args.has("help") {
        print!("{}", args::HELP_SERVE);
        return ExitCode::SUCCESS;
    }

    let config = (|| -> Result<ServerConfig, String> {
        let palette = load_palette(args)?;
        let mut config = ServerConfig::new(palette);

        let port = match args.get("port") {
            Some(p) => Some(p.to_string()),
            None => std::env::var("PORT").ok(),
        };
        if let Some(port) = port {
            config.port = port
                .trim()
                .parse()
                .map_err(|_| format!("--port: '{port}' is not a port number"))?;
        }
        if let Some(host) = args.get("host") {
            config.host = host.trim().to_string();
        }
        if let Some(hosts) = args.get("allow-host") {
            config.allowed_hosts = hosts
                .split(',')
                .map(|h| schemgen_server::host_name(h).to_string())
                .filter(|h| !h.is_empty())
                .collect();
        }
        // A server bound to a specific address answers to that name too.
        if !["127.0.0.1", "localhost", "::1", "0.0.0.0", "::"].contains(&config.host.as_str()) {
            config.allowed_hosts.push(config.host.clone());
        }
        config.token = resolve_token(args)?;

        if let Some(dir) = args.get("work-dir") {
            config.work_dir = PathBuf::from(dir);
        }
        config.ui_dir = match args.get("ui-dir") {
            Some(dir) => Some(PathBuf::from(dir)),
            None => schemgen_server::find_ui_dir(),
        };
        let ttl_hours = args.number::<f64>("job-ttl", 24.0)?;
        if !(ttl_hours.is_finite() && ttl_hours >= 0.0) {
            return Err("--job-ttl must be a number of hours, 0 or more".into());
        }
        config.job_ttl = (ttl_hours > 0.0).then(|| Duration::from_secs_f64(ttl_hours * 3600.0));
        if args.get("max-jobs").is_some() {
            config.max_jobs = args.number::<usize>("max-jobs", 1)?.max(1);
        }
        config.exit_with_stdin = args.has("exit-with-stdin");
        config.pid_file = args.get("pid-file").map(PathBuf::from);

        // The server's default target: --target, else SCHEMGEN_DATA_VERSION.
        let default_target = args
            .get("target")
            .map(str::to_string)
            .or_else(|| std::env::var("SCHEMGEN_DATA_VERSION").ok())
            .filter(|t| !t.trim().is_empty());
        if let Some(target) = default_target {
            config.defaults = Settings {
                target,
                ..Settings::default()
            }
            .normalized()
            .map_err(|e| e.to_string())?;
        }
        Ok(config)
    })();

    let config = match config {
        Ok(config) => config,
        Err(e) => return usage_error(&e, args::HELP_SERVE),
    };
    match schemgen_server::run_blocking(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
