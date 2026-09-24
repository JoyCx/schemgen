# CLI

`schemgen2` is one binary. No server, no browser, no Node — `convert` runs the
whole pipeline in-process and writes a `.litematic`.

```bash
cd backend
cargo build --release
./target/release/schemgen2 help
```

| Command | What it does |
|---|---|
| `schemgen2 serve` | HTTP API + web UI ([docs/api.md](api.md), [docs/web.md](web.md)) |
| `schemgen2 convert <FILE>...` | Convert models to `.litematic` headlessly |
| `schemgen2 palette` | Print the blocks a conversion may choose from |
| `schemgen2 targets` | Print the Minecraft versions a schematic can target |
| `schemgen2 schema` | Print the settings schema (what `GET /api/schema` serves) as JSON |
| `schemgen2 build-table <DIR> [OUT]` | Rebuild the color table from a texture pack |
| `schemgen2 help [COMMAND]`, `schemgen2 version` | Usage and version |

With no command at all it serves, so old shortcuts that ran the bare binary
still start the server.

## convert

```bash
# next to the input file
schemgen2 convert model.glb

# straight into a schematics folder, at a chosen size
schemgen2 convert model.glb --max-size 192 -d "%APPDATA%/.minecraft/schematics"

# a whole folder, four at a time
schemgen2 convert models/*.glb -d ~/schematics --threads 4
```

### Output

| Option | Meaning |
|---|---|
| `-o, --output <FILE>` | Exact path. Single input only. |
| `-d, --out-dir <DIR>` | Folder to write into. `~` and `%APPDATA%` expand; the folder is created if missing. |
| `-n, --name <NAME>` | Schematic name. Defaults to the input's file stem. Single input only. |
| `-f, --format <FORMAT>` | `litematic` (default), `schem` (Sponge v2 — WorldEdit, FAWE), `schem-v3` (WorldEdit 7.3+) or `nbt` (structure blocks, `/place template`). The extension follows the format. |

With neither `-o` nor `-d`, the file lands beside its input. Two identically
named models in one run become `name.litematic` and `name-2.litematic` rather
than overwriting each other. Files are written under a temporary name and
renamed into place, so a folder Litematica is watching never shows half a
schematic.

### Target

| Option | Default | Meaning |
|---|---|---|
| `-t, --target <VERSION>` | `1.21.8` | Minecraft version to write for — see `schemgen2 targets` |
| `--data-version <N>` | | Stamp this exact `MinecraftDataVersion` instead. Also `SCHEMGEN_DATA_VERSION`. |

The target decides the `MinecraftDataVersion` stamped into the file and
Litematica's schematic version (6 before 1.21, 7 from 1.21), and limits the
palette to blocks that exist in that version — see
[docs/versions.md](versions.md).

### Geometry

| Option | Default | Meaning |
|---|---|---|
| `--max-size <N>` | 128 | Longest axis of the result, in blocks (at most 2048) |
| `--voxel-size <F>` | derived | Explicit voxel pitch in model units; overrides `--max-size` |
| `--ram-limit <GB>` | 4 | Memory budget for the color sampler |

### Color

| Option | Default | Meaning |
|---|---|---|
| `--no-dither` | off | Skip 8×8 Bayer ordered dithering |
| `--no-color` | off | One block everywhere instead of matching colors |
| `--block <ID>` | `white` | Block for `--no-color`: `white`, `netherrack`, or any full ID |
| `--brightness <F>` | 0 | −1…1 |
| `--contrast <F>` | 1 | 0…3 |
| `--saturation <F>` | 1 | 0…3 |

### Lighting separation

Same model and same defaults as the web UI — see [docs/pipeline.md](pipeline.md)
for what each one does.

| Option | Default |
|---|---|
| `--light-dir <x,y,z>` | `0.35,0.85,0.4` (model space) |
| `--light-ambient <F>` | 0.32 |
| `--light-gloss <F>` | 0.5 |
| `--specular <F>` | 1.1 |
| `--highlight-rejection <F>` | 0.75 |
| `--highlight-recovery <F>` | 1 |
| `--delight <F>` | 0 (off) |

### Running

| Option | Default | Meaning |
|---|---|---|
| `--threads <N>` | 1 | Convert N files concurrently |
| `--voxelizer <NAME>` | `rust` | `python` runs SchemGen2 2.0's Python helper instead — only in builds made with `--features python-voxelizer`, for one release. Also `SCHEMGEN_VOXELIZER`. |
| `--python <PATH>` | `python3` (`python` on Windows) | Interpreter for `--voxelizer python`; it needs `backend/scripts/requirements.txt`. Also `SCHEMGEN_PYTHON`. Ignored, with a warning, by builds without the Python voxelizer. |
| `--palette <FILE>` | built in | Color table to use instead of the built-in one. Also `SCHEMGEN_PALETTE`. |
| `-q, --quiet` | | No progress lines |
| `-j, --json` | | Machine-readable result on stdout |

