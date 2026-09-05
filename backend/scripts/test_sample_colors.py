#!/usr/bin/env python3
"""Tests for the per-voxel color sampler.

Run with:  python scripts/test_sample_colors.py

Each case builds a mesh whose correct per-voxel color is known analytically,
so the assertions pin down behaviour the old point sampler got wrong:
area-averaging, linear-light blending, alpha weighting, and the glTF rule that
a missing baseColorFactor means white rather than grey.
"""

import sys
import numpy as np
from PIL import Image

sys.path.insert(0, __file__.rsplit("\\", 1)[0].rsplit("/", 1)[0])

import trimesh
from sample_colors import sample_colors, _srgb_to_linear, _linear_to_srgb


PITCH = 0.25
ORIGIN = [0.0, 0.0, 0.0]


def quad(uv_scale=1.0):
    """Unit quad in the z=0 plane, UVs covering the texture once."""
    verts = np.array([[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]], dtype=np.float64)
    faces = np.array([[0, 1, 2], [0, 2, 3]], dtype=np.int64)
    # trimesh flips V on glTF load, so author UVs in its bottom-left convention.
    uv = np.array([[0, 0], [1, 0], [1, 1], [0, 1]], dtype=np.float64) * uv_scale
    return verts, faces, uv


def textured_quad(image, uv_scale=1.0, base_factor=None):
    verts, faces, uv = quad(uv_scale)
    mat = trimesh.visual.material.PBRMaterial(baseColorTexture=image)
    if base_factor is not None:
        mat.baseColorFactor = base_factor
    m = trimesh.Trimesh(vertices=verts, faces=faces, process=False)
    m.visual = trimesh.visual.TextureVisuals(uv=uv, material=mat)
    return m


def all_voxels():
    """The 4x4 voxel grid the unit quad occupies at PITCH=0.25."""
    n = int(round(1.0 / PITCH))
    return np.array([[x, y, 0] for x in range(n) for y in range(n)], dtype=np.int32)


def run(mesh, coords=None, **kw):
    coords = all_voxels() if coords is None else coords
    return sample_colors([mesh], coords, PITCH, ORIGIN, ram_limit=4.0, **kw)


CHECKS = []


def check(name):
    def deco(fn):
        CHECKS.append((name, fn))
        return fn
    return deco


@check("uniform texture reproduces its color exactly")
def _():
    img = Image.new("RGBA", (64, 64), (200, 100, 50, 255))
    out = run(textured_quad(img))
    assert np.allclose(out, [200, 100, 50], atol=1.0), out[:3]


@check("checkerboard averages in linear light, not sRGB")
def _():
    # 1-texel black/white checker: 64x64 texels over 4x4 voxels = 256 per voxel.
    a = np.indices((64, 64)).sum(axis=0) % 2
    img = Image.fromarray(np.dstack([a * 255] * 3 + [np.full_like(a, 255)]).astype(np.uint8), "RGBA")
    out = run(textured_quad(img), samples_per_voxel=512)
    # Correct answer averages linearly: 0.5 linear -> ~188 sRGB.
    # Averaging the encoded bytes instead would give 128; point sampling 0 or 255.
    expected = float(_linear_to_srgb(np.array([0.5]))[0] * 255.0)
    assert abs(expected - 187.5) < 1.0, expected
    err = np.abs(out - expected).max()
    assert err < 6.0, f"expected ~{expected:.1f}, got {out.mean(axis=0)} (max dev {err:.1f})"
    assert np.abs(out - 128.0).min() > 20.0, "looks like an sRGB-space average"


@check("half the texture transparent -> opaque color wins, no grey wash")
def _():
    px = np.zeros((64, 64, 4), dtype=np.uint8)
    px[..., :3] = (255, 0, 0)
    px[:, :32, 3] = 255          # left half opaque red
    px[:, 32:, 3] = 0            # right half fully transparent
    out = run(textured_quad(Image.fromarray(px, "RGBA")), samples_per_voxel=512)
    # Alpha weighting must ignore the transparent texels entirely.
    assert np.allclose(out, [255, 0, 0], atol=6.0), out.mean(axis=0)


@check("missing baseColorFactor means white, not a grey tint")
def _():
    img = Image.new("RGBA", (64, 64), (255, 255, 255, 255))
    mesh = textured_quad(img)
    assert mesh.visual.material.baseColorFactor is None
    out = run(mesh)
    # Trusting trimesh's placeholder main_color here would land near 128.
    assert np.allclose(out, 255, atol=1.0), f"texture was tinted: {out.mean(axis=0)}"


