//! Per-voxel surface colors — a port of the Python helper's `sample_colors`.
//!
//! A voxel covers a patch of surface, not a point, and textures are usually
//! finer than the voxel grid, so one lookup per voxel aliases. The surface is
//! supersampled instead: points are scattered over every triangle by area,
//! shaded, binned into their voxels and averaged in linear light with alpha
//! as the weight. Voxels no sample reached — thin features the edge and
//! vertex passes found — take the surfaces crossing them instead.
//!
//! The lighting model (metal shading, de-lighting, highlight rejection and
//! recovery) is the Python sampler's, constant for constant, and so is the
//! random stream: sampling a model gives each voxel the same color to within
//! floating-point rounding.
//!
//! What a material *means* is [`Materials::Gltf`]: the glTF specification,
//! which is also what the web UI's model preview draws. The Python helper saw
//! materials through trimesh, which lost some of them on the way; the
//! differences are listed in `docs/pipeline.md`. [`Materials::Trimesh`]
//! reproduces that reading so the parity harness can compare like for like.

use std::num::NonZeroUsize;
use std::time::Instant;

use kiddo::ImmutableKdTree;
use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::mesh::{AlphaMode, Image, Model, Primitive, TextureRef, Wrap};
use crate::pipeline::{Progress, Stage};
use crate::rng::{pairwise_sum, NumpyRng};
use crate::types::LightingOptions;
use crate::voxel::SEED;

/// Surface samples averaged per voxel.
pub const SAMPLES_PER_VOXEL: u32 = 24;

/// Sharpness of the assumed highlight lobe.
const SPEC_EXPONENT: f32 = 16.0;
/// Rejection never reaches 1: a voxel whose samples are all highlight must
/// still resolve to its own color rather than fall through to the
/// nearest-surface pass.
const MAX_REJECTION: f64 = 0.95;
/// Below this the de-light divisor stops shrinking, so a sample facing away
/// from the light does not have its noise amplified into confetti.
const DELIGHT_FLOOR: f32 = 0.25;
/// Linear-light window over which a sample counts as clipped (8-bit 243–253).
const CLIP_LO: f64 = 0.85;
const CLIP_HI: f64 = 0.99;
/// A sample this untouched by rejection may speak for its material's albedo.
const REF_KEEP_MIN: f32 = 0.95;
/// Past this many primitives, per-voxel material tracking costs more than
/// recovery is worth, and one model-wide reference albedo is used instead.
const MAX_TRACKED_SOURCES: usize = 8;
/// Samples shaded per scatter pass — also where the Python helper's random
/// draws switch from one pass to the next, which is why it is fixed.
const CHUNK: i64 = 1_000_000;
/// Samples per parallel task within a pass.
const TASK: i64 = 1 << 16;
/// Distinct UV sets one material reads at most (base, metallic-roughness and
/// emissive textures).
const UV_SLOTS: usize = 3;
/// trimesh's color for a mesh with neither material nor vertex colors.
const TRIMESH_GREY: u8 = 102;
const DEFAULT_LIGHT_DIR: [f32; 3] = [0.35, 0.85, 0.40];

/// How a model's materials are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Materials {
    /// The glTF specification, as the web UI's preview draws it.
    #[default]
    Gltf,
    /// The Python helper's reading through trimesh, for the parity harness.
    Trimesh,
}

#[derive(Debug, Clone, Copy)]
pub struct SampleOptions {
    pub materials: Materials,
    pub samples_per_voxel: u32,
    /// Memory budget in GB; caps the number of samples.
    pub ram_limit: f32,
}

impl Default for SampleOptions {
    fn default() -> Self {
        SampleOptions {
            materials: Materials::Gltf,
            samples_per_voxel: SAMPLES_PER_VOXEL,
            ram_limit: 4.0,
        }
    }
}

