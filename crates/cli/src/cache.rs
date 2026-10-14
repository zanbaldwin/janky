//! The caches of generated results and of plugin files, all in `.janky/cache/` of the project.
//!
//! The media cache, `.janky/cache/media/`, holds the generated files of `media` jobs. Each
//! result is stored as `<hash>.<ext>`, with the metadata `<hash>.json` next to it (see [`Meta`]). [`store`] writes the file first and the metadata last, each to a temporary file
//! that it renames, so a paid result is never half-written. [`lookup`] counts a result as
//! cached only when the metadata is valid and the file is a regular file, not a link. Both
//! refuse a cache directory that is a link or that resolves to a place outside the project.
//!
//! The data cache, `.janky/cache/data/`, holds the results of `enrich` jobs as `<hash>.json`
//! ([`Data`]). [`store_data`] and [`lookup_data`] follow the same rules as the media cache.
//!
//! [`plugin_file`] gives the path of a file in a plugin instance's own file area,
//! `.janky/cache/plugins/<instance>/files/`, which `janky.http.download` writes.
//! [`plugin_files_dir`] gives the area itself, which `janky.fs` reads as `cache` (see
//! [`crate::fs`]). [`plugin_cache_file`] gives the SQLite file of `janky.cache` next to it (see
//! [`crate::plugin_cache`]). These paths have the same link and project checks.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cli::prepare_target;
use crate::generate::valid_ext;

/// The media cache directory, relative to the project root.
pub const MEDIA_DIR: &str = ".janky/cache/media";
/// The data cache directory, relative to the project root.
pub const DATA_DIR: &str = ".janky/cache/data";
/// The largest data value, in bytes of JSON.
pub const VALUE_LIMIT: usize = 1024 * 1024;
/// The directory of the plugin instances' own files and caches, relative to the project root.
pub const PLUGINS_DIR: &str = ".janky/cache/plugins";
const META_LIMIT: u64 = 64 * 1024;

/// The metadata of a cached media result, stored as `<hash>.json` in [`MEDIA_DIR`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// The extension of the cached file, without the dot.
    pub ext: String,
    /// The plugin instance that made the file.
    pub instance: String,
    /// The cost that `generate` reported. `None` when it reported none.
    pub cost: Option<f64>,
}

/// Returns the path of `file` in the media cache directory of `root`. It does not check that the
/// directory is safe to use (see [`media_dir`]).
pub fn media_path(root: &Path, file: &str) -> PathBuf {
    root.join(MEDIA_DIR).join(file)
}

fn valid_hash(hash: &str) -> bool {
    !hash.is_empty() && hash.len() <= 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Returns the canonical path of the media cache directory, or `None` when it does not exist or is
/// not exactly `.janky/cache/media` inside the canonical project root.
///
/// A link at `.janky`, `cache` or `media` makes the canonical paths differ, so it gives
/// `None`. An empty `root` means the current directory.
pub fn media_dir(root: &Path) -> Option<PathBuf> {
    exact_dir(root, MEDIA_DIR)
}

/// Returns the canonical path of the data cache directory, with the rules of [`media_dir`].
pub fn data_dir(root: &Path) -> Option<PathBuf> {
    exact_dir(root, DATA_DIR)
}

fn exact_dir(root: &Path, relative: &str) -> Option<PathBuf> {
    let root = if root.as_os_str().is_empty() { Path::new(".") } else { root };
    let (root, dir) = (root.canonicalize().ok()?, root.join(relative).canonicalize().ok()?);
    (dir == root.join(relative)).then_some(dir)
}

/// The cached result of a data job, stored as `<hash>.json` in [`DATA_DIR`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Data {
    /// The plugin instance that made the result.
    pub instance: String,
    /// The value: always a JSON object.
    pub value: serde_json::Value,
    /// The warning that `generate` returned, shown where `enrich` is called.
    pub warning: Option<String>,
    /// The info that `generate` returned, shown where `enrich` is called. The terminal condenses
    /// it unless `--verbose` is set. Files from before this key have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
    /// The cost that `generate` reported. `None` when it reported none.
    pub cost: Option<f64>,
}

/// Returns the cached result of the data job `hash`, or `None` when there is no usable one.
///
/// It gives `None` when the hash is not valid, when the data directory is a link or is outside
/// the project, or when the file is missing, is not a regular file, is larger than the value
/// limit allows, is not valid, or has a value that is not an object.
pub fn lookup_data(root: &Path, hash: &str) -> Option<Data> {
    if !valid_hash(hash) {
        return None;
    }
    let path = exact_dir(root, DATA_DIR)?.join(format!("{hash}.json"));
    if !std::fs::symlink_metadata(&path).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(&path).ok()?.take(2 * VALUE_LIMIT as u64).read_to_string(&mut text).ok()?;
    let data: Data = serde_json::from_str(&text).ok()?;
    data.value.is_object().then_some(data)
}

