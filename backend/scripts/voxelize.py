#!/usr/bin/env python3
"""
voxelize.py — memory-efficient hollow surface voxelization for SchemGen2.

Replaces the trimesh ray-cast voxelizer (which allocates a dense O(grid³)
coordinate array and OOMs on large resolutions) with stratified surface
point sampling: sample points on every triangle, snap to the voxel grid,
deduplicate.  Memory usage is O(n_samples) regardless of resolution.

Usage:
    python voxelize.py --input model.glb [--max-size 128] [--voxel-size 0.1]

Outputs JSON to stdout: coords, pitch, grid_dims, voxel_world_origin.
"""

import argparse
import json
import sys
import numpy as np
import trimesh


def load_meshes(input_path):
    """Load the file once, returning per-primitive meshes with materials.

    The caller concatenates them for voxelization; keeping the parts lets the
    color sampler run in the same process without re-parsing the GLB.
    """
    scene = trimesh.load(input_path, force=None, process=False)
    if isinstance(scene, trimesh.Scene):
        if not scene.geometry:
            raise ValueError("No geometry found in the file")
        # Applies node transforms while preserving each primitive's material.
        meshes = scene.dump(concatenate=False)
    elif isinstance(scene, trimesh.Trimesh):
        meshes = [scene]
    else:
        raise ValueError(f"Unsupported mesh type: {type(scene)}")
    meshes = [m for m in meshes if isinstance(m, trimesh.Trimesh) and len(m.faces)]
    if not meshes:
        raise ValueError("No triangle geometry found in the file")
    return meshes


def concatenated(meshes):
    # Copy so cleanup never reindexes the meshes the color sampler will use.
    mesh = meshes[0].copy() if len(meshes) == 1 else trimesh.util.concatenate(meshes)
    mesh.remove_unreferenced_vertices()
    return mesh


def _grid_keys(points, bounds_min, voxel_size, grid_dims):
    """Snap world-space points to the voxel grid and encode as int64 keys.

    The key is x*(Dy*Dz) + y*Dz + z, which is order-isomorphic to the
    lexicographic (x, y, z) ordering — so sorting/uniquing keys is equivalent
    to np.unique(coords, axis=0) but avoids a lexsort over an (N, 3) array.
    """
    gc = np.floor((points - bounds_min) / voxel_size).astype(np.int32)
    np.clip(gc, 0, grid_dims - 1, out=gc)
    dy, dz = int(grid_dims[1]), int(grid_dims[2])
    return (gc[:, 0].astype(np.int64) * (dy * dz)
            + gc[:, 1].astype(np.int64) * dz
            + gc[:, 2])


def _edge_sample_keys(edge_verts, segs_per_edge, bounds_min, voxel_size,
                      grid_dims, budget=2_000_000):
    """Grid keys for points sampled uniformly along every unique edge.

    Equivalent to looping over edges and building
    ``np.linspace(0, 1, n)`` samples one edge at a time, but vectorized over
    chunks of edges. Points are converted to grid keys per chunk so peak
    memory stays bounded regardless of mesh size.
    """
    n_edges = len(segs_per_edge)
    if n_edges == 0:
        return np.empty(0, dtype=np.int64)

    segs = segs_per_edge.astype(np.int64)
    # Prefix sums let us pick chunk boundaries that respect the sample budget.
    ends = np.cumsum(segs)
    starts = np.concatenate(([0], ends))

    chunks = []
    lo = 0
    while lo < n_edges:
        hi = int(np.searchsorted(starts, starts[lo] + budget, side="right")) - 1
        hi = min(max(hi, lo + 1), n_edges)

        n = segs[lo:hi]
        total = int(n.sum())
        off = starts[lo:hi] - starts[lo]                       # start of each edge run

        # Position within each edge's run of samples.
        local = np.arange(total, dtype=np.int64) - np.repeat(off, n)
        # np.linspace(0, 1, n) is exactly arange(n) * (1/(n-1)) with the final
        # element pinned to 1.0; reproduce it so results stay bit-identical.
        t = local * np.repeat(1.0 / (n - 1).astype(np.float64), n)
        t[off + n - 1] = 1.0
        t = t[:, None]

        v0 = np.repeat(edge_verts[lo:hi, 0], n, axis=0)
        v1 = np.repeat(edge_verts[lo:hi, 1], n, axis=0)
        pts = v0 * (1 - t) + v1 * t

        chunks.append(np.unique(_grid_keys(pts, bounds_min, voxel_size, grid_dims)))
        lo = hi

    return np.concatenate(chunks) if len(chunks) > 1 else chunks[0]


