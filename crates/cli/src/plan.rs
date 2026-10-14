//! The plan: what the next build changes, compared with the previous build.
//!
//! The lock file is the record of the previous build, so [`plan`] compares two [`LockFile`]
//! values: the one on disk and the one that the next build writes. It gives a [`Plan`], a list
//! of [`Change`] values in a fixed order (decks, presets, note types, notes, media). [`Plan::text`]
//! shows the plan to the author, and [`Plan::to_json`] gives the `--json` line. Both say what
//! Anki does with each change on import, because a package cannot do everything (for example,
//! it cannot remove a note or change a preset that Anki already has).
//!
//! `janky publish` compares the project with a copy of the user's collection instead, through
//! [`janky_anki::diff`]. [`publish_text`] and [`publish_json`] show that comparison.

use std::collections::HashMap;

use indexmap::IndexMap;
use janky_anki::Diff;
use serde::Serialize;

use crate::lock::{LockFile, Locked, LockedDeck};

/// One difference between the previous build and the next build.
///
/// Deck fields hold deck key paths (dotted, empty for the root deck). Note ids are entry ids.
/// The JSON form has a `kind` tag in kebab case, for example `note-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Change {
    /// An entry id that the previous build did not have.
    NoteAdded {
        /// The entry id.
        id: String,
        /// The deck key path of the note.
        deck: String,
    },
    /// The note hash changed, in the same deck and note type.
    NoteChanged {
        /// The entry id.
        id: String,
        /// The deck key path of the note.
        deck: String,
    },
    /// An entry id that the next build does not have. The note stays in Anki.
    NoteRemoved {
        /// The entry id.
        id: String,
        /// The deck key path of the note in the previous build.
        deck: String,
    },
    /// The note moved to another deck (`from` and `to` are deck key paths).
    NoteMoved {
        /// The entry id.
        id: String,
        /// The deck key path in the previous build.
        from: String,
        /// The deck key path in the next build.
        to: String,
    },
    /// The note changed note type (`from` and `to` are note type keys). Anki gets a new note.
    NoteRetyped {
        /// The entry id.
        id: String,
        /// The note type key in the previous build.
        from: String,
        /// The note type key in the next build.
        to: String,
    },
    /// A deck that the previous build did not have, or that it had marked as removed.
    DeckAdded {
        /// The deck key path.
        key: String,
        /// The full Anki name of the deck, or the key path if the lock file has no name.
        name: String,
    },
    /// The deck is marked as removed in the next lock file.
    DeckRemoved {
        /// The deck key path.
        key: String,
        /// The full Anki name of the deck, or the key path if the lock file has no name.
        name: String,
    },
    /// The full name of the deck changed. Anki matches decks by name, so the author must rename
    /// the deck in Anki before the import.
    DeckRenamed {
        /// The deck key path.
        key: String,
        /// The full Anki name in the previous build.
        from: String,
        /// The full Anki name in the next build.
        to: String,
    },
    /// A preset that the previous build did not have, or that it had marked as removed.
    PresetAdded {
        /// The preset key.
        key: String,
        /// The preset name, or the key if the lock file has no name.
        name: String,
    },
    /// The preset is marked as removed in the next lock file. The preset stays in Anki.
    PresetRemoved {
        /// The preset key.
        key: String,
        /// The preset name, or the key if the lock file has no name.
        name: String,
    },
    /// The hash or the name of the preset changed. A package cannot change a preset that Anki has.
    PresetChanged {
        /// The preset key.
        key: String,
        /// The preset name in the next build, or the key if it has no name.
        name: String,
    },
    /// A note type that the previous build did not have, or that it had marked as removed.
    NotetypeAdded {
        /// The note type key.
        key: String,
        /// The note type name, or the key if the lock file has no name.
        name: String,
    },
    /// The note type is marked as removed in the next lock file. The note type stays in Anki.
    NotetypeRemoved {
        /// The note type key.
        key: String,
        /// The note type name, or the key if the lock file has no name.
        name: String,
    },
    /// The note type has a new name. Reported only if the previous lock file has a name.
    NotetypeRenamed {
        /// The note type key.
        key: String,
        /// The name in the previous build.
        from: String,
        /// The name in the next build, or the key if it has no name.
        to: String,
    },
    /// Fields or card types were added, removed or moved. Anki needs a full sync.
    SchemaChanged {
        /// The note type key.
        notetype: String,
        /// The change to the fields, including the `_make_<card key>` flag fields.
        fields: OrderChange,
        /// The change to the card types.
        cards: OrderChange,
    },
    /// The hash changed but the schema did not, so the card templates or the CSS changed.
    /// Not reported together with a rename or a schema change.
    NotetypeChanged {
        /// The note type key.
        notetype: String,
    },
    /// A media file that the previous build did not have.
    MediaAdded {
        /// The file name in the package.
        name: String,
    },
    /// The content hash of the media file changed.
    MediaChanged {
        /// The file name in the package.
        name: String,
    },
    /// A media file that the next build does not have. The file stays in Anki.
    MediaRemoved {
        /// The file name in the package.
        name: String,
    },
}

