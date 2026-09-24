//! User-chosen output folder: resolving, validating and delivering finished
//! schematics into it.
//!
//! The server runs on the same machine as the browser, so a folder typed in the
//! UI (typically `%APPDATA%\.minecraft\schematics`) can be written to directly —
//! no manual download-and-move step.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Characters Windows rejects in file names, plus the path separators.
const ILLEGAL_NAME_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Expand `%VAR%` references in one left-to-right pass (no rescanning of the
/// substituted text, so a value containing `%` cannot loop).
fn expand_env(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            if let Some(rel) = chars[i + 1..].iter().position(|&c| c == '%') {
                let name: String = chars[i + 1..i + 1 + rel].iter().collect();
                if !name.is_empty() {
                    if let Ok(value) = std::env::var(&name) {
                        out.push_str(&value);
                        i += rel + 2;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Expand a leading `~` to the user's home directory.
fn expand_home(raw: &str) -> String {
    let rest = match raw.strip_prefix('~') {
        Some(rest) => rest,
        None => return raw.to_string(),
    };
    if !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\')) {
        return raw.to_string();
    }
    match home_dir() {
        Some(home) => format!("{}{}", home.display(), rest),
        None => raw.to_string(),
    }
}

/// Turn raw UI input into an absolute folder path. Does not touch the disk
/// beyond checking what is already there.
pub fn resolve(raw: &str) -> Result<PathBuf, String> {
    let expanded = expand_home(&expand_env(raw));
    let trimmed = expanded.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return Err("No output folder given".to_string());
    }
    // Accept either separator and report the native one, so a typed
    // "~/Downloads" does not come back as "C:\...".
    let native = if cfg!(windows) {
        trimmed.replace('/', "\\")
    } else {
        trimmed.to_string()
    };
    let path = PathBuf::from(&native);
    if !path.is_absolute() {
        return Err(format!(
            "Output folder must be an absolute path (got \"{native}\")"
        ));
    }
    if path.exists() && !path.is_dir() {
        return Err(format!("\"{}\" exists but is not a folder", path.display()));
    }
    Ok(path)
}

/// Verdict for a folder the user is typing: usable, and does it exist yet?
pub struct DirCheck {
    pub path: PathBuf,
    pub exists: bool,
}

fn probe_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".schemgen_write_test_{}", uuid::Uuid::new_v4()));
    std::fs::write(&probe, b"")
        .map_err(|e| format!("\"{}\" is not writable: {e}", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Check a folder *without* creating it — the UI validates on every keystroke,
/// and a half-typed path must not leave empty folders behind. A folder that
/// does not exist yet is fine as long as its nearest existing ancestor is
/// writable; `prepare` creates it when a conversion actually runs.
pub fn check(raw: &str) -> Result<DirCheck, String> {
    let dir = resolve(raw)?;
    if dir.is_dir() {
        probe_writable(&dir)?;
        return Ok(DirCheck {
            path: dir,
            exists: true,
        });
    }
    let mut ancestor = dir.parent();
    while let Some(p) = ancestor {
        if p.exists() {
            if !p.is_dir() {
                return Err(format!("\"{}\" is not a folder", p.display()));
            }
            probe_writable(p)?;
            return Ok(DirCheck {
                path: dir,
                exists: false,
            });
        }
        ancestor = p.parent();
    }
    Err(format!(
        "\"{}\" is not on a drive that exists",
        dir.display()
    ))
}

/// Resolve, create if missing, and confirm the folder is actually writable.
pub fn prepare(raw: &str) -> Result<PathBuf, String> {
    let dir = resolve(raw)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create \"{}\": {e}", dir.display()))?;
    probe_writable(&dir)?;
    Ok(dir)
}

/// Names Windows reserves for devices, whatever the extension.
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Reduce an arbitrary schematic name to a safe file name ending in
/// `.{extension}`.
///
/// Illegal characters are replaced *before* any path parsing, so both path
/// separators are gone by then — no input can escape the chosen folder, and a
/// name like `a:b` is not mistaken for a Windows drive prefix. Device names
/// Windows reserves (`con`, `nul`, …) get a leading underscore.
pub fn sanitize_filename(name: &str, extension: &str) -> String {
    let mut cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || ILLEGAL_NAME_CHARS.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    cleaned = cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string();

    if cleaned.is_empty() {
        cleaned = "schematic".to_string();
    }
    let suffix = format!(".{extension}");
    if !cleaned.to_lowercase().ends_with(&suffix) {
        cleaned.push_str(&suffix);
    }
    let device = cleaned
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if RESERVED_NAMES.contains(&device.as_str()) {
        cleaned.insert(0, '_');
    }
    cleaned
}

/// Append `-2`, `-3`, … to the stem until the name is unused within `taken`
/// (case-insensitive). Keeps two same-named models in one batch from
/// clobbering each other.
pub fn dedupe_filename(filename: &str, taken: &mut HashSet<String>) -> String {
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (filename, String::new()),
    };
    let mut candidate = filename.to_string();
    let mut n = 2;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = format!("{stem}-{n}{ext}");
        n += 1;
    }
    candidate
}

