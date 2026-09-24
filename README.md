# SchemGen2 — GLB → Litematica converter

Turn a `.glb` / `.gltf` model into a Minecraft `.litematic` schematic, with
perceptual (CIEDE2000) block matching, surface-only voxelization and optional
separation of lighting that was baked into the model's textures.

There are **three ways to run it**, all on one pipeline:

| | Surface | Start here | Needs |
|---|---|---|---|
| 1 | **Fabric mod with GUI** — convert from inside Minecraft, straight into your `schematics` folder | [docs/mod.md](docs/mod.md) | The server below, running |
| 2 | **CLI** — `schemgen2 convert model.glb`, scriptable, no server | [docs/cli.md](docs/cli.md) | Rust, Python |
| 3 | **Web app** — drag and drop, 3D preview, live settings | [docs/web.md](docs/web.md) | Rust, Python, Node |

Plus the [**HTTP API**](docs/api.md) the web app and the mod are both clients
of, and [**the pipeline**](docs/pipeline.md) they all share.

```
                          ┌──────────────────────────────┐
  Fabric mod  ─── HTTP ──▶│                              │
  Web app     ─── HTTP ──▶│   schemgen2 (Rust binary)    │──▶ .litematic
  CLI         ── direct ─▶│   voxelize → sample → dither │
                          │   → CIEDE2000 → NBT          │
                          └──────────────────────────────┘
```

## Prerequisites

- **Rust** 1.80+ — the converter itself
- **Python** 3.10+ with `pip install trimesh[easy] numpy scipy Pillow` — mesh
  voxelization and color sampling
- **Node** 22+ — only to build the web UI
- **Java 21+ and Gradle** — only to build the Fabric mod

## Quick start

**Convert one model, no server:**

```bash
cd backend
cargo build --release
./target/release/schemgen2 convert ../model.glb --max-size 128 -d "%APPDATA%/.minecraft/schematics"
```

**Run the web app and the API:**

```bash
cd frontend && npm install && npm run build
cd ../backend && cargo run --release -- serve
# http://localhost:3001
```

On Windows, [`run_server.bat`](run_server.bat) does the development pair
(backend on :3001, Vite on :5173) and opens the browser.

**Use it in game:** build the mod (`cd mod && gradle build`), drop the jar in
`mods/` next to Fabric API, keep `schemgen2 serve` running, then `/schemgen`
in game. Full instructions and current build status in [docs/mod.md](docs/mod.md).

## Repository layout

```
glb2litematic/
├── backend/                 Rust — the converter, the CLI and the HTTP server
│   ├── src/
│   │   ├── main.rs          Command dispatch, palette loading, static serving
│   │   ├── cli.rs           `convert` / `palette` command line
│   │   ├── api.rs           HTTP routes and job lifecycle
│   │   ├── converter.rs     Pipeline orchestrator
│   │   ├── voxelizer.rs     Drives the Python subprocess
│   │   ├── palette.rs       CIELAB, CIEDE2000, KD-tree matching
│   │   ├── color_table.rs   Build color tables from a texture pack
│   │   ├── blocks.rs        Anti-grief whitelist + copper remapping
│   │   ├── savedir.rs       Output-folder resolve / create / deliver / reveal
│   │   ├── dithering.rs     8×8 Bayer ordered dithering
│   │   ├── litematic.rs     Raw NBT .litematic writer
│   │   └── types.rs         Shared types
│   ├── scripts/             Python: voxelize.py, sample_colors.py + its tests
│   └── data/                color_table_safe.json — the curated palette
├── frontend/                Vite React web app
├── mod/                     Fabric mod for Minecraft 1.21.11
├── docs/                    cli · api · mod · web · pipeline
└── texture_pack/            Block PNGs used by `build-table`
```

## Which surface should I use?

- **Converting while you build** — the mod. It writes into the instance's own
  `schematics` folder, so Litematica sees it immediately.
- **Many models, or automation** — the CLI. `--json` gives a parseable result
  and the exit code tells you whether anything failed.
- **Tuning a difficult model** — the web app. Its 3D preview shows the de-lit
  albedo and the highlight-rejection mask before you commit to a conversion.

All three produce the same file for the same settings. The mod and the web app
are clients of the same server; the CLI runs the pipeline in-process.

## Rewrite notes

This is a rewrite of an earlier Python/Flask converter:

| Area | Old (Python) | Now (Rust) |
|---|---|---|
| Color matching | Euclidean LAB KD-tree + Mean Shift clustering | **CIEDE2000** perceptual distance + K-means in CIELAB |
| Voxelization | Solid fill | **Hollow** — surface only |
| Color table | Mean Shift (bandwidth guessing) | K-means (k=3, deterministic) or weighted average |
| Performance | Single-threaded Python | Multi-threaded Rust + async Actix-web |
| Front end | Jinja2 templates + vanilla JS | Vite React, plus a CLI and a Fabric mod |
| NBT writer | litemapy | Raw NBT written directly |
| Dithering | 8×8 Bayer (NumPy) | 8×8 Bayer (pure Rust, no deps) |

## Testing

```bash
cd backend && cargo test            # 29 passed
cd backend/scripts && python test_sample_colors.py   # 23/23 passed
```

See [docs/pipeline.md](docs/pipeline.md#testing) for what those cover and how
the preview shader is checked.

## License

MIT