@check("explicit baseColorFactor multiplies in linear space")
def _():
    img = Image.new("RGBA", (64, 64), (255, 255, 255, 255))
    out = run(textured_quad(img, base_factor=[0.5, 0.5, 0.5, 1.0]))
    expected = float(_linear_to_srgb(np.array([0.5]))[0] * 255.0)
    assert np.allclose(out, expected, atol=2.0), f"expected ~{expected:.1f}, got {out.mean(axis=0)}"


@check("tiled UVs outside [0,1] wrap instead of being clamped")
def _():
    px = np.zeros((64, 64, 4), dtype=np.uint8)
    px[..., 3] = 255
    px[:32] = (255, 0, 0, 255)     # top half red
    px[32:] = (0, 0, 255, 255)     # bottom half blue
    flat = run(textured_quad(Image.fromarray(px, "RGBA"), uv_scale=1.0), samples_per_voxel=512)
    tiled = run(textured_quad(Image.fromarray(px, "RGBA"), uv_scale=4.0), samples_per_voxel=512)
    # Tiling 4x puts both bands inside every voxel, so all of them go purple.
    # Clamping the UVs (the old behaviour) would leave the banding untouched.
    assert tiled[:, 0].min() > 150 and tiled[:, 2].min() > 150, f"UVs look clamped: {tiled}"
    # Unscaled, the quad keeps a red half and a blue half. (Neither is exactly 0
    # because REPEAT wrapping bleeds a little across the texture's own seam.)
    assert flat[:, 0].min() < 50 and flat[:, 0].max() > 200, f"expected bands: {flat}"


@check("vertex colors are interpolated and averaged")
def _():
    verts, faces, _ = quad()
    m = trimesh.Trimesh(vertices=verts, faces=faces, process=False)
    m.visual = trimesh.visual.ColorVisuals(
        mesh=m, vertex_colors=np.array([[255, 0, 0, 255]] * 4, dtype=np.uint8))
    out = run(m)
    assert np.allclose(out, [255, 0, 0], atol=3.0), out.mean(axis=0)


@check("untextured material color is used")
def _():
    verts, faces, _ = quad()
    m = trimesh.Trimesh(vertices=verts, faces=faces, process=False)
    mat = trimesh.visual.material.PBRMaterial(baseColorFactor=[1.0, 0.0, 0.0, 1.0])
    m.visual = trimesh.visual.TextureVisuals(material=mat)
    out = run(m)
    assert np.allclose(out, [255, 0, 0], atol=2.0), out.mean(axis=0)


@check("voxels off the surface still get the nearest surface color")
def _():
    img = Image.new("RGBA", (64, 64), (10, 220, 90, 255))
    # A voxel layer above the quad catches no scatter samples and must fall back.
    coords = np.array([[x, y, z] for x in range(4) for y in range(4) for z in (0, 1)],
                      dtype=np.int32)
    out = run(textured_quad(img), coords=coords)
    assert np.allclose(out, [10, 220, 90], atol=3.0), out


@check("rough metal bakes N.L shading instead of flat albedo")
def _():
    from sample_colors import DEFAULT_LIGHT_DIR as LIGHT_DIR
    img = Image.new("RGBA", (64, 64), (255, 255, 255, 255))
    mesh = textured_quad(img)
    mesh.visual.material.metallicFactor = 1.0   # roughness defaults to 1
    out = run(mesh)
    # Quad normal is +z; shade = 0.32 + 0.68 * (n . L), no gloss term.
    ndl = float(LIGHT_DIR[2])
    expected = float(_linear_to_srgb(np.array([0.32 + 0.68 * ndl]))[0] * 255.0)
    assert np.allclose(out, expected, atol=4.0), \
        f"expected ~{expected:.0f}, got {out.mean(axis=0)}"
    # And it must differ from the dielectric result (flat 255).
    assert out.max() < 240.0, "metallic surface came back unshaded"


@check("polished metal catches a highlight rough metal doesn't")
def _():
    # Upward-facing quad: its normal is close to the key light (n.L = 0.85),
    # which is where the gloss term actually contributes. At grazing angles
    # both finishes correctly shade the same.
    def up_quad(roughness):
        verts = np.array([[0, 0, 0], [0, 0, 1], [1, 0, 1], [1, 0, 0]], dtype=np.float64)
        faces = np.array([[0, 1, 2], [0, 2, 3]], dtype=np.int64)
        uv = np.array([[0, 0], [1, 0], [1, 1], [0, 1]], dtype=np.float64)
        img = Image.new("RGBA", (64, 64), (180, 180, 180, 255))
        mat = trimesh.visual.material.PBRMaterial(
            baseColorTexture=img, metallicFactor=1.0, roughnessFactor=roughness)
        m = trimesh.Trimesh(vertices=verts, faces=faces, process=False)
        m.visual = trimesh.visual.TextureVisuals(uv=uv, material=mat)
        return m

    coords = np.array([[x, 0, z] for x in range(4) for z in range(4)], dtype=np.int32)
    shiny = run(up_quad(0.0), coords=coords)
    rough = run(up_quad(1.0), coords=coords)
    assert shiny.mean() > rough.mean() + 2.0, (shiny.mean(), rough.mean())


