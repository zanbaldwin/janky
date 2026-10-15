//! The `[[type:…]]` tags of Anki's reviewer, for `janky preview`.
//!
//! rslib renders `{{type:Field}}` as `[[type:Field]]`, and Anki's reviewer replaces that tag in Python
//! (`typeAnsQuestionFilter` and `typeAnsAnswerFilter` in `qt/aqt/reviewer.py` of Anki 26.08.1). On the front, a text
//! input takes its place. On the back, the comparison of the typed answer with the field takes its place. This
//! module does the same, with the same HTML. The `cloze:` form is not handled, because janky note types are never
//! cloze note types.

use std::sync::LazyLock;

use anki::collection::{Collection, CollectionBuilder};
use anki::notes::Note;
use anki::notetype::Notetype;
use anki::services::CardRenderingService;
use anki_proto::card_rendering::CompareAnswerRequest;
use regex::{NoExpand, Regex};

use crate::{Result, fail};

/// The pattern of Anki's reviewer.
static PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[type:(.+?)\]\]").expect("a valid pattern"));

/// What the typed answer of a card is compared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAnswer {
    /// The value of the field, as HTML. Anki removes the HTML before it compares.
    pub expected: String,
    /// `false` for `{{type:nc:Field}}`, which compares without combining characters such as accents.
    pub combining: bool,
    /// The editor font of the field, which Anki also uses for the input and the comparison.
    pub font: String,
    /// The editor font size of the field, in pixels.
    pub size: u32,
}

/// Replaces the `[[type:…]]` tags of the question `html` as Anki does, and returns what the typed answer is
/// compared with.
///
/// The first tag names the field. An unknown field gives Anki's message in place of the tags. An empty field
/// removes the tags, and the result is `None`.
pub(crate) fn question(col: &Collection, html: &mut String, note: &Note, notetype: &Notetype) -> Option<TypeAnswer> {
    let tag = PATTERN.captures(html)?.get(1).expect("the pattern has one group").as_str();
    let (name, combining) = match tag.strip_prefix("nc:") {
        Some(name) => (name.to_string(), false),
        None => (tag.to_string(), true),
    };
    let field = notetype.fields.iter().zip(note.fields()).find(|(field, _)| field.name == name);
    let (replacement, answer) = match field {
        None => (col.tr().studying_type_answer_unknown_field(name.as_str()).into_owned(), None),
        Some((_, value)) if value.is_empty() => (String::new(), None),
        Some((field, value)) => {
            let font = field.config.font_name.clone();
            let size = field.config.font_size;
            let input = format!(
                "\n<center>\n<input type=text id=typeans onkeypress=\"_typeAnsPress();\"\n   \
                 style=\"font-family: '{font}'; font-size: {size}px;\">\n</center>\n"
            );
            (
                input,
                Some(TypeAnswer {
                    expected: value.clone(),
                    combining,
                    font,
                    size,
                }),
            )
        },
    };
    *html = PATTERN.replace_all(html, NoExpand(&replacement)).into_owned();
    answer
}

/// Replaces the `[[type:…]]` tags of the answer `html` as Anki does, with the comparison of `typed` with
/// `type_answer`. Without a `type_answer`, it removes the tags.
///
/// Like Anki, it moves `<hr id=answer>` in front of the comparison, so that the comparison is on the answer side of
/// the line when the back starts with `{{FrontSide}}`. It returns an [`Error`](crate::Error) only if rslib fails.
pub fn compare_typed(html: &str, type_answer: Option<&TypeAnswer>, typed: &str) -> Result<String> {
    let Some(type_answer) = type_answer else {
        return Ok(PATTERN.replace_all(html, "").into_owned());
    };
    let without = html.replace("<hr id=answer>", "");
    let had_line = without.len() != html.len();
    if had_line && !PATTERN.is_match(&without) {
        return Ok(html.to_string());
    }
    let mut col = CollectionBuilder::default().build().map_err(fail)?;
    let request = CompareAnswerRequest {
        expected: type_answer.expected.clone(),
        provided: typed.to_string(),
        combining: type_answer.combining,
    };
    let output = col.compare_answer(request).map_err(fail)?.val;
    let mut replacement = format!(
        "\n<div style=\"font-family: '{}'; font-size: {}px\">{output}</div>",
        type_answer.font, type_answer.size
    );
    if had_line {
        replacement.insert_str(0, "<hr id=answer>");
    }
    Ok(PATTERN.replace_all(&without, NoExpand(&replacement)).into_owned())
}
