//! Write .litematic files (Litematica schematic format) using raw NBT.
//!
//! The Litematica format requires:
//!   - GZip-compressed NBT (Big Endian, Java Edition)
//!   - Root compound with: Version, MinecraftDataVersion, Metadata, Regions
//!   - Each region has: Position, Size, BlockStatePalette, BlockStates (long[] palette indices)
//!   - BlockStates encodes a 3D grid as a bit-packed long array
//!
//! We implement NBT serialization directly (no library dependency) since the
//! format is well-defined and we only need a specific subset.

use std::io::Write;
use std::sync::atomic::{AtomicI32, Ordering};
use flate2::write::GzEncoder;
use flate2::Compression;

/// `MinecraftDataVersion` stamped into every schematic.
///
/// It must be at or above the newest block in the palette:
/// `color_table_safe.json` ships `resin_block` / `resin_bricks` /
/// `chiseled_resin_bricks`, added in 1.21.4. Declaring an *older* version makes
/// Minecraft's DataFixerUpper try to upgrade block names that did not exist yet.
/// Litematica reads schematics stamped below the running game, so the default
/// loads in 1.21.8 and every later version alike.
///
/// | Minecraft | Data version |
/// |---|---|
/// | 1.21.8  | 4440 (default) |
/// | 1.21.11 | 4671 |
///
/// Override with `--data-version` on the CLI or `SCHEMGEN_DATA_VERSION`.
pub const DEFAULT_DATA_VERSION: i32 = 4440;

static DATA_VERSION: AtomicI32 = AtomicI32::new(DEFAULT_DATA_VERSION);

/// The data version schematics are currently stamped with.
pub fn data_version() -> i32 {
    DATA_VERSION.load(Ordering::Relaxed)
}

/// Set the stamped data version. Values at or below zero are ignored, since a
/// schematic that claims data version 0 makes the game refuse the file outright.
pub fn set_data_version(version: i32) {
    if version > 0 {
        DATA_VERSION.store(version, Ordering::Relaxed);
    } else {
        log::warn!("Ignoring invalid data version {version}; keeping {}", data_version());
    }
}

/// Apply `SCHEMGEN_DATA_VERSION` if it is set to a positive integer. Called once
/// at startup so the server, the CLI and the mod all agree without a flag.
pub fn apply_env_data_version() {
    if let Ok(raw) = std::env::var("SCHEMGEN_DATA_VERSION") {
        match raw.trim().parse::<i32>() {
            Ok(v) if v > 0 => {
                set_data_version(v);
                log::info!("SCHEMGEN_DATA_VERSION={v}");
            }
            _ => log::warn!("SCHEMGEN_DATA_VERSION='{raw}' is not a positive integer — ignored"),
        }
    }
}

// ── NBT tag types ──────────────────────────────────────────────────────

const TAG_END: u8 = 0;
const TAG_BYTE: u8 = 1;
const TAG_SHORT: u8 = 2;
const TAG_INT: u8 = 3;
const TAG_LONG: u8 = 4;
const TAG_FLOAT: u8 = 5;
const TAG_DOUBLE: u8 = 6;
const TAG_BYTE_ARRAY: u8 = 7;
const TAG_STRING: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;
const TAG_INT_ARRAY: u8 = 11;
const TAG_LONG_ARRAY: u8 = 12;

struct NbtWriter {
    data: Vec<u8>,
}

impl NbtWriter {
    fn new() -> Self {
        Self { data: Vec::new() }
    }

    fn write_raw(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }

