//! Fetches remote note types from Git repositories and archive URLs.
//!
//! [`git`] keeps a bare clone of each repository and writes the files of one commit to a
//! checkout directory. [`archive`] downloads a `.tar.gz` or `.zip` file, checks its SHA-256 and
//! unpacks it. Both work in the user's cache directory ([`cache_root`]), not in the project.
//! Each [`Checkout`] directory is named after its commit or hash. janky does not write it
//! again once it exists, so later builds use it as it is.
//!
//! Directories are first written under a temporary name and then renamed. With `offline`, both
//! functions use only what is in the cache.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use sha2::{Digest, Sha256};

/// The largest archive that [`archive`] downloads or reads, in bytes.
pub const ARCHIVE_LIMIT: u64 = 5_000_000_000;

/// The part of a Git repository that a note type source asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reference<'a> {
    /// A tag name, without `refs/tags/`.
    Tag(&'a str),
    /// A branch name, without `refs/heads/`.
    Branch(&'a str),
    /// Any revision that Git can parse, such as a full or short commit id.
    Rev(&'a str),
}

impl Reference<'_> {
    fn spec(&self) -> String {
        match self {
            Reference::Tag(tag) => format!("refs/tags/{tag}^{{commit}}"),
            Reference::Branch(branch) => format!("refs/heads/{branch}^{{commit}}"),
            Reference::Rev(rev) => format!("{rev}^{{commit}}"),
        }
    }
}

/// The files of a fetched source, ready to read.
#[derive(Debug)]
pub struct Checkout {
    /// The directory with the files.
    pub dir: PathBuf,
    /// The full commit id of a Git source, or the SHA-256 of an archive, in hex.
    pub id: String,
}

/// Returns the cache directory for sources: `janky/sources` in `$XDG_CACHE_HOME`, else in
/// `$HOME/.cache`, else in `.cache` relative to the current directory.
pub fn cache_root() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("janky").join("sources")
}

/// Returns the directory of the bare clone of `url`, named after the first 8 bytes of the
/// SHA-256 of the URL, in hex.
pub fn repo_dir(cache: &Path, url: &str) -> PathBuf {
    let name: String = Sha256::digest(url.as_bytes())[..8].iter().map(|byte| format!("{byte:02x}")).collect();
    cache.join("git").join(name)
}

const OFFLINE: &str = "is not in the cache. Run janky without --offline";

/// Gets the files of a commit of a Git repository.
///
/// When `pinned` (a full commit id, usually from `deck.lock`) is given, that commit is used and
/// `reference` is ignored. The repository is fetched only when the commit is not in the cache.
/// Without `pinned`, the repository is fetched (unless it was just cloned or `offline` is set)
/// and `reference` is resolved to a commit. A fetch updates all branches and tags.
///
/// The checkout holds the regular files and directories of the commit. Links, submodules and
/// entries named `.git` are left out. The error is a message for the author. It comes back when
/// the clone or the fetch fails, the reference or commit does not exist, or `offline` is set and
/// the repository or the pinned commit is not in the cache.
pub fn git(
    url: &str,
    reference: Reference,
    pinned: Option<&str>,
    cache: &Path,
    offline: bool,
) -> Result<Checkout, String> {
    let repo_dir = repo_dir(cache, url);
    let mut fetched = offline;
    let repo = if repo_dir.is_dir() {
        gix::open(&repo_dir).map_err(|error| format!("cannot open the cached repository: {error}"))?
    } else if offline {
        return Err(format!("`{url}` {OFFLINE}"));
    } else {
        fetched = true;
        clone(url, &repo_dir)?
    };
    let pinned =
        pinned.map(|hex| gix::ObjectId::from_hex(hex.as_bytes())).transpose().map_err(|error| error.to_string())?;
    let commit = match pinned {
        Some(id) if repo.has_object(id) => id,
        Some(id) if offline => return Err(format!("commit {id} {OFFLINE}")),
        Some(id) if fetched => return Err(format!("the repository has no commit {id}")),
        Some(id) => {
            fetch(&repo)?;
            if !repo.has_object(id) {
                return Err(format!("the repository has no commit {id}"));
            }
            id
        },
        None => {
            if !fetched {
                fetch(&repo)?;
            }
            resolve(&repo, reference)?
        },
    };
    let dir = extract(&repo, commit, cache)?;
    Ok(Checkout { dir, id: commit.to_string() })
}

