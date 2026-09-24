#!/usr/bin/env python3
"""Check that every file naming SchemGen2's version agrees.

    python tools/check_versions.py [--tag v2.1.0 [--notes FILE [--link-base URL]]]
                                   [--github-output FILE]

The version is backend/Cargo.toml's `[workspace.package] version`. The
workspace crates in Cargo.lock, the web UI's package.json and lockfile, and
the server release the mod launches (`schemgen_server_version` in
schemgen-mod/gradle.properties) must all say the same.

With --tag, as the release workflow runs it, the tag must be `v<version>` and
CHANGELOG.md must have a `## <version>` section; --notes writes that section
to FILE for the release notes, with its relative links made absolute against
--link-base (release notes are not shown next to the files they link to).
--github-output appends `version=<version>` to FILE (the workflow's
$GITHUB_OUTPUT).
"""

import argparse
import json
import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SEMVER = re.compile(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$")
# A Markdown link target that is a path in the repository.
RELATIVE_LINK = re.compile(r"\]\((?![a-z][a-z0-9+.-]*:|#|/)([^)\s]+)\)")
CRATES = ("schemgen-core", "schemgen-server", "schemgen2")


def properties(path):
    """A Java .properties file's key=value lines."""
    values = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith(("#", "!")) and "=" in line:
            key, value = line.split("=", 1)
            values[key.strip()] = value.strip()
    return values


def changelog_section(version):
    """The text under `## <version>` in CHANGELOG.md, or None."""
    lines = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8").splitlines()
    heading = re.compile(rf"^## {re.escape(version)}(\s|$)")
    for i, line in enumerate(lines):
        if heading.match(line):
            end = next((j for j in range(i + 1, len(lines)) if lines[j].startswith("## ")), len(lines))
            return "\n".join(lines[i + 1 : end]).strip() + "\n"
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--tag", help="the release tag, v<version>")
    parser.add_argument("--notes", type=pathlib.Path, help="write the changelog section here")
    parser.add_argument("--link-base", help="URL relative links in the notes are resolved against")
    parser.add_argument("--github-output", type=pathlib.Path, help="append version=<version> here")
    args = parser.parse_args()

    cargo = tomllib.loads((ROOT / "backend/Cargo.toml").read_text(encoding="utf-8"))
    version = cargo["workspace"]["package"]["version"]
    problems = []
    if not SEMVER.match(version):
        problems.append(f"backend/Cargo.toml: {version!r} is not a version like 2.1.0")

    found = {}
    lock = tomllib.loads((ROOT / "backend/Cargo.lock").read_text(encoding="utf-8"))
    for package in lock["package"]:
        if package["name"] in CRATES:
            found[f"backend/Cargo.lock ({package['name']})"] = package["version"]
    package = json.loads((ROOT / "frontend/package.json").read_text(encoding="utf-8"))
    found["frontend/package.json"] = package["version"]
    package_lock = json.loads((ROOT / "frontend/package-lock.json").read_text(encoding="utf-8"))
    found["frontend/package-lock.json"] = package_lock["version"]
    found["frontend/package-lock.json (packages[''])"] = package_lock["packages"][""]["version"]
    mod = properties(ROOT / "schemgen-mod/gradle.properties")
    found["schemgen-mod/gradle.properties (schemgen_server_version)"] = mod.get("schemgen_server_version")

    for where, value in found.items():
        if value != version:
            problems.append(f"{where} says {value}, backend/Cargo.toml says {version}")

    if args.tag is not None:
        if args.tag != f"v{version}":
            problems.append(f"the tag is {args.tag}, but the version is {version}: tag v{version}")
        notes = changelog_section(version)
        if notes is None or not notes.strip():
            problems.append(f"CHANGELOG.md has no '## {version}' section to use as the release notes")
        elif args.notes:
            if args.link_base:
                base = args.link_base.rstrip("/") + "/"
                notes = RELATIVE_LINK.sub(lambda m: f"]({base}{m.group(1)})", notes)
            args.notes.write_text(notes, encoding="utf-8")

    if problems:
        for problem in problems:
            print(f"error: {problem}", file=sys.stderr)
        return 1
    if args.github_output:
        with args.github_output.open("a", encoding="utf-8") as out:
            out.write(f"version={version}\n")
    print(f"SchemGen2 {version}: {len(found) + 1} places agree")
    return 0


if __name__ == "__main__":
    sys.exit(main())
