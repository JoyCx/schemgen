# The conversion pipeline

Everything below is shared by both front ends. The CLI and the HTTP API end up
in the same `convert()` with the same defaults, so a given model and settings
produce the same file whichever door you came in through.

```
GLB/glTF
   │
   ├─ 1. voxelize + sample surface colors   backend/scripts/voxelize.py
   │                                        backend/scripts/sample_colors.py
   ├─ 2. brightness / contrast / saturation backend/src/converter.rs
   ├─ 3. ordered dithering                  backend/src/dithering.rs
   ├─ 4. CIEDE2000 block match              backend/src/palette.rs
   └─ 5. write NBT                          backend/src/litematic.rs
                                            → .litematic
```

One Python subprocess does stages 1 and 2's input: the GLB is parsed once for
both voxelization and color sampling, and only one interpreter starts.

## 1. Voxelization

Surface-only ("hollow"): the interior is never filled, so a closed model becomes
a shell rather than a solid mass of blocks. `--hollow` / `--solid` are accepted
by the script for compatibility but ignored.

`max_size` sets the longest axis in blocks and the voxel pitch is derived from
it, unless an explicit `voxel_size` overrides it.

## 2. Color sampling

Each voxel averages the mesh surface inside it, in linear light, weighted by
alpha and by the area each sample represents. Sampling is probabilistic —
roughly 12 samples per voxel-face of surface area.

## 3. Dithering

8×8 Bayer ordered dithering in pure Rust, applied in 3D across the voxel grid
before block matching. Off with `--no-dither` / the UI toggle.

## 4. Color matching

1. Query color → CIELAB
2. KD-tree K-nearest-neighbour search (Euclidean, K=7)
3. Rank those K by **CIEDE2000** perceptual distance
4. Best match wins

CIEDE2000 rather than plain LAB distance because it accounts for
lightness-dependent chroma/hue weighting, hue rotation (which fixes the blue
region), and chroma-hue interaction.

## Block palette

The backend loads `backend/data/color_table_safe.json`: a curated **anti-grief
full-block palette** (~180 blocks). Every entry is a valid, placeable, full-cube
block that cannot burn, fall, decay, be picked up by endermen, or interact with
redstone or storage. Unwaxed copper is stored as its waxed variant so builds
never oxidize.

There is intentionally no user-selectable "safety mode": a schematic that needs
scaffolding to stand up is not a useful schematic.

If the file is missing, a 39-block built-in fallback is used and a warning is
logged.

Rebuild it from a folder of block PNGs with
[`build-table`](cli.md#build-table); it computes each block's alpha-weighted
mean color in linear light and applies the curation filter in
`backend/src/blocks.rs`.

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
| Light azimuth / elevation | 41° / 58° | Where the key light is assumed to be |
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

Schematics are written as Litematica schematic **version 6** with
`MinecraftDataVersion` **4440 (Minecraft 1.21.8)** by default.

| Minecraft | Data version |
|---|---|
| 1.21.8 | 4440 (default) |
| 1.21.11 | 4671 |

Override per run with `--data-version`, or globally with the
`SCHEMGEN_DATA_VERSION` environment variable; the constant lives in
`backend/src/litematic.rs`.

The stamp must stay **at or above** the newest block in the palette —
`color_table_safe.json` ships `resin_block`, `resin_bricks` and
`chiseled_resin_bricks`, added in 1.21.4 — because declaring an older version
makes Minecraft's DataFixerUpper try to upgrade block names that did not exist
yet. Litematica reads schematics stamped *below* the running game, so the
default loads fine in 1.21.8 and everything after it.

To read the value out of a schematic Litematica itself saved:

```bash
python -c "import gzip,struct;d=gzip.open('some.litematic','rb').read();i=d.find(b'MinecraftDataVersion');print(struct.unpack('>i',d[i+20:i+24])[0])"
```

## Reproducibility

The same model with the same settings produces a **byte-identical**
`.litematic`, across separate runs and separate processes. Three things have to
hold for that, and all three are enforced:

1. **Seeded surface sampling** — `voxelize_surface()`
   (`backend/scripts/voxelize.py`) is seeded with `0x5CE2`. Face sampling is
   probabilistic, so without a fixed seed marginal voxels appear or vanish
   between otherwise identical runs and the block count drifts a little each
   conversion.
2. **Seeded color scatter** — `supersample()`
   (`backend/scripts/sample_colors.py`), same seed, same reason.
3. **Stable palette order** — `Palette::from_table`
   (`backend/src/palette.rs`) walks the color table in sorted block-name order
   rather than the `HashMap`'s own, which Rust randomizes per process. Entry
   indices break ties between equally distant blocks, so an unstable order
   changed a handful of blocks on every restart even though both samplers were
   seeded. `entries_are_ordered_by_block_name` locks this down.

Note that the schematic *name* is part of the file's metadata, so converting
the same model under two names correctly yields two different files.

## Testing

```bash
cd backend && cargo test
# 29 passed; 0 failed
#   CIEDE2000 identical / reference / wide gap, palette matching + KD-tree pruning
#   Litematic empty input + single block write
#   Anti-grief block filtering, copper remapping
#   Output-folder sanitizing, traversal rejection, batch de-duplication
#   Launcher instance scanning for schematics folders
#   CLI argument parsing, option defaults, clamps and usage errors
#   Deterministic palette entry order
```

```bash
cd backend/scripts && python test_sample_colors.py
# 23/23 passed — area averaging, linear light, alpha, lighting separation
```

The de-light preview shader is checked by compiling it against a real WebGL
context: run the dev server and open `/shader-check.html`.

## Performance notes

- Block matching and the color adjustments are Rayon-parallel.
- The voxelizer's memory use is bounded by `--ram-limit` (GB, default 4).
- Batch conversions run `threads` at a time, each in its own blocking thread
  with its own runtime, so one Python subprocess never starves the others.
