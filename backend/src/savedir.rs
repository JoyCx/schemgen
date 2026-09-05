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
    std::env::var("USERPROFILE").ok()
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
    let native = if cfg!(windows) { trimmed.replace('/', "\\") } else { trimmed.to_string() };
    let path = PathBuf::from(&native);
    if !path.is_absolute() {
        return Err(format!("Output folder must be an absolute path (got \"{native}\")"));
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
        return Ok(DirCheck { path: dir, exists: true });
    }
    let mut ancestor = dir.parent();
    while let Some(p) = ancestor {
        if p.exists() {
            if !p.is_dir() {
                return Err(format!("\"{}\" is not a folder", p.display()));
            }
            probe_writable(p)?;
            return Ok(DirCheck { path: dir, exists: false });
        }
        ancestor = p.parent();
    }
    Err(format!("\"{}\" is not on a drive that exists", dir.display()))
}

/// Resolve, create if missing, and confirm the folder is actually writable.
pub fn prepare(raw: &str) -> Result<PathBuf, String> {
    let dir = resolve(raw)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create \"{}\": {e}", dir.display()))?;
    probe_writable(&dir)?;
    Ok(dir)
}

/// Reduce an arbitrary schematic name to a safe `*.litematic` file name.
///
/// Illegal characters are replaced *before* any path parsing, so both path
/// separators are gone by then — no input can escape the chosen folder, and a
/// name like `a:b` is not mistaken for a Windows drive prefix.
pub fn sanitize_filename(name: &str) -> String {
    let mut cleaned: String = name
        .trim()
        .chars()
        .map(|c| if c.is_control() || ILLEGAL_NAME_CHARS.contains(&c) { '_' } else { c })
        .collect();
    cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();

    if cleaned.is_empty() {
        cleaned = "schematic".to_string();
    }
    if !cleaned.to_lowercase().ends_with(".litematic") {
        cleaned.push_str(".litematic");
    }
    cleaned
}

/// Append `-2`, `-3`, … until the name is unused within `taken` (case-insensitive).
/// Used to keep two same-named models in one batch from clobbering each other.
pub fn dedupe_filename(filename: &str, taken: &mut HashSet<String>) -> String {
    let stem = filename.strip_suffix(".litematic").unwrap_or(filename);
    let mut candidate = filename.to_string();
    let mut n = 2;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = format!("{stem}-{n}.litematic");
        n += 1;
    }
    candidate
}

/// Copy a finished schematic into the chosen folder, overwriting a previous
/// file of the same name. Returns the full destination path.
pub fn deliver(src: &Path, dir: &Path, filename: &str) -> Result<PathBuf, String> {
    if !src.exists() {
        return Err(format!("Schematic \"{}\" is missing", src.display()));
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Could not create \"{}\": {e}", dir.display()))?;

    let dest = dir.join(filename);
    std::fs::copy(src, &dest)
        .map_err(|e| format!("Could not write \"{}\": {e}", dest.display()))?;
    Ok(dest)
}

/// Human name of this platform's file manager, for button labels.
pub fn file_manager_name() -> &'static str {
    if cfg!(windows) { "Explorer" }
    else if cfg!(target_os = "macos") { "Finder" }
    else { "file manager" }
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
        Command::new("open").arg("-R").arg(path).spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open Finder: {e}"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // No portable "select the file" call — open the containing folder.
        let dir = path.parent().unwrap_or(path);
        Command::new("xdg-open").arg(dir).spawn()
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

/// Likely Litematica schematic folders for this platform, each flagged with
/// whether it already exists, so the UI can offer them as one-click choices.
/// Scan a launcher's instance root, returning the `schematics` folder of every
/// instance that actually has one. `inner` is the per-instance prefix the
/// launcher puts the game directory under ("" for CurseForge, ".minecraft" for
/// Prism/MultiMC). Instances without a schematics folder are skipped, so a
/// launcher with 16 packs does not bury the real answers.
fn instance_schematics(root: &Path, inner: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return found };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let mut dir = entry.path();
        if !inner.is_empty() {
            dir = dir.join(inner);
        }
        let schem = dir.join("schematics");
        if schem.is_dir() {
            found.push(schem);
        }
    }
    found.sort();
    found
}

pub fn suggestions() -> Vec<(PathBuf, bool)> {
    let mut paths: Vec<PathBuf> = Vec::new();

    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let appdata = PathBuf::from(appdata);
            paths.push(appdata.join(".minecraft").join("schematics"));
            // Prism and MultiMC keep the game dir under <instance>/.minecraft.
            paths.extend(instance_schematics(
                &appdata.join("PrismLauncher").join("instances"), ".minecraft"));
            paths.extend(instance_schematics(
                &appdata.join("com.modrinth.theseus").join("profiles"), ""));
        }
    }
    if let Some(home) = home_dir() {
        if cfg!(target_os = "macos") {
            paths.push(home.join("Library").join("Application Support")
                .join("minecraft").join("schematics"));
        }
        if cfg!(not(windows)) {
            paths.push(home.join(".minecraft").join("schematics"));
            paths.extend(instance_schematics(
                &home.join(".local").join("share").join("PrismLauncher").join("instances"),
                ".minecraft"));
        }
        // CurseForge puts instances straight in the profile, with no inner
        // .minecraft — this is where most Litematica users actually are.
        paths.extend(instance_schematics(
            &home.join("curseforge").join("minecraft").join("Instances"), ""));
        paths.extend(instance_schematics(
            &home.join("MultiMC").join("instances"), ".minecraft"));
        paths.push(home.join("Downloads"));
    }

    let mut seen = HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    let mut out: Vec<(PathBuf, bool)> = paths.into_iter()
        .map(|p| { let exists = p.is_dir(); (p, exists) })
        .collect();
    // Folders that exist first — a discovered instance beats a default that was
    // never created. Order within each group is left as built.
    out.sort_by_key(|(_, exists)| !*exists);
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

        let found = instance_schematics(&root, "");
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|p| p.ends_with("schematics")));
        assert!(found.iter().any(|p| p.starts_with(root.join("packA"))));
        assert!(found.iter().any(|p| p.starts_with(root.join("packB"))));

        // The inner-prefix form (Prism/MultiMC) only matches under .minecraft.
        assert!(instance_schematics(&root, ".minecraft").is_empty());
        std::fs::create_dir_all(root.join("packD").join(".minecraft").join("schematics")).unwrap();
        assert_eq!(instance_schematics(&root, ".minecraft").len(), 1);

        // A missing root is not an error.
        assert!(instance_schematics(&root.join("nope"), "").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sanitizes_and_suffixes() {
        assert_eq!(sanitize_filename("castle"), "castle.litematic");
        assert_eq!(sanitize_filename("castle.litematic"), "castle.litematic");
        assert_eq!(sanitize_filename("a:b?c"), "a_b_c.litematic");
        assert_eq!(sanitize_filename("   "), "schematic.litematic");
    }

    #[test]
    fn cannot_escape_the_chosen_folder() {
        for raw in ["../../evil", "..", "C:/Windows/system32/x", "sub/dir/name"] {
            let out = sanitize_filename(raw);
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
