//! glTF 2.0 models, loaded into what the voxelizer and the color sampler need.
//!
//! [`load`] reads a `.glb` or `.gltf` — buffers in the GLB, in data URIs or in
//! files beside the model; PNG, JPEG and WebP textures — walks the scene's
//! node hierarchy and bakes every node's transform into its primitives'
//! vertices.
//!
//! Some details follow the Python voxelizer (trimesh) rather than taste,
//! because they decide which random sample lands where, and so which voxels a
//! conversion produces:
//!
//! * primitives come out in trimesh's scene order: its node walk is a stack,
//!   so the *last* root and the last child are visited first;
//! * node transforms are read from the JSON as doubles and applied only when
//!   they differ from the identity, and a mirroring transform reverses each
//!   triangle's winding;
//! * vertex positions are doubles from the moment they are read.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine as _;
use gltf::mesh::Mode;
use gltf::texture::{MagFilter, WrappingMode};
use rayon::prelude::*;
use serde_json::Value;

use crate::error::{Error, Result};

/// A 4×4 matrix, row-major: `m[row][column]`.
pub type Mat4 = [[f64; 4]; 4];

const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// glTF extensions a model may *require* and still load. Anything else in
/// `extensionsRequired` changes what the data means, so such a model is
/// refused rather than misread.
const READABLE_EXTENSIONS: &[&str] = &[
    "KHR_texture_transform",
    "KHR_materials_pbrSpecularGlossiness",
    "KHR_materials_emissive_strength",
    "KHR_materials_unlit",
    "KHR_materials_ior",
    "KHR_materials_specular",
    "KHR_materials_transmission",
    "KHR_materials_volume",
    "KHR_materials_variants",
    "KHR_lights_punctual",
    // Texture formats: WebP decodes; a KTX2 or DDS texture that cannot is
    // skipped with a warning and its material sampled without it.
    "EXT_texture_webp",
    "KHR_texture_basisu",
    "MSFT_texture_dds",
];

static EXTERNAL_FILES: AtomicBool = AtomicBool::new(true);

/// Whether a `.gltf` may refer to files beside it. On by default; the server
/// turns it off, because an upload arrives alone and a reference could only
/// reach other files in its upload folder.
pub fn allow_external_files(allow: bool) {
    EXTERNAL_FILES.store(allow, Ordering::Relaxed);
}

/// A model: every triangle primitive in the scene, placed in world space.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// In trimesh's scene order (see the module docs).
    pub primitives: Vec<Primitive>,
    pub materials: Vec<Material>,
    /// Decoded textures by glTF image index; `None` for images no material
    /// samples, or that could not be decoded.
    pub images: Vec<Option<Image>>,
}

impl Model {
    pub fn triangle_count(&self) -> usize {
        self.primitives.iter().map(|p| p.triangles.len()).sum()
    }

    /// The material a primitive is drawn with (`None`: the glTF default).
    pub fn material_of(&self, primitive: &Primitive) -> Option<&Material> {
        primitive.material.and_then(|i| self.materials.get(i))
    }

    pub fn image(&self, texture: &TextureRef) -> Option<&Image> {
        self.images.get(texture.image).and_then(Option::as_ref)
    }
}

/// One triangle primitive, with its node transform applied.
#[derive(Debug, Clone, Default)]
pub struct Primitive {
    /// World-space positions.
    pub positions: Vec<[f64; 3]>,
    /// Vertex indices, three per triangle.
    pub triangles: Vec<[u32; 3]>,
    /// The file's normals, in world space and unit length.
    pub normals: Option<Vec<[f32; 3]>>,
    /// `TEXCOORD_n`, by `n`, as the file stores them (V pointing down).
    pub tex_coords: Vec<Vec<[f32; 2]>>,
    /// `COLOR_0` as linear RGBA.
    pub colors: Option<Vec<[f32; 4]>>,
    /// Index into [`Model::materials`]; `None` means the glTF default material.
    pub material: Option<usize>,
}

