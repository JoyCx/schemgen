//! Block textures for the web UI's preview, from the user's own Minecraft.
//!
//! Mojang's textures cannot ship with SchemGen2, but nearly everyone who
//! wants a schematic has the game installed. This finds a client jar the
//! launchers keep — or the jar, resource pack or texture folder named with
//! `--textures` — and serves `assets/minecraft/textures/block/<name>.png`
//! from it. The UI falls back to a public copy, then to flat palette colors,
//! for a texture that is not here.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use actix_web::{get, web, HttpResponse};
use serde_json::json;

use crate::savedir::launcher_roots;
use crate::shared::App;

const BLOCK_DIR: &str = "assets/minecraft/textures/block/";

/// Where textures come from.
enum Source {
    /// A client jar or resource-pack zip.
    Zip {
        path: PathBuf,
        version: Option<String>,
        entries: HashMap<String, ZipEntry>,
    },
    /// A folder of `<name>.png` files.
    Folder { path: PathBuf },
}

/// The block textures this server can hand out.
pub struct Textures {
    source: Option<Source>,
    cache: Mutex<HashMap<String, Option<Arc<Vec<u8>>>>>,
}

impl Textures {
    /// Textures from `explicit` (a jar, zip, resource pack or folder), or else
    /// from the newest release client jar a launcher on this machine keeps.
    pub fn discover(explicit: Option<&Path>) -> Self {
        let source = match explicit {
            Some(path) => open(path, None).map_err(|e| {
                log::warn!("--textures {}: {e}", path.display());
            }),
            None => newest_client_jar()
                .ok_or(())
                .and_then(|(version, jar)| open(&jar, Some(version)).map_err(|_| ())),
        }
        .ok();
        match &source {
            Some(Source::Zip { path, .. }) | Some(Source::Folder { path }) => {
                log::info!("Block textures for the preview: {}", path.display())
            }
            None => {
                log::info!("No Minecraft install found; the preview's textures come from the web")
            }
        }
        Textures {
            source,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// No local textures at all.
    pub fn none() -> Self {
        Textures {
            source: None,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The PNG of block texture `name` (`stone`, `oak_log_top`), if there is one.
    pub fn get(&self, name: &str) -> Option<Arc<Vec<u8>>> {
        if !valid_name(name) {
            return None;
        }
        if let Some(hit) = self.cache.lock().expect("texture cache").get(name) {
            return hit.clone();
        }
        let loaded = match self.source.as_ref()? {
            Source::Zip { path, entries, .. } => entries
                .get(name)
                .and_then(|entry| read_entry(path, entry).ok()),
            Source::Folder { path } => std::fs::read(path.join(format!("{name}.png"))).ok(),
        }
        .map(Arc::new);
        self.cache
            .lock()
            .expect("texture cache")
            .insert(name.to_string(), loaded.clone());
        loaded
    }

    /// What the UI shows about where the textures come from.
    pub fn describe(&self) -> serde_json::Value {
        match &self.source {
            Some(Source::Zip {
                path,
                version,
                entries,
            }) => json!({
                "source": "jar",
                "path": path,
                "version": version,
                "textures": entries.len(),
            }),
            Some(Source::Folder { path }) => json!({ "source": "folder", "path": path }),
            None => json!({ "source": null }),
        }
    }
}

/// Texture names are file stems: lowercase letters, digits and underscores.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn open(path: &Path, version: Option<String>) -> Result<Source, String> {
    if path.is_dir() {
        // A resource pack's root, or the block folder itself.
        let nested = path.join(BLOCK_DIR);
        let folder = if nested.is_dir() {
            nested
        } else {
            path.to_path_buf()
        };
        return Ok(Source::Folder { path: folder });
    }
    let entries = zip_index(path)
        .map_err(|e| format!("not a readable jar or zip: {e}"))?
        .into_iter()
        .filter_map(|(name, entry)| {
            let file = name.strip_prefix(BLOCK_DIR)?.strip_suffix(".png")?;
            valid_name(file).then(|| (file.to_string(), entry))
        })
        .collect::<HashMap<_, _>>();
    if entries.is_empty() {
        return Err("it has no block textures".into());
    }
    Ok(Source::Zip {
        path: path.to_path_buf(),
        version,
        entries,
    })
}

/// Client jars the launchers keep, as (version, path): the vanilla
/// launcher's and CurseForge's `versions/<v>/<v>.jar`, Prism's and MultiMC's
/// `libraries/com/mojang/minecraft/<v>/minecraft-<v>-client.jar`, the Modrinth
/// App's `meta/versions/<v>/<v>.jar`. Only plain release versions count.
fn client_jars() -> Vec<(String, PathBuf)> {
    let roots = launcher_roots();
    let mut found = Vec::new();
    let mut scan = |dir: PathBuf, jar: &dyn Fn(&str) -> String| {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            let version = entry.file_name().to_string_lossy().into_owned();
            let release =
                !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit() || b == b'.');
            let path = entry.path().join(jar(&version));
            if release && path.is_file() {
                found.push((version, path));
            }
        }
    };
    let same_name = |v: &str| format!("{v}.jar");
    let library = |v: &str| format!("minecraft-{v}-client.jar");
    for root in &roots.minecraft {
        scan(root.join("versions"), &same_name);
    }
    for root in &roots.curseforge {
        scan(root.join("Install").join("versions"), &same_name);
    }
    for root in &roots.modrinth {
        scan(root.join("meta").join("versions"), &same_name);
    }
    for root in roots.prism.iter().chain(&roots.multimc) {
        scan(
            root.join("libraries")
                .join("com")
                .join("mojang")
                .join("minecraft"),
            &library,
        );
    }
    found
}

fn newest_client_jar() -> Option<(String, PathBuf)> {
    let key = |v: &str| -> Vec<u32> { v.split('.').filter_map(|p| p.parse().ok()).collect() };
    client_jars()
        .into_iter()
        .max_by(|a, b| key(&a.0).cmp(&key(&b.0)))
}

// ---- A minimal zip reader --------------------------------------------------
//
// A client jar is an ordinary zip: its central directory lists every entry,
// stored or deflated. That is all this needs, so it does without a zip crate.

#[derive(Debug, Clone)]
struct ZipEntry {
    method: u16,
    compressed: u64,
    size: u64,
    header_offset: u64,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Every entry of the zip at `path`, by name.
fn zip_index(path: &Path) -> std::io::Result<HashMap<String, ZipEntry>> {
    let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_string());
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    // The end-of-central-directory record is the last 22 bytes, plus a
    // comment of at most 64 KiB.
    let tail_len = len.min(22 + 65_535);
    file.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = vec![0u8; tail_len as usize];
    file.read_exact(&mut tail)?;
    let eocd = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(&tail, i) == 0x0605_4b50)
        .ok_or_else(|| bad("no end of central directory"))?;
    let entries = u16_at(&tail, eocd + 10) as usize;
    let dir_size = u32_at(&tail, eocd + 12) as u64;
    let dir_offset = u32_at(&tail, eocd + 16) as u64;
    if dir_offset.checked_add(dir_size).is_none_or(|end| end > len) {
        return Err(bad("central directory out of range"));
    }

