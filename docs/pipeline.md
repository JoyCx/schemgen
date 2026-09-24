# The conversion pipeline

Everything below is shared by every front end. The CLI, the HTTP API (and so
the web app and the mod) build the same `Settings` from the same defaults and
call the same `pipeline::run`, so a given model and settings produce the same
blocks whichever door they came in through.

```
GLB/glTF
   │
   ├─ 1. voxelize + sample surface colors   backend/scripts/voxelize.py
   │                                        backend/scripts/sample_colors.py
   ├─ 2. brightness / contrast / saturation crates/core/src/pipeline.rs
   ├─ 3. ordered dithering                  crates/core/src/dither.rs
   ├─ 4. CIEDE2000 block match              crates/core/src/palette.rs
   │        → BlockGrid                     crates/core/src/grid.rs
   └─ 5. write the schematic                crates/core/src/formats/
```

(Paths under `crates/` are relative to `backend/`.) Stages 1–4 are
`pipeline::run`, which returns a `BlockGrid` — positions, a block per
position, and where the grid sits in the model. A conversion writes that grid
with one of the `formats` writers; a preview returns it as it is.

One Python subprocess does stage 1: the GLB is parsed once for both
voxelization and color sampling, and only one interpreter starts. It is
polled, not waited on, so a cancelled conversion kills it.

## 1. Voxelization

Surface-only ("hollow"): the interior is never filled, so a closed model becomes
a shell rather than a solid mass of blocks.

`max_size` sets the longest axis in blocks and the voxel pitch is derived from
it, unless an explicit `voxel_size` overrides it. A point on the model's far
face belongs to the last layer of blocks rather than opening one more, so the
longest axis is exactly `max_size` blocks.

## 2. Color sampling

Each voxel averages the mesh surface inside it, in linear light, weighted by
alpha and by the area each sample represents. Sampling is probabilistic —
roughly 12 samples per voxel-face of surface area.

## 3. Dithering

8×8 Bayer ordered dithering in pure Rust, applied in 3D across the voxel grid
before block matching. Off with `--no-dither` / the UI toggle.

## 4. Color matching

1. Colors are quantized to 8 bits per channel, and each distinct color is
   matched once — after dithering a model has far fewer distinct colors than
   voxels, so this is a fraction of the work of matching every voxel.
2. Color → CIELAB
3. KD-tree K-nearest-neighbour search (Euclidean, K=7)
4. Rank those K by **CIEDE2000** perceptual distance
5. Best match wins

CIEDE2000 rather than plain LAB distance because it accounts for
lightness-dependent chroma/hue weighting, hue rotation (which fixes the blue
region), and chroma-hue interaction.

## Block palette

The palette is `backend/data/color_table_safe.json`, compiled into the binary:
a curated **anti-grief full-block palette** (~180 blocks). Every entry is a
valid, placeable, full-cube block that cannot burn, fall, decay, be picked up
by endermen, or interact with redstone or storage. Unwaxed copper is stored as
its waxed variant so builds never oxidize.

There is intentionally no user-selectable "safety mode": a schematic that needs
scaffolding to stand up is not a useful schematic.

