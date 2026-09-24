//! Litematica `.litematic`: gzip-compressed NBT with one region.
//!
//! ```text
//! {
//!   Version, SubVersion, MinecraftDataVersion,
//!   Metadata: { Name, Author, Description, TimeCreated, TimeModified,
//!               EnclosingSize, RegionCount, TotalBlocks, TotalVolume },
//!   Regions: { <name>: { Position, Size, BlockStatePalette, BlockStates,
//!                        PendingBlockTicks, PendingFluidTicks,
//!                        TileEntities, Entities } }
//! }
//! ```
//!
//! `BlockStates` bit-packs palette indices as one continuous stream — a value
//! may straddle two longs — which is Litematica's `LitematicaBitArray`, not the
//! padded per-long packing of 1.16+ chunk sections.

use std::io::{self, Write};
use std::path::Path;

use super::nbt::{NbtWriter, TAG_COMPOUND};
use super::{write_file, Metadata};
use crate::error::Result;
use crate::grid::BlockGrid;
use crate::targets::Target;

/// Litematica's `SubVersion`, bumped with its sleeping-entity position fix.
const SUB_VERSION: i32 = 1;

const AIR: &str = "minecraft:air";

/// Write `grid` to `path` as a `.litematic` for `target`.
pub fn write(path: &Path, grid: &BlockGrid, meta: &Metadata, target: &Target) -> Result<()> {
    write_file(path, |out| encode(out, grid, meta, target))
}

/// Encode `grid` as gzip-compressed Litematica NBT into `out`.
pub fn encode(
    out: impl Write,
    grid: &BlockGrid,
    meta: &Metadata,
    target: &Target,
) -> io::Result<()> {
    if grid.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no blocks to write",
        ));
    }
    let [sx, sy, sz] = grid.size.map(|d| d as i32);
    let volume = grid.volume();

    // Palette index 0 is air, so every cell not written below reads as air.
    let mut palette: Vec<&str> = vec![AIR];
    let mut to_palette: Vec<u64> = Vec::with_capacity(grid.names.len());
    for name in &grid.names {
        if name == AIR {
            to_palette.push(0);
        } else {
            to_palette.push(palette.len() as u64);
            palette.push(name);
        }
    }
    let placed = grid
        .names
        .iter()
        .zip(grid.counts())
        .filter(|(name, _)| name.as_str() != AIR)
        .map(|(_, count)| count)
        .sum::<u64>();

    let block_states = pack_block_states(grid, &to_palette, palette.len());

    let gz = flate2::write::GzEncoder::new(out, flate2::Compression::default());
    let mut w = NbtWriter::new(io::BufWriter::with_capacity(1 << 16, gz));

    w.begin_compound("")?;
    w.int("Version", target.schematic_version)?;
    w.int("SubVersion", SUB_VERSION)?;
    w.int("MinecraftDataVersion", target.data_version)?;

    w.begin_compound("Metadata")?;
    w.string("Name", &meta.name)?;
    w.string("Author", &meta.author)?;
    w.string("Description", &meta.description)?;
    w.long("TimeCreated", meta.time_ms)?;
    w.long("TimeModified", meta.time_ms)?;
    w.begin_compound("EnclosingSize")?;
    w.int("x", sx)?;
    w.int("y", sy)?;
    w.int("z", sz)?;
    w.end()?;
    w.int("RegionCount", 1)?;
    w.int("TotalBlocks", clamp_i32(placed))?;
    w.int("TotalVolume", clamp_i32(volume))?;
    w.end()?;

    w.begin_compound("Regions")?;
    w.begin_compound(region_name(&meta.name))?;
    w.begin_compound("Position")?;
    w.int("x", 0)?;
    w.int("y", 0)?;
    w.int("z", 0)?;
    w.end()?;
    w.begin_compound("Size")?;
    w.int("x", sx)?;
    w.int("y", sy)?;
    w.int("z", sz)?;
    w.end()?;
    w.begin_list("BlockStatePalette", TAG_COMPOUND, palette.len())?;
    for name in &palette {
        w.string("Name", name)?;
        w.end()?;
    }
    w.long_array("BlockStates", &block_states)?;
    for empty in [
        "PendingBlockTicks",
        "PendingFluidTicks",
        "TileEntities",
        "Entities",
    ] {
        w.begin_list(empty, TAG_COMPOUND, 0)?;
    }
    w.end()?; // region
    w.end()?; // Regions
    w.end()?; // root

    let buffered = w.into_inner();
    let gz = buffered.into_inner().map_err(|e| e.into_error())?;
    gz.finish()?.flush()
}

