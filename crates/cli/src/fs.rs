//! Read-only file access for plugins: `janky.fs.read`, `list`, `zip_entries` and `json_items`.
//!
//! A plugin path is `<name>/<rest>`. The name is a directory that `[plugins.<name>.filesystem]`
//! grants, or `cache`, the instance's own file area that `janky.http.download` writes. The
//! plugin never sees host paths.
//!
//! janky opens the named directory, then each part of the rest relative to the directory before
//! it, with `O_NOFOLLOW`. So `..`, absolute paths and symbolic links inside a granted directory
//! are errors, and nothing can be swapped between a check and an open. A file opens with
//! `O_NONBLOCK` and `O_NOCTTY` and must be a regular file, so a FIFO or a terminal cannot stop or
//! change janky.
//!
//! Nothing that holds a secret value reaches the plugin: [`read`] and [`json_items`] refuse such
//! data, also inside a zip entry, which the scan of a download cannot see.
//!
//! [`json_items`] streams the items of a large JSON array on its own thread, from a file or from
//! an entry of a zip file. It limits the unpacked bytes, the size of one item and the number of
//! streams, and it keeps at most one item waiting.

use std::cell::Cell;
use std::fmt;
use std::io::{BufReader, Read};
use std::os::fd::OwnedFd;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use indexmap::IndexMap;
use rustix::fs::{FileType, Mode, OFlags};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value as Json;

/// The largest file that `janky.fs.read` reads, in bytes.
pub const READ_LIMIT: u64 = 16 * 1024 * 1024;
/// The most bytes that one `json_items` stream reads, after unpacking.
pub const STREAM_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
/// The largest item of a `json_items` stream, in bytes of JSON. janky counts the bytes that it
/// reads from the file, which runs up to 64 KiB ahead of the parser, so an item can exceed the
/// limit by up to that much.
pub const ITEM_LIMIT: u64 = 1024 * 1024;
/// The most entries that `list`, `zip_entries` and a zip file for `json_items` can have.
pub const ENTRY_LIMIT: usize = 100_000;
/// The most `json_items` streams that can be open at the same time.
pub const STREAMS_LIMIT: usize = 8;
/// The longest plugin path, in bytes.
pub const PATH_LIMIT: usize = 4096;

static STREAMS: AtomicUsize = AtomicUsize::new(0);

/// The directories that one plugin instance may read, by name.
#[derive(Debug, Clone)]
pub struct Grants {
    dirs: IndexMap<String, PathBuf>,
    root: PathBuf,
    instance: String,
}

impl Grants {
    /// Makes the grants of `instance`: the configured directories, and its file area in the
    /// project at `root`, which is made on first use.
    pub fn new(dirs: IndexMap<String, PathBuf>, root: &Path, instance: &str) -> Self {
        Self {
            dirs,
            root: root.to_path_buf(),
            instance: instance.to_string(),
        }
    }

    fn open(&self, path: &str, directory: bool) -> Result<OwnedFd, String> {
        if path.len() > PATH_LIMIT {
            return Err(format!("a path has at most {PATH_LIMIT} bytes"));
        }
        let shown = shown(path);
        let mut parts = path.split('/');
        let name = parts.next().unwrap_or_default();
        let base = match name {
            "cache" => crate::cache::plugin_files_dir(&self.root, &self.instance)?,
            name => self
                .dirs
                .get(name)
                .cloned()
                .ok_or_else(|| format!("`{}` is not a directory that this plugin may read", self::shown(name)))?,
        };
        let rest: Vec<&str> = parts.collect();
        if rest.iter().any(|part| part.is_empty() || *part == "." || *part == ".." || part.contains('\0')) {
            return Err(format!("`{shown}` must be a plain path inside `{name}`: no empty parts, `.` or `..`"));
        }
        let failed = |error: rustix::io::Errno| match error {
            rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR if !rest.is_empty() => {
                format!("cannot open `{shown}`: a part is a link or not a directory")
            },
            error => format!("cannot open `{shown}`: {}", std::io::Error::from(error)),
        };
        let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
        let mut fd = rustix::fs::open(&base, directory_flags, Mode::empty()).map_err(failed)?;
        for (index, part) in rest.iter().enumerate() {
            let last = index + 1 == rest.len();
            let flags = if last && !directory {
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC
            } else {
                directory_flags | OFlags::NOFOLLOW
            };
            fd = rustix::fs::openat(&fd, *part, flags, Mode::empty()).map_err(failed)?;
        }
        let kind = FileType::from_raw_mode(rustix::fs::fstat(&fd).map_err(failed)?.st_mode);
        match (directory, kind) {
            (true, FileType::Directory) | (false, FileType::RegularFile) => Ok(fd),
            (true, _) => Err(format!("`{shown}` is not a directory")),
            (false, _) => Err(format!("`{shown}` is not a regular file")),
        }
    }