## Streams and exit codes

Progress and summaries go to **stderr**; the finished paths go to **stdout**,
one per line, so the command composes:

```bash
schemgen2 convert *.glb -d ./out --quiet | wc -l
```

| Code | Meaning |
|---|---|
| 0 | Every file converted |
| 1 | At least one conversion failed |
| 2 | Bad usage — unknown option, missing file, malformed value |

An unknown option is an error rather than something silently ignored, so
`--max-sixe 64` stops instead of quietly converting at the default size.

### `--json`

```json
{
  "ok": true,
  "target": "1.21.8",
  "data_version": 4440,
  "files": [
    {
      "ok": true,
      "input": "model.glb",
      "output": "C:\\...\\model.litematic",
      "name": "model",
      "voxels": 5293,
      "unique_blocks": 96,
      "grid": [48, 19, 22],
      "seconds": 1.6
    }
  ]
}
```

A failed file has `"ok": false` and an `"error"` string in place of the stats,
and the run's `ok` is `false`. `grid` is the schematic's size, x × y × z.

## serve

```bash
schemgen2 serve                         # http://localhost:3001, API + web UI
schemgen2 serve --port 0 --token-file ~/.schemgen/token --exit-with-stdin
```

| Option | Default | Meaning |
|---|---|---|
| `--port <N>` | 3001 (or `PORT`) | Port to bind; `0` lets the OS pick one |
| `--host <ADDR>` | `127.0.0.1` | Address to bind. Anything else exposes the server — pair it with `--token` |
| `--allow-host <NAMES>` | | Extra host names requests may use, comma-separated |
| `--token <T>` | none (or `SCHEMGEN_TOKEN`) | Require `Authorization: Bearer <T>` on every `/api` call but `/api/health` |
| `--token-file <PATH>` | | Read the token from PATH, or write a new random one there if it is missing |
| `--work-dir <DIR>` | user cache folder | Where uploads and outputs are kept |
| `--ui-dir <DIR>` | `frontend/dist` if found (or `SCHEMGEN_UI_DIR`) | Built web UI to serve at `/` |
| `--textures <PATH>` | the newest client jar a launcher keeps | Block textures for the web preview: a Minecraft client jar, a resource pack or a folder of PNGs. Also `SCHEMGEN_TEXTURES`. |
| `--job-ttl <HOURS>` | 24 | Forget finished jobs and delete their files after this long; 0 never |
| `--target <VERSION>` | `1.21.8` | Default target for requests that do not name one |
| `--max-jobs <N>` | CPU count | Conversions running at once |
| `--exit-with-stdin` | | Stop when standard input closes |
| `--pid-file <PATH>` | | Write the process id there while running |
| `--voxelizer`, `--python`, `--palette` | | As for `convert` |

Once it accepts connections, `serve` prints exactly one line to stdout —
`listening http://127.0.0.1:<port>` — so a launcher using `--port 0` learns the
port. Logs go to stderr. See [docs/api.md](api.md#authentication-and-exposure)
for what the token and host checks protect against.

## targets

```bash
schemgen2 targets          # version, data version, Litematica version
schemgen2 targets --json
```

## schema

```bash
schemgen2 schema           # every setting: type, range, default, group, label, help
```

## palette

```bash
schemgen2 palette                   # name + hex, one block per line
schemgen2 palette --target 1.16.5   # only what exists in 1.16.5
schemgen2 palette --json            # {"stone": [125.0, 125.0, 125.0], ...}
```

## build-table

Rebuild `backend/data/color_table_safe.json` from a folder of block PNGs:

```bash
cd backend
cargo run --release -- build-table ../texture_pack data/color_table_safe.json
```

`DIR` is the `assets/minecraft/textures/block` folder of an extracted client
jar or resource pack; `OUT` defaults to `color_table_safe.json` in the current
folder. Each block's alpha-weighted mean color is computed in linear light,
and only curated anti-grief full blocks are kept — see
[docs/pipeline.md](pipeline.md#block-palette). The table in the binary changes
when it is rebuilt; to use a new table without rebuilding, pass it with
`--palette`.

## Where the palette comes from

The curated color table (`backend/data/color_table_safe.json`) is compiled into
the binary, so it needs no data folder beside it. To use another table — one
rebuilt from a resource pack, say — pass `--palette <file>`, or set
`SCHEMGEN_PALETTE`, or point `SCHEMGEN_DATA_DIR` at a folder holding a
`color_table_safe.json`. Whatever the source, only curated anti-grief blocks
are kept.
