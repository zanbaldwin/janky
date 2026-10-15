use janky_anki::{
    Card, Deck, Field, Note, Notetype, Package, RenderedCard, Sound, TypeAnswer, compare_typed, html_to_text, render,
};

fn field(id: i64, name: &str) -> Field {
    Field {
        id,
        name: name.into(),
        description: String::new(),
        collapsed: false,
    }
}

fn package(cards: Vec<Card>, fields: [&str; 2]) -> Package {
    Package {
        notetypes: vec![Notetype {
            id: 1001,
            name: "Vocabulary".into(),
            css: ".card { color: red }".into(),
            sort_field: 0,
            modified: 1000,
            fields: vec![field(11, "Front"), field(12, "Back")],
            cards,
        }],
        presets: Vec::new(),
        decks: vec![
            Deck {
                id: 100,
                name: "Japanese".into(),
                preset: None,
            },
            Deck {
                id: 101,
                name: "Japanese::Words 🗾".into(),
                preset: None,
            },
        ],
        notes: vec![Note {
            guid: "guid-1".into(),
            notetype: 1001,
            deck: 101,
            fields: fields.map(String::from).to_vec(),
            tags: vec!["tag".into()],
            modified: 1000,
        }],
        media: Vec::new(),
    }
}

fn card(id: i64, front: &str, back: &str) -> Card {
    Card {
        id,
        name: format!("Card {id}"),
        front: front.into(),
        back: back.into(),
    }
}

fn only(cards: &[Result<RenderedCard, String>]) -> &RenderedCard {
    let [card] = cards else { panic!("expected one card, got {}", cards.len()) };
    card.as_ref().unwrap()
}

#[test]
fn render_shows_the_front_side_on_the_back() {
    let package =
        package(vec![card(21, "<b>{{Front}}</b>", "{{FrontSide}}<hr id=answer>{{Back}}")], ["こんにちは", "hello"]);
    let rendered = render(&package, &[]).unwrap();
    let card = only(&rendered.notes[0]);
    assert_eq!(card.question.html, "<b>こんにちは</b>");
    assert_eq!(card.answer.html, "<b>こんにちは</b><hr id=answer>hello");
    assert!(!card.is_empty);
}

#[test]
fn render_makes_one_card_for_each_card_template() {
    let package =
        package(vec![card(21, "{{Front}}", "{{Back}}"), card(22, "{{Back}}", "{{Front}}")], ["front", "back"]);
    let rendered = render(&package, &[]).unwrap();
    let questions: Vec<_> =
        rendered.notes[0].iter().map(|card| card.as_ref().unwrap().question.html.as_str()).collect();
    assert_eq!(questions, ["front", "back"]);
}

#[test]
fn render_shows_the_deck_of_the_note_in_special_fields() {
    let package = package(vec![card(21, "{{Front}} {{Deck}} / {{Subdeck}} / {{Tags}}", "{{Back}}")], ["front", "back"]);
    let rendered = render(&package, &[]).unwrap();
    assert_eq!(only(&rendered.notes[0]).question.html, "front Japanese::Words 🗾 / Words 🗾 / tag");
}

#[test]
fn render_marks_a_card_with_an_empty_front() {
    let package = package(vec![card(21, "{{Front}}", "{{Back}}"), card(22, "{{Back}}", "{{Front}}")], ["front", ""]);
    let rendered = render(&package, &[]).unwrap();
    let cards: Vec<_> = rendered.notes[0].iter().map(|card| card.as_ref().unwrap()).collect();
    assert!(!cards[0].is_empty);
    assert!(cards[1].is_empty);
    assert!(cards[1].question.html.contains("The front of this card is blank."), "{}", cards[1].question.html);
}

#[test]
fn render_fills_empty_fields_of_a_sample_note_with_the_field_names() {
    let mut package = package(vec![card(21, "{{Front}} {{Deck}}", "{{FrontSide}} {{Back}}")], ["front", "back"]);
    package.notes.clear();
    let rendered = render(&package, &[1001]).unwrap();
    assert!(rendered.notes.is_empty());
    let card = only(&rendered.samples[0]);
    assert_eq!(card.question.html, "(Front) (Deck)");
    assert_eq!(card.answer.html, "(Front) (Deck) (Back)");
    assert!(!card.is_empty);
}

#[test]
fn render_fails_for_a_sample_of_an_unknown_notetype() {
    let package = package(vec![card(21, "{{Front}}", "{{Back}}")], ["front", "back"]);
    let error = render(&package, &[9999]).unwrap_err();
    assert_eq!(error.0, "note type 9999 is missing");
}

