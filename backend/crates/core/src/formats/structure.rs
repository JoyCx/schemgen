//! Vanilla structure files (`.nbt`), what structure blocks and
//! `/place template` load — no mod needed.
//!
//! ```text
//! { DataVersion, size: [x, y, z], palette: [{Name}], blocks: [{pos: [x, y, z], state}], entities: [] }
//! ```
//!
//! Only placed blocks are listed. A position left out is a structure void:
//! placing the structure keeps whatever was there, which is what a hollow
//! statue wants. Structure blocks load at most 48 × 48 × 48; `/place template`
//! takes larger ones.

use std::io::{self, Write};
use std::path::Path;

use super::nbt::{NbtWriter, TAG_COMPOUND, TAG_INT};
use super::{write_file, Metadata};
use crate::error::Result;
use crate::grid::BlockGrid;
use crate::targets::Target;

/// Largest structure a structure block can load, per axis.
pub const STRUCTURE_BLOCK_LIMIT: u32 = 48;

pub fn write(path: &Path, grid: &BlockGrid, meta: &Metadata, target: &Target) -> Result<()> {
    write_file(path, |out| encode(out, grid, meta, target))
}

pub fn encode(
    out: impl Write,
    grid: &BlockGrid,
    _meta: &Metadata,
    target: &Target,
) -> io::Result<()> {
    if grid.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no blocks to write",
        ));
    }
    let gz = flate2::write::GzEncoder::new(out, flate2::Compression::default());
    let mut w = NbtWriter::new(io::BufWriter::with_capacity(1 << 16, gz));

    w.begin_compound("")?;
    w.int("DataVersion", target.data_version)?;
    w.begin_list("size", TAG_INT, 3)?;
    for d in grid.size {
        w.list_int(d as i32)?;
    }

    w.begin_list("palette", TAG_COMPOUND, grid.names.len())?;
    for name in &grid.names {
        w.string("Name", name)?;
        w.end()?;
    }

    // Layer by layer, row by row, as the game itself saves them.
    let mut order: Vec<usize> = (0..grid.len()).collect();
    order.sort_unstable_by_key(|&i| {
        let [x, y, z] = grid.coords[i];
        (y, z, x)
    });
    w.begin_list("blocks", TAG_COMPOUND, grid.len())?;
    for i in order {
        w.begin_list("pos", TAG_INT, 3)?;
        for v in grid.coords[i] {
            w.list_int(v)?;
        }
        w.int("state", grid.blocks[i] as i32)?;
        w.end()?;
    }
    w.begin_list("entities", TAG_COMPOUND, 0)?;
    w.end()?;

    let buffered = w.into_inner();
    let gz = buffered.into_inner().map_err(|e| e.into_error())?;
    gz.finish()?.flush()
}

#[cfg(test)]
mod tests {
    use super::super::nbt::{read_gzip, Tag};
    use super::*;

    #[test]
    fn lists_placed_blocks_only() {
        let g = BlockGrid::from_names(
            vec![[2, 0, 1], [0, 1, 0], [1, 0, 0]],
            ["minecraft:stone", "minecraft:dirt", "minecraft:stone"],
            [0.0; 3],
            1.0,
        );
        let mut bytes = Vec::new();
        let meta = Metadata::new("s");
        encode(&mut bytes, &g, &meta, &Target::named("1.19.4").unwrap()).unwrap();
        let (_, root) = read_gzip(bytes.as_slice()).unwrap();
        assert_eq!(root.get("DataVersion"), Some(&Tag::Int(3337)));
        assert_eq!(
            root.get("size"),
            Some(&Tag::List(vec![Tag::Int(3), Tag::Int(2), Tag::Int(2)]))
        );
        let palette: Vec<&str> = root
            .get("palette")
            .and_then(Tag::as_list)
            .unwrap()
            .iter()
            .map(|p| p.get("Name").and_then(Tag::as_str).unwrap())
            .collect();
        assert_eq!(palette, ["minecraft:dirt", "minecraft:stone"]);
        let blocks = root.get("blocks").and_then(Tag::as_list).unwrap();
        assert_eq!(blocks.len(), 3);
        // y first: the dirt at y=1 comes last.
        let last = &blocks[2];
        assert_eq!(
            last.get("pos"),
            Some(&Tag::List(vec![Tag::Int(0), Tag::Int(1), Tag::Int(0)]))
        );
        assert_eq!(last.get("state"), Some(&Tag::Int(0)));
        assert_eq!(root.get("entities"), Some(&Tag::List(vec![])));
    }
}
