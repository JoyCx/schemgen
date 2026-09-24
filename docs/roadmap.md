# SchemGen2 roadmap

Where the project stands, and a phased plan for the four things on the table:

1. a better web UI,
2. an in-game Minecraft mod with its own UI that reuses the same conversion
   backend,
3. a leaner, faster codebase,
4. support for more Litematica / Minecraft versions.

Everything here was written against the code as of the `SchemGen2` initial
commit (`257106d`). File references point at that tree.

## Status

Every phase below has landed, for 2.1.0; [CHANGELOG.md](../CHANGELOG.md) says
what that release brings, and the two big decisions are recorded in
[adr/](adr/README.md).
Where the result differs from the plan:

| Phase | Where it went | Differences from the plan |
|---|---|---|
| 0 — Hygiene and CI | [ci.yml](../.github/workflows/ci.yml) | — |
| 1 — Core and API v2 | [api.md](api.md), [ADR 2](adr/0002-api-v2.md) | Cancellation is `Progress::is_cancelled` rather than a `CancellationToken`; instead of a shared `DashMap` cache, matching de-duplicates each run's quantized colors and matches each once. |
| 2 — Versions | [versions.md](versions.md) | Targets 1.16.5 to 26.3, a palette per target, and besides `.litematic` also Sponge `.schem` v2/v3 and structure `.nbt`. |
| 3 — Web UI | [web.md](web.md), [design.md](design.md) | — |
| 4 — Rust voxelizer | [pipeline.md](pipeline.md#parity-with-the-python-helper), [ADR 1](adr/0001-python-removal.md) | Materials follow the glTF specification rather than trimesh's reading of them (differences listed in pipeline.md). The Python helper remains behind a Cargo feature for one release. |
| 5 — Fabric mod | [mod.md](mod.md) | Minecraft 1.21.1, 1.21.4, 1.21.8 and 1.21.11; **not 26.x**, which ships unobfuscated and needs the Fabric layer moved to Mojang's names, Gradle 9, Loom 1.14 and Stonecutter 0.8 first ([mod.md](mod.md#minecraft-versions)). Compiled in CI; not yet played with. |
| 6 — Packaging | [releasing.md](releasing.md), [release.yml](../.github/workflows/release.yml) | A release workflow rather than `cargo dist`; the web UI is embedded by a build script rather than `include_dir`. Modrinth and CurseForge uploads, code signing and ARM Linux/Windows builds are not automated. |

**Next:** remove API v1, the `python-voxelizer` feature, `backend/scripts/`
and the parity job (all kept one release for compatibility); the mod on
Minecraft 26.x; the mod's pages on Modrinth and CurseForge.

---

## 1. The project today

### What it is

A GLB/glTF → `.litematic` converter with one pipeline and two doors in:

```
voxelize → sample surface colors → brightness/contrast/saturation
         → 8×8 Bayer dither → CIEDE2000 block match → NBT
```

| Layer | Tech | Size | Role |
|---|---|---|---|
| `backend/src` | Rust, Actix-web, Rayon, kiddo | ~4.0k lines, 12 modules | CLI (`convert`, `palette`, `build-table`), HTTP server (`serve`), pipeline, NBT writer |
| `backend/scripts` | Python, trimesh, numpy, scipy | ~1.8k lines | Voxelization and color sampling, run as a subprocess per job |
| `frontend/src` | Vite, React 19, three.js | ~2.6k lines | Drag-drop, 3D model view with light gizmo, live block preview, settings, batch queue, folder picker |
| `docs/` | Markdown | 4 files | cli · api · web · pipeline |

### What is good and should be kept

- **The matching core is solid.** CIELAB + KD-tree + CIEDE2000 re-ranking, a
  curated anti-grief palette, deterministic output (seeded sampling, sorted
  palette order). 29 Rust tests and 23 Python tests cover the maths, the NBT
  writer, path handling and CLI parsing. `cargo test` passes on Linux today.
- **The de-light model is a real differentiator** (specular subtraction,
  highlight rejection, blown-voxel recovery), and it is mirrored in GLSL so the
  web preview predicts the conversion.
- **The HTTP API is already client-agnostic.** `docs/api.md` describes a
  four-call flow (`health → convert → progress → download`) and even mentions
  "the mod pings it". The mod in this plan is exactly that client.
- **Output handling is thoughtful**: launcher-instance folder discovery,
  save-into-folder without re-conversion, reveal in file manager.

### What holds it back

**Backend**

- **The Python dependency is the biggest liability.** Every conversion spawns
  `python3 scripts/voxelize.py`, streams coordinates and colors back as JSON,
  and requires `trimesh` on the user's `PATH` interpreter (not configurable,
  see README). This blocks single-binary distribution, which the mod needs,
  costs a JSON round trip per job, and makes cancellation impossible.
