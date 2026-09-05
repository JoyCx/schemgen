//! SchemGen2 — GLB → Litematica converter.
//!
//! One binary, two front doors onto the same pipeline
//! (voxelize → color sample → dither → CIEDE2000 block match → litematic):
//!
//! * `schemgen2 serve` — HTTP API + the Vite React web UI
//! * `schemgen2 convert` — headless CLI conversion
//! * `schemgen2 palette` / `build-table` — palette inspection and rebuilding
//!
//! With no command it serves, so existing shortcuts keep working.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use actix_web::{web, App, HttpServer};
use actix_web::middleware;

mod api;
mod blocks;
mod cli;
mod color_table;
mod converter;
mod dithering;
mod litematic;
mod palette;
mod savedir;
mod types;
mod voxelizer;

use palette::Palette;
use types::ColorTable;

/// Load a color table from JSON, or return a minimal fallback.
fn load_color_table(path: &Path) -> ColorTable {
    match std::fs::read_to_string(path) {
        Ok(json) => {
            match serde_json::from_str::<ColorTable>(&json) {
                Ok(table) => {
                    log::info!("Loaded color table: {} blocks from {}", table.len(), path.display());
                    table
                }
                Err(e) => {
                    log::error!("Failed to parse color table {}: {e}", path.display());
                    fallback_table()
                }
            }
        }
        Err(_) => {
            log::warn!("No color table at {} — using built-in fallback", path.display());
            fallback_table()
        }
    }
}

fn fallback_table() -> ColorTable {
    let mut table = HashMap::new();
    // Minimal fallback with concrete/wool colors (R, G, B) → LAB approximate
    let blocks: &[(&str, [f32; 3])] = &[
        ("minecraft:white_concrete", [255.0, 255.0, 255.0]),
        ("minecraft:orange_concrete", [240.0, 118.0, 19.0]),
        ("minecraft:magenta_concrete", [189.0, 68.0, 179.0]),
        ("minecraft:light_blue_concrete", [58.0, 175.0, 217.0]),
        ("minecraft:yellow_concrete", [248.0, 198.0, 39.0]),
        ("minecraft:lime_concrete", [112.0, 185.0, 25.0]),
        ("minecraft:pink_concrete", [237.0, 141.0, 172.0]),
        ("minecraft:gray_concrete", [55.0, 58.0, 62.0]),
        ("minecraft:light_gray_concrete", [125.0, 125.0, 115.0]),
        ("minecraft:cyan_concrete", [21.0, 137.0, 145.0]),
        ("minecraft:purple_concrete", [127.0, 62.0, 182.0]),
        ("minecraft:blue_concrete", [45.0, 47.0, 143.0]),
        ("minecraft:brown_concrete", [96.0, 60.0, 32.0]),
        ("minecraft:green_concrete", [73.0, 91.0, 36.0]),
        ("minecraft:red_concrete", [142.0, 33.0, 33.0]),
        ("minecraft:black_concrete", [8.0, 10.0, 15.0]),
        ("minecraft:stone", [125.0, 125.0, 125.0]),
        ("minecraft:rooted_dirt", [144.0, 103.0, 76.0]),
        ("minecraft:crimson_planks", [101.0, 48.0, 70.0]),
        ("minecraft:warped_planks", [43.0, 104.0, 99.0]),
        ("minecraft:brown_terracotta", [77.0, 51.0, 35.0]),
        ("minecraft:sandstone", [216.0, 201.0, 146.0]),
        ("minecraft:bricks", [153.0, 83.0, 57.0]),
        ("minecraft:netherrack", [111.0, 54.0, 52.0]),
        ("minecraft:obsidian", [15.0, 10.0, 25.0]),
        ("minecraft:snow_block", [248.0, 250.0, 252.0]),
        ("minecraft:smooth_sandstone", [223.0, 214.0, 170.0]),
        ("minecraft:andesite", [136.0, 136.0, 136.0]),
        ("minecraft:terracotta", [152.0, 94.0, 67.0]),
        ("minecraft:iron_block", [216.0, 216.0, 216.0]),
        ("minecraft:gold_block", [249.0, 197.0, 40.0]),
        ("minecraft:diamond_block", [98.0, 219.0, 209.0]),
        ("minecraft:emerald_block", [72.0, 204.0, 80.0]),
        ("minecraft:quartz_block", [237.0, 233.0, 226.0]),
        ("minecraft:black_terracotta", [37.0, 23.0, 16.0]),
        ("minecraft:netherite_block", [60.0, 46.0, 46.0]),
        ("minecraft:deepslate", [80.0, 80.0, 85.0]),
        ("minecraft:tuff", [110.0, 107.0, 102.0]),
        ("minecraft:calcite", [221.0, 220.0, 215.0]),
    ];

    for (name, rgb) in blocks {
        let lab = palette::rgb_to_lab(rgb[0], rgb[1], rgb[2]);
        table.insert(name.to_string(), vec![
            crate::types::BlockColorEntry {
                lab: [lab.l, lab.a, lab.b],
                rgb: *rgb,
                weight: 1.0,
            }
        ]);
    }
    log::info!("Using fallback table: {} blocks", table.len());
    table
}

/// Where `data/color_table_safe.json` lives, tried in order so the CLI works
/// from any directory — not just from `backend/`.
///
/// `SCHEMGEN_DATA_DIR` wins, then the current directory, then next to the
/// executable, then the two levels up that `target/release/schemgen2` needs,
/// and finally the source tree the binary was compiled from.
fn resolve_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SCHEMGEN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_deref().and_then(Path::parent);
    let candidates = [
        std::env::current_dir().ok().map(|d| d.join("data")),
        std::env::current_dir().ok().map(|d| d.join("backend/data")),
        exe_dir.map(|d| d.join("data")),
        exe_dir.and_then(|d| d.parent()).and_then(|d| d.parent()).map(|d| d.join("data")),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data")),
    ];
    candidates.into_iter().flatten()
        .find(|dir| dir.join("color_table_safe.json").is_file())
        .unwrap_or_else(|| PathBuf::from("data"))
}

