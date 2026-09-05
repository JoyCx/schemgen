# HTTP API

Started with `schemgen2 serve` (port 3001 by default, `--port` or `PORT` to
change it). The web app is a client of this API and has no other way in.

```bash
cd backend && cargo run --release -- serve
curl http://localhost:3001/api/health
```

## Routes

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/health` | Liveness + version, palette size, stamped data version |
| `GET` | `/api/system` | Host OS and file-manager name (for button labels) |
| `GET` | `/api/palette` | Block palette: `{block_id: [r, g, b]}` |
| `POST` | `/api/preview` | Upload GLB + settings → block list for the in-browser preview (synchronous, no job) |
| `POST` | `/api/convert` | Upload GLB + settings → `{job_id}` |
| `POST` | `/api/convert-batch` | Upload many GLBs + settings → one `job_id` per file |
| `GET` | `/api/progress/{job_id}` | Poll job status |
| `GET` | `/api/download/{job_id}` | Download the finished `.litematic` |
| `POST` | `/api/save/{job_id}` | Copy a finished schematic into a folder (no re-conversion) |
| `POST` | `/api/reveal/{job_id}` | Show the finished schematic in Explorer/Finder |
| `POST` | `/api/output-dir/check` | Validate/create a folder chosen in the UI |
| `GET` | `/api/output-dir/suggestions` | Likely Litematica schematic folders on this machine |
| `POST` | `/api/reveal-folder` | Open a folder in the host file manager |

### `GET /api/health`

```json
{
  "status": "ok",
  "name": "schemgen2",
  "version": "2.0.0",
  "palette_entries": 181,
  "palette_blocks": 181,
  "data_version": 4440,
  "schematic_version": 6,
  "os": "windows"
}
```

Cheap and side-effect free. The mod pings it before offering to convert, so a
stopped server is reported up front instead of surfacing as a failed upload
minutes later.

### `POST /api/convert`

`multipart/form-data`. These fields are **mandatory** — omitting any one fails
the request with `Required field is missing: <name>`, even when the value is
empty:

`file` (or `files` for batch), `max_size`, `voxel_size`, `ram_limit`,
`dither`, `color_sampling`, `brightness`, `contrast`, `saturation`,
`no_color_block`, plus `schematic_name` (single) / `threads` (batch).

Send `voxel_size=` (empty) to derive the voxel pitch from `max_size`.

Optional: `output_dir`, `auto_save`, `light_dir`, `light_ambient`,
`light_gloss`, `specular`, `highlight_rejection`, `highlight_recovery`,
`delight`.

```bash
curl -X POST http://localhost:3001/api/convert \
  -F "file=@model.glb" \
  -F "max_size=96" -F "voxel_size=" -F "ram_limit=4" \
  -F "dither=true" -F "color_sampling=true" \
  -F "brightness=0" -F "contrast=1" -F "saturation=1" \
  -F "no_color_block=white" -F "schematic_name=my-build"
# {"job_id":"bf61ecd2-..."}
```

Returns immediately; the conversion runs in the background.

### `GET /api/progress/{job_id}`

```json
{
  "status": "running",
  "progress": 60.0,
  "message": "Matching blocks (CIEDE2000)...",
  "download_name": "my-build.litematic",
  "saved_path": null,
  "save_error": null
}
```

`status` is `running`, `done` or `error`. While a job runs, `progress` tracks
the real pipeline stage and stops at 99; the step to 100 is taken only once the
file is written and delivered, so a client that reacts to 100% always finds the
file in place. On `error`, `message` is the reason.

`saved_path` / `save_error` describe the optional copy into `output_dir` — a
failed copy does not fail the conversion, and the file is still downloadable.

### `GET /api/download/{job_id}`

The gzipped NBT `.litematic`, with `Content-Disposition` naming it after the
schematic. Jobs live in memory, so a server restart loses the mapping (the file
itself stays in `backend/outputs/`).

## Limits

- Single upload: 256 MB per file; total payload 4 GiB.
- Only `.glb` / `.gltf` are accepted; anything else is a 400.
- Jobs are in-memory and not persisted across restarts.

## Writing another client

A conversion is four calls: `health` → `convert` → `progress` until it finishes
→ `download`. That is all the web app does, and it is all any other client
needs to do — the upload is an ordinary `multipart/form-data` POST.

There is no authentication and no CORS handling: the server is meant to run on
the same machine as its clients. Do not expose it to a network you do not
control — `/api/reveal-folder` opens a file manager on the host, and
`output_dir` writes files where it is told.
