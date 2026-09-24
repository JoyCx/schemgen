# Minecraft mod

A client-side Fabric mod, in [`schemgen-mod/`](../schemgen-mod), that puts the
converter inside the game: pick a model, tune the settings, preview the result
on the block you are looking at, convert, and hand the schematic to
Litematica. It is a client of the same [HTTP API](api.md) as the web app and
contains none of the pipeline: the settings form is rendered from
`GET /api/schema`, conversions are server jobs.

> **Status.** The `common` module — API client, sidecar launcher, settings
> model, config — is unit-tested and integration-tested against a real
> `schemgen2`. The Fabric layer was written against the Yarn mappings, Fabric
> API and Litematica sources of each supported version (see
> [What was verified](#what-was-verified-and-against-what)) but could not be
> compiled where it was written; [CI](../.github/workflows/mod.yml) compiles it
> for every version. It has not been run in a game yet.

## For players

### Install

| Minecraft | Jar |
|---|---|
| 1.21.1 | `schemgen-<version>+mc1.21.1.jar` |
| 1.21.4 | `schemgen-<version>+mc1.21.4.jar` |
| 1.21.8 | `schemgen-<version>+mc1.21.8.jar` |
| 1.21.11 | `schemgen-<version>+mc1.21.11.jar` |

1. Install Fabric Loader (0.16 or later) and Fabric API for your version.
2. Drop the jar for your version into `mods/`. Until tagged releases exist
   (roadmap Phase 6), the jars are the `schemgen-fabric-<version>` artifacts of
   the **Mod** workflow's runs.
3. Optional: **Litematica** and **MaLiLib**. Without them the mod still
   converts and saves into `schematics/`; with them it previews through a
   real placement and can load results for you.
4. A SchemGen server — see [The server](#the-server-built-in-or-your-own).

### The screen

Press **K** in game (rebind under *Options → Controls → Key Binds → SchemGen*).

- **Left — models.** Every `.glb` and `.gltf` in the models folder.
  *Refresh* lists it again, *Folder* opens it in your file manager, *Browse…*
  opens your system's file dialog for a model anywhere else. Below the list:
  the last result's thumbnail, the server's isometric render.
- **Right — settings.** The server's settings, in the sections and order of
  its schema (*Size & shape*, *Color*, *Lighting*, *Target version*,
  *Output*), each as the widget its type calls for: sliders for ranges, on/off
  buttons, buttons that cycle through choices and blocks, text boxes (an empty
  *Voxel size* means "use *Max size*"), and the light direction as two
  sliders, azimuth and elevation. Hover a field for its help. Fields that only
  apply with another setting (dithering only with color sampling, …) are
  hidden while they do not. *Advanced settings* shows the rest. The list
  scrolls.
- **Minecraft version** shows the game you are playing — that is what the
  schematic is made for. *Change* lets you pick another; the choice is saved
  until you cycle back to "this game".
- **Bottom.** Progress bar, status line (hover it when it is cut off), and the
  actions:

| Button | What it does |
|---|---|
| Preview | Converts at preview size (64 blocks by default) and shows the result next to the block face you were looking at when you opened the screen, centered on that spot. With Litematica it is a temporary placement named *SchemGen preview*; without, translucent boxes in each block's color. |
| Rotate | Turns the preview a quarter turn clockwise (seen from above). |
| Clear | Removes the preview. |
| Convert | Converts the selected model with the form's settings and saves the schematic into the schematics folder, under the server's name for it (`castle.litematic`, then `castle-2.litematic`, … — nothing is overwritten). It keeps running if you close the screen and says in chat when it is done. |
| Load in Litematica | Loads the last schematic into Litematica and places it where the preview is, turned the same way — or at your feet without a preview. With *Place results in Litematica* on (the default), every conversion does this by itself. |
| Cancel | Stops the running conversion. |

*Rotate preview* and *Clear preview* are also key bindings, unbound by
default. Settings you change are remembered between sessions; the ones you
never touched follow the server's defaults.

### The server: built in or your own

*Server…* on the screen switches between two modes.

**Built in** (the default). The mod runs `schemgen2 serve` itself the first
time the screen opens, on a free port of `127.0.0.1` chosen by the system,
with a random token passed through the environment (never the command line),
and stops it when the game exits. The server also stops on its own if the game
dies (`--exit-with-stdin`). Where the program comes from, in order:

1. *schemgen2 program* in the server settings — your own build, run as is.
2. A binary bundled in the mod jar, extracted to `schemgen/bin/<version>/`
   once its SHA-256 matches the checksum pinned in the jar.
3. The GitHub release asset for your platform, downloaded to the same folder
   and run only if it matches the pinned checksum.
4. Otherwise the screen says why not, and suggests your own server.

> **Today:** no schemgen2 release exists yet and development builds pin no
> checksums, so steps 2 and 3 cannot happen — set *schemgen2 program* to a
> binary you built (`cargo build --release -p schemgen2` in `backend/`), or run
> your own server. The binary is all it needs: it voxelizes in Rust, with no
> Python and no files beside it.

**My own.** Start a server yourself — on this computer or another one on your
network — and enter its address, port and token:

```bash
schemgen2 serve                                        # this computer, port 3001
schemgen2 serve --host 192.168.1.20 --token <secret>   # reachable at that address
```

A server answers only to the names it was started for: `localhost`,
`127.0.0.1`, the `--host` address, and names given with `--allow-host` (a
guard against DNS rebinding). Enter one of those as the address. See
[cli.md](cli.md) for the options; the mod needs API v2 (schemgen2 2.0 or
later).

### Files

All relative to the game directory (`.minecraft`, or the instance folder of
your launcher):

| What | Where |
|---|---|
| Settings | `config/schemgen.json` |
| Models listed on the screen | `schemgen/models/` (changeable) |
| Finished schematics | `schematics/` — Litematica's folder (changeable) |
| Built-in server program | `schemgen/bin/<server version>/schemgen2[.exe]` |
| Server uploads and results | `schemgen/work/` — each job is forgotten 24 hours after it finished |
| Litematica previews | `schemgen/preview/` — deleted when the preview is cleared |
| Log | `logs/latest.log`: the `SchemGen` logger, the server's own output prefixed `[schemgen2]` |

`config/schemgen.json` (every key optional; a relative folder is relative to
the game directory; a file that cannot be parsed is renamed
`schemgen.json.broken` and the defaults are used):

| Key | Default | |
|---|---|---|
| `serverMode` | `"SIDECAR"` | `"SIDECAR"` (built in) or `"EXTERNAL"` (your own) |
| `host`, `port`, `token` | `"127.0.0.1"`, `3001`, `""` | Your own server |
| `binaryPath` | `""` | A schemgen2 to run instead of the bundled or downloaded one |
| `outputFolder` | `""` → `schematics` | Where schematics are saved |
| `modelsFolder` | `""` → `schemgen/models` | The folder the screen lists |
| `targetOverride` | `""` → this game | A Minecraft version to convert for instead |
| `autoLoadIntoLitematica` | `true` | Place every finished schematic in Litematica |
| `previewMaxSize` | `64` | Longest side of a preview, 1–256 blocks |
| `lastSettings` | `{}` | The settings you changed, restored next time |

### Troubleshooting

**"SchemGen server did not start: …"** — the rest of the sentence says why:

- *pins no server release*, *no checksum is pinned*, *not built for …*: there
  is no program the mod may run for you. Set *schemgen2 program* or use your
  own server (see [Today](#the-server-built-in-or-your-own)).
- *it exited with code …* or *it printed no address within 20 s*: the program
  started and failed. The message ends with its last lines of output; all of
  them are in `logs/latest.log` under `[schemgen2]`.
- An **antivirus** may quarantine or block a freshly downloaded executable.
  Allow `schemgen/bin/`, or point *schemgen2 program* at a copy elsewhere. On
  macOS, a binary you downloaded yourself may need
  `xattr -d com.apple.quarantine schemgen2`.
- **Permissions:** the mod has to write `schemgen/` in the game directory and,
  on Linux and macOS, mark the program executable. A game directory on a
  `noexec` mount cannot run it — set *schemgen2 program* to a copy elsewhere.

**Ports.** The built-in server asks the system for a free port, so it cannot
clash with anything. For your own server, *Cannot reach the SchemGen server at
… — is it running?* means nothing answered there: check it runs, the port, and
the other computer's firewall. *Unknown host* means the server was not
started for the address you entered: start it with `--host <that address>` or
`--allow-host <that name>`. *Missing or wrong token* means the token does not
match the server's `--token`. *Too old for the mod* means a schemgen2 from
before API v2.

**Grey preview boxes** are blocks the server's palette has no color for.

**No Litematica buttons working:** Litematica and MaLiLib are not installed,
or not the build for your Minecraft version.

## For developers

### Layout

```
schemgen-mod/
├── settings.gradle.kts        plugins; `common`; the Stonecutter tree `:fabric`
├── gradle.properties          mod version, pinned server version, loader
├── common/                    plain Java 21 — no Minecraft, tested anywhere
│   └── src/main/java/io/github/joycx/schemgen/common/
│       ├── backend/           BackendClient (API v2, SSE with polling fallback),
│       │                      BackendLauncher + Sidecar, ServerBinaries,
│       │                      BackendService, Conversions
│       ├── schema/, settings/ the schema and the form's model (SettingsModel)
│       ├── preview/           PreviewData, PreviewGrid, GhostMesh, Footprint
│       ├── model/, config/, files/
└── fabric/                    the Minecraft layer, one Loom project per version
    ├── stonecutter.gradle.kts the active version
    ├── build.gradle.kts       per-version build, bundleServerBinaries
    ├── versions/<mc>/gradle.properties   Yarn, Fabric API, Litematica, MaLiLib
    └── src/main/java/io/github/joycx/schemgen/
        ├── SchemGenClient     key bindings, lifecycle
        ├── SchemGenSession    state, worker thread, connection
        ├── Converter          the Convert button's job
        ├── ui/                SchemGenScreen, SettingsPanel, ServerSettingsScreen,
        │                      SettingSlider, FilePicker, Thumbnail
        ├── preview/           GhostPreview, GhostRenderer*
        ├── litematica/        LitematicaSupport, LitematicaBridge*
        └── compat/            ScreenCompat*
```

`*` — the only files with version-specific code. Everything that can be plain
Java is in `common`, so it is covered by tests without a game. The client
thread never waits: server work runs on a worker and reports back through
`MinecraftClient.execute`.

### Building

```bash
cd schemgen-mod
./gradlew :common:test                 # anywhere; no Minecraft needed
./gradlew :fabric:1.21.8:build         # → fabric/versions/1.21.8/build/libs/
./gradlew :fabric:1.21.8:runClient     # a development client with the mod
```

Gradle 8.14.3 (wrapper), Java 21. Building the Fabric part downloads Minecraft
and its libraries (Mojang), Loom's dependencies (`maven.fabricmc.net`) and
Litematica/MaLiLib (Modrinth's Maven). `org.gradle.configureondemand` keeps
`:common:test` from configuring the Fabric projects, so it needs none of that.

The integration tests start a real server and convert
`backend/fixtures/textured.glb`:

```bash
SCHEMGEN_BINARY=../backend/target/release/schemgen2 ./gradlew :common:test
```

Without `SCHEMGEN_BINARY` they are skipped.

CI (`.github/workflows/mod.yml`, on changes to `schemgen-mod/`, `backend/` or
the workflow) runs the common tests with a freshly built server, builds the jar
for every version, and uploads the jars as artifacts.

### Minecraft versions

| Minecraft | Yarn | Fabric API | Litematica | MaLiLib |
|---|---|---|---|---|
| 1.21.1 | 1.21.1+build.3 | 0.116.17+1.21.1 | 0.19.58 | 0.21.8 |
| 1.21.4 | 1.21.4+build.8 | 0.119.4+1.21.4 | 0.21.3 | 0.23.3 |
| **1.21.8** (active) | 1.21.8+build.1 | 0.136.1+1.21.8 | 0.23.3 | 0.25.4 |
| 1.21.11 | 1.21.11+build.3 | 0.141.6+1.21.11 | 0.26.6 | 0.27.10 |

Shared: Fabric Loader 0.18.4 to build against (the jar asks for 0.16 or
later), Loom 1.13.6 (the newest line on Gradle 8), Stonecutter 0.7.11 (0.8 and
later need Gradle 9). Litematica and MaLiLib are compile-only: the mod never
needs them at run time.

[Stonecutter](https://stonecutter.kikugie.dev/) builds each version from one
`fabric/src`, which is written for the **active** version, 1.21.8. Code for
other versions sits in comments that Stonecutter swaps in:

```java
//? if >=1.21.9 {
/*return new KeyBinding(translationKey, glfwKey, CATEGORY);
*///?} else {
return new KeyBinding(translationKey, glfwKey, "key.categories." + MOD_ID);
//?}
```

Every version builds from its own processed copy whatever is active. To edit
in another version's code, `./gradlew "Set active project to 1.21.1"`, and
`./gradlew "Reset active project"` before committing.

What differs, and where the boundary is (from the Yarn mappings of every 1.21
release, 1.21.1 to 1.21.11, unless noted):

| API | Before | From | In |
|---|---|---|---|
| Key binding category | a translation key string | `KeyBinding.Category.create(Identifier)` | 1.21.9 |
| `DrawContext.drawTexture` | `(Identifier, …)` | `(Function<Identifier, RenderLayer>, Identifier, …)`, then `(RenderPipeline, Identifier, …)` | 1.21.2, 1.21.6 |
| `NativeImageBackedTexture` | `(NativeImage)` | `(Supplier<String>, NativeImage)` | 1.21.5 |
| Fabric API world rendering | `rendering.v1.WorldRenderEvents`; `matrixStack()`, `camera().getPos()` | `rendering.v1.world.WorldRenderEvents`; `matrices()`, `worldState().cameraRenderState.pos` | Fabric API 1.21.10 (seen in 1.21.8 and 1.21.10 sources; 1.21.9 not checked) |
| Translucent quads layer | `RenderLayer.getDebugQuads()` | `RenderLayers.debugQuads()` | 1.21.11 |
| `LitematicaSchematic.createFromFile` | `(File dir, String name)` | `(Path dir, String name)` | Litematica 0.23 (1.21.8; 0.21.3 for 1.21.4 still takes a `File`) |

Everything else the mod calls — screens and widgets, text, `fill`, tooltips,
`MinecraftClient`, `BlockPos`, the vertex consumer calls, Fabric's key binding,
tick and lifecycle events, Litematica's placement API — has the same name and
signature in all four versions.

Fabric API and Litematica for 1.21.11 are written with Mojang's names, but
they are published remapped to intermediary, and Loom remaps them to Yarn for
this build — so the mod uses Yarn names throughout.

**Why not 26.x.** Minecraft 26.1 ships unobfuscated: there are no Yarn
mappings for it (the Yarn branch does not exist), Fabric's toolchain for it
uses Mojang's names, and it needs Java 25, Loom 1.14 or later and — for
Stonecutter 0.8 or later — Gradle 9. The way there is to move the Fabric layer
to Mojang's names first (Loom's `officialMojangMappings()` works for 1.21.x
too), then upgrade Gradle, Loom and Stonecutter, and add 26.1 like any other
version.

### Adding a Minecraft version

1. Add it to `versions(…)` in `settings.gradle.kts` and create
   `fabric/versions/<mc>/gradle.properties` (`minecraft_range`,
   `yarn_mappings`, `fabric_api_version`, `litematica_version`,
   `malilib_version`). The Litematica and MaLiLib numbers are their Modrinth
   version numbers.
2. Check every API in the table above for the new version, and add
   `//? if >=<mc>` branches in `ScreenCompat`, `GhostRenderer` or
   `LitematicaBridge` where it changed. Keep version checks out of every
   other file; add a method to `ScreenCompat` instead.
3. Add the version to the matrix in `.github/workflows/mod.yml`.
4. `./gradlew :fabric:<mc>:build`, then try it: `./gradlew :fabric:<mc>:runClient`.
5. The server needs a target for it — see [versions.md](versions.md#adding-a-version).

### Bundling the server

`schemgen_server_version` in `gradle.properties` is the schemgen2 release the
mod launches (the `[workspace.package] version` of `backend/Cargo.toml`). The
`bundleServerBinaries` task, part of every jar build, writes it into the jar:

```bash
./gradlew :fabric:1.21.8:build \
  -PserverChecksums=SHA256SUMS      # pin the release's checksums: small jar, downloads on first use
./gradlew :fabric:1.21.8:build \
  -PserverBinaries=dist/            # bundle the binaries too (~10 MB each)
```

- `-PserverBinaries=<dir>`: files named `schemgen2-<os>-<arch>` (`.exe` on
  Windows) for `windows`, `macos`, `linux` × `x64`, `arm64`; the ones present
  go into the jar as `bin/<os>-<arch>/schemgen2[.exe]` and their SHA-256 is
  pinned. Other files are ignored.
- `-PserverChecksums=<file>`: `sha256sum` output for the release assets
  (`<hex>  schemgen2-linux-x64`; a `*` or a path before the name is fine).
  A bundled binary that disagrees with it fails the build.
- Neither: the version and URLs are pinned but no checksums, so the mod will
  not download anything (a development build).

The jar's `schemgen-server.properties`:

```properties
version=2.0.0
url.linux-x64=https://github.com/JoyCx/schemgen/releases/download/v2.0.0/schemgen2-linux-x64
sha256.linux-x64=<64 hex digits, or empty>
# … the same two keys for windows-x64, windows-arm64, macos-x64, macos-arm64, linux-arm64
```

So a release publishes assets with exactly those names on the `v<version>`
tag, and builds the jars with that release's checksums. The platform comes
from `os.name` (`Windows…`, `Mac…`, `…Linux…`) and `os.arch` (`amd64`/`x86_64`
→ `x64`, `aarch64` → `arm64`). A binary is only ever run after its SHA-256 was
checked against the jar; a mismatch deletes it.

### What was verified, and against what

The Fabric layer was written where Fabric's Maven, Mojang's servers and
Modrinth could not be reached, so it could not be compiled there. Instead,
every Minecraft, Fabric API and Litematica name it uses was looked up, with
its descriptor, in each supported version's sources on
`raw.githubusercontent.com`:

| Source | Versions | For |
|---|---|---|
| `FabricMC/yarn`, `mappings/net/minecraft/**.mapping` | branches 1.21.1 to 1.21.11 | every Minecraft class, field, method and constructor, by descriptor; the boundaries above |
| `FabricMC/fabric-api` | 1.21.1, 1.21.4, 1.21.8, 1.21.10, 1.21.11 | `KeyBindingHelper`, `ClientTickEvents`, `ClientLifecycleEvents`, `WorldRenderEvents` and their contexts |
| `sakura-ryoko/litematica` | tags `1.21-0.19.58`, `1.21.4-0.21.3`, `1.21.8-0.23.3`, `1.21.11-0.26.6` | `DataManager`, `SchematicHolder`, `LitematicaSchematic`, `SchematicPlacement`, `SchematicPlacementManager` |
| `sakura-ryoko/malilib` | tags `1.21-0.21.8`, `1.21.4-0.23.3`, `1.21.8-0.25.4`, `1.21.11-0.27.10` | `InfoUtils`, `IMessageConsumer`, `Message.MessageType` |
| `FabricMC/fabric-example-mod` | 1.21.1, 1.21.4, 1.21.8, 1.21.11 | Fabric API versions |

Mappings carry names and descriptors but not class hierarchies or generic
bounds; where the mod relies on those (a widget passed to
`Screen.addDrawableChild`, `BlockPos.offset` returning a `BlockPos`, the
`BlockPos(int, int, int)` constructor) the same calls were found in
Litematica's sources or Fabric's documentation examples (`FabricMC/fabric-docs`).

The Yarn builds for 1.21.4 and 1.21.8 are the ones Litematica's own builds
use; those for 1.21.1 and 1.21.11 the ones other published mods and the Fabric
documentation use. The Modrinth coordinates are the ones other mods' builds
resolve (Modrinth's API could not be queried). The Stonecutter comments were
run through Stonecutter 0.7.11 itself for all four versions, and each
version's processed sources parse. The `bundleServerBinaries` task was run in
a scratch project with binaries, checksums, and both kinds of bad input.

Not yet done: compiling the Fabric layer (CI's first run), and playing with
it.
