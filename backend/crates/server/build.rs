//! Builds the web UI into the server.
//!
//! Every file of a built UI becomes a `&'static [u8]` in `$OUT_DIR/ui.rs`,
//! which `src/ui.rs` serves. The UI is the folder `SCHEMGEN_EMBED_UI` names
//! (an absolute path; the build fails if it holds no `index.html`), else this
//! checkout's `frontend/dist` if it has been built. With neither the binary
//! has no UI of its own, and serves one from disk if it finds one.

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SCHEMGEN_EMBED_UI");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let (dir, required) = match env::var_os("SCHEMGEN_EMBED_UI").filter(|v| !v.is_empty()) {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            assert!(
                dir.is_absolute(),
                "SCHEMGEN_EMBED_UI must be an absolute path, not {}",
                dir.display()
            );
            (dir, true)
        }
        None => (manifest_dir.join("../../../frontend/dist"), false),
    };

    let mut files = Vec::new();
    if dir.join("index.html").is_file() {
        // Cargo scans a directory for changes recursively. A folder that does
        // not exist is not watched: that would rerun this script, and rebuild
        // the server, on every build.
        println!("cargo:rerun-if-changed={}", dir.display());
        collect(&dir, &dir, &mut files);
        files.sort();
    } else if required {
        panic!(
            "SCHEMGEN_EMBED_UI={}: no index.html there. Build the web UI first \
             (cd frontend && npm ci && npm run build).",
            dir.display()
        );
    }

    let mut code = String::from(
        "/// The web UI built into this binary: (path, contents), sorted by path.\n\
         pub static FILES: &[(&str, &[u8])] = &[\n",
    );
    for (name, path) in &files {
        let path = path.to_str().expect("UI file paths are UTF-8");
        code.push_str(&format!("    ({name:?}, include_bytes!({path:?})),\n"));
    }
    code.push_str("];\n");

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo")).join("ui.rs");
    // Rewritten only when it changes, so an unchanged UI rebuilds nothing.
    if fs::read_to_string(&out).ok().as_deref() != Some(code.as_str()) {
        fs::write(&out, code).expect("write ui.rs");
    }
}

/// Every file under `dir`, named by its `/`-separated path below `root`.
/// Dot files (`.DS_Store` and the like) are left out.
fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            let name = path
                .strip_prefix(root)
                .expect("below the root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.push((name, path));
        }
    }
}