impl Primitive {
    /// A texture coordinate set, if the primitive has a usable one.
    pub fn tex_coords(&self, set: u32) -> Option<&[[f32; 2]]> {
        self.tex_coords
            .get(set as usize)
            .filter(|uv| !uv.is_empty())
            .map(Vec::as_slice)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlphaMode {
    Opaque,
    /// Texels below the cutoff are not drawn at all.
    Mask(f32),
    Blend,
}

/// A metallic-roughness material, as the sampler reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: Option<String>,
    /// `baseColorFactor`, linear RGBA, at the precision the file wrote it.
    pub base_color: [f64; 4],
    pub base_color_texture: Option<TextureRef>,
    /// `metallicFactor` — `None` when the file left it out, which the sampler
    /// does not take to mean the spec's default of 1.
    pub metallic: Option<f32>,
    pub roughness: f32,
    pub metallic_roughness_texture: Option<TextureRef>,
    /// `emissiveFactor` — `None` when the file left it out.
    pub emissive: Option<[f32; 3]>,
    /// `KHR_materials_emissive_strength`.
    pub emissive_strength: f32,
    pub emissive_texture: Option<TextureRef>,
    pub alpha_mode: AlphaMode,
    /// `KHR_materials_pbrSpecularGlossiness`, which replaces the
    /// metallic-roughness inputs above when present.
    pub specular_glossiness: Option<SpecularGlossiness>,
}

/// A specular-glossiness material (retired, but still in the wild).
#[derive(Debug, Clone, PartialEq)]
pub struct SpecularGlossiness {
    /// `diffuseFactor`, linear RGBA.
    pub diffuse: [f64; 4],
    pub diffuse_texture: Option<TextureRef>,
    /// `specularFactor`, linear RGB.
    pub specular: [f32; 3],
    pub glossiness: f32,
    /// RGB specular (sRGB), A glossiness (linear).
    pub specular_glossiness_texture: Option<TextureRef>,
}

impl Default for Material {
    /// glTF's default material: white, non-emissive, opaque.
    fn default() -> Self {
        Material {
            name: None,
            base_color: [1.0; 4],
            base_color_texture: None,
            metallic: None,
            roughness: 1.0,
            metallic_roughness_texture: None,
            emissive: None,
            emissive_strength: 1.0,
            emissive_texture: None,
            alpha_mode: AlphaMode::Opaque,
            specular_glossiness: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    Repeat,
    Clamp,
    Mirror,
}

/// A material's use of a texture.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureRef {
    /// Index into [`Model::images`].
    pub image: usize,
    /// Which `TEXCOORD_n` set it reads.
    pub tex_coord: u32,
    /// `KHR_texture_transform` as a 2×3 matrix applied to (u, v, 1).
    pub transform: Option<[[f32; 3]; 2]>,
    /// U and V wrap modes.
    pub wrap: [Wrap; 2],
    /// Magnification filter is `NEAREST` (pixel art).
    pub nearest: bool,
}

/// A decoded texture, RGBA8, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 4]>,
}

impl Image {
    pub fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        self.pixels[y as usize * self.width as usize + x as usize]
    }
}

/// What to load besides geometry.
#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    /// Decode the textures materials sample. Voxelizing without colors does
    /// not need them.
    pub textures: bool,
}

