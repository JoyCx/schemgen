# Minecraft versions

A schematic is made *for* a Minecraft version — its **target**. The target
decides two numbers stamped into the file.

## What the numbers mean

**`MinecraftDataVersion`** is the game version the file claims to come from.
When Litematica loads a schematic stamped *older* than the running game, it
upgrades the block palette through Minecraft's DataFixer; one stamped more
than a few releases *newer* than the game gets a warning. It is the number
that has to be right.

**Litematica's schematic `Version`** is the file format's own version:

| Litematica for | Writes | Reads |
|---|---|---|
| 1.18.x, 1.19.x, 1.20.x | `Version` 6 | 1–6 |
| 1.21.x and the builds for 26.x | `Version` 7 | 1–7 |

(Checked against `LitematicaSchematic.java` in maruohon's 1.18–1.20 branches and
sakura-ryoko's 1.21–26.x ones.) Version 7 only changed how sleeping entities'
positions are stored, and block-only schematics read identically under both,
so 6 would load everywhere from 1.18 on. SchemGen2 writes 7 for targets from
1.21 anyway, because that is what Litematica itself writes there.

## Targets

| Target | Data version | Litematica `Version` |
|---|---|---|
| 1.16.5 | 2586 | 6 |
| 1.17.1 | 2730 | 6 |
| 1.18.2 | 2975 | 6 |
| 1.19.4 | 3337 | 6 |
| 1.20.1 | 3465 | 6 |
| 1.20.4 | 3700 | 6 |
| 1.20.6 | 3839 | 6 |
| 1.21.1 | 3955 | 7 |
| 1.21.4 | 4189 | 7 |
| 1.21.5 | 4325 | 7 |
| **1.21.8** | **4440** (default) | 7 |
| 1.21.10 | 4556 | 7 |
| 1.21.11 | 4671 | 7 |
| 26.1 | 4786 | 7 |
| 26.2 | 4903 | 7 |
| 26.3 | 5023 | 7 |

Data versions are from PrismarineJS `minecraft-data`
(`data/pc/common/protocolVersions.json`). Mojang retired `1.x` numbering after
1.21.11; 26.1 is its successor. `schemgen2 targets` prints the table the binary
was built with.

Pick the version you play. A schematic for 1.21.4 loads in 1.21.4 and every
later version; one for 1.21.8 may warn in 1.21.4.

**Floor:** 1.16.5 — post-flattening block names, and the oldest version
Litematica is still meaningfully used on. Pre-1.13 numeric block IDs are out of
scope. **Ceiling:** the newest row.

For a version without a row, `--data-version <N>` (or a bare number as the
`target` setting) stamps any data version from 1.16.5's on. A number that
belongs to a named target *is* that target.

## Choosing a target

| Where | How |
|---|---|
| CLI | `schemgen2 convert model.glb --target 1.20.4` |
| API | `"target": "1.20.4"` in the `settings` JSON — see [api.md](api.md) |
| Server default | `schemgen2 serve --target 1.20.4`, or `SCHEMGEN_DATA_VERSION` |

## Adding a version

1. Look up its data version in `minecraft-data`'s `protocolVersions.json`.
2. Add a row to `TARGETS` in `backend/crates/core/src/targets.rs` (the
   Litematica version follows from the data version).
3. Update the table above.
