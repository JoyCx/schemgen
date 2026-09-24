# The conversion pipeline

Everything below is shared by every front end. The CLI, the HTTP API (and so
the web app and the mod) build the same `Settings` from the same defaults and
call the same `pipeline::run`, so a given model and settings produce the same
blocks whichever door they came in through.

```
GLB/glTF
   │
   ├─ 0. load the model                     crates/core/src/mesh.rs
   ├─ 1. voxelize + sample surface colors   crates/core/src/voxel.rs
   │                                        crates/core/src/sample.rs
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

Stages 0 and 1 run in-process: the model is parsed once for voxelization and
color sampling, the work is split across cores with Rayon, and a cancelled
conversion stops between sampling passes. (SchemGen2 2.0 ran them in a Python
subprocess; see [Parity with the Python helper](#parity-with-the-python-helper).)

## 0. Loading

`.glb` and `.gltf` — buffers in the GLB, in `data:` URIs or in files beside a
`.gltf`; PNG, JPEG and WebP textures. The default scene's node hierarchy is
flattened, each node's transform baked into its primitives' vertices, and a
mirroring transform reverses triangle winding. Triangles, strips and fans are
read; points and lines are ignored. Only the textures a material samples are
decoded, and only when colors are sampled.

A model that *requires* an extension SchemGen2 cannot read — Draco or meshopt
compression, mesh quantization — is refused with a message naming it. A KTX2 or
DDS texture is skipped with a warning and its material sampled without it. The
server refuses a `.gltf` that refers to files beside it: an upload arrives
alone, so such a reference could only reach other uploads.

## 1. Voxelization

Surface-only ("hollow"): the interior is never filled, so a closed model becomes
a shell rather than a solid mass of blocks.

`max_size` sets the longest axis in blocks and the voxel pitch is derived from
it, unless an explicit `voxel_size` overrides it (at most 2048 blocks along any
side either way). A point on the model's far face belongs to the last layer of
blocks rather than opening one more, so the longest axis is exactly `max_size`
blocks.

Three passes find the voxels the surface passes through, and no dense grid is
ever built, so memory follows the surface rather than the volume:

1. points scattered over the triangles by area — about twelve per voxel face of
   surface, between 100 000 and 8 million in all;
2. points along every edge, one per voxel length, which catches thin walls and
   sharp features scattering can miss;
3. every vertex.

## 2. Color sampling

A voxel covers a patch of surface, not a point, and textures are usually finer
than the voxel grid, so one lookup per voxel aliases. The surface is
supersampled instead — 24 points per voxel face of surface area, at least one
per triangle, capped by `ram_limit` — and each voxel averages the samples that
land in it, in linear light, weighted by alpha. Voxels no sample reached (thin
features the edge and vertex passes found) take the surfaces crossing them
instead, weighted by how much surface each triangle can put in one voxel.

### Materials

Materials are read the way the glTF specification defines them, which is also
what the web UI's model preview (three.js) draws:

| Input | How it is read |
|---|---|
| `baseColorFactor` | Linear RGBA, multiplying everything below |
| `baseColorTexture` | sRGB, filtered bilinearly in linear light — or nearest-texel when the sampler's magnification filter is `NEAREST` (pixel art); the texture's own `texCoord` set, `KHR_texture_transform`, and the sampler's wrap modes |
| `COLOR_0` | Linear RGBA, multiplying the base color, with or without a material |
| No material | glTF's default material: white |
| Alpha | By `alphaMode`: `OPAQUE` ignores alpha; `MASK` keeps what reaches `alphaCutoff`; `BLEND` weights samples by alpha |
| `metallicFactor`, `roughnessFactor`, their texture | Drive the metal bake below. A material with neither the factor nor the texture is taken as non-metal, although glTF's default is 1: exporters that leave the block out almost never mean "mirror" |
| `emissiveFactor`, `emissiveTexture`, `KHR_materials_emissive_strength` | Added on top of the lit color |
| `KHR_materials_pbrSpecularGlossiness` | Converted to metallic-roughness per sample, by the formulas the extension's authors published |
| `NORMAL` | The file's normals, for everything that depends on where the light falls; angle-weighted vertex normals when the file has none |
| Normal, occlusion and other maps | Not used |

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

The model lives in `crates/core/src/sample.rs` (`Light::response`,
`Light::delight`, `Light::keep`, `recover_highlights`) and is mirrored in GLSL
by `frontend/src/delightshader.js`. **The web preview only predicts the
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

1. **Seeded surface sampling** — `voxel::voxelize` draws its points from a
   generator seeded with `0x5CE2`. Face sampling is probabilistic, so without
   a fixed seed marginal voxels appear or vanish between otherwise identical
   runs and the block count drifts a little each conversion. The generator is
   NumPy's (`rng.rs`: `SeedSequence` + PCG64, bit for bit), drawn in the same
   order the Python helper drew it, and each parallel task jumps straight to
   its own part of the stream — so the result depends neither on the thread
   count nor on which implementation ran.
2. **Seeded color scatter** — `sample::sample_colors`, same seed, same reason,
   same stream. Sums are added in sample order within fixed passes, so they
   round the same way on any number of threads.
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

`cargo test` also covers the loader (scene order, transforms, winding, URIs,
refused extensions, malformed files), the voxelizer (exact `max_size`, hollow
shells, determinism) and the sampler (factors, vertex colors, alpha modes,
texture orientation, transforms, UV sets, emission, the metal bake,
specular-glossiness), and the server tests run real conversions — no Python
needed.

The de-light preview shader is checked by compiling it against a real WebGL
context: run the dev server and open `/shader-check.html`.

## Parity with the Python helper

SchemGen2 2.0 voxelized and sampled in Python (`backend/scripts/voxelize.py`,
`sample_colors.py`, on trimesh). The Rust implementation is a port of it —
the same algorithm, constants and random stream — and a harness holds it to
that:

```bash
pip install -r backend/scripts/requirements.txt
cd backend && cargo run --release -p schemgen-core --features python-voxelizer \
    --example parity -- [--max-size 64] [MODEL.glb ...]   # default: fixtures/*.glb
