//! `janky show`: prints the cards of one entry, for AI agents and scripts that make a deck.
//!
//! The command runs the pipeline of [`crate::preview`] once, renders only the note of the identifier, and prints the
//! result on stdout. It writes no files, and generators never run. An identifier is `<entry>` or `<entry>/<card>`.
//! An entry id can contain `/` if the schema of its note type allows any text, but a card key never can. So the whole
//! identifier is first tried as an entry id, and only then split at the last `/`. With `--sample`, the identifier
//! names a note type, and the command prints its sample note, as in the preview. Without an identifier, the command
//! lists the identifiers in the order of the preview table.
//!
//! The text output has a header line for each card and one line for each side. Play tags become `[sound:<name>]`
//! or `[tts <language>: <text>]`, and a type answer input becomes `[type answer]`. With `--json`, the output is one
//! line after the diagnostics, as the plan line of `janky plan`, and `--format` has no effect.
//!
//! `--format html` prints one card side as a document that needs no server, so that an agent can save it to a file
//! and open it with a headless browser through a `file:///` URL. The CSS and the JS of the card shim are in the
//! document, and the card data maps each media name to the `file://` URL of the file. MathJax does not run there,
//! because a browser does not load its fonts for a `file:///` page.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde_json::{Value, json};

use crate::diagnostic::{Diagnostic, Diagnostics, json_text, visible};
use crate::lock;
use crate::preview::{self, Platform, Row, Selection};
use crate::project;

/// One side of a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Side {
    Front,
    Back,
}

/// The output of `janky show` without `--json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Format {
    #[default]
    Text,
    Html,
}

/// What `janky show` prints.
pub struct Request<'a> {
    /// `None` lists the identifiers.
    pub identifier: Option<&'a str>,
    /// `None` prints both sides.
    pub side: Option<Side>,
    /// The typed answer, for a back with `{{type:Field}}`.
    pub typed: &'a str,
    /// The identifier names a note type, and the output is its sample note.
    pub sample: bool,
    /// One JSON line in place of the text.
    pub json: bool,
    /// Text, or one card side as an HTML document. `json` has priority.
    pub format: Format,
    /// The night classes on the card body, for the HTML output.
    pub night: bool,
    /// The platform classes on the card body, for the HTML output.
    pub platform: Platform,
}

/// Runs `janky show`, and returns the text for stdout. Problems go into `diagnostics`.
pub fn show(
    deck: &Path,
    options: project::Options,
    request: &Request,
    diagnostics: &mut Diagnostics,
) -> Option<String> {
    let html = request.format == Format::Html && !request.json;
    if html && !preview::ui_built() {
        diagnostics.push(
            Diagnostic::error(
                "show::no-ui",
                "`--format html` needs the preview UI, and this janky was built without it",
            )
            .with_help("run `just assets`, then build janky again"),
        );
        return None;
    }
    if html && request.side.is_none() {
        diagnostics.push(
            Diagnostic::error("show::one-side", "`--format html` shows one side of one card")
                .with_help("add `--side front` or `--side back`"),
        );
        return None;
    }
    let mut loaded = preview::load(deck, options, diagnostics)?;
    let Some(identifier) = request.identifier else {
        let identifiers = identifiers(&loaded, request.sample);
        return Some(if request.json {
            format!("{}\n", json_text(&json!({ "identifiers": identifiers })))
        } else {
            identifiers.iter().map(|identifier| format!("{}\n", visible(identifier))).collect()
        });
    };
    let (selection, card) = resolve(&loaded, identifier, request.sample, diagnostics)?;
    let media = preview::media_map(&loaded);
    let rows = match preview::render(&mut loaded, &selection) {
        Ok(rows) => rows,
        Err(message) => {
            diagnostics.push(Diagnostic::error("show::render", message));
            return None;
        },
    };
    let rows: Vec<Row> = rows.into_iter().filter(|row| card.is_none_or(|card| row.card_key == card)).collect();
    let cards: Vec<Card> = rows.iter().map(|row| Card::new(row, request)).collect();
    if html {
        return html_document(identifier, &cards, &media, request, diagnostics);
    }
    Some(if request.json {
        format!("{}\n", json_text(&json!({ "cards": cards.iter().map(Card::to_json).collect::<Vec<_>>() })))
    } else {
        let blocks: Vec<String> = cards.iter().map(Card::to_text).collect();
        visible(&blocks.join("\n"))
    })
}

