# 1. Voxelize and sample colors in Rust, not Python

- **Status:** accepted — shipped in 2.1.0
- **Decided in:** roadmap Phase 4 ([roadmap.md](../roadmap.md))

## Context

SchemGen2 2.0 was a Rust binary that handed the first half of every conversion
to a Python script: `backend/scripts/voxelize.py` loaded the glTF with
trimesh, voxelized its surface and sampled a color per voxel, and the binary
read the result back and did the rest (adjust, dither, CIEDE2000 match, NBT).

That split cost more than it looked:

- **Installing.** Everyone needed Python with trimesh, NumPy, SciPy and
  Pillow, found through `PATH` or `--python`, and the script folder beside the
  binary — three things to get right before a first conversion could work.
- **The mod.** The roadmap's Minecraft mod starts the server itself. Asking
  players to install a Python distribution first would have made that
  pointless; bundling one per platform would have made the mod huge and
  fragile.
- **Speed and memory.** The subprocess serialized every voxel and color
  across a pipe and held the model in NumPy several times over: 13 s and
  890 MB for DamagedHelmet at 256 blocks.
- **Fidelity.** trimesh's view of glTF materials dropped or bent several of
  them (vertex colors, alpha modes, texture coordinate sets and transforms,
  wrap modes, emission), so conversions disagreed with the web preview, which
  draws the file as the specification says.
- **Two languages for one model.** The lighting model lived in Python and in
  the preview's GLSL; cancelling a job meant killing a process.

## Decision

Port loading, voxelization and color sampling into `schemgen-core`, in Rust:

- `mesh.rs` reads glTF/GLB with the `gltf` crate — scene graph, transforms,
  materials, PNG/JPEG/WebP textures — and refuses, by name, the extensions it
  cannot read (Draco, meshopt, quantization).
- `voxel.rs` and `sample.rs` port the Python algorithm, constants and all.
- `rng.rs` reproduces NumPy's `SeedSequence` + PCG64 bit for bit, with jump
  ahead so parallel tasks draw exactly the stream the script drew.

The port is held to the script rather than merely inspired by it: the
`parity` example converts models both ways, and CI fails if blocks differ on
0.5 % or more of a fixture's voxels when materials are read the way trimesh
read them. What ships reads materials the way the specification does, and the
differences from 2.0 are listed in
[pipeline.md](../pipeline.md#what-changed-from-20).

The Python path stays for one release behind the `python-voxelizer` Cargo
feature (`--voxelizer python`), off in release builds, so the parity harness
has something to compare against and anyone surprised by a difference can
check it.

## Consequences

- One self-contained binary per platform; nothing to install. This is what
  makes the release downloads and the mod's sidecar possible
  ([0002](0002-api-v2.md), [releasing.md](../releasing.md)).
- About 12 times faster and 5 times less memory (1.1 s and 186 MB for the
  example above), cancellable between passes, no pipe.
- Conversions match the preview. Some models convert differently than in 2.0,
  on purpose — mostly vertex-colored ones and those whose materials trimesh
  mangled.
- Voxel shells are identical to 2.0's; colors agree exactly except where JPEG
  decoders round differently (up to 0.9 % of blocks) and for
  specular-glossiness materials (1.9 %), both explained in pipeline.md.
- We own a glTF material reader. New glTF extensions need Rust code rather
  than a trimesh upgrade.
- **Next release:** remove the `python-voxelizer` feature, `backend/scripts/`
  and the parity CI job.

## Alternatives considered

- **Bundle Python** (an embedded interpreter, PyO3, or a frozen executable):
  tens of megabytes per platform, native wheels to build for each, and still
  two languages for one model.
- **Keep Python optional,** with a lesser built-in voxelizer: two
  implementations that disagree, and the default one worse.
- **An existing Rust voxelizer crate:** different sampling semantics, so no
  parity with 2.0 and no way to show the port is faithful; none reads glTF
  materials the way the sampler needs.
