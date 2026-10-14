//! A thin wrapper around Anki's Rust core (`rslib`) that writes `.apkg` packages.
//!
//! This is the only crate in the workspace that depends on Anki. The caller describes the
//! whole package as plain data in a [`Package`], with every id already chosen, and
//! [`write_apkg`] turns it into a file that desktop Anki can import. [`import()`] reads a
//! package back into a fresh collection, so that tests can check what Anki actually stores.
//! A [`Session`] syncs a copy of the user's collection with a sync server (see [`open_copy`]),
//! [`diff()`] shows what [`apply()`] would change in that copy, and `apply` writes a package into
//! it.
//!
//! The ids in a [`Package`] are kept exactly. Anki must not assign its own ids, because the
//! next build of the same deck has to update the same notes, decks and presets. A write fails
//! if Anki changes any id or media file name.

use std::fmt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anki::collection::{Collection, CollectionBuilder};
use anki::deckconfig::{DeckConfSchema11, DeckConfig, DeckConfigId};
use anki::decks::{Deck as AnkiDeck, DeckId, DeckKind, DeckSchema11, NativeDeckName};
use anki::import_export::package::ExportAnkiPackageOptions;
use anki::notetype::{CardTemplate, NoteField, Notetype as AnkiNotetype, NotetypeId};
use anki::services::{DeckConfigService, DecksService};
use anki::timestamp::TimestampSecs;
use anki_proto::decks::AddOrUpdateDeckLegacyRequest;
use anki_proto::generic::Json;

mod apply;
mod diff;
mod import;
mod preset;
mod sync;

pub use apply::{Applied, Apply, apply};
pub use diff::{Diff, diff};
pub use import::{ImportOptions, Imported, ImportedNote, ImportedNotetype, import};
pub use preset::{LeechAction, NewGatherOrder, NewSortOrder, PresetOptions, ReviewMix, ReviewSortOrder};
pub use sync::{MEDIA_LIMIT, Pulled, Pushed, Session, open_copy};

const DEFAULT_PRESET: i64 = 1;

/// An error from Anki or from the file system, kept as its message only.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// A [`Result`](std::result::Result) with this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

fn fail(error: impl fmt::Display) -> Error {
    Error(error.to_string())
}

/// Everything that goes into one `.apkg` file, with every id already chosen.
///
/// [`write_apkg`] writes the items in a fixed order: note types, presets, decks, media, then
/// notes. A [`Note`] refers to its note type and deck by id, so those ids must be in the same
/// package.
#[derive(Debug, Default)]
pub struct Package {
    /// The note types that the notes use.
    pub notetypes: Vec<Notetype>,
    /// The presets that the decks use. Anki adds a preset on import only if it does not have it.
    pub presets: Vec<Preset>,
    /// The decks, each with its full name.
    pub decks: Vec<Deck>,
    /// The notes. Anki makes their cards from the card templates of the note type.
    pub notes: Vec<Note>,
    /// The media files that the notes refer to.
    pub media: Vec<Media>,
}

/// An Anki note type: its fields, its card templates and the CSS they share.
#[derive(Debug)]
pub struct Notetype {
    /// Anki's note type id. The caller keeps it the same from one build to the next.
    pub id: i64,
    /// The name that Anki shows.
    pub name: String,
    /// The CSS that all card templates of the note type share.
    pub css: String,
    /// The index in [`fields`](Self::fields) of the field that Anki sorts the browser by.
    pub sort_field: u32,
    /// The modification time in seconds since the Unix epoch. Anki imports a changed note
    /// type only if this time is newer than the time in the collection.
    pub modified: i64,
    /// The fields, in the order of the values of each [`Note`].
    pub fields: Vec<Field>,
    /// The card templates, in template order.
    pub cards: Vec<Card>,
}

