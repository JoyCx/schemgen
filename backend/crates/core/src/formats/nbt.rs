//! Named Binary Tag, Java Edition: big-endian, strings in modified UTF-8.
//!
//! [`NbtWriter`] streams tags straight into any [`Write`] — normally a gzip
//! encoder over a file — so a schematic is never held in memory twice. The
//! [`read`] half parses a whole tree back, for tests and inspection.

use std::io::{self, Read, Write};

pub const TAG_END: u8 = 0;
pub const TAG_BYTE: u8 = 1;
pub const TAG_SHORT: u8 = 2;
pub const TAG_INT: u8 = 3;
pub const TAG_LONG: u8 = 4;
pub const TAG_FLOAT: u8 = 5;
pub const TAG_DOUBLE: u8 = 6;
pub const TAG_BYTE_ARRAY: u8 = 7;
pub const TAG_STRING: u8 = 8;
pub const TAG_LIST: u8 = 9;
pub const TAG_COMPOUND: u8 = 10;
pub const TAG_INT_ARRAY: u8 = 11;
pub const TAG_LONG_ARRAY: u8 = 12;

/// Streaming NBT writer. Compound and list structure is the caller's to
/// balance: every `begin_compound` needs an `end`, and a list's elements must
/// match the type and count its header declared.
pub struct NbtWriter<W: Write> {
    out: W,
}

impl<W: Write> NbtWriter<W> {
    pub fn new(out: W) -> Self {
        Self { out }
    }

    pub fn into_inner(self) -> W {
        self.out
    }

    fn header(&mut self, tag: u8, name: &str) -> io::Result<()> {
        self.out.write_all(&[tag])?;
        self.raw_string(name)
    }

    /// A string payload: u16 byte length, then modified UTF-8.
    fn raw_string(&mut self, s: &str) -> io::Result<()> {
        let bytes = modified_utf8(s);
        let len = u16::try_from(bytes.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "NBT string longer than 65535 bytes",
            )
        })?;
        self.out.write_all(&len.to_be_bytes())?;
        self.out.write_all(&bytes)
    }

    fn len_prefix(&mut self, len: usize) -> io::Result<()> {
        let len = i32::try_from(len)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NBT array too long"))?;
        self.out.write_all(&len.to_be_bytes())
    }

    /// Open a named compound (the root compound has an empty name).
    pub fn begin_compound(&mut self, name: &str) -> io::Result<()> {
        self.header(TAG_COMPOUND, name)
    }

    /// Close the innermost open compound, or one compound element of a list.
    pub fn end(&mut self) -> io::Result<()> {
        self.out.write_all(&[TAG_END])
    }

    pub fn byte(&mut self, name: &str, v: i8) -> io::Result<()> {
        self.header(TAG_BYTE, name)?;
        self.out.write_all(&v.to_be_bytes())
    }

    pub fn short(&mut self, name: &str, v: i16) -> io::Result<()> {
        self.header(TAG_SHORT, name)?;
        self.out.write_all(&v.to_be_bytes())
    }

    pub fn int(&mut self, name: &str, v: i32) -> io::Result<()> {
        self.header(TAG_INT, name)?;
        self.out.write_all(&v.to_be_bytes())
    }

    pub fn long(&mut self, name: &str, v: i64) -> io::Result<()> {
        self.header(TAG_LONG, name)?;
        self.out.write_all(&v.to_be_bytes())
    }

    pub fn string(&mut self, name: &str, v: &str) -> io::Result<()> {
        self.header(TAG_STRING, name)?;
        self.raw_string(v)
    }

    pub fn byte_array(&mut self, name: &str, v: &[u8]) -> io::Result<()> {
        self.header(TAG_BYTE_ARRAY, name)?;
        self.len_prefix(v.len())?;
        self.out.write_all(v)
    }

    pub fn int_array(&mut self, name: &str, v: &[i32]) -> io::Result<()> {
        self.header(TAG_INT_ARRAY, name)?;
        self.len_prefix(v.len())?;
        for x in v {
            self.out.write_all(&x.to_be_bytes())?;
        }
        Ok(())
    }

    /// A long array given as raw 64-bit words — bit-packed block states are
    /// built unsigned, and NBT stores the same bits as signed.
    pub fn long_array(&mut self, name: &str, v: &[u64]) -> io::Result<()> {
        self.header(TAG_LONG_ARRAY, name)?;
        self.len_prefix(v.len())?;
        let mut chunk = Vec::with_capacity(8 * 1024);
        for block in v.chunks(1024) {
            chunk.clear();
            for x in block {
                chunk.extend_from_slice(&x.to_be_bytes());
            }
            self.out.write_all(&chunk)?;
        }
        Ok(())
    }

    /// Open a list of `len` elements of type `element`. Compound elements are
    /// written as their fields followed by [`Self::end`]; scalar elements with
    /// the `list_*` methods.
    pub fn begin_list(&mut self, name: &str, element: u8, len: usize) -> io::Result<()> {
        self.header(TAG_LIST, name)?;
        // An empty list conventionally declares TAG_End as its element type.
        let element = if len == 0 { TAG_END } else { element };
        self.out.write_all(&[element])?;
        self.len_prefix(len)
    }

    /// One element of a list of ints.
    pub fn list_int(&mut self, v: i32) -> io::Result<()> {
        self.out.write_all(&v.to_be_bytes())
    }

    /// One element of a list of strings.
    pub fn list_string(&mut self, v: &str) -> io::Result<()> {
        self.raw_string(v)
    }
}

