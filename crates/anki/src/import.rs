//! Imports an `.apkg` with Anki's own importer, and reads back what the collection then holds.
//!
//! The tests use [`import`] to check what desktop Anki keeps from a package that
//! [`write_apkg`](crate::write_apkg) wrote. [`ImportOptions`] maps to the options that desktop
//! Anki asks for on import. [`Imported`] is a plain snapshot of the whole collection after the
//! import, so that tests can compare it with `assert_eq!`.

use std::collections::HashMap;
use std::path::Path;

use anki::import_export::package::{ImportAnkiPackageOptions, UpdateCondition};

use crate::{Result, fail, open};

/// The result of one [`import`]: Anki's note counts for this import, then the state of the
/// whole collection.
///
/// The collection lists include items that were there before the import, such as Anki's stock
/// note types and earlier imports into the same directory.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    /// The number of notes that Anki added.
    pub new: usize,
    /// The number of existing notes that Anki updated.
    pub updated: usize,
    /// The number of existing notes that Anki did not update, because the update condition was
    /// false. Notes that Anki skips because of a note type conflict (merge off) are not counted.
    pub duplicate: usize,
    /// Every note type in the collection, by ascending id.
    pub notetypes: Vec<ImportedNotetype>,
    /// Every preset as `(id, name, new cards per day, desired retention)`, by ascending id.
    pub presets: Vec<(i64, String, u32, f32)>,
    /// Every deck as `(full name, preset id)`, sorted. The preset id is 0 for a filtered deck.
    pub decks: Vec<(String, i64)>,
    /// Every note, by ascending note id.
    pub notes: Vec<ImportedNote>,
    /// Every file in the media folder as `(file name, content)`, sorted by name.
    pub media: Vec<(String, Vec<u8>)>,
}

/// A note type as the collection stores it after the import.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedNotetype {
    /// The note type id in the collection.
    pub id: i64,
    /// The note type name.
    pub name: String,
    /// The modification time in seconds since the Unix epoch.
    pub modified: i64,
    /// The CSS that the card templates share.
    pub css: String,
    /// Each field as `(field id, name, collapsed)`, in field order.
    pub fields: Vec<(Option<i64>, String, bool)>,
    /// Each card template as `(template id, name, front, back)`, in template order.
    pub cards: Vec<(Option<i64>, String, String, String)>,
}

/// A note as the collection stores it after the import, with a summary of its cards.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedNote {
    /// The GUID of the note.
    pub guid: String,
    /// The id of the note type.
    pub notetype: i64,
    /// The modification time in seconds since the Unix epoch.
    pub modified: i64,
    /// The full name of the deck of the card with the lowest deck id. Empty if the note has no
    /// cards.
    pub deck: String,
    /// Anki's space-separated tag string, without the leading and trailing spaces.
    pub tags: String,
    /// The field values, in field order.
    pub fields: Vec<String>,
    /// The template ordinals (0-based) of the note's cards, in ascending order.
    pub cards: Vec<u32>,
    /// The lowest `due` value of the note's cards. 0 if the note has no cards.
    pub due: i64,
}

/// The import options that desktop Anki shows. `docs/import-behaviour.md` describes their
/// effect.
#[derive(Debug, Clone, Copy)]
pub struct ImportOptions {
    /// "Update notes" and "Update note types": `true` is "If newer" (Anki's default), `false`
    /// is "Always".
    pub update_if_newer: bool,
    /// "Merge note types".
    pub merge_notetypes: bool,
    /// "Import any deck presets".
    pub with_deck_configs: bool,
}