```

It converts each model both ways and reports voxels only one side produced,
the CIEDE2000 difference over the voxels both produced, and blocks that differ
after the shared adjust → dither → match stages. Rows marked `trimesh` read
materials the way the Python helper could through trimesh, so they measure the
port; rows marked `gltf` are what ships. CI runs it on the fixtures and fails
if a `trimesh` row reaches 0.5 % of blocks different.

Measured at `max_size` 64 (and 256 for the larger ones) on the fixtures and 20
Khronos glTF sample models:

| Models | Voxels only one side made | Mean ΔE₀₀ | Blocks different |
|---|---|---|---|
| The fixtures; Khronos samples with PNG textures — BoomBox, Duck, Avocado, Lantern, WaterBottle, FlightHelmet (`.gltf` + files), BoxTextured (data URIs), MetalRoughSpheres, NegativeScaleTest, OrientationTest, TextureCoordinateTest, TextureSettingsTest, TextureTransformMultiTest, VertexColorTest, BoxVertexColors | 0 | < 0.0002 | 0 % |
| JPEG-textured: DamagedHelmet, CesiumMan, AlphaBlendModeTest | 0 | 0.003–0.06 | 0.06–0.9 % |
| The same three with their textures re-encoded as PNG | 0 | 0.0000 | 0 % |
| SpecGlossVsMetalRough | 0 | 0.07 | 1.9 % |

The voxel shells are identical everywhere, transforms and mirrored nodes
included. Colors differ in two understood places: JPEG decoders round
differently (libjpeg-turbo, which Pillow uses, and the Rust decoders disagree by
up to 4 levels on a few percent of texels, and dithering turns some of that
into different blocks), and trimesh converted specular-glossiness materials
per texel into 8-bit textures where the Rust sampler converts per sample.

### What changed from 2.0

The Python helper saw materials through trimesh, which lost some of them on
the way. The Rust sampler reads them as the specification (and the preview)
does, so these models convert differently than they did in 2.0 — on purpose:

| | 2.0 (Python, via trimesh) | Now |
|---|---|---|
| `COLOR_0` | Ignored on primitives with a material; read as sRGB on those without | Linear, always applied |
| No material | Grey (102, 102, 102) | White |
| `baseColorFactor` | Rounded to 1/255; a near-black factor (every channel under 0.006) turned white | Exact |
| Alpha | Texture alpha weighted every sample, whatever `alphaMode` said | By `alphaMode` |
| Texture coordinates | `TEXCOORD_0` only, no `KHR_texture_transform`, always repeating, always bilinear | The texture's own set, transform, wrap modes and `NEAREST` filter |
| Normals | Recomputed from the geometry; the file's discarded | The file's, when it has them |
| Emission | A texture without a factor (or with an all-zero one) glowed at full strength; `KHR_materials_emissive_strength` ignored | Factor × strength × texture, as specified |
| Triangle fans | Skipped | Read |

Normals change the most models, but only a little: they only move where
highlight rejection discounts samples (DamagedHelmet: 1.9 % of blocks).
Vertex-colored models change the most: `COLOR_0` written as sRGB bytes — as
trimesh-made files do — now comes out lighter, the way every glTF viewer,
the preview included, shows it.

`--voxelizer python` (and `--python <path>`) still run the Python helper in
builds made with `--features python-voxelizer`, for one release.

## Performance notes

- Voxelization, color sampling, adjustment, dithering and block matching are
  Rayon-parallel, and matching does each distinct quantized color once.
- The sampler's memory use is bounded by `ram_limit` (GB, default 4), which caps
  the number of surface samples.
- Against 2.0's Python helper, on DamagedHelmet with 4 cores: 1.1 s and 186 MB
  instead of 13 s and 890 MB at `max_size` 256; 3.4 s and 270 MB instead of
  44 s and 1.7 GB at 512.
- Schematics are streamed through the gzip encoder as they are written rather
  than built in memory first.
- The server runs at most `--max-jobs` conversions at once (default: the CPU
  count); a batch request further limits its own files with `threads`.