/// Copy a finished schematic into the chosen folder, overwriting a previous
/// file of the same name. Returns the full destination path.
///
/// The copy lands under a temporary name and is renamed into place, so
/// Litematica, which watches its schematics folder, never lists half a file.
pub fn deliver(src: &Path, dir: &Path, filename: &str) -> Result<PathBuf, String> {
    if !src.exists() {
        return Err(format!("Schematic \"{}\" is missing", src.display()));
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Could not create \"{}\": {e}", dir.display()))?;

    let dest = dir.join(filename);
    let tmp = schemgen_core::formats::temp_path(&dest);
    std::fs::copy(src, &tmp)
        .and_then(|_| std::fs::rename(&tmp, &dest))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("Could not write \"{}\": {e}", dest.display())
        })?;
    Ok(dest)
}

/// Human name of this platform's file manager, for button labels.
pub fn file_manager_name() -> &'static str {
    if cfg!(windows) {
        "Explorer"
    } else if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "file manager"
    }
}

/// Open the OS file manager with `path` selected.
///
/// Arguments are passed straight to the process (no shell), and `path` always
/// comes from our own job state, so there is nothing to escape.
pub fn reveal(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("\"{}\" no longer exists", path.display()));
    }

    #[cfg(windows)]
    {
        // Paths here are already absolute, so no canonicalize() — that would add
        // a verbatim prefix Explorer refuses. Explorer also exits 1 on success,
        // so only the spawn itself is checked.
        Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open Explorer: {e}"))
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open Finder: {e}"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // No portable "select the file" call — open the containing folder.
        let dir = path.parent().unwrap_or(path);
        Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open file manager: {e}"))
    }
}

/// Open a folder itself in the system file manager.
pub fn reveal_dir(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("\"{}\" is not a folder", dir.display()));
    }
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(program)
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open the file manager: {e}"))
}

/// A launcher instance a suggested folder belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// The instance's folder name, which launchers use as its display name.
    pub name: String,
    pub launcher: &'static str,
    /// The Minecraft version it runs, when its launcher records one.
    pub mc_version: Option<String>,
}

/// A folder a UI can offer as a one-click output choice.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub path: PathBuf,
    pub exists: bool,
    pub instance: Option<Instance>,
}

