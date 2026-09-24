//! Sponge schematics (`.schem`), the format WorldEdit and FAWE read and
//! write — and Litematica can import.
//!
//! * **Version 2** keeps everything at the root: `Width`/`Height`/`Length`
//!   (shorts), `Palette` (block ID → index) and `BlockData`, the indices as
//!   varints, x fastest, then z, then y. WorldEdit 7.x reads it everywhere.
//! * **Version 3** nests the same data in a `Schematic` compound, with the
//!   blocks under `Blocks { Palette, Data, BlockEntities }`. WorldEdit writes
//!   it from 7.3 on.
//!
//! Both are dense: every cell of the box is written, air included, so the
//! palette always has `minecraft:air` (index 0).

use std::io::{self, Write};
use std::path::Path;

use super::nbt::{NbtWriter, TAG_COMPOUND, TAG_STRING};
use super::{write_file, Metadata};
use crate::error::Result;
use crate::grid::BlockGrid;
use crate::targets::Target;

const AIR: &str = "minecraft:air";

pub fn write_v2(path: &Path, grid: &BlockGrid, meta: &Metadata, target: &Target) -> Result<()> {
    write_file(path, |out| encode(out, grid, meta, target, 2))
}

pub fn write_v3(path: &Path, grid: &BlockGrid, meta: &Metadata, target: &Target) -> Result<()> {
    write_file(path, |out| encode(out, grid, meta, target, 3))
}

/// Encode `grid` as gzip-compressed Sponge NBT, version 2 or 3.
pub fn encode(
    out: impl Write,
    grid: &BlockGrid,
    meta: &Metadata,
    target: &Target,
    version: i32,
) -> io::Result<()> {
    if grid.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no blocks to write",
        ));
    }
    let [sx, sy, sz] = grid.size;
    if [sx, sy, sz].iter().any(|&d| d > u16::MAX as u32) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Sponge schematics are at most 65535 blocks on a side",
        ));
    }
    let (palette, cells) = cells(grid);

    let gz = flate2::write::GzEncoder::new(out, flate2::Compression::default());
    let mut w = NbtWriter::new(io::BufWriter::with_capacity(1 << 16, gz));
    if version >= 3 {
        w.begin_compound("")?;
        w.begin_compound("Schematic")?;
    } else {
        w.begin_compound("Schematic")?;
    }
    w.int("Version", version)?;
    w.int("DataVersion", target.data_version)?;

    w.begin_compound("Metadata")?;
    w.string("Name", &meta.name)?;
    w.string("Author", &meta.author)?;
    w.long("Date", meta.time_ms)?;
    if version >= 3 {
        w.begin_list("RequiredMods", TAG_STRING, 0)?;
    }
    w.end()?;

    // Sizes are unsigned shorts stored in NBT's signed ones.
    w.short("Width", sx as u16 as i16)?;
    w.short("Height", sy as u16 as i16)?;
    w.short("Length", sz as u16 as i16)?;
    w.int_array("Offset", &[0, 0, 0])?;

    let write_palette = |w: &mut NbtWriter<_>| -> io::Result<()> {
        w.begin_compound("Palette")?;
        for (i, name) in palette.iter().enumerate() {
            w.int(name, i as i32)?;
        }
        w.end()
    };
    if version >= 3 {
        w.begin_compound("Blocks")?;
        write_palette(&mut w)?;
        write_varints(&mut w, "Data", grid, &cells)?;
        w.begin_list("BlockEntities", TAG_COMPOUND, 0)?;
        w.end()?; // Blocks
                  // Optional in the spec, but some readers index it unconditionally.
        w.begin_list("Entities", TAG_COMPOUND, 0)?;
        w.end()?; // Schematic
        w.end()?; // root
    } else {
        w.int("PaletteMax", palette.len() as i32)?;
        write_palette(&mut w)?;
        write_varints(&mut w, "BlockData", grid, &cells)?;
        w.begin_list("BlockEntities", TAG_COMPOUND, 0)?;
        w.begin_list("Entities", TAG_COMPOUND, 0)?;
        w.end()?; // root
    }

    let buffered = w.into_inner();
    let gz = buffered.into_inner().map_err(|e| e.into_error())?;
    gz.finish()?.flush()
}

/// The file palette (air first) and every placed block as
/// `(cell index, palette index)`, sorted by cell.
fn cells(grid: &BlockGrid) -> (Vec<&str>, Vec<(u64, u32)>) {
    let mut palette: Vec<&str> = vec![AIR];
    let mut to_palette = Vec::with_capacity(grid.names.len());
    for name in &grid.names {
        if name == AIR {
            to_palette.push(0u32);
        } else {
            to_palette.push(palette.len() as u32);
            palette.push(name);
        }
    }
    let [sx, _, sz] = grid.size.map(|d| d as u64);
    let mut cells: Vec<(u64, u32)> = grid
        .coords
        .iter()
        .zip(&grid.blocks)
        .map(|(c, &b)| {
            let [x, y, z] = c.map(|v| v as u64);
            (x + z * sx + y * sx * sz, to_palette[b as usize])
        })
        .collect();
    cells.sort_unstable_by_key(|&(cell, _)| cell);
    (palette, cells)
}

