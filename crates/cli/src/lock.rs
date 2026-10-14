//! `deck.lock`: the Anki ids of the project, and the record of the previous build.
//!
//! [`LockFile`] is the whole file. [`LockedDeck`], [`Locked`] (presets, fields and card types)
//! and [`LockedNotetype`] give each key of `deck.toml` and of the note types a fixed Anki id.
//! [`LockedNote`] and [`LockedMedia`] record each note and media file of the last build.
//! [`LockedSource`] records the resolved commit or archive of a remote note type. These types
//! also make the JSON Schema of `deck.lock`, so their docs are in the `schemars` attributes.
//!
//! A build reads the file with [`read`], then calls [`LockFile::update`] to give new keys an
//! id. A key that disappears stays in the file, marked as removed, so that no other key gets
//! its id. [`crate::package::package`] then stamps the note types and notes and records the
//! media, and the caller writes the result with [`LockFile::to_toml`].
//!
//! A note type or note keeps its modification time while its hash stays the same. A changed
//! hash raises the time, so that Anki's default "update if newer" import takes the change.
//!
//! New namespaces and ids come from the operating system's random source. The functions that
//! make them panic if that source fails.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config;
use crate::config::deck::Deck;
use crate::diagnostic::{Diagnostic, Diagnostics, Source};
use crate::project::Project;
use crate::tree::child;