/// Load the glTF model at `path`.
pub fn load(path: &Path, options: LoadOptions) -> Result<Model> {
    let bytes = std::fs::read(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    // The glTF crate trusts the data it has validated; a file malformed in a
    // way validation does not catch must not take the process down with it.
    std::panic::catch_unwind(|| load_bytes(&bytes, &base, options))
        .unwrap_or_else(|_| Err(Error::Mesh("the file is malformed".into())))
}

/// Load a model from its bytes; external files resolve against `base`.
pub fn load_bytes(bytes: &[u8], base: &Path, options: LoadOptions) -> Result<Model> {
    let gltf = gltf::Gltf::from_slice_without_validation(bytes)
        .map_err(|e| Error::Mesh(format!("not a readable glTF file: {e}")))?;
    check_required_extensions(&gltf)?;
    validate(&gltf)?;
    let raw = raw_json(bytes)?;

    let gltf::Gltf { document, blob } = gltf;
    let buffers = load_buffers(&document, blob, base)?;

    let meshes: Vec<gltf::Mesh> = document.meshes().collect();
    let mut primitives = Vec::new();
    for (mesh, world) in scene_meshes(&document, &raw) {
        for primitive in meshes[mesh].primitives() {
            if let Some(p) = read_primitive(&primitive, &buffers, &world)? {
                primitives.push(p);
            }
        }
    }
    if primitives.is_empty() {
        return Err(Error::NoGeometry);
    }

    let raw_materials = raw.get("materials").and_then(Value::as_array);
    let materials: Vec<Material> = document
        .materials()
        .map(|m| {
            let raw_material = m
                .index()
                .and_then(|i| raw_materials.and_then(|all| all.get(i)));
            read_material(&m, raw_material, document.images().len())
        })
        .collect();

    let mut images = vec![None; document.images().len()];
    if options.textures {
        let used: HashSet<usize> = primitives.iter().filter_map(|p| p.material).collect();
        let mut wanted: Vec<usize> = used
            .iter()
            .filter_map(|&i| materials.get(i))
            .flat_map(|m| {
                let sg = m.specular_glossiness.as_ref();
                [
                    m.base_color_texture.as_ref(),
                    m.metallic_roughness_texture.as_ref(),
                    m.emissive_texture.as_ref(),
                    sg.and_then(|s| s.diffuse_texture.as_ref()),
                    sg.and_then(|s| s.specular_glossiness_texture.as_ref()),
                ]
            })
            .filter_map(|t| t.map(|t| t.image))
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        let views: Vec<gltf::buffer::View> = document.views().collect();
        let json = document.as_json();
        let decoded: Vec<(usize, Option<Image>)> = wanted
            .par_iter()
            .map(|&i| (i, decode_image(i, &json.images[i], &views, &buffers, base)))
            .collect();
        for (i, image) in decoded {
            images[i] = image;
        }
    }

    Ok(Model {
        primitives,
        materials,
        images,
    })
}

fn check_required_extensions(gltf: &gltf::Gltf) -> Result<()> {
    let missing: Vec<&str> = gltf
        .extensions_required()
        .filter(|e| !READABLE_EXTENSIONS.contains(e))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let compressed = missing.iter().any(|e| {
        matches!(
            *e,
            "KHR_draco_mesh_compression" | "EXT_meshopt_compression" | "KHR_mesh_quantization"
        )
    });
    let hint = if compressed {
        " — export it again without mesh compression or quantization"
    } else {
        ""
    };
    Err(Error::Mesh(format!(
        "the model needs glTF extension{} {}, which SchemGen2 cannot read{hint}",
        if missing.len() > 1 { "s" } else { "" },
        missing.join(", ")
    )))
}

fn validate(gltf: &gltf::Gltf) -> Result<()> {
    use gltf::json::validation::Validate;
    let root = gltf.document.as_json();
    let mut problems = Vec::new();
    root.validate(root, gltf::json::Path::new, &mut |path, error| {
        // Required extensions were judged above, against what this loader
        // reads rather than what the glTF crate has been told about.
        let path = path();
        if !path.as_str().starts_with("extensionsRequired") {
            problems.push(format!("{path}: {error}"));
        }
    });
    if problems.is_empty() {
        Ok(())
    } else {
        let shown = problems.len().min(3);
        Err(Error::Mesh(format!(
            "the file is not valid glTF ({}{})",
            problems[..shown].join("; "),
            if problems.len() > shown { "; …" } else { "" }
        )))
    }
}

/// The JSON as plain values: whether a material spelled out a factor, and
/// node transforms at the precision the file wrote them, are lost in the
/// typed document.
fn raw_json(bytes: &[u8]) -> Result<Value> {
    let json = if bytes.starts_with(b"glTF") {
        gltf::binary::Glb::from_slice(bytes)
            .map_err(|e| Error::Mesh(format!("not a readable GLB file: {e}")))?
            .json
    } else {
        std::borrow::Cow::Borrowed(bytes)
    };
    serde_json::from_slice(&json).map_err(|e| Error::Mesh(format!("unreadable glTF JSON: {e}")))
}

// ---- Buffers and images ---------------------------------------------------

fn load_buffers(
    document: &gltf::Document,
    mut blob: Option<Vec<u8>>,
    base: &Path,
) -> Result<Vec<Vec<u8>>> {
    document
        .buffers()
        .map(|buffer| {
            let data = match buffer.source() {
                gltf::buffer::Source::Bin => blob.take().ok_or_else(|| {
                    Error::Mesh(format!(
                        "buffer {} refers to the GLB's binary chunk, which is missing",
                        buffer.index()
                    ))
                })?,
                gltf::buffer::Source::Uri(uri) => read_uri(uri, base)?,
            };
            if data.len() < buffer.length() {
                return Err(Error::Mesh(format!(
                    "buffer {} holds {} bytes but the file says {}",
                    buffer.index(),
                    data.len(),
                    buffer.length()
                )));
            }
            Ok(data)
        })
        .collect()
}

/// The bytes a buffer or image URI names: a base64 data URI, or a file beside
/// the model.
fn read_uri(uri: &str, base: &Path) -> Result<Vec<u8>> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (meta, payload) = rest
            .split_once(',')
            .ok_or_else(|| Error::Mesh("malformed data URI".into()))?;
        if !meta.ends_with(";base64") {
            return Err(Error::Mesh("data URIs must be base64-encoded".into()));
        }
        let engine = base64::engine::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            base64::engine::GeneralPurposeConfig::new()
                .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
        );
        return engine
            .decode(payload.trim())
            .map_err(|e| Error::Mesh(format!("bad base64 in a data URI: {e}")));
    }
    if !EXTERNAL_FILES.load(Ordering::Relaxed) {
        return Err(Error::Mesh(format!(
            "the model refers to a separate file ({uri}); use a .glb, or a .gltf with its data embedded"
        )));
    }
    let path = relative_path(uri).ok_or_else(|| {
        Error::Mesh(format!(
            "only relative file references are supported: {uri}"
        ))
    })?;
    let full = base.join(&path);
    std::fs::read(&full).map_err(|e| {
        Error::Mesh(format!(
            "could not read {} beside the model: {e}",
            path.display()
        ))
    })
}