Rebuild it from a folder of block PNGs with
[`build-table`](cli.md#build-table); it computes each block's alpha-weighted
mean color in linear light and applies the curation filter in
`crates/core/src/blocks.rs`. A rebuilt table can be used without recompiling
through `--palette`.

## Lighting separation (de-light)

Many GLBs — most photogrammetry scans and generated meshes — ship a "base color"
texture that is really a render, with lighting already baked into the pixels. A
specular highlight on a dark surface is *additive white light*, so a shiny black
panel arrives as white texels, and no amount of averaging recovers the black.

Three passes address it, all driven by one key light direction stored in **model
space** — glTF's Y-up right-handed frame, which is also three.js's — so orbiting
the camera never changes the result.

| Control | Default | What it does |
|---|---|---|
| Light direction | (0.35, 0.85, 0.40) — azimuth 41°, elevation 58° | Where the key light is assumed to be |
| De-light | 0 (off) | Subtracts the specular lobe, then divides out the diffuse term |
| Assumed gloss | 0.5 | How sharp a highlight to assume was baked in; 0 disables the lobe entirely |
| Ambient | 0.32 | Light still reaching surfaces facing away |
| Specular gain | 1.1 | Strength of the highlight lobe |
| Highlight rejection | 0.75 | Discounts samples inside the lobe when averaging a voxel |
| Blown-voxel recovery | 1.0 | Rebuilds fully clipped voxels from the rest of their material |

Rejection reweights samples *within* a voxel, so a uniformly bright surface comes
out unchanged — white paint is never mistaken for a highlight. It wins back
voxels the highlight only partly covers; a voxel the highlight covers entirely is
what de-lighting and recovery are for.

The model lives in `backend/scripts/sample_colors.py` (`lighting_response`,
`apply_delight`, `highlight_weight`, `apply_highlight_recovery`) and is mirrored
in GLSL by `frontend/src/delightshader.js`. **The web preview only predicts the
conversion while those two agree** — change one and change the other.

## Minecraft version

Every conversion has a **target**: the Minecraft version the schematic is for,
`1.21.8` unless told otherwise (`--target`, the `target` setting, or the
server's `--target`). The target fixes the `MinecraftDataVersion` stamped into
the file and Litematica's schematic `Version` (6 before 1.21, 7 from it). See
[docs/versions.md](versions.md) for the table and what each number does.

## Reproducibility

The same model with the same settings produces the same blocks, across
separate runs and separate processes. Three things have to hold for that, and
all three are enforced:

1. **Seeded surface sampling** — `voxelize_surface()`
   (`backend/scripts/voxelize.py`) is seeded with `0x5CE2`. Face sampling is
   probabilistic, so without a fixed seed marginal voxels appear or vanish
   between otherwise identical runs and the block count drifts a little each
   conversion.
2. **Seeded color scatter** — `supersample()`
   (`backend/scripts/sample_colors.py`), same seed, same reason.
3. **Stable palette order** — `Palette::from_table`
   (`crates/core/src/palette.rs`) walks the color table in sorted block-name
   order rather than the `HashMap`'s own, which Rust randomizes per process.
   Entry indices break ties between equally distant blocks, so an unstable
   order changed a handful of blocks on every restart even though both
   samplers were seeded. `entries_are_ordered_by_block_name` locks this down.

The *file* also records when it was made (`TimeCreated` / `TimeModified`, as
Litematica shows them). Set `SOURCE_DATE_EPOCH` (seconds, the
reproducible-builds convention) to stamp a fixed time instead, and the same
model, settings and name produce a byte-identical `.litematic`. The schematic
*name* is part of the file too, so the same model under two names correctly
yields two different files.

## Testing

```bash
cd backend && cargo test
#   core:   CIEDE2000, KD-tree pruning and the match cache against linear
#           scans, settings ranges, the schema, targets, the NBT writer and
#           reader, the .litematic bit packing read back cell by cell, the
#           thumbnail renderer, anti-grief filtering
#   server: every v2 and v1 route, events, cancelling, the token and
#           host/origin checks, the TTL sweep
#   cli:    argument parsing, defaults, clamps and usage errors
```

```bash
cd backend/scripts && python test_sample_colors.py
# area averaging, linear light, alpha, lighting separation, exact max_size
```

The server tests that run real conversions need the Python voxelizer and skip,
saying so, when no interpreter with `trimesh` is found (`SCHEMGEN_PYTHON`
picks one). The de-light preview shader is checked by compiling it against a
real WebGL context: run the dev server and open `/shader-check.html`.

## Performance notes

- Color adjustment, dithering and block matching are Rayon-parallel, and
  matching does each distinct quantized color once.
- The voxelizer's memory use is bounded by `ram_limit` (GB, default 4).
- Schematics are streamed through the gzip encoder as they are written rather
  than built in memory first.
- The server runs at most `--max-jobs` conversions at once (default: the CPU
  count); a batch request further limits its own files with `threads`.