/// Imports the `.apkg` file `apkg` into the collection in `dir`, and returns what the
/// collection then holds.
///
/// The function makes a new empty collection if `dir` has none (see [`open`]).
/// It imports without scheduling information. It returns an [`Error`](crate::Error) if Anki
/// rejects the package, or if the collection or its media folder cannot be read.
pub fn import(apkg: &Path, dir: &Path, options: ImportOptions) -> Result<Imported> {
    let mut col = open(dir)?;
    let condition = if options.update_if_newer { UpdateCondition::IfNewer } else { UpdateCondition::Always } as i32;
    let options = ImportAnkiPackageOptions {
        merge_notetypes: options.merge_notetypes,
        update_notes: condition,
        update_notetypes: condition,
        with_scheduling: false,
        with_deck_configs: options.with_deck_configs,
    };
    let output = col.import_apkg(apkg, options).map_err(fail)?.output;

    let mut notetypes = Vec::new();
    for notetype in col.get_all_notetypes().map_err(fail)? {
        notetypes.push(ImportedNotetype {
            id: notetype.id.0,
            name: notetype.name.clone(),
            modified: notetype.mtime_secs.0,
            css: notetype.config.css.clone(),
            fields: notetype
                .fields
                .iter()
                .map(|field| (field.config.id, field.name.clone(), field.config.collapsed))
                .collect(),
            cards: notetype
                .templates
                .iter()
                .map(|card| {
                    (card.config.id, card.name.clone(), card.config.q_format.clone(), card.config.a_format.clone())
                })
                .collect(),
        });
    }
    notetypes.sort_by_key(|notetype| notetype.id);

    let names: HashMap<i64, String> =
        col.get_all_deck_names(false).map_err(fail)?.into_iter().map(|(id, name)| (id.0, name)).collect();
    let db = col.storage.db();
    let presets = db
        .prepare("select id, name from deck_config order by id")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<std::result::Result<Vec<(i64, String)>, _>>()
        })
        .map_err(fail)?;
    let mut decks = Vec::new();
    for (id, name) in &names {
        let deck = col.get_deck(anki::decks::DeckId(*id)).map_err(fail)?.expect("deck");
        decks.push((name.clone(), deck.config_id().map_or(0, |config| config.0)));
    }
    decks.sort();
    let mut preset_values = Vec::new();
    for (id, name) in presets {
        let config = anki::collection::Collection::get_deck_config(&col, anki::deckconfig::DeckConfigId(id), false)
            .map_err(fail)?
            .expect("preset");
        preset_values.push((id, name, config.inner.new_per_day, config.inner.desired_retention));
    }

    let db = col.storage.db();
    let mut statement = db
        .prepare(
            "select n.guid, n.mid, n.mod, n.tags, n.flds, \
             (select group_concat(c.ord) from (select ord from cards where nid = n.id order by ord) c), \
             (select min(c.due) from cards c where c.nid = n.id), \
             (select min(c.did) from cards c where c.nid = n.id) \
             from notes n order by n.id",
        )
        .map_err(fail)?;
    let notes = statement
        .query_map([], |row| {
            let fields: String = row.get(4)?;
            let cards: Option<String> = row.get(5)?;
            let deck: Option<i64> = row.get(7)?;
            Ok(ImportedNote {
                guid: row.get(0)?,
                notetype: row.get(1)?,
                modified: row.get(2)?,
                tags: row.get::<_, String>(3)?.trim().to_string(),
                fields: fields.split('\x1f').map(str::to_string).collect(),
                cards: cards.unwrap_or_default().split(',').filter_map(|ord| ord.parse().ok()).collect(),
                due: row.get::<_, Option<i64>>(6)?.unwrap_or_default(),
                deck: deck.and_then(|deck| names.get(&deck).cloned()).unwrap_or_default(),
            })
        })
        .map_err(fail)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(fail)?;
    drop(statement);

    let mut media = Vec::new();
    for entry in std::fs::read_dir(dir.join("media")).map_err(fail)? {
        let path = entry.map_err(fail)?.path();
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        media.push((name, std::fs::read(&path).map_err(fail)?));
    }
    media.sort();
    col.close(None).map_err(fail)?;

    Ok(Imported {
        new: output.new.len(),
        updated: output.updated.len(),
        duplicate: output.duplicate.len(),
        notetypes,
        presets: preset_values,
        decks,
        notes,
        media,
    })
}
