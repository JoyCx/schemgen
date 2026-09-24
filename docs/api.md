# HTTP API

`schemgen2 serve` starts it (port 3001 by default). The web app and the
Minecraft mod are both clients of this API and have no other way in.

```bash
cd backend && cargo run --release -- serve
curl http://localhost:3001/api/health
```

Two versions are mounted side by side. **v2** (this page) is what the web app
and the mod use. **v1**, the flat-field API earlier clients were written
against, still works for this release — see [v1](#v1-legacy) at the end.

## A conversion in four calls

```bash
# 1. Is it up?
curl -s localhost:3001/api/health

# 2. Upload a model; every setting is optional.
curl -s -F file=@castle.glb -F 'settings={"max_size": 96, "target": "1.20.4"}' \
     localhost:3001/api/jobs
# {"job_id":"bf61…","jobs":[{"job_id":"bf61…","filename":"castle.glb","name":"castle"}], …}

# 3. Follow it (server-sent events) — or poll GET /api/jobs/bf61…
curl -sN localhost:3001/api/jobs/bf61…/events

# 4. Fetch the schematic.
curl -sOJ localhost:3001/api/jobs/bf61…/download
```

That is all any client needs. A client that renders a settings form reads
[`GET /api/schema`](#get-apischema) first, so it never hard-codes a field.

## Routes

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/health` | Liveness, version, palette size, default target |
| `GET` | `/api/schema` | Every setting with type, range, default, group, label and help; targets; formats |
| `POST` | `/api/jobs` | Upload one or more models + settings → jobs |
| `GET` | `/api/jobs` | All jobs the server remembers, newest first |
| `GET` | `/api/jobs/{id}` | One job |
| `GET` | `/api/jobs/{id}/events` | Server-sent events for one job |
| `DELETE` | `/api/jobs/{id}` | Cancel a job, or forget a finished one and delete its files |
| `GET` | `/api/jobs/{id}/download` | The finished schematic |
| `GET` | `/api/jobs/{id}/thumbnail.png` | 256 × 256 isometric render of the result |
| `POST` | `/api/jobs/{id}/save` | Copy the finished schematic into a folder |
| `POST` | `/api/jobs/{id}/reveal` | Show it in the host's file manager |
| `POST` | `/api/preview` | Convert without writing a file; returns the blocks |
| `GET` | `/api/palette` | Blocks a conversion may choose: `{id: [r, g, b]}`; `?target=` for another version |
| `GET` | `/api/system` | Host OS and what its file manager is called |
| `POST` | `/api/output-dir/check` | Validate a folder without creating it |
| `GET` | `/api/output-dir/suggestions` | Likely schematics folders on this machine |
| `POST` | `/api/reveal-folder` | Open a folder in the host's file manager |

Errors are JSON — `{"error": "…"}` — with a status that says whose fault it
was: 400 for a bad request (the message names the field), 404 for an unknown
job, 409 for an action on a job that has not finished, 413 for an upload over
the size limit, 422 for a model that could not be converted.

### `GET /api/health`

```json
{
  "status": "ok",
  "name": "schemgen2",
  "version": "2.1.0",
  "api": 2,
  "palette_entries": 181,
  "palette_blocks": 181,
  "target": "1.21.8",
  "data_version": 4440,
  "schematic_version": 7,
  "os": "windows",
  "auth": false,
  "voxelizer": "rust"
}
```

Cheap and side-effect free, and the one route that never needs the token, so
a client can check the server is up before it has credentials. `auth` says
whether the other routes do. `voxelizer` is `python` only when a build with the
`python-voxelizer` feature was started with `--voxelizer python`.

### `GET /api/schema`

```json
{
  "version": "2.1.0",
  "groups": [{ "key": "size", "label": "Size & shape" }, …],
  "fields": [
    {
      "key": "max_size", "type": "int", "group": "size",
      "label": "Max size", "help": "Length of the longest side of the result, in blocks.",
      "default": 128, "scope": "conversion",
      "min": 1, "max": 2048, "step": 1, "slider": [8, 512], "unit": "blocks"
    },
    {
      "key": "dither", "type": "bool", "group": "color", "label": "Dithering", …,
      "when": { "field": "color_sampling", "equals": true }
    },
    …
  ],
  "targets": [{ "id": "26.3", "data_version": 5023, "schematic_version": 7, "blocks": 181 }, …],
  "default_target": "1.21.8",
  "formats": [{ "id": "litematic", "label": "Litematica (.litematic)", "extension": "litematic" }, …],
  "palette": { "entries": 181, "blocks": 181 },
  "limits": { "max_upload_bytes": 1073741824 }
}
```

Groups come in display order and fields in display order within their group.
A field's `type` is one of:

| `type` | Value | Extra keys |
|---|---|---|
| `int`, `float` | number | `min`, `max`, `step`; `slider` when a slider should span less than `min`…`max`; `unit` |
| `bool` | boolean | |
| `text` | string | `placeholder` |
| `block` | block ID, e.g. `minecraft:stone` | `choices` as suggestions |
| `choice` | one of `choices[].value` | `choices` |
| `direction` | `[x, y, z]`, model space, Y up | UIs usually edit it as azimuth + elevation |
| `folder` | absolute path on the server's machine | |

`nullable: true` means `null` is a value (`voxel_size: null` derives the size
from `max_size`). `advanced: true` fields are tucked away by default in both
UIs. `when` shows or enables a field only while another has a given value.
`scope` is `conversion` for settings that change the blocks and `job` for the
two that only change how a job runs (`output_dir`, `threads`).

Defaults are the server's own: `schemgen2 serve --target 1.20.4` reports
`"default": "1.20.4"` for `target`. Each target's `blocks` is the size of its
palette — older versions have fewer blocks (see [versions.md](versions.md)).
`formats` are the values the `format` setting takes: `litematic`, `schem`
(Sponge v2), `schem-v3` and `nbt` (vanilla structure).

### `POST /api/jobs`

`multipart/form-data` with:

- one or more model parts — any field name, but each must carry a file name
  ending in `.glb` or `.gltf` (other files are skipped and listed in
  `skipped`);
- optionally one `settings` part: a JSON object with any of the fields
  `GET /api/schema` lists. Everything left out takes the server's default.

```bash
curl -F files=@a.glb -F files=@b.glb \
     -F 'settings={"max_size": 64, "delight": 0.4, "output_dir": "~/.minecraft/schematics", "threads": 2}' \
     localhost:3001/api/jobs
```

`202 Accepted`:

```json
{
  "job_id": "bf61ecd2-…",
  "jobs": [
    { "job_id": "bf61ecd2-…", "filename": "a.glb", "name": "a" },
    { "job_id": "0c9a77e1-…", "filename": "b.glb", "name": "b" }
  ],
  "skipped": [],
  "ignored": [],
  "output_dir": "/home/me/.minecraft/schematics"
}
```

- `job_id` is the first job's id — the whole answer for a single model.
- `ignored` lists settings keys the server did not recognize. They are not an
  error, but a typo shows up here instead of silently doing nothing.
- Values outside a field's range are clamped. A value that cannot mean
  anything — a string for a number, a zero-length light direction, an unknown
  target — is a 400 naming the field: `settings.max_size: invalid type: …`.
- `output_dir` is resolved, created and checked for write access *before* any
  conversion starts, so an unusable folder fails the request instead of
  surfacing minutes later. `~` and `%VAR%` expand.
- `schematic_name` names the schematic when one model is uploaded; in a batch
  each model keeps its own file name, and two with the same name become
  `name` and `name-2`.
- `threads` (default 4) caps how many of *this request's* models convert at
  once; the server also caps conversions across all requests
  (`--max-jobs`, default: the CPU count). Waiting jobs are `queued`.

### A job

`GET /api/jobs/{id}`, each item of `GET /api/jobs`, and the data of every
event:

```json
{
  "id": "bf61ecd2-…",
  "status": "running",
  "progress": 42.5,
  "stage": "voxelize",
  "message": "Sampling 7952 voxels | faces=2572 | 24 samples/voxel",
  "input_name": "castle.glb",
  "name": "castle",
  "download_name": "castle.litematic",
  "format": "litematic",
  "target": "1.20.4",
  "created_ms": 1727150000000,
  "finished_ms": null,
  "error": null,
  "result": null,
  "saved_path": null,
  "save_error": null,
  "links": {
    "download": "/api/jobs/bf61ecd2-…/download",
    "thumbnail": "/api/jobs/bf61ecd2-…/thumbnail.png",
    "events": "/api/jobs/bf61ecd2-…/events"
  }
}
```

`status` is `queued`, `running`, `done`, `error` or `cancelled`. `stage` is
`queued`, `start`, `voxelize`, `adjust`, `dither`, `match`, `write`,
`thumbnail`, then `done`, `error` or `cancelled`. `progress` runs 0–100 and
stays below 100 until the file is written *and* delivered, so a client that
reacts to `done` always finds the file in place.

When `done`, `result` is:

```json
{
  "blocks": 7952,
  "unique_blocks": 34,
  "dims": [64, 29, 40],
  "seconds": 0.9,
  "materials": [{ "name": "minecraft:gold_block", "count": 2210 }, …],
  "target": "1.20.4",
  "data_version": 3700
}
```

`materials` is the material list — every block and how many, most used
first. `saved_path` / `save_error` describe the copy into `output_dir`; a
failed copy does not fail the conversion, and the file can still be
downloaded or saved elsewhere.

### `GET /api/jobs/{id}/events`

`text/event-stream`. The stream starts with the job's current state — so a
client that connects late or reconnects loses nothing — then sends one event
per change:

```
retry: 3000

event: progress
data: {"id":"bf61…","status":"running","progress":27.0,"stage":"voxelize",…}

event: done
data: {"id":"bf61…","status":"done","progress":100.0,"result":{…},…}
```

Event names are `progress`, then exactly one of `done`, `error` or
`cancelled`, after which the stream closes. Rapid changes may be merged; the
latest state always arrives. A `: keep-alive` comment goes out every 15 s.

Browsers' `EventSource` reconnects on its own when a stream ends, so close it
on the final event. It cannot send headers either — with a token, pass it as
`?token=`.

### `DELETE /api/jobs/{id}`

- On a `queued` or `running` job: cancels it — `202` with the job. A queued
  job is `cancelled` at once; a running one stops at the pipeline's next
  checkpoint (its voxelizer process is killed) and reports `cancelled` then.
- On a finished job: forgets it and deletes its files — `204`.

### `GET /api/jobs/{id}/download`, `/thumbnail.png`

The schematic, named for its format (`castle.litematic`, `castle.schem`,
`castle.nbt`) in `Content-Disposition: attachment`, and a
256 × 256 transparent PNG of it, drawn isometrically. Both are `409` until
the job is `done`.

### `POST /api/jobs/{id}/save`

```json
{ "path": "%APPDATA%/.minecraft/schematics" }
```

Copies the finished schematic there — for when the folder is picked after the
conversion — without converting again. `200 {"saved_path": "…"}`.

### `POST /api/jobs/{id}/reveal`

Opens the host's file manager with the schematic selected: the copy in the
output folder when there is one, else the server's own. The server and its
browser are on the same machine, so this beats downloading a second copy.

### `POST /api/preview`

Same upload as `POST /api/jobs` (one model), but synchronous: the pipeline
runs without writing a file and the blocks come back.

```json
{
  "dims": [48, 34, 34],
  "origin": [-1.0, -1.0, -1.0],
  "pitch": 0.0583,
  "palette": ["minecraft:blue_concrete", "minecraft:yellow_terracotta", …],
  "count": 5213,
  "blocks": "AAAAAA…",
  "materials": [{ "name": "minecraft:blue_concrete", "count": 1807 }, …],
  "target": "1.21.8",
  "capped": false,
  "seconds": 0.8
}
```

`blocks` is base64 of little-endian 32-bit integers, four per block:
`x, y, z, palette index`. In a browser that is

```js
const bytes = Uint8Array.from(atob(data.blocks), (c) => c.charCodeAt(0))
const blocks = new Int32Array(bytes.buffer) // [x0, y0, z0, i0, x1, …]
```

— about a fifth of the size of one JSON object per block. `origin` and
`pitch` place the grid in the model's own coordinates: block `(x, y, z)`
spans `origin + (x, y, z) · pitch` to `origin + (x+1, y+1, z+1) · pitch`, which
is what lets a viewer overlay blocks on the model. Previews above 256 blocks
are drawn at 256 (`capped: true`). A preview whose request is dropped stops
converting.

## Authentication and exposure

The server belongs to the machine it runs on:

- It binds `127.0.0.1` unless `--host` says otherwise.
- Requests must be addressed to `localhost`, `127.0.0.1` or `[::1]` (or a name
  given with `--allow-host`): a web page cannot reach it through DNS
  rebinding.
- A browser request from another origin is refused, so a web page cannot post
  a conversion or open a file manager behind your back.
- With `--token <T>` (or `SCHEMGEN_TOKEN`, or `--token-file`, which creates a
  random token if the file does not exist yet), every route except
  `/api/health` needs `Authorization: Bearer <T>`, or `?token=<T>` where a
  header is impossible (`EventSource`, links, images). The server logs a URL
  with the token for the web UI, which picks it up from the address bar.

A launcher that starts the server for itself — the Minecraft mod does —
passes a token, `--port 0` and `--exit-with-stdin`, then reads the one line
the server prints to stdout once it accepts connections:

```
listening http://127.0.0.1:50731
```

With `--exit-with-stdin` the server stops when its standard input closes, so
it cannot outlive a launcher that crashes.

## Jobs and files

Uploads and results live in the work folder (`--work-dir`; default
`%LOCALAPPDATA%\schemgen2`, `~/Library/Caches/schemgen2` or
`~/.cache/schemgen2`). An upload is deleted as soon as its conversion ends.
Finished jobs and their files are forgotten after 24 hours (`--job-ttl <hours>`,
0 to keep them until restart); files left there by earlier runs are swept the
same way. Jobs are kept in memory, so a restart forgets them.

Limits: 1 GiB per model file; `max_size` at most 2048.

### `GET /api/output-dir/suggestions`

```json
{
  "suggestions": [
    { "path": "C:\\Users\\me\\curseforge\\minecraft\\Instances\\ATM9\\schematics", "exists": true,
      "instance": { "name": "ATM9", "launcher": "CurseForge", "mc_version": "1.20.1", "target": "1.20.1" } },
    { "path": "C:\\Users\\me\\AppData\\Roaming\\.minecraft\\schematics", "exists": false, "instance": null }
  ]
}
```

Existing folders first. A folder inside a launcher instance (CurseForge, Prism
Launcher, MultiMC, Modrinth App) carries the instance, the Minecraft version it
runs when the launcher records one, and the target that suits it.

## v1 (legacy)

Kept for one release so existing scripts keep working. They share v2's jobs:
a job started through v1 shows up in `GET /api/jobs` and vice versa.

| Method | Path | Instead, use |
|---|---|---|
| `POST` | `/api/convert` | `POST /api/jobs` |
| `POST` | `/api/convert-batch` | `POST /api/jobs` with several files |
| `GET` | `/api/progress/{id}` | `GET /api/jobs/{id}` or `/events` |
| `GET` | `/api/download/{id}` | `GET /api/jobs/{id}/download` |
| `POST` | `/api/save/{id}` | `POST /api/jobs/{id}/save` |
| `POST` | `/api/reveal/{id}` | `POST /api/jobs/{id}/reveal` |

v1 sends each setting as its own multipart text field — `max_size`,
`voxel_size`, `ram_limit`, `dither`, `color_sampling`, `brightness`,
`contrast`, `saturation`, `no_color_block` (`white` or `netherrack`),
`schematic_name`, `output_dir`, `auto_save`, `light_dir` (`x,y,z`),
`light_ambient`, `light_gloss`, `specular`, `highlight_rejection`,
`highlight_recovery`, `delight`, and `threads` for a batch. They are now all
optional, and a value that does not parse falls back to its default.
`/api/progress` reports `running`, `done` or `error` (a queued job reads as
running, a cancelled one as an error). A `POST /api/preview` that sends
`max_size` as its own field gets the v1 answer:
`{"grid": [x, y, z], "blocks": [{"x", "y", "z", "name"}, …]}`.
