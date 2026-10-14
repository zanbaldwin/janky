//! `janky publish`: puts the project into the user's collection through their sync server.
//!
//! janky keeps its own copy of the collection in [`PUBLISH_DIR`] and syncs it like one more
//! device. A run takes the lock `lock` in that directory, prepares the package like `build`,
//! logs in, pulls (a full download when the server asks), compares the package with the copy
//! ([`janky_anki::diff`]) and shows the plan. Then it asks, applies the package
//! ([`janky_anki::apply`]), pushes, and syncs the media. At the end it writes `state.json` and
//! `deck.lock`.
//!
//! `state.json` records, for the next run, the collection id of each deck and the entry id and
//! deck of each published note. A note whose deck changed since the last publish moves its
//! cards. Only notes that this record or `deck.lock` knows can be deleted, and only with
//! `--delete-removed`.
//!
//! Before `apply`, janky writes the marker `applying`. It removes the marker after the push. A
//! run that finds the marker throws the copy away and downloads the collection again, because
//! a failed `apply` can leave part of its changes in the copy, and those must never reach the
//! server.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use janky_anki::{Apply, Pushed, Session, open_copy};
use serde::{Deserialize, Serialize};

use crate::cli::{Cli, check, confirm, lock_project, missing_media, prepare, prepare_target, write_lock};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::generate::Kind;
use crate::{lock, plan, project, secrets};

/// The directory of the collection copy, relative to the project directory.
pub const PUBLISH_DIR: &str = ".janky/publish";

/// The names in [`PUBLISH_DIR`] that janky or rslib write. None of them can be a link, so that no
/// write leaves the project.
const COPY_FILES: [&str; 10] = [
    "lock",
    "applying",
    "state.json",
    "state.json.tmp",
    "collection.anki2",
    "collection.anki2-wal",
    "collection.anki2-shm",
    "collection.mdb",
    "collection.media",
    "collection.mdb-wal",
];

/// The flags of `janky publish`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Flags {
    pub yes: bool,
    pub dry_run: bool,
    pub allow_full_sync: bool,
    pub delete_removed: bool,
}

/// What the last publish recorded in `state.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// The sync server and the account of the copy. When they change, the copy and this record
    /// start again.
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    username: String,
    /// The collection deck id of each package deck id.
    decks: BTreeMap<String, i64>,
    /// The entry id and package deck id of each published note, by GUID.
    notes: BTreeMap<String, PublishedNote>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PublishedNote {
    id: String,
    deck: i64,
}