    fn file(&self, path: &str) -> Result<std::fs::File, String> {
        Ok(std::fs::File::from(self.open(path, false)?))
    }
}

fn shown(path: &str) -> String {
    match path.char_indices().nth(200) {
        Some((end, _)) => format!("{}…", &path[..end]),
        None => path.to_string(),
    }
}

/// Reads a whole file, at most [`READ_LIMIT`] bytes.
///
/// It fails when the file is larger, or when its text holds a secret value.
pub fn read(grants: &Grants, path: &str) -> Result<Vec<u8>, String> {
    let file = grants.file(path)?;
    let mut data = Vec::new();
    file.take(READ_LIMIT + 1)
        .read_to_end(&mut data)
        .map_err(|error| format!("cannot read `{}`: {error}", shown(path)))?;
    if data.len() as u64 > READ_LIMIT {
        return Err(format!("`{}` is larger than {READ_LIMIT} bytes: use `janky.fs.json_items`", shown(path)));
    }
    if crate::diagnostic::holds_secret(&String::from_utf8_lossy(&data)) {
        return Err(format!("`{}` holds a secret value, so janky does not give it to the plugin", shown(path)));
    }
    Ok(data)
}

/// Lists a directory: the names, sorted, with `/` after the name of a directory. Names that are
/// not UTF-8 are left out, because a plugin could not open them.
pub fn list(grants: &Grants, path: &str) -> Result<Vec<String>, String> {
    let fd = grants.open(path, true)?;
    let failed = |error: rustix::io::Errno| format!("cannot list `{}`: {error}", shown(path));
    let mut dir = rustix::fs::Dir::read_from(&fd).map_err(failed)?;
    let mut names = Vec::new();
    while let Some(entry) = dir.read() {
        let entry = entry.map_err(failed)?;
        let Ok(name) = entry.file_name().to_str() else { continue };
        if name == "." || name == ".." {
            continue;
        }
        if names.len() >= ENTRY_LIMIT {
            return Err(format!("`{}` has more than {ENTRY_LIMIT} entries", shown(path)));
        }
        let kind = match entry.file_type() {
            FileType::Unknown => rustix::fs::statat(&fd, entry.file_name(), rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
                .map(|stat| FileType::from_raw_mode(stat.st_mode))
                .unwrap_or(FileType::Unknown),
            kind => kind,
        };
        names.push(if kind == FileType::Directory { format!("{name}/") } else { name.to_string() });
    }
    names.sort();
    Ok(names)
}

/// Lists the file entries of a zip file, in the order of the archive.
pub fn zip_entries(grants: &Grants, path: &str) -> Result<Vec<String>, String> {
    let archive =
        open_zip(grants.file(path)?).map_err(|error| format!("cannot read the zip file `{}`: {error}", shown(path)))?;
    let mut names = Vec::new();
    for name in archive.file_names() {
        let name = name.map_err(|error| format!("cannot read the zip file `{}`: {error}", shown(path)))?;
        if !name.ends_with('/') {
            names.push(name.into_owned());
        }
    }
    Ok(names)
}

/// Opens a zip file after it checks the number of entries in the end record, so that a changed
/// file cannot make the zip crate reserve memory for millions of entries.
fn open_zip(file: std::fs::File) -> Result<zip::ZipArchive<BufReader<std::fs::File>>, String> {
    let count = zip_entry_count(&file)?;
    if count > ENTRY_LIMIT as u64 {
        return Err(format!("it has more than {ENTRY_LIMIT} entries"));
    }
    zip::ZipArchive::new(BufReader::new(file)).map_err(|error| error.to_string())
}

fn zip_entry_count(file: &std::fs::File) -> Result<u64, String> {
    const END: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const LOCATOR: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
    const END64: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    let tail_length = length.min(65_557);
    let mut tail = vec![0u8; tail_length as usize];
    file.read_exact_at(&mut tail, length - tail_length).map_err(|error| error.to_string())?;
    let at = tail
        .windows(4)
        .rposition(|window| window == END)
        .filter(|at| at + 22 <= tail.len())
        .ok_or("it is not a zip file")?;
    let count = u16::from_le_bytes([tail[at + 10], tail[at + 11]]);
    if count != u16::MAX {
        return Ok(count.into());
    }
    let locator = at.checked_sub(20).map(|start| &tail[start..at]).filter(|locator| locator[..4] == LOCATOR);
    let locator = locator.ok_or("its Zip64 end record is missing")?;
    let offset = u64::from_le_bytes(locator[8..16].try_into().expect("8 bytes"));
    let mut record = [0u8; 40];
    file.read_exact_at(&mut record, offset).map_err(|error| error.to_string())?;
    if record[..4] != END64 {
        return Err("its Zip64 end record is broken".into());
    }
    Ok(u64::from_le_bytes(record[32..40].try_into().expect("8 bytes")))
}

/// One message of a [`json_items`] stream.
#[derive(Debug)]
pub enum Message {
    /// The next item of the array.
    Item(Json),
    /// The array ended. A stream that closes without this message stopped early.
    End,
    /// The error that ended the stream.
    Error(String),
}

/// Streams the items of the JSON array at `pointer` in a file, or in the zip entry `entry` of a
/// file, and gives back the receiving end.
///
/// The file opens before this returns, so a wrong path is an error at once. A thread parses the
/// JSON and sends each item, then [`Message::End`]. It stops when the receiver is dropped, at the
/// end of the array, or at the first error: bad JSON, a pointer that does not lead to an array,
/// more than [`STREAM_LIMIT`] bytes, an item larger than [`ITEM_LIMIT`], or an item that holds a
/// secret value. The pointer holds object keys only, as in JSON Pointer (`~1` is `/`, `~0` is
/// `~`); the empty pointer is the whole document. At most [`STREAMS_LIMIT`] streams run at once.
pub fn json_items(
    grants: &Grants,
    path: &str,
    pointer: &str,
    entry: Option<String>,
) -> Result<tokio::sync::mpsc::Receiver<Message>, String> {
    let keys = parse_pointer(pointer)?;
    let file = grants.file(path)?;
    let shown = match &entry {
        Some(entry) => format!("`{}` in `{}`", self::shown(entry), self::shown(path)),
        None => format!("`{}`", self::shown(path)),
    };
    let source = match entry {
        Some(entry) => {
            let archive = open_zip(file).map_err(|error| format!("cannot read the zip file {shown}: {error}"))?;
            Source::Zip(archive, entry)
        },
        None => Source::File(file),
    };
    if STREAMS.fetch_add(1, Ordering::SeqCst) >= STREAMS_LIMIT {
        STREAMS.fetch_sub(1, Ordering::SeqCst);
        return Err(format!("at most {STREAMS_LIMIT} `json_items` streams can be open at the same time"));
    }
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let spawned = std::thread::Builder::new().name("janky-json".into()).spawn(move || {
        struct Open;
        impl Drop for Open {
            fn drop(&mut self) {
                STREAMS.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _open = Open;
        let run = std::panic::AssertUnwindSafe(|| match source {
            Source::File(file) => stream(file, &keys, &sender),
            Source::Zip(mut archive, entry) => match archive.by_name(&entry) {
                Ok(file) => stream(file, &keys, &sender),
                Err(error) => Err(error.to_string()),
            },
        });
        let message = match std::panic::catch_unwind(run) {
            Ok(Ok(())) => Message::End,
            Ok(Err(message)) => Message::Error(format!("cannot read the JSON items of {shown}: {message}")),
            Err(_) => Message::Error(format!("the reader of {shown} stopped unexpectedly")),
        };
        let _ = sender.blocking_send(message);
    });
    if let Err(error) = spawned {
        STREAMS.fetch_sub(1, Ordering::SeqCst);
        return Err(format!("cannot start a thread for `json_items`: {error}"));
    }
    Ok(receiver)
}

enum Source {
    File(std::fs::File),
    Zip(zip::ZipArchive<BufReader<std::fs::File>>, String),
}

fn parse_pointer(pointer: &str) -> Result<Vec<String>, String> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    let Some(rest) = pointer.strip_prefix('/') else {
        return Err(format!("`{}` is not a JSON Pointer: it must be empty or start with `/`", shown(pointer)));
    };
    Ok(rest.split('/').map(|key| key.replace("~1", "/").replace("~0", "~")).collect())
}

struct Counted<'a, R> {
    inner: R,
    total: u64,
    item: &'a Cell<u64>,
}

impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.total += read as u64;
        self.item.set(self.item.get() + read as u64);
        if self.total > STREAM_LIMIT {
            return Err(std::io::Error::other(format!("more than {STREAM_LIMIT} bytes")));
        }
        if self.item.get() > ITEM_LIMIT {
            return Err(std::io::Error::other(format!("an item is larger than {ITEM_LIMIT} bytes")));
        }
        Ok(read)
    }
}

