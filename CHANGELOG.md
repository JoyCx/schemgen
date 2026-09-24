# Changelog

## 2.1.0

The first release with downloads: one self-contained binary per platform with
the web UI inside, and a Minecraft mod that drives it from inside the game.

### Everywhere

- **No Python.** Loading, voxelizing and color sampling run in Rust. The voxel
  shells are identical to 2.0's Python helper; conversions are about 12 times
  faster and use a fifth of the memory. Materials follow the glTF
  specification — as the web preview draws them — where 2.0 lost some of them
  on the way in: vertex colors, alpha modes, texture coordinate sets and
  transforms, wrap modes, emission, specular-glossiness materials
  ([pipeline.md](docs/pipeline.md#parity-with-the-python-helper)).
- **Minecraft 1.16.5 to 26.3.** A schematic for a version uses only blocks
  that exist in it (`--target`, the *Minecraft version* setting).
  [versions.md](docs/versions.md)
- **More formats:** WorldEdit `.schem` (Sponge v2, and v3 for WorldEdit 7.3+)
  and structure-block `.nbt` beside Litematica's `.litematic`.
- `max_size` is exact: 128 gives 128 blocks, not 129.

### Web app

- A new workspace: the model and its blocks in one 3D view with a draggable
  split, a live preview that follows every setting, the material list in
  stacks and shulker boxes, one queue for one model or a hundred, dark mode,
  keyboard shortcuts. [design.md](docs/design.md)
- Blocks are drawn with the textures of the Minecraft installed on your
  computer, when there is one.
- Folder suggestions know launcher instances and set the Minecraft version
  from the instance you pick.
- Built into the binary: run `schemgen2` and the browser opens on it.
  `run_server.bat` is gone.

### Server and CLI

- **API v2** ([api.md](docs/api.md)): the settings schema UIs draw
  themselves from, jobs with server-sent events and cancellation, packed
  previews that stop when the client goes away. The v1 routes keep working
  for this release.
- Safer by default: binds `127.0.0.1`, checks `Host` and `Origin`, optional
  bearer token (`--token`, `--token-file`); finished jobs and their files are
  forgotten after `--job-ttl` hours.
- For launchers: `--port 0` prints the address it got, `--exit-with-stdin`,
  `--pid-file`, `--max-jobs`.
- `serve --open` opens the web UI in the browser; `--textures` names the
  block textures to use.

### Minecraft mod (new)

A Fabric mod for Minecraft 1.21.1, 1.21.4, 1.21.8 and 1.21.11: pick a model
in game, tune the same settings, see a ghost preview where you are looking,
convert, and place the result with Litematica. It starts the server itself,
downloading the binary of this release on first use and checking its
checksum. [mod.md](docs/mod.md)

### Removed

- The Python voxelizer's runtime dependency. Builds made with
  `--features python-voxelizer` can still run 2.0's helper
  (`--voxelizer python`) for this one release.
- `run_server.bat`.