def voxelize_surface(mesh, voxel_size, max_samples=8_000_000, seed=0x5CE2):
    """
    Surface-only voxelization via stratified point sampling.

    Strategy
    --------
    1.  Area-weighted stratified sampling across all triangles — ensures every
        face contributes proportionally to its area.
    2.  Extra passes along mesh edges and at every vertex — catches thin walls
        and sharp features that face sampling can miss.
    3.  Snap all world-space points to the voxel grid, deduplicate.

    Memory: O(n_samples + n_voxels).  No dense 3-D boolean array is built.
    """
    bounds_min = mesh.bounding_box.bounds[0]
    bounds_max = mesh.bounding_box.bounds[1]
    extents    = bounds_max - bounds_min

    # Grid dimensions (add 1 to account for floating-point rounding at edges)
    grid_dims  = (np.ceil(extents / voxel_size) + 1).astype(np.int32)

    # ── 1. Face (area-weighted) sampling ────────────────────────────────
    # Target: ~12 samples per voxel-face worth of surface area so every
    # surface voxel is hit with high probability.
    surface_area     = max(mesh.area, 1e-12)
    voxel_face_area  = voxel_size ** 2
    n_face_samples   = int(surface_area / voxel_face_area * 12)
    n_face_samples   = max(min(n_face_samples, max_samples), 100_000)

    # Seeded so the same model and settings always yield the same voxel set.
    # Face sampling is probabilistic, so an unseeded RNG makes marginal
    # voxels appear or vanish between otherwise identical conversions.
    face_pts, _ = trimesh.sample.sample_surface(mesh, n_face_samples, seed=seed)

    # ── 2. Edge sampling — subdivide each unique edge into segments ──────
    edges     = mesh.edges_unique                          # (E, 2)
    edge_verts = mesh.vertices[edges]                     # (E, 2, 3)
    edge_lens  = np.linalg.norm(edge_verts[:, 1] - edge_verts[:, 0], axis=1)
    # One sample per voxel_size length along the edge (at least 2 per edge)
    segs_per_edge = np.maximum((edge_lens / voxel_size).astype(int) + 1, 2)

    # ── 3. Merge, snap to grid, deduplicate ─────────────────────────────
    # Each pass is reduced to grid keys independently — flooring is elementwise,
    # so this yields the same voxel set as snapping one concatenated point array.
    keys = np.concatenate([
        _grid_keys(face_pts, bounds_min, voxel_size, grid_dims),
        _edge_sample_keys(edge_verts, segs_per_edge, bounds_min, voxel_size, grid_dims),
        _grid_keys(mesh.vertices, bounds_min, voxel_size, grid_dims),
    ])
    keys = np.unique(keys)

    dy, dz = int(grid_dims[1]), int(grid_dims[2])
    coords = np.empty((len(keys), 3), dtype=np.int32)
    coords[:, 0] = keys // (dy * dz)
    coords[:, 1] = (keys // dz) % dy
    coords[:, 2] = keys % dz

    # World-space origin of voxel (0, 0, 0)
    voxel_world_origin = bounds_min.astype(np.float32)

    return coords, float(voxel_size), tuple(int(d) for d in grid_dims), voxel_world_origin


def parse_direction(text):
    """Parse an "x,y,z" light direction, or None if it is missing or malformed."""
    if not text:
        return None
    try:
        parts = [float(p) for p in text.split(",")]
    except ValueError:
        return None
    return parts if len(parts) == 3 else None


def lighting_from_args(args):
    """Collect the lighting flags that were actually given.

    Keys left out fall through to `sample_colors.DEFAULT_LIGHTING`, so adding a
    flag here never silently redefines a default in two places.
    """
    given = {
        "light_dir": parse_direction(args.light_dir),
        "ambient": args.light_ambient,
        "gloss": args.light_gloss,
        "specular": args.specular,
        "rejection": args.highlight_rejection,
        "recovery": args.highlight_recovery,
        "delight": args.delight,
    }
    return {k: v for k, v in given.items() if v is not None}


def main():
    parser = argparse.ArgumentParser(description="Trimesh voxelizer for SchemGen2")
    parser.add_argument("--input",      "-i", required=True)
    parser.add_argument("--max-size",   type=int,   default=128)
    parser.add_argument("--voxel-size", type=float, default=None)
    # hollow/solid flags kept for CLI compatibility; sampling is always surface-only
    parser.add_argument("--hollow", action="store_true", default=True)
    parser.add_argument("--solid",  action="store_true", default=False)
    # Sample per-voxel surface colors in the same process. Saves a second
    # Python startup and a second parse of the GLB.
    parser.add_argument("--sample-colors", action="store_true", default=False)
    parser.add_argument("--ram-limit", type=float, default=4.0)
    parser.add_argument("--samples-per-voxel", type=int, default=24)
    # Lighting separation — see sample_colors.DEFAULT_LIGHTING. Omitted flags
    # keep the sampler's own defaults rather than being forced to a value here.
    parser.add_argument("--light-dir", type=str, default=None,
                        help="Key light direction in model space, as x,y,z")
    parser.add_argument("--light-ambient", type=float, default=None,
                        help="Light still reaching surfaces facing away, 0..1")
    parser.add_argument("--light-gloss", type=float, default=None,
                        help="How sharp a highlight to assume was baked in, 0..1")
    parser.add_argument("--specular", type=float, default=None,
                        help="Gain on the highlight lobe")
    parser.add_argument("--highlight-rejection", type=float, default=None,
                        help="How hard to discount samples inside the lobe, 0..1")
    parser.add_argument("--highlight-recovery", type=float, default=None,
                        help="How far to rebuild blown voxels from material albedo, 0..1")
    parser.add_argument("--delight", type=float, default=None,
                        help="Strength of removing baked lighting from albedo, 0..1")
    args = parser.parse_args()

    try:
        meshes = load_meshes(args.input)
        mesh = concatenated(meshes)

        extents = mesh.bounding_box.extents
        voxel_size = args.voxel_size
        if voxel_size is None:
            voxel_size = max(extents) / args.max_size

        coords, pitch, grid_dims, origin = voxelize_surface(mesh, voxel_size)

        if len(coords) == 0:
            raise ValueError(
                "Voxelization produced no voxels. "
                "Try a smaller --voxel-size or check that the mesh has surface geometry."
            )

        result = {
            "coords":             coords.tolist(),
            "pitch":              pitch,
            "grid_dims":          list(grid_dims),
            "voxel_world_origin": origin.tolist(),
        }

        if args.sample_colors:
            from sample_colors import sample_colors
            colors = sample_colors(
                meshes, coords, pitch, origin,
                ram_limit=args.ram_limit,
                samples_per_voxel=max(1, args.samples_per_voxel),
                lighting=lighting_from_args(args),
            )
            result["colors"] = colors.tolist()

        print(json.dumps(result))
        sys.stdout.flush()

    except Exception as e:
        print(f"ERROR: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