- **The pipeline is duplicated.** `converter::convert` and
  `converter::preview_litematic` are the same five stages written twice, and
  the preview writes a `.litematic` to a temp file only to delete it
  (`api.rs:352-376`).
- **Version stamping is a process-wide global** (`litematic.rs`
  `static DATA_VERSION: AtomicI32`), the schematic `Version` is a hard-coded
  `6`, and nothing gates palette entries by Minecraft version: the shipped
  table contains `resin_block` (1.21.4+), so stamping anything older produces a
  file that fails DataFixer, as `docs/pipeline.md` itself warns.
- **API ergonomics**: 11 mandatory multipart fields (an omitted field is a
  400), progress by 800 ms polling, no cancel, no job list, jobs and their
  files in `backend/uploads` / `backend/outputs` never expire.
- Errors are `String`s although `thiserror` is a dependency; no CI; no
  `clippy`/`rustfmt` gate.

**Frontend**

- **It does not build on a case-sensitive filesystem.** `main.jsx` imports
  `./App.jsx` while the file is `app.jsx`; `app.jsx` imports
  `DropZone.jsx`, `Settings.jsx`, `ModelPreview.jsx`, `MinecraftPreview.jsx`,
  `PaletteGrid.jsx`, `App.css` while the files are lowercase. `npm run build`
  fails on Linux and macOS-with-case-sensitive-APFS with
  `Could not resolve "./App.jsx"`. It only works on Windows today.
- `app.jsx` is a 500-line component holding all state, six effects and a set
  of refs to dodge stale closures. `ResultPanel.jsx` and `ProgressPanel.jsx`
  are dead code (not imported anywhere).
- Setting defaults are repeated three times in `api.js` and once more in
  `app.jsx`; adding a setting means touching four frontend places plus the
  backend (`docs/web.md` documents this as a known hazard).
- Two independent three.js renderers (model view, block view) each with its
  own WebGL context; block textures for the preview are fetched from a
  third-party CDN pinned to 1.21.5 (`minecraftpreview.jsx:61`).
- Design: a single 720 px column, light theme only, emoji as icons, lighting
  controls live in the model-preview panel while the rest live in the settings
  panel. It is clean but reads as a form, not a workspace. No material list
  (block counts), no compare view, no error toasts.
- No lint, no tests, footer says `v2.1.0` while `package.json` and
  `Cargo.toml` say `2.0.0`.

---

## 2. Target architecture

```
                 ┌───────────────────────────────────────────────┐
                 │ schemgen-core (Rust lib, no I/O policy)       │
                 │  mesh load (gltf) → voxelize → sample →       │
                 │  adjust → dither → match → BlockGrid          │
                 │  formats: litematic (versioned), later .schem │
                 │  targets: MC version table, palette gating    │
                 └───────┬───────────────┬───────────────┬───────┘
                         │               │               │
              schemgen-cli        schemgen-server      (tests, bench)
                                  HTTP API v2
                                  ├── SSE progress, cancel, job TTL
                                  ├── GET /api/schema (settings + targets)
                                  └── serves frontend/dist
                                         ▲                 ▲
                                         │                 │
                                   Web UI (React)   Fabric mod (Java)
                                   two-pane         Minecraft-native screen,
                                   workspace        same sections/labels,
                                                    launches server as sidecar,
                                                    hands result to Litematica
```