/// The Python helper received every setting as the decimal text of an `f32`;
/// widening through that text keeps `0.32` meaning 0.32.
pub(crate) fn widen(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

// ---- Lighting ---------------------------------------------------------------

/// The assumed key light, in range — `sample_colors.lighting_options`.
#[derive(Debug, Clone, Copy)]
struct Light {
    dir: [f32; 3],
    ambient: f64,
    specular: f64,
    gloss: f64,
    /// Already scaled by [`MAX_REJECTION`].
    rejection: f64,
    recovery: f64,
    delight: f64,
}

impl Light {
    fn new(options: &LightingOptions) -> Self {
        let normalize = |d: [f32; 3]| {
            let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            (length >= 1e-8).then(|| d.map(|v| v / length))
        };
        Light {
            dir: normalize(options.light_dir)
                .or_else(|| normalize(DEFAULT_LIGHT_DIR))
                .expect("the default light has a direction"),
            ambient: widen(options.ambient).clamp(0.0, 1.0),
            specular: widen(options.specular).max(0.0),
            gloss: widen(options.gloss).clamp(0.0, 1.0),
            rejection: widen(options.rejection).clamp(0.0, 1.0) * MAX_REJECTION,
            recovery: widen(options.recovery).clamp(0.0, 1.0),
            delight: widen(options.delight).clamp(0.0, 1.0),
        }
    }

    fn needs_normals(&self) -> bool {
        self.rejection > 0.0 || self.delight > 0.0
    }

    /// N·L for a normal, or `None` for a degenerate one.
    fn facing(&self, normal: Option<[f32; 3]>) -> Option<f32> {
        let n = normal?;
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length <= 1e-8 {
            return None;
        }
        let n = n.map(|v| v / length.max(1e-8));
        let d = self.dir;
        Some((n[0] * d[0] + n[1] * d[1] + n[2] * d[2]).clamp(0.0, 1.0))
    }

    /// Brightness the light puts on a surface: ambient floor, broad diffuse
    /// term and a gloss-sharpened highlight.
    fn response(&self, ndl: f32, gloss: f32) -> f32 {
        self.ambient as f32
            + (1.0 - self.ambient) as f32 * ndl
            + self.specular as f32 * gloss * ndl.powf(SPEC_EXPONENT)
    }

    /// Share of [`Self::response`] that is highlight — geometric, never a
    /// judgment about how bright a texel is.
    fn specular_fraction(&self, ndl: f32, gloss: f32) -> f32 {
        let spec = self.specular as f32 * gloss * ndl.powf(SPEC_EXPONENT);
        (spec / self.response(ndl, gloss).max(1e-6)).clamp(0.0, 1.0)
    }

    /// The shinier of the material and the light setting.
    fn assumed_gloss(&self, material_gloss: f32) -> f32 {
        (self.gloss as f32).max(material_gloss)
    }

    /// Take the assumed baked lighting back out of an albedo sample: the
    /// highlight is added light, so it is subtracted; diffuse shading scales
    /// the albedo, so it is divided out.
    fn delight(&self, rgb: [f32; 3], ndl: Option<f32>, material_gloss: f32) -> [f32; 3] {
        let Some(ndl) = ndl.filter(|_| self.delight > 0.0) else {
            return rgb;
        };
        let gloss = self.assumed_gloss(material_gloss);
        let strength = self.delight;
        let specular = (strength * self.specular) as f32 * gloss * ndl.powf(SPEC_EXPONENT);
        let diffuse = self.ambient as f32 + (1.0 - self.ambient) as f32 * ndl;
        let divisor = 1.0 + strength as f32 * (diffuse - 1.0);
        rgb.map(|c| (c - specular).max(0.0) / divisor.max(DELIGHT_FLOOR))
    }

    /// Bake the light into the metallic part of a surface, which has no
    /// diffuse color of its own.
    fn metal(&self, rgb: [f32; 3], ndl: Option<f32>, metallic: f32, roughness: f32) -> [f32; 3] {
        let gloss = (1.0 - roughness) * (1.0 - roughness);
        let shade = ndl.map_or(1.0, |ndl| self.response(ndl, gloss));
        let factor = 1.0 + metallic * (shade - 1.0);
        rgb.map(|c| c * factor)
    }

    /// How much to trust a sample: those sitting in the highlight lobe count
    /// for less, clipped ones much less.
    fn keep(&self, rgb: [f32; 3], ndl: Option<f32>, material_gloss: f32) -> f32 {
        let Some(ndl) = ndl.filter(|_| self.rejection > 0.0) else {
            return 1.0;
        };
        let frac = self.specular_fraction(ndl, self.assumed_gloss(material_gloss));
        let in_lobe = smoothstep(frac, 0.02, 0.25);
        let brightest = rgb[0].max(rgb[1]).max(rgb[2]);
        let clipped = smoothstep(brightest, CLIP_LO, CLIP_HI);
        let suspicion = (in_lobe * (0.35 + 0.65 * clipped)).clamp(0.0, 1.0);
        1.0 - self.rejection as f32 * suspicion
    }
}

fn smoothstep(x: f32, lo: f64, hi: f64) -> f32 {
    let t = ((x - lo as f32) / (hi - lo).max(1e-6) as f32).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep64(x: f64, lo: f64, hi: f64) -> f64 {
    let t = ((x - lo) / (hi - lo).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ---- Color spaces -----------------------------------------------------------

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// 8-bit sRGB to linear, by table.
struct Srgb8([f32; 256]);

impl Srgb8 {
    fn new() -> Self {
        Srgb8(std::array::from_fn(|i| {
            srgb_to_linear(i as f64 / 255.0) as f32
        }))
    }
    fn rgb(&self, texel: [u8; 4]) -> [f32; 3] {
        [
            self.0[texel[0] as usize],
            self.0[texel[1] as usize],
            self.0[texel[2] as usize],
        ]
    }
}

// ---- Textures ---------------------------------------------------------------

/// A texture as one material slot uses it.
struct Texture<'m> {
    image: &'m Image,
    /// Which per-source UV set ([`Source::uvs`]) it reads.
    uv_slot: usize,
    transform: Option<[[f32; 3]; 2]>,
    wrap: [Wrap; 2],
    nearest: bool,
}

fn wrap_index(i: i64, n: i64, wrap: Wrap) -> i64 {
    match wrap {
        Wrap::Repeat => i.rem_euclid(n),
        Wrap::Clamp => i.clamp(0, n - 1),
        Wrap::Mirror => {
            let k = i.rem_euclid(2 * n);
            if k < n {
                k
            } else {
                2 * n - 1 - k
            }
        }
    }
}

impl Texture<'_> {
    fn uv(&self, uv: [f32; 2]) -> [f32; 2] {
        match self.transform {
            Some(m) => [
                m[0][0] * uv[0] + m[0][1] * uv[1] + m[0][2],
                m[1][0] * uv[0] + m[1][1] * uv[1] + m[1][2],
            ],
            None => uv,
        }
    }

    /// The texel under a glTF UV (V pointing down the image).
    fn nearest(&self, uv: [f32; 2]) -> [u8; 4] {
        let (w, h) = (self.image.width as i64, self.image.height as i64);
        let x = wrap_index((uv[0] * w as f32).floor() as i64, w, self.wrap[0]);
        let y = wrap_index((uv[1] * h as f32).floor() as i64, h, self.wrap[1]);
        self.image.texel(x as u32, y as u32)
    }

    /// Bilinear filtering in linear light (alpha is linear already).
    /// `py` is the texel row coordinate before the half-texel shift.
    fn bilinear(&self, lut: &Srgb8, px: f32, py: f32) -> ([f32; 3], f32) {
        let (w, h) = (self.image.width as i64, self.image.height as i64);
        let (px, py) = (px - 0.5, py - 0.5);
        let (fx, fy) = (px.floor(), py.floor());
        let (dx, dy) = (px - fx, py - fy);
        let x0 = wrap_index(fx as i64, w, self.wrap[0]);
        let y0 = wrap_index(fy as i64, h, self.wrap[1]);
        let x1 = wrap_index(fx as i64 + 1, w, self.wrap[0]);
        let y1 = wrap_index(fy as i64 + 1, h, self.wrap[1]);
        let t = |x: i64, y: i64| self.image.texel(x as u32, y as u32);
        let (t00, t10, t01, t11) = (t(x0, y0), t(x1, y0), t(x0, y1), t(x1, y1));
        let (c00, c10, c01, c11) = (lut.rgb(t00), lut.rgb(t10), lut.rgb(t01), lut.rgb(t11));
        let rgb = std::array::from_fn(|k| {
            c00[k]
                + (c10[k] - c00[k]) * dx
                + (c01[k] - c00[k]) * dy
                + (c00[k] - c10[k] - c01[k] + c11[k]) * dx * dy
        });
        let a = |t: [u8; 4]| f32::from(t[3]);
        let (a00, a10, a01, a11) = (a(t00), a(t10), a(t01), a(t11));
        let alpha =
            (a00 + (a10 - a00) * dx + (a01 - a00) * dy + (a00 - a10 - a01 + a11) * dx * dy) / 255.0;
        (rgb, alpha)
    }

    /// Bilinear lookup at a glTF UV.
    fn sample(&self, lut: &Srgb8, uv: [f32; 2]) -> ([f32; 3], f32) {
        if self.nearest {
            let texel = self.nearest(uv);
            return (lut.rgb(texel), f32::from(texel[3]) / 255.0);
        }
        let (w, h) = (self.image.width as f32, self.image.height as f32);
        self.bilinear(lut, uv[0] * w, uv[1] * h)
    }
}

// ---- Color sources ----------------------------------------------------------

/// One primitive's material, resolved for per-sample shading.
struct Source<'m> {
    triangles: &'m [[u32; 3]],
    /// Per-vertex UV sets the textures read. Under [`Materials::Trimesh`]
    /// there is one, `TEXCOORD_0` with V flipped, as trimesh stores it.
    uvs: Vec<Vec<[f32; 2]>>,
    trimesh: bool,
    /// Linear base color factor, applied to every sample.
    base: [f32; 3],
    base_alpha: f32,
    alpha_mode: AlphaMode,
    texture: Option<Texture<'m>>,
    /// Per-vertex colors: linear RGBA under glTF; under trimesh, the byte
    /// colors decoded as sRGB and multiplied by the base, alpha 1.
    vertex_colors: Option<Vec<[f32; 4]>>,
    metallic: f32,
    roughness: f32,
    is_metal: bool,
    metallic_roughness: Option<Texture<'m>>,
    emissive: Option<[f32; 3]>,
    emissive_texture: Option<Texture<'m>>,
    /// A specular-glossiness material, converted per sample.
    specular_glossiness: Option<SpecGloss<'m>>,
}

/// `KHR_materials_pbrSpecularGlossiness` inputs, resolved.
struct SpecGloss<'m> {
    diffuse: [f32; 4],
    diffuse_texture: Option<Texture<'m>>,
    specular: [f32; 3],
    glossiness: f32,
    texture: Option<Texture<'m>>,
    /// trimesh baked the conversion into 8-bit textures (truncating) when
    /// the material had any; reproduce the rounding under trimesh.
    bytes: bool,
}

impl<'m> Source<'m> {
    fn new(model: &'m Model, primitive: &'m Primitive, materials: Materials, lut: &Srgb8) -> Self {
        match materials {
            Materials::Gltf => Self::gltf(model, primitive),
            Materials::Trimesh => Self::trimesh(model, primitive, lut),
        }
    }