/// Java's modified UTF-8: supplementary characters as surrogate pairs of
/// three bytes each, and U+0000 as the two bytes `C0 80`.
pub fn modified_utf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for unit in s.encode_utf16() {
        let u = unit as u32;
        match u {
            0x0001..=0x007F => out.push(u as u8),
            0x0000 | 0x0080..=0x07FF => {
                out.push(0xC0 | (u >> 6) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
            _ => {
                out.push(0xE0 | (u >> 12) as u8);
                out.push(0x80 | ((u >> 6) & 0x3F) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
        }
    }
    out
}

/// Inverse of [`modified_utf8`].
pub fn decode_modified_utf8(bytes: &[u8]) -> io::Result<String> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "invalid modified UTF-8");
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i] as u16;
        let cont = |k: usize| -> io::Result<u16> {
            match bytes.get(i + k) {
                Some(&c) if c & 0xC0 == 0x80 => Ok((c & 0x3F) as u16),
                _ => Err(bad()),
            }
        };
        if b < 0x80 {
            units.push(b);
            i += 1;
        } else if b & 0xE0 == 0xC0 {
            units.push(((b & 0x1F) << 6) | cont(1)?);
            i += 2;
        } else if b & 0xF0 == 0xE0 {
            units.push(((b & 0x0F) << 12) | (cont(1)? << 6) | cont(2)?);
            i += 3;
        } else {
            return Err(bad());
        }
    }
    String::from_utf16(&units).map_err(|_| bad())
}

// ── Reading ─────────────────────────────────────────────────────────────

/// A parsed tag. Compounds keep their fields in file order.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    /// A field of a compound.
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Follow a path of compound keys.
    pub fn at(&self, path: &[&str]) -> Option<&Tag> {
        path.iter().try_fold(self, |tag, key| tag.get(key))
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Tag::Byte(v) => Some(v as i64),
            Tag::Short(v) => Some(v as i64),
            Tag::Int(v) => Some(v as i64),
            Tag::Long(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Tag]> {
        match self {
            Tag::List(v) => Some(v),
            _ => None,
        }
    }

    pub fn keys(&self) -> Vec<&str> {
        match self {
            Tag::Compound(fields) => fields.iter().map(|(k, _)| k.as_str()).collect(),
            _ => Vec::new(),
        }
    }
}

/// Parse an uncompressed NBT document: the root tag's name and value.
pub fn read(mut input: impl Read) -> io::Result<(String, Tag)> {
    let tag = read_u8(&mut input)?;
    if tag != TAG_COMPOUND {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "NBT root is not a compound",
        ));
    }
    let name = read_string(&mut input)?;
    let value = read_payload(&mut input, tag, 0)?;
    Ok((name, value))
}

/// Parse a gzip-compressed NBT document — what `.litematic`, `.schem` and
/// structure `.nbt` files are.
pub fn read_gzip(input: impl Read) -> io::Result<(String, Tag)> {
    read(flate2::read::GzDecoder::new(input))
}

const MAX_DEPTH: usize = 512;

fn read_payload(input: &mut impl Read, tag: u8, depth: usize) -> io::Result<Tag> {
    if depth > MAX_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "NBT nested too deep",
        ));
    }
    Ok(match tag {
        TAG_BYTE => Tag::Byte(read_u8(input)? as i8),
        TAG_SHORT => Tag::Short(i16::from_be_bytes(read_n(input)?)),
        TAG_INT => Tag::Int(i32::from_be_bytes(read_n(input)?)),
        TAG_LONG => Tag::Long(i64::from_be_bytes(read_n(input)?)),
        TAG_FLOAT => Tag::Float(f32::from_be_bytes(read_n(input)?)),
        TAG_DOUBLE => Tag::Double(f64::from_be_bytes(read_n(input)?)),
        TAG_BYTE_ARRAY => {
            let len = read_len(input)?;
            let mut v = vec![0u8; len];
            input.read_exact(&mut v)?;
            Tag::ByteArray(v)
        }
        TAG_STRING => Tag::String(read_string(input)?),
        TAG_LIST => {
            let element = read_u8(input)?;
            let len = read_len(input)?;
            let mut items = Vec::with_capacity(len.min(1 << 16));
            for _ in 0..len {
                items.push(read_payload(input, element, depth + 1)?);
            }
            Tag::List(items)
        }
        TAG_COMPOUND => {
            let mut fields = Vec::new();
            loop {
                let t = read_u8(input)?;
                if t == TAG_END {
                    break;
                }
                let name = read_string(input)?;
                fields.push((name, read_payload(input, t, depth + 1)?));
            }
            Tag::Compound(fields)
        }
        TAG_INT_ARRAY => {
            let len = read_len(input)?;
            let mut v = Vec::with_capacity(len.min(1 << 20));
            for _ in 0..len {
                v.push(i32::from_be_bytes(read_n(input)?));
            }
            Tag::IntArray(v)
        }
        TAG_LONG_ARRAY => {
            let len = read_len(input)?;
            let mut v = Vec::with_capacity(len.min(1 << 20));
            for _ in 0..len {
                v.push(i64::from_be_bytes(read_n(input)?));
            }
            Tag::LongArray(v)
        }
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown NBT tag type {other}"),
            ))
        }
    })
}

