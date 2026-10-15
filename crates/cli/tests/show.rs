mod common;

use std::path::Path;
use std::process::Command;

use janky_cli::diagnostic::Diagnostics;
use janky_cli::preview::Platform;
use janky_cli::show::{Format, Request, Side, show};
use serde_json::Value;

fn request(identifier: Option<&str>) -> Request<'_> {
    Request {
        identifier,
        side: None,
        typed: "",
        sample: false,
        json: false,
        format: Format::Text,
        night: false,
        platform: Platform::Desktop,
    }
}

fn run(dir: &Path, request: &Request) -> (Option<String>, Diagnostics) {
    let mut diagnostics = Diagnostics::default();
    let output = show(&dir.join("deck.toml"), Default::default(), request, &mut diagnostics);
    (output, diagnostics)
}

fn text(dir: &Path, request: &Request) -> String {
    let (output, diagnostics) = run(dir, request);
    assert!(!diagnostics.has_errors(), "{}", diagnostics.to_json_lines());
    output.unwrap()
}

fn error(dir: &Path, request: &Request) -> (String, String, Option<String>) {
    let (output, diagnostics) = run(dir, request);
    assert_eq!(output, None);
    let json: Vec<Value> =
        diagnostics.to_json_lines().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let error = json.iter().find(|diagnostic| diagnostic["severity"] == "error").unwrap();
    (
        error["code"].as_str().unwrap().to_string(),
        error["message"].as_str().unwrap().to_string(),
        error["help"].as_str().map(str::to_string),
    )
}

fn type_answer(dir: &Path) {
    let cards = dir.join("notetypes/group/cards");
    std::fs::write(cards.join("group.front.html"), "{{Group}} {{tts en_GB:Group}} {{type:Property}}").unwrap();
    std::fs::write(cards.join("group.back.html"), "{{Group}}<hr id=answer>{{type:Property}}").unwrap();
}

#[test]
fn show_lists_the_identifiers_in_the_order_of_the_preview() {
    let (_work, dir) = common::example();
    let list = text(&dir, &request(None));
    let lines: Vec<&str> = list.lines().collect();
    assert_eq!(&lines[..2], ["li/recognition", "li/production"]);
    assert_eq!(&lines[lines.len() - 2..], ["alkali-metals/group", "halogens/group"]);
    assert!(lines.iter().all(|line| line.contains('/')), "{list}");
}

#[test]
fn show_lists_the_sample_identifiers() {
    let (_work, dir) = common::example();
    let list = text(&dir, &Request { sample: true, ..request(None) });
    assert_eq!(list, "element/recognition\nelement/production\ngroup/group\n");
}

#[test]
fn show_prints_the_text_of_both_sides_of_each_card() {
    let (_work, dir) = common::example();
    let output = text(&dir, &request(Some("li")));
    let blocks: Vec<&str> = output.split("\n\n").collect();
    assert_eq!(blocks.len(), 2, "{output}");
    assert!(
        blocks[0].starts_with(
            "li/recognition · Element · Recognition · content/periods/early.yaml #0\nfront: Li\nback: Li "
        ),
        "{output}"
    );
    assert!(
        blocks[1].starts_with("li/production · Element · Production · content/periods/early.yaml #0\n"),
        "{output}"
    );
    assert!(blocks[0].contains("[sound:") && blocks[0].contains(".ogg]"), "{output}");
}

#[test]
fn show_prints_one_side_of_one_card() {
    let (_work, dir) = common::example();
    let output = text(
        &dir,
        &Request {
            side: Some(Side::Front),
            ..request(Some("alkali-metals/group"))
        },
    );
    assert_eq!(
        output,
        "alkali-metals/group · Element Group · Group · content/groups/basics.toml #0\nfront: Group 1\n"
    );
}

#[test]
fn show_writes_sound_tags_and_the_type_answer_as_text() {
    let (_work, dir) = common::example();
    type_answer(&dir);
    let output = text(
        &dir,
        &Request {
            side: Some(Side::Front),
            ..request(Some("halogens/group"))
        },
    );
    assert!(output.ends_with("front: Group 17 [tts en_GB: Group 17] [type answer]\n"), "{output}");
}