    fn gltf(model: &'m Model, primitive: &'m Primitive) -> Self {
        let default = crate::mesh::Material::default();
        let material = model.material_of(primitive).unwrap_or(&default);
        let mut uvs: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut texture = |t: &Option<TextureRef>| -> Option<Texture<'m>> {
            let t = t.as_ref()?;
            let image = model.image(t)?;
            let set = primitive.tex_coords(t.tex_coord)?;
            let uv_slot = uvs
                .iter()
                .position(|u| u.as_slice() == set)
                .unwrap_or_else(|| {
                    uvs.push(set.to_vec());
                    uvs.len() - 1
                });
            Some(Texture {
                image,
                uv_slot,
                transform: t.transform,
                wrap: t.wrap,
                nearest: t.nearest,
            })
        };
        let base_texture = texture(&material.base_color_texture);
        let metallic_roughness = texture(&material.metallic_roughness_texture);
        let emissive_texture = texture(&material.emissive_texture);
        let specular_glossiness = material.specular_glossiness.as_ref().map(|sg| SpecGloss {
            diffuse: sg.diffuse.map(|c| c as f32),
            diffuse_texture: texture(&sg.diffuse_texture),
            specular: sg.specular,
            glossiness: sg.glossiness,
            texture: texture(&sg.specular_glossiness_texture),
            bytes: false,
        });
        let metallic = metalness(
            material.metallic,
            has_image(model, &material.metallic_roughness_texture),
        );
        let emissive = material
            .emissive
            .map(|e| e.map(|c| c * material.emissive_strength))
            .filter(|e| e.iter().any(|&c| c > 0.0));
        Source {
            triangles: &primitive.triangles,
            uvs,
            trimesh: false,
            base: [0, 1, 2].map(|k| material.base_color[k] as f32),
            base_alpha: material.base_color[3] as f32,
            alpha_mode: material.alpha_mode,
            texture: base_texture,
            vertex_colors: primitive.colors.clone(),
            metallic,
            roughness: material.roughness.clamp(0.0, 1.0),
            // A specular-glossiness material's metalness is per sample.
            is_metal: metallic > 0.0
                || metallic_roughness.is_some()
                || specular_glossiness.is_some(),
            metallic_roughness,
            emissive,
            emissive_texture,
            specular_glossiness,
        }
    }

    /// The Python helper's view of a primitive, through trimesh: textures read
    /// `TEXCOORD_0` only, always repeat and always filter bilinearly; the base
    /// color factor is rounded to bytes; vertex colors count only on a
    /// primitive without a material, as sRGB bytes; alpha comes from the
    /// texture alone.
    fn trimesh(model: &'m Model, primitive: &'m Primitive, lut: &Srgb8) -> Self {
        let material = model.material_of(primitive);
        let uv = primitive.tex_coords(0);
        let texture = |t: Option<&TextureRef>| -> Option<Texture<'m>> {
            uv?;
            Some(Texture {
                image: model.image(t?)?,
                uv_slot: 0,
                transform: None,
                wrap: [Wrap::Repeat; 2],
                nearest: false,
            })
        };
        let base_texture = texture(material.and_then(|m| m.base_color_texture.as_ref()));
        let metallic_roughness =
            texture(material.and_then(|m| m.metallic_roughness_texture.as_ref()));
        let emissive_texture = texture(material.and_then(|m| m.emissive_texture.as_ref()));
        let sg = material.and_then(|m| m.specular_glossiness.as_ref());
        let specular_glossiness = sg.map(|sg| {
            // trimesh converted the material to metallic-roughness while
            // loading, into textures whenever it had one to convert.
            let bytes = has_image(model, &sg.diffuse_texture)
                || has_image(model, &sg.specular_glossiness_texture);
            SpecGloss {
                diffuse: sg.diffuse.map(|c| c as f32),
                diffuse_texture: texture(sg.diffuse_texture.as_ref()),
                specular: sg.specular,
                glossiness: sg.glossiness,
                texture: texture(sg.specular_glossiness_texture.as_ref()),
                bytes,
            }
        });
        let any_texture = base_texture.is_some()
            || metallic_roughness.is_some()
            || emissive_texture.is_some()
            || specular_glossiness
                .as_ref()
                .is_some_and(|s| s.diffuse_texture.is_some() || s.texture.is_some());
        let uvs = match uv {
            Some(uv) if any_texture => vec![uv.iter().map(|&[u, v]| [u, 1.0 - v]).collect()],
            _ => Vec::new(),
        };

        // The factor went through `to_rgba`: rounded to bytes, then read back
        // as 0..1 only if some channel was above 1.
        let base = match material {
            Some(m) => {
                let bytes = [0, 1, 2].map(|k| {
                    (m.base_color[k] * 255.0)
                        .clamp(0.0, 255.0)
                        .round_ties_even()
                });
                let scale = if bytes.iter().any(|&b| b > 1.01) {
                    255.0
                } else {
                    1.0
                };
                bytes.map(|b| (b / scale).clamp(0.0, 1.0) as f32)
            }
            None => [1.0; 3],
        };

        // A primitive without a material is trimesh "vertex colored": its
        // COLOR_0 as bytes, or a flat grey when it has none.
        let vertex_colors = match material {
            Some(_) => None,
            None => {
                let bytes: Vec<[f32; 3]> = match &primitive.colors {
                    Some(colors) => colors
                        .iter()
                        .map(|c| {
                            [0, 1, 2].map(|k| (c[k] * 255.0).clamp(0.0, 255.0).round_ties_even())
                        })
                        .collect(),
                    None => vec![[f32::from(TRIMESH_GREY); 3]; primitive.positions.len()],
                };
                let scaled = bytes.iter().flatten().any(|&b| b > 1.01);
                Some(
                    bytes
                        .iter()
                        .map(|rgb| {
                            let rgb = rgb.map(|b| if scaled { b / 255.0 } else { b });
                            let byte = rgb.map(|c| (c * 255.0).clamp(0.0, 255.0) as u8);
                            let [r, g, b] = lut.rgb([byte[0], byte[1], byte[2], 255]);
                            [r * base[0], g * base[1], b * base[2], 1.0]
                        })
                        .collect(),
                )
            }
        };

        // Metalness and emission defaults were settled from the textures a
        // material has, before the ones without UVs to read were dropped.
        let metallic = metalness(
            material.and_then(|m| m.metallic),
            material.is_some_and(|m| has_image(model, &m.metallic_roughness_texture)),
        );
        let emissive = match material
            .and_then(|m| m.emissive)
            .filter(|e| e.iter().any(|&c| c > 0.0))
        {
            Some(e) => Some(e),
            // A texture without a factor was taken to mean full strength.
            None => material
                .is_some_and(|m| has_image(model, &m.emissive_texture))
                .then_some([1.0; 3]),
        };
        Source {
            triangles: &primitive.triangles,
            uvs,
            trimesh: true,
            base,
            base_alpha: 1.0,
            alpha_mode: AlphaMode::Blend,
            texture: base_texture,
            vertex_colors,
            metallic,
            roughness: material.map_or(1.0, |m| m.roughness.clamp(0.0, 1.0)),
            // A specular-glossiness material's metalness is per sample.
            is_metal: metallic > 0.0
                || metallic_roughness.is_some()
                || specular_glossiness.is_some(),
            metallic_roughness,
            emissive,
            emissive_texture,
            specular_glossiness,
        }
    }

    /// Interpolate a per-vertex attribute at barycentric weights `w`.
    fn lerp<const N: usize>(values: &[[f32; N]], corners: [u32; 3], w: [f32; 3]) -> [f32; N] {
        let [a, b, c] = corners.map(|i| values[i as usize]);
        std::array::from_fn(|k| w[0] * a[k] + w[1] * b[k] + w[2] * c[k])
    }

    /// Shade one surface point: linear RGB, alpha, and how far to trust it.
    fn shade(
        &self,
        lut: &Srgb8,
        light: &Light,
        face: usize,
        w: [f32; 3],
        normal: Option<[f32; 3]>,
    ) -> ([f32; 3], f32, f32) {
        let corners = self.triangles[face];
        let mut uv = [[0.0f32; 2]; UV_SLOTS];
        for (slot, set) in uv.iter_mut().zip(&self.uvs) {
            *slot = Self::lerp(set, corners, w);
        }

        let ndl = if normal.is_some() {
            light.facing(normal)
        } else {
            None
        };
        let (mut rgb, alpha, metallic, roughness) = match &self.specular_glossiness {
            Some(sg) => self.spec_gloss(sg, lut, &uv, corners, w),
            None => {
                let (rgb, alpha) = if self.trimesh {
                    self.trimesh_color(lut, &uv, corners, w)
                } else {
                    self.gltf_color(lut, &uv, corners, w)
                };
                let (mut metallic, mut roughness) = (self.metallic, self.roughness);
                if let Some(t) = &self.metallic_roughness {
                    let texel = self.texel(t, &uv);
                    roughness *= f32::from(texel[1]) / 255.0;
                    metallic *= f32::from(texel[2]) / 255.0;
                }
                (rgb, alpha, metallic, roughness)
            }
        };
        let material_gloss = (1.0 - roughness) * (1.0 - roughness);

        // Recover the albedo first, then relight: de-lighting undoes what an
        // exporter baked in; the metal bake is this sampler's own.
        if normal.is_some() {
            rgb = light.delight(rgb, ndl, material_gloss);
            let per_sample =
                self.metallic_roughness.is_some() || self.specular_glossiness.is_some();
            if self.is_metal && (per_sample || self.metallic > 1e-3) {
                rgb = light.metal(rgb, ndl, metallic, roughness);
            }
        }
        if let Some(e) = self.emissive {
            let glow = match &self.emissive_texture {
                Some(t) => {
                    let c = lut.rgb(self.texel(t, &uv));
                    [e[0] * c[0], e[1] * c[1], e[2] * c[2]]
                }
                None => e,
            };
            rgb = [rgb[0] + glow[0], rgb[1] + glow[1], rgb[2] + glow[2]];
        }
        // Scored before the clamp, so a sample the bake pushed past white
        // still reads as blown rather than as an ordinary white one.
        let keep = if normal.is_some() {
            light.keep(rgb, ndl, material_gloss)
        } else {
            1.0
        };
        (rgb.map(|c| c.clamp(0.0, 1.0)), alpha, keep)
    }

    /// Nearest texel for the metallic-roughness and emissive maps.
    fn texel(&self, t: &Texture, uv: &[[f32; 2]]) -> [u8; 4] {
        let [u, v] = uv[t.uv_slot];
        if self.trimesh {
            // trimesh's V is flipped; flip it back the way the helper did.
            let (w, h) = (t.image.width as i64, t.image.height as i64);
            let x = ((u * w as f32).floor() as i64).rem_euclid(w);
            let y = (((1.0 - v) * h as f32).floor() as i64).rem_euclid(h);
            t.image.texel(x as u32, y as u32)
        } else {
            t.nearest(t.uv([u, v]))
        }
    }

    fn trimesh_color(
        &self,
        lut: &Srgb8,
        uv: &[[f32; 2]],
        corners: [u32; 3],
        w: [f32; 3],
    ) -> ([f32; 3], f32) {
        if let Some(t) = &self.texture {
            let [u, v] = uv[t.uv_slot];
            let (w_px, h_px) = (t.image.width as f32, t.image.height as f32);
            let (rgb, alpha) = t.bilinear(lut, u * w_px, (1.0 - v) * h_px);
            return (
                [
                    rgb[0] * self.base[0],
                    rgb[1] * self.base[1],
                    rgb[2] * self.base[2],
                ],
                alpha,
            );
        }
        if let Some(colors) = &self.vertex_colors {
            let c = Self::lerp(colors, corners, w);
            return ([c[0], c[1], c[2]], 1.0);
        }
        (self.base, 1.0)
    }

    /// Base color × texture × vertex color, alpha by the material's mode.
    fn gltf_color(
        &self,
        lut: &Srgb8,
        uv: &[[f32; 2]],
        corners: [u32; 3],
        w: [f32; 3],
    ) -> ([f32; 3], f32) {
        let mut rgb = self.base;
        let mut alpha = self.base_alpha;
        if let Some(t) = &self.texture {
            let (c, a) = t.sample(lut, t.uv(uv[t.uv_slot]));
            rgb = [rgb[0] * c[0], rgb[1] * c[1], rgb[2] * c[2]];
            alpha *= a;
        }
        if let Some(colors) = &self.vertex_colors {
            let c = Self::lerp(colors, corners, w);
            rgb = [rgb[0] * c[0], rgb[1] * c[1], rgb[2] * c[2]];
            alpha *= c[3];
        }
        let alpha = match self.alpha_mode {
            AlphaMode::Opaque => 1.0,
            AlphaMode::Mask(cutoff) => {
                if alpha >= cutoff {
                    1.0
                } else {
                    0.0
                }
            }
            AlphaMode::Blend => alpha.clamp(0.0, 1.0),
        };
        (rgb, alpha)
    }
}