const MIN_ID: i64 = 1_000_000_000_000;
const MAX_ID: i64 = 1 << 53;
const BASE91: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&()*+,-./:;<=>?@[]^_`{|}~";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockFile {
    #[schemars(title = r#"Lock file version"#, description = r#"Always 1."#)]
    #[schemars(extend("const" = 1))]
    pub version: u32,
    #[schemars(
        title = r#"GUID namespace"#,
        description = r#"A random value that makes the note GUIDs of this project unique."#
    )]
    #[schemars(pattern(r"^[0-9a-f]{32}$"))]
    pub namespace: String,
    /// `None` only until the first [`update`](Self::update).
    #[schemars(
        title = r#"Root deck"#,
        description = r#"The id and full name of the root deck, and its children."#
    )]
    pub deck: Option<LockedDeck>,
    #[schemars(title = r#"Presets"#, description = r#"The id, name and hash of each preset."#)]
    #[serde(default)]
    pub presets: IndexMap<String, Locked>,
    #[schemars(
        title = r#"Note types"#,
        description = r#"The ids, names, hashes and sources of the note types, with their fields and card types."#
    )]
    #[serde(default)]
    pub notetypes: IndexMap<String, LockedNotetype>,
    /// Only the entries of the last build. Records of removed entries are dropped.
    #[schemars(
        title = r#"Notes"#,
        description = r#"The hash, modification time, note type and deck of each entry, by entry id."#
    )]
    #[serde(default)]
    pub notes: IndexMap<String, LockedNote>,
    #[schemars(
        title = r#"Media"#,
        description = r#"The content hash of each media file, by Anki file name."#
    )]
    #[serde(default)]
    pub media: IndexMap<String, LockedMedia>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Locked {
    #[schemars(title = r#"Anki id"#, description = r#"The Anki id of this key. It never changes."#)]
    #[schemars(range(min = 1))]
    pub id: i64,
    #[schemars(title = r#"Name"#, description = r#"The name at the last build."#)]
    pub name: Option<String>,
    /// Always `None` for fields and card types.
    #[schemars(
        title = r#"Hash"#,
        description = r#"A hash of the preset's table at the last build. Fields and card types do not have it."#
    )]
    #[schemars(pattern(r"^[0-9a-f]{16}$"))]
    pub hash: Option<String>,
    #[schemars(
        title = r#"Removed"#,
        description = r#"The key no longer exists. Its id stays reserved, and comes back with the key."#
    )]
    #[serde(default)]
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockedDeck {
    #[schemars(title = r#"Anki id"#, description = r#"The Anki id of the deck. It never changes."#)]
    #[schemars(range(min = 1))]
    pub id: i64,
    #[schemars(
        title = r#"Full name"#,
        description = r#"The full Anki name of the deck at the last build."#
    )]
    pub name: Option<String>,
    #[schemars(
        title = r#"Removed"#,
        description = r#"The deck no longer exists. Its id stays reserved."#
    )]
    #[serde(default)]
    pub removed: bool,
    #[schemars(title = r#"Child decks"#, description = r#"The child decks, by key."#)]
    #[serde(default)]
    pub children: IndexMap<String, LockedDeck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockedNotetype {
    #[schemars(
        title = r#"Anki id"#,
        description = r#"The Anki id of the note type. It never changes."#
    )]
    #[schemars(range(min = 1))]
    pub id: i64,
    #[schemars(title = r#"Name"#, description = r#"The note type name at the last build."#)]
    pub name: Option<String>,
    #[schemars(
        title = r#"Removed"#,
        description = r#"The note type no longer exists. Its id stays reserved."#
    )]
    #[serde(default)]
    pub removed: bool,
    #[schemars(title = r#"Hash"#, description = r#"A hash of the note type at the last build."#)]
    #[serde(default)]
    #[schemars(pattern(r"^[0-9a-f]{16}$"))]
    pub hash: Option<String>,
    #[schemars(
        title = r#"Modification time"#,
        description = r#"Seconds since 1970. It rises when the hash changes."#
    )]
    #[schemars(range(min = 1))]
    pub modified: Option<i64>,
    #[schemars(
        title = r#"Source"#,
        description = r#"The resolved remote source: a Git commit or an archive hash."#
    )]
    pub source: Option<LockedSource>,
    #[schemars(
        title = r#"Fields"#,
        description = r#"The field ids, in the order of the last build. Removed fields come last."#
    )]
    #[serde(default)]
    pub fields: IndexMap<String, Locked>,
    #[schemars(
        title = r#"Card types"#,
        description = r#"The card type ids, in the order of the last build. Removed card types come last."#
    )]
    #[serde(default)]
    pub cards: IndexMap<String, Locked>,
}

trait Entry {
    fn new(id: i64) -> Self;
    fn id(&self) -> i64;
    fn set_removed(&mut self, removed: bool);
}

impl Entry for Locked {
    fn new(id: i64) -> Self {
        Self {
            id,
            name: None,
            hash: None,
            removed: false,
        }
    }

    fn id(&self) -> i64 {
        self.id
    }

    fn set_removed(&mut self, removed: bool) {
        self.removed = removed;
    }
}

impl Entry for LockedDeck {
    fn new(id: i64) -> Self {
        Self {
            id,
            name: None,
            removed: false,
            children: IndexMap::new(),
        }
    }

    fn id(&self) -> i64 {
        self.id
    }

    fn set_removed(&mut self, removed: bool) {
        self.removed = removed;
        if removed {
            self.children.values_mut().for_each(|child| child.set_removed(true));
        }
    }
}

impl Entry for LockedNotetype {
    fn new(id: i64) -> Self {
        Self {
            id,
            name: None,
            removed: false,
            hash: None,
            modified: None,
            source: None,
            fields: IndexMap::new(),
            cards: IndexMap::new(),
        }
    }

    fn id(&self) -> i64 {
        self.id
    }

    fn set_removed(&mut self, removed: bool) {
        self.removed = removed;
        if removed {
            self.fields.values_mut().chain(self.cards.values_mut()).for_each(|entry| entry.set_removed(true));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockedNote {
    #[schemars(
        title = r#"Hash"#,
        description = r#"A hash of the note's rendered fields, tags and note type."#
    )]
    #[schemars(pattern(r"^[0-9a-f]{16}$"))]
    pub hash: String,
    #[schemars(
        title = r#"Modification time"#,
        description = r#"Seconds since 1970. It rises when the hash changes."#
    )]
    #[schemars(range(min = 1))]
    pub modified: i64,
    #[schemars(title = r#"Note type"#, description = r#"The note type key at the last build."#)]
    pub notetype: String,
    /// The deck keys joined with `.`, for example `lessons.basics`.
    #[schemars(
        title = r#"Deck"#,
        description = r#"The deck key path at the last build. Empty for the root deck."#
    )]
    pub deck: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockedSource {
    #[schemars(title = r#"Git repository"#, description = r#"The repository URL."#)]
    pub git: Option<String>,
    #[schemars(
        title = r#"Git tag"#,
        description = r#"The tag in deck.toml when the commit was resolved."#
    )]
    pub tag: Option<String>,
    #[schemars(
        title = r#"Git commit"#,
        description = r#"The rev in deck.toml when the commit was resolved."#
    )]
    pub rev: Option<String>,
    #[schemars(
        title = r#"Git branch"#,
        description = r#"The branch in deck.toml when the commit was resolved."#
    )]
    pub branch: Option<String>,
    #[schemars(title = r#"Commit"#, description = r#"The resolved commit id: 40 or 64 hex digits."#)]
    #[schemars(pattern(r"^([0-9a-f]{40}|[0-9a-f]{64})$"))]
    pub commit: Option<String>,
    #[schemars(title = r#"Archive URL"#, description = r#"The archive URL."#)]
    pub url: Option<String>,
    #[schemars(title = r#"Archive SHA-256"#, description = r#"The SHA-256 hash of the archive."#)]
    #[schemars(pattern(r"^[0-9a-f]{64}$"))]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockedMedia {
    #[schemars(title = r#"Hash"#, description = r#"A hash of the file content at the last build."#)]
    #[schemars(pattern(r"^[0-9a-f]{16}$"))]
    pub hash: String,
}

/// One note of the current build, for [`LockFile::stamp_notes`].
pub struct Stamp {
    /// The entry id.
    pub id: String,
    /// The [`content_hash`] of the note.
    pub hash: String,
    /// The note type key.
    pub notetype: String,
    /// The deck keys joined with `.`. Empty for the root deck.
    pub deck: String,
}

/// The hash of a media file: the first 8 bytes of its SHA-256, as 16 lowercase hex digits.
pub fn file_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)[..8].iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A hash of a sequence of strings, in the same form as [`file_hash`].
///
/// Each part goes into the hash with its length in front, so `["ab", "c"]` and `["a", "bc"]`
/// give different hashes.
pub fn content_hash<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.len().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(part.as_bytes());
    }
    hasher.finalize()[..8].iter().map(|byte| format!("{byte:02x}")).collect()
}

fn next_modified(previous: Option<(&str, i64)>, hash: &str, now: i64) -> i64 {
    match previous {
        Some((old, modified)) if old == hash => modified,
        Some((_, modified)) => now.max(modified + 1),
        None => now,
    }
}

/// Reads the lock file at `path`, or returns a new [`LockFile`] if there is no file.
///
/// It returns `None` if the file cannot be read (a `lock::read` error), or if it is not valid
/// TOML or does not match the lock file schema (the errors of [`config::load`]).
pub fn read(path: &Path, diagnostics: &mut Diagnostics) -> Option<LockFile> {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into());
    match std::fs::read_to_string(path) {
        Ok(text) => config::load::<LockFile>(Source::new(name, text), diagnostics).map(|(lock, _)| lock),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(LockFile::new()),
        Err(error) => {
            diagnostics.push(Diagnostic::error("lock::read", format!("cannot read `{}`: {error}", path.display())));
            None
        },
    }
}

/// The Anki GUID of a note: the first 8 bytes of
/// `sha256("<namespace>:<note type key>:<entry id>")`, written in base 91.
///
/// The same entry id with another note type key gives another GUID, so Anki sees a new note.
pub fn guid(namespace: &str, notetype: &str, entry_id: &str) -> String {
    let digest = Sha256::digest(format!("{namespace}:{notetype}:{entry_id}").as_bytes());
    let mut number = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes"));
    let mut out = Vec::new();
    while number > 0 {
        out.push(BASE91[(number % BASE91.len() as u64) as usize]);
        number /= BASE91.len() as u64;
    }
    if out.is_empty() {
        out.push(BASE91[0]);
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII")
}

impl LockFile {
    /// Makes an empty lock file with a new random GUID namespace.
    pub fn new() -> Self {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).expect("random bytes");
        let namespace = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Self {
            version: 1,
            namespace,
            deck: None,
            presets: IndexMap::new(),
            notetypes: IndexMap::new(),
            notes: IndexMap::new(),
            media: IndexMap::new(),
        }
    }

    /// Gives every deck, preset, note type, field and card type key of `project` an id.
    ///
    /// A new key gets a random id that no other entry of the lock file has. A key that the
    /// project no longer has is marked as removed, and so are its child decks, fields and card
    /// types. A key that comes back keeps its old id. Each table lists the current keys in
    /// project order, then the removed keys. The fields include the flag field `_make_<card
    /// key>` of each card type with `when`.
    ///
    /// Only the presets that a deck names get an id. A preset that is only a base for `extends`
    /// does not get one.
    ///
    /// The function also records the names, the hash of each preset table (with the tables of
    /// its `extends` chain) and the source of each note type. It does not change the notes or
    /// media.
    pub fn update(&mut self, project: &Project) {
        let mut ids = Ids(self.used_ids());
        let root = self.deck.get_or_insert_with(|| LockedDeck::new(ids.next()));
        root.removed = false;
        let root_deck = &project.deck.deck.deck;
        sync_deck(root, root_deck, &root_deck.name, &mut ids);
        let used = project.deck.used_presets();
        sync(&mut self.presets, used.iter().copied(), &mut ids);
        for key in used {
            let preset = &project.deck.presets[key];
            let tables: Vec<String> = project
                .deck
                .preset_chain(key)
                .into_iter()
                .filter_map(|current| project.deck_tree.value.pointer(&child("/presets", current)))
                .map(Value::to_string)
                .collect();
            let locked = &mut self.presets[key];
            locked.name = Some(preset.name.clone().unwrap_or_else(|| key.clone()));
            locked.hash = Some(content_hash(tables.iter().map(String::as_str)));
        }
        sync(&mut self.notetypes, project.notetypes.keys(), &mut ids);
        for (key, notetype) in &project.notetypes {
            let locked = self.notetypes.get_mut(key).expect("synced");
            locked.name = Some(notetype.file.name.clone());
            locked.source = project.sources.get(key).cloned();
            let flags: Vec<String> = notetype
                .file
                .cards
                .iter()
                .filter(|(_, card)| card.when.is_some())
                .map(|(card, _)| format!("_make_{card}"))
                .collect();
            sync(&mut locked.fields, notetype.file.fields.keys().chain(flags.iter()), &mut ids);
            sync(&mut locked.cards, notetype.file.cards.keys(), &mut ids);
        }
    }

    /// Writes the lock file as TOML, in the fixed layout that `docs/spec.md` shows.
    ///
    /// Keys are quoted only when they are not bare TOML keys. Notes, media, fields and card
    /// types are inline tables, one line each.
    pub fn to_toml(&self) -> String {
        let mut out = String::from("# Written by janky. Do not edit this file.\n");
        let _ = writeln!(out, "version = {}", self.version);
        let _ = writeln!(out, "namespace = {}", quote(&self.namespace));
        if let Some(deck) = &self.deck {
            write_deck(&mut out, "deck", deck);
        }
        for (key, preset) in &self.presets {
            let _ = write!(out, "\n[presets.{}]\n", bare(key));
            write_entry(&mut out, preset.id, preset.name.as_deref(), preset.removed);
            if let Some(hash) = &preset.hash {
                let _ = writeln!(out, "hash = {}", quote(hash));
            }
        }
        for (key, notetype) in &self.notetypes {
            let path = format!("notetypes.{}", bare(key));
            let _ = write!(out, "\n[{path}]\n");
            write_entry(&mut out, notetype.id, notetype.name.as_deref(), notetype.removed);
            if let Some(hash) = &notetype.hash {
                let _ = writeln!(out, "hash = {}", quote(hash));
            }
            if let Some(modified) = notetype.modified {
                let _ = writeln!(out, "modified = {modified}");
            }
            if let Some(source) = &notetype.source {
                let parts: Vec<String> = [
                    ("git", &source.git),
                    ("tag", &source.tag),
                    ("rev", &source.rev),
                    ("branch", &source.branch),
                    ("commit", &source.commit),
                    ("url", &source.url),
                    ("sha256", &source.sha256),
                ]
                .into_iter()
                .filter_map(|(key, value)| value.as_ref().map(|value| format!("{key} = {}", quote(value))))
                .collect();
                let _ = writeln!(out, "source = {{ {} }}", parts.join(", "));
            }
            for (table, entries) in [("fields", &notetype.fields), ("cards", &notetype.cards)] {
                if entries.is_empty() {
                    continue;
                }
                let _ = write!(out, "\n[{path}.{table}]\n");
                for (name, entry) in entries {
                    let removed = if entry.removed { ", removed = true" } else { "" };
                    let _ = writeln!(out, "{} = {{ id = {}{removed} }}", bare(name), entry.id);
                }
            }
        }
        if !self.notes.is_empty() {
            out.push_str("\n[notes]\n");
            for (id, note) in &self.notes {
                let _ = writeln!(
                    out,
                    "{} = {{ hash = {}, modified = {}, notetype = {}, deck = {} }}",
                    bare(id),
                    quote(&note.hash),
                    note.modified,
                    quote(&note.notetype),
                    quote(&note.deck)
                );
            }
        }
        if !self.media.is_empty() {
            out.push_str("\n[media]\n");
            for (name, media) in &self.media {
                let _ = writeln!(out, "{} = {{ hash = {} }}", bare(name), quote(&media.hash));
            }
        }
        out
    }

    /// Records the hash of the note type `key`, and returns its modification time.
    ///
    /// The time stays the same if the hash did not change. Otherwise it becomes `now`, or the
    /// old time plus one if that is later. Call [`update`](Self::update) first: the function
    /// panics if `key` is not in the lock file.
    pub fn stamp_notetype(&mut self, key: &str, hash: String, now: i64) -> i64 {
        let notetype = self.notetypes.get_mut(key).expect("synced note type");
        let previous = notetype.hash.as_deref().zip(notetype.modified);
        let modified = next_modified(previous, &hash, now);
        notetype.hash = Some(hash);
        notetype.modified = Some(modified);
        modified
    }

    /// Replaces the notes table with `notes`, and returns the modification time of each note
    /// in the same order.
    ///
    /// The times follow the same rule as [`stamp_notetype`](Self::stamp_notetype). A new note
    /// gets `now`. Notes that are not in `notes` are dropped.
    pub fn stamp_notes(&mut self, notes: Vec<Stamp>, now: i64) -> Vec<i64> {
        let mut stamped = IndexMap::new();
        let mut times = Vec::new();
        for Stamp { id, hash, notetype, deck } in notes {
            let previous = self.notes.get(&id).map(|note| (note.hash.as_str(), note.modified));
            let modified = next_modified(previous, &hash, now);
            times.push(modified);
            stamped.insert(id, LockedNote { hash, modified, notetype, deck });
        }
        self.notes = stamped;
        times
    }

    fn used_ids(&self) -> HashSet<i64> {
        let mut used = HashSet::new();
        let mut decks: Vec<&LockedDeck> = self.deck.iter().collect();
        while let Some(deck) = decks.pop() {
            used.insert(deck.id);
            decks.extend(deck.children.values());
        }
        used.extend(self.presets.values().map(Entry::id));
        for notetype in self.notetypes.values() {
            used.insert(notetype.id);
            used.extend(notetype.fields.values().chain(notetype.cards.values()).map(Entry::id));
        }
        used
    }
}