/// How the ordered fields or card types of a note type changed, by key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OrderChange {
    /// The keys that the next build has and the previous build did not have.
    pub added: Vec<String>,
    /// The keys that the previous build has and the next build does not have.
    pub removed: Vec<String>,
    /// The keys in both builds are in a different order.
    pub moved: bool,
}

impl OrderChange {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && !self.moved
    }
}

/// The changes of the next build, made by [`plan`].
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// The changes in a fixed order: decks, presets, note types, notes, then media.
    pub changes: Vec<Change>,
    /// The notes with the same note type, deck and hash. They are not in [`changes`](Self::changes).
    pub unchanged_notes: usize,
}

/// Compares the previous lock file with the next one.
///
/// Decks, presets, note types and their fields and card types stay in the lock file with a
/// `removed` flag, so a removal shows only when the flag changes. Notes and media leave the
/// lock file when they go, so a removal is a key that `next` does not have.
pub fn plan(previous: &LockFile, next: &LockFile) -> Plan {
    let mut changes = Vec::new();
    decks(previous, next, &mut changes);
    presets(previous, next, &mut changes);
    notetypes(previous, next, &mut changes);
    let unchanged_notes = notes(previous, next, &mut changes);
    media(previous, next, &mut changes);
    Plan { changes, unchanged_notes }
}

fn display_name(name: &Option<String>, key: &str) -> String {
    name.clone().unwrap_or_else(|| key.to_string())
}

fn decks(previous: &LockFile, next: &LockFile, changes: &mut Vec<Change>) {
    let before = flatten_decks(previous.deck.as_ref());
    for (key, (to, removed)) in flatten_decks(next.deck.as_ref()) {
        match (before.get(&key), removed) {
            (Some((_, false)), true) => changes.push(Change::DeckRemoved { key, name: to }),
            (Some((from, false)), false) if *from != to => {
                changes.push(Change::DeckRenamed { key, from: from.clone(), to })
            },
            (None | Some((_, true)), false) => changes.push(Change::DeckAdded { key, name: to }),
            _ => {},
        }
    }
}

fn flatten_decks(root: Option<&LockedDeck>) -> IndexMap<String, (String, bool)> {
    let mut out = IndexMap::new();
    let mut stack: Vec<(String, &LockedDeck)> = root.into_iter().map(|deck| (String::new(), deck)).collect();
    while let Some((key, deck)) = stack.pop() {
        for (child, child_deck) in deck.children.iter().rev() {
            let path = if key.is_empty() { child.clone() } else { format!("{key}.{child}") };
            stack.push((path, child_deck));
        }
        out.insert(key.clone(), (display_name(&deck.name, &key), deck.removed));
    }
    out
}