impl Source<'_> {
    /// A specular-glossiness sample as metallic-roughness: base color, alpha,
    /// metalness and roughness, by the conversion the extension's authors
    /// published (and trimesh uses).
    fn spec_gloss(
        &self,
        sg: &SpecGloss,
        lut: &Srgb8,
        uv: &[[f32; 2]],
        corners: [u32; 3],
        w: [f32; 3],
    ) -> ([f32; 3], f32, f32, f32) {
        let fetch = |t: &Texture| -> ([f32; 3], f32) {
            let [u, v] = uv[t.uv_slot];
            if self.trimesh {
                let (width, height) = (t.image.width as f32, t.image.height as f32);
                t.bilinear(lut, u * width, (1.0 - v) * height)
            } else {
                t.sample(lut, t.uv([u, v]))
            }
        };
        let mut diffuse = sg.diffuse;
        if let Some(t) = &sg.diffuse_texture {
            let (c, a) = fetch(t);
            diffuse = [
                diffuse[0] * c[0],
                diffuse[1] * c[1],
                diffuse[2] * c[2],
                diffuse[3] * a,
            ];
        }
        if let (Some(colors), false) = (&self.vertex_colors, self.trimesh) {
            let c = Self::lerp(colors, corners, w);
            diffuse = [
                diffuse[0] * c[0],
                diffuse[1] * c[1],
                diffuse[2] * c[2],
                diffuse[3] * c[3],
            ];
        }
        let (mut specular, mut glossiness) = (sg.specular, sg.glossiness);
        if let Some(t) = &sg.texture {
            let (c, a) = fetch(t);
            specular = [specular[0] * c[0], specular[1] * c[1], specular[2] * c[2]];
            glossiness *= a;
        }

        let (mut base, mut metallic) =
            metal_rough_from_spec_gloss([diffuse[0], diffuse[1], diffuse[2]], specular);
        let mut alpha = diffuse[3];
        let mut roughness = 1.0 - glossiness;

        if self.trimesh {
            let byte = |v: f32| f32::from((v.clamp(0.0, 1.0) * 255.0) as u8);
            if sg.bytes {
                // Written to 8-bit textures (sRGB, truncated) and read back.
                base = base.map(|c| {
                    let encoded = (linear_to_srgb(f64::from(c)) as f32).clamp(0.0, 1.0);
                    lut.0[(encoded * 255.0) as u8 as usize]
                });
                alpha = byte(alpha) / 255.0;
                roughness = byte(roughness) / 255.0;
                metallic = byte(metallic) / 255.0;
            } else {
                // A factor: rounded to bytes like any other.
                let bytes =
                    base.map(|c| (f64::from(c) * 255.0).clamp(0.0, 255.0).round_ties_even());
                let scale = if bytes.iter().any(|&b| b > 1.01) {
                    255.0
                } else {
                    1.0
                };
                base = bytes.map(|b| (b / scale) as f32);
            }
            return (base, alpha, metallic, roughness);
        }
        let alpha = match self.alpha_mode {
            AlphaMode::Opaque => 1.0,
            AlphaMode::Mask(cutoff) => f32::from(u8::from(alpha >= cutoff)),
            AlphaMode::Blend => alpha.clamp(0.0, 1.0),
        };
        (base, alpha, metallic, roughness)
    }
}

/// Base color and metalness for a specular-glossiness surface, by the
/// conversion the extension's authors published (and trimesh uses): solve for
/// the metalness that reproduces the perceived specular brightness, then
/// blend the base color from diffuse (dielectric) toward specular (metal).
fn metal_rough_from_spec_gloss(diffuse: [f32; 3], specular: [f32; 3]) -> ([f32; 3], f32) {
    const DIELECTRIC: f32 = 0.04;
    const EPSILON: f32 = 1e-6;
    let perceived =
        |c: [f32; 3]| (0.299 * c[0] * c[0] + 0.587 * c[1] * c[1] + 0.114 * c[2] * c[2]).sqrt();
    let one_minus_strength = 1.0 - specular[0].max(specular[1]).max(specular[2]);
    let (pd, ps) = (perceived(diffuse), perceived(specular));
    let metallic = if ps < DIELECTRIC {
        0.0
    } else {
        let b = pd * one_minus_strength / (1.0 - DIELECTRIC) + ps - 2.0 * DIELECTRIC;
        let c = DIELECTRIC - ps;
        let d = (b * b - 4.0 * DIELECTRIC * c).max(EPSILON);
        ((-b + d.sqrt()) / (2.0 * DIELECTRIC)).clamp(0.0, 1.0)
    };
    let mm = metallic * metallic;
    let base = std::array::from_fn(|k| {
        let from_diffuse =
            diffuse[k] * (one_minus_strength / (1.0 - DIELECTRIC) / (1.0 - metallic).max(EPSILON));
        let from_specular =
            (specular[k] - DIELECTRIC * (1.0 - metallic)) * (1.0 / metallic.max(EPSILON));
        (mm * from_specular + (1.0 - mm) * from_diffuse).clamp(0.0, 1.0)
    });
    (base, metallic)
}

/// glTF's default metalness is 1, but a material that gives neither the
/// factor nor a metallic-roughness texture almost never means "mirror" —
/// exporters just left the block out. Treating it as metal would darken
/// every plain model, so it is taken as 0.
fn metalness(factor: Option<f32>, has_texture: bool) -> f32 {
    match factor {
        Some(m) => m.clamp(0.0, 1.0),
        None if has_texture => 1.0,
        None => 0.0,
    }
}

fn has_image(model: &Model, texture: &Option<TextureRef>) -> bool {
    texture.as_ref().and_then(|t| model.image(t)).is_some()
}