/// Writes the result of a data job to the data cache and returns its file name, `<hash>.json`.
///
/// It makes the directory when it is missing. It fails when the hash is not valid, when the
/// directory is a link or is outside the project, when the target is a link, or when a write
/// fails.
pub fn store_data(root: &Path, hash: &str, data: &Data) -> Result<String, String> {
    if !valid_hash(hash) {
        return Err(format!("`{hash}` is not a valid hash"));
    }
    let file = format!("{hash}.json");
    prepare_target(root, &root.join(DATA_DIR).join(&file))?;
    let dir = exact_dir(root, DATA_DIR)
        .ok_or_else(|| format!("`{DATA_DIR}` must be a directory in the project, not a link"))?;
    let json = serde_json::to_vec(data).map_err(|error| error.to_string())?;
    write(&dir.join(&file), &json)?;
    Ok(file)
}

fn inside(root: &Path) -> bool {
    media_dir(root).is_some()
}

/// Returns the file name of a cached media result for `hash`, or `None` when there is no usable one.
///
/// It gives `None` when the hash is not 1 to 64 hex digits, or when [`media_dir`] gives
/// `None`. It also gives `None` when the metadata is missing, is not a regular file or is not
/// valid, or when the file is missing or is not a regular file. When `ext` is given, the
/// extension in the metadata must match it.
pub fn lookup(root: &Path, hash: &str, ext: Option<&str>) -> Option<String> {
    if !valid_hash(hash) || !inside(root) {
        return None;
    }
    let meta = read_meta(&media_path(root, &format!("{hash}.json")))?;
    if !valid_ext(&meta.ext) || ext.is_some_and(|ext| ext != meta.ext) {
        return None;
    }
    let file = format!("{hash}.{}", meta.ext);
    std::fs::symlink_metadata(media_path(root, &file)).ok().filter(|metadata| metadata.is_file()).map(|_| file)
}

fn read_meta(path: &Path) -> Option<Meta> {
    if !std::fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(META_LIMIT).read_to_string(&mut text).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes a media result and its metadata to the media cache and returns the file name, `<hash>.<ext>`.
///
/// It makes the cache directory when it is missing. It fails when the hash or the extension is
/// not valid, when the directory is a link or is outside the project, when a target is a link,
/// or when a write fails. The error is a message for the author.
pub fn store(root: &Path, hash: &str, data: &[u8], meta: &Meta) -> Result<String, String> {
    if !valid_hash(hash) {
        return Err(format!("`{hash}` is not a valid hash"));
    }
    if !valid_ext(&meta.ext) {
        return Err(format!("`{}` is not a valid file extension", meta.ext));
    }
    let file = format!("{hash}.{}", meta.ext);
    prepare_target(root, &media_path(root, &file))?;
    let dir = media_dir(root).ok_or_else(|| format!("`{MEDIA_DIR}` must be a directory in the project, not a link"))?;
    write(&dir.join(&file), data)?;
    let json = serde_json::to_vec(meta).map_err(|error| error.to_string())?;
    write(&dir.join(format!("{hash}.json")), &json)?;
    Ok(file)
}

fn write(target: &Path, data: &[u8]) -> Result<(), String> {
    let (temporary, mut file) = temporary(target)?;
    let result = file.write_all(data).and_then(|()| file.sync_all()).and_then(|()| std::fs::rename(&temporary, target));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|error| format!("cannot write `{}`: {error}", target.display()))
}

/// Makes a new temporary file next to `target`, named `.<name>.tmp-<random>`, and returns its
/// path and the open file. The caller renames it to `target` when it is complete.
///
/// It fails when `target` is a symbolic link, or when the file cannot be made.
pub fn temporary(target: &Path) -> Result<(PathBuf, std::fs::File), String> {
    if std::fs::symlink_metadata(target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(format!("`{}` is a symbolic link", target.display()));
    }
    let name = target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|error| error.to_string())?;
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let temporary = target.with_file_name(format!(".{name}.tmp-{suffix}"));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("cannot write `{}`: {error}", target.display()))?;
    Ok((temporary, file))
}

