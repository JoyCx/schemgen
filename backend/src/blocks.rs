//! Curated anti-grief building palette.
//!
//! Every ID here is a real, placeable Minecraft block (Java 1.21) that is:
//!   - a full, stable cube: placeable in mid-air, no gravity, no attachment
//!   - not flammable: fire cannot ignite or destroy it (per the fire-spread
//!     registry — so no wool, overworld wood, hay, bookshelves, coal blocks,
//!     dried kelp, target blocks; nether "wood" is fine because crimson and
//!     warped materials do not burn)
//!   - not carriable by endermen (`#enderman_holdable`: dirt, grass, podzol,
//!     mycelium, nylium, sand, gravel, clay, pumpkins, melons, TNT, cacti)
//!   - not a block entity and not a redstone emitter/receiver, so pasting a
//!     schematic can never activate machinery or store items
//!   - not subject to environmental decay: no melting ice, no drying coral,
//!     no oxidizing copper (unwaxed copper is remapped to its waxed variant)
//!   - not an ore or treasure block texture that invites mining on survival
//!     servers (mineral storage blocks like iron/gold blocks are kept — they
//!     are deliberate, uniform build materials rather than "found loot")

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// Unwaxed copper → waxed equivalent. The textures are identical; the waxed
/// IDs simply never oxidize, which is exactly what a schematic wants.
const COPPER_WAX_REMAP: &[(&str, &str)] = &[
    ("copper_block", "waxed_copper_block"),
    ("exposed_copper", "waxed_exposed_copper"),
    ("weathered_copper", "waxed_weathered_copper"),
    ("oxidized_copper", "waxed_oxidized_copper"),
    ("cut_copper", "waxed_cut_copper"),
    ("exposed_cut_copper", "waxed_exposed_cut_copper"),
    ("weathered_cut_copper", "waxed_weathered_cut_copper"),
    ("oxidized_cut_copper", "waxed_oxidized_cut_copper"),
    ("chiseled_copper", "waxed_chiseled_copper"),
    ("exposed_chiseled_copper", "waxed_exposed_chiseled_copper"),
    ("weathered_chiseled_copper", "waxed_weathered_chiseled_copper"),
    ("oxidized_chiseled_copper", "waxed_oxidized_chiseled_copper"),
];

const DYE_COLORS: &[&str] = &[
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink",
    "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red",
    "black",
];

/// Blocks allowed in addition to the per-color families below.
const BASE_ALLOWED: &[&str] = &[
    // Stone
    "stone", "granite", "polished_granite", "diorite", "polished_diorite",
    "andesite", "polished_andesite", "cobblestone", "mossy_cobblestone",
    "stone_bricks", "mossy_stone_bricks", "cracked_stone_bricks",
    "chiseled_stone_bricks", "smooth_stone",
    // Deepslate
    "deepslate", "cobbled_deepslate", "polished_deepslate", "deepslate_bricks",
    "cracked_deepslate_bricks", "deepslate_tiles", "cracked_deepslate_tiles",
    "chiseled_deepslate", "reinforced_deepslate",
    // Tuff / calcite / dripstone
    "tuff", "polished_tuff", "tuff_bricks", "chiseled_tuff",
    "chiseled_tuff_bricks", "calcite", "dripstone_block",
    // Blackstone
    "blackstone", "polished_blackstone", "polished_blackstone_bricks",
    "cracked_polished_blackstone_bricks", "chiseled_polished_blackstone",
    // Basalt
    "basalt", "polished_basalt", "smooth_basalt",
    // Sandstone (sandstone does not fall — only sand does)
    "sandstone", "chiseled_sandstone", "cut_sandstone", "smooth_sandstone",
    "red_sandstone", "chiseled_red_sandstone", "cut_red_sandstone",
    "smooth_red_sandstone",
    // End
    "end_stone", "end_stone_bricks", "purpur_block", "purpur_pillar",
    "obsidian", "crying_obsidian",
    // Nether
    "netherrack", "nether_bricks", "cracked_nether_bricks",
    "chiseled_nether_bricks", "red_nether_bricks", "quartz_block",
    "chiseled_quartz_block", "quartz_bricks", "quartz_pillar", "smooth_quartz",
    "glowstone", "magma_block", "soul_soil", "nether_wart_block",
    "warped_wart_block", "shroomlight",
    "ochre_froglight", "verdant_froglight", "pearlescent_froglight",
    // Nether wood — crimson/warped materials are not flammable
    "crimson_planks", "warped_planks", "crimson_stem", "warped_stem",
    "stripped_crimson_stem", "stripped_warped_stem",
    "crimson_hyphae", "warped_hyphae",
    "stripped_crimson_hyphae", "stripped_warped_hyphae",
    // Ocean
    "prismarine", "prismarine_bricks", "dark_prismarine", "sea_lantern",
    "dead_tube_coral_block", "dead_brain_coral_block",
    "dead_bubble_coral_block", "dead_fire_coral_block",
    "dead_horn_coral_block", "sponge", "wet_sponge",
    // Mineral storage blocks (not ores)
    "iron_block", "gold_block", "diamond_block", "emerald_block",
    "lapis_block", "netherite_block", "amethyst_block", "raw_iron_block",
    "raw_copper_block", "raw_gold_block", "bone_block",
    // Ice & snow that never melts
    "packed_ice", "blue_ice", "snow_block",
    // Earth-tones that endermen cannot pick up and fire cannot burn
    "rooted_dirt", "packed_mud", "mud_bricks", "muddy_mangrove_roots",
    // Mushroom blocks are full cubes and not flammable
    "brown_mushroom_block", "red_mushroom_block", "mushroom_stem",
    // Misc
    "bricks", "terracotta", "honeycomb_block", "sculk", "jack_o_lantern",
    "resin_block", "resin_bricks", "chiseled_resin_bricks",
];