/// Angle-weighted vertex normals (Thürrner & Wüthrich), as trimesh computes
/// them for a mesh that brought none.
fn smooth_normals(positions: &[[f64; 3]], triangles: &[[u32; 3]]) -> Vec<[f32; 3]> {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let unit = |v: [f64; 3]| {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if n > 1e-12 {
            v.map(|x| x / n)
        } else {
            [0.0; 3]
        }
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut sums = vec![[0.0f64; 3]; positions.len()];
    for t in triangles {
        let [a, b, c] = t.map(|i| positions[i as usize]);
        let (e0, e1) = (sub(b, a), sub(c, b));
        let cross = [
            e0[1] * e1[2] - e0[2] * e1[1],
            e0[2] * e1[0] - e0[0] * e1[2],
            e0[0] * e1[1] - e0[1] * e1[0],
        ];
        let normal = unit(cross);
        if dot(normal, normal) <= 0.5 {
            continue;
        }
        let (u, v, w) = (unit(sub(b, a)), unit(sub(c, a)), unit(sub(c, b)));
        let a0 = dot(u, v).clamp(-1.0, 1.0).acos();
        let a1 = dot(u.map(|x| -x), w).clamp(-1.0, 1.0).acos();
        let angles = [a0, a1, std::f64::consts::PI - a0 - a1];
        if angles.iter().any(|&x| x < 1e-8) {
            continue;
        }
        for (corner, angle) in t.iter().zip(angles) {
            let s = &mut sums[*corner as usize];
            for k in 0..3 {
                s[k] += angle * normal[k];
            }
        }
    }
    sums.into_iter()
        .map(|s| unit(s).map(|x| x as f32))
        .collect()
}

// ---- The model as the sampler sees it ---------------------------------------

struct Surface<'m> {
    sources: Vec<Source<'m>>,
    /// Triangle corners, single precision, all primitives in scene order.
    corners: Vec<[[f32; 3]; 3]>,
    /// Corner normals, when anything needs them.
    normals: Option<Vec<[[f32; 3]; 3]>>,
    areas: Vec<f64>,
    /// Per triangle: source (primitive) and its index within it.
    owner: Vec<(u32, u32)>,
}

impl<'m> Surface<'m> {
    fn new(model: &'m Model, light: &Light, materials: Materials, lut: &Srgb8) -> Self {
        let sources: Vec<Source> = model
            .primitives
            .iter()
            .map(|p| Source::new(model, p, materials, lut))
            .collect();
        let mut corners = Vec::with_capacity(model.triangle_count());
        let mut owner = Vec::with_capacity(model.triangle_count());
        for (s, p) in model.primitives.iter().enumerate() {
            for (i, t) in p.triangles.iter().enumerate() {
                corners.push(t.map(|v| p.positions[v as usize].map(|x| x as f32)));
                owner.push((s as u32, i as u32));
            }
        }
        let areas = corners
            .par_iter()
            .map(|[a, b, c]| {
                let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let x = [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ];
                0.5 * f64::from((x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt())
            })
            .collect();

        let normals = (light.needs_normals() || sources.iter().any(|s| s.is_metal)).then(|| {
            let per_primitive: Vec<Vec<[f32; 3]>> = model
                .primitives
                .par_iter()
                .map(|p| match (&p.normals, materials) {
                    (Some(n), Materials::Gltf) => n.clone(),
                    _ => smooth_normals(&p.positions, &p.triangles),
                })
                .collect();
            model
                .primitives
                .iter()
                .zip(&per_primitive)
                .flat_map(|(p, n)| p.triangles.iter().map(move |t| t.map(|v| n[v as usize])))
                .collect()
        });

        Surface {
            sources,
            corners,
            normals,
            areas,
            owner,
        }
    }

    fn normal(&self, triangle: usize, w: [f64; 3]) -> Option<[f32; 3]> {
        let n = &self.normals.as_ref()?[triangle];
        Some(std::array::from_fn(|k| {
            (w[0] * f64::from(n[0][k]) + w[1] * f64::from(n[1][k]) + w[2] * f64::from(n[2][k]))
                as f32
        }))
    }

    fn normal32(&self, triangle: usize, w: [f32; 3]) -> Option<[f32; 3]> {
        let n = &self.normals.as_ref()?[triangle];
        Some(std::array::from_fn(|k| {
            w[0] * n[0][k] + w[1] * n[1][k] + w[2] * n[2][k]
        }))
    }

    fn shade(
        &self,
        lut: &Srgb8,
        light: &Light,
        triangle: usize,
        w: [f32; 3],
        normal: Option<[f32; 3]>,
    ) -> ([f32; 3], f32, f32) {
        let (source, face) = self.owner[triangle];
        self.sources[source as usize].shade(lut, light, face as usize, w, normal)
    }
}

// ---- Binning ----------------------------------------------------------------

/// Maps model-space points onto the requested voxels.
struct Binner {
    origin: [f64; 3],
    pitch: f64,
    hi: [i64; 3],
    lookup: Lookup,
}

enum Lookup {
    /// Row per cell of the bounding grid, `u32::MAX` for none.
    Dense(Vec<u32>),
    /// Sorted cell keys and their rows.
    Sorted(Vec<(i64, u32)>),
}

impl Binner {
    fn new(coords: &[[i32; 3]], pitch: f64, origin: [f32; 3]) -> Self {
        let mut hi = [0i64; 3];
        for c in coords {
            for k in 0..3 {
                hi[k] = hi[k].max(i64::from(c[k]));
            }
        }
        let binner = |lookup| Binner {
            origin: origin.map(f64::from),
            pitch,
            hi,
            lookup,
        };
        let cells = (hi[0] + 1) * (hi[1] + 1) * (hi[2] + 1);
        let mut dummy = binner(Lookup::Sorted(Vec::new()));
        if cells <= 1 << 24 {
            let mut dense = vec![u32::MAX; cells as usize];
            for (row, c) in coords.iter().enumerate() {
                dense[dummy.key(c.map(i64::from)) as usize] = row as u32;
            }
            dummy.lookup = Lookup::Dense(dense);
        } else {
            let mut sorted: Vec<(i64, u32)> = coords
                .iter()
                .enumerate()
                .map(|(row, c)| (dummy.key(c.map(i64::from)), row as u32))
                .collect();
            sorted.sort_unstable();
            dummy.lookup = Lookup::Sorted(sorted);
        }
        dummy
    }

    fn key(&self, g: [i64; 3]) -> i64 {
        let stride_y = self.hi[2] + 1;
        let stride_x = (self.hi[1] + 1) * stride_y;
        g[0] * stride_x + g[1] * stride_y + g[2]
    }

