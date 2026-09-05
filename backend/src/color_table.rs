//! Build block → color lookup tables from Minecraft texture packs.
//!
//! For each block texture the sampler computes the alpha-weighted mean color
//! in *linear light* — which is what the block actually reads as from a
//! distance, and therefore what CIEDE2000 matching should compare against.
//! Averaging the raw sRGB bytes (or clustering them, as the old K-means
//! sampler did) systematically skews dark and lets voxels match a minority
//! speckle color instead of the block's overall appearance.
//!
//! A block with several face textures (e.g. log side + top) averages each
//! file separately, then averages the per-file means with equal weight, so a
//! large texture cannot outvote a small one.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::palette::rgb_to_lab;
use crate::types::{BlockColorEntry, ColorTable};

// ── Texture override rules ──────────────────────────────────────────────

/// Mapping from block ID (without minecraft: prefix) to texture file names.
type TextureOverrides = HashMap<String, Vec<String>>;

fn texture_overrides() -> TextureOverrides {
    let mut m = HashMap::new();
    // Blocks are seen from the side in a statue, so side textures lead.
    let ov: &[(_, &[_])] = &[
        ("snow_block", &["snow.png"]),
        ("magma_block", &["magma.png"]),
        ("grass_block", &["grass_block_side.png"]),
        ("dirt_path", &["dirt_path_side.png", "dirt_path_top.png"]),
        ("hay_block", &["hay_block_side.png", "hay_block_top.png"]),
        ("pumpkin", &["pumpkin_side.png"]),
        ("carved_pumpkin", &["pumpkin_side.png"]),
        ("melon", &["melon_side.png"]),
        ("crafting_table", &["crafting_table_front.png", "crafting_table_top.png"]),
        ("furnace", &["furnace_side.png", "furnace_top.png"]),
        ("tnt", &["tnt_side.png"]),
        ("bookshelf", &["bookshelf.png"]),
        ("podzol", &["podzol_side.png", "podzol_top.png"]),
        ("mycelium", &["mycelium_side.png", "mycelium_top.png"]),
        ("infested_stone", &["stone.png"]),
        ("moss_carpet", &["moss_block.png"]),
        ("nether_brick_fence", &["nether_bricks.png"]),
        ("smooth_quartz", &["quartz_block_top.png"]),
        ("smooth_red_sandstone", &["red_sandstone_top.png"]),
        ("smooth_sandstone", &["sandstone_top.png"]),
        ("scaffolding", &["scaffolding_top.png"]),
        ("water", &["water_still.png"]),
        ("lava", &["lava_still.png"]),
        ("fire", &["fire_0.png"]),
        ("oak_log", &["oak_log.png"]),
        ("spruce_log", &["spruce_log.png"]),
        ("birch_log", &["birch_log.png"]),
        ("jungle_log", &["jungle_log.png"]),
        ("acacia_log", &["acacia_log.png"]),
        ("dark_oak_log", &["dark_oak_log.png"]),
        ("mangrove_log", &["mangrove_log.png"]),
        ("cherry_log", &["cherry_log.png"]),
        ("pale_oak_log", &["pale_oak_log.png"]),
        ("crimson_stem", &["crimson_stem.png"]),
        ("warped_stem", &["warped_stem.png"]),
        ("crimson_hyphae", &["crimson_stem.png"]),
        ("warped_hyphae", &["warped_stem.png"]),
        ("stripped_oak_log", &["stripped_oak_log.png"]),
        ("stripped_spruce_log", &["stripped_spruce_log.png"]),
        ("stripped_birch_log", &["stripped_birch_log.png"]),
        ("stripped_jungle_log", &["stripped_jungle_log.png"]),
        ("stripped_acacia_log", &["stripped_acacia_log.png"]),
        ("stripped_dark_oak_log", &["stripped_dark_oak_log.png"]),
        ("stripped_mangrove_log", &["stripped_mangrove_log.png"]),
        ("stripped_cherry_log", &["stripped_cherry_log.png"]),
        ("stripped_pale_oak_log", &["stripped_pale_oak_log.png"]),
        ("stripped_crimson_stem", &["stripped_crimson_stem.png"]),
        ("stripped_warped_stem", &["stripped_warped_stem.png"]),
        ("stripped_crimson_hyphae", &["stripped_crimson_stem.png"]),
        ("stripped_warped_hyphae", &["stripped_warped_stem.png"]),
        ("purpur_block", &["purpur_block.png"]),
        ("purpur_pillar", &["purpur_pillar.png"]),
        ("quartz_pillar", &["quartz_pillar.png"]),
        ("polished_basalt", &["polished_basalt_side.png"]),
        ("basalt", &["basalt_side.png"]),
        ("bone_block", &["bone_block_side.png"]),
        ("ochre_froglight", &["ochre_froglight_side.png"]),
        ("verdant_froglight", &["verdant_froglight_side.png"]),
        ("pearlescent_froglight", &["pearlescent_froglight_side.png"]),
        ("ancient_debris", &["ancient_debris_side.png"]),
        ("respawn_anchor", &["respawn_anchor_side0.png"]),
        ("lodestone", &["lodestone_side.png"]),
        ("dried_kelp_block", &["dried_kelp_side.png"]),
        ("target", &["target_side.png"]),
    ];
    for (k, v) in ov { m.insert(k.to_string(), v.iter().map(|s| s.to_string()).collect()); }
    m
}

