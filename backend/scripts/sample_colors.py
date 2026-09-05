#!/usr/bin/env python3
"""
sample_colors.py — Per-voxel mesh surface color sampling using trimesh + scipy.

Usage:
    python sample_colors.py --input model.glb --coords coords.json [--voxel-pitch 0.1]
        [--voxel-origin 0,0,0] [--ram-limit 4.0] [--samples-per-voxel 16]

Reads voxel coordinates from a JSON file, outputs RGB colors as JSON array to stdout.

Method
------
A voxel covers a patch of surface, not a point. Sampling one point per voxel
aliases badly whenever the texture is finer than the voxel grid — typically
tens to hundreds of texels fall inside a single voxel footprint.

So the surface is supersampled instead: points are scattered over every
triangle with density proportional to area, shaded, binned into the voxel grid,
and averaged. That makes each voxel an area-weighted average of the surface it
actually contains, which is what the block-matching stage wants.

Averaging happens in linear light with alpha as the weight, so half-covered
texels and transparent cutouts contribute proportionally instead of dragging
the result toward black.

Voxels that catch no samples — thin features picked up by the voxelizer's edge
and vertex passes — fall back to an exact closest-point-on-triangle lookup.
"""

import argparse
import json
import sys
import time
import numpy as np
from PIL import Image
from scipy.spatial import cKDTree


# ── sRGB transfer function ──────────────────────────────────────────────

def _srgb_to_linear(c):
    """c in [0,1] → linear. Vectorized, matches the sRGB EOTF."""
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def _linear_to_srgb(c):
    """Inverse of `_srgb_to_linear`."""
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1.0 / 2.4) - 0.055)


# 8-bit texels decode through a lookup table rather than the pow() above.
SRGB8_TO_LINEAR = _srgb_to_linear(np.arange(256, dtype=np.float64) / 255.0).astype(np.float32)


def base_color(mesh):
    """Material base-color factor as linear RGB in [0,1].

    glTF defines baseColorFactor in linear space, and a material that omits it
    means white — no tint. Deliberately does *not* consult `visual.main_color`
    or an untouched `SimpleMaterial.diffuse`: trimesh synthesizes those as a
    grey placeholder when the file said nothing, and multiplying a texture by
    that placeholder darkens the whole model by about half.

    Values arrive as 0..1 floats or 0..255 bytes depending on the source, so
    normalize by range rather than assuming one.
    """
    try:
        mat = getattr(mesh.visual, "material", None)
        if mat is not None:
            for attr in ("baseColorFactor", "diffuse", "ambient"):
                c = getattr(mat, attr, None)
                if c is None:
                    continue
                c = np.asarray(c, dtype=np.float64).ravel()[:3]
                if c.size == 3:
                    return np.clip(c / 255.0 if c.max() > 1.01 else c, 0.0, 1.0).astype(np.float32)
    except Exception:
        pass
    # glTF's default material is plain white.
    return np.ones(3, dtype=np.float32)


def load_texture(mesh):
    """Return texture ndarray (H, W, 4) RGBA uint8, or None."""
    try:
        v = mesh.visual
        if hasattr(v, "material"):
            mat = v.material
            for attr in ("image", "baseColorTexture", "diffuseTexture"):
                tex = getattr(mat, attr, None)
                if tex is None:
                    continue
                img = tex.image if hasattr(tex, "image") else tex
                if not isinstance(img, Image.Image):
                    try:
                        img = Image.fromarray(np.asarray(img))
                    except Exception:
                        continue
                return np.asarray(img.convert("RGBA"), dtype=np.uint8)
    except Exception:
        pass
    return None


def bilinear_sample(tex, uv, base_linear):
    """Bilinear texture fetch in linear light.

    Returns (rgb_linear (N,3) float32, alpha (N,) float32).

    Texel centers sit at half-integer coordinates and coordinates wrap
    (glTF's default REPEAT), so tiled UVs outside [0,1] sample correctly.
    RGB decodes from sRGB before filtering — blending encoded values is what
    makes naive samplers look muddy — while alpha is already linear.
    """
    th, tw = tex.shape[:2]
    u = uv[:, 0]
    v = uv[:, 1]

    px = u * tw - 0.5
    py = (1.0 - v) * th - 0.5          # trimesh flips glTF's V on load

    fx = np.floor(px)
    fy = np.floor(py)
    dx = (px - fx).astype(np.float32)[:, None]
    dy = (py - fy).astype(np.float32)[:, None]

    x0 = np.mod(fx.astype(np.int64), tw)
    y0 = np.mod(fy.astype(np.int64), th)
    x1 = np.mod(x0 + 1, tw)
    y1 = np.mod(y0 + 1, th)

    t00 = tex[y0, x0]
    t10 = tex[y0, x1]
    t01 = tex[y1, x0]
    t11 = tex[y1, x1]

    c00 = SRGB8_TO_LINEAR[t00[:, :3]]
    c10 = SRGB8_TO_LINEAR[t10[:, :3]]
    c01 = SRGB8_TO_LINEAR[t01[:, :3]]
    c11 = SRGB8_TO_LINEAR[t11[:, :3]]
    rgb = c00 + (c10 - c00) * dx + (c01 - c00) * dy + (c00 - c10 - c01 + c11) * dx * dy

    if tex.shape[2] == 4:
        a00 = t00[:, 3].astype(np.float32)
        a10 = t10[:, 3].astype(np.float32)
        a01 = t01[:, 3].astype(np.float32)
        a11 = t11[:, 3].astype(np.float32)
        dx0, dy0 = dx[:, 0], dy[:, 0]
        alpha = (a00 + (a10 - a00) * dx0 + (a01 - a00) * dy0
                 + (a00 - a10 - a01 + a11) * dx0 * dy0) / 255.0
    else:
        alpha = np.ones(len(uv), dtype=np.float32)

    # glTF multiplies texture and factor in linear space.
    return rgb * base_linear[None, :], alpha