fn presets(previous: &LockFile, next: &LockFile, changes: &mut Vec<Change>) {
    for (key, preset) in &next.presets {
        let name = display_name(&preset.name, key);
        let before = previous.presets.get(key).filter(|before| !before.removed);
        match (before, preset.removed) {
            (Some(_), true) => changes.push(Change::PresetRemoved { key: key.clone(), name }),
            (None, false) => changes.push(Change::PresetAdded { key: key.clone(), name }),
            (Some(before), false) if before.hash != preset.hash || before.name != preset.name => {
                changes.push(Change::PresetChanged { key: key.clone(), name });
            },
            _ => {},
        }
    }
}

fn notetypes(previous: &LockFile, next: &LockFile, changes: &mut Vec<Change>) {
    for (key, notetype) in &next.notetypes {
        let name = display_name(&notetype.name, key);
        let before = previous.notetypes.get(key).filter(|before| !before.removed);
        let before = match (before, notetype.removed) {
            (Some(_), true) => {
                changes.push(Change::NotetypeRemoved { key: key.clone(), name });
                continue;
            },
            (None, false) => {
                changes.push(Change::NotetypeAdded { key: key.clone(), name });
                continue;
            },
            (Some(before), false) => before,
            (None, true) => continue,
        };
        let renamed = before.name.is_some() && before.name.as_deref() != Some(name.as_str());
        if renamed {
            let from = display_name(&before.name, key);
            changes.push(Change::NotetypeRenamed { key: key.clone(), from, to: name.clone() });
        }
        let fields = order_change(&before.fields, &notetype.fields);
        let cards = order_change(&before.cards, &notetype.cards);
        if !fields.is_empty() || !cards.is_empty() {
            changes.push(Change::SchemaChanged { notetype: key.clone(), fields, cards });
        } else if !renamed && before.hash.is_some() && before.hash != notetype.hash {
            changes.push(Change::NotetypeChanged { notetype: key.clone() });
        }
    }
}

fn order_change(before: &IndexMap<String, Locked>, after: &IndexMap<String, Locked>) -> OrderChange {
    let active = |map: &IndexMap<String, Locked>| -> Vec<String> {
        map.iter().filter(|(_, entry)| !entry.removed).map(|(key, _)| key.clone()).collect()
    };
    let (before, after) = (active(before), active(after));
    let added: Vec<String> = after.iter().filter(|key| !before.contains(key)).cloned().collect();
    let removed: Vec<String> = before.iter().filter(|key| !after.contains(key)).cloned().collect();
    let kept_before: Vec<&String> = before.iter().filter(|key| after.contains(key)).collect();
    let kept_after: Vec<&String> = after.iter().filter(|key| before.contains(key)).collect();
    OrderChange {
        added,
        removed,
        moved: kept_before != kept_after,
    }
}

fn notes(previous: &LockFile, next: &LockFile, changes: &mut Vec<Change>) -> usize {
    let mut unchanged = 0;
    for (id, note) in &next.notes {
        let Some(before) = previous.notes.get(id) else {
            changes.push(Change::NoteAdded { id: id.clone(), deck: note.deck.clone() });
            continue;
        };
        if before.notetype != note.notetype {
            changes.push(Change::NoteRetyped {
                id: id.clone(),
                from: before.notetype.clone(),
                to: note.notetype.clone(),
            });
        } else if before.deck != note.deck {
            changes.push(Change::NoteMoved {
                id: id.clone(),
                from: before.deck.clone(),
                to: note.deck.clone(),
            });
        } else if before.hash != note.hash {
            changes.push(Change::NoteChanged { id: id.clone(), deck: note.deck.clone() });
        } else {
            unchanged += 1;
        }
    }
    for (id, note) in &previous.notes {
        if !next.notes.contains_key(id) {
            changes.push(Change::NoteRemoved { id: id.clone(), deck: note.deck.clone() });
        }
    }
    unchanged
}

fn media(previous: &LockFile, next: &LockFile, changes: &mut Vec<Change>) {
    for (name, file) in &next.media {
        match previous.media.get(name) {
            None => changes.push(Change::MediaAdded { name: name.clone() }),
            Some(before) if before.hash != file.hash => changes.push(Change::MediaChanged { name: name.clone() }),
            _ => {},
        }
    }
    for name in previous.media.keys() {
        if !next.media.contains_key(name) {
            changes.push(Change::MediaRemoved { name: name.clone() });
        }
    }
}