/// A URI reference as a relative path, or `None` for anything with a scheme
/// or a root.
fn relative_path(uri: &str) -> Option<PathBuf> {
    let before_slash = uri.split('/').next().unwrap_or("");
    if uri.is_empty() || uri.starts_with('/') || uri.starts_with('\\') || before_slash.contains(':')
    {
        return None;
    }
    let decoded = percent_decode(uri.split(['?', '#']).next().unwrap_or(uri))?;
    Some(PathBuf::from(decoded))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn decode_image(
    index: usize,
    image: &gltf::json::Image,
    views: &[gltf::buffer::View],
    buffers: &[Vec<u8>],
    base: &Path,
) -> Option<Image> {
    // Straight from the JSON: the typed accessor insists on a MIME type the
    // decoder does not need, and some exporters leave it out.
    let bytes: std::borrow::Cow<[u8]> = if let Some(view) = &image.buffer_view {
        let view = views.get(view.value())?;
        let data = buffers.get(view.buffer().index())?;
        let slice = data.get(view.offset()..view.offset().checked_add(view.length())?);
        std::borrow::Cow::Borrowed(slice?)
    } else {
        match read_uri(image.uri.as_deref()?, base) {
            Ok(bytes) => std::borrow::Cow::Owned(bytes),
            Err(e) => {
                log::warn!("texture {index}: {e}");
                return None;
            }
        }
    };
    match image::load_from_memory(&bytes) {
        Ok(decoded) => {
            let rgba = decoded.to_rgba8();
            let (width, height) = rgba.dimensions();
            let pixels = rgba
                .into_raw()
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2], p[3]])
                .collect();
            Some(Image {
                width,
                height,
                pixels,
            })
        }
        Err(e) => {
            log::warn!(
                "texture {index} could not be decoded ({e}); its material is sampled without it"
            );
            None
        }
    }
}

// ---- Materials ------------------------------------------------------------

