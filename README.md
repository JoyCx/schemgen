# SchemGen2 — 3D model → Minecraft schematic

Turn a `.glb` / `.gltf` model into a Minecraft `.litematic` schematic, with
perceptual (CIEDE2000) block matching, surface-only voxelization and optional
removal of lighting that was baked into the model's textures.

Two ways to run it, one pipeline:

| | Surface | Start here | Needs |
|---|---|---|---|
| 1 | **CLI** — `schemgen2 convert model.glb`, scriptable, no server, no browser | [docs/cli.md](docs/cli.md) | Rust |
| 2 | **Web app** — drag and drop, 3D preview, live settings | [docs/web.md](docs/web.md) | Rust, Node |

Plus the [**HTTP API**](docs/api.md) the web app is a client of, the
[**roadmap**](docs/roadmap.md) for the UI redesign, the in-game mod and
multi-version support, and
[**the pipeline**](docs/pipeline.md) both share.

```
                        ┌──────────────────────────────┐
  Web app ─── HTTP ───▶│   schemgen2 (Rust binary)    │
                        │   voxelize → sample → dither │──▶ .litematic
  CLI     ── direct ──▶│   → CIEDE2000 → NBT          │
                        └──────────────────────────────┘
```

Both produce the same blocks for the same settings: the CLI runs the pipeline
in-process, the web app posts to the server, and they meet in the same
`pipeline::run`.

## Prerequisites

| | Version | Needed for |
|---|---|---|
| **Rust** | 1.88+ | the converter itself — CLI and server |
| **Node** | 22+ | building the web UI only — the CLI does not need it |