/// The state of one parse: whether the array was found, and what ended the parse early (the
/// array is complete, the receiver is gone, or an item holds a secret value).
#[derive(Default)]
struct Stop {
    found: bool,
    done: bool,
    gone: bool,
    secret: bool,
}

fn stream(reader: impl Read, keys: &[String], sender: &tokio::sync::mpsc::Sender<Message>) -> Result<(), String> {
    let item = Cell::new(0);
    let counted = Counted { inner: reader, total: 0, item: &item };
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::with_capacity(64 * 1024, counted));
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    let mut stop = Stop::default();
    let mut send = |value: Json| -> Result<(), &'static str> {
        item.set(0);
        if crate::diagnostic::holds_secret(&value.to_string()) {
            return Err("secret");
        }
        sender.blocking_send(Message::Item(value)).map_err(|_| "gone")
    };
    let result = (Items {
        keys: &keys,
        send: &mut send,
        stop: &mut stop,
        item: &item,
    })
    .deserialize(&mut deserializer);
    match result {
        _ if stop.secret => Err("an item holds a secret value, so janky does not give it to the plugin".into()),
        _ if stop.done || stop.gone => Ok(()),
        Ok(()) if !stop.found => Err("the pointer does not lead to an array".into()),
        Ok(()) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

struct Items<'a, F> {
    keys: &'a [&'a str],
    send: &'a mut F,
    stop: &'a mut Stop,
    item: &'a Cell<u64>,
}

