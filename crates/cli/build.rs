//! Embeds the preview UI from `ui/dist/`, which `just assets` builds with Vite.
//!
//! The script writes two files to `OUT_DIR`. `vite-manifest.json` is the Vite manifest, and `assets.rs` is a list of
//! `(path, include_bytes!(..))` pairs for each built file outside `.vite/`. If the UI is built, the script sets
//! `cfg(ui_built)`. A debug build without the UI gets an empty manifest and an empty list, so the CLI still compiles.
//! A release build without the UI fails, so a release binary never has a missing UI.
//!
//! `janky show --format html` puts the JS and the CSS of the `card` entry inline in a document. So the build fails if
//! that entry imports another JS file, or if its JS or CSS contains `</script` or `</style`, which would end the
//! element early.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DIST: &str = "ui/dist";
const MANIFEST: &str = "ui/dist/.vite/manifest.json";

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={MANIFEST}");
    println!("cargo::rustc-check-cfg=cfg(ui_built)");
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));

    let (manifest, files) = match fs::read_to_string(MANIFEST) {
        Ok(json) => {
            let parsed = json.parse::<vite_chunks::Manifest>().expect("the Vite manifest to be valid");
            check_card(&parsed, &root.join(DIST));
            let mut files = Vec::new();
            collect(&root.join(DIST), "", &mut files);
            files.sort();
            println!("cargo::rustc-cfg=ui_built");
            (json, files)
        },
        Err(_) => {
            assert!(
                env::var("PROFILE").expect("cargo sets PROFILE") != "release",
                "the preview UI is not built: run `just assets` first"
            );
            ("{}".to_owned(), Vec::new())
        },
    };

    let mut assets = String::from("&[\n");
    for (path, source) in &files {
        assets.push_str(&format!("    ({path:?}, include_bytes!({source:?})),\n"));
    }
    assets.push(']');
    fs::write(out.join("vite-manifest.json"), manifest).expect("write the manifest to OUT_DIR");
    fs::write(out.join("assets.rs"), assets).expect("write the asset list to OUT_DIR");
}

/// Makes sure that the JS and the CSS of the `card` entry can be inline in an HTML document.
fn check_card(manifest: &vite_chunks::Manifest, dist: &Path) {
    let entry = manifest.entry("card").expect("the UI build to have the `card` entry");
    assert!(entry.preloads.is_empty(), "the `card` entry must be one JS file, but it imports other files");
    let script = entry.script.expect("the `card` entry to have a script").to_string();
    for url in std::iter::once(script).chain(entry.stylesheets.iter().map(ToString::to_string)) {
        let path = dist.join(url.trim_start_matches('/'));
        let text = fs::read_to_string(&path).expect("read a file of the `card` entry").to_ascii_lowercase();
        for end in ["</script", "</style"] {
            assert!(!text.contains(end), "`{url}` of the `card` entry contains `{end}`, so it cannot be inline");
        }
    }
}

/// Adds each regular file below `dir` to `files`, as its URL path and its absolute path. It skips `.vite/` and every
/// symbolic link.
fn collect(dir: &Path, prefix: &str, files: &mut Vec<(String, String)>) {
    for item in fs::read_dir(dir).expect("read the UI build directory") {
        let item = item.expect("read the UI build directory");
        let name = item.file_name().into_string().expect("UTF-8 file names in the UI build");
        let path = format!("{prefix}{name}");
        let kind = item.file_type().expect("read the file type");
        if kind.is_dir() && path != ".vite" {
            collect(&item.path(), &format!("{path}/"), files);
        } else if kind.is_file() {
            let source = item.path().into_os_string().into_string().expect("a UTF-8 path to the UI build");
            files.push((path, source));
        }
    }
}