/// Load the curated full-block palette. There is intentionally no
/// user-selectable grief/safety mode: every candidate must be a stable,
/// full-cube block that can be placed in mid-air without falling.
fn load_palette(data_dir: &Path) -> (Palette, usize) {
    let color_table = filter_full_blocks(load_color_table(&data_dir.join("color_table_safe.json")));
    let blocks = color_table.len();
    let palette = Palette::from_table(&color_table).unwrap_or_else(|| {
        log::warn!("Empty palette after loading — using fallback");
        Palette::from_table(&filter_full_blocks(fallback_table())).unwrap()
    });
    (palette, blocks)
}

/// CLI commands want a quiet log; `serve` wants the request/pipeline log.
fn init_logging(default_level: &str) {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(default_level)).try_init();
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match cli::parse(&argv) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("error: {e}\n\n{}", cli::HELP);
            std::process::exit(2);
        }
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
    crate::litematic::apply_env_data_version();

    match command {
        "help" => {
            print!("{}", match args.positional.first().map(String::as_str) {
                Some("convert") => cli::HELP_CONVERT,
                Some("serve") => cli::HELP_SERVE,
                _ => cli::HELP,
            });
            Ok(())
        }

        "version" => {
            println!("schemgen2 {} (litematic schematic v6, data version {})",
                env!("CARGO_PKG_VERSION"), crate::litematic::data_version());
            Ok(())
        }

        "convert" => {
            if args.has("help") {
                print!("{}", cli::HELP_CONVERT);
                return Ok(());
            }
            let (palette, _) = load_palette(&resolve_data_dir());
            match cli::convert(&args, Arc::new(palette)).await {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("error: {e}\n\nRun `schemgen2 help convert` for the options.");
                    std::process::exit(2);
                }
            }
        }

        "palette" => {
            let (palette, _) = load_palette(&resolve_data_dir());
            cli::palette(&args, &palette);
            Ok(())
        }

        // `schemgen2 build-table [texture_dir] [output.json]` regenerates the
        // curated color table and exits. Defaults match the repo layout.
        "build-table" => {
            init_logging("info");
            let pack = args.positional.first().map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("../texture_pack"));
            let out = args.positional.get(1).map(PathBuf::from)
                .unwrap_or_else(|| resolve_data_dir().join("color_table_safe.json"));
            let table = color_table::build_table(&pack, &out, true);
            log::info!("build-table done: {} blocks → {}", table.len(), out.display());
            Ok(())
        }

        "" | "serve" => serve(&args).await,

        other => {
            eprintln!("error: unknown command '{other}'\n\n{}", cli::HELP);
            std::process::exit(2);
        }
    }
}

/// Start the HTTP API, plus the built web UI when `frontend/dist` exists.
async fn serve(args: &cli::ParsedArgs) -> std::io::Result<()> {
    init_logging("info");

    if args.has("help") {
        print!("{}", cli::HELP_SERVE);
        return Ok(());
    }

    let port = args.get("port").map(str::to_string)
        .or_else(|| std::env::var("PORT").ok())
        .unwrap_or_else(|| "3001".to_string());

    // Paths — everything hangs off the resolved data folder, so `serve` also
    // works when the binary is started from outside `backend/`.
    let data_dir = resolve_data_dir();
    let backend_dir = data_dir.parent().map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let upload_dir = backend_dir.join("uploads");
    let output_dir = backend_dir.join("outputs");

    fs_create(&upload_dir);
    fs_create(&output_dir);

    let (palette, block_count) = load_palette(&data_dir);

    log::info!("Palette ready: {} entries for {block_count} unique blocks", palette.len());

    let state = Arc::new(crate::api::AppState {
        jobs: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        palette,
        block_count,
        upload_dir,
        output_dir,
    });

    // Frontend dist path (for production serving)
    let frontend_dist = backend_dir.join("../frontend/dist");
    let serve_frontend = frontend_dist.exists();

    if serve_frontend {
        log::info!("Serving frontend from {}", frontend_dist.display());
    } else {
        log::warn!("No frontend dist at {} — run 'cd frontend && npm run build'", frontend_dist.display());
    }

    let bind = format!("0.0.0.0:{port}");
    log::info!("Starting SchemGen2 server on http://localhost:{port}");

    let server = HttpServer::new(move || {
        let mut app = App::new()
            // Raise the default 256KB payload limit — GLB uploads can be hundreds of MB
            .app_data(web::PayloadConfig::new(4usize * 1024 * 1024 * 1024))
            .app_data(web::Data::new(state.clone()))
            .configure(crate::api::configure);

        // Serve frontend SPA if built
        if serve_frontend {
            app = app.service(
                actix_files::Files::new("/", frontend_dist.clone())
                    .index_file("index.html")
                    .prefer_utf8(true),
            );
        }

        app
    });

    server.bind(&bind)?.run().await
}

/// Keep only curated anti-grief full blocks, renaming where needed (unwaxed
/// copper → waxed). Entries whose keys collapse to the same block are merged.
fn filter_full_blocks(table: ColorTable) -> ColorTable {
    let mut out: ColorTable = HashMap::new();
    for (name, entries) in table {
        if let Some(safe_name) = blocks::sanitize(&name) {
            out.entry(safe_name).or_default().extend(entries);
        }
    }
    out
}
fn fs_create(path: &Path) {
    if let Err(e) = std::fs::create_dir_all(path) {
        log::warn!("Warning: could not create {}: {e}", path.display());
    }
}