@check("metallicFactor omitted entirely means dielectric, not chrome")
def _():
    img = Image.new("RGBA", (64, 64), (200, 100, 50, 255))
    mesh = textured_quad(img)
    assert mesh.visual.material.metallicFactor is None
    out = run(mesh)
    assert np.allclose(out, [200, 100, 50], atol=1.0), out.mean(axis=0)


@check("emissive factor adds light on top of base color")
def _():
    verts, faces, _ = quad()
    m = trimesh.Trimesh(vertices=verts, faces=faces, process=False)
    mat = trimesh.visual.material.PBRMaterial(
        baseColorFactor=[0.0, 0.0, 0.0, 1.0], emissiveFactor=[1.0, 0.0, 0.0])
    m.visual = trimesh.visual.TextureVisuals(material=mat)
    out = run(m)
    assert np.allclose(out, [255, 0, 0], atol=2.0), out.mean(axis=0)


@check("results are deterministic across runs")
def _():
    img = Image.new("RGBA", (64, 64), (123, 45, 67, 255))
    a = run(textured_quad(img))
    b = run(textured_quad(img))
    assert np.array_equal(a, b), "sampler should be reproducible"


@check("empty input is handled")
def _():
    img = Image.new("RGBA", (8, 8), (1, 2, 3, 255))
    out = run(textured_quad(img), coords=np.zeros((0, 3), dtype=np.int32))
    assert out.shape == (0, 3), out.shape


@check("sRGB transfer round-trips")
def _():
    x = np.linspace(0, 1, 4096)
    assert np.abs(_linear_to_srgb(_srgb_to_linear(x)) - x).max() < 1e-9


# ── Lighting separation ─────────────────────────────────────────────────────


def shiny_dark_sphere(albedo=0.02):
    """A near-black sphere carrying a blown-out specular highlight.

    Built the way a renderer actually blows one out: the specular term is
    *additive* white light, not a tint on the albedo, which is why a shiny
    black surface comes back white while its albedo is still black.
    """
    from sample_colors import DEFAULT_LIGHT_DIR

    sphere = trimesh.creation.uv_sphere(radius=0.45, count=[64, 64])
    sphere.apply_translation([0.5, 0.5, 0.5])
    ndl = np.clip(np.asarray(sphere.vertex_normals) @ DEFAULT_LIGHT_DIR, 0.0, 1.0)
    lit = np.clip(albedo * (0.32 + 0.68 * ndl) + ndl ** 16, 0.0, 1.0)
    grey = (_linear_to_srgb(lit) * 255.0).astype(np.uint8)
    rgba = np.column_stack([grey, grey, grey, np.full(len(grey), 255, np.uint8)])
    sphere.visual = trimesh.visual.ColorVisuals(mesh=sphere, vertex_colors=rgba)
    return sphere


def sphere_voxels():
    n = int(round(1.0 / PITCH))
    return np.array([[x, y, z] for x in range(n) for y in range(n) for z in range(n)],
                    dtype=np.int32)


@check("highlight rejection pulls back a voxel the highlight only partly covers")
def _():
    # One voxel swallowing the whole sphere: most of what it contains is the
    # near-black body, and a small cap of it is highlight. Rejection reweights
    # *within* a voxel, so this is the shape of problem it addresses — a voxel
    # the highlight covers entirely has nothing left to reweight toward, which
    # is what de-lighting is for.
    sphere = shiny_dark_sphere()
    one = np.array([[0, 0, 0]], dtype=np.int32)
    off = sample_colors([sphere], one, 1.0, ORIGIN,
                        lighting={"rejection": 0.0, "recovery": 0.0})
    on = sample_colors([sphere], one, 1.0, ORIGIN, lighting={"gloss": 1.0})
    assert on.max() < off.max() * 0.95, (on.max(), off.max())


@check("de-lighting takes a blown highlight off a shiny black surface")
def _():
    sphere = shiny_dark_sphere()
    coords = sphere_voxels()
    off = sample_colors([sphere], coords, PITCH, ORIGIN,
                        lighting={"rejection": 0.0, "recovery": 0.0, "delight": 0.0})
    on = sample_colors([sphere], coords, PITCH, ORIGIN,
                       lighting={"gloss": 1.0, "delight": 1.0})
    assert off.max() > 120.0, f"mesh should blow out first, got {off.max():.0f}"
    # The albedo under the highlight is near-black; recovering it is the point.
    assert on.max() < 60.0, f"highlight survived de-lighting: {on.max():.0f}"