// ── Texture scanning ────────────────────────────────────────────────────

fn skip_texture(stem: &str) -> bool {
    let skips = ["_particle", "_gui", "destroy_stage", "debug", "item/", "entity/"];
    skips.iter().any(|s| stem.contains(s))
}

fn infer_block_id(filename: &str) -> Option<String> {
    let stem = Path::new(filename).file_stem()?.to_str()?;
    if skip_texture(stem) { return None; }

    let suffixes = [
        "_side", "_top", "_front", "_bottom", "_inner", "_outer",
        "_overlay", "_back", "_on", "_off", "_lit", "_side0", "_side1",
        "_stage0", "_stage1", "_stage2", "_stage3",
    ];

    let mut base = stem.to_string();
    for s in &suffixes {
        if base.ends_with(s) {
            base = base[..base.len() - s.len()].to_string();
        }
    }

    // Reject growth stages (not final block)
    if stem.contains("_stage") && !stem.ends_with("_stage3") && !stem.ends_with("_stage7") {
        return None;
    }

    Some(base)
}

fn group_textures(block_dir: &Path) -> HashMap<String, Vec<PathBuf>> {
    let mut block_to_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let overrides = texture_overrides();

    // Collect all PNG files
    let mut all_files = Vec::new();
    if let Ok(entries) = fs::read_dir(block_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "png") {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !name.contains(".mcmeta") {
                        all_files.push(path);
                    }
                }
            }
        }
    }

    // 1. Map every file to its inferred block ID
    for fpath in &all_files {
        if let Some(fname) = fpath.file_name().and_then(|n| n.to_str()) {
            if let Some(block_id) = infer_block_id(fname) {
                block_to_files.entry(block_id).or_default().push(fpath.clone());
            }
        }
    }

    // 2. Apply overrides
    for (block_id, textures) in &overrides {
        let mut found = Vec::new();
        for tname in textures {
            let p = block_dir.join(tname);
            if p.exists() {
                found.push(p);
            }
        }
        if !found.is_empty() {
            block_to_files.insert(block_id.clone(), found);
        }
    }

    block_to_files
}

// ── Color extraction ────────────────────────────────────────────────────

fn srgb8_to_linear(v: u8) -> f64 {
    let c = v as f64 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb8(c: f64) -> f32 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as f32
}

