# Releasing

A release is a tag. Pushing `v<version>` runs
[`.github/workflows/release.yml`](../.github/workflows/release.yml), which
builds everything from that commit and publishes it as the GitHub release of
the tag:

| Asset | What it is |
|---|---|
| `schemgen2-windows-x64.exe` | The server and CLI, web UI inside, C runtime linked in |
| `schemgen2-macos-arm64`, `schemgen2-macos-x64` | The same for Apple silicon and Intel Macs (unsigned) |
| `schemgen2-linux-x64` | The same for Linux with glibc 2.35 or newer |
| `SHA256SUMS` | `sha256sum` of the four binaries |
| `schemgen-<mod version>+mc<Minecraft>.jar` | The mod, one per Minecraft version, with `SHA256SUMS` pinned |

The asset names are the mod's contract ([mod.md](mod.md#bundling-the-server)):
a mod jar downloads `schemgen2-<os>-<arch>[.exe]` from the release of the
server version it pins, and runs it only if its SHA-256 matches.

## Cutting a release

1. **Pick the version.** Semantic versioning for the server and CLI: a new
   API version or a removed option is a new major version, new settings or
   routes a minor one.
2. **Set it everywhere** — the check below lists what it compares:
   - `backend/Cargo.toml`, `[workspace.package] version`, then
     `cargo update --workspace` in `backend/` for `Cargo.lock`;
   - `frontend/package.json`, then `npm install --package-lock-only` in
     `frontend/`;
   - `schemgen_server_version` in `schemgen-mod/gradle.properties` — the
     server the mod launches. Raise `mod_version` there too when the mod
     changed.
3. **Write the changelog.** `CHANGELOG.md` needs a `## <version>` section;
   it becomes the release notes.
4. **Check:**

   ```bash
   python3 tools/check_versions.py --tag v2.1.0
   ```

5. **Merge to the default branch**, wait for CI, then tag that commit and push
   the tag:

   ```bash
   git tag v2.1.0
   git push origin v2.1.0
   ```

The workflow checks the versions again (a mismatch stops it before anything
is built), builds the web UI once, builds the server on each platform with
that UI embedded (`SCHEMGEN_EMBED_UI`), smoke-tests each binary it can run
([`tools/smoke_release.sh`](../tools/smoke_release.sh): it starts away from
the source tree, serves its own UI and API, converts a fixture), computes
`SHA256SUMS`, builds the mod for every Minecraft version with those checksums
pinned (`-PserverChecksums`) and checks each jar carries all four, and
finally creates the release. A version with a hyphen (`2.2.0-rc.1`) becomes a
pre-release.

A failed run publishes nothing. Fix the cause on the default branch, then
move the tag (`git tag -f v2.1.0 <commit>`,
`git push -f origin v2.1.0`) or release the next patch version.

## Trying it without releasing

Every push that changes the release workflow, the server's build script or the
two tools it runs goes through the same jobs except the last, as does a manual
run from the Actions tab on a branch. The binaries, checksums and jars are
kept as workflow artifacts.

## By hand

The same build for one platform:

```bash
cd frontend && npm ci && npm run build && cd ..
SCHEMGEN_EMBED_UI="$PWD/frontend/dist" cargo build --release --locked -p schemgen2 \
  --manifest-path backend/Cargo.toml
tools/smoke_release.sh backend/target/release/schemgen2 backend/fixtures/textured.glb
```

`SCHEMGEN_EMBED_UI` makes the build fail if the UI is missing, rather than
quietly producing a binary without one.

## Not automated

- **Modrinth and CurseForge.** The mod's pages there do not exist yet; upload
  the jars from the GitHub release by hand, with Fabric API as a required
  dependency and Litematica as an optional one.
- **Signing.** The macOS binaries are not notarized and the Windows one is
  not signed, so a browser download is quarantined or warned about once
  (see the README). A mod-downloaded binary is not.
- **Linux on ARM and Windows on ARM** are not built; the mod tells players on
  them to run their own server.
