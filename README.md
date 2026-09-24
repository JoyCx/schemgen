# SchemGen2 — 3D model → Minecraft schematic

Turn a `.glb` / `.gltf` model into a Minecraft schematic — Litematica
`.litematic`, WorldEdit `.schem` or structure-block `.nbt`, for any version
from 1.16.5 to 26.3 — with perceptual (CIEDE2000) block matching,
surface-only voxelization and optional removal of lighting that was baked into
the model's textures.

Three ways to use it, one pipeline:

| | Surface | Start here |
|---|---|---|
| 1 | **Web app** — drag and drop, the model and its blocks side by side in 3D, live settings | [docs/web.md](docs/web.md) |
| 2 | **CLI** — `schemgen2 convert model.glb`, scriptable, no server, no browser | [docs/cli.md](docs/cli.md) |
| 3 | **Minecraft mod** — pick a model in game, see a ghost preview, place it with Litematica | [docs/mod.md](docs/mod.md) |

Plus the [**HTTP API**](docs/api.md) the web app and the mod are clients of,
[**the pipeline**](docs/pipeline.md) all three share, the
[**design**](docs/design.md) both UIs follow, the
[**decisions**](docs/adr/README.md) behind it, the
[**changelog**](CHANGELOG.md) and the [**roadmap**](docs/roadmap.md).

```
                        ┌──────────────────────────────┐
  Web app ─── HTTP ───▶│   schemgen2 (Rust binary)    │
  Mod     ─── HTTP ───▶│   voxelize → sample → dither │──▶ .litematic
                        │   → CIEDE2000 → NBT          │    .schem  .nbt
  CLI     ── direct ──▶│   (web UI built in)          │
                        └──────────────────────────────┘
```

All three produce the same blocks for the same settings: the CLI runs the
pipeline in-process, the web app and the mod post to the server, and they
meet in the same `pipeline::run`.

## Download

Each [release](https://github.com/JoyCx/schemgen/releases) has one
self-contained binary per platform — the converter, the server and the web
UI in one file, nothing to install — plus `SHA256SUMS` and the mod.

| Platform | File | Run it |
|---|---|---|
| Windows 10/11, x64 | `schemgen2-windows-x64.exe` | Double-click it. The first time, SmartScreen may ask: *More info → Run anyway* (the binary is not signed). |
| macOS, Apple silicon | `schemgen2-macos-arm64` | `chmod +x schemgen2-macos-arm64 && xattr -d com.apple.quarantine schemgen2-macos-arm64`, then `./schemgen2-macos-arm64` (not notarized). |
| macOS, Intel | `schemgen2-macos-x64` | As above. |
| Linux, x64 (glibc 2.35+) | `schemgen2-linux-x64` | `chmod +x schemgen2-linux-x64 && ./schemgen2-linux-x64` |

Started with no arguments it serves the web app and opens it in your browser
(http://localhost:3001). The same file is the CLI: `schemgen2-linux-x64
convert model.glb`. Rename it `schemgen2` if you like — the docs do.

For the mod, take the jar for your Minecraft version from the same release;
it fetches the server by itself ([docs/mod.md](docs/mod.md)).

## Building from source

| | Version | Needed for |
|---|---|---|
| **Rust** | 1.88+ | the converter itself — CLI and server |
| **Node** | 22+ | the web UI — the CLI does not need it |
| **Java** | 21 | the mod only |

```bash
cd frontend && npm ci && npm run build   # first, so the server build embeds it
cd ../backend && cargo build --release
./target/release/schemgen2               # serves the web app and opens it
```

Loading the model, voxelizing and color sampling are all Rust. (SchemGen2 2.0
ran those in Python; builds made with `--features python-voxelizer` can still
do so with `--voxelizer python`, for one release — see
[docs/pipeline.md](docs/pipeline.md#parity-with-the-python-helper).)

## Quick start

### CLI — convert one model, no server

```bash
schemgen2 convert model.glb --max-size 128 -d ~/schematics
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
schemgen2                     # or: schemgen2 serve --open
```

The UI is inside the binary and on the same port as the API, so nothing needs
a proxy. For hot reload during development, run the two separately:

```bash
cd backend && cargo run --release -- serve   # :3001
cd frontend && npm run dev                   # :5173, proxies /api to :3001
```

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

### Minecraft mod — in game

Install Fabric Loader and Fabric API, drop the jar for your Minecraft version
(1.21.1, 1.21.4, 1.21.8 or 1.21.11) into `mods/`, and press **K** in game.
Litematica is optional: with it, the preview is a real placement and results
are placed for you. See [docs/mod.md](docs/mod.md).

## Loading the result

The output is a Litematica `.litematic` made for Minecraft 1.21.8 by default,
so it loads in 1.21.8 and every version after it. Put it in your instance's
`schematics/` folder — or point `-d` there directly — and load it with the
[Litematica](https://github.com/maruohon/litematica) mod. For another version,
pass `--target` (`schemgen2 targets` lists them, from 1.16.5 to 26.3); for
WorldEdit or a structure block, `--format schem` or `--format nbt`. See
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
  palette` prints the 181 it may choose from (fewer for older versions).

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
│   │   ├── server/          schemgen-server: API v2 + v1, jobs, events, auth,
│   │   │                    and the web UI (build.rs embeds frontend/dist)
│   │   └── cli/             schemgen2: the binary — convert, serve, …
│   ├── scripts/             2.0's Python voxelizer, for --voxelizer python and
│   │                        the parity harness (crates/core/examples/parity.rs)
│   ├── fixtures/            Small test models (tools/make_fixtures.py)
│   └── data/                color_table_safe.json — the curated palette
├── frontend/                The web app: React + TypeScript, three.js, Vite
├── schemgen-mod/            The Fabric mod: common (plain Java) + fabric (Stonecutter)
├── tools/                   Fixtures, output checks, version and release checks
└── docs/                    cli · api · web · design · mod · pipeline · versions ·
                             releasing · adr/ · roadmap
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
cd schemgen-mod && SCHEMGEN_BINARY=../backend/target/release/schemgen2 ./gradlew :common:test
```

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs all of it on
Linux and Windows, plus `cargo fmt --check`, `cargo clippy -D warnings`,
`prettier --check` and a check that every version number agrees;
[`mod.yml`](.github/workflows/mod.yml) tests the mod against a fresh server
and builds it for every Minecraft version.

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

## Releasing

Push a tag `v<version>`; [docs/releasing.md](docs/releasing.md) has the
steps and what the release workflow builds, checks and publishes.

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