def sample_nearest_texels(tex, uv):
    """Nearest-neighbor fetch of raw uint8 texels (REPEAT wrap, V flipped).

    Used for metallicRoughness and emissive maps, where bilinear filtering
    buys nothing; the caller decides how to decode the channels.
    """
    th, tw = tex.shape[:2]
    x = np.mod(np.floor(uv[:, 0] * tw).astype(np.int64), tw)
    y = np.mod(np.floor((1.0 - uv[:, 1]) * th).astype(np.int64), th)
    return tex[y, x]


def pbr_properties(mesh):
    """Metallic/roughness/emissive description of one mesh's material.

    glTF's *default* metallicFactor is 1.0, but a material that carries
    neither the factor nor a metallicRoughness texture almost never means
    "mirror" — exporters simply omitted the PBR block. Treating that case as
    metallic would darken every plain textured model, so it maps to 0.
    """
    metallic = 0.0
    roughness = 1.0
    mr_tex = None
    emissive_factor = None
    emissive_tex = None
    try:
        mat = getattr(mesh.visual, "material", None)
        if mat is not None:
            mf = getattr(mat, "metallicFactor", None)
            rf = getattr(mat, "roughnessFactor", None)
            tex = getattr(mat, "metallicRoughnessTexture", None)
            if tex is not None:
                img = tex.image if hasattr(tex, "image") else tex
                if isinstance(img, Image.Image):
                    mr_tex = np.asarray(img.convert("RGB"), dtype=np.uint8)
            if mf is not None:
                metallic = float(np.clip(mf, 0.0, 1.0))
            elif mr_tex is not None:
                metallic = 1.0          # spec default applies as a multiplier
            if rf is not None:
                roughness = float(np.clip(rf, 0.0, 1.0))

            ef = getattr(mat, "emissiveFactor", None)
            if ef is not None:
                ef = np.asarray(ef, dtype=np.float32).ravel()[:3]
                if ef.size == 3 and ef.max() > 0.0:
                    emissive_factor = ef
            etex = getattr(mat, "emissiveTexture", None)
            if etex is not None:
                img = etex.image if hasattr(etex, "image") else etex
                if isinstance(img, Image.Image):
                    emissive_tex = np.asarray(img.convert("RGB"), dtype=np.uint8)
                    if emissive_factor is None:
                        # Texture present with no factor → spec default 0 would
                        # kill it; exporters that set a texture mean full white.
                        emissive_factor = np.ones(3, dtype=np.float32)
    except Exception:
        pass
    return {
        "metallic_factor": metallic,
        "roughness_factor": roughness,
        "mr_texture": mr_tex,
        "emissive_factor": emissive_factor,
        "emissive_texture": emissive_tex,
    }


# ── Assumed lighting model ──────────────────────────────────
#
# One directional key light drives three jobs that all have to agree with each
# other — and with what the preview draws:
#
#   * baking a response into metals, which have no diffuse term of their own;
#   * de-lighting — dividing lighting back out of a base-color texture that an
#     exporter already baked it into, which is what turns a shiny dark surface
#     into a white one;
#   * deciding which samples sit inside the specular lobe, so they can be
#     discounted instead of dragging their voxel toward white.

SPEC_EXPONENT = 16.0

# Default key light (unit vector, from above-front).
DEFAULT_LIGHT_DIR = np.array([0.35, 0.85, 0.40], dtype=np.float32)
DEFAULT_LIGHT_DIR = DEFAULT_LIGHT_DIR / np.linalg.norm(DEFAULT_LIGHT_DIR)

DEFAULT_LIGHTING = {
    "light_dir": DEFAULT_LIGHT_DIR,
    # Fraction of full illumination still reaching surfaces facing away.
    "ambient": 0.32,
    # Gain on the highlight lobe. 0 leaves metals with diffuse response only.
    "specular": 1.1,
    # How sharp a highlight to assume the texture was baked with.
    "gloss": 0.5,
    # How hard to discount samples sitting inside that lobe.
    "rejection": 0.75,
    # How far a blown-out voxel is pulled back toward its material's albedo.
    "recovery": 1.0,
    # Strength of dividing the assumed lighting back out of the albedo.
    "delight": 0.0,
}

# Rejection never reaches 1.0: a voxel whose samples are *all* highlight must
# still resolve to its own color rather than dropping out of the scatter pass
# and falling through to the nearest-surface path.
MAX_REJECTION = 0.95

# Below this the de-light divisor stops shrinking. A sample facing away from
# the light carries almost no signal, and dividing by a near-zero response
# would amplify whatever noise it does carry into confetti.
DELIGHT_FLOOR = 0.25

# Linear-light window over which a sample counts as "clipped". Linear 0.85 and
# 0.99 are 8-bit texels 243 and 253 — the top of the encodable range, where a
# renderer that overshot has already thrown the color away.
CLIP_LO, CLIP_HI = 0.85, 0.99

# A sample has to be nearly untouched by rejection to speak for its material.
REF_KEEP_MIN = 0.95

# Past this many primitives, per-voxel material tracking costs more memory than
# the recovery pass is worth, and one global reference albedo is used instead.
MAX_TRACKED_SOURCES = 8


def lighting_options(**overrides):
    """Merge caller overrides onto the defaults and put them in range."""
    opts = dict(DEFAULT_LIGHTING)
    for key, value in overrides.items():
        if key in opts and value is not None:
            opts[key] = value

    d = np.asarray(opts["light_dir"], dtype=np.float32).ravel()[:3]
    length = float(np.linalg.norm(d)) if d.size == 3 else 0.0
    opts["light_dir"] = DEFAULT_LIGHT_DIR if length < 1e-8 else (d / length).astype(np.float32)

    opts["ambient"] = float(np.clip(opts["ambient"], 0.0, 1.0))
    opts["specular"] = float(max(opts["specular"], 0.0))
    opts["gloss"] = float(np.clip(opts["gloss"], 0.0, 1.0))
    # Scaled here so callers get a plain 0..1 knob and the ceiling stays internal.
    opts["rejection"] = float(np.clip(opts["rejection"], 0.0, 1.0)) * MAX_REJECTION
    opts["recovery"] = float(np.clip(opts["recovery"], 0.0, 1.0))
    opts["delight"] = float(np.clip(opts["delight"], 0.0, 1.0))
    return opts