    /// The voxel row a point falls in, if it is one of the requested voxels.
    fn bin(&self, p: [f64; 3]) -> Option<u32> {
        let g: [i64; 3] =
            std::array::from_fn(|k| ((p[k] - self.origin[k]) / self.pitch).floor() as i64);
        if (0..3).any(|k| g[k] < 0 || g[k] > self.hi[k]) {
            return None;
        }
        let key = self.key(g);
        match &self.lookup {
            Lookup::Dense(rows) => Some(rows[key as usize]).filter(|&r| r != u32::MAX),
            Lookup::Sorted(sorted) => sorted
                .binary_search_by_key(&key, |&(k, _)| k)
                .ok()
                .map(|i| sorted[i].1),
        }
    }
}

// ---- Scatter ----------------------------------------------------------------

/// One shaded sample that landed in a voxel.
#[derive(Clone, Copy)]
struct Hit {
    row: u32,
    source: u32,
    rgb: [f32; 3],
    alpha: f32,
    keep: f32,
}

struct Scatter {
    sums: Vec<[f64; 3]>,
    weights: Vec<f64>,
    raw_weights: Vec<f64>,
    /// Per voxel and source, how much of the voxel each material covers.
    source_weights: Option<Vec<f64>>,
    reference_sums: Vec<[f64; 3]>,
    reference_weights: Vec<f64>,
    samples: i64,
}

/// Per-pass totals that start at zero and are added in once the pass is
/// done — the Python helper's `np.bincount` per pass, whose rounding this
/// keeps.
struct PassTotals {
    sums: Vec<[f64; 3]>,
    weights: Vec<f64>,
    raw: Vec<f64>,
    source_weights: Vec<f64>,
    touched: Vec<u32>,
    mark: Vec<bool>,
}

#[allow(clippy::too_many_arguments)]
fn supersample(
    surface: &Surface,
    binner: &Binner,
    n_voxels: usize,
    voxel_area: f64,
    density: u32,
    max_samples: i64,
    lut: &Srgb8,
    light: &Light,
    progress: &mut dyn Progress,
) -> Result<Scatter> {
    let n_sources = surface.sources.len();
    let track = n_sources > 0 && n_sources <= MAX_TRACKED_SOURCES;

    // Samples per triangle: by how many voxel faces it spans, at least one.
    let mut per_face: Vec<i64> = surface
        .areas
        .iter()
        .map(|&a| ((a / voxel_area * f64::from(density)).ceil() as i64).max(1))
        .collect();
    let mut total: i64 = per_face.iter().sum();
    if total > max_samples {
        let scale = max_samples as f64 / total as f64;
        for n in &mut per_face {
            *n = ((*n as f64 * scale) as i64).max(1);
        }
        total = per_face.iter().sum();
    }
    let mut starts = Vec::with_capacity(per_face.len() + 1);
    starts.push(0i64);
    for n in &per_face {
        starts.push(starts.last().unwrap() + n);
    }

    let mut out = Scatter {
        sums: vec![[0.0; 3]; n_voxels],
        weights: vec![0.0; n_voxels],
        raw_weights: vec![0.0; n_voxels],
        source_weights: track.then(|| vec![0.0; n_voxels * n_sources]),
        reference_sums: vec![[0.0; 3]; n_sources.max(1)],
        reference_weights: vec![0.0; n_sources.max(1)],
        samples: total,
    };
    let mut pass = PassTotals {
        sums: vec![[0.0; 3]; n_voxels],
        weights: vec![0.0; n_voxels],
        raw: vec![0.0; n_voxels],
        source_weights: if track {
            vec![0.0; n_voxels * n_sources]
        } else {
            Vec::new()
        },
        touched: Vec::new(),
        mark: vec![false; n_voxels],
    };

    let rng = NumpyRng::new(SEED);
    let n_faces = per_face.len();
    let mut lo = 0usize;
    while lo < n_faces {
        if progress.is_cancelled() {
            return Err(Error::Cancelled);
        }
        // The Python helper's pass boundaries: as many whole triangles as fit
        // in CHUNK samples, and at least one.
        let limit = starts[lo] + CHUNK;
        let hi = (starts.partition_point(|&s| s <= limit) - 1)
            .max(lo + 1)
            .min(n_faces);
        let m = starts[hi] - starts[lo];
        // Each pass draws all its first coordinates, then all its second ones.
        let offset = 2 * starts[lo];

        // Split the pass into tasks at triangle boundaries.
        let mut tasks = vec![lo];
        while *tasks.last().unwrap() < hi {
            let from = *tasks.last().unwrap();
            let next = starts.partition_point(|&s| s <= starts[from] + TASK) - 1;
            tasks.push(next.max(from + 1).min(hi));
        }
        let hits: Vec<Vec<Hit>> = tasks
            .par_windows(2)
            .map(|w| {
                let (f0, f1) = (w[0], w[1]);
                let local = starts[f0] - starts[lo];
                let mut r1 = rng.skipped((offset + local) as u128);
                let mut r2 = rng.skipped((offset + m + local) as u128);
                let mut hits = Vec::new();
                for (face, &count) in per_face.iter().enumerate().take(f1).skip(f0) {
                    let tri = surface.corners[face];
                    for _ in 0..count {
                        let (a, b) = (r1.random(), r2.random());
                        let su = a.sqrt();
                        let bw = [1.0 - su, su * (1.0 - b), su * b];
                        let point: [f64; 3] = std::array::from_fn(|k| {
                            bw[0] * f64::from(tri[0][k])
                                + bw[1] * f64::from(tri[1][k])
                                + bw[2] * f64::from(tri[2][k])
                        });
                        let Some(row) = binner.bin(point) else {
                            continue;
                        };
                        let normal = surface.normal(face, bw);
                        let w32 = bw.map(|x| x as f32);
                        let (rgb, alpha, keep) = surface.shade(lut, light, face, w32, normal);
                        hits.push(Hit {
                            row,
                            source: surface.owner[face].0,
                            rgb,
                            alpha,
                            keep,
                        });
                    }
                }
                hits
            })
            .collect();

        accumulate(&mut out, &mut pass, hits.iter().flatten(), n_sources);
        lo = hi;
        progress.update(
            Stage::Voxelize,
            0.30 + 0.35 * (starts[lo] as f32 / total.max(1) as f32),
            &format!("Sampled {} of {} surface points", starts[lo], total),
        );
    }
    Ok(out)
}

/// Add one pass's samples in, in sample order.
fn accumulate<'a>(
    out: &mut Scatter,
    pass: &mut PassTotals,
    hits: impl Iterator<Item = &'a Hit>,
    n_sources: usize,
) {
    let mut reference_sums = vec![[0.0f64; 3]; out.reference_sums.len()];
    let mut reference_weights = vec![0.0f64; out.reference_weights.len()];
    for hit in hits {
        let row = hit.row as usize;
        if !pass.mark[row] {
            pass.mark[row] = true;
            pass.touched.push(hit.row);
        }
        let raw = f64::from(hit.alpha);
        let w = raw * f64::from(hit.keep);
        for k in 0..3 {
            pass.sums[row][k] += f64::from(hit.rgb[k]) * w;
        }
        pass.weights[row] += w;
        pass.raw[row] += raw;
        if hit.keep >= REF_KEEP_MIN {
            let s = hit.source as usize;
            reference_weights[s] += w;
            for (sum, &c) in reference_sums[s].iter_mut().zip(&hit.rgb) {
                *sum += f64::from(c) * w;
            }
        }
        if !pass.source_weights.is_empty() {
            pass.source_weights[row * n_sources + hit.source as usize] += raw;
        }
    }
    for (total, part) in out.reference_weights.iter_mut().zip(&reference_weights) {
        *total += part;
    }
    for (total, part) in out.reference_sums.iter_mut().zip(&reference_sums) {
        for k in 0..3 {
            total[k] += part[k];
        }
    }
    for &row in &pass.touched {
        let row = row as usize;
        for k in 0..3 {
            out.sums[row][k] += pass.sums[row][k];
        }
        out.weights[row] += pass.weights[row];
        out.raw_weights[row] += pass.raw[row];
        pass.sums[row] = [0.0; 3];
        pass.weights[row] = 0.0;
        pass.raw[row] = 0.0;
        pass.mark[row] = false;
        if let Some(sw) = &mut out.source_weights {
            for s in 0..n_sources {
                sw[row * n_sources + s] += pass.source_weights[row * n_sources + s];
                pass.source_weights[row * n_sources + s] = 0.0;
            }
        }
    }
    pass.touched.clear();
}

/// Per-voxel albedo of the material that owns it, or `None` if no material
/// was ever seen away from its highlights.
fn reference_albedo(scatter: &Scatter, n_voxels: usize) -> Option<Vec<[f32; 3]>> {
    let total_weight = pairwise_sum(&scatter.reference_weights);
    if total_weight <= 1e-9 {
        return None;
    }
    let mut global = [0.0f64; 3];
    for s in &scatter.reference_sums {
        for k in 0..3 {
            global[k] += s[k];
        }
    }
    let global = global.map(|v| (v / total_weight) as f32);
    let Some(owner) = &scatter.source_weights else {
        return Some(vec![global; n_voxels]);
    };
    let n_sources = scatter.reference_weights.len();
    let per_source: Vec<[f32; 3]> = scatter
        .reference_sums
        .iter()
        .zip(&scatter.reference_weights)
        .map(|(s, &w)| {
            if w > 1e-9 {
                s.map(|v| (v / w) as f32)
            } else {
                global
            }
        })
        .collect();
    Some(
        owner
            .chunks_exact(n_sources)
            .map(|row| {
                // First largest, like np.argmax.
                let mut best = 0;
                for (s, &v) in row.iter().enumerate() {
                    if v > row[best] {
                        best = s;
                    }
                }
                per_source[best]
            })
            .collect(),
    )
}

/// Rebuild voxels whose samples were almost all highlight from their
/// material's albedo, seen elsewhere away from the lobe.
fn recover_highlights(
    linear: &mut [[f32; 3]],
    scatter: &Scatter,
    light: &Light,
    progress: &mut dyn Progress,
) {
    if light.recovery <= 0.0 || light.rejection <= 0.0 {
        return;
    }
    let t: Vec<f32> = scatter
        .raw_weights
        .iter()
        .zip(&scatter.weights)
        .map(|(&raw, &kept)| {
            // Weight falls no further than the rejection ceiling, so this is
            // "how much of the voxel was highlight", 0..1.
            let blown = if raw > 1e-6 {
                (1.0 - kept / raw) / light.rejection.max(1e-6)
            } else {
                0.0
            };
            (smoothstep64(blown, 0.7, 0.98) * light.recovery) as f32
        })
        .collect();
    if !t.iter().any(|&t| t > 1e-4) {
        return;
    }
    let Some(reference) = reference_albedo(scatter, linear.len()) else {
        return;
    };
    let rebuilt = t.iter().filter(|&&t| t > 0.5).count();
    if rebuilt > 0 {
        progress.update(
            Stage::Voxelize,
            0.66,
            &format!("Rebuilding {rebuilt} blown-out voxels from material albedo"),
        );
    }
    for ((c, &t), r) in linear.iter_mut().zip(&t).zip(&reference) {
        *c = std::array::from_fn(|k| c[k] * (1.0 - t) + r[k] * t);
    }
}

// ---- Nearest surface ---------------------------------------------------------