fn allowed_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut set: HashSet<&'static str> = BASE_ALLOWED.iter().copied().collect();
        for (_, waxed) in COPPER_WAX_REMAP {
            set.insert(waxed);
        }
        for color in DYE_COLORS {
            for family in ["concrete", "terracotta", "glazed_terracotta"] {
                set.insert(Box::leak(format!("{color}_{family}").into_boxed_str()));
            }
        }
        set
    })
}

fn wax_remap() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| COPPER_WAX_REMAP.iter().copied().collect())
}

/// Map a color-table key to the block ID that should actually be placed.
///
/// Returns `None` for anything that is not on the curated anti-grief list —
/// including texture-derived pseudo-IDs (`cartography_table_side2`,
/// `grass_block_snow`, …) that are not placeable blocks at all. Unwaxed
/// copper passes through as its waxed variant.
pub fn sanitize(name: &str) -> Option<String> {
    let id = name.strip_prefix("minecraft:").unwrap_or(name);
    let id = wax_remap().get(id).copied().unwrap_or(id);
    allowed_set()
        .contains(id)
        .then(|| format!("minecraft:{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_safe_full_blocks() {
        for id in ["minecraft:stone", "minecraft:red_concrete", "cyan_terracotta",
                   "minecraft:crimson_planks", "minecraft:packed_ice"] {
            assert!(sanitize(id).is_some(), "{id} should be allowed");
        }
    }

    #[test]
    fn remaps_copper_to_waxed() {
        assert_eq!(sanitize("minecraft:oxidized_cut_copper").as_deref(),
                   Some("minecraft:waxed_oxidized_cut_copper"));
        assert_eq!(sanitize("minecraft:waxed_copper_block").as_deref(),
                   Some("minecraft:waxed_copper_block"));
    }

    #[test]
    fn rejects_griefable_and_invalid() {
        for id in [
            // flammable
            "minecraft:oak_planks", "minecraft:white_wool", "minecraft:hay_block",
            "minecraft:coal_block", "minecraft:bookshelf", "minecraft:dried_kelp_block",
            "minecraft:target", "minecraft:bamboo_planks",
            // gravity
            "minecraft:sand", "minecraft:gravel", "minecraft:red_concrete_powder",
            // endermen-carriable
            "minecraft:dirt", "minecraft:grass_block", "minecraft:clay",
            "minecraft:pumpkin", "minecraft:melon", "minecraft:mycelium",
            "minecraft:warped_nylium",
            // block entities / redstone
            "minecraft:furnace", "minecraft:barrel", "minecraft:observer",
            "minecraft:redstone_block", "minecraft:shulker_box", "minecraft:jukebox",
            // decay / environment
            "minecraft:ice", "minecraft:brain_coral_block", "minecraft:copper_bulb",
            // ores / treasure textures
            "minecraft:diamond_ore", "minecraft:ancient_debris", "minecraft:gilded_blackstone",
            // not full cubes
            "minecraft:smooth_stone_slab", "minecraft:glass", "minecraft:soul_sand",
            "minecraft:mud", "minecraft:oak_shelf",
            // texture-derived pseudo IDs
            "minecraft:cartography_table_side2", "minecraft:grass_block_snow",
            "minecraft:mushroom_block_inside", "minecraft:crafter_east_triggered",
            "minecraft:beehive_end", "minecraft:barrel_top_open",
        ] {
            assert!(sanitize(id).is_none(), "{id} should be rejected");
        }
    }
}
