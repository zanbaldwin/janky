//! Applies a [`Package`] to a copy of a user's collection, where an `.apkg` import falls short.
//!
//! [`apply`] imports the package like desktop Anki ("merge note types", "update notes if
//! newer", presets), and then does what the importer does not: it renames decks, updates
//! presets that the collection already has, sets the preset of each deck, moves the cards of
//! notes that changed deck in the project, and deletes notes on request.
//!
//! The importer matches decks by name, so a deck in the collection can have a different id than
//! in the package. [`Applied::decks`] maps one to the other, and the caller gives the map back
//! in [`Apply::decks`] next time, so that a renamed deck is found.

use std::collections::{HashMap, HashSet};

use anki::collection::Collection;
use anki::deckconfig::{DeckConfSchema11, DeckConfigId};
use anki::decks::{DeckId, DeckKind};
use anki::import_export::package::{ImportAnkiPackageOptions, UpdateCondition};
use anki::notes::NoteId;
use anki::notetype::NotetypeId;
use anki::prelude::CardId;
use anki::services::DeckConfigService;
use anki_proto::generic::Json;

use crate::{Package, Result, fail, preset, write_apkg};

/// What [`apply`] needs besides the package.
#[derive(Debug, Default)]
pub struct Apply {
    /// The collection deck id of each package deck id, from the last [`Applied::decks`].
    /// A deck that is not in the map is found by its full name.
    pub decks: HashMap<i64, i64>,
    /// The GUIDs of the notes whose deck changed in the project since the last publish. Only
    /// their cards move, so a card that the user moved in Anki stays where it is.
    pub moved: Vec<String>,
    /// The GUIDs of the notes to delete. Their cards go with them. Anki keeps their review log.
    pub delete: Vec<String>,
}

/// What [`apply`] changed.
#[derive(Debug, Default, PartialEq)]
pub struct Applied {
    /// The collection deck id of each package deck id.
    pub decks: HashMap<i64, i64>,
    /// The decks that got a new name, as (old name, new name).
    pub renamed: Vec<(String, String)>,
    /// The ids of the presets that changed.
    pub presets: Vec<i64>,
    /// The number of cards that moved to another deck.
    pub moved: usize,
    /// The number of notes that were deleted.
    pub deleted: usize,
}

/// Applies `package` to `col`. Each change gets the usn of a local change, and the collection's
/// modification time is set after its last sync, so the next normal sync sends the changes.
///
/// A preset keeps the options that the package does not set. A card in a filtered deck does
/// not move. A GUID in [`Apply::delete`] that the collection does not have is ignored.
///
/// A rename to a name that another deck of the collection has is an error, because Anki would
/// add `+` to the name, and the import would then fill the other deck. So is a deck name, or
/// the name of one of its parents, that is a filtered deck in the collection.
///
/// [`diff`](crate::diff) makes the same two checks, so a plan shows these errors before anything
/// changes.
///
/// The steps are separate transactions. After an error, the copy can hold part of the changes,
/// so the caller must not push it, and must replace it with a full download first.
pub fn apply(col: &mut Collection, package: &Package, request: &Apply) -> Result<Applied> {
    let mut applied = Applied::default();
    check(col, package, request)?;
    for deck in &package.decks {
        let Some(id) = request.decks.get(&deck.id) else { continue };
        let Some(current) = col.get_deck(DeckId(*id)).map_err(fail)? else { continue };
        let old = current.human_name();
        if old != deck.name {
            col.rename_deck(DeckId(*id), &deck.name).map_err(fail)?;
            applied.renamed.push((old, deck.name.clone()));
        }
    }

    let work = tempfile::tempdir().map_err(fail)?;
    let apkg = work.path().join("publish.apkg");
    write_apkg(package, &apkg)?;
    let condition = UpdateCondition::IfNewer as i32;
    let options = ImportAnkiPackageOptions {
        merge_notetypes: true,
        update_notes: condition,
        update_notetypes: condition,
        with_scheduling: false,
        with_deck_configs: true,
    };
    let before = note_times(col)?;
    col.import_apkg(&apkg, options).map_err(fail)?;

    // "Merge note types" keeps the collection's order and puts a new field or card type last.
    // The project's order wins, so that the next diff finds no schema change.
    for notetype in &package.notetypes {
        let Some(current) = col.get_notetype(NotetypeId(notetype.id)).map_err(fail)? else {
            continue;
        };
        let mut wanted = (*current).clone();
        let fields: Vec<i64> = notetype.fields.iter().map(|field| field.id).collect();
        let cards: Vec<i64> = notetype.cards.iter().map(|card| card.id).collect();
        let fields_moved = reorder(&mut wanted.fields, &fields, |field| field.config.id);
        let cards_moved = reorder(&mut wanted.templates, &cards, |card| card.config.id);
        if fields_moved || cards_moved {
            col.update_notetype(&mut wanted, false).map_err(fail)?;
        }
    }

    // A new field rewrites every note of its note type during the import, before the notes are
    // compared, so "update notes if newer" keeps them all. "Newer" is decided from the times
    // before the import, as in the diff.
    for note in &package.notes {
        let Some((id, modified)) = before.get(&note.guid) else { continue };
        if note.modified <= *modified {
            continue;
        }
        let Some(mut current) = col.storage.get_note(*id).map_err(fail)? else {
            continue;
        };
        if current.notetype_id.0 != note.notetype {
            continue;
        }
        let have_tags: HashSet<&str> = current.tags.iter().map(String::as_str).collect();
        let want_tags: HashSet<&str> = note.tags.iter().map(String::as_str).collect();
        if current.fields() == &note.fields && have_tags == want_tags {
            continue;
        }
        for (index, text) in note.fields.iter().enumerate() {
            current.set_field(index, text).map_err(fail)?;
        }
        current.tags = note.tags.clone();
        col.update_note(&mut current).map_err(fail)?;
    }

    for preset in &package.presets {
        let Some(current) = Collection::get_deck_config(col, DeckConfigId(preset.id), false).map_err(fail)? else {
            return Err(crate::Error(format!("preset `{}` is missing after the import", preset.name)));
        };
        let mut wanted = current.clone();
        wanted.name = preset.name.clone();
        preset::apply(&preset.options, &mut wanted);
        if wanted.name != current.name || wanted.inner != current.inner {
            let json = serde_json::to_vec(&DeckConfSchema11::from(wanted)).map_err(fail)?;
            let updated = col.add_or_update_deck_config_legacy(Json { json }).map_err(fail)?;
            if updated.dcid != preset.id {
                return Err(crate::Error(format!(
                    "preset `{}` got id {} instead of {}",
                    preset.name, updated.dcid, preset.id
                )));
            }
            applied.presets.push(preset.id);
        }
    }

    for deck in &package.decks {
        let id = col.get_or_create_normal_deck(&deck.name).map_err(fail)?.id;
        applied.decks.insert(deck.id, id.0);
        let preset = deck.preset.unwrap_or(crate::DEFAULT_PRESET);
        let mut current = (*col.get_deck(id).map_err(fail)?.ok_or_else(|| fail("deck vanished"))?).clone();
        if let DeckKind::Normal(normal) = &mut current.kind
            && normal.config_id != preset
        {
            normal.config_id = preset;
            col.update_deck(&mut current).map_err(fail)?;
        }
    }

    for note in package.notes.iter().filter(|note| request.moved.contains(&note.guid)) {
        let Some(target) = applied.decks.get(&note.deck) else { continue };
        let cards: Vec<CardId> = col
            .storage
            .db()
            .prepare(
                "select c.id from cards c join notes n on n.id = c.nid where n.guid = ? and c.odid = 0 and c.did != ?",
            )
            .map_err(fail)?
            .query_map((&note.guid, target), |row| row.get(0).map(CardId))
            .map_err(fail)?
            .collect::<std::result::Result<_, _>>()
            .map_err(fail)?;
        if !cards.is_empty() {
            applied.moved += col.set_deck(&cards, DeckId(*target)).map_err(fail)?.output;
        }
    }

    let mut doomed: Vec<NoteId> = Vec::new();
    for guid in &request.delete {
        let found: Vec<NoteId> = col
            .storage
            .db()
            .prepare("select id from notes where guid = ?")
            .map_err(fail)?
            .query_map([guid], |row| row.get(0).map(NoteId))
            .map_err(fail)?
            .collect::<std::result::Result<_, _>>()
            .map_err(fail)?;
        doomed.extend(found);
    }
    if !doomed.is_empty() {
        applied.deleted = col.remove_notes(&doomed).map_err(fail)?.output;
    }
    // The server answers "no changes" when both collections have the same modification time.
    // After a sync, the copy has the server's time, and the changes above have the local time,
    // which can be equal by chance. A time after the last sync makes the next push send them.
    col.storage.db().execute("update col set mod = max(mod, ls + 1)", []).map_err(fail)?;
    Ok(applied)
}