impl Default for LockFile {
    fn default() -> Self {
        Self::new()
    }
}

struct Ids(HashSet<i64>);

impl Ids {
    fn next(&mut self) -> i64 {
        loop {
            let random = getrandom::u64().expect("random number");
            let id = MIN_ID + (random % (MAX_ID - MIN_ID) as u64) as i64;
            if self.0.insert(id) {
                return id;
            }
        }
    }
}

fn sync<'a, T: Entry>(map: &mut IndexMap<String, T>, keys: impl IntoIterator<Item = &'a String>, ids: &mut Ids) {
    let keys: Vec<&String> = keys.into_iter().collect();
    for (key, entry) in map.iter_mut() {
        if !keys.contains(&key) {
            entry.set_removed(true);
        }
    }
    let mut ordered = IndexMap::new();
    for key in keys {
        let entry = match map.shift_remove(key) {
            Some(mut entry) => {
                entry.set_removed(false);
                entry
            },
            None => T::new(ids.next()),
        };
        ordered.insert(key.clone(), entry);
    }
    ordered.extend(map.drain(..));
    *map = ordered;
}

fn sync_deck(locked: &mut LockedDeck, deck: &Deck, full_name: &str, ids: &mut Ids) {
    locked.name = Some(full_name.to_string());
    sync(&mut locked.children, deck.children.keys(), ids);
    for (key, child) in &deck.children {
        let name = format!("{full_name}::{}", child.name);
        sync_deck(locked.children.get_mut(key).expect("synced"), child, &name, ids);
    }
}