/// Alpha-weighted mean color of one texture in linear light.
///
/// Animated textures (tall strips with an .mcmeta) contribute only their
/// first square frame. Returns `None` when the texture is effectively empty.
fn texture_mean_linear(tex_path: &Path) -> Option<[f64; 3]> {
    let img = image::open(tex_path).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let frame = w.min(h);

    let mut sum = [0.0f64; 3];
    let mut weight = 0.0f64;
    for y in 0..frame {
        for x in 0..frame {
            let px = rgba.get_pixel(x, y);
            let a = px[3] as f64 / 255.0;
            if a <= 0.0 { continue; }
            sum[0] += srgb8_to_linear(px[0]) * a;
            sum[1] += srgb8_to_linear(px[1]) * a;
            sum[2] += srgb8_to_linear(px[2]) * a;
            weight += a;
        }
    }

    // Nearly fully transparent textures carry no usable color.
    if weight < (frame as f64 * frame as f64) * 0.02 {
        return None;
    }
    Some([sum[0] / weight, sum[1] / weight, sum[2] / weight])
}

// ── Public API ──────────────────────────────────────────────────────────

/// Build a color table from a texture pack's `block/` directory.
///
/// When `anti_grief` is set, block IDs pass through [`crate::blocks::sanitize`]
/// so the resulting table contains only valid, placeable, grief-resistant
/// full blocks (and unwaxed copper is stored under its waxed ID).
pub fn build_table(texture_pack_dir: &Path, output_path: &Path, anti_grief: bool) -> ColorTable {
    log::info!("Scanning textures in: {}", texture_pack_dir.display());
    let block_to_files = group_textures(texture_pack_dir);

    let total = block_to_files.len();
    log::info!("Processing {} texture groups...", total);

    let mut table: ColorTable = HashMap::new();
    let mut skipped = 0usize;

    for (block_name, tex_paths) in &block_to_files {
        let full_name = if anti_grief {
            match crate::blocks::sanitize(block_name) {
                Some(name) => name,
                None => { skipped += 1; continue; }
            }
        } else if block_name.contains(':') {
            block_name.clone()
        } else {
            format!("minecraft:{block_name}")
        };

        // Equal weight per face texture, regardless of resolution.
        let means: Vec<[f64; 3]> = tex_paths.iter()
            .filter_map(|tp| texture_mean_linear(tp))
            .collect();
        if means.is_empty() { continue; }
        let n = means.len() as f64;
        let mean = means.iter().fold([0.0f64; 3], |acc, m| {
            [acc[0] + m[0] / n, acc[1] + m[1] / n, acc[2] + m[2] / n]
        });

        let rgb = [
            linear_to_srgb8(mean[0]),
            linear_to_srgb8(mean[1]),
            linear_to_srgb8(mean[2]),
        ];
        let lab = rgb_to_lab(rgb[0], rgb[1], rgb[2]);

        // One entry per block: its area-average appearance. Multiple entries
        // (the old per-cluster scheme) let voxels match a texture's minority
        // speckle color, which reads as the wrong block in-game.
        table.entry(full_name).or_default().push(BlockColorEntry {
            lab: [lab.l, lab.a, lab.b],
            rgb,
            weight: 1.0,
        });
    }

    // Keys that collapsed to the same block (e.g. hyphae + stem) merge into
    // one averaged entry so the palette holds a single color per block.
    for entries in table.values_mut() {
        if entries.len() > 1 {
            let n = entries.len() as f32;
            let rgb = entries.iter().fold([0.0f32; 3], |acc, e| {
                [acc[0] + e.rgb[0] / n, acc[1] + e.rgb[1] / n, acc[2] + e.rgb[2] / n]
            });
            let lab = rgb_to_lab(rgb[0], rgb[1], rgb[2]);
            *entries = vec![BlockColorEntry { lab: [lab.l, lab.a, lab.b], rgb, weight: 1.0 }];
        }
    }

    log::info!("Built table: {} blocks ({} texture groups filtered out).", table.len(), skipped);

    let json = serde_json::to_string_pretty(&table).unwrap();
    fs::write(output_path, &json).unwrap();
    log::info!("Saved color table → {}", output_path.display());

    table
}