/// Whether `name` can name a file in a plugin's file area: 1 to 100 ASCII letters, digits,
/// `.`, `_` and `-`, not starting with `.`.
pub fn valid_file_name(name: &str) -> bool {
    (1..=100).contains(&name.len())
        && !name.starts_with('.')
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Returns the path of `file` in the file area of the plugin instance `instance`,
/// `.janky/cache/plugins/<instance>/files/`, and makes the directory when it is missing.
///
/// It fails when `file` fails [`valid_file_name`], when the instance name is not a plain
/// name, when the directory is a link or is outside the project, or when the file is a link.
/// The error is a message for the author.
pub fn plugin_file(root: &Path, instance: &str, file: &str) -> Result<PathBuf, String> {
    if !valid_file_name(file) {
        return Err(format!(
            "`{file}` is not a valid file name: use 1 to 100 letters, digits, `.`, `_` and `-`, not starting with `.`"
        ));
    }
    plugin_path(root, instance, "files", file)
}

/// Returns the file area of `instance`, `.janky/cache/plugins/<instance>/files/`, and makes it
/// when it is missing. It fails like [`plugin_file`].
pub fn plugin_files_dir(root: &Path, instance: &str) -> Result<PathBuf, String> {
    let marker = plugin_path(root, instance, "files", "dir")?;
    Ok(marker.parent().map(Path::to_path_buf).unwrap_or(marker))
}

/// Returns the path of the plugin cache of `instance`,
/// `.janky/cache/plugins/<instance>/cache.sqlite`, and makes the directory when it is missing.
///
/// It fails like [`plugin_file`].
pub fn plugin_cache_file(root: &Path, instance: &str) -> Result<PathBuf, String> {
    plugin_path(root, instance, "", "cache.sqlite")
}

fn plugin_path(root: &Path, instance: &str, sub: &str, file: &str) -> Result<PathBuf, String> {
    if instance.is_empty() || !instance.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(format!("`{instance}` is not a valid plugin instance name"));
    }
    let mut relative = Path::new(PLUGINS_DIR).join(instance);
    if !sub.is_empty() {
        relative.push(sub);
    }
    let root = if root.as_os_str().is_empty() { Path::new(".") } else { root };
    prepare_target(root, &root.join(&relative).join(file))?;
    let canonical = root.canonicalize().map_err(|error| error.to_string())?;
    let dir = root.join(&relative).canonicalize().map_err(|error| error.to_string())?;
    if dir != canonical.join(&relative) {
        return Err(format!("`{}` must be a directory in the project, not a link", relative.display()));
    }
    Ok(dir.join(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(ext: &str) -> Meta {
        Meta {
            ext: ext.into(),
            instance: "voice".into(),
            cost: Some(3.0),
        }
    }

    #[test]
    fn stored_files_are_found_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(lookup(root, "abc", None), None);
        assert_eq!(store(root, "abc", b"sound", &meta("ogg")).unwrap(), "abc.ogg");
        assert!(store(root, "../x", b"sound", &meta("ogg")).is_err());
        assert_eq!(lookup(root, "abc", None).as_deref(), Some("abc.ogg"));
        assert_eq!(lookup(root, "abc", Some("ogg")).as_deref(), Some("abc.ogg"));
        assert_eq!(lookup(root, "abc", Some("mp3")), None);
        assert_eq!(std::fs::read(media_path(root, "abc.ogg")).unwrap(), b"sound");
        assert!(store(root, "abc", b"x", &meta("../x")).is_err());
    }

    #[test]
    fn broken_or_linked_cache_entries_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        store(root, "0a1", b"sound", &meta("ogg")).unwrap();
        std::fs::remove_file(media_path(root, "0a1.ogg")).unwrap();
        assert_eq!(lookup(root, "0a1", None), None);
        std::fs::write(media_path(root, "0b2.json"), r#"{"ext":"../../x","instance":"v","cost":null}"#).unwrap();
        assert_eq!(lookup(root, "0b2", None), None);
        std::fs::write(media_path(root, "elsewhere"), b"secret").unwrap();
        std::fs::write(media_path(root, "0c3.json"), r#"{"ext":"ogg","instance":"v","cost":null}"#).unwrap();
        std::os::unix::fs::symlink(media_path(root, "elsewhere"), media_path(root, "0c3.ogg")).unwrap();
        assert_eq!(lookup(root, "0c3", None), None);
    }

    #[test]
    fn a_cache_directory_outside_the_project_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let (root, outside) = (dir.path().join("project"), dir.path().join("outside"));
        std::fs::create_dir_all(&outside).unwrap();
        store(&outside, "abc", b"sound", &meta("ogg")).unwrap();
        std::fs::create_dir_all(root.join(".janky")).unwrap();
        std::os::unix::fs::symlink(outside.join(".janky/cache"), root.join(".janky/cache")).unwrap();
        assert_eq!(lookup(&outside, "abc", None).as_deref(), Some("abc.ogg"));
        assert_eq!(lookup(&root, "abc", None), None);
        let linked = dir.path().join("linked");
        std::fs::create_dir_all(linked.join("media")).unwrap();
        std::fs::create_dir_all(linked.join(".janky/cache")).unwrap();
        std::os::unix::fs::symlink("../../media", linked.join(".janky/cache/media")).unwrap();
        assert!(media_dir(&linked).is_none());
        assert!(store(&linked, "abc", b"sound", &meta("ogg")).is_err());
        assert!(!linked.join("media/abc.ogg").exists());
    }
}