fn read_u8(input: &mut impl Read) -> io::Result<u8> {
    Ok(read_n::<1>(input)?[0])
}

fn read_n<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut buf = [0u8; N];
    input.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_len(input: &mut impl Read) -> io::Result<usize> {
    let len = i32::from_be_bytes(read_n(input)?);
    usize::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "negative NBT length"))
}

fn read_string(input: &mut impl Read) -> io::Result<String> {
    let len = u16::from_be_bytes(read_n(input)?) as usize;
    let mut bytes = vec![0u8; len];
    input.read_exact(&mut bytes)?;
    decode_modified_utf8(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_utf8_round_trips() {
        for s in [
            "",
            "castle",
            "Größe",
            "日本語",
            "emoji 🧱 ok",
            "nul\u{0}here",
        ] {
            let bytes = modified_utf8(s);
            assert!(!bytes.contains(&0), "no raw NUL bytes in {s:?}");
            assert_eq!(decode_modified_utf8(&bytes).unwrap(), s);
        }
        // Supplementary characters are two three-byte surrogates, not four bytes.
        assert_eq!(modified_utf8("🧱").len(), 6);
    }

    #[test]
    fn written_tags_read_back() {
        let mut w = NbtWriter::new(Vec::new());
        w.begin_compound("").unwrap();
        w.byte("b", -3).unwrap();
        w.short("s", 300).unwrap();
        w.int("i", -70000).unwrap();
        w.long("l", 1 << 40).unwrap();
        w.string("name", "château").unwrap();
        w.byte_array("bytes", &[1, 2, 255]).unwrap();
        w.int_array("ints", &[1, -1]).unwrap();
        w.long_array("longs", &[u64::MAX, 7]).unwrap();
        w.begin_list("pos", TAG_INT, 3).unwrap();
        for v in [4, 5, 6] {
            w.list_int(v).unwrap();
        }
        w.begin_list("items", TAG_COMPOUND, 2).unwrap();
        for id in ["a", "b"] {
            w.string("id", id).unwrap();
            w.end().unwrap();
        }
        w.begin_list("empty", TAG_COMPOUND, 0).unwrap();
        w.begin_compound("nested").unwrap();
        w.int("x", 1).unwrap();
        w.end().unwrap();
        w.end().unwrap();

        let bytes = w.into_inner();
        let (name, root) = read(bytes.as_slice()).unwrap();
        assert_eq!(name, "");
        assert_eq!(root.get("b"), Some(&Tag::Byte(-3)));
        assert_eq!(root.get("s").and_then(Tag::as_i64), Some(300));
        assert_eq!(root.get("i").and_then(Tag::as_i64), Some(-70000));
        assert_eq!(root.get("l").and_then(Tag::as_i64), Some(1 << 40));
        assert_eq!(root.get("name").and_then(Tag::as_str), Some("château"));
        assert_eq!(root.get("bytes"), Some(&Tag::ByteArray(vec![1, 2, 255])));
        assert_eq!(root.get("ints"), Some(&Tag::IntArray(vec![1, -1])));
        assert_eq!(root.get("longs"), Some(&Tag::LongArray(vec![-1, 7])));
        assert_eq!(
            root.get("pos"),
            Some(&Tag::List(vec![Tag::Int(4), Tag::Int(5), Tag::Int(6)]))
        );
        let items = root.get("items").and_then(Tag::as_list).unwrap();
        assert_eq!(items[1].get("id").and_then(Tag::as_str), Some("b"));
        assert_eq!(root.get("empty"), Some(&Tag::List(vec![])));
        assert_eq!(root.at(&["nested", "x"]), Some(&Tag::Int(1)));
        assert_eq!(
            root.keys(),
            [
                "b", "s", "i", "l", "name", "bytes", "ints", "longs", "pos", "items", "empty",
                "nested"
            ]
        );
    }

    #[test]
    fn overlong_strings_are_rejected() {
        let mut w = NbtWriter::new(Vec::new());
        assert!(w.string("x", &"a".repeat(70_000)).is_err());
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        let mut w = NbtWriter::new(Vec::new());
        w.begin_compound("").unwrap();
        w.string("name", "castle").unwrap();
        w.end().unwrap();
        let bytes = w.into_inner();
        for cut in 0..bytes.len() {
            assert!(read(&bytes[..cut]).is_err(), "cut at {cut}");
        }
    }
}