/// Closest point of a triangle to `p`, the Python helper's way: barycentric
/// coordinates of the projection, each clamped to [0, 1] and renormalized.
fn barycentric(p: [f32; 3], [a, b, c]: [[f32; 3]; 3]) -> [f32; 3] {
    let sub = |x: [f32; 3], y: [f32; 3]| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
    let dot = |x: [f32; 3], y: [f32; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d00, d01, d11, d20, d21) = (
        dot(ab, ab),
        dot(ab, ac),
        dot(ac, ac),
        dot(ap, ab),
        dot(ap, ac),
    );
    let mut denom = d00 * d11 - d01 * d01;
    if denom.abs() < 1e-12 {
        denom = 1.0;
    }
    let v = (d11 * d20 - d01 * d21) / denom;
    let w = (d00 * d21 - d01 * d20) / denom;
    let u = 1.0 - v - w;
    let (u, v, w) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0), w.clamp(0.0, 1.0));
    let mut s = u + v + w;
    if s == 0.0 {
        s = 1.0;
    }
    [u / s, v / s, w / s]
}

/// Colors for voxels the scatter missed: the surfaces crossing each voxel,
/// weighted by alpha and by how much surface a triangle can put in one voxel;
/// failing that, the single nearest surface point.
#[allow(clippy::too_many_arguments)]
fn nearest_surface(
    surface: &Surface,
    coords: &[[i32; 3]],
    missing: &[usize],
    pitch: f64,
    origin: [f32; 3],
    lut: &Srgb8,
    light: &Light,
    progress: &mut dyn Progress,
) -> Result<Vec<[f32; 3]>> {
    let centers: Vec<[f64; 3]> = surface
        .corners
        .iter()
        .map(|[a, b, c]| std::array::from_fn(|k| f64::from((a[k] + b[k] + c[k]) / 3.0)))
        .collect();
    let tree: ImmutableKdTree<f64, 3> = ImmutableKdTree::new_from_slice(&centers);
    let k = NonZeroUsize::new(32.min(surface.corners.len())).expect("the model has triangles");
    let half = (pitch * 0.5) as f32;
    let voxel_area = pitch * pitch;

    let mut out = Vec::with_capacity(missing.len());
    for batch in missing.chunks(8192) {
        if progress.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let colors: Vec<[f32; 3]> = batch
            .par_iter()
            .map(|&row| {
                let c = coords[row];
                let center: [f32; 3] = std::array::from_fn(|i| {
                    c[i] as f32 * pitch as f32 + origin[i] + (pitch * 0.5) as f32
                });
                let query = center.map(f64::from);
                let found = tree.nearest_n::<kiddo::SquaredEuclidean>(&query, k);

                let mut weights = Vec::with_capacity(found.len());
                let mut shaded = Vec::with_capacity(found.len());
                let mut distances = Vec::with_capacity(found.len());
                for n in &found {
                    let t = n.item as usize;
                    let tri = surface.corners[t];
                    let bw = barycentric(center, tri);
                    let closest: [f32; 3] = std::array::from_fn(|i| {
                        tri[0][i] * bw[0] + tri[1][i] * bw[1] + tri[2][i] * bw[2]
                    });
                    let bw = barycentric(closest, tri);
                    let (rgb, alpha, keep) =
                        surface.shade(lut, light, t, bw, surface.normal32(t, bw));
                    let inside = (0..3).all(|i| (closest[i] - center[i]).abs() <= half);
                    let weight = f64::from(alpha * keep) * surface.areas[t].min(voxel_area);
                    weights.push(if inside { weight } else { 0.0 });
                    shaded.push(rgb.map(f64::from));
                    distances.push(
                        (0..3)
                            .map(|i| (closest[i] - center[i]).powi(2))
                            .sum::<f32>(),
                    );
                }
                let total = pairwise_sum(&weights);
                if total > 1e-12 {
                    let mut blended = [0.0f64; 3];
                    for (w, rgb) in weights.iter().zip(&shaded) {
                        for i in 0..3 {
                            blended[i] += w * rgb[i];
                        }
                    }
                    blended.map(|v| (v / total) as f32)
                } else {
                    let mut best = 0;
                    for (i, &d) in distances.iter().enumerate() {
                        if d < distances[best] {
                            best = i;
                        }
                    }
                    shaded[best].map(|v| v as f32)
                }
            })
            .collect();
        out.extend(colors);
    }
    Ok(out)
}

// ---- Entry point -------------------------------------------------------------

