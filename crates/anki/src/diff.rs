//! Compares a [`Package`] with a copy of a user's collection, before [`apply`](crate::apply).
//!
//! [`diff`] reports what `apply` and the next sync will change in the collection, including the
//! values that the user changed in Anki and that the project sets back. Notes are named by their
//! GUID, so the caller can show the entry ids.

use std::collections::{HashMap, HashSet};

use anki::collection::Collection;
use anki::deckconfig::DeckConfigId;
use anki::decks::{DeckId, DeckKind};
use anki::notetype::NotetypeId;

use crate::{Apply, DEFAULT_PRESET, Package, Result, fail, preset};

/// What [`apply`](crate::apply) would change in a collection.
#[derive(Debug, Default, PartialEq)]
pub struct Diff {
    /// Note types that the collection does not have.
    pub notetypes_added: Vec<String>,
    /// Note types whose templates, CSS or names change. A note type that is newer in the
    /// collection stays as it is, so it is not listed.
    pub notetypes_changed: Vec<String>,
    /// Note types whose fields or card types are added, removed or moved, or that the collection
    /// has notes of under another note type (the import changes their note type). Each one
    /// forces a full sync.
    pub schema_changed: Vec<String>,
    /// Copies of a package note type from an import without "merge note types", as (copy, note
    /// type). The import moves their notes to the note type and deletes the copy, which forces a
    /// full sync.
    pub notetypes_merged: Vec<(String, String)>,
    /// Presets that the collection does not have.
    pub presets_added: Vec<String>,
    /// Presets whose name or options change: the preset name and the `deck.toml` keys that
    /// change (`name` for the name).
    pub presets_changed: Vec<(String, Vec<&'static str>)>,
    /// Decks that the collection does not have.
    pub decks_added: Vec<String>,
    /// Decks that get a new name, as (name in the collection, new name).
    pub decks_renamed: Vec<(String, String)>,
    /// Decks whose preset is set back to the project's preset.
    pub decks_preset: Vec<String>,
    /// GUIDs of notes that the collection does not have.
    pub notes_added: Vec<String>,
    /// GUIDs of notes whose fields or tags change.
    pub notes_changed: Vec<String>,
    /// GUIDs of notes that differ, but the collection's note is newer, because it was edited in
    /// Anki. "Update notes if newer" keeps it, unless [`Apply::overwrite`].
    pub notes_kept: Vec<String>,
    /// The number of notes that stay as they are.
    pub notes_unchanged: usize,
    /// GUIDs of notes of the package's note types that the package does not have.
    pub notes_removed: Vec<String>,
    /// The number of cards that move to the deck of their note.
    pub cards_moved: usize,
}

impl Diff {
    /// Whether `apply` would change nothing.
    pub fn is_empty(&self) -> bool {
        *self
            == Diff {
                notes_unchanged: self.notes_unchanged,
                ..Diff::default()
            }
    }
}

/// Compares `package` with `col`. `request` is what the caller passes to `apply`: the known deck
/// ids and the notes whose deck changed. It gives the same errors as `apply` for a package that
/// `apply` refuses. [`Diff::notes_removed`] does not depend on
/// [`Apply::delete`].
pub fn diff(col: &mut Collection, package: &Package, request: &Apply) -> Result<Diff> {
    crate::apply::check(col, package, request)?;
    let mut diff = Diff::default();
    let all = col.get_all_notetypes().map_err(fail)?;

    for notetype in &package.notetypes {
        let Some(current) = col.get_notetype(NotetypeId(notetype.id)).map_err(fail)? else {
            diff.notetypes_added.push(notetype.name.clone());
            continue;
        };
        let siblings: Vec<(String, String)> = all
            .iter()
            .filter(|other| other.id.0 != notetype.id && other.config.original_id == Some(notetype.id))
            .map(|other| (other.name.clone(), notetype.name.clone()))
            .collect();
        if !siblings.is_empty() {
            diff.notetypes_merged.extend(siblings);
            diff.schema_changed.push(notetype.name.clone());
            continue;
        }
        let fields: Vec<Option<i64>> = current.fields.iter().map(|field| field.config.id).collect();
        let cards: Vec<Option<i64>> = current.templates.iter().map(|card| card.config.id).collect();
        let wanted_fields: Vec<Option<i64>> = notetype.fields.iter().map(|field| Some(field.id)).collect();
        let wanted_cards: Vec<Option<i64>> = notetype.cards.iter().map(|card| Some(card.id)).collect();
        if fields != wanted_fields || cards != wanted_cards || current.config.sort_field_idx != notetype.sort_field {
            diff.schema_changed.push(notetype.name.clone());
            continue;
        }
        let same_fields = current.fields.iter().zip(&notetype.fields).all(|(have, want)| {
            have.name == want.name
                && have.config.description == want.description
                && have.config.collapsed == want.collapsed
        });
        let same_cards = current.templates.iter().zip(&notetype.cards).all(|(have, want)| {
            have.name == want.name && have.config.q_format == want.front && have.config.a_format == want.back
        });
        let differs =
            current.name != notetype.name || current.config.css != notetype.css || !same_fields || !same_cards;
        // "Update note types if newer" keeps a note type that was changed in Anki later.
        if differs && current.mtime_secs.0 < notetype.modified {
            diff.notetypes_changed.push(notetype.name.clone());
        }
    }

    for preset in &package.presets {
        let Some(current) = Collection::get_deck_config(col, DeckConfigId(preset.id), false).map_err(fail)? else {
            diff.presets_added.push(preset.name.clone());
            continue;
        };
        let mut keys = Vec::new();
        if current.name != preset.name {
            keys.push("name");
        }
        keys.extend(preset::changed(&preset.options, &current));
        if !keys.is_empty() {
            diff.presets_changed.push((preset.name.clone(), keys));
        }
    }

    let mut targets = HashMap::new();
    for deck in &package.decks {
        let known = match request.decks.get(&deck.id) {
            Some(id) => col.get_deck(DeckId(*id)).map_err(fail)?,
            None => None,
        };
        let found = match known {
            Some(found) => Some(found),
            None => match col.get_deck_id(&deck.name).map_err(fail)? {
                Some(id) => col.get_deck(id).map_err(fail)?,
                None => None,
            },
        };
        let Some(found) = found else {
            diff.decks_added.push(deck.name.clone());
            continue;
        };
        targets.insert(deck.id, found.id.0);
        let name = found.human_name();
        if name != deck.name {
            diff.decks_renamed.push((name, deck.name.clone()));
        }
        if let DeckKind::Normal(normal) = &found.kind
            && normal.config_id != deck.preset.unwrap_or(DEFAULT_PRESET)
        {
            diff.decks_preset.push(deck.name.clone());
        }
    }

    let notetypes: HashMap<i64, &str> =
        package.notetypes.iter().map(|notetype| (notetype.id, notetype.name.as_str())).collect();
    let mut existing: HashMap<String, (i64, String, String, i64)> = HashMap::new();
    {
        let db = col.storage.db();
        let mut statement = db.prepare("select guid, mid, flds, tags, mod from notes").map_err(fail)?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))))
            .map_err(fail)?;
        for row in rows {
            let (guid, note) = row.map_err(fail)?;
            existing.insert(guid, note);
        }
    }
    let package_guids: HashSet<&str> = package.notes.iter().map(|note| note.guid.as_str()).collect();
    for note in &package.notes {
        let Some((notetype, fields, tags, modified)) = existing.get(&note.guid) else {
            diff.notes_added.push(note.guid.clone());
            continue;
        };
        if *notetype != note.notetype {
            let name = notetypes.get(&note.notetype).copied().unwrap_or_default().to_string();
            if !diff.schema_changed.contains(&name) {
                diff.schema_changed.push(name);
            }
            diff.notes_changed.push(note.guid.clone());
            continue;
        }
        let have_tags: HashSet<&str> = tags.split_whitespace().collect();
        let want_tags: HashSet<&str> = note.tags.iter().map(String::as_str).collect();
        if *fields == note.fields.join("\u{1f}") && have_tags == want_tags {
            diff.notes_unchanged += 1;
        } else if note.modified > *modified || request.overwrite {
            diff.notes_changed.push(note.guid.clone());
        } else {
            diff.notes_kept.push(note.guid.clone());
        }
    }
    let mut removed: Vec<String> = existing
        .iter()
        .filter(|(guid, (notetype, ..))| notetypes.contains_key(notetype) && !package_guids.contains(guid.as_str()))
        .map(|(guid, _)| guid.clone())
        .collect();
    removed.sort();
    diff.notes_removed = removed;

    for note in package.notes.iter().filter(|note| request.moved.contains(&note.guid)) {
        // A deck that the collection does not have yet gets every card of the note.
        let target = targets.get(&note.deck).copied().unwrap_or(0);
        let moving: i64 = col
            .storage
            .db()
            .query_row(
                "select count() from cards c join notes n on n.id = c.nid where n.guid = ? and c.odid = 0 and c.did != ?",
                (&note.guid, &target),
                |row| row.get(0),
            )
            .map_err(fail)?;
        diff.cards_moved += moving as usize;
    }
    Ok(diff)
}