impl<'de, F: FnMut(Json) -> Result<(), &'static str>> DeserializeSeed<'de> for Items<'_, F> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        if self.keys.is_empty() { deserializer.deserialize_seq(self) } else { deserializer.deserialize_map(self) }
    }
}

impl<'de, F: FnMut(Json) -> Result<(), &'static str>> Visitor<'de> for Items<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(if self.keys.is_empty() { "an array" } else { "an object" })
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            if key == self.keys[0] && !self.stop.found {
                map.next_value_seed(Items {
                    keys: &self.keys[1..],
                    send: &mut *self.send,
                    stop: &mut *self.stop,
                    item: self.item,
                })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        self.stop.found = true;
        self.item.set(0);
        while let Some(item) = seq.next_element::<Json>()? {
            match (self.send)(item) {
                Ok(()) => {},
                Err("secret") => {
                    self.stop.secret = true;
                    return Err(serde::de::Error::custom("stopped"));
                },
                Err(_) => {
                    self.stop.gone = true;
                    return Err(serde::de::Error::custom("stopped"));
                },
            }
        }
        self.stop.done = true;
        Err(serde::de::Error::custom("stopped"))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn setup() -> (tempfile::TempDir, Grants) {
        let dir = tempfile::tempdir().unwrap();
        let dict = dir.path().join("dict");
        std::fs::create_dir_all(dict.join("sub")).unwrap();
        std::fs::write(dict.join("a.txt"), "⚛️🧪").unwrap();
        std::fs::write(dict.join("sub/b.json"), r#"{"x": {"items": [1, {"k": "v"}, "t"]}, "y": 2}"#).unwrap();
        std::fs::write(dir.path().join("secret.txt"), "outside").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), dict.join("out")).unwrap();
        std::os::unix::fs::symlink(dict.join("a.txt"), dict.join("in")).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(dict.join("dump.zip")).unwrap());
        zip.add_directory("data/", zip::write::SimpleFileOptions::default()).unwrap();
        zip.start_file("data/1_20260819.json", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"{"L": {"E": [{"w": "a"}, {"w": "b"}]}}"#).unwrap();
        zip.finish().unwrap();
        let grants = Grants::new(IndexMap::from([("dict".to_string(), dict)]), dir.path(), "voice");
        (dir, grants)
    }

    fn items(grants: &Grants, path: &str, pointer: &str, entry: Option<&str>) -> Result<Vec<Json>, String> {
        let mut receiver = json_items(grants, path, pointer, entry.map(str::to_string))?;
        let mut items = Vec::new();
        loop {
            match receiver.blocking_recv() {
                Some(Message::Item(item)) => items.push(item),
                Some(Message::End) => return Ok(items),
                Some(Message::Error(message)) => return Err(message),
                None => return Err("the stream stopped early".into()),
            }
        }
    }

    #[test]
    fn paths_stay_inside_their_directory() {
        let (_dir, grants) = setup();
        assert_eq!(read(&grants, "dict/a.txt").unwrap(), "⚛️🧪".as_bytes());
        assert_eq!(list(&grants, "dict").unwrap(), ["a.txt", "dump.zip", "in", "out", "sub/"]);
        assert_eq!(list(&grants, "dict/sub").unwrap(), ["b.json"]);
        for (path, message) in [
            ("dict/../secret.txt", "must be a plain path"),
            ("dict//a.txt", "must be a plain path"),
            ("dict/./a.txt", "must be a plain path"),
            ("other/a.txt", "not a directory that this plugin may read"),
            ("dict/out", "cannot open"),
            ("dict/in", "cannot open"),
            ("dict/sub", "not a regular file"),
            ("dict/missing.txt", "cannot open"),
        ] {
            let error = read(&grants, path).unwrap_err();
            assert!(error.contains(message), "{path}: {error}");
        }
        assert!(list(&grants, "dict/a.txt").is_err());
    }

    #[test]
    fn fifos_and_large_files_are_refused() {
        let (dir, grants) = setup();
        let fifo = dir.path().join("dict/fifo");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        assert!(read(&grants, "dict/fifo").unwrap_err().contains("not a regular file"));
        std::fs::write(dir.path().join("dict/big.bin"), vec![0u8; READ_LIMIT as usize + 1]).unwrap();
        assert!(read(&grants, "dict/big.bin").unwrap_err().contains("larger than"));
    }

    #[test]
    fn json_items_stream_from_files_and_zip_entries() {
        let (_dir, grants) = setup();
        assert_eq!(zip_entries(&grants, "dict/dump.zip").unwrap(), ["data/1_20260819.json"]);
        assert_eq!(
            items(&grants, "dict/sub/b.json", "/x/items", None).unwrap(),
            [
                serde_json::json!(1),
                serde_json::json!({ "k": "v" }),
                serde_json::json!("t")
            ]
        );
        assert_eq!(
            items(&grants, "dict/dump.zip", "/L/E", Some("data/1_20260819.json")).unwrap(),
            [serde_json::json!({ "w": "a" }), serde_json::json!({ "w": "b" })]
        );
        assert!(items(&grants, "dict/sub/b.json", "/y", None).unwrap_err().contains("expected an array"));
        assert!(items(&grants, "dict/sub/b.json", "/nothing", None).unwrap_err().contains("does not lead to an array"));
        assert!(items(&grants, "dict/a.txt", "", None).unwrap_err().contains("cannot read the JSON items"));
        assert!(items(&grants, "dict/dump.zip", "/L/E", Some("missing.json")).is_err());
        assert!(items(&grants, "dict/sub/b.json", "x", None).unwrap_err().contains("not a JSON Pointer"));
    }

    #[test]
    fn secrets_never_reach_the_plugin() {
        let (dir, grants) = setup();
        crate::diagnostic::redact("fs-secret-0123456789", "api_key");
        std::fs::write(dir.path().join("dict/key.txt"), "x fs-secret-0123456789").unwrap();
        assert!(read(&grants, "dict/key.txt").unwrap_err().contains("holds a secret value"));
        std::fs::write(dir.path().join("dict/items.json"), r#"[1, {"k": "fs-secret-0123456789"}]"#).unwrap();
        assert!(items(&grants, "dict/items.json", "", None).unwrap_err().contains("holds a secret value"));
    }

    #[test]
    fn data_after_the_array_does_not_count_as_an_item() {
        let (dir, grants) = setup();
        let text = format!(r#"{{"a": [1, 2], "z": "{}"}}"#, "a".repeat(2 * ITEM_LIMIT as usize));
        std::fs::write(dir.path().join("dict/trailer.json"), text).unwrap();
        assert_eq!(
            items(&grants, "dict/trailer.json", "/a", None).unwrap(),
            [serde_json::json!(1), serde_json::json!(2)]
        );
    }

    #[test]
    fn zip_files_with_too_many_entries_and_long_paths_are_refused() {
        let (dir, grants) = setup();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x50, 0x4b, 0x06, 0x06]);
        data.extend_from_slice(&[0u8; 28]);
        data.extend_from_slice(&1_000_000_000u64.to_le_bytes());
        data.extend_from_slice(&[0u8; 16]);
        data.extend_from_slice(&[0x50, 0x4b, 0x06, 0x07, 0, 0, 0, 0]);
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&[1, 0, 0, 0]);
        data.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
        data.extend_from_slice(&[0u8; 10]);
        std::fs::write(dir.path().join("dict/bomb.zip"), data).unwrap();
        let error = zip_entries(&grants, "dict/bomb.zip").unwrap_err();
        assert!(error.contains("more than 100000 entries"), "{error}");
        assert!(
            read(&grants, &format!("dict/{}", "a/".repeat(PATH_LIMIT))).unwrap_err().contains("at most 4096 bytes")
        );
    }

    #[test]
    fn one_item_is_limited() {
        let (dir, grants) = setup();
        let huge = format!(r#"[1, "{}", 3]"#, "a".repeat(ITEM_LIMIT as usize + 1024 * 1024));
        std::fs::write(dir.path().join("dict/huge.json"), huge).unwrap();
        let Err(error) = items(&grants, "dict/huge.json", "", None) else {
            panic!("no error")
        };
        assert!(error.contains("an item is larger than"), "{error}");
    }
}