fn read_material(material: &gltf::Material, raw: Option<&Value>, images: usize) -> Material {
    let pbr = material.pbr_metallic_roughness();
    let texture = |info: Option<gltf::texture::Info>| info.and_then(|i| texture_ref(&i, images));
    let has = |path: &[&str]| {
        let mut value = raw;
        for key in path {
            value = value.and_then(|v| v.get(key));
        }
        value.is_some_and(|v| !v.is_null())
    };

    let factor = |value: Option<&Value>, fallback: [f32; 4]| -> [f64; 4] {
        let parsed: Option<Vec<f64>> = value
            .and_then(Value::as_array)
            .and_then(|v| v.iter().map(Value::as_f64).collect());
        match parsed {
            Some(v) if v.len() == 4 => [v[0], v[1], v[2], v[3]],
            _ => fallback.map(f64::from),
        }
    };
    let raw_pbr = raw.and_then(|m| m.get("pbrMetallicRoughness"));

    let mut out = Material {
        name: material.name().map(str::to_string),
        base_color: factor(
            raw_pbr.and_then(|p| p.get("baseColorFactor")),
            pbr.base_color_factor(),
        ),
        base_color_texture: texture(pbr.base_color_texture()),
        metallic: has(&["pbrMetallicRoughness", "metallicFactor"]).then(|| pbr.metallic_factor()),
        roughness: pbr.roughness_factor(),
        metallic_roughness_texture: texture(pbr.metallic_roughness_texture()),
        emissive: has(&["emissiveFactor"]).then(|| material.emissive_factor()),
        emissive_strength: material.emissive_strength().unwrap_or(1.0),
        emissive_texture: texture(material.emissive_texture()),
        alpha_mode: match material.alpha_mode() {
            gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
            gltf::material::AlphaMode::Mask => {
                AlphaMode::Mask(material.alpha_cutoff().unwrap_or(0.5))
            }
            gltf::material::AlphaMode::Blend => AlphaMode::Blend,
        },
        specular_glossiness: None,
    };
    if let Some(sg) = material.pbr_specular_glossiness() {
        let raw_sg = raw
            .and_then(|m| m.get("extensions"))
            .and_then(|e| e.get("KHR_materials_pbrSpecularGlossiness"));
        out.specular_glossiness = Some(SpecularGlossiness {
            diffuse: factor(
                raw_sg.and_then(|s| s.get("diffuseFactor")),
                sg.diffuse_factor(),
            ),
            diffuse_texture: texture(sg.diffuse_texture()),
            specular: sg.specular_factor(),
            glossiness: sg.glossiness_factor(),
            specular_glossiness_texture: texture(sg.specular_glossiness_texture()),
        });
    }
    out
}

fn texture_ref(info: &gltf::texture::Info, images: usize) -> Option<TextureRef> {
    let texture = info.texture();
    // EXT_texture_webp names its own image; `source` is then a PNG/JPEG
    // fallback, or absent.
    let webp = texture
        .extension_value("EXT_texture_webp")
        .and_then(|v| v.get("source"))
        .and_then(Value::as_u64)
        .map(|i| i as usize);
    let image = webp
        .or_else(|| texture.source().map(|s| s.index()))
        .filter(|&i| i < images)?;
    let sampler = texture.sampler();
    let wrap = |mode: WrappingMode| match mode {
        WrappingMode::Repeat => Wrap::Repeat,
        WrappingMode::ClampToEdge => Wrap::Clamp,
        WrappingMode::MirroredRepeat => Wrap::Mirror,
    };
    let transform = info.texture_transform();
    Some(TextureRef {
        image,
        tex_coord: transform
            .as_ref()
            .and_then(|t| t.tex_coord())
            .unwrap_or(info.tex_coord()),
        transform: transform.map(|t| {
            let [ox, oy] = t.offset();
            let [sx, sy] = t.scale();
            let (sin, cos) = t.rotation().sin_cos();
            // T · R · S from the extension's spec, applied to (u, v, 1).
            [[cos * sx, sin * sy, ox], [-sin * sx, cos * sy, oy]]
        }),
        wrap: [wrap(sampler.wrap_s()), wrap(sampler.wrap_t())],
        nearest: sampler.mag_filter() == Some(MagFilter::Nearest),
    })
}

// ---- Scene graph ----------------------------------------------------------

/// Every mesh instance in the default scene with its world transform, in the
/// order trimesh's `Scene.dump` lists them.
fn scene_meshes(document: &gltf::Document, raw: &Value) -> Vec<(usize, Mat4)> {
    let nodes: Vec<gltf::Node> = document.nodes().collect();
    let raw_nodes = raw.get("nodes").and_then(Value::as_array);
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next());
    let Some(scene) = scene else {
        // A file with no scene is a library of meshes: take them all, in place.
        return document.meshes().map(|m| (m.index(), IDENTITY)).collect();
    };

    let mut stack: Vec<(Option<usize>, usize)> = scene.nodes().map(|n| (None, n.index())).collect();
    let mut seen = HashSet::new();
    let mut world: Vec<Mat4> = vec![IDENTITY; nodes.len()];
    let mut out = Vec::new();
    while let Some((parent, index)) = stack.pop() {
        if !seen.insert((parent, index)) {
            continue;
        }
        let node = &nodes[index];
        stack.extend(node.children().map(|c| (Some(index), c.index())));
        let local = local_matrix(raw_nodes.and_then(|n| n.get(index)));
        world[index] = match parent {
            Some(p) => mul(&world[p], &local),
            None => local,
        };
        if let Some(mesh) = node.mesh() {
            out.push((mesh.index(), world[index]));
        }
    }
    out
}