fn clone(url: &str, repo_dir: &Path) -> Result<gix::Repository, String> {
    let parent = repo_dir.parent().expect("cache directory");
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = temporary_path(repo_dir);
    let interrupt = AtomicBool::new(false);
    let result = gix::prepare_clone_bare(url, &temporary).map_err(|error| error.to_string()).and_then(|mut prepare| {
        prepare.fetch_only(gix::progress::Discard, &interrupt).map_err(|error| error.to_string()).map(|_| ())
    });
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err(format!("cannot clone `{url}`: {error}"));
    }
    std::fs::rename(&temporary, repo_dir).map_err(|error| error.to_string())?;
    gix::open(repo_dir).map_err(|error| error.to_string())
}

fn fetch(repo: &gix::Repository) -> Result<(), String> {
    let interrupt = AtomicBool::new(false);
    let mut remote = repo
        .find_default_remote(gix::remote::Direction::Fetch)
        .ok_or("the cached repository has no remote")?
        .map_err(|error| error.to_string())?;
    remote
        .replace_refspecs(["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"], gix::remote::Direction::Fetch)
        .map_err(|error| error.to_string())?;
    let result: Result<(), Box<dyn std::error::Error>> = (|| {
        remote
            .connect(gix::remote::Direction::Fetch)?
            .prepare_fetch(gix::progress::Discard, Default::default())?
            .receive(gix::progress::Discard, &interrupt)?;
        Ok(())
    })();
    result.map_err(|error| {
        let mut message = format!("cannot fetch: {error}");
        let mut cause = error.source();
        while let Some(inner) = cause {
            message.push_str(&format!(": {inner}"));
            cause = inner.source();
        }
        message
    })
}

fn resolve(repo: &gix::Repository, reference: Reference) -> Result<gix::ObjectId, String> {
    let what = match reference {
        Reference::Tag(tag) => format!("tag `{tag}`"),
        Reference::Branch(branch) => format!("branch `{branch}`"),
        Reference::Rev(rev) => format!("commit `{rev}`"),
    };
    repo.rev_parse_single(reference.spec().as_str())
        .map(|id| id.detach())
        .map_err(|_| format!("the repository has no {what}"))
}