#[test]
fn render_fails_on_a_card_template_that_anki_rejects() {
    let package = package(vec![card(21, "{{#Front}}{{Front}}", "{{Back}}")], ["front", "back"]);
    let error = render(&package, &[]).unwrap_err();
    assert_eq!(
        error.0,
        "Card template 1 in note type 'Vocabulary' has a problem. See the preview for more information."
    );
}

#[test]
fn render_replaces_sound_tags_with_play_tags() {
    let package =
        package(vec![card(21, "{{Front}}{{tts ja_JP:Back}}", "{{Back}}")], ["hello [sound:hello.ogg]", "こんにちは"]);
    let rendered = render(&package, &[]).unwrap();
    let card = only(&rendered.notes[0]);
    assert_eq!(card.question.html, "hello [anki:play:q:0][anki:play:q:1]");
    assert_eq!(
        card.question.sounds,
        [
            Sound::File("hello.ogg".into()),
            Sound::Speech {
                text: "こんにちは".into(),
                language: "ja_JP".into(),
            },
        ]
    );
    assert_eq!(card.answer.html, "こんにちは");
    assert!(card.answer.sounds.is_empty());
}

fn typed_card(front: &str, back: &str, fields: [&str; 2]) -> RenderedCard {
    let package = package(vec![card(21, front, back)], fields);
    let mut rendered = render(&package, &[]).unwrap();
    rendered.notes.remove(0).remove(0).unwrap()
}

#[test]
fn render_puts_a_text_input_in_place_of_a_type_tag() {
    let card = typed_card("{{Front}}{{type:Back}}", "{{Front}}<hr id=answer>{{type:Back}}", ["猫", "neko"]);
    assert_eq!(
        card.question.html,
        "猫\n<center>\n<input type=text id=typeans onkeypress=\"_typeAnsPress();\"\n   \
         style=\"font-family: 'Arial'; font-size: 20px;\">\n</center>\n"
    );
    assert_eq!(
        card.type_answer,
        Some(TypeAnswer {
            expected: "neko".into(),
            combining: true,
            font: "Arial".into(),
            size: 20,
        })
    );
    assert!(card.answer.html.contains("[[type:Back]]"), "{}", card.answer.html);
}

#[test]
fn render_compares_without_combining_characters_for_type_nc() {
    let card = typed_card("{{Front}}{{type:nc:Back}}", "{{type:nc:Back}}", ["café", "café"]);
    assert!(!card.type_answer.unwrap().combining);
}

#[test]
fn render_removes_the_type_tag_of_an_empty_field() {
    let card = typed_card("{{Front}}{{type:Back}}", "{{FrontSide}}<hr id=answer>{{type:Back}}", ["猫", ""]);
    assert_eq!(card.question.html, "猫");
    assert_eq!(card.type_answer, None);
    assert_eq!(compare_typed(&card.answer.html, None, "anything").unwrap(), "猫<hr id=answer>");
}

#[test]
fn compare_typed_marks_the_wrong_characters() {
    let card = typed_card("{{Front}}{{type:Back}}", "{{Front}}<hr id=answer>{{type:Back}}", ["猫", "neko"]);
    let html = compare_typed(&card.answer.html, card.type_answer.as_ref(), "nako").unwrap();
    assert!(
        html.starts_with("猫<hr id=answer>\n<div style=\"font-family: 'Arial'; font-size: 20px\"><code id=typeans>"),
        "{html}"
    );
    assert!(html.contains("typeGood") && html.contains("typeBad"), "{html}");
    assert!(!html.contains("[[type:"), "{html}");
}

#[test]
fn compare_typed_shows_the_answer_when_nothing_is_typed() {
    let card = typed_card("{{Front}}{{type:Back}}", "{{type:Back}}", ["猫", "<b>neko</b>"]);
    let html = compare_typed(&card.answer.html, card.type_answer.as_ref(), "").unwrap();
    assert_eq!(html, "\n<div style=\"font-family: 'Arial'; font-size: 20px\"><code id=typeans>neko</code></div>");
}

#[test]
fn compare_typed_keeps_a_back_without_type_tags_after_the_line() {
    let card = typed_card("{{Front}}{{type:Back}}", "{{Front}}<hr id=answer>{{Back}}", ["猫", "neko"]);
    let html = compare_typed(&card.answer.html, card.type_answer.as_ref(), "neko").unwrap();
    assert_eq!(html, "猫<hr id=answer>neko");
}

#[test]
fn html_to_text_gives_one_line_with_image_names_and_play_tags() {
    assert_eq!(
        html_to_text("<b>猫</b><br>neko &amp; <img src=\"cat.png\"> [anki:play:a:0]"),
        "猫 neko &  cat.png  [anki:play:a:0]"
    );
}
