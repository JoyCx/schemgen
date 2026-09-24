# Design

What SchemGen2 looks like and how it behaves, written down once so that two
very different front ends — the web UI (React, three.js) and the in-game mod
(Minecraft's GUI) — can be the same product. When a screen and this document
disagree, one of them is wrong; fix whichever it is and keep them in step.

The settings sections come from `GET /api/schema`, so their order, labels,
defaults, ranges and help text are the server's, and the tables below are what
it serves today. Everything else here is a choice a front end makes, and both
make it the same way.

## Principles

1. **See it before you commit.** Every setting that changes the schematic
   updates a low-resolution preview a moment after it stops changing. Convert
   is the deliberate step, not the first one.
2. **One list.** One model or a hundred go through the same queue with the same
   settings. There is no single-file mode.
3. **The model and its blocks, side by side.** The same camera shows both, so a
   difference is a difference in blocks, not in viewpoint.
4. **Where the file goes is part of converting.** The schematic lands in the
   game's schematics folder, not in a downloads folder to be moved by hand.
5. **Say it once, plainly.** Help is a sentence under the control, not a
   tooltip to hunt for. Errors say what happened and what to do.

## Layout

```
┌──────────────────────────────────────────────────────────────┐
│ Header: name · health · target · palette size    [▣] [◐]    │
├──────────────────────────────┬───────────────────────────────┤
│ Viewport                     │ Settings                      │
│ [Model] [Blocks] [Split]     │ ▾ Size & shape                │
│  (+ Lit / De-lit / Rejection │ ▾ Color                       │
│   when the model shows)      │ ▸ Lighting                    │
│                              │ ▾ Target version              │
│ Materials                    │ ▾ Output                      │
│                              │ ▸ Advanced (behind a switch)  │
├──────────────────────────────┴───────────────────────────────┤
│ Queue: [+ Add models] summary      [Clear] [▶ Convert]      │
│  row per model: name · size · status · progress · actions    │
└──────────────────────────────────────────────────────────────┘
```

- The settings pane is 380 px wide; the viewport takes the rest.
- Below 900 px the viewport and the settings become two tabs, **Preview** and
  **Settings**; the queue stays underneath.
- **The mod** keeps the same three regions: the viewport is the world itself
  (the preview is a ghost in place, see [mod.md](mod.md)), the settings are a
  scrolling panel on the right, and the queue is the list on the left.

## Header

| Element | Content |
|---|---|
| Name | "SchemGen2" |
| Health | A dot and "Server ok · Minecraft 1.21.8 · 181 blocks" — the selected target and its palette size. "Connecting…" at first, "Server unreachable" (red) when a health check fails. Checked every 15 s. |
| Palette | Every block the target may use, with its color, filterable |
| Theme | Cycles *follow the system* → *light* → *dark* |

## Viewport

| Tab | Shortcut | Shows |
|---|---|---|
| Model | `1` | The glTF, in one of three shadings: **Lit** — as its materials describe it; **De-lit** — what the color sampler reads once the assumed key light is removed; **Rejection** — red where the highlight pass discounts samples |
| Blocks | `2` | The live preview: the pipeline at preview resolution, textured blocks |
| Split | `3` | Model left of a draggable divider, blocks right of it, same camera |

- **The key light** is an amber handle orbiting the model. Dragging it sets
  *Lighting → Key light*; the sliders there move it too. It lives in the model
  view only.
- A **camera reset** button frames the model again; a **turn** toggle rotates it
  slowly. Framing keeps the model and the light handle in view, portrait or
  landscape.
- While a preview builds: "Building the block preview…" with **Cancel**. A
  preview capped below the requested size says so: "Preview at 256 blocks; the
  conversion uses the full size." (Previews are built at most 256 blocks
  long.)
- Block textures come from the player's own Minecraft; a block with none is
  drawn in its palette color, never black.

## Materials

Every block and how many, under the viewport.

- Header: **Materials**, a badge — *preview* (grey) until the selected model is
  converted with the current settings, then *final* (amber) — and
  "4 532 blocks · 27 kinds".
- Rows: color swatch, block name in sentence case ("Waxed cut copper"), count
  with thousands separators, and the count in shulker boxes and stacks
  ("2 sb + 3 st + 12"; sb = 1 728, st = 64).