/// Scan a launcher's instance root, returning the `schematics` folder of every
/// instance that actually has one. `inner` is the per-instance prefix the
/// launcher puts the game directory under ("" for CurseForge, ".minecraft" for
/// Prism/MultiMC). Instances without a schematics folder are skipped, so a
/// launcher with 16 packs does not bury the real answers.
fn instance_schematics(
    root: &Path,
    launcher: &'static str,
    inner: &str,
) -> Vec<(PathBuf, Instance)> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let dir = entry.path();
        let game = if inner.is_empty() {
            dir.clone()
        } else {
            dir.join(inner)
        };
        let schem = game.join("schematics");
        if schem.is_dir() {
            found.push((
                schem,
                Instance {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    launcher,
                    mc_version: instance_version(&dir),
                },
            ));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > 4 * 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The Minecraft version an instance folder records, whichever launcher
/// made it: Prism Launcher and MultiMC in `mmc-pack.json`, CurseForge in
/// `minecraftinstance.json`, the Modrinth App's older profiles in
/// `profile.json`.
fn instance_version(dir: &Path) -> Option<String> {
    let text = |v: &serde_json::Value| v.as_str().map(str::to_string);
    if let Some(pack) = read_json(&dir.join("mmc-pack.json")) {
        let minecraft = pack["components"]
            .as_array()?
            .iter()
            .find(|c| c["uid"] == "net.minecraft")?;
        return text(&minecraft["version"]);
    }
    if let Some(cf) = read_json(&dir.join("minecraftinstance.json")) {
        return text(&cf["gameVersion"]).or_else(|| text(&cf["baseModLoader"]["minecraftVersion"]));
    }
    let profile = read_json(&dir.join("profile.json"))?;
    text(&profile["metadata"]["game_version"]).or_else(|| text(&profile["game_version"]))
}

/// Where the launchers keep their data on this platform. Each list holds the
/// data folders that may exist; nothing here checks that they do.
pub(crate) struct LauncherRoots {
    /// The vanilla launcher's game folder (`.minecraft`).
    pub minecraft: Vec<PathBuf>,
    /// Prism Launcher's data folder: `instances/`, `libraries/`.
    pub prism: Vec<PathBuf>,
    /// MultiMC's folder: `instances/`, `libraries/`.
    pub multimc: Vec<PathBuf>,
    /// CurseForge's `minecraft` folder: `Instances/`, `Install/`.
    pub curseforge: Vec<PathBuf>,
    /// The Modrinth App's data folder: `profiles/`, `meta/`.
    pub modrinth: Vec<PathBuf>,
    pub downloads: Option<PathBuf>,
}

pub(crate) fn launcher_roots() -> LauncherRoots {
    let mut roots = LauncherRoots {
        minecraft: Vec::new(),
        prism: Vec::new(),
        multimc: Vec::new(),
        curseforge: Vec::new(),
        modrinth: Vec::new(),
        downloads: None,
    };
    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let appdata = PathBuf::from(appdata);
            roots.minecraft.push(appdata.join(".minecraft"));
            roots.prism.push(appdata.join("PrismLauncher"));
            roots.modrinth.push(appdata.join("com.modrinth.theseus"));
            roots.modrinth.push(appdata.join("ModrinthApp"));
        }
    }
    if let Some(home) = home_dir() {
        if cfg!(target_os = "macos") {
            let support = home.join("Library").join("Application Support");
            roots.minecraft.push(support.join("minecraft"));
            roots.prism.push(support.join("PrismLauncher"));
            roots.modrinth.push(support.join("ModrinthApp"));
        }
        if cfg!(not(windows)) {
            roots.minecraft.push(home.join(".minecraft"));
            let share = home.join(".local").join("share");
            roots.prism.push(share.join("PrismLauncher"));
            roots.modrinth.push(share.join("ModrinthApp"));
        }
        roots
            .curseforge
            .push(home.join("curseforge").join("minecraft"));
        roots.multimc.push(home.join("MultiMC"));
        roots.downloads = Some(home.join("Downloads"));
    }
    roots
}