    fn write_be_u16(&mut self, v: u16) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }

    fn write_be_i32(&mut self, v: i32) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }

    fn write_be_i64(&mut self, v: i64) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }

    fn write_be_f32(&mut self, v: f32) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }

    fn write_be_f64(&mut self, v: f64) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }

    fn write_string(&mut self, s: &str) {
        let bytes = cesu8::to_cesu8(s);
        self.write_be_u16(bytes.len() as u16);
        self.write_raw(&bytes);
    }

    fn begin_compound_named(&mut self, name: &str) {
        self.data.push(TAG_COMPOUND);
        self.write_string(name);
    }

    fn end_compound(&mut self) {
        self.data.push(TAG_END);
    }

    fn write_byte(&mut self, name: &str, v: i8) {
        self.data.push(TAG_BYTE);
        self.write_string(name);
        self.data.push(v as u8);
    }

    fn write_int(&mut self, name: &str, v: i32) {
        self.data.push(TAG_INT);
        self.write_string(name);
        self.write_be_i32(v);
    }

    fn write_long(&mut self, name: &str, v: i64) {
        self.data.push(TAG_LONG);
        self.write_string(name);
        self.write_be_i64(v);
    }

    fn write_string_tag(&mut self, name: &str, v: &str) {
        self.data.push(TAG_STRING);
        self.write_string(name);
        self.write_string(v);
    }

    fn begin_list(&mut self, name: &str, elem_type: u8, len: i32) {
        self.data.push(TAG_LIST);
        self.write_string(name);
        self.data.push(elem_type);
        self.write_be_i32(len);
    }

    fn begin_compound_in_list(&mut self) {
        // List elements do not repeat the element tag or a name. The list
        // header already declares TAG_COMPOUND; each element is only the
        // compound payload terminated by TAG_END.
    }

    fn end_list_compound(&mut self) {
        self.data.push(TAG_END);
    }

    fn write_int_in_list(&mut self, v: i32) {
        self.data.push(TAG_INT);
    }

    /// Writes packed block states. Takes u64 because that is how the bit
    /// packing builds them; NBT stores them as signed longs with the same bits.
    fn write_long_array(&mut self, name: &str, longs: &[u64]) {
        self.data.push(TAG_LONG_ARRAY);
        self.write_string(name);
        self.write_be_i32(longs.len() as i32);
        self.data.reserve(longs.len() * 8);
        for &l in longs {
            self.data.extend_from_slice(&l.to_be_bytes());
        }
    }

    fn write_int_array(&mut self, name: &str, values: &[i32]) {
        self.data.push(TAG_INT_ARRAY);
        self.write_string(name);
        self.write_be_i32(values.len() as i32);
        for &v in values {
            self.write_be_i32(v);
        }
    }

    fn finish(self) -> Vec<u8> {
        self.data
    }
}

// ── Cesu-8 encoding (Java's modified UTF-8) ────────────────────────────

mod cesu8 {
    pub fn to_cesu8(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for ch in s.chars() {
            let cp = ch as u32;
            if cp <= 0x7F {
                out.push(cp as u8);
            } else if cp <= 0x7FF {
                out.push(0xC0 | ((cp >> 6) & 0x1F) as u8);
                out.push(0x80 | (cp & 0x3F) as u8);
            } else if cp <= 0xFFFF {
                out.push(0xE0 | ((cp >> 12) & 0x0F) as u8);
                out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
                out.push(0x80 | (cp & 0x3F) as u8);
            } else {
                // Surrogate pair for supplementary planes
                let cp2 = cp - 0x10000;
                let hi = 0xD800 | ((cp2 >> 10) & 0x3FF);
                let lo = 0xDC00 | (cp2 & 0x3FF);

                out.push(0xED);
                out.push(0xA0 | ((hi >> 6) & 0x0F) as u8);
                out.push(0x80 | (hi & 0x3F) as u8);
                out.push(0xED);
                out.push(0xB0 | ((lo >> 6) & 0x0F) as u8);
                out.push(0x80 | (lo & 0x3F) as u8);
            }
        }
        out
    }
}

// ── Litematic writer ────────────────────────────────────────────────────

