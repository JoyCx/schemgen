#!/usr/bin/env python3
"""Convert a model for every target and format with the real binary, then
check each file with readers SchemGen2 did not write.

    python tools/check_outputs.py --binary backend/target/release/schemgen2

For every target `schemgen2 targets` lists and every format, the fixture is
converted and the file is read back:

* `.litematic` with litemapy — its own NBT and bit-array decoding;
* `.schem` (Sponge v2) and `.nbt` (structure) with litemapy's importers;
* `.schem` v3, which litemapy does not import, with nbtlib and a varint
  decoder here.

Each check confirms the stamped data version (and Litematica version), that
the block count matches what the converter reported, and — the point of
per-target palettes — that every block in the file exists in that Minecraft
version according to PrismarineJS minecraft-data.

Needs litemapy and nbtlib (`pip install litemapy nbtlib`), network access to
raw.githubusercontent.com for the block lists, and whatever the binary itself
needs to convert.
"""

import argparse
import gzip
import json
import pathlib
import subprocess
import sys
import tempfile
import urllib.request

import nbtlib
from litemapy import Region, Schematic

ROOT = pathlib.Path(__file__).resolve().parent.parent
DATA = "https://raw.githubusercontent.com/PrismarineJS/minecraft-data/master/data"
AIR = "minecraft:air"


def fetch_json(url):
    with urllib.request.urlopen(url, timeout=60) as response:
        return json.load(response)


class BlockLists:
    """minecraft-data block names per game version, newest list carried
    forward for versions it does not cover yet (as gen_block_versions does)."""

    def __init__(self):
        self.paths = fetch_json(f"{DATA}/dataPaths.json")["pc"]
        self.cache = {}

    def names(self, version, ordered_versions):
        if version in self.cache:
            return self.cache[version]
        candidates = ordered_versions[: ordered_versions.index(version) + 1]
        for v in reversed(candidates):
            entry = self.paths.get(v)
            if entry and "blocks" in entry:
                blocks = fetch_json(f"{DATA}/{entry['blocks']}/blocks.json")
                self.cache[version] = {f"minecraft:{b['name']}" for b in blocks}
                return self.cache[version]
        raise SystemExit(f"no minecraft-data block list for {version} or older")


def varints(data):
    out, value, shift = [], 0, 0
    for byte in data:
        byte &= 0xFF
        value |= (byte & 0x7F) << shift
        if byte & 0x80:
            shift += 7
        else:
            out.append(value)
            value, shift = 0, 0
    return out


def region_blocks(region):
    """Non-air block ids and their total, read through litemapy."""
    ids, count = set(), 0
    for x, y, z in region.block_positions():
        block = region[x, y, z].id
        if block != AIR:
            ids.add(block)
            count += 1
    return ids, count


def check_litematic(path, target):
    s = Schematic.load(str(path))
    root = nbtlib.load(path)
    assert int(root["MinecraftDataVersion"]) == target["data_version"], "data version"
    assert int(root["Version"]) == target["schematic_version"], "Litematica version"
    (region,) = s.regions.values()
    return region_blocks(region)


def check_schem_v2(path, target):
    root = nbtlib.load(path)
    assert int(root["Version"]) == 2, "Sponge version"
    assert int(root["DataVersion"]) == target["data_version"], "data version"
    region, data_version = Region.from_sponge_nbt(root)
    assert int(data_version) == target["data_version"]
    return region_blocks(region)


def check_schem_v3(path, target):
    root = nbtlib.load(path)["Schematic"]
    assert int(root["Version"]) == 3, "Sponge version"
    assert int(root["DataVersion"]) == target["data_version"], "data version"
    blocks = root["Blocks"]
    by_index = {int(v): k for k, v in blocks["Palette"].items()}
    values = varints(bytes(b & 0xFF for b in blocks["Data"]))
    volume = int(root["Width"]) * int(root["Height"]) * int(root["Length"])
    assert len(values) == volume, f"{len(values)} cells for a volume of {volume}"
    names = [by_index[v] for v in values]
    placed = [n for n in names if n != AIR]
    return set(placed), len(placed)


def check_structure(path, target):
    root = nbtlib.load(path)
    assert int(root["DataVersion"]) == target["data_version"], "data version"
    size = [int(v) for v in root["size"]]
    for block in root["blocks"]:
        pos = [int(v) for v in block["pos"]]
        assert all(0 <= p < s for p, s in zip(pos, size)), f"{pos} outside {size}"
    region, _ = Region.from_structure_nbt(root)
    return region_blocks(region)


CHECKS = {
    "litematic": check_litematic,
    "schem": check_schem_v2,
    "schem-v3": check_schem_v3,
    "nbt": check_structure,
}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    parser.add_argument("--fixture", type=pathlib.Path,
                        default=ROOT / "backend" / "fixtures" / "metal_transforms.glb")
    parser.add_argument("--max-size", type=int, default=40)
    parser.add_argument("--targets", default="all", help="comma-separated ids, or all")
    args = parser.parse_args()

    listing = json.loads(subprocess.run([str(args.binary), "targets", "--json"],
                                        check=True, capture_output=True, text=True).stdout)
    targets = listing["targets"]
    ordered = [t["id"] for t in targets]
    if args.targets != "all":
        wanted = set(args.targets.split(","))
        targets = [t for t in targets if t["id"] in wanted]

    lists = BlockLists()
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        for target in targets:
            known = lists.names(target["id"], ordered) | {AIR}
            for fmt, check in CHECKS.items():
                out = pathlib.Path(tmp) / f"{target['id']}-{fmt}.out"
                run = subprocess.run(
                    [str(args.binary), "convert", str(args.fixture), "-o", str(out),
                     "-t", target["id"], "-f", fmt, "--max-size", str(args.max_size), "--json"],
                    capture_output=True, text=True)
                label = f"{target['id']:>8} {fmt:<9}"
                if run.returncode != 0:
                    failures += 1
                    print(f"FAIL {label} convert exited {run.returncode}: {run.stderr.strip()[-300:]}")
                    continue
                reported = json.loads(run.stdout)["files"][0]["voxels"]
                try:
                    ids, count = check(out, target)
                    assert count == reported, f"{count} blocks read, {reported} reported"
                    unknown = sorted(ids - known)
                    assert not unknown, f"not in Minecraft {target['id']}: {', '.join(unknown)}"
                    print(f"ok   {label} {count:>6} blocks, {len(ids):>3} kinds")
                except (AssertionError, KeyError, ValueError) as e:
                    failures += 1
                    print(f"FAIL {label} {type(e).__name__}: {e}")
    total = len(targets) * len(CHECKS)
    print(f"\n{total - failures}/{total} files passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