fn extract(repo: &gix::Repository, commit: gix::ObjectId, cache: &Path) -> Result<PathBuf, String> {
    let dir = cache.join("checkouts").join("git").join(commit.to_string());
    if dir.is_dir() {
        return Ok(dir);
    }
    std::fs::create_dir_all(dir.parent().expect("checkouts directory")).map_err(|error| error.to_string())?;
    let temporary = temporary_path(&dir);
    let tree = repo
        .find_commit(commit)
        .and_then(|commit| commit.tree_id().map(|id| id.detach()))
        .map_err(|error| error.to_string())?;
    if let Err(error) = write_tree(repo, tree, &temporary) {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, &dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn write_tree(repo: &gix::Repository, tree: gix::ObjectId, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let tree = repo.find_tree(tree).map_err(|error| error.to_string())?;
    for entry in tree.iter() {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.filename().to_string();
        if name.is_empty() || name == "." || name == ".." || name == ".git" || name.contains(['/', '\\', '\0']) {
            continue;
        }
        let mode = entry.mode();
        let id = entry.oid().to_owned();
        if mode.is_tree() {
            write_tree(repo, id, &dir.join(&name))?;
        } else if mode.is_blob() {
            let data = repo.find_object(id).map_err(|error| error.to_string())?.detach().data;
            std::fs::write(dir.join(&name), data).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// The archive formats that a `url` source can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    /// A gzip-compressed tar file (`.tar.gz` or `.tgz`).
    TarGz,
    /// A zip file (`.zip`).
    Zip,
}

/// Returns the archive format from the end of the URL path, ignoring case. `None` when the
/// path does not end in `.tar.gz`, `.tgz` or `.zip`.
pub fn archive_kind(url: &str) -> Option<ArchiveKind> {
    let path = url.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    if path.ends_with(".tar.gz") || path.ends_with(".tgz") {
        Some(ArchiveKind::TarGz)
    } else if path.ends_with(".zip") {
        Some(ArchiveKind::Zip)
    } else {
        None
    }
}

/// Gets the files of an archive whose SHA-256 is `sha256` (64 lower-case hex digits).
///
/// It uses the checkout when it exists, then a cached archive with the right hash, and only
/// then downloads the URL. A downloaded archive is kept only when its hash matches. Unpacking
/// writes only regular files and directories. It skips links, and entries that are named
/// `.git` or are inside one.
///
/// # Errors
///
/// - The URL does not name a known archive format.
/// - `offline` is set and neither the checkout nor the archive is in the cache.
/// - The download fails, or the archive has a different SHA-256.
/// - The archive is larger than [`ARCHIVE_LIMIT`], or a redirect goes to a URL that is not
///   `https://`.
/// - An archive entry has a path that leaves the archive, or the archive cannot be read.
/// - A file system operation in the cache fails.
pub fn archive(url: &str, sha256: &str, cache: &Path, offline: bool) -> Result<Checkout, String> {
    let kind = archive_kind(url).ok_or("the URL must end in `.tar.gz`, `.tgz` or `.zip`")?;
    let extension = if kind == ArchiveKind::Zip { "zip" } else { "tar.gz" };
    let file = cache.join("archives").join(format!("{sha256}.{extension}"));
    let dir = cache.join("checkouts").join("archives").join(sha256);
    if dir.is_dir() {
        return Ok(Checkout { dir, id: sha256.to_string() });
    }
    let cached = std::fs::File::open(&file).ok().and_then(|mut cached| copy_hashed(&mut cached, std::io::sink()).ok());
    match cached {
        Some(digest) if digest == sha256 => {},
        _ if offline => return Err(format!("`{url}` {OFFLINE}")),
        _ => download(url, sha256, &file)?,
    }
    let archive = std::fs::File::open(&file).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(dir.parent().expect("checkouts directory")).map_err(|error| error.to_string())?;
    let temporary = temporary_path(&dir);
    let result = match kind {
        ArchiveKind::TarGz => unpack_tar(archive, &temporary),
        ArchiveKind::Zip => unpack_zip(archive, &temporary),
    };
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, &dir).map_err(|error| error.to_string())?;
    Ok(Checkout { dir, id: sha256.to_string() })
}

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Copies `reader` to `out` and gives back the SHA-256 of what it copied. It fails when there
/// is more than [`ARCHIVE_LIMIT`].
fn copy_hashed(reader: &mut impl Read, mut out: impl Write) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut total = 0;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > ARCHIVE_LIMIT {
            return Err(std::io::Error::other(format!("the archive is larger than {ARCHIVE_LIMIT} bytes")));
        }
        hasher.update(&buffer[..read]);
        out.write_all(&buffer[..read])?;
    }
    out.flush()?;
    Ok(hex(hasher.finalize()))
}

fn download(url: &str, sha256: &str, file: &Path) -> Result<(), String> {
    let failed = |error: &dyn std::fmt::Display| format!("cannot download `{url}`: {error}");
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                attempt.error("a redirect must go to an `https://` URL")
            } else if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|error| failed(&error))?;
    let mut response = client.get(url).send().map_err(|error| failed(&crate::http::describe(error)))?;
    let status = response.status();
    if !status.is_success() {
        return Err(failed(&format!("the server answered {status}")));
    }
    if response.content_length().is_some_and(|length| length > ARCHIVE_LIMIT) {
        return Err(failed(&format!("the archive is larger than {ARCHIVE_LIMIT} bytes")));
    }
    std::fs::create_dir_all(file.parent().expect("archives directory")).map_err(|error| error.to_string())?;
    let temporary = temporary_path(file);
    let written =
        std::fs::File::create(&temporary).map(std::io::BufWriter::new).and_then(|out| copy_hashed(&mut response, out));
    let actual = match written {
        Ok(actual) => actual,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(failed(&error));
        },
    };
    if actual != sha256 {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("the archive has SHA-256 {actual}, not {sha256}"));
    }
    std::fs::rename(&temporary, file).map_err(|error| error.to_string())
}