/// A field of a [`Notetype`].
#[derive(Debug)]
pub struct Field {
    /// Anki's field id. The caller keeps it the same from one build to the next.
    pub id: i64,
    /// The field name that card templates refer to.
    pub name: String,
    /// The hint that Anki's editor shows in an empty field.
    pub description: String,
    /// Whether Anki's editor shows the field collapsed.
    pub collapsed: bool,
}

/// A card template of a [`Notetype`]. Each template makes one card for each note.
#[derive(Debug)]
pub struct Card {
    /// Anki's template id. The caller keeps it the same from one build to the next.
    pub id: i64,
    /// The card type name that Anki shows.
    pub name: String,
    /// The Anki template source of the question side.
    pub front: String,
    /// The Anki template source of the answer side.
    pub back: String,
}

/// An Anki deck options preset ("deck config" in Anki's code).
#[derive(Debug)]
pub struct Preset {
    /// Anki's preset id. A [`write_apkg`] fails if Anki gives the preset another id.
    pub id: i64,
    /// The name that Anki shows.
    pub name: String,
    /// The options. An option that is `None` keeps the value of Anki's default preset.
    pub options: PresetOptions,
}

/// A deck. Anki makes child decks from names that contain `::`.
#[derive(Debug)]
pub struct Deck {
    /// Anki's deck id. A [`write_apkg`] fails if Anki gives the deck another id.
    pub id: i64,
    /// The full name, with `::` between the names of the parent decks and the deck.
    pub name: String,
    /// The id of a [`Preset`] in the same package. `None` uses Anki's default preset.
    pub preset: Option<i64>,
}

/// A note, with its field values already rendered to the HTML that Anki stores.
#[derive(Debug)]
pub struct Note {
    /// The GUID that Anki uses to match this note to an existing note on import.
    pub guid: String,
    /// The id of a [`Notetype`] in the same package.
    pub notetype: i64,
    /// The id of a [`Deck`] in the same package. All cards of the note go into this deck.
    pub deck: i64,
    /// One value for each field, in the order of [`Notetype::fields`].
    pub fields: Vec<String>,
    /// The tags of the note. A tag that contains `::` is a hierarchical tag in Anki.
    pub tags: Vec<String>,
    /// The modification time in seconds since the Unix epoch. Anki imports a changed note
    /// only if this time is newer than the time in the collection.
    pub modified: i64,
}

/// A media file to copy into the package.
#[derive(Debug)]
pub struct Media {
    /// The file name that the notes refer to. Anki must store the file under this exact name.
    pub name: String,
    /// Where to read the file from.
    pub path: PathBuf,
}

/// Opens the collection in `dir`, or makes a new empty one there.
///
/// The collection file is `collection.anki2`. Media goes in `media/` and `media.db`. The
/// function makes `media/` if it does not exist.
pub fn open(dir: &Path) -> Result<Collection> {
    std::fs::create_dir_all(dir.join("media")).map_err(fail)?;
    CollectionBuilder::new(dir.join("collection.anki2"))
        .set_media_paths(dir.join("media"), dir.join("media.db"))
        .build()
        .map_err(fail)
}

/// Writes `package` to the `.apkg` file `out`, and returns the number of notes in it.
///
/// The function builds the collection in a temporary directory, then exports it with presets
/// and media but without scheduling information. It makes the parent directories of `out`, and
/// gives the file mode 0644 (Anki itself writes 0600).
///
/// It keeps every id and modification time in `package`. It returns an [`Error`] if Anki
/// gives a preset or deck a different id, stores a media file under a different name, or
/// rejects any item. A note type that a note refers to must be in the package.
pub fn write_apkg(package: &Package, out: &Path) -> Result<usize> {
    let work = tempfile::tempdir().map_err(fail)?;
    let mut col = open(work.path())?;
    write_collection(&mut col, package)?;
    col.close(None).map_err(fail)?;

    let mut col = open(work.path())?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(fail)?;
    }
    let options = ExportAnkiPackageOptions {
        with_scheduling: false,
        with_deck_configs: true,
        with_media: true,
        legacy: false,
    };
    let count = col.export_apkg(out, options, "", None).map_err(fail)?;
    col.close(None).map_err(fail)?;
    std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o644)).map_err(fail)?;
    Ok(count)
}