impl Plan {
    /// The import options that the changes need, one sentence each. Empty if the default
    /// import options are enough.
    pub fn advice(&self) -> Vec<String> {
        let mut advice = Vec::new();
        if self.changes.iter().any(|change| matches!(change, Change::PresetAdded { .. })) {
            advice.push(r#"Tick "Import any deck presets". Anki adds a preset only the first time."#.to_string());
        }
        let schema: Vec<&OrderChange> = self
            .changes
            .iter()
            .filter_map(|change| match change {
                Change::SchemaChanged { fields, .. } => Some(fields),
                _ => None,
            })
            .collect();
        if !schema.is_empty() {
            advice.push(r#"Turn on "Merge note types". The import needs a full sync afterwards."#.to_string());
        }
        if schema.iter().any(|fields| !fields.added.is_empty()) {
            advice.push(r#"Set "Update notes" to "Always", so that the new fields get their values."#.to_string());
        }
        advice
    }

    /// The plan as text for the author: a line with the note counts, one line per change, and
    /// the [`advice`](Self::advice) under "When you import".
    ///
    /// The lock files must be the same as for [`plan`]. They give the display names of decks
    /// and note types, also for the ones that were removed.
    pub fn text(&self, previous: &LockFile, next: &LockFile) -> String {
        let (before, after) = (flatten_decks(previous.deck.as_ref()), flatten_decks(next.deck.as_ref()));
        let deck = |key: &str| {
            after.get(key).or_else(|| before.get(key)).map_or_else(|| key.to_string(), |(name, _)| name.clone())
        };
        let notetype = |key: &str| {
            let locked = next.notetypes.get(key).or_else(|| previous.notetypes.get(key));
            locked.map_or_else(|| key.to_string(), |locked| display_name(&locked.name, key))
        };
        let count = |kind: fn(&Change) -> bool| self.changes.iter().filter(|change| kind(change)).count();
        let mut out = format!(
            "Notes: {} added, {} changed, {} removed, {} moved, {} unchanged\n",
            count(|change| matches!(change, Change::NoteAdded { .. })),
            count(|change| matches!(change, Change::NoteChanged { .. })),
            count(|change| matches!(change, Change::NoteRemoved { .. })),
            count(|change| matches!(change, Change::NoteMoved { .. } | Change::NoteRetyped { .. })),
            self.unchanged_notes,
        );
        if self.changes.is_empty() {
            out.push_str("No changes.\n");
            return out;
        }
        for change in &self.changes {
            let line = match change {
                Change::NoteAdded { id, deck: key } => format!("+ note {id} in {}", deck(key)),
                Change::NoteChanged { id, deck: key } => format!("~ note {id} in {}", deck(key)),
                Change::NoteRemoved { id, deck: key } => {
                    format!("- note {id} in {}: the note stays in Anki. Delete it there", deck(key))
                },
                Change::NoteMoved { id, from, to } => format!(
                    "~ note {id} moves from {} to {}: its cards stay in {}. Move them in Anki (Browse, Change Deck)",
                    deck(from),
                    deck(to),
                    deck(from)
                ),
                Change::NoteRetyped { id, from, to } => format!(
                    "~ note {id} changes note type from {} to {}: janky adds a new note. Delete the old note in Anki",
                    notetype(from),
                    notetype(to)
                ),
                Change::DeckAdded { name, .. } => format!("+ deck {name}"),
                Change::DeckRemoved { name, .. } => format!("- deck {name}: the deck stays in Anki"),
                Change::DeckRenamed { from, to, .. } => {
                    format!("~ deck {from} is renamed to {to}: rename it in Anki before you import")
                },
                Change::PresetAdded { name, .. } => format!("+ preset {name}"),
                Change::PresetRemoved { name, .. } => format!("- preset {name}: the preset stays in Anki"),
                Change::PresetChanged { name, .. } => {
                    format!("~ preset {name}: a package cannot change a preset that Anki has. Change it in Anki")
                },
                Change::NotetypeAdded { name, .. } => format!("+ note type {name}"),
                Change::NotetypeRemoved { name, .. } => format!("- note type {name}: the note type stays in Anki"),
                Change::NotetypeRenamed { from, to, .. } => format!("~ note type {from} is renamed to {to}"),
                Change::SchemaChanged { notetype: key, fields, cards } => {
                    let mut parts = Vec::new();
                    describe(&mut parts, "fields", fields);
                    describe(&mut parts, "card types", cards);
                    let mut line = format!("~ note type {}: {}. Needs a full sync", notetype(key), parts.join("; "));
                    if !fields.removed.is_empty() || fields.moved || !cards.removed.is_empty() || cards.moved {
                        line.push_str(". Removed or moved fields and card types do not reach Anki: change them there");
                    }
                    line
                },
                Change::NotetypeChanged { notetype: key } => {
                    format!("~ note type {}: card templates or CSS", notetype(key))
                },
                Change::MediaAdded { name } => format!("+ media {name}"),
                Change::MediaChanged { name } => format!("~ media {name}"),
                Change::MediaRemoved { name } => {
                    format!("- media {name}: the file stays in Anki. Tools, Check Media removes unused files")
                },
            };
            out.push_str(&line);
            out.push('\n');
        }
        let advice = self.advice();
        if !advice.is_empty() {
            out.push_str("When you import:\n");
            for line in advice {
                out.push_str("  ");
                out.push_str(&line);
                out.push('\n');
            }
        }
        out
    }

    /// The `--json` form: `{"plan": {"changes", "unchanged_notes", "advice"}}`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "plan": { "changes": self.changes, "unchanged_notes": self.unchanged_notes, "advice": self.advice() },
        })
    }
}