- Sorted by count, most first; one button toggles alphabetical.
- **Copy** puts the list on the clipboard as aligned text:

  ```
  Materials for castle.litematic (Minecraft 1.21.8)
  1,240  Stone       19 st + 24
      7  Oak planks  7
  1,247 blocks, 2 kinds (sb = shulker box, st = stack of 64)
  ```

## Settings

Sections in schema order, each with a chevron and, when open, a reset button
("Reset color to the defaults"). *Size & shape*, *Color*, *Target version* and
*Output* start open; *Lighting* and *Advanced* start closed. Fields marked
*advanced* appear only with **Show advanced settings** on. A field with a
condition appears only while it holds.

Controls by type:

| Type | Control |
|---|---|
| `int`, `float` with a range | Label and an exact-value box on one line, a slider over the usual range below it, the unit after the box. The box takes values beyond the slider, up to the field's full range. |
| `float`, nullable, no range | A text box; empty (or 0) is its placeholder's meaning, "Auto" |
| `bool` | A switch, label to its right |
| `choice`, `block` | A dropdown. Targets read "1.21.8 — 181 blocks (default)" |
| `direction` | Azimuth and elevation sliders in degrees, plus the handle in the viewport |
| `folder` | A switch ("Save into folder"); when on, a path box that the server checks as you type ("Saving to …", "… will be created", or the problem), an open-in-file-manager button, and chips for the schematics folders found on this computer ("CurseForge · ATM9 · 1.20.1"). A chip inside a launcher instance also sets the target to that instance's version. |
| `text` | A text box with its placeholder |

Every field shows its help text under it. Numbers show to their step: 0.32,
not 0.3199999928.

### Size & shape (`size`)

| Label | Key | Control | Default | Range | Shown |
|---|---|---|---|---|---|
| Max size | `max_size` | slider + number | 128 blocks | 1–2048 (slider 8–512) | always |
| Voxel size | `voxel_size` | text box | Auto | > 0 | advanced |

### Color (`color`)

| Label | Key | Control | Default | Range | Shown |
|---|---|---|---|---|---|
| Color sampling | `color_sampling` | switch | on | | always |
| Dithering | `dither` | switch | on | | when color sampling is on |
| Brightness | `brightness` | slider + number | 0 | −1–1 (slider −0.5–0.5) | when color sampling is on |
| Contrast | `contrast` | slider + number | 1 | 0–3 (slider 0.5–2) | when color sampling is on |
| Saturation | `saturation` | slider + number | 1 | 0–3 (slider 0–2) | when color sampling is on |
| Block | `default_block` | dropdown | White concrete | | when color sampling is off |

### Lighting (`lighting`)

*Separates lighting baked into the model's textures from its real colors.
Leave it alone unless the model looks shiny or shaded in its texture.*

| Label | Key | Control | Default | Range | Shown |
|---|---|---|---|---|---|
| Key light | `light_dir` | azimuth + elevation sliders, light handle | 41° / 58° | | when color sampling is on |
| De-light | `delight` | slider + number | 0 | 0–1 | when color sampling is on |
| Assumed gloss | `light_gloss` | slider + number | 0.5 | 0–1 | when color sampling is on |
| Ambient | `light_ambient` | slider + number | 0.32 | 0–1 | when color sampling is on |
| Specular gain | `specular` | slider + number | 1.1 | 0–4 (slider 0–2) | when color sampling is on |
| Highlight rejection | `highlight_rejection` | slider + number | 0.75 | 0–1 | when color sampling is on |
| Blown-voxel recovery | `highlight_recovery` | slider + number | 1 | 0–1 | when color sampling is on |

### Target version (`target`)

*The Minecraft version the schematic is for. Only blocks that exist in that
version are used.*

| Label | Key | Control | Default | Range | Shown |
|---|---|---|---|---|---|
| Minecraft version | `target` | dropdown | 1.21.8 | 1.16.5 … 26.3 | always |

### Output (`output`)