def lighting_needs_normals(light):
    """Whether these settings make the sampler depend on surface normals."""
    return light["rejection"] > 0.0 or light["delight"] > 0.0


def _unit_normals(normals):
    """Normalized normals, plus a mask of the ones that were not degenerate."""
    n = np.asarray(normals, dtype=np.float32)
    length = np.linalg.norm(n, axis=1, keepdims=True)
    return n / np.maximum(length, 1e-8), length[:, 0] > 1e-8


def _smoothstep(x, lo, hi):
    t = np.clip((x - lo) / max(hi - lo, 1e-6), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def lighting_response(ndl, ambient, specular, gloss):
    """Brightness the assumed key light puts on a surface with this N·L.

    An ambient floor, a broad diffuse term and a gloss-sharpened highlight —
    the same curve whether it is being baked in or divided back out.
    """
    return ambient + (1.0 - ambient) * ndl + specular * gloss * ndl ** SPEC_EXPONENT


def specular_fraction(ndl, ambient, specular, gloss):
    """Share of `lighting_response` coming from the highlight lobe, 0..1.

    This is the signal highlight rejection keys on, and it is deliberately
    geometric: it asks whether the assumed light would put a highlight *here*,
    never whether a texel happens to be bright and white. Genuinely white
    texels on a flat surface all score identically, so the weighted mean over
    them comes out unchanged — only a highlight falling across part of a
    curved surface actually moves a voxel.
    """
    spec = specular * gloss * ndl ** SPEC_EXPONENT
    total = lighting_response(ndl, ambient, specular, gloss)
    return np.clip(spec / np.maximum(total, 1e-6), 0.0, 1.0)


def assumed_gloss(light, material_gloss=None):
    """Gloss to assume for a surface: the shinier of material and light setting."""
    if material_gloss is None:
        return light["gloss"]
    return np.maximum(light["gloss"], material_gloss)


def apply_metal_shading(rgb, normals, metallic, roughness, light=None):
    """Bake simple directional lighting into the metallic part of a surface.

    An unlit albedo sample is fine for diffuse materials — Minecraft's own
    block lighting supplies the depth cues — but metals have no diffuse term:
    their rendered look *is* the lighting response, so sampling raw base color
    turns chrome and gold into flat cardboard. This approximates a studio
    environment: an ambient floor, a broad N·L term, and a gloss-sharpened
    highlight, blended in by each sample's metalness. Dielectric samples
    (metallic = 0) pass through bit-identically.
    """
    light = DEFAULT_LIGHTING if light is None else light
    m = np.asarray(metallic, dtype=np.float32)
    if normals is None or not np.any(m > 1e-3):
        return rgb

    n, good = _unit_normals(normals)
    ndl = np.clip(n @ light["light_dir"], 0.0, 1.0)

    gloss = (1.0 - np.asarray(roughness, dtype=np.float32)) ** 2
    shade = lighting_response(ndl, light["ambient"], light["specular"], gloss)

    # lerp(rgb, rgb*shade, metallic); degenerate normals shade as flat.
    shade = np.where(good, shade, 1.0)
    factor = 1.0 + m * (shade - 1.0)
    return rgb * factor[:, None]


def apply_delight(rgb, normals, light, material_gloss=None):
    """Take the assumed baked lighting back out of an albedo sample.

    The two halves of `lighting_response` come off in different ways, and
    getting that split right is the whole point:

      * the highlight is reflected *light*, which a white source adds equally
        to every channel, so it is **subtracted**. Dividing it out instead is
        what leaves a blown highlight as grey rather than restoring the color
        under it — and on a dark surface, where the highlight is nearly all of
        the signal, division barely moves it at all;
      * the diffuse term scales the albedo, so it is **divided** out. The
        curve peaks at 1, so this only ever lifts the shadowed side back up
        rather than brightening the model as a whole.

    Exact for lighting this file baked, and a fair approximation of the
    studio-lit renders that photogrammetry and generated-mesh pipelines ship
    as "base color".
    """
    if light["delight"] <= 0.0 or normals is None:
        return rgb

    n, good = _unit_normals(normals)
    ndl = np.clip(n @ light["light_dir"], 0.0, 1.0)
    gloss = assumed_gloss(light, material_gloss)

    strength = light["delight"]
    specular = strength * light["specular"] * gloss * ndl ** SPEC_EXPONENT
    diffuse = light["ambient"] + (1.0 - light["ambient"]) * ndl
    divisor = 1.0 + strength * (diffuse - 1.0)

    # Degenerate normals carry no direction to reason about, so they pass through.
    specular = np.where(good, specular, 0.0)
    divisor = np.where(good, divisor, 1.0)

    # Floored: a sample facing away from the light carries almost no signal,
    # and dividing by a near-zero response amplifies its noise into confetti.
    return np.maximum(rgb - specular[:, None], 0.0) / np.maximum(divisor, DELIGHT_FLOOR)[:, None]


def highlight_weight(rgb, normals, light, material_gloss=None):
    """Per-sample weight that discounts samples sitting in the specular lobe.

    This is what makes "shiny black stays black" work: a voxel that catches a
    highlight across part of its footprint averages the surviving body samples
    instead of the blown ones. Because the weights are normalized away, a voxel
    whose samples *all* score the same — a flat white wall, say — comes out
    unchanged, so this cannot quietly darken legitimately bright surfaces.
    """
    m = len(rgb)
    if light["rejection"] <= 0.0 or normals is None or m == 0:
        return np.ones(m, dtype=np.float32)

    n, good = _unit_normals(normals)
    ndl = np.clip(n @ light["light_dir"], 0.0, 1.0)
    frac = specular_fraction(ndl, light["ambient"], light["specular"],
                             assumed_gloss(light, material_gloss))

    # Geometric gate: would the assumed light put a highlight here at all? The
    # lobe is narrow, so this is close to binary — which is what keeps the
    # discount off the rest of the surface. A gloss of 0 opens no lobe and
    # therefore rejects nothing.
    in_lobe = _smoothstep(frac, 0.02, 0.25)

    # A sample whose brightest channel reached the top of the encodable range
    # has lost its color to clipping outright, so inside the lobe it is
    # discounted hard. One that is merely brightened still carries its hue and
    # is only partly discounted.
    clipped = _smoothstep(np.asarray(rgb, dtype=np.float32).max(axis=1), CLIP_LO, CLIP_HI)
    suspicion = np.clip(in_lobe * (0.35 + 0.65 * clipped), 0.0, 1.0)

    keep = 1.0 - light["rejection"] * suspicion
    return np.where(good, keep, 1.0).astype(np.float32)


def make_color_source(mesh):
    """Prepare one mesh's color data without losing its material."""
    tex_data = load_texture(mesh)
    face_uvs = None
    vert_colors = None
    face_colors = None
    base_linear = base_color(mesh)
    pbr = pbr_properties(mesh)

    needs_uv = (tex_data is not None or pbr["mr_texture"] is not None
                or pbr["emissive_texture"] is not None)
    if needs_uv:
        uv = getattr(mesh.visual, "uv", None)
        if uv is not None and len(uv):
            uvs = np.asarray(uv, dtype=np.float32).copy()
            # Raw normalized-integer accessors occasionally arrive undecoded.
            # Values are left unclamped otherwise so tiled UVs keep tiling.
            if uvs.max() > 100.0:
                uvs /= 65535.0
            face_uvs = uvs[np.asarray(mesh.faces, dtype=np.int32)]
        else:
            if tex_data is not None:
                print("No UV map — ignoring texture", file=sys.stderr)
            tex_data = None
            pbr["mr_texture"] = None
            pbr["emissive_texture"] = None

    if tex_data is None and getattr(mesh.visual, "vertex_colors", None) is not None:
        vc = np.asarray(mesh.visual.vertex_colors, dtype=np.float32)[:, :3]
        if vc.size and vc.max() > 1.01:
            vc = vc / 255.0
        vc = SRGB8_TO_LINEAR[np.clip(vc * 255.0, 0, 255).astype(np.uint8)]
        vert_colors = vc[np.asarray(mesh.faces, dtype=np.int32)] * base_linear[None, None, :]

    if tex_data is None and vert_colors is None:
        try:
            fc = mesh.visual.to_color().face_colors
            if fc is not None:
                fc = np.asarray(fc, dtype=np.float32)[:, :3]
                if fc.size and fc.max() > 1.01:
                    fc = fc / 255.0
                fc = SRGB8_TO_LINEAR[np.clip(fc * 255.0, 0, 255).astype(np.uint8)]
                face_colors = fc * base_linear[None, :]
        except Exception:
            pass

    return {
        "texture": tex_data,
        "face_uvs": face_uvs,
        "vertex_colors": vert_colors,
        "face_colors": face_colors,
        "base_color": base_linear,
        "pbr": pbr,
    }


def shade(sources, face_sources, source_faces, faces, weights, normals=None,
          pbr_shading=True, light=None):
    """Shade surface points in linear light.

    `faces` indexes the merged face table; `weights` holds the barycentric
    coordinates of each point and `normals` the interpolated surface normals
    (used by everything that depends on the light direction). Returns
    (rgb_linear (M,3), alpha (M,), keep (M,)), where `keep` is how much the
    caller should trust each sample — see `highlight_weight`.
    """
    light = DEFAULT_LIGHTING if light is None else light
    m = len(faces)
    rgb = np.empty((m, 3), dtype=np.float32)
    alpha = np.ones(m, dtype=np.float32)
    keep = np.ones(m, dtype=np.float32)
    if m == 0:
        return rgb, alpha, keep

    batch_sources = face_sources[faces]
    for source_index in np.unique(batch_sources):
        rows = np.flatnonzero(batch_sources == source_index)
        source = sources[source_index]
        local_faces = source_faces[faces[rows]]
        pbr = source["pbr"]

        uv = None
        if source["face_uvs"] is not None:
            uv_face = source["face_uvs"][local_faces]
            uv = np.einsum("ni,nij->nj", weights[rows], uv_face)

        if source["texture"] is not None and uv is not None:
            rgb[rows], alpha[rows] = bilinear_sample(source["texture"], uv, source["base_color"])
        elif source["vertex_colors"] is not None:
            rgb[rows] = np.einsum("ni,nij->nj", weights[rows], source["vertex_colors"][local_faces])
        elif source["face_colors"] is not None:
            rgb[rows] = source["face_colors"][local_faces]
        else:
            rgb[rows] = source["base_color"]

        if not pbr_shading:
            continue

        sub_normals = None if normals is None else normals[rows]

        # Metallic response — factors, optionally modulated by the
        # metallicRoughness texture (G = roughness, B = metallic, linear).
        metallic = pbr["metallic_factor"]
        roughness = pbr["roughness_factor"]
        is_metal = metallic > 0.0 or pbr["mr_texture"] is not None
        if is_metal and pbr["mr_texture"] is not None and uv is not None:
            mr = sample_nearest_texels(pbr["mr_texture"], uv).astype(np.float32) / 255.0
            roughness = roughness * mr[:, 1]   # G channel, linear
            metallic = metallic * mr[:, 2]     # B channel, linear
        material_gloss = (1.0 - np.asarray(roughness, dtype=np.float32)) ** 2

        # Recover albedo first, then relight: de-lighting undoes whatever the
        # exporter baked into the texture, and the metal bake is ours to add.
        rgb[rows] = apply_delight(rgb[rows], sub_normals, light, material_gloss)

        if is_metal:
            rgb[rows] = apply_metal_shading(
                rgb[rows], sub_normals, metallic, roughness, light)

        # Emissive adds on top, unaffected by lighting.
        if pbr["emissive_factor"] is not None:
            emissive = pbr["emissive_factor"][None, :]
            if pbr["emissive_texture"] is not None and uv is not None:
                texels = sample_nearest_texels(pbr["emissive_texture"], uv)
                emissive = emissive * SRGB8_TO_LINEAR[texels[:, :3]]
            rgb[rows] = rgb[rows] + emissive

        # Scored before the clamp below, so a sample the bake pushed past white
        # still reads as blown rather than as an ordinary white one.
        keep[rows] = highlight_weight(rgb[rows], sub_normals, light, material_gloss)

    # A sample cannot be brighter than white. Clamping here rather than after
    # averaging stops one overshooting sample from inflating its whole voxel.
    np.clip(rgb, 0.0, 1.0, out=rgb)
    return rgb, alpha, keep


def closest_point_on_triangle(points, tri_v):
    """Vectorized closest point on triangle. Returns (B, 3)."""
    a = tri_v[:, 0, :]
    b = tri_v[:, 1, :]
    c = tri_v[:, 2, :]
    ab = b - a
    ac = c - a
    ap = points - a
    d00 = np.sum(ab * ab, axis=1)
    d01 = np.sum(ab * ac, axis=1)
    d11 = np.sum(ac * ac, axis=1)
    d20 = np.sum(ap * ab, axis=1)
    d21 = np.sum(ap * ac, axis=1)
    denom = d00 * d11 - d01 * d01
    denom[np.abs(denom) < 1e-12] = 1.0
    v = (d11 * d20 - d01 * d21) / denom
    w = (d00 * d21 - d01 * d20) / denom
    u = 1.0 - v - w
    u = np.clip(u, 0, 1)
    v = np.clip(v, 0, 1)
    w = np.clip(w, 0, 1)
    s = u + v + w
    s[s == 0] = 1.0
    u /= s
    v /= s
    w /= s
    return a * u[:, None] + b * v[:, None] + c * w[:, None]


def bary_weights(points, tri_v):
    """Compute (u, v, w) barycentric weights for points on triangle."""
    a = tri_v[:, 0, :]
    b = tri_v[:, 1, :]
    c = tri_v[:, 2, :]
    ab = b - a
    ac = c - a
    ap = points - a
    d00 = np.sum(ab * ab, axis=1)
    d01 = np.sum(ab * ac, axis=1)
    d11 = np.sum(ac * ac, axis=1)
    d20 = np.sum(ap * ab, axis=1)
    d21 = np.sum(ap * ac, axis=1)
    denom = d00 * d11 - d01 * d01
    denom[np.abs(denom) < 1e-12] = 1.0
    v = (d11 * d20 - d01 * d21) / denom
    w = (d00 * d21 - d01 * d20) / denom
    u = 1.0 - v - w
    u = np.clip(u, 0, 1)
    v = np.clip(v, 0, 1)
    w = np.clip(w, 0, 1)
    s = u + v + w
    s[s == 0] = 1.0
    u /= s
    v /= s
    w /= s
    return u, v, w


class VoxelBinner:
    """Maps world-space points onto the requested voxel set."""

    def __init__(self, voxel_coords, pitch, origin):
        self.pitch = pitch
        self.origin = np.asarray(origin, dtype=np.float64)
        self.hi = voxel_coords.max(axis=0).astype(np.int64)
        self.stride_y = int(self.hi[2]) + 1
        self.stride_x = (int(self.hi[1]) + 1) * self.stride_y
        keys = (voxel_coords[:, 0].astype(np.int64) * self.stride_x
                + voxel_coords[:, 1].astype(np.int64) * self.stride_y
                + voxel_coords[:, 2])
        self.order = np.argsort(keys, kind="stable")
        self.sorted_keys = keys[self.order]

    def bin(self, points):
        """Return (row indices into voxel_coords, indices of the points that hit).

        Both arrays are the same length: element i says point `landed[i]` fell
        into voxel `rows[i]`. Points outside the requested voxel set are dropped.
        """
        g = np.floor((points - self.origin) / self.pitch).astype(np.int64)
        inside = np.all((g >= 0) & (g <= self.hi), axis=1)
        g = g[inside]
        keys = g[:, 0] * self.stride_x + g[:, 1] * self.stride_y + g[:, 2]
        pos = np.searchsorted(self.sorted_keys, keys)
        np.clip(pos, 0, len(self.sorted_keys) - 1, out=pos)
        hit = self.sorted_keys[pos] == keys
        rows = self.order[pos[hit]]
        # Re-expand the "landed" mask to the original point indexing.
        landed = np.flatnonzero(inside)[hit]
        return rows, landed


def supersample(meshes_data, binner, n_voxels, voxel_area, density, max_samples,
                light=None, seed=0x5CE2, chunk=1_000_000):
    """Scatter points over the surface and accumulate per-voxel color sums.

    Returns a dict with the highlight-weighted color sums per voxel, the
    unweighted totals they get compared against to tell how much of a voxel was
    rejected as highlight, and a reference albedo per material accumulated only
    from samples the specular lobe never touched.
    """
    light = DEFAULT_LIGHTING if light is None else light
    tri_verts = meshes_data["tri_verts"]
    areas = meshes_data["areas"]
    n_sources = len(meshes_data["sources"])
    # Per-voxel material tracking is an (n_voxels x n_sources) table; past a
    # handful of primitives that outgrows what recovery is worth.
    track_sources = 0 < n_sources <= MAX_TRACKED_SOURCES

    # Sample count per triangle: proportional to how many voxel footprints it
    # spans, with at least one so no face is ever skipped entirely.
    per_face = np.ceil(areas / voxel_area * density).astype(np.int64)
    np.maximum(per_face, 1, out=per_face)
    total = int(per_face.sum())
    if total > max_samples:
        per_face = np.maximum((per_face * (max_samples / total)).astype(np.int64), 1)
        total = int(per_face.sum())

    sums = np.zeros((n_voxels, 3), dtype=np.float64)
    weights_total = np.zeros(n_voxels, dtype=np.float64)
    raw_total = np.zeros(n_voxels, dtype=np.float64)
    voxel_source_weights = (np.zeros((n_voxels, n_sources), dtype=np.float64)
                            if track_sources else None)
    reference_sums = np.zeros((max(n_sources, 1), 3), dtype=np.float64)
    reference_weights = np.zeros(max(n_sources, 1), dtype=np.float64)

    rng = np.random.default_rng(seed)
    ends = np.cumsum(per_face)
    starts = np.concatenate(([0], ends))

    lo = 0
    n_faces = len(per_face)
    while lo < n_faces:
        hi = int(np.searchsorted(starts, starts[lo] + chunk, side="right")) - 1
        hi = min(max(hi, lo + 1), n_faces)

        counts = per_face[lo:hi]
        faces = np.repeat(np.arange(lo, hi, dtype=np.int64), counts)
        m = len(faces)

        # Uniform barycentric coordinates over each triangle.
        r1 = rng.random(m)
        r2 = rng.random(m)
        su = np.sqrt(r1)
        bw = np.empty((m, 3), dtype=np.float64)
        bw[:, 0] = 1.0 - su
        bw[:, 1] = su * (1.0 - r2)
        bw[:, 2] = su * r2

        tri = tri_verts[faces]
        points = np.einsum("ni,nij->nj", bw, tri)

        rows, landed = binner.bin(points)
        if len(rows):
            tri_normals = meshes_data["tri_normals"]
            normals = None
            if tri_normals is not None:
                normals = np.einsum("ni,nij->nj", bw[landed],
                                    tri_normals[faces[landed]]).astype(np.float32)
            rgb, alpha, keep = shade(meshes_data["sources"], meshes_data["face_sources"],
                                     meshes_data["source_faces"], faces[landed],
                                     bw[landed].astype(np.float32), normals,
                                     light=light)
            raw = alpha.astype(np.float64)
            w = raw * keep.astype(np.float64)
            for c in range(3):
                sums[:, c] += np.bincount(rows, weights=rgb[:, c].astype(np.float64) * w,
                                          minlength=n_voxels)
            weights_total += np.bincount(rows, weights=w, minlength=n_voxels)
            # Kept separately: the gap between the two is how much of the voxel
            # was thrown away as highlight, which is what recovery keys on.
            raw_total += np.bincount(rows, weights=raw, minlength=n_voxels)

            # What each material looks like away from its highlights.
            src = meshes_data["face_sources"][faces[landed]]
            confident = keep >= REF_KEEP_MIN
            if np.any(confident):
                trusted, trusted_w = src[confident], w[confident]
                reference_weights += np.bincount(trusted, weights=trusted_w,
                                                 minlength=n_sources)
                for c in range(3):
                    reference_sums[:, c] += np.bincount(
                        trusted, weights=rgb[confident, c].astype(np.float64) * trusted_w,
                        minlength=n_sources)

            if track_sources:
                for source_index in np.unique(src):
                    pick = src == source_index
                    voxel_source_weights[:, source_index] += np.bincount(
                        rows[pick], weights=raw[pick], minlength=n_voxels)

        lo = hi

    return {
        "sums": sums,
        "weights": weights_total,
        "raw_weights": raw_total,
        "voxel_source_weights": voxel_source_weights,
        "reference_sums": reference_sums,
        "reference_weights": reference_weights,
        "n_samples": total,
    }


def reference_albedo(scatter, n_voxels):
    """Per-voxel albedo of the material that owns it, or None if unknown.

    Materials that were *entirely* highlight contribute no trusted samples of
    their own and fall back to the model-wide average, which is still a better
    answer than the clipped white they would otherwise keep.
    """
    sums = scatter["reference_sums"]
    weights = scatter["reference_weights"]
    if weights.sum() <= 1e-9:
        return None
    global_ref = (sums.sum(axis=0) / weights.sum()).astype(np.float32)

    owner = scatter["voxel_source_weights"]
    if owner is None:
        return np.tile(global_ref, (n_voxels, 1))

    per_source = np.tile(global_ref, (len(weights), 1))
    usable = weights > 1e-9
    per_source[usable] = (sums[usable] / weights[usable, None]).astype(np.float32)
    return per_source[np.argmax(owner, axis=1)]


def apply_highlight_recovery(linear, scatter, light, n_voxels):
    """Rebuild voxels whose samples were almost entirely specular highlight.

    Rejection alone cannot save a voxel where the highlight covered *every*
    sample: the weights normalize away and the blown color comes straight back
    out. What such a voxel does still have is a material, and that material was
    sampled away from the lobe elsewhere on the mesh. Blending toward its
    reference albedo is what keeps a polished black surface black instead of
    settling for the grey that dividing a clipped white gives you.
    """
    if light["recovery"] <= 0.0 or light["rejection"] <= 0.0:
        return linear

    raw = scatter["raw_weights"]
    kept = scatter["weights"]
    blown = np.zeros(n_voxels, dtype=np.float64)
    covered = raw > 1e-6
    # Weight can fall no further than the rejection ceiling, so dividing by it
    # turns the shortfall into a plain 0..1 "how much of this was highlight".
    blown[covered] = (1.0 - kept[covered] / raw[covered]) / max(light["rejection"], 1e-6)

    t = (_smoothstep(blown, 0.7, 0.98) * light["recovery"]).astype(np.float32)
    if not np.any(t > 1e-4):
        return linear

    ref = reference_albedo(scatter, n_voxels)
    if ref is None:
        return linear

    rebuilt = int(np.count_nonzero(t > 0.5))
    if rebuilt:
        print(f"Rebuilding {rebuilt} blown-out voxels from material albedo", file=sys.stderr)
    return (linear * (1.0 - t)[:, None] + ref * t[:, None]).astype(np.float32)


def nearest_surface_colors(meshes_data, centroids, pitch, batch_size, light=None):
    """Shade voxels the scatter missed, by averaging the surfaces crossing them.

    For each voxel the nearest triangles are collected and their closest points
    computed. Every one of those points that actually falls inside the voxel
    cube contributes, weighted by alpha and by how much surface the triangle
    can put in a single voxel — so a voxel straddling two materials blends them
    instead of picking whichever happens to be marginally nearer. If nothing
    lands inside (the voxel sits just off the surface), the single closest
    point is used.
    """
    light = DEFAULT_LIGHTING if light is None else light
    tri_verts = meshes_data["tri_verts"]
    areas = meshes_data["areas"]
    tree = meshes_data["tree"]
    n = len(centroids)
    out = np.empty((n, 3), dtype=np.float32)
    candidate_count = min(32, len(tri_verts))
    half = pitch * 0.5
    voxel_area = pitch * pitch

    for start in range(0, n, batch_size):
        end = min(start + batch_size, n)
        batch = centroids[start:end]
        b = len(batch)

        # Triangle centroids only form a search index. The wider shortlist
        # avoids borrowing a neighboring, differently colored surface.
        _, candidate_idx = tree.query(batch, k=candidate_count, workers=-1)
        if candidate_count == 1:
            candidate_idx = candidate_idx[:, None]
        candidate_tri = tri_verts[candidate_idx]
        candidate_points = np.repeat(batch[:, None, :], candidate_count, axis=1)
        candidate_closest = closest_point_on_triangle(
            candidate_points.reshape(-1, 3), candidate_tri.reshape(-1, 3, 3),
        ).reshape(b, candidate_count, 3)

        flat_idx = candidate_idx.reshape(-1)
        flat_closest = candidate_closest.reshape(-1, 3)
        u, v_, w = bary_weights(flat_closest, tri_verts[flat_idx])
        flat_bw = np.stack([u, v_, w], axis=1).astype(np.float32)
        tri_normals = meshes_data["tri_normals"]
        normals = None
        if tri_normals is not None:
            normals = np.einsum("ni,nij->nj", flat_bw,
                                tri_normals[flat_idx]).astype(np.float32)
        rgb, alpha, keep = shade(meshes_data["sources"], meshes_data["face_sources"],
                                 meshes_data["source_faces"], flat_idx, flat_bw,
                                 normals, light=light)
        rgb = rgb.reshape(b, candidate_count, 3).astype(np.float64)

        # A triangle far larger than a voxel still only fills one voxel's worth,
        # and a candidate sitting in the highlight speaks for the surface less.
        area_weight = np.minimum(areas[candidate_idx], voxel_area)
        weight = (alpha * keep).reshape(b, candidate_count).astype(np.float64) * area_weight
        inside = np.all(np.abs(candidate_closest - candidate_points) <= half, axis=2)
        weight *= inside

        total = weight.sum(axis=1)
        blended = np.einsum("bk,bkc->bc", weight, rgb)
        ok = total > 1e-12
        result = np.empty((b, 3), dtype=np.float64)
        result[ok] = blended[ok] / total[ok, None]

        # Nothing crossed the voxel — take the single nearest surface point.
        if not np.all(ok):
            miss = np.flatnonzero(~ok)
            nearest = np.argmin(
                np.sum((candidate_closest[miss] - candidate_points[miss]) ** 2, axis=2), axis=1)
            result[miss] = rgb[miss, nearest]

        out[start:end] = result

    return out


def build_mesh_data(meshes, light=None):
    """Merge every primitive into one face table, keeping per-source materials."""
    light = DEFAULT_LIGHTING if light is None else light
    sources = [make_color_source(mesh) for mesh in meshes]
    vertices, faces, face_sources, source_faces = [], [], [], []
    vertex_offset = 0
    for source_index, mesh in enumerate(meshes):
        verts = np.asarray(mesh.vertices, dtype=np.float32)
        local_faces = np.asarray(mesh.faces, dtype=np.int32)
        vertices.append(verts)
        faces.append(local_faces + vertex_offset)
        face_sources.append(np.full(len(local_faces), source_index, dtype=np.int32))
        source_faces.append(np.arange(len(local_faces), dtype=np.int32))
        vertex_offset += len(verts)

    verts = np.concatenate(vertices, axis=0)
    faces = np.concatenate(faces, axis=0)
    tri_verts = verts[faces]
    ab = tri_verts[:, 1] - tri_verts[:, 0]
    ac = tri_verts[:, 2] - tri_verts[:, 0]
    areas = 0.5 * np.linalg.norm(np.cross(ab, ac), axis=1).astype(np.float64)

    # Smooth per-vertex normals, gathered per face corner so samples can
    # interpolate them barycentrically. Needed by metallic shading and by
    # anything that reasons about where the key light falls.
    tri_normals = None
    if lighting_needs_normals(light) or any(
            s["pbr"]["metallic_factor"] > 0.0 or s["pbr"]["mr_texture"] is not None
            for s in sources):
        normal_parts = []
        for mesh in meshes:
            try:
                vn = np.asarray(mesh.vertex_normals, dtype=np.float32)
            except Exception:
                vn = np.zeros((len(mesh.vertices), 3), dtype=np.float32)
            normal_parts.append(np.nan_to_num(vn))
        vert_normals = np.concatenate(normal_parts, axis=0)
        tri_normals = vert_normals[faces]

    for source in sources:
        if source["texture"] is not None:
            print(f"Texture found: {source['texture'].shape}", file=sys.stderr)
        if source["pbr"]["metallic_factor"] > 0.0 or source["pbr"]["mr_texture"] is not None:
            print(f"Metallic material: factor={source['pbr']['metallic_factor']:.2f} "
                  f"roughness={source['pbr']['roughness_factor']:.2f} "
                  f"mr_texture={'yes' if source['pbr']['mr_texture'] is not None else 'no'}",
                  file=sys.stderr)

    return {
        "sources": sources,
        "tri_verts": tri_verts,
        "tri_normals": tri_normals,
        "areas": areas,
        "face_sources": np.concatenate(face_sources),
        "source_faces": np.concatenate(source_faces),
        "tree": cKDTree(tri_verts.mean(axis=1)),
    }


def sample_colors(meshes, voxel_coords, voxel_pitch, voxel_world_origin,
                  ram_limit=4.0, batch_size=None, samples_per_voxel=24,
                  lighting=None):
    """Sample colors while preserving every GLB primitive's material.

    `lighting` overrides `DEFAULT_LIGHTING` — the key light direction and how
    hard to work at separating baked lighting from albedo.
    """
    light = lighting_options(**(lighting or {}))
    N = len(voxel_coords)
    if N == 0:
        return np.zeros((0, 3), dtype=np.float32)

    meshes = [mesh for mesh in meshes if len(mesh.faces)]
    if not meshes:
        raise ValueError("No faces in file")
    F = sum(len(mesh.faces) for mesh in meshes)
    if batch_size is None:
        base = int(ram_limit * 12000)
        if F > 2000000:
            batch_size = min(base, 1000)
        elif F > 1000000:
            batch_size = min(base, 2000)
        elif F > 500000:
            batch_size = min(base, 4000)
        elif F > 100000:
            batch_size = min(base, 15000)
        else:
            batch_size = min(base, 50000)

    print(f"Sampling {N} voxels | faces={F} | {samples_per_voxel} samples/voxel",
          file=sys.stderr)
    d = light["light_dir"]
    print(f"Lighting: dir=({d[0]:.2f},{d[1]:.2f},{d[2]:.2f}) ambient={light['ambient']:.2f} "
          f"specular={light['specular']:.2f} gloss={light['gloss']:.2f} "
          f"rejection={light['rejection']:.2f} recovery={light['recovery']:.2f} "
          f"delight={light['delight']:.2f}", file=sys.stderr)
    t0 = time.time()

    data = build_mesh_data(meshes, light)
    binner = VoxelBinner(voxel_coords, voxel_pitch, voxel_world_origin)

    max_samples = max(int(ram_limit * 16_000_000), 4_000_000)
    scatter = supersample(
        data, binner, N, voxel_pitch ** 2, samples_per_voxel, max_samples, light,
    )
    weights = scatter["weights"]
    print(f"Scattered {scatter['n_samples']} surface samples in {time.time() - t0:.1f}s",
          file=sys.stderr)

    covered = weights > 1e-6
    linear = np.zeros((N, 3), dtype=np.float32)
    linear[covered] = (scatter["sums"][covered] / weights[covered, None]).astype(np.float32)
    linear = apply_highlight_recovery(linear, scatter, light, N)

    # Voxels the scatter missed (thin features, fully transparent texels).
    missing = np.flatnonzero(~covered)
    if len(missing):
        print(f"Falling back to nearest-surface for {len(missing)} voxels", file=sys.stderr)
        voxel_origin = np.asarray(voxel_world_origin, dtype=np.float32)
        centroids = (voxel_coords[missing].astype(np.float32) * voxel_pitch
                     + voxel_origin + voxel_pitch * 0.5)
        linear[missing] = nearest_surface_colors(data, centroids, voxel_pitch,
                                                 batch_size, light)

    print(f"Done sampling in {time.time() - t0:.1f}s", file=sys.stderr)
    return (_linear_to_srgb(linear.astype(np.float64)) * 255.0).astype(np.float32)


def main():
    parser = argparse.ArgumentParser(description="Per-voxel color sampler for SchemGen2")
    parser.add_argument("--input", "-i", required=True, help="Input GLB file")
    parser.add_argument("--coords", required=True, help="JSON file with voxel coords array [[x,y,z],...]")
    parser.add_argument("--voxel-pitch", type=float, required=True, help="Voxel pitch in world units")
    parser.add_argument("--voxel-origin", type=str, default="0,0,0",
                        help="Voxel world origin as x,y,z")
    parser.add_argument("--ram-limit", type=float, default=4.0, help="RAM limit in GB")
    parser.add_argument("--batch-size", type=int, default=None, help="Override batch size")
    parser.add_argument("--samples-per-voxel", type=int, default=24,
                        help="Surface samples averaged per voxel (higher = smoother)")
    args = parser.parse_args()

    import trimesh

    # Load mesh
    scene = trimesh.load(args.input, force=None, process=False)
    if isinstance(scene, trimesh.Scene):
        if not scene.geometry:
            raise ValueError("No geometry in file")
        # Applies node transforms while preserving each primitive material.
        meshes = scene.dump(concatenate=False)
    elif isinstance(scene, trimesh.Trimesh):
        meshes = [scene]
    else:
        raise ValueError(f"Unsupported type: {type(scene)}")

    # Load coords
    with open(args.coords) as f:
        coords_list = json.load(f)
    coords = np.array(coords_list, dtype=np.int32)

    # Parse origin
    origin_parts = [float(x) for x in args.voxel_origin.split(",")]

    colors = sample_colors(
        meshes, coords, args.voxel_pitch, origin_parts,
        ram_limit=args.ram_limit, batch_size=args.batch_size,
        samples_per_voxel=max(1, args.samples_per_voxel),
    )

    # Output as JSON array of [r, g, b]. tolist() widens each float32 to the
    # same double a per-element float() call produces, just far faster.
    json.dump(colors.tolist(), sys.stdout)
    sys.stdout.flush()


if __name__ == "__main__":
    main()