/// The text of the publish plan: a summary line, then one line for each change.
///
/// `ids` maps note GUIDs to entry ids, also of entries that the project removed (from
/// `deck.lock`). A removed note with an entry id is a removed entry, which `--delete-removed`
/// deletes. A removed note without one was made in Anki: it shows by its GUID, and janky never
/// deletes it. `delete_removed` is the `--delete-removed` flag.
///
/// Names and GUIDs come from the collection, which any synced device can change. Each line goes
/// through [`crate::diagnostic::visible`], and a line break in a name shows as `\n`, so a name
/// cannot hide or forge a line.
pub fn publish_text(diff: &Diff, ids: &HashMap<String, String>, delete_removed: bool) -> String {
    let id = |guid: &String| ids.get(guid).cloned().unwrap_or_else(|| guid.clone());
    let sorted = |guids: &[String]| {
        let mut names: Vec<String> = guids.iter().map(id).collect();
        names.sort();
        names
    };
    let (removed, foreign): (Vec<String>, Vec<String>) =
        diff.notes_removed.iter().cloned().partition(|guid| ids.contains_key(guid));
    let mut out = format!(
        "Notes: {} added, {} changed, {} kept, {} removed, {} unchanged\n",
        diff.notes_added.len(),
        diff.notes_changed.len(),
        diff.notes_kept.len(),
        removed.len(),
        diff.notes_unchanged
    );
    if diff.is_empty() {
        out.push_str("No changes.\n");
        return out;
    }
    let mut lines = Vec::new();
    lines.extend(diff.notetypes_added.iter().map(|name| format!("+ note type {name}")));
    lines.extend(diff.notetypes_changed.iter().map(|name| format!("~ note type {name}: card templates, CSS or names")));
    lines.extend(diff.notetypes_merged.iter().map(|(copy, name)| {
        format!("- note type {copy}: a copy of {name}. Its notes move to {name}, and the copy is deleted")
    }));
    lines.extend(diff.schema_changed.iter().map(|name| {
        format!("~ note type {name}: fields or card types change. Needs a full sync (`--allow-full-sync`)")
    }));
    lines.extend(diff.presets_added.iter().map(|name| format!("+ preset {name}")));
    lines.extend(diff.presets_changed.iter().map(|(name, keys)| format!("~ preset {name}: sets {}", keys.join(", "))));
    lines.extend(diff.decks_added.iter().map(|name| format!("+ deck {name}")));
    lines.extend(diff.decks_renamed.iter().map(|(from, to)| format!("~ deck {from} is renamed to {to}")));
    lines.extend(diff.decks_preset.iter().map(|name| format!("~ deck {name}: sets the preset back")));
    lines.extend(sorted(&diff.notes_added).into_iter().map(|name| format!("+ note {name}")));
    lines.extend(sorted(&diff.notes_changed).into_iter().map(|name| format!("~ note {name}")));
    lines.extend(
        sorted(&diff.notes_kept)
            .into_iter()
            .map(|name| format!("= note {name}: edited in Anki, so it stays as it is")),
    );
    lines.extend(sorted(&removed).into_iter().map(|name| {
        if delete_removed {
            format!("- note {name}: deleted, with its cards")
        } else {
            format!("- note {name}: stays in Anki. `--delete-removed` deletes it")
        }
    }));
    lines.extend(
        sorted(&foreign)
            .into_iter()
            .map(|guid| format!("= note {guid}: made in Anki with a janky note type. janky never deletes it")),
    );
    if diff.cards_moved > 0 {
        let what = if diff.cards_moved == 1 {
            "card moves to the deck of its note"
        } else {
            "cards move to the decks of their notes"
        };
        lines.push(format!("~ {} {what}", diff.cards_moved));
    }
    for line in lines {
        out.push_str(&crate::diagnostic::visible(&line).replace('\n', "\\n"));
        out.push('\n');
    }
    out
}