/// sRGB colors (0–255 per channel) for the voxels at `coords`, which sit on a
/// grid of `pitch` model units whose cell (0, 0, 0) starts at `origin`.
pub fn sample_colors(
    model: &Model,
    coords: &[[i32; 3]],
    pitch: f64,
    origin: [f32; 3],
    lighting: &LightingOptions,
    options: &SampleOptions,
    progress: &mut dyn Progress,
) -> Result<Vec<[f32; 3]>> {
    let n = coords.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    if model.triangle_count() == 0 {
        return Err(Error::NoGeometry);
    }
    let started = Instant::now();
    let light = Light::new(lighting);
    let lut = Srgb8::new();
    let surface = Surface::new(model, &light, options.materials, &lut);
    progress.update(
        Stage::Voxelize,
        0.30,
        &format!(
            "Sampling {n} voxels | triangles={} | {} samples/voxel",
            surface.corners.len(),
            options.samples_per_voxel
        ),
    );

    let binner = Binner::new(coords, pitch, origin);
    let max_samples = ((widen(options.ram_limit) * 16_000_000.0) as i64).max(4_000_000);
    let scatter = supersample(
        &surface,
        &binner,
        n,
        pitch * pitch,
        options.samples_per_voxel.max(1),
        max_samples,
        &lut,
        &light,
        progress,
    )?;
    progress.update(
        Stage::Voxelize,
        0.65,
        &format!(
            "Scattered {} surface samples in {:.1}s",
            scatter.samples,
            started.elapsed().as_secs_f32()
        ),
    );

    let mut linear: Vec<[f32; 3]> = scatter
        .sums
        .iter()
        .zip(&scatter.weights)
        .map(|(s, &w)| {
            if w > 1e-6 {
                s.map(|v| (v / w) as f32)
            } else {
                [0.0; 3]
            }
        })
        .collect();
    recover_highlights(&mut linear, &scatter, &light, progress);

    // Voxels the scatter missed: thin features, or only transparent texels.
    let missing: Vec<usize> = (0..n).filter(|&i| scatter.weights[i] <= 1e-6).collect();
    if !missing.is_empty() {
        progress.update(
            Stage::Voxelize,
            0.70,
            &format!(
                "Falling back to nearest-surface for {} voxels",
                missing.len()
            ),
        );
        let colors = nearest_surface(
            &surface, coords, &missing, pitch, origin, &lut, &light, progress,
        )?;
        for (&row, color) in missing.iter().zip(colors) {
            linear[row] = color;
        }
    }
    progress.update(
        Stage::Voxelize,
        0.80,
        &format!("Done sampling in {:.1}s", started.elapsed().as_secs_f32()),
    );
    Ok(linear
        .iter()
        .map(|c| c.map(|v| (linear_to_srgb(f64::from(v)) * 255.0) as f32))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Material, Primitive};
    use crate::pipeline::NoProgress;

    fn quad(material: Option<usize>, colors: Option<Vec<[f32; 4]>>) -> Primitive {
        Primitive {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tex_coords: vec![vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]],
            colors,
            material,
            ..Primitive::default()
        }
    }

    fn flat_light() -> LightingOptions {
        LightingOptions {
            light_dir: [0.35, 0.85, 0.40],
            ambient: 0.32,
            gloss: 0.5,
            specular: 1.1,
            rejection: 0.0,
            recovery: 0.0,
            delight: 0.0,
        }
    }

    fn colors_of(model: &Model, materials: Materials) -> Vec<[f32; 3]> {
        let coords: Vec<[i32; 3]> = (0..4)
            .flat_map(|x| (0..4).map(move |y| [x, y, 0]))
            .collect();
        let options = SampleOptions {
            materials,
            ..SampleOptions::default()
        };
        sample_colors(
            model,
            &coords,
            0.25,
            [0.0, 0.0, -0.125],
            &flat_light(),
            &options,
            &mut NoProgress,
        )
        .unwrap()
    }

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 0.6)
    }

    #[test]
    fn a_factor_is_linear() {
        let model = Model {
            primitives: vec![quad(Some(0), None)],
            materials: vec![Material {
                base_color: [0.9, 0.1, 0.1, 1.0],
                ..Material::default()
            }],
            ..Model::default()
        };
        for c in colors_of(&model, Materials::Gltf) {
            assert!(near(c, [243.4, 89.0, 89.0]), "{c:?}");
        }
    }

    #[test]
    fn vertex_colors_are_linear_under_gltf_and_srgb_under_trimesh() {
        let colors = Some(vec![[0.2, 0.5, 0.8, 1.0]; 4]);
        let model = Model {
            primitives: vec![quad(None, colors)],
            ..Model::default()
        };
        for c in colors_of(&model, Materials::Gltf) {
            assert!(near(c, [123.6, 187.5, 231.1]), "{c:?}");
        }
        for c in colors_of(&model, Materials::Trimesh) {
            assert!(near(c, [51.0, 128.0, 204.0]), "{c:?}");
        }
    }

    #[test]
    fn no_material_is_white_under_gltf_and_grey_under_trimesh() {
        let model = Model {
            primitives: vec![quad(None, None)],
            ..Model::default()
        };
        assert!(colors_of(&model, Materials::Gltf)
            .iter()
            .all(|&c| near(c, [255.0; 3])));
        assert!(colors_of(&model, Materials::Trimesh)
            .iter()
            .all(|&c| near(c, [102.0; 3])));
    }

    #[test]
    fn textures_are_sampled_right_way_up() {
        // Top half of the image red, bottom half blue; glTF V runs down the
        // image, and this quad's V is 1 at y = 0.
        let mut pixels = vec![[255, 0, 0, 255]; 8 * 4];
        pixels.extend(vec![[0, 0, 255, 255]; 8 * 4]);
        let model = Model {
            primitives: vec![quad(Some(0), None)],
            materials: vec![Material {
                base_color_texture: Some(TextureRef {
                    image: 0,
                    tex_coord: 0,
                    transform: None,
                    wrap: [Wrap::Repeat; 2],
                    nearest: true,
                }),
                ..Material::default()
            }],
            images: vec![Some(Image {
                width: 8,
                height: 8,
                pixels,
            })],
        };
        // Voxels (0, 0) and (0, 3) of the 4×4 quad: y = 0 is the image's
        // bottom. (Trimesh filters bilinearly and always repeats, so a little
        // of the far edge bleeds in.)
        for materials in [Materials::Gltf, Materials::Trimesh] {
            let colors = colors_of(&model, materials);
            let (bottom, top) = (colors[0], colors[3]);
            assert!(bottom[2] > bottom[0] + 150.0, "{materials:?} {bottom:?}");
            assert!(top[0] > top[2] + 150.0, "{materials:?} {top:?}");
        }
    }

    #[test]
    fn metal_is_shaded_and_plain_is_not() {
        let mut light = flat_light();
        light.rejection = 0.75;
        let model = |metallic: Option<f32>| Model {
            primitives: vec![quad(Some(0), None)],
            materials: vec![Material {
                base_color: [0.5, 0.5, 0.5, 1.0],
                metallic,
                ..Material::default()
            }],
            ..Model::default()
        };
        let coords = vec![[1, 1, 0]];
        let sample = |m: &Model| {
            sample_colors(
                m,
                &coords,
                0.25,
                [0.0, 0.0, -0.125],
                &light,
                &SampleOptions::default(),
                &mut NoProgress,
            )
            .unwrap()[0]
        };
        let plain = sample(&model(None));
        let metal = sample(&model(Some(1.0)));
        assert!(near(plain, [188.0; 3]), "{plain:?}");
        assert!(
            metal[0] < plain[0] - 10.0,
            "metal {metal:?} vs plain {plain:?}"
        );
    }

    /// An 8×8 texture: left half transparent red, right half opaque green.
    fn half_transparent() -> Image {
        let pixels = (0..64)
            .map(|i| {
                if i % 8 < 4 {
                    [255, 0, 0, 0]
                } else {
                    [0, 255, 0, 255]
                }
            })
            .collect();
        Image {
            width: 8,
            height: 8,
            pixels,
        }
    }

    fn textured(material: Material, image: Image, primitive: Primitive) -> Model {
        Model {
            primitives: vec![primitive],
            materials: vec![material],
            images: vec![Some(image)],
        }
    }

    fn texture_ref() -> TextureRef {
        TextureRef {
            image: 0,
            tex_coord: 0,
            transform: None,
            wrap: [Wrap::Repeat; 2],
            nearest: true,
        }
    }

    /// The whole quad as one voxel, sampled densely enough to average well.
    fn one_voxel(model: &Model) -> [f32; 3] {
        let options = SampleOptions {
            samples_per_voxel: 20_000,
            ..SampleOptions::default()
        };
        sample_colors(
            model,
            &[[0, 0, 0]],
            1.0,
            [0.0, 0.0, -0.5],
            &flat_light(),
            &options,
            &mut NoProgress,
        )
        .unwrap()[0]
    }

    #[test]
    fn alpha_modes() {
        let with = |alpha_mode| {
            let material = Material {
                base_color_texture: Some(texture_ref()),
                alpha_mode,
                ..Material::default()
            };
            one_voxel(&textured(material, half_transparent(), quad(Some(0), None)))
        };
        // Opaque ignores alpha: half red, half green.
        let opaque = with(AlphaMode::Opaque);
        assert!(
            (opaque[0] - 187.5).abs() < 3.0 && (opaque[1] - 187.5).abs() < 3.0,
            "{opaque:?}"
        );
        // Blend and mask drop the transparent half.
        assert!(
            near(with(AlphaMode::Blend), [0.0, 255.0, 0.0]),
            "{:?}",
            with(AlphaMode::Blend)
        );
        assert!(
            near(with(AlphaMode::Mask(0.5)), [0.0, 255.0, 0.0]),
            "{:?}",
            with(AlphaMode::Mask(0.5))
        );
    }

    #[test]
    fn texture_transform_and_uv_sets() {
        // Scaling U by a half maps the whole quad onto the red half.
        let scaled = Material {
            base_color_texture: Some(TextureRef {
                transform: Some([[0.5, 0.0, 0.0], [0.0, 1.0, 0.0]]),
                ..texture_ref()
            }),
            ..Material::default()
        };
        let color = one_voxel(&textured(scaled, half_transparent(), quad(Some(0), None)));
        assert!(near(color, [255.0, 0.0, 0.0]), "{color:?}");

        // TEXCOORD_1 pinned inside the green half.
        let mut primitive = quad(Some(0), None);
        primitive.tex_coords.push(vec![[0.75, 0.5]; 4]);
        let second_set = Material {
            base_color_texture: Some(TextureRef {
                tex_coord: 1,
                ..texture_ref()
            }),
            ..Material::default()
        };
        let color = one_voxel(&textured(second_set, half_transparent(), primitive));
        assert!(near(color, [0.0, 255.0, 0.0]), "{color:?}");
    }

    #[test]
    fn emissive_strength_scales_emission() {
        let glowing = |strength| Material {
            base_color: [0.0, 0.0, 0.0, 1.0],
            emissive: Some([0.1, 0.0, 0.0]),
            emissive_strength: strength,
            ..Material::default()
        };
        let model = |m| Model {
            primitives: vec![quad(Some(0), None)],
            materials: vec![m],
            ..Model::default()
        };
        let dim = one_voxel(&model(glowing(1.0)));
        let bright = one_voxel(&model(glowing(5.0)));
        assert!(near(dim, [89.0, 0.0, 0.0]), "{dim:?}");
        assert!(near(bright, [188.0, 0.0, 0.0]), "{bright:?}");
    }

    #[test]
    fn specular_glossiness_conversion() {
        // A dielectric keeps its diffuse color and has no metal.
        let (base, metallic) = metal_rough_from_spec_gloss([0.5, 0.2, 0.2], [0.04; 3]);
        assert_eq!(metallic, 0.0);
        assert!(
            base.iter()
                .zip([0.5, 0.2, 0.2])
                .all(|(a, b)| (a - b).abs() < 1e-4),
            "{base:?}"
        );
        // Gold: no diffuse, colored specular — all metal, base = specular.
        let gold = [1.0, 0.766, 0.336];
        let (base, metallic) = metal_rough_from_spec_gloss([0.0; 3], gold);
        assert!((metallic - 1.0).abs() < 1e-3, "{metallic}");
        assert!(
            base.iter().zip(gold).all(|(a, b)| (a - b).abs() < 1e-3),
            "{base:?}"
        );
    }

    #[test]
    fn wrap_modes() {
        assert_eq!(wrap_index(-1, 4, Wrap::Repeat), 3);
        assert_eq!(wrap_index(-1, 4, Wrap::Clamp), 0);
        assert_eq!(wrap_index(4, 4, Wrap::Mirror), 3);
        assert_eq!(wrap_index(-1, 4, Wrap::Mirror), 0);
        assert_eq!(wrap_index(9, 4, Wrap::Mirror), 1);
    }
}