#[test]
fn show_compares_the_typed_answer() {
    let (_work, dir) = common::example();
    type_answer(&dir);
    let back = |typed| {
        text(
            &dir,
            &Request {
                side: Some(Side::Back),
                typed,
                ..request(Some("halogens/group"))
            },
        )
    };
    assert!(back("").ends_with("back: Group 17 forms salts\n"), "{}", back(""));
    let typed = back("forms sa");
    assert!(typed.contains("forms sa") && typed.contains("↓") && typed.contains("forms salts"), "{typed}");
}

#[test]
fn show_rejects_an_unknown_entry() {
    let (_work, dir) = common::example();
    let (code, message, help) = error(&dir, &request(Some("unobtainium")));
    assert_eq!(code, "show::unknown-entry");
    assert_eq!(message, "no entry has the id `unobtainium`");
    assert!(help.unwrap().starts_with("run `janky show` to list the identifiers"));
}

#[test]
fn show_rejects_an_unknown_card_and_lists_the_card_types() {
    let (_work, dir) = common::example();
    let (code, message, help) = error(&dir, &request(Some("li/spelling")));
    assert_eq!(code, "show::unknown-card");
    assert_eq!(message, "the note type of entry `li` has no card type `spelling`");
    assert_eq!(help.unwrap(), "the card types of `element` are `recognition` and `production`");
}

#[test]
fn show_rejects_an_unknown_notetype_for_a_sample() {
    let (_work, dir) = common::example();
    let (code, _, help) = error(&dir, &Request { sample: true, ..request(Some("isotope")) });
    assert_eq!(code, "show::unknown-notetype");
    assert_eq!(help.unwrap(), "the note types are `element` and `group`");
}

#[test]
fn show_finds_an_entry_id_that_contains_a_slash() {
    let (_work, dir) = common::example();
    let notetype = dir.join("notetypes/group.toml");
    let text_of = std::fs::read_to_string(&notetype).unwrap();
    std::fs::write(&notetype, text_of.replace("[schema]\n", "[schema]\nid = { type = \"string\", required = true }\n"))
        .unwrap();
    let basics = dir.join("content/groups/basics.toml");
    let content = std::fs::read_to_string(&basics).unwrap();
    std::fs::write(&basics, content.replace("id = 'halogens'", "id = 'halogens/17'")).unwrap();
    let whole = text(&dir, &request(Some("halogens/17")));
    assert!(whole.starts_with("halogens/17/group · Element Group · Group"), "{whole}");
    let card = text(&dir, &request(Some("halogens/17/group")));
    assert_eq!(card, whole);
}

#[test]
fn show_renders_the_sample_of_a_notetype() {
    let (_work, dir) = common::example();
    let output = text(
        &dir,
        &Request {
            sample: true,
            ..request(Some("group/group"))
        },
    );
    assert_eq!(output, "group/group · Element Group · Group · sample\nfront: (Group)\nback: (Group) (Property)\n");
}

#[test]
fn show_writes_no_files() {
    let (_work, dir) = common::example();
    let before = common::tree(&dir);
    text(&dir, &request(Some("li")));
    text(&dir, &request(None));
    assert_eq!(common::tree(&dir), before);
}