fn safe_path(name: &str) -> Result<PathBuf, String> {
    let mut path = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            std::path::Component::Normal(part) if part != ".git" => path.push(part),
            std::path::Component::Normal(_) => return Ok(PathBuf::new()),
            std::path::Component::CurDir => {},
            _ => return Err(format!("the archive entry `{name}` leaves the archive")),
        }
    }
    Ok(path)
}

fn unpack_tar(file: std::fs::File, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(std::io::BufReader::new(file)));
    for entry in archive.entries().map_err(|error| format!("cannot read the archive: {error}"))? {
        let mut entry = entry.map_err(|error| format!("cannot read the archive: {error}"))?;
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let path = safe_path(&name)?;
        let kind = entry.header().entry_type();
        if path.as_os_str().is_empty() || !(kind.is_file() || kind.is_dir()) {
            continue;
        }
        let target = dir.join(&path);
        if kind.is_dir() {
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        std::fs::create_dir_all(target.parent().expect("parent")).map_err(|error| error.to_string())?;
        let mut out = std::fs::File::create(&target).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|error| format!("cannot read the archive: {error}"))?;
    }
    Ok(())
}

fn unpack_zip(file: std::fs::File, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|error| format!("cannot read the archive: {error}"))?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| format!("cannot read the archive: {error}"))?;
        let name = file.name().map_err(|error| format!("cannot read the archive: {error}"))?.into_owned();
        let path = safe_path(&name)?;
        if path.as_os_str().is_empty() || file.is_symlink() {
            continue;
        }
        let target = dir.join(&path);
        if file.is_dir() {
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        std::fs::create_dir_all(target.parent().expect("parent")).map_err(|error| error.to_string())?;
        let mut out = std::fs::File::create(&target).map_err(|error| error.to_string())?;
        std::io::copy(&mut file, &mut out).map_err(|error| format!("cannot read the archive: {error}"))?;
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.tmp-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_cmd(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn repository(root: &Path, format: &str) -> (String, PathBuf) {
        let work = root.join(format!("{format}-work"));
        std::fs::create_dir_all(work.join("elements")).unwrap();
        git_cmd(&work, &["init", "-q", &format!("--object-format={format}")]);
        std::fs::write(work.join("elements/notetype.toml"), "name = \"⚛️ Element\"\n").unwrap();
        git_cmd(&work, &["add", "."]);
        git_cmd(&work, &["commit", "-qm", "one"]);
        git_cmd(&work, &["tag", "v1"]);
        (format!("file://{}", work.display()), work)
    }

    #[test]
    fn tags_branches_and_revs_resolve_to_commits() {
        let root = tempfile::tempdir().unwrap();
        let (url, work) = repository(root.path(), "sha1");
        let head = git_cmd(&work, &["rev-parse", "HEAD"]);
        let cache = root.path().join("cache");
        for reference in [
            Reference::Tag("v1"),
            Reference::Branch("main"),
            Reference::Rev(&head[..8]),
        ] {
            let checkout = git(&url, reference, None, &cache, false).unwrap();
            assert_eq!(checkout.id, head);
            let text = std::fs::read_to_string(checkout.dir.join("elements/notetype.toml")).unwrap();
            assert_eq!(text, "name = \"⚛️ Element\"\n");
        }
        let error = git(&url, Reference::Tag("v9"), None, &cache, false).unwrap_err();
        assert_eq!(error, "the repository has no tag `v9`");
    }

    #[test]
    fn a_pinned_commit_needs_no_remote_and_a_fetch_finds_new_tags() {
        let root = tempfile::tempdir().unwrap();
        let (url, work) = repository(root.path(), "sha1");
        let cache = root.path().join("cache");
        let first = git(&url, Reference::Tag("v1"), None, &cache, false).unwrap();

        std::fs::write(work.join("elements/notetype.toml"), "name = \"⚛️ Element 2\"\n").unwrap();
        git_cmd(&work, &["commit", "-qam", "two"]);
        git_cmd(&work, &["tag", "v2"]);
        let second = git(&url, Reference::Tag("v2"), None, &cache, false).unwrap();
        assert_ne!(second.id, first.id);
        assert_eq!(
            std::fs::read_to_string(second.dir.join("elements/notetype.toml")).unwrap(),
            "name = \"⚛️ Element 2\"\n"
        );

        std::fs::remove_dir_all(&work).unwrap();
        let pinned = git(&url, Reference::Tag("v1"), Some(&first.id), &cache, false).unwrap();
        assert_eq!(pinned.id, first.id);
        assert_eq!(
            std::fs::read_to_string(pinned.dir.join("elements/notetype.toml")).unwrap(),
            "name = \"⚛️ Element\"\n"
        );
    }

    #[test]
    fn sha256_repositories_work() {
        let root = tempfile::tempdir().unwrap();
        let (url, _) = repository(root.path(), "sha256");
        let checkout = git(&url, Reference::Tag("v1"), None, &root.path().join("cache"), false).unwrap();
        assert_eq!(checkout.id.len(), 64);
        assert!(checkout.dir.join("elements/notetype.toml").is_file());
    }

    fn serve(files: Vec<(&'static str, Vec<u8>)>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for _ in 0..files.len() {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    request.extend_from_slice(&buffer[..read]);
                }
                let line = String::from_utf8_lossy(&request).lines().next().unwrap_or_default().to_string();
                let path = line.split(' ').nth(1).unwrap_or_default().to_string();
                match files.iter().find(|(name, _)| path == format!("/{name}")) {
                    Some((_, body)) => {
                        let head =
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        stream.write_all(head.as_bytes()).unwrap();
                        stream.write_all(body).unwrap();
                    },
                    None => stream
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .unwrap(),
                }
            }
        });
        (base, handle)
    }

    fn answer(response: &'static str) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
            }
            stream.write_all(response.as_bytes()).unwrap();
        });
        (base, handle)
    }

    fn tar_gz(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
        for (name, text) in entries {
            let mut header = tar::Header::new_gnu();
            let bytes = name.as_bytes();
            header.as_old_mut().name[..bytes.len()].copy_from_slice(bytes);
            header.set_size(text.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append(&header, text.as_bytes()).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn zip_file(entries: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, text) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn archives_download_check_and_unpack() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let tar = tar_gz(&[("pack/elements/notetype.toml", "name = \"⚛️ Element\"\n")]);
        let zip = zip_file(&[("pack/elements/notetype.toml", "name = \"zip\"\n")]);
        let (tar_hash, zip_hash) = (hex(Sha256::digest(&tar)), hex(Sha256::digest(&zip)));
        let (base, server) = serve(vec![("a.tar.gz", tar), ("b.zip", zip.clone()), ("c.zip", zip)]);

        let checkout = archive(&format!("{base}/a.tar.gz"), &tar_hash, &cache, false).unwrap();
        assert_eq!(checkout.id, tar_hash);
        assert_eq!(
            std::fs::read_to_string(checkout.dir.join("pack/elements/notetype.toml")).unwrap(),
            "name = \"⚛️ Element\"\n"
        );
        let checkout = archive(&format!("{base}/b.zip"), &zip_hash, &cache, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(checkout.dir.join("pack/elements/notetype.toml")).unwrap(),
            "name = \"zip\"\n"
        );
        let wrong = "0".repeat(64);
        let error = archive(&format!("{base}/c.zip"), &wrong, &cache, false).unwrap_err();
        assert_eq!(error, format!("the archive has SHA-256 {zip_hash}, not {wrong}"));
        server.join().unwrap();

        std::fs::remove_dir_all(cache.join("checkouts")).unwrap();
        let offline = archive("http://127.0.0.1:1/a.tar.gz", &tar_hash, &cache, false).unwrap();
        assert!(offline.dir.join("pack/elements/notetype.toml").is_file());
    }

    #[test]
    fn downloads_refuse_plain_redirects_and_large_archives() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let hash = "0".repeat(64);
        let (base, server) = answer(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/a.tar.gz\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let error = archive(&format!("{base}/a.tar.gz"), &hash, &cache, false).unwrap_err();
        server.join().unwrap();
        assert!(error.ends_with("a redirect must go to an `https://` URL"), "{error}");

        let (base, server) = answer("HTTP/1.1 200 OK\r\nContent-Length: 5000000001\r\nConnection: close\r\n\r\n");
        let error = archive(&format!("{base}/a.tar.gz"), &hash, &cache, false).unwrap_err();
        server.join().unwrap();
        assert_eq!(error, format!("cannot download `{base}/a.tar.gz`: the archive is larger than 5000000000 bytes"));
        assert!(!cache.join("archives").exists());
    }

    #[test]
    fn entries_that_leave_the_archive_are_errors() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let tar = tar_gz(&[("pack/ok.toml", "x"), ("../evil.toml", "x")]);
        let zip = zip_file(&[("pack/../../evil.toml", "x")]);
        let (tar_hash, zip_hash) = (hex(Sha256::digest(&tar)), hex(Sha256::digest(&zip)));
        let (base, server) = serve(vec![("a.tar.gz", tar), ("b.zip", zip)]);
        let error = archive(&format!("{base}/a.tar.gz"), &tar_hash, &cache, false).unwrap_err();
        assert_eq!(error, "the archive entry `../evil.toml` leaves the archive");
        let error = archive(&format!("{base}/b.zip"), &zip_hash, &cache, false).unwrap_err();
        assert_eq!(error, "the archive entry `pack/../../evil.toml` leaves the archive");
        server.join().unwrap();
        assert!(!root.path().join("evil.toml").exists() && !cache.join("evil.toml").exists());
        assert!(!cache.join("checkouts").join("archives").join(&tar_hash).exists());
    }

    #[test]
    fn offline_uses_only_the_cache() {
        let root = tempfile::tempdir().unwrap();
        let (url, work) = repository(root.path(), "sha1");
        let cache = root.path().join("cache");
        let error = git(&url, Reference::Tag("v1"), None, &cache, true).unwrap_err();
        assert_eq!(error, format!("`{url}` is not in the cache. Run janky without --offline"));

        let first = git(&url, Reference::Tag("v1"), None, &cache, false).unwrap();
        std::fs::remove_dir_all(&work).unwrap();
        assert_eq!(git(&url, Reference::Tag("v1"), None, &cache, true).unwrap().id, first.id);
        let missing = "0123456789abcdef0123456789abcdef01234567";
        let error = git(&url, Reference::Tag("v1"), Some(missing), &cache, true).unwrap_err();
        assert_eq!(error, format!("commit {missing} is not in the cache. Run janky without --offline"));

        let error = archive("https://example.invalid/a.zip", &"0".repeat(64), &cache, true).unwrap_err();
        assert_eq!(error, "`https://example.invalid/a.zip` is not in the cache. Run janky without --offline");
    }
}