fn write_deck(out: &mut String, path: &str, deck: &LockedDeck) {
    let _ = write!(out, "\n[{path}]\n");
    write_entry(out, deck.id, deck.name.as_deref(), deck.removed);
    for (key, child) in &deck.children {
        write_deck(out, &format!("{path}.children.{}", bare(key)), child);
    }
}

fn write_entry(out: &mut String, id: i64, name: Option<&str>, removed: bool) {
    let _ = writeln!(out, "id = {id}");
    if let Some(name) = name {
        let _ = writeln!(out, "name = {}", quote(name));
    }
    if removed {
        out.push_str("removed = true\n");
    }
}

fn bare(key: &str) -> String {
    if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        key.to_string()
    } else {
        quote(key)
    }
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("string serialises")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_matches_reference_values() {
        assert_eq!(guid("00112233445566778899aabbccddeeff", "element", "lithium"), "v3NV1k][qJ");
        assert_eq!(guid("00112233445566778899aabbccddeeff", "element", "⚛️"), "l9*NX8)4<x");
        assert_eq!(guid("ffeeddccbbaa99887766554433221100", "element", "lithium"), "poZ1qH~*3H");
        assert_eq!(guid("00112233445566778899aabbccddeeff", "grammar", "lithium"), "uUW3KojC>a");
    }

    #[test]
    fn new_ids_are_unique_and_in_range() {
        let mut ids = Ids(HashSet::new());
        let made: HashSet<i64> = (0..1000).map(|_| ids.next()).collect();
        assert_eq!(made.len(), 1000);
        assert!(made.iter().all(|id| (MIN_ID..MAX_ID).contains(id)));
    }

    #[test]
    fn modified_rises_only_when_the_hash_changes() {
        assert_eq!(next_modified(None, "a", 100), 100);
        assert_eq!(next_modified(Some(("a", 50)), "a", 100), 50);
        assert_eq!(next_modified(Some(("a", 50)), "b", 100), 100);
        assert_eq!(next_modified(Some(("a", 150)), "b", 100), 151);
        assert_ne!(content_hash(["ab", "c"]), content_hash(["a", "bc"]));
    }

    #[test]
    fn sources_are_written_and_read_back() {
        let mut lock = LockFile::new();
        let mut notetype = LockedNotetype::new(1_000_000_000_001);
        notetype.source = Some(LockedSource {
            git: Some("https://example.com/notetypes.git".into()),
            tag: None,
            rev: None,
            branch: None,
            commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
            url: None,
            sha256: None,
        });
        lock.notetypes.insert("grammar".into(), notetype);
        let text = lock.to_toml();
        assert!(text.contains(
            "source = { git = \"https://example.com/notetypes.git\", commit = \"0123456789abcdef0123456789abcdef01234567\" }"
        ));
        let mut diagnostics = Diagnostics::default();
        let (read, _) = config::load::<LockFile>(Source::new("deck.lock", text), &mut diagnostics).unwrap();
        assert!(diagnostics.is_empty());
        assert_eq!(read, lock);
    }

    #[test]
    fn keys_are_quoted_only_when_needed() {
        assert_eq!(bare("_make_production"), "_make_production");
        assert_eq!(bare("a.b"), "\"a.b\"");
    }
}