/// One card side as a document for a `file:///` URL.
fn html_document(
    identifier: &str,
    cards: &[Card],
    media: &std::collections::HashMap<String, std::path::PathBuf>,
    request: &Request,
    diagnostics: &mut Diagnostics,
) -> Option<String> {
    let [card] = cards else {
        let first = cards.first().map_or("card", |card| card.row.card_key.as_str());
        diagnostics.push(
            Diagnostic::error(
                "show::one-side",
                format!("`{identifier}` names {} cards, and `--format html` shows one", cards.len()),
            )
            .with_help(format!("add the card key, for example `{identifier}/{first}`")),
        );
        return None;
    };
    if let Err(message) = &card.row.rendered {
        diagnostics.push(Diagnostic::error("show::render", message.clone()));
        return None;
    }
    let rendered = card.row.rendered.as_ref().expect("checked above");
    let (side, shown, sounds) = match request.side {
        Some(Side::Back) => ("back", &card.back, &rendered.answer.sounds),
        _ => ("front", &card.front, &rendered.question.sounds),
    };
    let html = &shown.as_ref().expect("the side of the request").html;
    if html.contains("\\(") || html.contains("\\[") {
        diagnostics.push(
            Diagnostic::warning("show::mathjax", "MathJax does not run in the HTML output, so the maths shows as TeX")
                .with_help("use `janky preview` to see the maths"),
        );
    }
    let urls: BTreeMap<&str, String> = media
        .iter()
        .filter_map(|(name, path)| {
            let path = std::path::absolute(path).ok()?;
            Some((name.as_str(), url::Url::from_file_path(path).ok()?.to_string()))
        })
        .collect();
    let body_class = preview::body_class(card.row.ord, request.night, request.platform);
    let mut data = preview::card_json(&card.row.css, html, sounds, &body_class, side);
    data["media"] = json!(urls);
    data["mathjax"] = json!(false);
    preview::file_document(&data, &body_class)
}

/// The identifiers of all cards, in the order of the preview table, or of all sample cards.
fn identifiers(loaded: &preview::Loaded, sample: bool) -> Vec<String> {
    let project = &loaded.checked.project;
    if sample {
        return project
            .notetypes
            .iter()
            .flat_map(|(key, notetype)| notetype.file.cards.keys().map(move |card| format!("{key}/{card}")))
            .collect();
    }
    let notes: HashMap<String, _> = loaded
        .checked
        .rendered
        .notes
        .iter()
        .map(|note| (lock::guid(&loaded.lock.namespace, &note.notetype, &note.id), note))
        .collect();
    loaded
        .package
        .notes
        .iter()
        .map(|note| notes[&note.guid])
        .flat_map(|note| {
            let cards = project.notetypes[&note.notetype].file.cards.keys();
            cards.map(move |card| format!("{}/{card}", note.id))
        })
        .collect()
}

/// Finds the note or the note type of `identifier`, and the card key if it has one.
fn resolve<'a>(
    loaded: &preview::Loaded,
    identifier: &'a str,
    sample: bool,
    diagnostics: &mut Diagnostics,
) -> Option<(Selection<'a>, Option<&'a str>)> {
    let project = &loaded.checked.project;
    let notetype_of =
        |id: &str| loaded.checked.rendered.notes.iter().find(|note| note.id == id).map(|note| note.notetype.as_str());
    let known = |name: &str| {
        if sample { project.notetypes.get_key_value(name).map(|(key, _)| key.as_str()) } else { notetype_of(name) }
    };
    let (name, card) = match known(identifier) {
        Some(_) => (identifier, None),
        None => match identifier.rsplit_once('/') {
            Some((name, card)) if known(name).is_some() => (name, Some(card)),
            _ => {
                diagnostics.push(unknown(identifier, sample, project));
                return None;
            },
        },
    };
    let notetype = known(name).expect("a known name");
    let cards = &project.notetypes[notetype].file.cards;
    if let Some(card) = card
        && !cards.contains_key(card)
    {
        let keys: Vec<String> = cards.keys().map(|key| format!("`{key}`")).collect();
        let owner = if sample { format!("note type `{name}`") } else { format!("entry `{name}`") };
        diagnostics.push(
            Diagnostic::error("show::unknown-card", format!("the note type of {owner} has no card type `{card}`"))
                .with_help(format!("the card types of `{notetype}` are {}", list(&keys))),
        );
        return None;
    }
    let selection = if sample { Selection::Sample(name) } else { Selection::Entry(name) };
    Some((selection, card))
}

fn unknown(identifier: &str, sample: bool, project: &project::Project) -> Diagnostic {
    if sample {
        let keys: Vec<String> = project.notetypes.keys().map(|key| format!("`{key}`")).collect();
        Diagnostic::error("show::unknown-notetype", format!("`deck.toml` has no note type `{identifier}`"))
            .with_help(format!("the note types are {}", list(&keys)))
    } else {
        Diagnostic::error("show::unknown-entry", format!("no entry has the id `{identifier}`")).with_help(
            "run `janky show` to list the identifiers. An entry with errors is not in the list, so look at the errors above",
        )
    }
}