/// The id and modification time of each note in `col`, by GUID.
fn note_times(col: &Collection) -> Result<HashMap<String, (NoteId, i64)>> {
    let db = col.storage.db();
    let mut statement = db.prepare("select guid, id, mod from notes").map_err(fail)?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, (NoteId(row.get(1)?), row.get(2)?))))
        .map_err(fail)?;
    rows.collect::<std::result::Result<_, _>>().map_err(fail)
}

/// Sorts `items` into the order of `wanted`, by id. Items that `wanted` does not have go last, in
/// their order. Returns whether anything moved.
fn reorder<T>(items: &mut [T], wanted: &[i64], id: impl Fn(&T) -> Option<i64>) -> bool {
    let rank = |item: &T| id(item).and_then(|id| wanted.iter().position(|want| *want == id)).unwrap_or(wanted.len());
    if items.is_sorted_by_key(rank) {
        return false;
    }
    items.sort_by_key(rank);
    true
}

/// The checks that stop [`apply`] before it changes anything: a rename to a name that another
/// deck has, and a deck name (or the name of a parent) that is a filtered deck.
pub(crate) fn check(col: &mut Collection, package: &Package, request: &Apply) -> Result<()> {
    for deck in &package.decks {
        if let Some(id) = request.decks.get(&deck.id)
            && let Some(current) = col.get_deck(DeckId(*id)).map_err(fail)?
            && current.human_name() != deck.name
            && col.get_deck_id(&deck.name).map_err(fail)?.is_some_and(|other| other.0 != *id)
        {
            return Err(crate::Error(format!(
                "cannot rename the deck `{}` to `{}`: the collection already has a deck with that name",
                current.human_name(),
                deck.name
            )));
        }
        let parts: Vec<&str> = deck.name.split("::").collect();
        for depth in 1..=parts.len() {
            let name = parts[..depth].join("::");
            let Some(id) = col.get_deck_id(&name).map_err(fail)? else { continue };
            let found = col.get_deck(id).map_err(fail)?;
            if found.is_some_and(|found| matches!(found.kind, DeckKind::Filtered(_))) {
                return Err(crate::Error(format!(
                    "the deck `{}` cannot be published: `{name}` is a filtered deck in the collection",
                    deck.name
                )));
            }
        }
    }
    Ok(())
}