fn write_collection(col: &mut Collection, package: &Package) -> Result<()> {
    for notetype in &package.notetypes {
        let mut anki = AnkiNotetype {
            id: NotetypeId(notetype.id),
            name: notetype.name.clone(),
            mtime_secs: TimestampSecs(notetype.modified),
            ..Default::default()
        };
        for field in &notetype.fields {
            let mut anki_field = NoteField::new(&field.name);
            anki_field.config.id = Some(field.id);
            anki_field.config.description = field.description.clone();
            anki_field.config.collapsed = field.collapsed;
            anki.fields.push(anki_field);
        }
        for card in &notetype.cards {
            let mut template = CardTemplate::new(&card.name, &card.front, &card.back);
            template.config.id = Some(card.id);
            template.mtime_secs = TimestampSecs(notetype.modified);
            anki.templates.push(template);
        }
        anki.config.css = notetype.css.clone();
        anki.config.sort_field_idx = notetype.sort_field;
        col.add_or_update_notetype_with_existing_id(&mut anki, false).map_err(fail)?;
        col.storage
            .db()
            .execute("update notetypes set mtime_secs = ? where id = ?", (notetype.modified, notetype.id))
            .map_err(fail)?;
    }

    for preset in &package.presets {
        let mut config = DeckConfig {
            id: DeckConfigId(preset.id),
            name: preset.name.clone(),
            ..Default::default()
        };
        preset::apply(&preset.options, &mut config);
        let json = serde_json::to_vec(&DeckConfSchema11::from(config)).map_err(fail)?;
        let added = col.add_or_update_deck_config_legacy(Json { json }).map_err(fail)?;
        if added.dcid != preset.id {
            return Err(Error(format!("preset `{}` got id {} instead of {}", preset.name, added.dcid, preset.id)));
        }
    }

    for deck in &package.decks {
        let mut anki = AnkiDeck::new_normal();
        anki.id = DeckId(deck.id);
        anki.name = NativeDeckName::from_human_name(&deck.name);
        if let DeckKind::Normal(normal) = &mut anki.kind {
            normal.config_id = deck.preset.unwrap_or(DEFAULT_PRESET);
        }
        let json = serde_json::to_vec(&DeckSchema11::from(anki)).map_err(fail)?;
        let request = AddOrUpdateDeckLegacyRequest { deck: json, preserve_usn_and_mtime: true };
        let added = col.add_or_update_deck_legacy(request).map_err(fail)?;
        if added.did != deck.id {
            return Err(Error(format!("deck `{}` got id {} instead of {}", deck.name, added.did, deck.id)));
        }
    }

    for media in &package.media {
        let bytes = std::fs::read(&media.path).map_err(|error| Error(format!("{}: {error}", media.path.display())))?;
        let stored = col.media().map_err(fail)?.add_file(&media.name, &bytes).map_err(fail)?;
        if stored != media.name {
            return Err(Error(format!("Anki stored media file `{}` as `{stored}`", media.name)));
        }
    }

    for note in &package.notes {
        let notetype = col
            .get_notetype(NotetypeId(note.notetype))
            .map_err(fail)?
            .ok_or_else(|| Error(format!("note type {} is missing", note.notetype)))?;
        let mut anki = notetype.new_note();
        anki.guid = note.guid.clone();
        for (index, value) in note.fields.iter().enumerate() {
            anki.set_field(index, value).map_err(fail)?;
        }
        anki.tags = note.tags.clone();
        col.add_note(&mut anki, DeckId(note.deck)).map_err(fail)?;
        col.storage
            .db()
            .execute("update notes set mod = ? where guid = ?", (note.modified, &note.guid))
            .map_err(fail)?;
    }
    Ok(())
}