@check("highlight rejection leaves a uniformly bright surface alone")
def _():
    # The safety property: rejection reweights samples *within* a voxel, so a
    # surface whose samples all score the same must come out untouched — white
    # paint must not be mistaken for a highlight.
    img = Image.new("RGBA", (64, 64), (255, 255, 255, 255))
    out = run(textured_quad(img), lighting={"rejection": 1.0, "gloss": 1.0})
    assert np.allclose(out, 255.0, atol=1.0), out.mean(axis=0)


@check("gloss 0 opens no lobe, so rejection changes nothing")
def _():
    sphere = shiny_dark_sphere()
    coords = sphere_voxels()
    a = sample_colors([sphere], coords, PITCH, ORIGIN,
                      lighting={"rejection": 0.0, "recovery": 0.0})
    b = sample_colors([sphere], coords, PITCH, ORIGIN,
                      lighting={"rejection": 1.0, "gloss": 0.0})
    assert np.allclose(a, b, atol=0.5), np.abs(a - b).max()


@check("de-lighting flattens lighting that was baked into the texture")
def _():
    from sample_colors import DEFAULT_LIGHT_DIR as L

    albedo = 0.18

    def baked(ndl):
        """Solid texture holding albedo already lit by the key light.

        Built the way a renderer builds it: the diffuse term scales the albedo
        and the highlight adds white on top.
        """
        v = albedo * (0.32 + 0.68 * ndl) + 1.1 * 0.5 * ndl ** 16
        level = int(round(float(_linear_to_srgb(np.asarray([v]))[0]) * 255.0))
        return Image.new("RGBA", (32, 32), (level, level, level, 255))

    def up_quad(image):
        verts = np.array([[0, 0, 0], [0, 0, 1], [1, 0, 1], [1, 0, 0]], dtype=np.float64)
        faces = np.array([[0, 1, 2], [0, 2, 3]], dtype=np.int64)
        uv = np.array([[0, 0], [1, 0], [1, 1], [0, 1]], dtype=np.float64)
        m = trimesh.Trimesh(vertices=verts, faces=faces, process=False)
        m.visual = trimesh.visual.TextureVisuals(
            uv=uv, material=trimesh.visual.material.PBRMaterial(baseColorTexture=image))
        return m

    # Same albedo, two orientations: one facing the light (n.L = 0.85), one
    # nearly edge-on to it (n.L = 0.40).
    facing = up_quad(baked(float(L[1])))
    edge_on = textured_quad(baked(float(L[2])))
    side_coords = np.array([[x, 0, z] for x in range(4) for z in range(4)], dtype=np.int32)

    lit_ratio = (run(facing, coords=side_coords).mean() / run(edge_on).mean())
    flat_ratio = (run(facing, coords=side_coords, lighting={"delight": 1.0}).mean()
                  / run(edge_on, lighting={"delight": 1.0}).mean())

    assert lit_ratio > 1.2, f"test setup should be visibly lit, ratio={lit_ratio:.3f}"
    assert abs(flat_ratio - 1.0) < 0.05, f"de-light left a {flat_ratio:.3f} gradient"


@check("de-lighting is off unless asked for")
def _():
    img = Image.new("RGBA", (64, 64), (200, 100, 50, 255))
    assert np.array_equal(run(textured_quad(img)),
                          run(textured_quad(img), lighting={"delight": 0.0}))


@check("lighting options are normalized and clamped")
def _():
    from sample_colors import lighting_options, DEFAULT_LIGHT_DIR, MAX_REJECTION

    opts = lighting_options(light_dir=[0.0, 10.0, 0.0], ambient=5.0, gloss=-1.0,
                            rejection=3.0, delight=-2.0)
    assert np.allclose(opts["light_dir"], [0.0, 1.0, 0.0]), opts["light_dir"]
    assert opts["ambient"] == 1.0 and opts["gloss"] == 0.0
    assert opts["rejection"] == MAX_REJECTION and opts["delight"] == 0.0
    # A zero-length direction is meaningless, not an error.
    assert np.allclose(lighting_options(light_dir=[0, 0, 0])["light_dir"], DEFAULT_LIGHT_DIR)


def main():
    import io, contextlib
    failures = 0
    for name, fn in CHECKS:
        try:
            with contextlib.redirect_stderr(io.StringIO()):
                fn()
            print(f"  PASS  {name}")
        except AssertionError as e:
            failures += 1
            print(f"  FAIL  {name}\n          {e}")
        except Exception as e:
            failures += 1
            print(f"  ERROR {name}\n          {type(e).__name__}: {e}")
    print(f"\n{len(CHECKS) - failures}/{len(CHECKS)} passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