/// Litematica shows the region name in its placement list; the schematic
/// name reads better there than a fixed placeholder.
fn region_name(name: &str) -> &str {
    if name.trim().is_empty() {
        "main"
    } else {
        name
    }
}

fn clamp_i32(v: u64) -> i32 {
    v.min(i32::MAX as u64) as i32
}

/// Bits per palette index: at least 2, as Litematica requires.
fn bits_for(palette_len: usize) -> usize {
    (usize::BITS - (palette_len.max(2) - 1).leading_zeros()).max(2) as usize
}

/// Pack every cell's palette index into Litematica's continuous bit stream.
/// Cells are ordered x fastest, then z, then y.
fn pack_block_states(grid: &BlockGrid, to_palette: &[u64], palette_len: usize) -> Vec<u64> {
    let [sx, _, sz] = grid.size.map(|d| d as usize);
    let bits = bits_for(palette_len);
    let mask = (1u64 << bits) - 1;
    let total = grid.volume() as usize;
    let mut words = vec![0u64; (total * bits).div_ceil(64)];

    for (coord, &block) in grid.coords.iter().zip(&grid.blocks) {
        let [x, y, z] = coord.map(|c| c as usize);
        let index = x + z * sx + y * sx * sz;
        let value = to_palette[block as usize] & mask;
        let start = index * bits;
        let (word, bit) = (start / 64, start % 64);
        // Cells start out as air (0) and are written once, so OR is enough.
        words[word] |= value << bit;
        if bit + bits > 64 {
            words[word + 1] |= value >> (64 - bit);
        }
    }
    words
}

#[cfg(test)]
mod tests {
    use super::super::nbt::{read_gzip, Tag};
    use super::*;

    fn meta() -> Metadata {
        Metadata {
            name: "test".into(),
            author: "SchemGen2".into(),
            description: "unit test".into(),
            time_ms: 1_700_000_000_000,
        }
    }

    /// Read cell `index` back out of a packed stream.
    fn unpack(words: &[i64], bits: usize, index: usize) -> u64 {
        let start = index * bits;
        let (word, bit) = (start / 64, start % 64);
        let mut v = (words[word] as u64) >> bit;
        if bit + bits > 64 {
            v |= (words[word + 1] as u64) << (64 - bit);
        }
        v & ((1u64 << bits) - 1)
    }

    #[test]
    fn bit_width_follows_palette_size() {
        assert_eq!(bits_for(1), 2);
        assert_eq!(bits_for(2), 2);
        assert_eq!(bits_for(4), 2);
        assert_eq!(bits_for(5), 3);
        assert_eq!(bits_for(17), 5);
        assert_eq!(bits_for(182), 8);
        assert_eq!(bits_for(257), 9);
    }

    #[test]
    fn empty_grid_is_an_error() {
        let g = BlockGrid::from_names(vec![], std::iter::empty(), [0.0; 3], 1.0);
        assert!(encode(Vec::new(), &g, &meta(), &Target::default()).is_err());
    }