Decisions this plan commits to:

1. **One backend, one pipeline.** The web app and the mod are both HTTP
   clients of the same server. The mod never re-implements conversion; its
   "different visual representation" is in-game rendering of the *result*
   (ghost blocks via Litematica placements), not a port of the pipeline.
2. **Drop Python.** Port `voxelize.py` and `sample_colors.py` to Rust. This is
   the prerequisite for shipping the server as a single binary the mod can
   bundle and launch. It is also the largest single speed and reliability win.
3. **Settings are schema-driven.** The server publishes its settings schema
   (`GET /api/schema`: fields, types, ranges, defaults, groups, help text).
   Both UIs render their forms from it. That is how the mod "looks like the
   web one" without sharing code with it, and how a new setting stops being a
   five-place edit.
4. **Targets are data, not globals.** A `Target { mc_version, data_version,
   schematic_version }` travels with each job. The palette is filtered per
   target.
5. **Fabric first for the mod**, multi-version via one Gradle project, with
   the Litematica-touching code isolated behind a tiny bridge interface.
   Forge/NeoForge (Forgematica) is a later port, not a first target.

---

## 3. Workstreams

### Phase 0 — Hygiene and CI (about 1 week)

Cheap, unblocks everything else, and can be merged immediately.

- Fix the case-sensitive imports (rename files to the PascalCase the imports
  use, or fix the imports). Verify `npm run build` on Linux.
- Delete `ResultPanel.jsx`, `ProgressPanel.jsx`; keep `shader-check.html` but
  register it in `vite.config.mjs` as a second input so it is actually served
  from `dist`.
- One `settingsDefaults.js` in the frontend; `api.js` uses it. (Superseded by
  the schema endpoint in Phase 1, but it removes the triple copy now.)
- GitHub Actions: `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test`, `python test_sample_colors.py`, `npm ci && npm run build`,
  on Linux and Windows.
