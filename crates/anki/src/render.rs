//! Renders the cards of a [`Package`] with Anki's own template renderer, for `janky preview`.
//!
//! [`render`] writes the package without media into a temporary collection, then renders each card template of
//! each note with rslib. The HTML is what Anki's reviewer gets before it replaces the play tags. The `[[type:…]]`
//! tags of the question are already replaced (see [`crate::type_answer`]), and [`crate::compare_typed`] replaces
//! those of the answer. Because the notes are in the collection, `{{Deck}}` and the other special fields show the
//! same values as in Anki.
//!
//! A sample note type gets one note that is not in the collection. rslib fills its empty fields with
//! `(Field name)`, as in Anki's card layout screen. The preview uses samples for note types that no entry uses.

use anki::collection::Collection;
use anki::notes::{Note as AnkiNote, NoteId};
use anki::notetype::NotetypeId;
use anki::template::RenderedNode;
use anki_proto::card_rendering::av_tag;

use crate::type_answer::{self, TypeAnswer};
use crate::{Error, Package, Result, fail, open, write_collection};

/// The rendered cards of a package.
#[derive(Debug)]
pub struct Rendered {
    /// One list for each note of the package, with one card for each card template of its note type.
    pub notes: Vec<Vec<std::result::Result<RenderedCard, String>>>,
    /// One list for each note type id of the samples, with one card for each card template.
    pub samples: Vec<Vec<std::result::Result<RenderedCard, String>>>,
}

/// The two sides of one card.
#[derive(Debug)]
pub struct RenderedCard {
    /// The question side.
    pub question: RenderedSide,
    /// The answer side.
    pub answer: RenderedSide,
    /// Whether the front side is empty. Anki does not make such a card, and the question shows Anki's message.
    pub is_empty: bool,
    /// What a typed answer is compared with, if the question has a `[[type:…]]` tag for a field that is not empty.
    pub type_answer: Option<TypeAnswer>,
}

/// One side of a card.
#[derive(Debug)]
pub struct RenderedSide {
    /// The HTML, with `[anki:play:q:N]` or `[anki:play:a:N]` in place of each sound and speech tag.
    pub html: String,
    /// The sounds, in the order of the play tags. `N` is the index in this list.
    pub sounds: Vec<Sound>,
}

/// A sound that a play tag refers to.
#[derive(Debug, PartialEq, Eq)]
pub enum Sound {
    /// A media file, from `[sound:name]`.
    File(String),
    /// Text to speech, from `{{tts …:Field}}`.
    Speech {
        /// The text to speak.
        text: String,
        /// The language code, for example `en_US`.
        language: String,
    },
}

/// Renders each card template of each note in `package`, and of one sample note for each note type id in
/// `samples`.
///
/// The function reads no media files. It returns an [`Error`] if Anki rejects an item of the package, or if a
/// sample note type is not in the package. An error that affects only one card goes in the `Err` of that card.
pub fn render(package: &Package, samples: &[i64]) -> Result<Rendered> {
    let work = tempfile::tempdir().map_err(fail)?;
    let mut col = open(work.path())?;
    write_collection(&mut col, package, false)?;

    let mut notes = Vec::with_capacity(package.notes.len());
    for note in &package.notes {
        let id = col
            .storage
            .db()
            .query_row("select id from notes where guid = ?", [&note.guid], |row| row.get(0))
            .map_err(fail)?;
        let mut anki = col
            .storage
            .get_note(NoteId(id))
            .map_err(fail)?
            .ok_or_else(|| Error(format!("note `{}` is missing", note.guid)))?;
        notes.push(cards(&mut col, &mut anki, false)?);
    }

    let mut rendered_samples = Vec::with_capacity(samples.len());
    for &id in samples {
        let notetype = col
            .get_notetype(NotetypeId(id))
            .map_err(fail)?
            .ok_or_else(|| Error(format!("note type {id} is missing")))?;
        let mut anki = notetype.new_note();
        rendered_samples.push(cards(&mut col, &mut anki, true)?);
    }

    col.close(None).map_err(fail)?;
    Ok(Rendered { notes, samples: rendered_samples })
}

fn cards(
    col: &mut Collection,
    note: &mut AnkiNote,
    sample: bool,
) -> Result<Vec<std::result::Result<RenderedCard, String>>> {
    let notetype = col
        .get_notetype(note.notetype_id)
        .map_err(fail)?
        .ok_or_else(|| Error(format!("note type {} is missing", note.notetype_id)))?;
    let mut cards = Vec::with_capacity(notetype.templates.len());
    for (ord, template) in notetype.templates.iter().enumerate() {
        let ord = u16::try_from(ord).expect("Anki allows fewer than 65536 card templates");
        let card = match col.render_uncommitted_card(note, template, ord, sample, false) {
            Ok(output) => {
                let mut question = side(col, &join(&output.qnodes), true);
                let type_answer = type_answer::question(col, &mut question.html, note, &notetype);
                Ok(RenderedCard {
                    question,
                    answer: side(col, &join(&output.anodes), false),
                    is_empty: output.is_empty,
                    type_answer,
                })
            },
            Err(error) => Err(error.message(col.tr())),
        };
        cards.push(card);
    }
    Ok(cards)
}

/// The text of card HTML on one line, as Anki's browser shows it: without tags, with entities decoded, and with the
/// file name of each image. The `[anki:play:…]` tags stay.
pub fn html_to_text(html: &str) -> String {
    anki::text::html_to_text_line(html, true).into_owned()
}

/// Joins the nodes of one side, as Anki's Python layer does when no add-on handles the filters that rslib left.
fn join(nodes: &[RenderedNode]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            RenderedNode::Text { text } => text.as_str(),
            RenderedNode::Replacement { current_text, .. } => current_text.as_str(),
        })
        .collect()
}

fn side(col: &Collection, html: &str, question: bool) -> RenderedSide {
    let (html, tags) = anki::card_rendering::extract_av_tags(html, question, col.tr());
    let sounds = tags
        .into_iter()
        .map(|tag| match tag.value.expect("rslib sets the value of each AV tag") {
            av_tag::Value::SoundOrVideo(name) => Sound::File(name),
            av_tag::Value::Tts(speech) => Sound::Speech {
                text: speech.field_text,
                language: speech.lang,
            },
        })
        .collect();
    RenderedSide { html, sounds }
}