/// Runs `janky publish`. Returns the JSON plan for `--json`, once the plan exists.
pub fn publish(
    cli: &Cli,
    flags: Flags,
    options: project::Options,
    diagnostics: &mut Diagnostics,
    messages: &mut Vec<String>,
    log: &mut Vec<String>,
) -> Option<serde_json::Value> {
    let root = cli.project.parent().unwrap_or(Path::new(""));
    let dir = root.join(PUBLISH_DIR);
    let copy_path = dir.join("collection.anki2");
    if let Err(message) = prepare_target(root, &copy_path) {
        diagnostics.push(Diagnostic::error("publish::write", format!("cannot use `{}`: {message}", dir.display())));
        return None;
    }
    if let Some(name) = COPY_FILES
        .iter()
        .find(|name| std::fs::symlink_metadata(dir.join(name)).is_ok_and(|metadata| metadata.file_type().is_symlink()))
    {
        diagnostics
            .push(Diagnostic::error("publish::write", format!("`{}` is a symbolic link", dir.join(name).display())));
        return None;
    }
    let _lock = lock_copy(&dir, diagnostics)?;

    let lock_before = std::fs::read(root.join("deck.lock")).ok();
    let prepared = prepare(&cli.project, options, diagnostics, log)?;
    missing_media(&prepared.checked.rendered, !cli.allow_missing, diagnostics);
    if diagnostics.has_errors() {
        return None;
    }
    let project = &prepared.checked.project;
    let Some(config) = &project.deck.publish else {
        diagnostics.push(
            Diagnostic::error("publish::config", "deck.toml has no `[publish]` table")
                .with_help("add `[publish]` with `endpoint`, `username` and `password`"),
        );
        return None;
    };
    let password = secrets::publish_password(project, diagnostics)?;

    let mut state = read_state(&dir);
    if !state.endpoint.is_empty() && (state.endpoint != config.endpoint || state.username != config.username) {
        if let Err(error) = discard(&dir) {
            diagnostics.push(Diagnostic::error("publish::write", format!("cannot remove the old copy: {error}")));
            return None;
        }
        state = State::default();
        messages.push("The sync server or the account changed, so janky downloads the collection again.".into());
    }
    let marker = dir.join("applying");
    if std::fs::symlink_metadata(&marker).is_ok() {
        if let Err(error) = discard(&dir) {
            diagnostics.push(Diagnostic::error("publish::write", format!("cannot remove the old copy: {error}")));
            return None;
        }
        messages.push("The last publish stopped before it ended, so janky downloads the collection again.".into());
    }
    let session = match Session::login(&config.endpoint, &config.username, &password) {
        Ok(session) => session,
        Err(error) => {
            diagnostics.push(Diagnostic::error("publish::login", error.0).with_help(
                "check `username` and the password in deck.toml. Both must match `SYNC_USER<n>` on the server",
            ));
            return None;
        },
    };
    let copy = match open_copy(&copy_path) {
        Ok(copy) => copy,
        Err(error) => {
            diagnostics.push(Diagnostic::error("publish::copy", error.0));
            return None;
        },
    };
    let mut copy = match session.pull(copy) {
        Ok((copy, _)) => copy,
        Err(error) => {
            diagnostics.push(Diagnostic::error("publish::sync", error.0));
            return None;
        },
    };

    let ids = entry_ids(&state, &prepared.previous, &prepared.next);
    let request = Apply {
        decks: state.decks.iter().filter_map(|(key, id)| Some((key.parse().ok()?, *id))).collect(),
        moved: prepared
            .package
            .notes
            .iter()
            .filter(|note| state.notes.get(&note.guid).is_some_and(|known| known.deck != note.deck))
            .map(|note| note.guid.clone())
            .collect(),
        delete: Vec::new(),
    };
    let diff = match janky_anki::diff(&mut copy, &prepared.package, &request) {
        Ok(diff) => diff,
        Err(error) => {
            diagnostics.push(Diagnostic::error("publish::plan", error.0));
            return None;
        },
    };
    let json = plan::publish_json(&diff, &ids);
    messages.push(plan::publish_text(&diff, &ids, flags.delete_removed).trim_end().to_string());
    if !cli.quiet {
        for message in messages.drain(..) {
            eprintln!("{}", crate::diagnostic::visible(&message));
        }
    }
    if flags.dry_run {
        messages.push("Dry run: janky changed nothing on the server.".into());
        return Some(json);
    }
    if !diff.schema_changed.is_empty() && !flags.allow_full_sync {
        diagnostics.push(
            Diagnostic::error(
                "publish::full-sync",
                "the changes need a full sync, which replaces the collection on the server",
            )
            .with_help(
                "sync your other devices first, then run again with `--allow-full-sync`. Reviews that \
                 the server does not have yet are lost",
            ),
        );
        return Some(json);
    }
    if !diff.is_empty()
        && !confirm(
            "Publish these changes to the sync server?",
            flags.yes,
            "publish::confirm",
            "janky asks before it changes your collection, and stdin is not a terminal",
            diagnostics,
        )
    {
        return Some(json);
    }

    let mut request = request;
    if flags.delete_removed {
        request.delete = diff.notes_removed.iter().filter(|guid| ids.contains_key(*guid)).cloned().collect();
    }
    if let Err(error) = std::fs::write(&marker, b"") {
        diagnostics.push(Diagnostic::error("publish::write", format!("cannot write `{}`: {error}", marker.display())));
        return Some(json);
    }
    let applied = match janky_anki::apply(&mut copy, &prepared.package, &request) {
        Ok(applied) => applied,
        Err(error) => {
            diagnostics.push(
                Diagnostic::error("publish::apply", error.0)
                    .with_help("nothing was sent. The next publish downloads the collection again"),
            );
            return Some(json);
        },
    };
    let mut full = None;
    match session.push(&mut copy) {
        Ok(Pushed::Done) => {},
        Ok(Pushed::FullSyncRequired { server_empty }) if server_empty || flags.allow_full_sync => {
            match session.full_upload(copy) {
                Ok(uploaded) => {
                    copy = uploaded;
                    full = Some(server_empty);
                },
                Err(error) => {
                    diagnostics.push(Diagnostic::error("publish::sync", error.0));
                    return Some(json);
                },
            }
        },
        Ok(Pushed::FullSyncRequired { .. }) => {
            diagnostics.push(
                Diagnostic::error("publish::full-sync", "the server asks for a full sync, which the plan did not show")
                    .with_help(
                        "nothing was sent. Run again with `--allow-full-sync` to replace the collection on the server",
                    ),
            );
            return Some(json);
        },
        Err(error) => {
            diagnostics.push(Diagnostic::error("publish::sync", error.0));
            return Some(json);
        },
    }
    if let Err(error) = session.sync_media(&copy) {
        diagnostics.push(
            Diagnostic::error("publish::media", error.0)
                .with_help("the notes arrived. Run `janky publish` again for the media"),
        );
    }
    let _ = copy.close(None);
    let _ = std::fs::remove_file(&marker);

    // A removed entry stays in the record until it is deleted, so that a later
    // `--delete-removed` still knows that it came from the project.
    let mut notes: BTreeMap<String, PublishedNote> = diff
        .notes_removed
        .iter()
        .filter(|guid| !request.delete.contains(*guid))
        .filter_map(|guid| {
            let id = ids.get(guid)?.clone();
            let deck = state.notes.get(guid).map_or(0, |known| known.deck);
            Some((guid.clone(), PublishedNote { id, deck }))
        })
        .collect();
    for note in &prepared.package.notes {
        let id = ids.get(&note.guid).cloned().unwrap_or_default();
        notes.insert(note.guid.clone(), PublishedNote { id, deck: note.deck });
    }
    let state = State {
        endpoint: config.endpoint.clone(),
        username: config.username.clone(),
        decks: applied.decks.iter().map(|(package, collection)| (package.to_string(), *collection)).collect(),
        notes,
    };
    if let Err(error) = write_state(root, &dir, &state) {
        diagnostics.push(Diagnostic::error("publish::write", error));
    }
    let lock_path = root.join("deck.lock");
    if let Err(error) =
        prepare_target(root, &lock_path).and_then(|()| write_lock_unchanged(&lock_path, &lock_before, &prepared.next))
    {
        diagnostics
            .push(Diagnostic::error("publish::lock", format!("cannot write `{}`: {error}", lock_path.display())));
    }
    messages.push(format!(
        "Published: {} notes added, {} changed, {} deleted, {} cards moved.",
        diff.notes_added.len(),
        diff.notes_changed.len(),
        applied.deleted,
        applied.moved
    ));
    match full {
        Some(true) => messages.push(
            "The account on the server had no cards, so janky uploaded the whole collection. When another \
             device syncs, Anki asks whether to upload or to download. Download replaces the collection on \
             that device with the server's. Upload replaces the server's collection with the one on the \
             device, and the next `janky publish` adds the project again."
                .into(),
        ),
        Some(false) => messages.push(
            "janky replaced the collection on the server. On each other device, sync and choose to download \
             from the server, not to upload."
                .into(),
        ),
        None => {},
    }
    Some(json)
}

