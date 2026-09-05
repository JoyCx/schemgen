# CLI

`schemgen2` is one binary with four commands. No server, no browser, no Node —
`convert` runs the whole pipeline in-process and writes a `.litematic`.

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
| `schemgen2 build-table <DIR> <OUT>` | Rebuild the color table from a texture pack |
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

With neither `-o` nor `-d`, the file lands beside its input. Two identically
named models in one run become `name.litematic` and `name-2.litematic` rather
than overwriting each other.

### Geometry

| Option | Default | Meaning |
|---|---|---|
| `--max-size <N>` | 128 | Longest axis of the result, in blocks |
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

### Output format and running

| Option | Default | Meaning |
|---|---|---|
| `--data-version <N>` | 4440 | `MinecraftDataVersion` stamped into the file. 4440 = 1.21.8, 4671 = 1.21.11. Also `SCHEMGEN_DATA_VERSION`. |
| `--threads <N>` | 1 | Convert N files concurrently |
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
  "data_version": 4440,
  "files": [
    {
      "ok": true,
      "input": "model.glb",
      "output": "C:\\...\\model.litematic",
      "name": "model",
      "voxels": 5293,
      "unique_blocks": 96,
      "grid": [49, 19, 22],
      "seconds": 1.6
    }
  ]
}
```

A failed file has `"ok": false` and an `"error"` string in place of the stats,
and the run's `ok` is `false`.

## palette

```bash
schemgen2 palette              # name + hex, one block per line
schemgen2 palette --json       # {"minecraft:stone": [125.0, 125.0, 125.0], ...}
```

## build-table

Rebuild `backend/data/color_table_safe.json` from a folder of block PNGs:

```bash
cd backend
cargo run --release -- build-table ../texture_pack data/color_table_safe.json
```

Each block's alpha-weighted mean color is computed in linear light, and only
curated anti-grief full blocks are kept — see [docs/pipeline.md](pipeline.md#block-palette).

## Where the palette is found

`convert` and `palette` need `data/color_table_safe.json`. It is looked for in
this order, so the binary works from any directory:

1. `$SCHEMGEN_DATA_DIR`
2. `./data`, then `./backend/data`
3. `<the binary's folder>/data`
4. two levels above the binary — how `backend/target/release/schemgen2` finds `backend/data`
5. the source tree the binary was compiled from

If none of them has the file, a 39-block built-in fallback is used and a warning
is logged; conversions still succeed but the color match is much coarser.