/// Write a .litematic schematic file.
///
/// `voxel_coords`: (N, 3) int voxel positions [x, y, z]
/// `block_names`: N block IDs like "minecraft:stone"
pub fn write_litematic(
    output_path: &str,
    voxel_coords: &[[i32; 3]],
    block_names: &[&str],
    name: &str,
    author: &str,
    description: &str,
) -> std::io::Result<()> {
    if voxel_coords.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "No voxels"));
    }

    // Compute grid dimensions
    let mut sx: i32 = 0;
    let mut sy: i32 = 0;
    let mut sz: i32 = 0;
    for coord in voxel_coords {
        if coord[0] + 1 > sx { sx = coord[0] + 1; }
        if coord[1] + 1 > sy { sy = coord[1] + 1; }
        if coord[2] + 1 > sz { sz = coord[2] + 1; }
    }

    // Build palette: collect unique block names. Borrowing into the set keeps
    // this to one pass with no per-voxel String clone.
    let unique: std::collections::BTreeSet<&str> =
        block_names.iter().copied().collect();
    log::info!("Writing litematic: {} blocks, {} unique", voxel_coords.len(), unique.len());

    let mut palette: Vec<String> = unique.into_iter().map(str::to_string).collect();

    // Add minecraft:air as index 0
    if palette[0] != "minecraft:air" {
        palette.insert(0, "minecraft:air".to_string());
    }

    let palette_map: std::collections::HashMap<&str, usize> = palette.iter()
        .enumerate()
        .map(|(i, b)| (b.as_str(), i))
        .collect();

    // Build block state long array. Litematica uses a *continuous* bit-packed
    // long array (like Minecraft's pre-1.16 format): palette indices are packed
    // as an uninterrupted stream of `bits_per_block`-wide values, and a single
    // value may straddle the boundary between two consecutive longs. This is
    // NOT the per-long packing used by 1.16+ chunk sections — using that here
    // scrambles every block and inflates the file.
    let total = (sx * sy * sz) as usize;
    let bits_per_block = palette.len().max(2).next_power_of_two().ilog2().max(2) as usize;
    // ceil(total * bits / 64)
    let longs_needed = (total * bits_per_block).div_ceil(64);
    let mask: u64 = (1u64 << bits_per_block) - 1;

    let mut block_states = vec![0u64; longs_needed];

    // Set the value at a linear grid index using Litematica's LitematicaBitArray
    // packing. The array starts zeroed and each cell is written exactly once, so
    // a simple OR (no clear) is sufficient.
    let set_at = |arr: &mut [u64], index: usize, value: u64| {
        let start_offset = index * bits_per_block;
        let start_arr = start_offset >> 6; // / 64
        let end_arr = ((index + 1) * bits_per_block - 1) >> 6;
        let start_bit = start_offset & 63; // % 64

        arr[start_arr] |= (value & mask) << start_bit;
        if start_arr != end_arr {
            // The entry spans two longs; write the overflow high bits into the
            // next long. start_bit > 0 here, so (64 - start_bit) < 64.
            arr[end_arr] |= (value & mask) >> (64 - start_bit);
        }
    };

    // Place each voxel. Litematica linear index: y*width*length + z*width + x,
    // where width = sx (size.x) and length = sz (size.z).
    for (coord, bname) in voxel_coords.iter().zip(block_names) {
        let idx = (coord[0] + coord[2] * sx + coord[1] * sx * sz) as usize;
        if let Some(&pal_idx) = palette_map.get(bname) {
            set_at(&mut block_states, idx, pal_idx as u64);
        }
    }

    // Write NBT
    let mut w = NbtWriter::new();
    let data_version = data_version(); // see DEFAULT_DATA_VERSION
    let version = 6;                   // Litematica schematic version

    w.begin_compound_named("");

    // Metadata
    w.begin_compound_named("Metadata");
    w.write_long("TimeCreated", 0);
    w.write_long("TimeModified", 0);
    w.begin_compound_named("EnclosingSize");
    w.write_int("x", sx);
    w.write_int("y", sy);
    w.write_int("z", sz);
    w.end_compound();
    w.write_string_tag("Name", name);
    w.write_string_tag("Author", author);
    w.write_string_tag("Description", description);
    w.write_string_tag("Software", "SchemGen2");
    w.write_int("RegionCount", 1);
    w.write_int("TotalBlocks", voxel_coords.len() as i32);
    w.write_int("TotalVolume", total as i32);
    w.write_int_array("PreviewImageData", &[]);
    w.end_compound();

    // Regions
    w.begin_compound_named("Regions");

    // Main region
    w.begin_compound_named("main");

    // Position
    w.begin_compound_named("Position");
    w.write_int("x", 0);
    w.write_int("y", 0);
    w.write_int("z", 0);
    w.end_compound();

    // Size
    w.begin_compound_named("Size");
    w.write_int("x", sx);
    w.write_int("y", sy);
    w.write_int("z", sz);
    w.end_compound();

    // BlockStatePalette
    w.begin_list("BlockStatePalette", TAG_COMPOUND, palette.len() as i32);
    for block_name in &palette {
        w.begin_compound_in_list();
        w.write_string_tag("Name", block_name);
        w.end_list_compound();
    }
    // End list: no explicit end for lists in NBT (they just stop)

    // BlockStates
    w.write_long_array("BlockStates", &block_states);

    // PendingBlockTicks (empty list)
    w.begin_list("PendingBlockTicks", TAG_COMPOUND, 0);
    // empty

    // PendingFluidTicks (empty list)
    w.begin_list("PendingFluidTicks", TAG_COMPOUND, 0);
    // empty

    // TileEntities (empty list)
    w.begin_list("TileEntities", TAG_COMPOUND, 0);
    // empty

    // Entities (empty list)
    w.begin_list("Entities", TAG_COMPOUND, 0);
    // empty

    w.end_compound(); // end main region
    w.end_compound(); // end Regions

    // Version
    w.write_int("Version", version);
    w.write_int("SubVersion", 1);

    // MinecraftDataVersion
    w.write_int("MinecraftDataVersion", data_version);


    w.end_compound(); // end root

    // Compress with GZip
    let raw = w.finish();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&raw)?;
    let compressed = encoder.finish()?;

    std::fs::write(output_path, &compressed)?;
    log::info!("Litematic written: {} → {} bytes compressed", output_path, compressed.len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        assert!(write_litematic(
            "test_empty.litematic",
            &[],
            &[],
            "test", "test", "test",
        ).is_err());
    }

    #[test]
    fn test_single_block() {
        let coords = &[[0, 0, 0]];
        let names = &["minecraft:stone"];
        let res = write_litematic(
            "test_single.litematic",
            coords, names, "test", "test", "test",
        );
        assert!(res.is_ok());
        // Clean up
        let _ = std::fs::remove_file("test_single.litematic");
    }
}