The binary is self-contained: loading the model, voxelizing and color sampling
are all Rust. (SchemGen2 2.0 ran those in Python; builds made with
`--features python-voxelizer` can still do so with `--voxelizer python`, for
one release — see [docs/pipeline.md](docs/pipeline.md#parity-with-the-python-helper).)

## Quick start

### CLI — convert one model, no server

```bash
# 1. Build
cd backend
cargo build --release

# 2. Convert
./target/release/schemgen2 convert ../model.glb --max-size 128 -d ~/schematics
```

`-d` is the folder to write into; `~` and `%APPDATA%` expand and the folder is
created if it is missing. With no `-d` or `-o` the `.litematic` lands next to
its input. Full option list — sizing, dithering, color grading, lighting
separation, batch conversion, `--json` output — in [docs/cli.md](docs/cli.md).

```bash
schemgen2 convert models/*.glb -d ~/schematics --threads 4   # a whole folder
schemgen2 palette                                            # what blocks it may use
schemgen2 help convert                                       # every option
```

Progress goes to stderr and finished paths to stdout, one per line, so the
command composes. Exit code is 0 when every file converted, 1 when any failed,
2 on bad usage.

### Web app — drag, drop, preview, convert

```bash
cd frontend && npm install && npm run build
cd ../backend && cargo run --release -- serve
# open http://localhost:3001
```

`serve` mounts `frontend/dist` at `/` when it exists, so the UI and the API are
one origin on one port and nothing needs a proxy. For hot reload during
development, run the two separately:

```bash
cd backend && cargo run --release -- serve   # :3001
cd frontend && npm run dev                   # :5173, proxies /api to :3001
```

On Windows, [`run_server.bat`](run_server.bat) does that development pair for
you: it builds the backend if needed, installs npm dependencies if needed,
starts both and opens the browser.

The web app adds what a CLI cannot: the model and its blocks side by side in
one 3D view (with a draggable split between them), a live low-resolution preview
that follows every setting, the material list with counts in stacks and
shulker boxes, a key-light handle to drag, a de-lit view of what the sampler
reads, one queue for one model or many, and a folder picker that finds the
`schematics` folders of your CurseForge / Prism / MultiMC / Modrinth instances.
See [docs/web.md](docs/web.md); [docs/design.md](docs/design.md) is the screen,
section by section.

> The server is meant for the machine it runs on: it binds `127.0.0.1`, answers
> only requests addressed to `localhost`, and refuses browser requests from
> other sites. `--token` additionally requires a bearer token on every API
> call. Exposing it with `--host` is possible but not what it is for — see
> [docs/api.md](docs/api.md#authentication-and-exposure).

## Loading the result

The output is a Litematica `.litematic` made for Minecraft 1.21.8 by default,
so it loads in 1.21.8 and every version after it. Put it in your instance's
`schematics/` folder — or point `-d` there directly — and load it with the
[Litematica](https://github.com/maruohon/litematica) mod. For another version,
pass `--target` (`schemgen2 targets` lists them, from 1.16.5 to 26.3); see
[docs/versions.md](docs/versions.md).

## How it works

`voxelize → color sample → dither → CIEDE2000 match → NBT`, described in full
in [docs/pipeline.md](docs/pipeline.md):

- **Voxelization is hollow.** Only the surface becomes blocks, so a solid model
  does not turn into a solid cube of stone.
- **Color matching is perceptual.** Block colors are compared in CIELAB with
  CIEDE2000 rather than Euclidean RGB or LAB distance, through a KD-tree.
- **Lighting can be separated.** Models with lighting baked into their textures
  match badly, so an optional de-light pass estimates and removes the diffuse
  and specular contribution before sampling.
- **Dithering is 8×8 Bayer ordered**, which keeps large flat areas from banding
  into a single block.
- **The palette is anti-grief.** Only curated full solid blocks are eligible —
  no falling blocks, no gravity, no blocks that need support. `schemgen2
  palette` prints the 182 it may choose from.

## Repository layout

```
schemgen2/
├── backend/                 Rust workspace — the converter, the CLI and the server
│   ├── crates/
│   │   ├── core/            schemgen-core: no I/O policy, no HTTP
│   │   │   ├── settings.rs  Every setting, its default and range
│   │   │   ├── schema.rs    The settings schema UIs render from
│   │   │   ├── pipeline.rs  voxelize → adjust → dither → match → BlockGrid
│   │   │   ├── mesh.rs      glTF loading: scene graph, transforms, materials
│   │   │   ├── voxel.rs     Surface voxelization
│   │   │   ├── sample.rs    Per-voxel color sampling and the lighting model
│   │   │   ├── rng.rs       NumPy's random stream, bit for bit
│   │   │   ├── voxelizer/   Runs the above, or the Python helper (feature-gated)
│   │   │   ├── palette.rs   CIELAB, CIEDE2000, KD-tree matching
│   │   │   ├── targets.rs   Minecraft versions and their data versions
│   │   │   ├── formats/     NBT writer/reader, .litematic
│   │   │   ├── thumbnail.rs Isometric preview image
│   │   │   └── …            dithering, color tables, anti-grief list
│   │   ├── server/          schemgen-server: API v2 + v1, jobs, events, auth
│   │   └── cli/             schemgen2: the binary — convert, serve, …
│   ├── scripts/             2.0's Python voxelizer, for --voxelizer python and
│   │                        the parity harness (crates/core/examples/parity.rs)
│   ├── fixtures/            Small test models (tools/make_fixtures.py)
│   └── data/                color_table_safe.json — the curated palette
├── frontend/                The web app: React + TypeScript, three.js, Vite
└── docs/                    cli · api · web · design · pipeline · versions · roadmap
```

## Rebuilding the color table

`backend/data/color_table_safe.json` ships ready to use, so this is only needed
to retarget a different Minecraft version or a custom resource pack. It is not
possible to ship the source textures — they are Mojang's — so point
`build-table` at your own extracted block textures:

```bash
cd backend
cargo run --release -- build-table /path/to/block/textures data/color_table_safe.json
```

That is the `assets/minecraft/textures/block` folder of an extracted client jar
or resource pack. Each block's alpha-weighted mean color is computed in linear
light and only the curated anti-grief blocks are kept.

## Testing

```bash
cd backend && cargo test                              # core, server and CLI
cd frontend && npm run lint && npm run typecheck && npm test && npm run build
cd frontend && npm run test:e2e                       # the built UI against the real server
```

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs all of it on
Linux and Windows, plus `cargo fmt --check`, `cargo clippy -D warnings` and
`prettier --check`.

The Rust tests cover the glTF loader, the voxelizer and the color sampler, the
CIEDE2000 implementation against reference values, the KD-tree's pruning and
the match cache against linear scans, the NBT writer and reader, the settings
schema, and every API route with real conversions. CI also converts the
fixtures for every Minecraft version and format and reads them back with
independent readers, and holds the Rust voxelizer to 2.0's Python one with a
parity harness ([docs/pipeline.md](docs/pipeline.md#parity-with-the-python-helper)).
The web preview's GLSL mirror of the lighting model has no assertion to make, so it is
checked by compiling it against a real WebGL context — run the dev server and
open `/shader-check.html`.

## Notes

This is a Rust rewrite of an earlier Python/Flask converter. The changes that
mattered: CIEDE2000 instead of Euclidean LAB, K-means instead of Mean Shift for
the color table, hollow instead of solid voxelization, a raw NBT writer instead
of `litemapy`, and multi-threaded conversion instead of single-threaded Python.
The voxelizer and color sampler followed in 2.1, ported from Python to Rust.

## License

**Personal, non-commercial use only** — see [LICENSE](LICENSE).

Source-available, not open source. You may run it, modify it for yourself, and
share unmodified copies for free; academic and research use is fine. Commercial
use of any kind, redistributing modified versions, and charging for it are not
permitted without written permission. Schematics you generate are yours.

Not affiliated with Mojang or Microsoft. Minecraft is a trademark of Mojang
Synergies AB.