    file.seek(SeekFrom::Start(dir_offset))?;
    let mut dir = vec![0u8; dir_size as usize];
    file.read_exact(&mut dir)?;
    let mut out = HashMap::with_capacity(entries);
    let mut at = 0;
    while at + 46 <= dir.len() && u32_at(&dir, at) == 0x0201_4b50 {
        let name_len = u16_at(&dir, at + 28) as usize;
        let extra_len = u16_at(&dir, at + 30) as usize;
        let comment_len = u16_at(&dir, at + 32) as usize;
        let name_end = at + 46 + name_len;
        if name_end > dir.len() {
            return Err(bad("truncated central directory"));
        }
        let name = String::from_utf8_lossy(&dir[at + 46..name_end]).into_owned();
        out.insert(
            name,
            ZipEntry {
                method: u16_at(&dir, at + 10),
                compressed: u32_at(&dir, at + 20) as u64,
                size: u32_at(&dir, at + 24) as u64,
                header_offset: u32_at(&dir, at + 42) as u64,
            },
        );
        at = name_end + extra_len + comment_len;
    }
    Ok(out)
}

fn read_entry(path: &Path, entry: &ZipEntry) -> std::io::Result<Vec<u8>> {
    let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_string());
    // Textures are small; anything claiming more is not one.
    if entry.size > 16 << 20 || entry.compressed > 16 << 20 {
        return Err(bad("entry too large"));
    }
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(entry.header_offset))?;
    let mut header = [0u8; 30];
    file.read_exact(&mut header)?;
    if u32_at(&header, 0) != 0x0403_4b50 {
        return Err(bad("bad local header"));
    }
    let skip = u16_at(&header, 26) as i64 + u16_at(&header, 28) as i64;
    file.seek(SeekFrom::Current(skip))?;
    let mut data = vec![0u8; entry.compressed as usize];
    file.read_exact(&mut data)?;
    match entry.method {
        0 => Ok(data),
        8 => {
            let mut out = Vec::with_capacity(entry.size as usize);
            flate2::read::DeflateDecoder::new(&data[..])
                .take(entry.size)
                .read_to_end(&mut out)?;
            Ok(out)
        }
        _ => Err(bad("unsupported compression")),
    }
}

