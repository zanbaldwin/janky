//! `janky preview --export <DIR>`: the preview as static files, for a web server such as Nginx.
//!
//! The files have the same paths as the URLs of the live server, so the page and the card documents do not change.
//! The export runs the pipeline of `janky build` once, and stops on any error, so that a public site never shows
//! diagnostics. `preview.json` has `"exported": true`. The back of a card is Anki's output for an empty typed answer,
//! because no server can compare a typed answer.
//!
//! The generation is a BLAKE3 hash of the rows, the card documents and the media files, so the same project gives the
//! same files. A web server can then keep the files below `{generation}/` for a long time.
//!
//! `deck.apkg` is the package of `janky build`, but only if `deck.lock` exists and the build would not change it.
//! Otherwise the ids, GUIDs and times of the package would differ from the next real build, and an import of both
//! would make duplicates. The export then has no `deck.apkg`, and gives a warning.
//!
//! The export writes into a new directory next to `DIR`, then renames it to `DIR`. It writes no file in the project
//! and never writes through a link. `DIR` must not exist, be an empty directory, or be an earlier export: a
//! directory with the marker file [`MARKER`]. An earlier export is renamed away, and is then deleted.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use serde_json::json;

use super::{ASSETS, Loaded, Selection, document, generation, media_map, render, row_json, ui_built};
use crate::cli::{missing_media, prepare};
use crate::diagnostic::{Diagnostic, Diagnostics, json_text};
use crate::project;

/// The file that marks a directory as an export, so that a later export may replace it.
const MARKER: &str = ".janky-export";

/// The directory below the staging directory that becomes `{generation}/` when the hash is known.
const PENDING: &str = ".generation";

/// What is at the export directory before the export.
enum Target {
    Absent,
    Empty,
    Previous,
}

/// Runs `janky preview --export`. Problems go into `diagnostics`.
pub fn export(
    deck: &Path,
    options: project::Options,
    dir: &Path,
    diagnostics: &mut Diagnostics,
    messages: &mut Vec<String>,
    log: &mut Vec<String>,
) {
    if !ui_built() {
        diagnostics.push(
            Diagnostic::error("preview::no-ui", "the export needs the preview UI, and this janky was built without it")
                .with_help("run `just assets`, then build janky again"),
        );
        return;
    }
    if let Err(message) = target(dir) {
        diagnostics.push(target_error(dir, message));
        return;
    }
    let Some(prepared) = prepare(deck, options, diagnostics, log) else {
        return;
    };
    missing_media(&prepared.checked.rendered, false, diagnostics);
    let lock_path = prepared.checked.project.root.join("deck.lock");
    let current =
        fs::symlink_metadata(&lock_path).is_ok_and(|metadata| metadata.is_file()) && prepared.previous == prepared.next;
    if !current {
        diagnostics.push(
            Diagnostic::warning(
                "preview::export-lock",
                "the export has no `deck.apkg`, because `deck.lock` is missing or out of date",
            )
            .with_help("run `janky build`, then export again"),
        );
    }
    let mut loaded = Loaded {
        checked: prepared.checked,
        lock: prepared.next,
        package: prepared.package,
    };
    let rows = match render(&mut loaded, &Selection::All) {
        Ok(rows) => rows,
        Err(message) => {
            diagnostics.push(Diagnostic::error("preview::render", message));
            return;
        },
    };
    let mut media: Vec<(String, PathBuf)> = media_map(&loaded).into_iter().collect();
    media.sort();
    if let Some((name, _)) = media.iter().find(|(name, _)| !plain_name(name)) {
        diagnostics
            .push(Diagnostic::error("preview::export-write", format!("the media name `{name}` is not a file name")));
        return;
    }

    let staging = match sibling(dir, "tmp") {
        Ok(staging) => staging,
        Err(message) => {
            diagnostics.push(write_error(dir, message));
            return;
        },
    };
    let written = (|| {
        fs::create_dir(&staging).map_err(|error| format!("cannot create `{}`: {error}", staging.display()))?;
        write_site(&staging, &rows, &media, current.then_some(&loaded.package))
    })();
    let written = written.and_then(|()| swap(&staging, dir));
    match written {
        Ok(()) => {
            let media = match media.len() {
                1 => "1 media file".to_string(),
                count => format!("{count} media files"),
            };
            let package = if current { ", deck.apkg" } else { "" };
            messages.push(format!("wrote {}: {} cards, {media}{package}", dir.display(), rows.len()));
        },
        Err(message) => {
            let _ = fs::remove_dir_all(&staging);
            diagnostics.push(write_error(dir, message));
        },
    }
}

/// Checks what is at `dir`. It refuses a link, a file, and a directory that is neither empty nor an export.
fn target(dir: &Path) -> Result<Target, String> {
    let metadata = match fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Target::Absent),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() {
        return Err("it is a symbolic link".into());
    }
    if !metadata.is_dir() {
        return Err("it is not a directory".into());
    }
    if fs::read_dir(dir).map_err(|error| error.to_string())?.next().is_none() {
        return Ok(Target::Empty);
    }
    if fs::symlink_metadata(dir.join(MARKER)).is_ok_and(|marker| marker.is_file()) {
        return Ok(Target::Previous);
    }
    Err(format!("it is not empty, and it has no `{MARKER}` file of an earlier export"))
}