/// A node's own transform, `matrix · T · R · S`, from the JSON's doubles.
fn local_matrix(node: Option<&Value>) -> Mat4 {
    let numbers = |key: &str, n: usize| -> Option<Vec<f64>> {
        let values = node?.get(key)?.as_array()?;
        let parsed: Option<Vec<f64>> = values.iter().map(Value::as_f64).collect();
        parsed.filter(|v| v.len() == n && v.iter().all(|x| x.is_finite()))
    };
    let mut m = match numbers("matrix", 16) {
        // Column-major in the file.
        Some(c) => std::array::from_fn(|r| std::array::from_fn(|col| c[col * 4 + r])),
        None => IDENTITY,
    };
    if let Some(t) = numbers("translation", 3) {
        let mut tm = IDENTITY;
        for (row, value) in tm.iter_mut().zip(&t) {
            row[3] = *value;
        }
        m = mul(&m, &tm);
    }
    if let Some(q) = numbers("rotation", 4) {
        m = mul(&m, &quaternion_matrix([q[3], q[0], q[1], q[2]]));
    }
    if let Some(s) = numbers("scale", 3) {
        let mut sm = IDENTITY;
        for (i, value) in s.iter().enumerate() {
            sm[i][i] = *value;
        }
        m = mul(&m, &sm);
    }
    m
}

/// Rotation matrix of a `[w, x, y, z]` quaternion, normalizing it first — the
/// formula trimesh (Gohlke's `transformations.py`) uses.
fn quaternion_matrix(q: [f64; 4]) -> Mat4 {
    let n = q.iter().map(|v| v * v).sum::<f64>();
    if n < f64::EPSILON * 4.0 {
        return IDENTITY;
    }
    let s = (2.0 / n).sqrt();
    let q = q.map(|v| v * s);
    let o = |a: usize, b: usize| q[a] * q[b];
    [
        [
            1.0 - o(2, 2) - o(3, 3),
            o(1, 2) - o(3, 0),
            o(1, 3) + o(2, 0),
            0.0,
        ],
        [
            o(1, 2) + o(3, 0),
            1.0 - o(1, 1) - o(3, 3),
            o(2, 3) - o(1, 0),
            0.0,
        ],
        [
            o(1, 3) - o(2, 0),
            o(2, 3) + o(1, 0),
            1.0 - o(1, 1) - o(2, 2),
            0.0,
        ],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c] + a[r][3] * b[3][c]
        })
    })
}

/// Peak-to-peak of the difference from the identity over the top-left
/// `size`×`size` block — trimesh's `allclose`.
fn spread_from_identity(m: &Mat4, size: usize) -> f64 {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (r, row) in m.iter().enumerate().take(size) {
        for (c, value) in row.iter().enumerate().take(size) {
            let d = value - IDENTITY[r][c];
            lo = lo.min(d);
            hi = hi.max(d);
        }
    }
    hi - lo
}

fn determinant3(m: &Mat4) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

// ---- Primitives -----------------------------------------------------------