/// Likely Litematica schematic folders for this platform, each flagged with
/// whether it already exists and, for launcher instances, which instance and
/// Minecraft version it belongs to — so a UI can offer them as one-click
/// choices and suggest the matching target.
pub fn suggestions() -> Vec<Suggestion> {
    // Vanilla folders, then every launcher instance, then Downloads.
    let roots = launcher_roots();
    let vanilla: Vec<PathBuf> = roots
        .minecraft
        .iter()
        .map(|m| m.join("schematics"))
        .collect();
    let mut instances: Vec<(PathBuf, Instance)> = Vec::new();
    let mut scan = |root: PathBuf, launcher: &'static str, inner: &str| {
        instances.extend(instance_schematics(&root, launcher, inner));
    };
    // Prism and MultiMC keep the game dir under <instance>/.minecraft;
    // CurseForge and the Modrinth App put it straight in the instance —
    // CurseForge is where most Litematica users actually are.
    for root in &roots.prism {
        scan(root.join("instances"), "Prism Launcher", ".minecraft");
    }
    for root in &roots.modrinth {
        scan(root.join("profiles"), "Modrinth App", "");
    }
    for root in &roots.curseforge {
        scan(root.join("Instances"), "CurseForge", "");
    }
    for root in &roots.multimc {
        scan(root.join("instances"), "MultiMC", ".minecraft");
    }
    let downloads = roots.downloads;

    let ordered = vanilla
        .into_iter()
        .map(|p| (p, None))
        .chain(instances.into_iter().map(|(p, i)| (p, Some(i))))
        .chain(downloads.map(|p| (p, None)));
    let mut seen = HashSet::new();
    let mut out: Vec<Suggestion> = ordered
        .filter(|(path, _)| seen.insert(path.clone()))
        .map(|(path, instance)| Suggestion {
            exists: path.is_dir(),
            path,
            instance,
        })
        .collect();
    // Folders that exist first — a discovered instance beats a default that
    // was never created. Order within each group is left as built.
    out.sort_by_key(|s| !s.exists);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_instances_that_have_a_schematics_folder() {
        let root = std::env::temp_dir().join("schemgen2_inst_scan_test");
        let _ = std::fs::remove_dir_all(&root);
        // Two packs with schematics, one without, plus a stray file.
        std::fs::create_dir_all(root.join("packA").join("schematics")).unwrap();
        std::fs::create_dir_all(root.join("packB").join("schematics")).unwrap();
        std::fs::create_dir_all(root.join("packC").join("mods")).unwrap();
        std::fs::write(root.join("notadir.txt"), b"x").unwrap();
        // CurseForge records the version it runs.
        std::fs::write(
            root.join("packA").join("minecraftinstance.json"),
            br#"{"name": "Pack A", "gameVersion": "1.20.1"}"#,
        )
        .unwrap();

        let found = instance_schematics(&root, "CurseForge", "");
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|(p, _)| p.ends_with("schematics")));
        let (_, a) = found
            .iter()
            .find(|(p, _)| p.starts_with(root.join("packA")))
            .unwrap();
        assert_eq!(a.name, "packA");
        assert_eq!(a.launcher, "CurseForge");
        assert_eq!(a.mc_version.as_deref(), Some("1.20.1"));
        let (_, b) = found
            .iter()
            .find(|(p, _)| p.starts_with(root.join("packB")))
            .unwrap();
        assert_eq!(b.mc_version, None);

        // The inner-prefix form (Prism/MultiMC) only matches under .minecraft,
        // and reads the version from mmc-pack.json.
        assert!(instance_schematics(&root, "Prism Launcher", ".minecraft").is_empty());
        std::fs::create_dir_all(root.join("packD").join(".minecraft").join("schematics")).unwrap();
        std::fs::write(
            root.join("packD").join("mmc-pack.json"),
            br#"{"components": [{"uid": "org.lwjgl3", "version": "3.3.3"},
                                {"uid": "net.minecraft", "version": "1.21.4"}]}"#,
        )
        .unwrap();
        let prism = instance_schematics(&root, "Prism Launcher", ".minecraft");
        assert_eq!(prism.len(), 1);
        assert_eq!(prism[0].1.mc_version.as_deref(), Some("1.21.4"));

        // A missing root is not an error.
        assert!(instance_schematics(&root.join("nope"), "CurseForge", "").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sanitizes_and_suffixes() {
        assert_eq!(sanitize_filename("castle", "litematic"), "castle.litematic");
        assert_eq!(
            sanitize_filename("castle.litematic", "litematic"),
            "castle.litematic"
        );
        assert_eq!(sanitize_filename("a:b?c", "litematic"), "a_b_c.litematic");
        assert_eq!(sanitize_filename("   ", "litematic"), "schematic.litematic");
        assert_eq!(sanitize_filename("castle", "schem"), "castle.schem");
        assert_eq!(sanitize_filename("CON", "nbt"), "_CON.nbt");
        assert_eq!(
            sanitize_filename("nul.backup", "schem"),
            "_nul.backup.schem"
        );
        assert_eq!(sanitize_filename("console", "nbt"), "console.nbt");
    }

    #[test]
    fn cannot_escape_the_chosen_folder() {
        for raw in ["../../evil", "..", "C:/Windows/system32/x", "sub/dir/name"] {
            let out = sanitize_filename(raw, "litematic");
            assert!(!out.chars().any(std::path::is_separator), "{raw} -> {out}");
            assert!(!out.starts_with('.'), "{raw} -> {out}");
            assert!(out.ends_with(".litematic"), "{raw} -> {out}");
        }
    }

    #[test]
    fn dedupes_within_batch() {
        let mut taken = HashSet::new();
        assert_eq!(dedupe_filename("a.litematic", &mut taken), "a.litematic");
        assert_eq!(dedupe_filename("a.litematic", &mut taken), "a-2.litematic");
        assert_eq!(dedupe_filename("a.litematic", &mut taken), "a-3.litematic");
        assert_eq!(dedupe_filename("a.schem", &mut taken), "a.schem");
        assert_eq!(dedupe_filename("a.schem", &mut taken), "a-2.schem");
    }

    #[test]
    fn rejects_relative_paths() {
        assert!(resolve("schematics").is_err());
        assert!(resolve("").is_err());
    }

    #[test]
    fn expands_env_without_looping() {
        std::env::set_var("SCHEMGEN_TEST_DIR", "X%Y");
        assert_eq!(expand_env("a%SCHEMGEN_TEST_DIR%b"), "aX%Yb");
        assert_eq!(expand_env("100% done"), "100% done");
    }
}