/// Every cell's palette index as a varint, air (0) where nothing is placed —
/// streamed, with the length worked out first, so the box is never held in
/// memory whole.
fn write_varints<W: Write>(
    w: &mut NbtWriter<W>,
    name: &str,
    grid: &BlockGrid,
    cells: &[(u64, u32)],
) -> io::Result<()> {
    let volume = grid.volume();
    let placed_bytes: u64 = cells.iter().map(|&(_, v)| varint_len(v) as u64).sum();
    let len = volume - cells.len() as u64 + placed_bytes;
    let len = i32::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "schematic too large"))?;
    w.begin_byte_array(name, len)?;

    let mut buf = Vec::with_capacity(1 << 16);
    let mut next = cells.iter().peekable();
    for cell in 0..volume {
        match next.peek() {
            Some(&&(at, value)) if at == cell => {
                push_varint(&mut buf, value);
                next.next();
            }
            _ => buf.push(0),
        }
        if buf.len() >= (1 << 16) - 8 {
            w.raw(&buf)?;
            buf.clear();
        }
    }
    w.raw(&buf)
}

fn varint_len(mut v: u32) -> usize {
    let mut n = 1;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

fn push_varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

#[cfg(test)]
mod tests {
    use super::super::nbt::{read_gzip, Tag};
    use super::*;

    fn meta() -> Metadata {
        Metadata {
            name: "statue".into(),
            author: "SchemGen2".into(),
            description: String::new(),
            time_ms: 1_700_000_000_000,
        }
    }

    /// 130 block types so some palette indices need two varint bytes.
    fn grid() -> BlockGrid {
        let names: Vec<String> = (0..130).map(|i| format!("minecraft:b{i:03}")).collect();
        let mut coords = Vec::new();
        let mut per_block = Vec::new();
        for i in 0..400i32 {
            let c = [i % 9, (i / 9) % 7, i / 63];
            if (c[0] + c[1] + c[2]) % 3 != 0 {
                coords.push(c);
                per_block.push(names[(i as usize * 7) % 130].as_str());
            }
        }
        BlockGrid::from_names(coords, per_block, [0.0; 3], 1.0)
    }

    fn decode_varints(bytes: &[u8]) -> Vec<u32> {
        let mut out = Vec::new();
        let (mut value, mut shift) = (0u32, 0);
        for &b in bytes {
            value |= ((b & 0x7F) as u32) << shift;
            if b & 0x80 == 0 {
                out.push(value);
                value = 0;
                shift = 0;
            } else {
                shift += 7;
            }
        }
        out
    }

    fn check(palette_tag: &Tag, data: &[u8], g: &BlockGrid) {
        let Tag::Compound(entries) = palette_tag else {
            panic!("palette")
        };
        let mut by_index = vec![""; entries.len()];
        for (name, idx) in entries {
            by_index[idx.as_i64().unwrap() as usize] = name.as_str();
        }
        assert_eq!(by_index[0], "minecraft:air");
        let values = decode_varints(data);
        assert_eq!(values.len() as u64, g.volume());
        let [sx, _, sz] = g.size.map(|d| d as usize);
        let mut placed = 0;
        for (i, c) in g.coords.iter().enumerate() {
            let [x, y, z] = c.map(|v| v as usize);
            assert_eq!(
                by_index[values[x + z * sx + y * sx * sz] as usize],
                g.name_at(i)
            );
            placed += 1;
        }
        let non_air = values.iter().filter(|&&v| v != 0).count();
        assert_eq!(non_air, placed);
    }

    #[test]
    fn v2_round_trips() {
        let g = grid();
        let mut bytes = Vec::new();
        encode(
            &mut bytes,
            &g,
            &meta(),
            &Target::named("1.20.4").unwrap(),
            2,
        )
        .unwrap();
        let (name, root) = read_gzip(bytes.as_slice()).unwrap();
        assert_eq!(name, "Schematic");
        assert_eq!(root.get("Version"), Some(&Tag::Int(2)));
        assert_eq!(root.get("DataVersion"), Some(&Tag::Int(3700)));
        assert_eq!(root.get("Width"), Some(&Tag::Short(g.size[0] as i16)));
        assert_eq!(
            root.get("PaletteMax"),
            Some(&Tag::Int(g.names.len() as i32 + 1))
        );
        assert_eq!(
            root.at(&["Metadata", "Name"]).and_then(Tag::as_str),
            Some("statue")
        );
        let Some(Tag::ByteArray(data)) = root.get("BlockData") else {
            panic!("BlockData")
        };
        check(root.get("Palette").unwrap(), data, &g);
    }

    #[test]
    fn v3_nests_under_schematic() {
        let g = grid();
        let mut bytes = Vec::new();
        encode(&mut bytes, &g, &meta(), &Target::default(), 3).unwrap();
        let (name, root) = read_gzip(bytes.as_slice()).unwrap();
        assert_eq!(name, "");
        let s = root.get("Schematic").expect("Schematic compound");
        assert_eq!(s.get("Version"), Some(&Tag::Int(3)));
        assert_eq!(s.get("DataVersion"), Some(&Tag::Int(4440)));
        assert!(s.at(&["Metadata", "RequiredMods"]).is_some());
        let Some(Tag::ByteArray(data)) = s.at(&["Blocks", "Data"]) else {
            panic!("Data")
        };
        check(s.at(&["Blocks", "Palette"]).unwrap(), data, &g);
    }

    #[test]
    fn varints_encode_like_protobuf() {
        let mut v = Vec::new();
        for x in [0, 1, 127, 128, 300, 16384] {
            push_varint(&mut v, x);
            assert_eq!(varint_len(x), {
                let mut one = Vec::new();
                push_varint(&mut one, x);
                one.len()
            });
        }
        assert_eq!(v, [0, 1, 127, 0x80, 1, 0xAC, 2, 0x80, 0x80, 1]);
        assert_eq!(decode_varints(&v), [0, 1, 127, 128, 300, 16384]);
    }
}