fn read_primitive(
    primitive: &gltf::Primitive,
    buffers: &[Vec<u8>],
    world: &Mat4,
) -> Result<Option<Primitive>> {
    let mode = primitive.mode();
    if !matches!(
        mode,
        Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan
    ) {
        return Ok(None);
    }
    let Some(position_accessor) = primitive.get(&gltf::Semantic::Positions) else {
        return Ok(None);
    };
    if position_accessor.data_type() != gltf::accessor::DataType::F32
        || position_accessor.dimensions() != gltf::accessor::Dimensions::Vec3
    {
        return Err(Error::Mesh("vertex positions must be float triples".into()));
    }
    if position_accessor.count() == 0 {
        return Ok(None);
    }

    let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
    let unreadable = || {
        Error::Mesh(format!(
            "primitive {} has unreadable data",
            primitive.index()
        ))
    };
    let positions: Vec<[f32; 3]> = reader.read_positions().ok_or_else(unreadable)?.collect();
    let n = positions.len();
    if positions.iter().flatten().any(|v| !v.is_finite()) {
        return Err(Error::Mesh(
            "the model has non-finite vertex positions".into(),
        ));
    }

    let indices: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => (0..n as u32).collect(),
    };
    if indices.iter().any(|&i| i as usize >= n) {
        return Err(Error::Mesh(format!(
            "primitive {} indexes past its {n} vertices",
            primitive.index()
        )));
    }
    let mut triangles: Vec<[u32; 3]> = match mode {
        Mode::Triangles => indices
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect(),
        // Every other strip triangle is reversed, keeping the winding.
        Mode::TriangleStrip => indices
            .windows(3)
            .enumerate()
            .map(|(i, t)| {
                if i % 2 == 0 {
                    [t[0], t[1], t[2]]
                } else {
                    [t[2], t[1], t[0]]
                }
            })
            .collect(),
        _ => (1..indices.len().saturating_sub(1))
            .map(|i| [indices[0], indices[i], indices[i + 1]])
            .collect(),
    };
    if triangles.is_empty() {
        return Ok(None);
    }

    let normals = primitive
        .get(&gltf::Semantic::Normals)
        .filter(|a| {
            a.data_type() == gltf::accessor::DataType::F32
                && a.dimensions() == gltf::accessor::Dimensions::Vec3
        })
        .and_then(|_| reader.read_normals())
        .map(|it| it.collect::<Vec<[f32; 3]>>())
        .filter(|v| v.len() == n);
    let mut tex_coords = Vec::new();
    while let Some(set) = reader.read_tex_coords(tex_coords.len() as u32) {
        let set: Vec<[f32; 2]> = set.into_f32().collect();
        tex_coords.push(if set.len() == n { set } else { Vec::new() });
    }
    let colors = reader
        .read_colors(0)
        .map(|c| c.into_rgba_f32().collect::<Vec<[f32; 4]>>())
        .filter(|v| v.len() == n);

    let mut positions: Vec<[f64; 3]> = positions.iter().map(|p| p.map(f64::from)).collect();
    let mut normals = normals;
    // trimesh leaves a mesh alone under a transform within 1e-8 of the
    // identity; "rotation" there means any change to the 3×3 part.
    if spread_from_identity(world, 4) >= 1e-8 {
        let m = world;
        let off_identity = (0..4)
            .flat_map(|r| (0..4).map(move |c| (r, c)))
            .map(|(r, c)| (m[r][c] - IDENTITY[r][c]).abs())
            .fold(0.0, f64::max);
        if off_identity >= 1e-8 {
            for p in &mut positions {
                let [x, y, z] = *p;
                *p = std::array::from_fn(|r| m[r][0] * x + m[r][1] * y + m[r][2] * z + m[r][3]);
            }
        }
        if spread_from_identity(world, 3) >= 1e-6 {
            if determinant3(world) < 0.0 {
                for t in &mut triangles {
                    t.swap(0, 2);
                }
            }
            if let Some(normals) = &mut normals {
                transform_normals(normals, world);
            }
        }
    }

    Ok(Some(Primitive {
        positions,
        triangles,
        normals,
        tex_coords,
        colors,
        material: primitive.material().index(),
    }))
}