/// Runs `janky pull`: downloads the files of the pending media jobs from the sync server
/// into the media cache, with their metadata. A file must match the content hash that
/// `deck.lock` records for it, so the server cannot change it. It takes the lock of `generate` and `prune`,
/// because it writes the cache. It never replaces a cached file, and it needs no collection
/// copy.
pub fn pull_media(
    cli: &Cli,
    options: project::Options,
    diagnostics: &mut Diagnostics,
    messages: &mut Vec<String>,
    log: &mut Vec<String>,
) {
    let Some(_lock) = lock_project(&cli.project, diagnostics) else { return };
    let Some(checked) = check(&cli.project, options, diagnostics, log) else {
        return;
    };
    if diagnostics.has_errors() {
        return;
    }
    let project = &checked.project;
    let Some(config) = &project.deck.publish else {
        diagnostics.push(
            Diagnostic::error("publish::config", "deck.toml has no `[publish]` table")
                .with_help("add `[publish]` with `endpoint`, `username` and `password`"),
        );
        return;
    };
    let pending = crate::generate::pending(&checked.rendered.jobs);
    let data = pending.iter().filter(|job| job.kind == Kind::Data).count();
    let media: Vec<_> = pending.into_iter().filter(|job| job.kind == Kind::Media).collect();
    if data > 0 {
        messages.push(format!(
            "{data} data results are missing. They are not on the server: `janky generate` makes them again."
        ));
    }
    if media.is_empty() {
        messages.push("No generated media files are missing.".into());
        return;
    }
    let Some(password) = secrets::publish_password(project, diagnostics) else {
        return;
    };
    let session = match Session::login(&config.endpoint, &config.username, &password) {
        Ok(session) => session,
        Err(error) => {
            diagnostics.push(Diagnostic::error("publish::login", error.0));
            return;
        },
    };
    let names = match session.media_names() {
        Ok(names) => names,
        Err(error) => {
            diagnostics.push(Diagnostic::error("publish::media", error.0));
            return;
        },
    };
    // deck.lock records the content hash of each media file of the last build. A file that it
    // does not record cannot be checked, so janky does not take it from the server.
    let Some(locked) = lock::read(&project.root.join("deck.lock"), diagnostics) else {
        return;
    };
    let mut wanted = Vec::new();
    let mut unrecorded = 0;
    for job in &media {
        let found = names.iter().find(|name| {
            name.strip_prefix(&format!("{}.", job.hash)).is_some_and(|ext| {
                crate::generate::valid_ext(ext) && job.ext.as_deref().is_none_or(|planned| planned == ext)
            })
        });
        match found {
            Some(name) if locked.media.contains_key(name) => wanted.push((name.clone(), *job)),
            Some(_) => unrecorded += 1,
            None => {},
        }
    }
    let names: Vec<String> = wanted.iter().map(|(name, _)| name.clone()).collect();
    let mut stored = 0;
    let mut received = 0;
    let mut found = Diagnostics::default();
    let downloaded = session.download_media(&names, |name, data| {
        received += 1;
        let Some((_, job)) = wanted.iter().find(|(wanted, _)| *wanted == name) else {
            return;
        };
        let ext = name.rsplit_once('.').map_or("", |(_, ext)| ext).to_string();
        if crate::cache::lookup(&project.root, &job.hash, Some(&ext)).is_some() {
            return;
        }
        // A file without its metadata may be a paid result from a stopped run: keep it.
        if std::fs::symlink_metadata(crate::cache::media_path(&project.root, &name)).is_ok() {
            found.push(Diagnostic::warning(
                "publish::media",
                format!("`{name}` is already in the cache without its metadata, so janky did not replace it"),
            ));
            return;
        }
        if locked.media.get(&name).is_none_or(|entry| entry.hash != lock::file_hash(&data)) {
            found.push(Diagnostic::error(
                "publish::media",
                format!("`{name}` from the server does not match deck.lock, so janky did not store it"),
            ));
            return;
        }
        let meta = crate::cache::Meta {
            ext,
            instance: job.instance.clone(),
            cost: None,
        };
        match crate::cache::store(&project.root, &job.hash, &data, &meta) {
            Ok(_) => stored += 1,
            Err(message) => found.push(Diagnostic::error("publish::media", message)),
        }
    });
    diagnostics.append_sorted(found);
    match downloaded {
        Err(error) => diagnostics.push(Diagnostic::error("publish::media", error.0)),
        Ok(()) if received < names.len() => diagnostics.push(
            Diagnostic::warning(
                "publish::media",
                format!("the server listed {} media files but did not send them", names.len() - received),
            )
            .with_help("run `janky pull` again"),
        ),
        Ok(()) => {},
    }
    messages.push(format!("Pulled {stored} media files from the server."));
    if unrecorded > 0 {
        messages.push(format!(
            "{unrecorded} media files on the server are not in deck.lock, so janky cannot check them: \
             `janky generate` makes them."
        ));
    }
    let missing = media.len() - wanted.len() - unrecorded;
    if missing > 0 {
        messages.push(format!("{missing} media files are not on the server: `janky generate` makes them."));
    }
}