| Label | Key | Control | Default | Range | Shown |
|---|---|---|---|---|---|
| File format | `format` | dropdown | Litematica (.litematic) | | always |
| Schematic name | `schematic_name` | text box | Model file name | | always |
| Save into folder | `output_dir` | switch + path + folder chips | Only keep it on the server | | always |

### Advanced (`advanced`)

| Label | Key | Control | Default | Range | Shown |
|---|---|---|---|---|---|
| Parallel conversions | `threads` | slider + number | 4 | 1–64 | advanced |
| Memory budget | `ram_limit` | slider + number | 4 GB | 0.5–256 (slider 0.5–32) | advanced |

**Remembered** between visits: the output folder and whether it is used, the
target, the format. Everything else starts from the defaults.

## Queue

- **Add models** opens a file picker (`.glb`, `.gltf`, several at once);
  dropping files or folders anywhere does the same. A file already in the list
  is not added twice; anything else is skipped with a note.
- The summary reads "2 models · 1 done", or, when empty, "Drop .glb / .gltf
  files or a folder anywhere on the page."
- **Convert** (`Ctrl+Enter`, ⌘+Enter on a Mac) converts, in one batch, every
  model that is ready, failed, cancelled, or finished with other settings. It
  reads "Convert 3" when there is more than one, and is disabled when there is
  nothing to do or the server is unreachable.
- **Save all to folder** appears when a folder is set and something finished;
  **Clear** empties the list, cancelling what runs.

Each row: status icon, file name (selects it), size, a status pill, a progress
bar while it runs, and one line of detail.

| Status | Pill | Detail line | Actions |
|---|---|---|---|
| Ready | Ready | "Ready" | Remove |
| Uploading / Queued / Converting | Uploading · Queued · Converting 62% (amber) | The server's stage message ("Matching blocks (CIEDE2000)…") | Cancel |
| Done | Done (green) | "Saved to …" or "4 532 blocks · 128 × 58 × 80 · Minecraft 1.21.8" | Show in file manager, Save to folder (when a folder is set and it was not saved), Download, Remove |
| Failed | Failed (red) | The server's error | Remove |
| Cancelled | Cancelled | "Cancelled" | Remove |

A finished model whose settings have since changed also shows a *Settings
changed* pill, and Convert includes it again. The selected row has an amber
edge; the viewport and the material list follow it.

## Feedback

- **Toasts**, bottom right, for what belongs to no single control: a failed
  upload, a conversion that failed ("castle.glb: …"), a save or reveal that did
  not work, a model the viewer cannot draw. Errors stay 9 s, confirmations 4 s;
  each can be dismissed. The same message twice is one toast.
- Inline text for what does belong to a control: a folder that cannot be used,
  a preview that failed.
- Wording: sentence case; say what happened, then what to do; no exclamation
  marks; units after numbers ("128 blocks", "4 GB").

## Look

| Token | Light | Dark | Used for |
|---|---|---|---|
| `--bg` | `#f4f3ef` | `#121316` | Page |
| `--surface` | `#ffffff` | `#1b1d22` | Panels |
| `--text` | `#1a1a1a` | `#ecebe7` | Text; the primary button |
| `--text-muted` | `#77756f` | `#9a9892` | Help, secondary text |
| `--highlight` | `#e39a14` | `#f0a92a` | Selection, sliders, progress, the light handle, the split divider |
| `--ok` | `#2f8f4e` | `#4cbf6f` | Done |
| `--danger` | `#c2412d` | `#f07058` | Failed, errors |

- Near-monochrome, with amber as the one accent: whatever is selected, active
  or draggable.
- Radius 12 px for panels, 8 px for controls. System UI font; monospace for
  numbers in boxes and counts.
- Icons from [lucide](https://lucide.dev) — `Box` Model, `Boxes` Blocks,
  `Columns2` Split, `Play` Convert, `FolderOpen` show in file manager,
  `FolderInput` save to folder, `Download`, `Ban` cancel, `Trash2` remove,
  `Copy` copy, `Palette`, `Sun`/`Moon`/`Monitor` theme. No emoji.
- Motion is short (≤ 0.3 s) and off under *reduce motion*.

**The mod** maps the tokens onto Minecraft's GUI: panels on the vanilla dark
background, amber for selection and progress, green and red for done and
failed, the same labels and the same order.