/// Normals go through the inverse transpose of the 3×3 part, which keeps them
/// perpendicular under non-uniform scale, then back to unit length.
fn transform_normals(normals: &mut [[f32; 3]], m: &Mat4) {
    // The cofactor matrix is the inverse transpose up to a positive or
    // negative scale, and the sign is fixed by the determinant.
    let c =
        |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    let cof = [
        [c(1, 1, 2, 2), -c(1, 0, 2, 2), c(1, 0, 2, 1)],
        [-c(0, 1, 2, 2), c(0, 0, 2, 2), -c(0, 0, 2, 1)],
        [c(0, 1, 1, 2), -c(0, 0, 1, 2), c(0, 0, 1, 1)],
    ];
    let sign = if determinant3(m) < 0.0 { -1.0 } else { 1.0 };
    for n in normals {
        let v = n.map(f64::from);
        let t: [f64; 3] = std::array::from_fn(|r| {
            sign * (cof[r][0] * v[0] + cof[r][1] * v[1] + cof[r][2] * v[2])
        });
        let len = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
        *n = if len > 1e-12 {
            t.map(|x| (x / len) as f32)
        } else {
            [0.0; 3]
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    fn load_fixture(name: &str) -> Model {
        load(&fixture(name), LoadOptions { textures: true }).unwrap()
    }

    #[test]
    fn scene_order_follows_trimesh() {
        // trimesh dumps metal_transforms.glb as ball1, ball0, slab: the last
        // root first.
        let model = load_fixture("metal_transforms.glb");
        let counts: Vec<usize> = model.primitives.iter().map(|p| p.triangles.len()).collect();
        assert_eq!(counts, vec![1280, 1280, 12]);
    }

    #[test]
    fn textures_and_factors_are_read() {
        let model = load_fixture("textured.glb");
        assert_eq!(model.primitives.len(), 2);
        let textured = model
            .primitives
            .iter()
            .find(|p| {
                model
                    .material_of(p)
                    .is_some_and(|m| m.base_color_texture.is_some())
            })
            .expect("a textured primitive");
        let material = model.material_of(textured).unwrap();
        let image = model
            .image(material.base_color_texture.as_ref().unwrap())
            .unwrap();
        assert_eq!((image.width, image.height), (64, 64));
        assert!(textured.tex_coords(0).is_some());
        let red = model
            .primitives
            .iter()
            .filter_map(|p| model.material_of(p))
            .find(|m| m.base_color_texture.is_none())
            .unwrap();
        // trimesh wrote the factor it was given, (0.9, 0.1, 0.1), rounded to bytes.
        let expected = [230.0 / 255.0, 26.0 / 255.0, 26.0 / 255.0, 1.0];
        assert!(red
            .base_color
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 1e-9));
    }

    #[test]
    fn vertex_colors_are_linear_floats() {
        let model = load_fixture("vertex_colors.glb");
        let colors = model.primitives[0].colors.as_ref().unwrap();
        assert_eq!(colors.len(), model.primitives[0].positions.len());
        assert!(colors.iter().flatten().all(|c| (0.0..=1.0).contains(c)));
    }

    #[test]
    fn transforms_are_baked_in() {
        let model = load_fixture("metal_transforms.glb");
        // The balls are scaled 1.3 in Y and lifted 0.7 around their centers.
        let ball = &model.primitives[0];
        let (lo, hi) = ball
            .positions
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                (lo.min(p[1]), hi.max(p[1]))
            });
        assert!((hi - lo - 1.3).abs() < 1e-6, "height {}", hi - lo);
        assert!(((hi + lo) / 2.0 - 0.7).abs() < 1e-6);
    }

    #[test]
    fn mirroring_reverses_winding() {
        let mut m = IDENTITY;
        m[0][0] = -1.0;
        assert!(determinant3(&m) < 0.0);
        assert!(spread_from_identity(&m, 3) >= 1e-6);
        assert!(spread_from_identity(&IDENTITY, 4) < 1e-8);
    }

    #[test]
    fn quaternions_rotate_like_trimesh() {
        // 90° about Y: x → -z.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let m = quaternion_matrix([h, 0.0, h, 0.0]);
        assert!((m[2][0] + 1.0).abs() < 1e-12 && m[0][0].abs() < 1e-12);
    }

    #[test]
    fn uris_are_relative_and_decoded() {
        assert_eq!(
            relative_path("tex/a%20b.png"),
            Some(PathBuf::from("tex/a b.png"))
        );
        assert_eq!(relative_path("/etc/passwd"), None);
        assert_eq!(relative_path("file:///etc/passwd"), None);
        assert_eq!(relative_path("C:\\x.png"), None);
        assert_eq!(
            read_uri("data:application/octet-stream;base64,AAEC", Path::new("")).unwrap(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn required_extensions_are_refused_by_name() {
        let json = br#"{"asset":{"version":"2.0"},"extensionsUsed":["KHR_draco_mesh_compression"],
            "extensionsRequired":["KHR_draco_mesh_compression"]}"#;
        let err = load_bytes(json, Path::new(""), LoadOptions { textures: false }).unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("KHR_draco_mesh_compression")
                && text.contains("without mesh compression"),
            "{text}"
        );
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(load_bytes(
            b"glTF\x02\0\0\0garbage",
            Path::new(""),
            LoadOptions { textures: false }
        )
        .is_err());
        assert!(load_bytes(b"{}", Path::new(""), LoadOptions { textures: false }).is_err());
    }
}