#[test]
fn show_puts_the_cards_after_the_diagnostics_in_json() {
    let (_work, dir) = common::example();
    std::fs::remove_dir_all(dir.join(".janky/cache")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_janky"))
        .args([
            "--deck",
            dir.join("deck.toml").to_str().unwrap(),
            "--json",
            "show",
            "li/recognition",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let lines: Vec<Value> =
        String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let (last, diagnostics) = lines.split_last().unwrap();
    assert!(!diagnostics.is_empty() && diagnostics.iter().all(|line| line["code"].is_string()));
    let cards = last["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    let card = &cards[0];
    assert_eq!(card["id"], "li/recognition");
    assert_eq!(card["entry"], serde_json::json!({ "id": "li", "file": "content/periods/early.yaml", "index": 0 }));
    assert_eq!(card["front"]["text"], "Li");
    assert_eq!(card["front"]["html"], "Li\n");
    assert_eq!(card["type_answer"], Value::Null);
    assert!(card["css"].as_str().unwrap().contains(".card"));
}

fn html(identifier: &str, side: Side) -> Request<'_> {
    Request {
        side: Some(side),
        format: Format::Html,
        ..request(Some(identifier))
    }
}

fn card_data(document: &str) -> Value {
    let start = document.find("id=\"janky-card\">").unwrap() + "id=\"janky-card\">".len();
    let end = start + document[start..].find("</script>").unwrap();
    assert!(!document[start..end].contains('<'));
    serde_json::from_str(&document[start..end]).unwrap()
}

#[test]
fn show_html_needs_one_side_of_one_card() {
    let (_work, dir) = common::example();
    let (code, message, help) = error(
        &dir,
        &Request {
            format: Format::Html,
            ..request(Some("li/recognition"))
        },
    );
    assert_eq!(code, if janky_cli::preview::ui_built() { "show::one-side" } else { "show::no-ui" });
    if !janky_cli::preview::ui_built() {
        return;
    }
    assert_eq!(message, "`--format html` shows one side of one card");
    assert_eq!(help.unwrap(), "add `--side front` or `--side back`");
    let (code, message, help) = error(&dir, &html("li", Side::Front));
    assert_eq!(code, "show::one-side");
    assert_eq!(message, "`li` names 2 cards, and `--format html` shows one");
    assert_eq!(help.unwrap(), "add the card key, for example `li/recognition`");
}

#[test]
fn show_html_has_file_urls_and_loads_nothing_from_janky() {
    let (_work, dir) = common::example();
    if !janky_cli::preview::ui_built() {
        assert_eq!(error(&dir, &html("c/recognition", Side::Back)).0, "show::no-ui");
        return;
    }
    let document = text(&dir, &html("c/recognition", Side::Back));
    assert!(document.starts_with("<!doctype html>"), "{document}");
    assert!(document.contains("<body class=\"card card1 isLin\">"), "{document}");
    assert!(document.contains("<script type=\"module\">"), "{document}");
    for reference in ["src=\"/", "href=\"/", "<base", "/assets/card."] {
        assert!(!document.contains(reference), "{reference} in {document}");
    }
    let data = card_data(&document);
    assert_eq!(data["side"], "back");
    assert_eq!(data["mathjax"], false);
    let image = data["media"]["diamond.png"].as_str().unwrap();
    let path = std::path::absolute(dir.join("media/images/diamond.png")).unwrap();
    assert_eq!(image, url::Url::from_file_path(path).unwrap().as_str());
    assert!(data["html"].as_str().unwrap().contains("diamond.png"));
}

#[test]
fn show_html_sets_the_night_and_platform_classes() {
    let (_work, dir) = common::example();
    if !janky_cli::preview::ui_built() {
        return;
    }
    let request = Request {
        night: true,
        platform: Platform::Iphone,
        ..html("li/production", Side::Front)
    };
    let document = text(&dir, &request);
    assert!(document.contains("<body class=\"card card2 mobile iphone nightMode night_mode\">"), "{document}");
    assert_eq!(card_data(&document)["bodyclass"], "card card2 mobile iphone nightMode night_mode");
}

#[test]
fn show_html_warns_about_mathjax() {
    let (_work, dir) = common::example();
    if !janky_cli::preview::ui_built() {
        return;
    }
    let basics = dir.join("content/groups/basics.toml");
    let content = std::fs::read_to_string(&basics).unwrap();
    std::fs::write(&basics, content.replace("'forms salts'", "'forms salts: \\(NaCl\\)'")).unwrap();
    let (output, diagnostics) = run(&dir, &html("halogens/group", Side::Back));
    assert!(output.unwrap().contains("NaCl"));
    let codes: Vec<String> = diagnostics.iter().map(|diagnostic| diagnostic.to_json()["code"].to_string()).collect();
    assert!(codes.contains(&"\"show::mathjax\"".to_string()), "{codes:?}");
}