fn target_error(dir: &Path, message: String) -> Diagnostic {
    Diagnostic::error("preview::export-target", format!("cannot export to `{}`: {message}", dir.display()))
        .with_help("use a new directory, or remove the directory first")
}

fn write_error(dir: &Path, message: String) -> Diagnostic {
    Diagnostic::error("preview::export-write", format!("cannot export to `{}`: {message}", dir.display()))
}

/// A new path next to `dir`, for example `site.tmp-<hex>` for `site`.
fn sibling(dir: &Path, kind: &str) -> Result<PathBuf, String> {
    let name = dir.file_name().ok_or("the path has no directory name")?.to_string_lossy();
    let parent = dir.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err(format!("`{}` is not a directory", parent.display()));
    }
    Ok(parent.join(format!("{name}.{kind}-{}", &generation()[..12])))
}

/// One normal path component, so that a media file stays in `media/`.
fn plain_name(name: &str) -> bool {
    !name.contains(['/', '\\', '\0'])
        && matches!(Path::new(name).components().collect::<Vec<_>>().as_slice(), [Component::Normal(_)])
}

/// Writes the site into the new directory `staging`, with `deck.apkg` if there is a `package`, and the marker last.
fn write_site(
    staging: &Path,
    rows: &[super::Row],
    media: &[(String, PathBuf)],
    package: Option<&janky_anki::Package>,
) -> Result<(), String> {
    let mut hasher = blake3::Hasher::new();
    let mut hash = |path: &str, bytes: &[u8]| {
        hasher.update(&(path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    let rows_json: Vec<_> = rows.iter().enumerate().map(|(index, row)| row_json(index, row)).collect();
    hash("rows", json_text(&json!(rows_json)).as_bytes());

    for (path, bytes) in ASSETS {
        write(staging, path, bytes)?;
    }
    let pending = format!("{PENDING}/cards");
    fs::create_dir_all(staging.join(&pending)).map_err(|error| error.to_string())?;
    for (index, row) in rows.iter().enumerate() {
        let Ok(rendered) = &row.rendered else {
            continue;
        };
        let back = janky_anki::compare_typed(&rendered.answer.html, rendered.type_answer.as_ref(), "")
            .map_err(|error| error.0)?;
        for (side, html, sounds) in [
            ("front", &rendered.question.html, &rendered.question.sounds),
            ("back", &back, &rendered.answer.sounds),
        ] {
            let path = format!("cards/{index}-{side}.html");
            let body = document(&row.css, row.ord, html, sounds, side);
            hash(&path, body.as_bytes());
            write(staging, &format!("{PENDING}/{path}"), body.as_bytes())?;
        }
    }
    fs::create_dir_all(staging.join(PENDING).join("media")).map_err(|error| error.to_string())?;
    for (name, source) in media {
        let bytes = fs::read(source).map_err(|error| format!("cannot read `{}`: {error}", source.display()))?;
        let path = format!("media/{name}");
        hash(&path, &bytes);
        write(staging, &format!("{PENDING}/{path}"), &bytes)?;
    }

    let digest = hasher.finalize().to_hex();
    let generation = &digest[..32];
    fs::rename(staging.join(PENDING), staging.join(generation)).map_err(|error| error.to_string())?;
    let preview = json!({
        "generation": generation,
        "exported": true,
        "stale": false,
        "diagnostics": [],
        "rows": rows_json,
    });
    write(staging, "preview.json", json_text(&preview).as_bytes())?;
    if let Some(package) = package {
        janky_anki::write_apkg(package, &staging.join("deck.apkg"))
            .map_err(|error| format!("cannot write `deck.apkg`: {}", error.0))?;
    }
    write(staging, MARKER, b"janky preview --export\n")
}

/// Writes a new file below `root`. Its directories are made first.
fn write(root: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|error| format!("cannot write `{}`: {error}", target.display()))?;
    file.write_all(bytes).map_err(|error| format!("cannot write `{}`: {error}", target.display()))
}

/// Puts `staging` at `dir`. An earlier export is renamed away first, and is deleted after the swap.
fn swap(staging: &Path, dir: &Path) -> Result<(), String> {
    match target(dir)? {
        Target::Absent | Target::Empty => fs::rename(staging, dir).map_err(|error| error.to_string()),
        Target::Previous => {
            let old = sibling(dir, "old")?;
            fs::rename(dir, &old).map_err(|error| error.to_string())?;
            if let Err(error) = fs::rename(staging, dir) {
                let _ = fs::rename(&old, dir);
                return Err(error.to_string());
            }
            fs::remove_dir_all(&old).map_err(|error| format!("cannot remove `{}`: {error}", old.display()))
        },
    }
}