// ---- Routes -----------------------------------------------------------------

/// Where the block textures come from, if anywhere.
#[get("/api/textures")]
async fn info(app: App) -> HttpResponse {
    let app = app.into_inner();
    let described = web::block(move || app.textures().describe()).await;
    HttpResponse::Ok().json(described.unwrap_or_else(|_| json!({ "source": null })))
}

/// One block texture, as PNG.
#[get("/api/textures/block/{name}.png")]
async fn block(app: App, name: web::Path<String>) -> HttpResponse {
    let name = name.into_inner();
    if !valid_name(&name) {
        return HttpResponse::BadRequest().json(json!({ "error": "Not a texture name" }));
    }
    let app = app.into_inner();
    match web::block(move || app.textures().get(&name)).await {
        Ok(Some(png)) => HttpResponse::Ok()
            .content_type("image/png")
            .insert_header(("Cache-Control", "private, max-age=86400"))
            .body(png.as_ref().clone()),
        _ => HttpResponse::NotFound().json(json!({ "error": "No such texture here" })),
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(info).service(block);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A zip holding `files`, deflated, as a jar would store them.
    pub(crate) fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in files {
            let mut enc =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(data).unwrap();
            let packed = enc.finish().unwrap();
            let offset = out.len() as u32;
            let mut local = vec![0u8; 30];
            local[0..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
            local[8..10].copy_from_slice(&8u16.to_le_bytes());
            local[18..22].copy_from_slice(&(packed.len() as u32).to_le_bytes());
            local[22..26].copy_from_slice(&(data.len() as u32).to_le_bytes());
            local[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&local);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&packed);
            let mut entry = vec![0u8; 46];
            entry[0..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
            entry[10..12].copy_from_slice(&8u16.to_le_bytes());
            entry[20..24].copy_from_slice(&(packed.len() as u32).to_le_bytes());
            entry[24..28].copy_from_slice(&(data.len() as u32).to_le_bytes());
            entry[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
            entry[42..46].copy_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(&entry);
            central.extend_from_slice(name.as_bytes());
        }
        let dir_offset = out.len() as u32;
        out.extend_from_slice(&central);
        let mut end = vec![0u8; 22];
        end[0..4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
        end[8..10].copy_from_slice(&(files.len() as u16).to_le_bytes());
        end[10..12].copy_from_slice(&(files.len() as u16).to_le_bytes());
        end[12..16].copy_from_slice(&(central.len() as u32).to_le_bytes());
        end[16..20].copy_from_slice(&dir_offset.to_le_bytes());
        out.extend_from_slice(&end);
        std::fs::write(path, out).unwrap();
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("schemgen_textures_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_block_textures_out_of_a_jar() {
        let dir = temp("jar");
        let jar = dir.join("1.21.4.jar");
        write_zip(
            &jar,
            &[
                (
                    "assets/minecraft/textures/block/stone.png",
                    b"stone png bytes",
                ),
                ("assets/minecraft/textures/item/apple.png", b"not a block"),
                ("net/minecraft/Main.class", b"code"),
            ],
        );
        let textures = Textures {
            source: Some(open(&jar, Some("1.21.4".into())).unwrap()),
            cache: Mutex::new(HashMap::new()),
        };
        assert_eq!(
            textures.get("stone").unwrap().as_slice(),
            b"stone png bytes"
        );
        assert!(textures.get("apple").is_none());
        assert!(textures.get("../item/apple").is_none());
        assert_eq!(textures.describe()["textures"], 1);
        assert_eq!(textures.describe()["version"], "1.21.4");
    }

    #[test]
    fn reads_a_resource_pack_folder() {
        let dir = temp("folder");
        let blocks = dir.join(BLOCK_DIR);
        std::fs::create_dir_all(&blocks).unwrap();
        std::fs::write(blocks.join("dirt.png"), b"dirt").unwrap();
        let textures = Textures::discover(Some(&dir));
        assert_eq!(textures.get("dirt").unwrap().as_slice(), b"dirt");
        assert!(textures.get("stone").is_none());
        assert_eq!(textures.describe()["source"], "folder");
    }

    #[test]
    fn names_are_file_stems_only() {
        assert!(valid_name("oak_log_top"));
        for bad in ["", "Stone", "../x", "a/b", "a.png", &"a".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