/// Writes `lock` to `path`, but only if the file still holds `before`, the content that the
/// run read at its start. Another command, such as `janky update` while `publish` waited for an
/// answer, may have written it since.
fn write_lock_unchanged(path: &Path, before: &Option<Vec<u8>>, lock: &lock::LockFile) -> Result<(), String> {
    if std::fs::read(path).ok() != *before {
        return Err("another command changed it while janky publish ran, so janky left it as it is. The \
                    collection is up to date: run `janky publish` again to update deck.lock"
            .into());
    }
    write_lock(path, lock).map_err(|error| error.to_string())
}

/// The entry id of each GUID that janky knows: from `state.json`, the previous lock file and
/// the next one.
fn entry_ids(state: &State, previous: &lock::LockFile, next: &lock::LockFile) -> HashMap<String, String> {
    let mut ids: HashMap<String, String> = state
        .notes
        .iter()
        .filter(|(_, note)| !note.id.is_empty())
        .map(|(guid, note)| (guid.clone(), note.id.clone()))
        .collect();
    for file in [previous, next] {
        for (id, note) in &file.notes {
            ids.insert(lock::guid(&file.namespace, &note.notetype, id), id.clone());
        }
    }
    ids
}

fn read_state(dir: &Path) -> State {
    std::fs::read(dir.join("state.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_state(root: &Path, dir: &Path, state: &State) -> Result<(), String> {
    let path = dir.join("state.json");
    let temporary = dir.join("state.json.tmp");
    prepare_target(root, &path)?;
    prepare_target(root, &temporary)?;
    let text = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    std::fs::write(&temporary, text).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

/// Removes the copy: the collection file, its journal files, the media database and the media
/// folder. `state.json` stays.
fn discard(dir: &Path) -> std::io::Result<()> {
    for name in [
        "collection.anki2",
        "collection.anki2-wal",
        "collection.anki2-shm",
        "collection.mdb",
    ] {
        match std::fs::remove_file(dir.join(name)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {},
        }
    }
    match std::fs::remove_dir_all(dir.join("collection.media")) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {},
    }
    // The marker goes last: a run that stops in the middle leaves it, and the next run starts again.
    match std::fs::remove_file(dir.join("applying")) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn lock_copy(dir: &Path, diagnostics: &mut Diagnostics) -> Option<std::fs::File> {
    let path = dir.join("lock");
    let file = match std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path) {
        Ok(file) => file,
        Err(error) => {
            diagnostics.push(Diagnostic::error("publish::lock", format!("cannot open `{}`: {error}", path.display())));
            return None;
        },
    };
    match file.try_lock() {
        Ok(()) => Some(file),
        Err(std::fs::TryLockError::WouldBlock) => {
            diagnostics.push(
                Diagnostic::error("publish::locked", "another `janky publish` is running for this project")
                    .with_help("wait until it ends, then run again"),
            );
            None
        },
        Err(std::fs::TryLockError::Error(error)) => {
            diagnostics.push(Diagnostic::error("publish::lock", format!("cannot lock `{}`: {error}", path.display())));
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_lock_is_written_only_when_nobody_changed_it() {
        let work = tempfile::tempdir().unwrap();
        let path = work.path().join("deck.lock");
        std::fs::write(&path, "first").unwrap();
        let before = std::fs::read(&path).ok();
        let lock = lock::LockFile::new();
        std::fs::write(&path, "changed by update").unwrap();
        let error = write_lock_unchanged(&path, &before, &lock).unwrap_err();
        assert!(error.contains("another command changed it"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "changed by update");

        let before = std::fs::read(&path).ok();
        write_lock_unchanged(&path, &before, &lock).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("# Written by janky"));
    }
}