/// `a`, `a and b`, or `a, b and c`.
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// One card as `janky show` prints it.
struct Card<'a> {
    row: &'a Row,
    front: Option<ShownSide>,
    back: Option<ShownSide>,
}

/// The HTML and the text of one side.
struct ShownSide {
    html: String,
    text: String,
    sounds: Vec<Value>,
}

impl<'a> Card<'a> {
    fn new(row: &'a Row, request: &Request) -> Self {
        let Ok(rendered) = &row.rendered else {
            return Card { row, front: None, back: None };
        };
        let front = (request.side != Some(Side::Back)).then(|| {
            let mut text = text(&rendered.question.html, &rendered.question.sounds);
            if rendered.type_answer.is_some() {
                text.push_str(" [type answer]");
            }
            ShownSide {
                html: rendered.question.html.clone(),
                text,
                sounds: preview::sounds_json(&rendered.question.sounds),
            }
        });
        let back = (request.side != Some(Side::Front)).then(|| {
            let html = janky_anki::compare_typed(&rendered.answer.html, rendered.type_answer.as_ref(), request.typed)
                .unwrap_or_else(|error| error.0);
            ShownSide {
                text: text(&html, &rendered.answer.sounds),
                html,
                sounds: preview::sounds_json(&rendered.answer.sounds),
            }
        });
        Card { row, front, back }
    }

    fn identifier(&self) -> String {
        let name = self.row.entry.as_ref().map_or(self.row.notetype.as_str(), |entry| entry.id.as_str());
        format!("{name}/{}", self.row.card_key)
    }

    fn to_text(&self) -> String {
        let row = self.row;
        let source = match &row.entry {
            Some(entry) => match entry.index {
                Some(index) => format!("{} #{index}", entry.file),
                None => entry.file.clone(),
            },
            None => "sample".to_string(),
        };
        let mut out = format!("{} · {} · {} · {source}\n", self.identifier(), row.notetype_name, row.card_name);
        match &row.rendered {
            Err(message) => out.push_str(&format!("error: {}\n", one_line(message))),
            Ok(rendered) => {
                if rendered.is_empty {
                    out.push_str("empty: Anki does not make this card\n");
                }
                for (label, side) in [("front", &self.front), ("back", &self.back)] {
                    if let Some(side) = side {
                        out.push_str(&format!("{label}: {}\n", side.text));
                    }
                }
            },
        }
        out
    }

    fn to_json(&self) -> Value {
        let row = self.row;
        let side = |side: &Option<ShownSide>| {
            side.as_ref().map(|side| json!({ "html": side.html, "text": side.text, "sounds": side.sounds }))
        };
        let rendered = row.rendered.as_ref().ok();
        json!({
            "id": self.identifier(),
            "notetype": { "key": row.notetype, "name": row.notetype_name },
            "entry": row.entry.as_ref().map(|entry| json!({ "id": entry.id, "file": entry.file, "index": entry.index })),
            "deck": row.deck.as_ref().map(|(key, full_name)| json!({ "key": key, "full_name": full_name })),
            "card": { "key": row.card_key, "name": row.card_name, "ord": row.ord },
            "css": row.css,
            "empty": rendered.is_some_and(|card| card.is_empty),
            "error": row.rendered.as_ref().err(),
            "front": side(&self.front),
            "back": side(&self.back),
            "type_answer": rendered
                .and_then(|card| card.type_answer.as_ref())
                .map(|answer| json!({ "expected": answer.expected, "combining": answer.combining })),
        })
    }
}

/// The text of one side, with each play tag replaced by its sound.
fn text(html: &str, sounds: &[janky_anki::Sound]) -> String {
    let text = one_line(&janky_anki::html_to_text(html));
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find("[anki:play:") {
        out.push_str(&rest[..start]);
        let tag = &rest[start..];
        let sound = tag.find(']').and_then(|end| {
            let index: usize = tag[..end].rsplit(':').next()?.parse().ok()?;
            Some((end, sounds.get(index)?))
        });
        match sound {
            Some((end, janky_anki::Sound::File(name))) => {
                out.push_str(&format!("[sound:{name}]"));
                rest = &tag[end + 1..];
            },
            Some((end, janky_anki::Sound::Speech { text, language })) => {
                out.push_str(&format!("[tts {language}: {text}]"));
                rest = &tag[end + 1..];
            },
            None => {
                out.push_str("[anki:play:");
                rest = &tag["[anki:play:".len()..];
            },
        }
    }
    out.push_str(rest);
    out
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