    #[test]
    fn round_trips_through_the_reader() {
        // 5 block types plus air → 3 bits, so values straddle long boundaries.
        let names = [
            "minecraft:stone",
            "minecraft:dirt",
            "minecraft:glass",
            "minecraft:tuff",
            "minecraft:clay",
        ];
        let mut coords = Vec::new();
        let mut per_block = Vec::new();
        for x in 0..7 {
            for y in 0..5 {
                for z in 0..6 {
                    if (x + 2 * y + 3 * z) % 4 != 0 {
                        coords.push([x, y, z]);
                        per_block.push(names[((x * 7 + y * 3 + z) % 5) as usize]);
                    }
                }
            }
        }
        let grid = BlockGrid::from_names(coords.clone(), per_block.iter().copied(), [0.0; 3], 1.0);
        let target = Target::named("1.20.4").unwrap();
        let mut bytes = Vec::new();
        encode(&mut bytes, &grid, &meta(), &target).unwrap();

        let (_, root) = read_gzip(bytes.as_slice()).unwrap();
        assert_eq!(root.get("Version"), Some(&Tag::Int(6)));
        assert_eq!(root.get("SubVersion"), Some(&Tag::Int(1)));
        assert_eq!(root.get("MinecraftDataVersion"), Some(&Tag::Int(3700)));
        assert_eq!(
            root.at(&["Metadata", "TotalBlocks"]),
            Some(&Tag::Int(coords.len() as i32))
        );
        assert_eq!(
            root.at(&["Metadata", "TotalVolume"]),
            Some(&Tag::Int(7 * 5 * 6))
        );
        assert_eq!(
            root.at(&["Metadata", "TimeCreated"]),
            Some(&Tag::Long(1_700_000_000_000))
        );
        assert_eq!(
            root.at(&["Metadata", "EnclosingSize", "x"]),
            Some(&Tag::Int(7))
        );

        let region = root
            .at(&["Regions", "test"])
            .expect("region named after the schematic");
        let palette: Vec<&str> = region
            .get("BlockStatePalette")
            .and_then(Tag::as_list)
            .unwrap()
            .iter()
            .map(|t| t.get("Name").and_then(Tag::as_str).unwrap())
            .collect();
        assert_eq!(palette[0], "minecraft:air");
        let Some(Tag::LongArray(words)) = region.get("BlockStates") else {
            panic!("BlockStates missing")
        };
        let bits = bits_for(palette.len());
        assert_eq!(bits, 3);
        assert_eq!(words.len(), (7 * 5 * 6 * bits).div_ceil(64));

        let mut placed = 0;
        for x in 0..7usize {
            for y in 0..5usize {
                for z in 0..6usize {
                    let got = palette[unpack(words, bits, x + z * 7 + y * 7 * 6) as usize];
                    let want = coords
                        .iter()
                        .position(|&c| c == [x as i32, y as i32, z as i32])
                        .map(|i| per_block[i])
                        .unwrap_or("minecraft:air");
                    assert_eq!(got, want, "cell {x},{y},{z}");
                    placed += (got != "minecraft:air") as usize;
                }
            }
        }
        assert_eq!(placed, coords.len());
    }

    #[test]
    fn version_7_from_1_21() {
        let g = BlockGrid::from_names(vec![[0, 0, 0]], ["minecraft:stone"], [0.0; 3], 1.0);
        let mut bytes = Vec::new();
        encode(&mut bytes, &g, &meta(), &Target::named("1.21.11").unwrap()).unwrap();
        let (_, root) = read_gzip(bytes.as_slice()).unwrap();
        assert_eq!(root.get("Version"), Some(&Tag::Int(7)));
        assert_eq!(root.get("MinecraftDataVersion"), Some(&Tag::Int(4671)));
    }

    #[test]
    fn writes_atomically_to_disk() {
        let dir = std::env::temp_dir().join(format!("schemgen_lm_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("one.litematic");
        let g = BlockGrid::from_names(vec![[0, 0, 0]], ["minecraft:stone"], [0.0; 3], 1.0);
        write(&path, &g, &meta(), &Target::default()).unwrap();
        let (_, root) = read_gzip(std::fs::File::open(&path).unwrap()).unwrap();
        assert!(root.get("Regions").is_some());
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(leftovers.len(), 1, "no temporary files left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