/// The `--json` form of the publish plan: `{"publish": {...}}`, with entry ids for notes. Write
/// it with [`crate::diagnostic::json_text`], which escapes the characters that `serde_json` keeps.
pub fn publish_json(diff: &Diff, ids: &HashMap<String, String>) -> serde_json::Value {
    let (removed, foreign): (Vec<String>, Vec<String>) =
        diff.notes_removed.iter().cloned().partition(|guid| ids.contains_key(guid));
    let names = |guids: &[String]| {
        let mut names: Vec<String> =
            guids.iter().map(|guid| ids.get(guid).cloned().unwrap_or_else(|| guid.clone())).collect();
        names.sort();
        names
    };
    let presets: Vec<serde_json::Value> =
        diff.presets_changed.iter().map(|(name, keys)| serde_json::json!({ "name": name, "keys": keys })).collect();
    let merged: Vec<serde_json::Value> =
        diff.notetypes_merged.iter().map(|(copy, name)| serde_json::json!({ "copy": copy, "into": name })).collect();
    let renamed: Vec<serde_json::Value> =
        diff.decks_renamed.iter().map(|(from, to)| serde_json::json!({ "from": from, "to": to })).collect();
    serde_json::json!({
        "publish": {
            "notetypes_added": diff.notetypes_added,
            "notetypes_changed": diff.notetypes_changed,
            "notetypes_merged": merged,
            "schema_changed": diff.schema_changed,
            "presets_added": diff.presets_added,
            "presets_changed": presets,
            "decks_added": diff.decks_added,
            "decks_renamed": renamed,
            "decks_preset": diff.decks_preset,
            "notes_added": names(&diff.notes_added),
            "notes_changed": names(&diff.notes_changed),
            "notes_kept": names(&diff.notes_kept),
            "notes_removed": names(&removed),
            "notes_from_anki": names(&foreign),
            "notes_unchanged": diff.notes_unchanged,
            "cards_moved": diff.cards_moved,
        }
    })
}

fn describe(parts: &mut Vec<String>, what: &str, change: &OrderChange) {
    if !change.added.is_empty() {
        parts.push(format!("{what} added: {}", change.added.join(", ")));
    }
    if !change.removed.is_empty() {
        parts.push(format!("{what} removed: {}", change.removed.join(", ")));
    }
    if change.moved {
        parts.push(format!("{what} moved"));
    }
}
