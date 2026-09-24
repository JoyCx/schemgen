# Web app

A React + TypeScript front end for the same [API](api.md): drop models in,
orbit them in 3D next to their blocks, tune the settings against a live
preview, convert, and have the files written straight into your schematics
folder. [docs/design.md](design.md) describes the screen itself — the layout,
every section and label — and is what the in-game mod's screen follows.

## Run

**From a release:** download `schemgen2` for your platform from the
[releases](https://github.com/JoyCx/schemgen/releases) and run it —
double-click it, or `schemgen2 serve --open` — and the browser opens on
http://localhost:3001. The web UI is inside the binary; there is nothing else
to install.

**From source — one server, one port:**

```bash
cd frontend && npm ci && npm run build
cd ../backend && cargo run --release -- serve --open
```

Building the server after the UI builds `frontend/dist` into it (the build
script, `backend/crates/server/build.rs`, embeds whatever is there, and
rebuilds when it changes). A server built before the UI serves
`frontend/dist` from disk instead when it finds it, and `--ui-dir` serves any
folder. Either way the UI and the API are the same origin and nothing needs a
proxy.

**Development — hot reload:**

```bash
cd backend && cargo run --release -- serve      # :3001
cd frontend && npm run dev                       # :5173, proxies /api to :3001
```

## The workspace

```
┌──────────────────────────────────────────────────────────────┐
│ SchemGen2   ● Server ok · Minecraft 1.21.8 · 181 blocks  ◐ ▣ │
├──────────────────────────────┬───────────────────────────────┤
│ [Model] [Blocks] [Split]     │ ▾ Size & shape                │
│                              │ ▾ Color                       │
│   one renderer, one camera   │ ▸ Lighting                    │
│                              │ ▾ Target version              │
│ Materials  1 240 stone …     │ ▾ Output                      │
├──────────────────────────────┴───────────────────────────────┤
│ + Add models   2 models · 1 done         Clear  [▶ Convert]  │
│ castle.glb   Done · 4 532 blocks · 128 × 58 × 80    ⧉ ⤓ 🗑   │
│ tower.glb    Converting 62 % ▓▓▓▓▓▓░░░ Matching blocks…      │
└──────────────────────────────────────────────────────────────┘
```

| Part | What it does |
|---|---|
| Header | The server's health (checked every 15 s), the target version and its palette size; the block palette; the theme (system, light, dark) |
| Viewport | **Model** is the glTF as its materials describe it — or *De-lit*, what the color sampler reads once the assumed light is removed, or *Rejection*, red where the highlight pass discounts samples. **Blocks** is the live preview. **Split** draws both from the same camera, the model left of a draggable divider and the blocks right of it. Drag the amber handle to aim the key light |
| Materials | Every block and how many — the preview's until the model is converted, then the final list — sortable, with counts in shulker boxes and stacks, and **Copy** as text |
| Settings | Drawn from `GET /api/schema`: its groups, fields, ranges, defaults, labels and help. Advanced fields wait behind a switch |
| Queue | One list for one model or a hundred. **Convert** converts every model not already converted with the current settings, in one batch; each row can be shown in the file manager, saved to the folder, downloaded, cancelled or removed. A finished model whose settings have since changed says so |

Drop files or folders anywhere on the page. Below 900 px wide the viewport
and the settings become two tabs.

**Shortcuts:** `Ctrl+Enter` (⌘+Enter on a Mac) converts; `1`, `2`, `3` switch
between Model, Blocks and Split.

Errors that do not belong to one row — an upload the server refused, a save
that failed — appear as toasts.

## Block textures

The Blocks view textures each block with the game's own art, which SchemGen2
cannot ship. The server serves it from the Minecraft already on this machine
(`GET /api/textures`): the newest release client jar a launcher keeps —
vanilla, CurseForge, Prism Launcher, MultiMC or the Modrinth App — or the jar,
resource pack or folder named with `serve --textures`. A texture it does not
have comes from a public copy on the web instead, and failing that the block is
drawn in its flat palette color.

## The de-light preview

The *De-lit* and *Rejection* views are a GLSL mirror of the color sampler's
lighting model ([`frontend/src/delightshader.js`](../frontend/src/delightshader.js)
against `backend/crates/core/src/sample.rs`). The preview only predicts the
conversion while those two agree — see
[docs/pipeline.md](pipeline.md#lighting-separation-de-light).

## Output folder

The server runs on the same machine as the browser, so finished schematics can
be written where you want them instead of going through the browser's download
folder.

In **Output → Save into folder**, enter an absolute path (`%APPDATA%` and `~`
expand, the folder is created if missing) or click one of the folders found on
this computer; the server checks the path as you type. Picking a folder inside
a launcher instance also sets the target to that instance's Minecraft version.
The folder, the target and the format are remembered in `localStorage`.

`/api/output-dir/suggestions` finds vanilla `.minecraft/schematics` and also
scans per-instance launcher folders, listing only instances that actually have a
`schematics` folder, existing paths first:

| Launcher | Scanned |
|---|---|
| CurseForge | `~/curseforge/minecraft/Instances/<pack>/schematics` |
| Prism Launcher | `%APPDATA%/PrismLauncher/instances/<pack>/.minecraft/schematics` |
| MultiMC | `~/MultiMC/instances/<pack>/.minecraft/schematics` |
| Modrinth App | `%APPDATA%/com.modrinth.theseus/profiles/<pack>/schematics` |

Every conversion lands there as `<schematic name>.<extension>` and can be shown
in the file manager. Changing the folder afterwards only copies the existing
file (**Save to folder**) — it never re-runs the conversion. Two identically
named models in one batch become `name.litematic` and `name-2.litematic`.

## Source map

```
frontend/src/
├── App.tsx                 Layout only
├── api.ts                  API client (v2)
├── types.ts                What the server sends
├── settings.ts             Schema → form state → the API's settings object
├── materials.ts            Material list maths and text
├── hooks/
│   ├── useServer.ts        Schema, health, palette
│   ├── useSettings.ts      Form state from the schema, remembered choices
│   ├── useConversion.ts    The queue: files, jobs, save, reveal
│   ├── useJobEvents.ts     Server-sent events for a set of jobs
│   ├── usePreview.ts       The debounced live preview
│   └── useTheme.ts, useShortcuts.ts, useMediaQuery.ts
├── viewport/
│   ├── engine.ts           One renderer: two scenes, one camera, split
│   ├── model.ts            glTF, de-lit materials, the light handle
│   ├── blocks.ts           Instanced blocks and their textures
│   └── modes.ts            The view tabs
├── components/             Header, Viewport, MaterialList, SettingsForm
│                           (+ fields/), JobQueue, PaletteDialog, DropOverlay
├── delightshader.js        GLSL mirror of the de-light model
└── lighting.js             Azimuth/elevation ↔ direction
```

New code is TypeScript; `allowJs` lets the older modules stay JavaScript.

## Checks

```bash
npm run lint          # ESLint, including the React Compiler rules
npm run typecheck     # tsc
npm test              # Vitest + Testing Library: settings, hooks, forms
npm run format:check  # Prettier
npm run test:e2e      # Playwright, against the real server (see below)
```

`test:e2e` starts `backend/target/release/schemgen2 serve` on its own port with
the built UI, uploads `backend/fixtures/textured.glb`, waits for the preview,
changes the target, converts and downloads the result. Build both first
(`cargo build --release`, `npm run build`); `SCHEMGEN_BINARY` points it at
another server build, `PLAYWRIGHT_CHROMIUM` at a browser to use. CI runs all of
it.

## Talking to the server

The UI uses API v2 ([docs/api.md](api.md)): `POST /api/jobs` with the files
and one `settings` JSON object, then follows each job over server-sent events
(`GET /api/jobs/{id}/events`, falling back to polling if a proxy breaks the
stream). `toApiSettings` in `settings.ts` turns the form into that object,
field by field from the schema. When the server runs with `--token`, open the
URL it logs (`…/?token=…`): the UI takes the token from the address bar, keeps
it for the tab, and sends it with every request.

## Adding a setting

1. `backend/crates/core/src/settings.rs` — the field on `Settings`, with its
   default, and its range in `normalized`
2. `backend/crates/core/src/schema.rs` — its description: type, range, group,
   label, help. A test fails until the two agree.
3. [docs/design.md](design.md) — its row in its section.

The web UI draws it from the schema with no change of its own, and the HTTP API
accepts it. A new field *type* needs a control in
`frontend/src/components/SettingsForm.tsx`. If the setting belongs on the CLI
too, add a flag in `settings_from_args` (`backend/crates/cli/src/args.rs`) and
to the help text there.

## Checking the shader

The de-light preview shader has no assertion to make, so it is checked by
compiling it against a real WebGL context: run the dev server and open
`/shader-check.html`, which reports PASS or the driver's error log.
