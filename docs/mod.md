# Fabric mod

An in-game front end for the converter: pick a model, watch it convert, and the
finished schematic lands in this instance's `schematics` folder where Litematica
finds it.

- **Minecraft** 1.21.11, **Fabric Loader** 0.19.3, **Fabric API** 0.141.6, **Java** 21+
- **Client-only.** It never talks to the game server you are playing on, does
  not place blocks, and is safe on multiplayer.
- **Needs `schemgen2 serve` running** on the same machine. The mod converts
  nothing itself — it is an HTTP client for [the API](api.md).

> **Build status:** the mod's sources are complete and syntax-checked, but they
> have **not been compiled against Minecraft** in this repository — Gradle is
> not installed here, and Loom needs network access to fetch Minecraft and the
> Fabric toolchain. Expect to run `gradle build` once yourself (below). If a
> Yarn mapping shifted in 1.21.11, it surfaces as a named compile error rather
> than a runtime surprise.

## Build

Gradle is not vendored — there is no `gradle-wrapper.jar` in the repo, only the
`gradle-wrapper.properties` that pins the version.

```bash
# once, if you have no gradle: winget install Gradle.Gradle   (or: scoop install gradle)
cd mod
gradle wrapper      # optional: generates ./gradlew for future builds
gradle build
```

The jar lands in `mod/build/libs/schemgen2-fabric-2.0.0.jar` (ignore the
`-sources` jar). Drop it into your instance's `mods/` folder alongside
[Fabric API](https://modrinth.com/mod/fabric-api).

The first build downloads Minecraft, Yarn mappings and the Fabric toolchain, so
it needs network access and a few minutes. Later builds are incremental.

### Retargeting another Minecraft version

Refresh all four coordinates in [`mod/gradle.properties`](../mod/gradle.properties)
together — a mismatched Yarn/loader pair fails with confusing mapping errors:

| Property | Source |
|---|---|
| `minecraft_version` | the version you want |
| `yarn_mappings` | `https://meta.fabricmc.net/v2/versions/yarn/<version>` |
| `loader_version` | `https://meta.fabricmc.net/v2/versions/loader/<version>` |
| `fabric_version` | `https://api.modrinth.com/v2/project/fabric-api/version?game_versions=["<version>"]` |

Also check the data version stamped into schematics — see
[docs/pipeline.md](pipeline.md#minecraft-version).

## Use

Start the converter server first:

```bash
cd backend && cargo run --release -- serve
```

Then in game:

| Command | Effect |
|---|---|
| `/schemgen` | Open the GUI |
| `/schemgen convert <path>` | Convert a model with the saved settings, reporting in chat |
| `/schemgen status` | Check the server is up; prints version, palette size, data version |
| `/schemgen server <url>` | Point the mod at another server (default `http://localhost:3001`) |
| `/schemgen folder` | Open the schematics folder |

These are **client** commands: they work in singleplayer, on any multiplayer
server, and never leave your machine.

### The GUI

| Control | Meaning |
|---|---|
| Path field + **Browse…** | The `.glb` / `.gltf` to convert. Browse uses the OS file dialog; the field takes a pasted path when no dialog backend exists. |
| Name field | Schematic name. Empty means the model's file name. |
| **Longest side** | 16–512 blocks along the model's longest axis. |
| **Dithering** | 8×8 Bayer ordered dithering. |
| **De-light** | Lighting-separation strength, 0 = off ([why](pipeline.md#lighting-separation-de-light)). |
| **Convert** / **Cancel** | Cancel stops reporting and saving; the server still finishes the job it started, since the API has no cancel route. |
| **Open schematics folder** | Opens `<instance>/schematics` in your file manager. |

Progress runs upload → convert → save. Closing the screen does not abandon a
running conversion; it still saves, and reopening shows the result.

Settings persist in `config/schemgen.json` in the instance folder:

```json
{
  "serverUrl": "http://localhost:3001",
  "maxSize": 128,
  "dither": true,
  "delight": 0.0,
  "lastDirectory": ""
}
```

## Where files go

`<game dir>/schematics/<name>.litematic`, where the game dir is the *running
instance's* folder as Fabric reports it — the per-instance one your launcher
created, not vanilla `.minecraft`. An existing name is never overwritten:
a second `castle` becomes `castle-2.litematic`.

Load it in game with Litematica: **M** → *Load Schematics* → pick the file.

## Source map

```
mod/src/main/java/com/schemgen/mod/
├── SchemGenMod.java          Client entrypoint; defers screen opening one tick
├── SchemGenCommands.java     /schemgen client commands
├── SchemGenConfig.java       config/schemgen.json
├── ApiClient.java            The four HTTP calls + hand-built multipart body
├── ConversionTask.java       One conversion on its own thread; volatile progress
├── SchematicsFolder.java     Instance schematics folder, sanitizing, de-duplication
├── FilePicker.java           LWJGL TinyFileDialogs, off the render thread
└── gui/SchemGenScreen.java   The GUI
```

The mod adds no third-party dependencies: the HTTP client (`java.net.http`),
the JSON parser (Gson) and the file dialog (LWJGL TinyFileDialogs) all ship with
Minecraft already, so the jar stays small and needs no shading.

## Troubleshooting

| Symptom | Cause |
|---|---|
| "No SchemGen2 server at http://localhost:3001" | `schemgen2 serve` is not running, or it is on another port — `/schemgen server <url>`. |
| Browse does nothing | No native dialog backend (a bare Linux box without zenity/kdialog). Paste the path into the field instead. |
| Schematic missing in Litematica | Check `/schemgen folder` — the mod writes to the *instance's* folder; Litematica reads the same one. |
| "Only .glb and .gltf models can be converted" | The pipeline reads glTF; convert from OBJ/FBX in Blender first. |
