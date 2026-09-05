# Web app

A Vite + React front end for the same [API](api.md): drop a model in, orbit it
in 3D, tune the settings against a live preview, convert, and have the file
written straight into your schematics folder.

## Run

**Production — one server, one port:**

```bash
cd frontend && npm install && npm run build
cd ../backend && cargo run --release -- serve
# http://localhost:3001
```

`serve` mounts `frontend/dist` at `/` when it exists, so the UI and the API are
the same origin and nothing needs a proxy.

**Development — hot reload:**

```bash
cd backend && cargo run --release -- serve      # :3001
cd frontend && npm run dev                       # :5173, proxies /api to :3001
```

On Windows, [`run_server.bat`](../run_server.bat) does the dev pair for you:
checks Python/trimesh, builds the backend if needed, installs npm dependencies
if needed, starts both and opens the browser.

## What the UI adds over the CLI

| | Why it only exists here |
|---|---|
| 3D model preview | Orbit the GLB and drag the amber handle to set the key light direction |
| Live block preview | `/api/preview` converts at low resolution so you see block choices before committing |
| Preview modes | *De-lit albedo* renders exactly what the sampler will read; *Rejection mask* paints red over surface the highlight pass is discounting |
| Palette grid | Every block the palette may choose, with its color |
| Batch panel | Queue many models with shared settings and a thread count |
| Folder picker | Suggests the Litematica folders it finds on this machine |

The de-light preview is a GLSL mirror of the Python sampler
([`frontend/src/delightshader.js`](../frontend/src/delightshader.js) against
`backend/scripts/sample_colors.py`). The preview only predicts the conversion
while those two agree — see [docs/pipeline.md](pipeline.md#lighting-separation-de-light).

## Output folder

The server runs on the same machine as the browser, so finished schematics can
be written where you want them instead of going through the browser's download
folder.

In **Conversion Settings → Save .litematic straight into a folder**, enter an
absolute path (`%APPDATA%` and `~` expand, the folder is created if missing) or
click a suggestion. The choice is remembered in `localStorage`.

`/api/output-dir/suggestions` finds vanilla `.minecraft/schematics` and also
scans per-instance launcher folders, listing only instances that actually have a
`schematics` folder, existing paths first:

| Launcher | Scanned |
|---|---|
| CurseForge | `~/curseforge/minecraft/Instances/<pack>/schematics` |
| Prism Launcher | `%APPDATA%/PrismLauncher/instances/<pack>/.minecraft/schematics` |
| MultiMC | `~/MultiMC/instances/<pack>/.minecraft/schematics` |
| Modrinth App | `%APPDATA%/com.modrinth.theseus/profiles/<pack>/schematics` |

Most Litematica users run through a launcher, so suggesting only
`%APPDATA%\.minecraft\schematics` misses the folder they actually use.

Every conversion, single or batch, lands there as `<schematic name>.litematic`
and gets a **Show in Explorer** button. Changing the folder afterwards only
copies the existing file — it never re-runs the conversion. Two identically
named models in one batch become `name.litematic` and `name-2.litematic`.

## Source map

```
frontend/src/
├── app.jsx                  Main app and state
├── api.js                   API client
├── delightshader.js         GLSL mirror of the de-light model
├── lighting.js              Shared light-direction math
├── app.css                  Styling
└── components/
    ├── dropzone.jsx         Drag-and-drop upload
    ├── settings.jsx         Conversion settings
    ├── progresspanel.jsx    Live progress
    ├── resultpanel.jsx      Download / save / reveal
    ├── palettegrid.jsx      Block palette
    ├── BatchPanel.jsx       Multi-file queue
    ├── modelpreview.jsx     3D GLB viewer + light handle
    └── minecraftpreview.jsx Voxel/block preview
```

## Adding a setting

A setting has to be added in three places or it is silently dropped:

1. `frontend/src/components/settings.jsx` — the control
2. `frontend/src/api.js` — put it in the multipart body
3. `backend/src/api.rs` — the `ConvertForm` / `BatchConvertForm` field, and
   `options_from_fields`

If it also belongs on the CLI, add it to `options_from_args` in
`backend/src/cli.rs` and to the help text there.

## Checking the shader

The de-light preview shader has no assertion to make, so it is checked by
compiling it against a real WebGL context: run the dev server and open
`/shader-check.html`, which reports PASS or the driver's error log.