- ESLint + Prettier for the frontend; `rustfmt.toml` for the backend.
- `SCHEMGEN_PYTHON` env var / `--python` flag so a venv interpreter can be
  named (quick fix for the README's "not currently configurable"). Goes away
  in Phase 4, but it costs 10 lines and helps every user until then.
- Align the version string (one source: `Cargo.toml`, surfaced through
  `/api/health`, which the UI already reads).

### Phase 1 — Core extraction and API v2 (2–3 weeks)

**Backend restructuring**

- Turn the crate into a Cargo workspace: `crates/core`, `crates/cli`,
  `crates/server`. Only `core` knows about meshes, palettes and NBT; only
  `server` knows about Actix, jobs and folders.
- Single pipeline entry point:
  `core::pipeline::run(source, &Options, &Palette, &mut dyn Progress) ->
  Result<BlockGrid, Error>` where `BlockGrid { dims, coords, block_ids,
  palette_names }`. Conversion = `run` + `formats::litematic::write`;
  preview = `run` at low resolution, no file. This deletes
  `preview_litematic`.
- `SchemgenError` enum via `thiserror`; `String` errors go.
- Cancellation: `tokio_util::CancellationToken` threaded into `run`; the
  voxelizer subprocess (until Phase 4) is killed on cancel.
- Palette match cache: after dithering most voxels share a handful of colors;
  quantize to 8-bit RGB and memoize CIEDE2000 results in a `DashMap`. Expected
  2–5× on the match stage for large models.
- Stream the NBT straight into the `GzEncoder` instead of building the whole
  uncompressed buffer first; stamp real `TimeCreated`/`TimeModified`.

**API v2** (keep v1 routes alive one release for the existing UI)

| Route | Change |
|---|---|
| `GET /api/schema` | New. Settings fields with type, range, default, group, label, help; list of targets (MC versions); palette summary. |
| `POST /api/jobs` | Multipart: `file` + one `settings` JSON part. All fields optional with defaults. Returns `{job_id}`. Replaces `/convert` and `/convert-batch` (batch = N files in one request, same as now). |
| `GET /api/jobs/{id}/events` | SSE: `progress`, `done`, `error`. Polling stays as `GET /api/jobs/{id}`. |
| `DELETE /api/jobs/{id}` | Cancel, or delete a finished job's files. |
| `GET /api/jobs` | List (the mod's "recent conversions"). |
| `POST /api/preview` | Returns `{dims, palette: [names], blocks: base64 of Int32 x,y,z,idx}` — roughly 5× smaller than the current per-block `{x,y,z,name}` JSON. |
| `GET /api/jobs/{id}/thumbnail.png` | New. Offscreen isometric render of the block grid in Rust (`image` crate, flat-shaded cubes). Cheap, and it is what the mod shows in its job list. |
| all | Optional `Authorization: Bearer <token>`; the server prints or writes the token at startup, the mod passes it when it launches the sidecar. Closes the "any local process can call `/api/reveal-folder`" hole the README warns about. |
| jobs | TTL sweep of `uploads/` and `outputs/` (default 24 h, configurable). |

Acceptance: web UI running on v2 with identical behavior; `docs/api.md`
rewritten; the four-call flow still holds for any client.

### Phase 2 — More Litematica / Minecraft versions (1–2 weeks, parallel with Phase 1 after the `Target` type lands)

**What "version" actually means here.** Three independent numbers:

1. **Litematica schematic `Version`.** Verified against the mod's
   `LitematicaSchematic.java`:

   | Litematica branch | Writes | Accepts |
   |---|---|---|
   | 1.18.x, 1.19.x, 1.20.x | `Version` 6, `SubVersion` 1 | 1–6 |
   | 1.21.x and the sakura-ryoko builds for 26.x | `Version` 7, `SubVersion` 1 | 1–7 |

   Version 7 only changed how sleeping-entity positions are stored; reading a
   block-only schematic is identical for 6 and 7. **The current writer's
   `Version` 6 already loads everywhere from 1.18 to 26.x.** So the field
   becomes part of `Target` (6 for targets below 1.21, 7 from 1.21 up, for
   parity with what the game itself writes) but it is not where the
   compatibility work is.

2. **`MinecraftDataVersion`.** This is the real switch. DataFixer upgrades a
   schematic stamped *older* than the running game; it refuses names that did
   not exist at the stamped version. Table to ship in `core/targets.rs`
   (values from PrismarineJS `minecraft-data`):

   | Target | Data version |
   |---|---|
   | 1.16.5 | 2586 |
   | 1.17.1 | 2730 |
   | 1.18.2 | 2975 |
   | 1.19.4 | 3337 |
   | 1.20.1 | 3465 |
   | 1.20.4 | 3700 |
   | 1.20.6 | 3839 |
   | 1.21.1 | 3955 |
   | 1.21.4 | 4189 |
   | 1.21.5 | 4325 |
   | 1.21.8 | 4440 (current default) |
   | 1.21.10 | 4556 |
   | 1.21.11 | 4671 |
   | 26.1 | look up when adding (year-based scheme starts here) |
   | 26.2 | 4903 |
   | 26.3 | 5023 |

   Note Mojang retired `1.x` after 1.21.11; `26.1` is the successor, and
   Litematica builds exist for 1.21.8, 1.21.10, 1.21.11, 26.1, 26.2 (sakura-ryoko
   fork). The UI should present these as a dropdown of *game versions*, never
   raw data versions.

3. **Palette availability and names per version.** Each color-table entry
   gets `since` (data version the block first exists at) and optional
   `renamed_from`. Generate these automatically with a script over
   `minecraft-data`'s per-version `blocks.json` rather than by hand.
   Examples the current table needs: `resin_*` (1.21.4), `pale_oak_*` (1.21.4),
   tuff bricks / polished tuff / chiseled copper family (1.21), mud, mangrove,
   froglights (1.19), cherry, bamboo (1.20), deepslate / copper / tuff base
   (1.17). `Palette::for_target(&Target)` filters before the KD-tree is built,
   so an older target simply has fewer candidate blocks and never emits a name
   the game does not know.

**Floor and ceiling.** Floor: 1.16.5 (post-flattening names, and the oldest
version Litematica/Forgematica meaningfully target). 1.12 numeric IDs are out
of scope. Ceiling: whatever the latest table entry is; adding a version is one
row plus regenerating `since` tags, and should be a documented one-line
procedure.

**Other formats (stretch, ~300 lines each once `BlockGrid` exists).**
Sponge `.schem` v2/v3 for WorldEdit/FAWE, and vanilla structure `.nbt`.
This widens "which mods and servers can load the output" more than another
Litematica version does.

**Testing.** Add a minimal NBT *reader* in `core` for round-trip tests; a CI
step that loads each target's output with `litemapy` (Python, only in CI); and
a manual matrix documented in `docs/versions.md`: one Prism instance per
target, load the same model, screenshot.

**UI.** "Target Minecraft version" dropdown in both UIs, remembered per
machine; the folder picker already knows which instance a folder belongs to,
so it can suggest the target from the instance's `mmc-pack.json` /
`minecraftinstance.json` (nice-to-have).

### Phase 3 — Web UI redesign (2–3 weeks, after Phase 1's schema endpoint)

**Layout.** From a stacked column to a workspace:

```
┌──────────────────────────────────────────────────────────────┐
│ SchemGen2      ● backend ok · 1.21.8 · 181 blocks    [Dark]  │
├──────────────────────────────┬───────────────────────────────┤
│  Viewport                    │  Settings (schema-driven)     │
│  [Model] [Blocks] [Split]    │  ▸ Size & shape               │
│                              │  ▸ Color                      │
│   one three.js renderer,     │  ▸ Lighting (gizmo lives in   │
│   scenes swapped per tab,    │     the Model tab; sliders    │
│   split = draggable divider  │     here)                     │
│                              │  ▸ Target version             │
│  Material list  ────────     │  ▸ Output folder              │
│  stone 1 240 · ...           │                               │
├──────────────────────────────┴───────────────────────────────┤
│  Drop zone / job queue (single and batch are the same list)  │
│  [Convert]  ▓▓▓▓▓▓░░ 62% Matching blocks…   [Show] [Save]    │
└──────────────────────────────────────────────────────────────┘
```

- Single job list instead of "single mode vs batch mode": one file is a
  batch of one. Removes the `files.length === 1` branching in `app.jsx`.
- Material list with counts per block (from `BlockGrid`), sortable, with a
  copy-as-text button — the same data the mod will show, and what builders
  actually need next.
- Compare slider between model and blocks in the same camera.
- Design tokens (`--bg`, `--surface`, `--accent`…) already exist; add a dark
  theme, replace emoji with an icon set (lucide-react), keyboard shortcuts
  (`Ctrl+Enter` convert, `1/2/3` tabs), toasts for errors instead of inline
  red paragraphs, backend-health chip in the header.
- Responsive: the two panes collapse to tabs below 900 px.
- Write `docs/design.md`: the section order, labels, defaults and groupings.
  The mod screen follows this document, which is what "looks like the web
  one" means across two very different rendering stacks.

**Code.** `useConversion()`, `useJobEvents()` (SSE), `useSettings()` with the
schema; `app.jsx` becomes layout only. Vitest + Testing Library for the hooks
and the settings form; Playwright smoke test that uploads a fixture GLB against
the real server in CI. Gradual TypeScript: new files `.tsx`, `allowJs` on.
Block textures for the preview: serve them from the backend (extracted from
the user's own client jar, which the folder scanner can already locate) with
the CDN as fallback, so the preview does not depend on a third-party host.

### Phase 4 — Rust voxelizer and sampler (3–5 weeks, independent track; start early)

The largest piece of work and the one with the most payoff.

- `core::mesh`: load glTF/GLB with the `gltf` crate (embedded and external
  buffers, textures via `image`), flatten the node hierarchy, collect
  triangles with UVs, vertex colors, normals and material base color /
  metallic / roughness — the same inputs `sample_colors.py:build_mesh_data`
  gathers.
- `core::voxel`: surface-only voxelization by seeded area-weighted triangle
  sampling (port of `voxelize_surface`), plus edge sampling, in Rayon.
  Deterministic with `rand_pcg` seeded `0x5CE2`.
- `core::sample`: port of `supersample`, `shade`, `apply_delight`,
  `highlight_weight`, `apply_highlight_recovery`, `reference_albedo`. Bilinear
  texture lookup in linear light, alpha weighting, per-voxel accumulation.
- Parity harness: the existing Python tests become fixtures; a `parity`
  example runs both implementations on the sample models and reports block
  mismatch rate and mean CIEDE2000 delta. Target: < 0.5 % blocks differ, and
  those only at dither boundaries. Byte-identical output *will* change once
  (different RNG stream); document it and re-baseline.
- Keep the Python path behind `--features python-voxelizer` for one release,
  then delete `backend/scripts` and the Python section of the README.

Payoff: single static binary (Windows, macOS, Linux); no interpreter hunt; no
JSON round trip (tens of MB for large models); cancellable; 2–4× faster end to
end from removing process spawn, JSON parse and Python overhead; and the mod
can bundle it.

### Phase 5 — Fabric mod (4–6 weeks, after Phase 1; sidecar launch after Phase 4)

**Scope.** A client-side Fabric mod, `schemgen-mod/`, Java 21, Gradle with
Fabric Loom. Multi-version from one source tree using Stonecutter (one
`versions/` folder per target: 1.21.1, 1.21.4, 1.21.8, 1.21.11, 26.x), with
all Minecraft-version-specific code confined to `LitematicaBridge` and
`ScreenCompat`. Dependencies: Fabric API; Litematica + MaLiLib as *optional*
runtime deps (detected via `FabricLoader.isModLoaded`); without them the mod
still converts and drops the file into `schematics/`.

**Modules.**

| Package | Responsibility |
|---|---|
| `backend.BackendClient` | `java.net.http.HttpClient` against API v2: health, schema, jobs, SSE events, preview, thumbnail, download. Bearer token. |
| `backend.BackendLauncher` | Find `schemgen2` (config path, or extracted from the jar's `resources/bin/<os>-<arch>/` into `.minecraft/schemgen/bin/`), start `schemgen2 serve --port 0 --token …`, read the chosen port from stdout, health-poll, stop on game exit. Falls back to "connect to an already running server at host:port" (Phase-1 mode before the sidecar exists). |
| `ui.SchemGenScreen` | The main screen. Same sections, labels, defaults and order as `docs/design.md`; widgets are vanilla `ClickableWidget`s (or MaLiLib's if present) so it feels native: left column model list + thumbnail, right column settings rendered from `/api/schema`, bottom bar with progress and actions. |
| `ui.FilePicker` | Model source. Two paths: a watched folder `.minecraft/schemgen/models/` listed in-game, and a real native dialog through LWJGL's bundled `tinyfd` (`TinyFileDialogs.tinyfd_openFileDialog`), which the game already ships. |
| `preview.GhostPreview` | The in-game visual representation. On preview response, build `BlockState[]` from `palette` + indices and render as translucent ghost blocks anchored at the player's look target, with a size/rotation handle. If Litematica is present, create a temporary placement instead — it already renders ghost blocks well. |
| `litematica.LitematicaBridge` | `loadIntoLitematica(Path)`: add the file to `SchematicHolder` and create a `SchematicPlacement` at the player's position. Internal API, changes per version; this class is the only one compiled per target. |
| `config` | Server mode (sidecar / external), host, port, token, output folder (defaults to the instance's `schematics/`), target version (defaults to the running game's data version, read from `SharedConstants`). |

**In-game flow.** Keybind opens the screen → pick model → settings come
pre-filled from the schema → "Preview" shows ghost blocks in the world →
"Convert" streams progress → result lands in `schematics/` → "Load in
Litematica" places it. The target version defaults to the game you are in, so
the version dropdown is mostly invisible in the mod.

**What the mod does *not* do.** Render the GLB itself (a glTF renderer in
Minecraft is a project of its own); the backend's thumbnail and the ghost
preview cover the need. It also does not contain any of the pipeline.

**Distribution.** Modrinth + CurseForge, one jar per Minecraft target, with
the platform-specific server binaries as separate downloads the mod fetches on
first run (checksums pinned in the jar) to keep the jar small; or bundled, if
size (~10 MB per platform) is acceptable.

### Phase 6 — Packaging and release (1 week)

- `cargo dist` or a release workflow producing `schemgen2` for
  win-x64, mac-arm64/x64, linux-x64, with `frontend/dist` embedded via
  `include_dir` so the web UI is inside the binary. `run_server.bat` goes away.
- Tagged releases: server binaries, mod jars, checksums. The mod's launcher
  pins the server version it was tested with.
- `docs/`: update `api.md` (v2), `web.md`, add `mod.md`, `versions.md`,
  `design.md`, and an ADR folder with the two big decisions (Python removal,
  API v2).

---

## 4. Sequencing

| # | Phase | Weeks | Depends on | Deliverable |
|---|---|---|---|---|
| 0 | Hygiene & CI | 1 | — | Builds on all OSes, CI green, dead code gone |
| 1 | Core extraction & API v2 | 2–3 | 0 | Workspace crates, single pipeline, schema endpoint, SSE, cancel, token, TTL |
| 2 | Multi-version | 1–2 | 1 (Target type) | Version dropdown, per-target palette, tests per target |
| 4 | Rust voxelizer | 3–5 | 0 (parallel with 1–3) | Python gone, single binary |
| 3 | Web UI redesign | 2–3 | 1 | Two-pane workspace, material list, dark mode, tests |
| 5 | Fabric mod | 4–6 | 1 (client), 4 (sidecar) | Jar per MC target, in-game screen, ghost preview, Litematica hand-off |
| 6 | Packaging | 1 | 3, 4, 5 | Release pipeline, docs |

Roughly 14–20 weeks of single-developer work end to end; 4 and 5 are the
critical path. If time is short, the order that keeps every merged step
useful is **0 → 1 → 2 → 3 → 4 → 5**, and the mod can ship first in
"external server" mode (user runs `schemgen2 serve`) before Phase 4 lands.

---

## 5. Risks

| Risk | Mitigation |
|---|---|
| Rust port of the sampler drifts from the Python model, and the web shader (which mirrors Python) with it | Parity harness with the existing 23 Python tests as fixtures; shader is only touched once the Rust model is the reference |
| Litematica internals change per Minecraft version | Only `LitematicaBridge` touches them; everything else uses files + folders, which never change |
| Year-based Minecraft versions (26.x) and rapid Litematica fork releases | Target table is data; adding a row is the documented procedure; CI matrix loads output with `litemapy` |
| Sidecar process management on three OSes (zombie servers, port clashes, antivirus on Windows) | `--port 0` + printed port, PID file, kill on exit hook, "external server" fallback mode always available |
| Unauthenticated local API exposed by a game process | Bearer token from Phase 1, bound to `127.0.0.1` only |
| Scope creep in the UI redesign | `docs/design.md` first, then build; the mod follows the same doc |

---

## 6. First pull requests, in order

1. **Fix frontend build on Linux/macOS** (rename to PascalCase), delete
   `ResultPanel.jsx` / `ProgressPanel.jsx`, single defaults module, version
   string from `/api/health`. Small, safe, immediately useful.
2. **CI workflow** for Rust, Python and Node on Linux + Windows.
3. **`SCHEMGEN_PYTHON` / `--python`** and job/file TTL sweep.
4. **`Target` type + `targets.rs` table + per-target palette filtering**, with
   `--target 1.20.4` on the CLI and a dropdown in the current UI. This is the
   whole of "more versions" for users, and it does not wait for the API
   rewrite.
5. **Single pipeline function** replacing `convert`/`preview_litematic`;
   preview stops writing a file.
6. **`GET /api/schema`** and the schema-driven settings form. From here the
   web redesign and the mod screen can be built in parallel by different
   people.
