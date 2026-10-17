mod common;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;

use janky_cli::preview::Server;
use serde_json::Value;

fn start(dir: &Path) -> SocketAddr {
    let server = Server::bind(&dir.join("deck.toml"), Default::default(), 0).unwrap();
    let address = server.address();
    std::thread::spawn(move || server.run().unwrap());
    address
}

fn request_bytes(address: SocketAddr, method: &str, path: &str, host: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(address).unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response.windows(4).position(|window| window == b"\r\n\r\n").unwrap();
    let head = String::from_utf8(response[..split].to_vec()).unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, head.to_lowercase(), response[split + 4..].to_vec())
}

fn request(address: SocketAddr, method: &str, path: &str, host: &str) -> (u16, String, String) {
    let (status, head, body) = request_bytes(address, method, path, host);
    (status, head, String::from_utf8(body).unwrap())
}

fn preview(address: SocketAddr) -> Value {
    let (status, head, body) = request(address, "GET", "/preview.json", &address.to_string());
    assert_eq!(status, 200, "{body}");
    assert!(head.contains("content-type: application/json"), "{head}");
    assert!(head.contains("cache-control: no-store"), "{head}");
    serde_json::from_str(&body).unwrap()
}

fn rows(json: &Value) -> &Vec<Value> {
    json["rows"].as_array().unwrap()
}

fn errors(json: &Value) -> Vec<&str> {
    json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["severity"] == "error")
        .map(|diagnostic| diagnostic["code"].as_str().unwrap())
        .collect()
}

fn edit(path: &Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{from:?} not in {}", path.display());
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

fn card_count(dir: &Path) -> usize {
    let mut diagnostics = janky_cli::diagnostic::Diagnostics::default();
    let checked =
        janky_cli::cli::check(&dir.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new()).unwrap();
    checked.rendered.notes.iter().map(|note| checked.project.notetypes[&note.notetype].file.cards.len()).sum()
}

#[test]
fn preview_returns_one_row_for_each_card_of_the_example_project() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let json = preview(address);
    assert_eq!(json["stale"], false);
    assert_eq!(json["exported"], false);
    let generation = json["generation"].as_str().unwrap();
    assert!(generation.len() == 32 && generation.bytes().all(|byte| byte.is_ascii_hexdigit()), "{generation}");
    assert!(errors(&json).is_empty(), "{json}");
    let rows = rows(&json);
    assert_eq!(rows.len(), card_count(&dir));
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row["row"], index);
        assert!(row["entry"]["id"].is_string(), "{row}");
        assert!(row["entry"]["file"].as_str().unwrap().starts_with("content/"), "{row}");
        assert!(row["deck"]["full_name"].as_str().unwrap().starts_with("Periodic Table::"), "{row}");
        assert!(row["empty"] == false || row["card"]["key"] == "production", "{row}");
        assert_eq!(row["error"], Value::Null);
        assert_eq!(row["type_answer"], false);
    }
    assert!(rows.iter().any(|row| row["empty"] == true), "a card with `when` gives an empty front");
    let notetypes: Vec<&str> = rows.iter().map(|row| row["notetype"]["key"].as_str().unwrap()).collect();
    let first_group = notetypes.iter().position(|key| *key == "group").unwrap();
    assert!(notetypes[..first_group].iter().all(|key| *key == "element"), "the elements deck comes first");
    assert!(notetypes[first_group..].iter().all(|key| *key == "group"));
    let group = &rows[first_group];
    assert_eq!(group["entry"]["id"], "alkali-metals");
    assert_eq!(group["entry"]["file"], "content/groups/basics.toml");
    assert_eq!(group["entry"]["index"], 0);
    assert_eq!(rows[first_group + 1]["entry"]["index"], 1);
    assert_eq!(group["deck"]["key"], "groups");
    assert_eq!(group["deck"]["full_name"], "Periodic Table::1 Groups 🧪");
    assert_eq!(group["notetype"]["name"], "Element Group");
    assert_eq!(group["card"], serde_json::json!({ "key": "group", "name": "Group", "ord": 0 }));
}

#[test]
fn preview_orders_the_files_of_a_deck_by_its_content_patterns() {
    let (_work, dir) = common::example();
    edit(
        &dir.join("deck.toml"),
        r#"content = ["content/periods/*.yaml", "content/periods/*.json"]"#,
        r#"content = ["content/periods/*.json", "content/periods/*.yaml"]"#,
    );
    let address = start(&dir);
    let json = preview(address);
    let files: Vec<&str> = rows(&json).iter().filter_map(|row| row["entry"]["file"].as_str()).collect();
    let first_yaml = files.iter().position(|file| file.ends_with(".yaml")).unwrap();
    assert!(files[..first_yaml].iter().all(|file| *file == "content/periods/late.json"), "{files:?}");
    assert!(files[first_yaml..].iter().all(|file| *file != "content/periods/late.json"), "{files:?}");
}

#[test]
fn preview_adds_a_sample_row_for_an_unused_notetype() {
    let (_work, dir) = common::example();
    let notetype = std::fs::read_to_string(dir.join("notetypes/group.toml")).unwrap();
    std::fs::write(dir.join("notetypes/unused.toml"), notetype.replace("Element Group", "Unused Group")).unwrap();
    let mut deck = std::fs::read_to_string(dir.join("deck.toml")).unwrap();
    deck.push_str("[notetypes.unused]\npath = \"notetypes/unused.toml\"\n");
    std::fs::write(dir.join("deck.toml"), deck).unwrap();

    let json = preview(start(&dir));
    assert!(errors(&json).is_empty(), "{json}");
    let rows = rows(&json);
    assert_eq!(rows.len(), card_count(&dir) + 1);
    let sample = rows.last().unwrap();
    assert_eq!(sample["notetype"], serde_json::json!({ "key": "unused", "name": "Unused Group" }));
    assert_eq!(sample["entry"], Value::Null);
    assert_eq!(sample["deck"], Value::Null);
    assert_eq!(sample["card"]["key"], "group");
    assert_eq!(sample["error"], Value::Null);
}

#[test]
fn preview_writes_no_files() {
    let (_work, dir) = common::example();
    let before = common::tree(&dir);
    let address = start(&dir);
    preview(address);
    preview(address);
    assert_eq!(common::tree(&dir), before);
}

#[test]
fn preview_refuses_a_foreign_host_header() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let port = address.port();
    for host in [
        "evil.example".to_string(),
        format!("evil.example:{port}"),
        format!("127.0.0.1:{}", port + 1),
    ] {
        let (status, _, _) = request(address, "GET", "/preview.json", &host);
        assert_eq!(status, 421, "{host}");
    }
    let (status, _, _) = request(address, "GET", "/preview.json", &format!("localhost:{port}"));
    assert_eq!(status, 200);
}

#[test]
fn preview_refuses_other_methods() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let (status, _, _) = request(address, "POST", "/preview.json", &address.to_string());
    assert_eq!(status, 405);
}

#[test]
fn preview_serves_the_page() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let (status, head, body) = request(address, "GET", "/", &address.to_string());
    assert_eq!(status, 200);
    assert!(head.contains("content-type: text/html; charset=utf-8"), "{head}");
    assert!(head.contains("content-security-policy: default-src 'self'"), "{head}");
    assert!(body.contains("<title>janky preview</title>"), "{body}");
}

#[test]
fn preview_reads_the_files_again_on_each_request() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let first = preview(address);
    let before = rows(&first).len();
    let basics = dir.join("content/groups/basics.toml");
    let mut text = std::fs::read_to_string(&basics).unwrap();
    text.push_str("\n[[entries]]\nid = 'noble-gases'\ngroup = 'Group 18'\nproperty = 'does not react'\n");
    std::fs::write(&basics, text).unwrap();
    let json = preview(address);
    assert_ne!(json["generation"], first["generation"]);
    let rows = rows(&json);
    assert_eq!(rows.len(), before + 1);
    assert!(rows.iter().any(|row| row["entry"]["id"] == "noble-gases"));
}

#[test]
fn preview_returns_rows_when_an_entry_has_an_error() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let before = rows(&preview(address)).len();
    edit(&dir.join("content/groups/basics.toml"), "property = 'forms salts'\n", "");
    let json = preview(address);
    assert!(!errors(&json).is_empty(), "{json}");
    assert_eq!(json["stale"], false);
    let rows = rows(&json);
    assert_eq!(rows.len(), before - 1);
    assert!(!rows.iter().any(|row| row["entry"]["id"] == "halogens"));
}

#[test]
fn preview_keeps_the_last_rows_when_the_project_breaks() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let good = preview(address);
    edit(&dir.join("deck.toml"), "[deck]", "[deck");
    let broken = preview(address);
    assert_eq!(broken["stale"], true);
    assert_ne!(broken["generation"], good["generation"]);
    assert!(!errors(&broken).is_empty(), "{broken}");
    assert_eq!(broken["rows"], good["rows"]);
}

#[test]
fn preview_does_not_read_the_media_files() {
    use std::os::unix::fs::PermissionsExt;
    let (_work, dir) = common::example();
    let image = dir.join("media/images/diamond.png");
    std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&image).is_ok() {
        return;
    }
    let json = preview(start(&dir));
    assert_eq!(json["stale"], false);
    assert!(errors(&json).is_empty(), "{json}");
}

#[test]
fn preview_reports_a_card_template_that_anki_rejects() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let good = preview(address);
    let front = dir.join("notetypes/group/cards/group.front.html");
    let text = std::fs::read_to_string(&front).unwrap();
    std::fs::write(&front, format!("{{{{#Group}}}}{text}")).unwrap();
    let broken = preview(address);
    assert_eq!(broken["stale"], true);
    assert_eq!(errors(&broken), ["preview::render"]);
    assert_eq!(broken["rows"], good["rows"]);
}

fn card_data(body: &str) -> Value {
    let start = body.find("id=\"janky-card\">").unwrap() + "id=\"janky-card\">".len();
    let end = start + body[start..].find("</script>").unwrap();
    let data = &body[start..end];
    assert!(!data.contains('<'), "{data}");
    serde_json::from_str(data).unwrap()
}

fn row_with_ord(json: &Value, ord: usize) -> usize {
    rows(json).iter().position(|row| row["card"]["ord"] == ord).unwrap()
}

fn first_row_of(json: &Value, notetype: &str) -> usize {
    rows(json).iter().position(|row| row["notetype"]["key"] == notetype).unwrap()
}

#[test]
fn preview_serves_a_card_document() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let json = preview(address);
    let generation = json["generation"].as_str().unwrap();
    let row = first_row_of(&json, "element");
    let (status, head, body) =
        request(address, "GET", &format!("/{generation}/cards/{row}-front.html"), &address.to_string());
    assert_eq!(status, 200, "{body}");
    assert!(head.contains("content-type: text/html; charset=utf-8"), "{head}");
    assert!(head.contains("content-security-policy: sandbox allow-scripts"), "{head}");
    assert!(head.contains("cache-control: no-store"), "{head}");
    assert!(body.contains("<base href=\"../media/\">"), "{body}");
    assert!(body.contains("<body class=\"card card1\">"), "{body}");
    assert!(!body.contains(generation), "the document has only relative URLs: {body}");
    assert!(!body.contains(" src="), "the shim is inline: {body}");
    let data = card_data(&body);
    assert_eq!(data["ord"], 0);
    assert_eq!(data["bodyclass"], Value::Null);
    assert_eq!(data["side"], "front");
    let html = data["html"].as_str().unwrap();
    assert!(html.starts_with("<style>.card { font-family: sans-serif; }"), "{html}");
}

#[test]
fn preview_leaves_the_night_and_platform_classes_to_the_shim() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let json = preview(address);
    let generation = json["generation"].as_str().unwrap();
    let row = row_with_ord(&json, 1);
    let host = address.to_string();
    let (_, _, plain) = request(address, "GET", &format!("/{generation}/cards/{row}-back.html"), &host);
    assert_eq!(card_data(&plain)["ord"], 1);
    assert_eq!(card_data(&plain)["side"], "back");
    for query in ["night=1&platform=android", "night=yes&platform=symbian"] {
        let path = format!("/{generation}/cards/{row}-back.html?{query}");
        let (status, _, body) = request(address, "GET", &path, &host);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body, plain, "{query}");
    }
}

#[test]
fn preview_refuses_an_unknown_side_or_row() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let json = preview(address);
    let generation = json["generation"].as_str().unwrap();
    let host = address.to_string();
    for path in [
        format!("/{generation}/cards/0-middle.html"),
        format!("/{generation}/cards/0-front"),
        format!("/{generation}/cards/9999-front.html"),
        format!("/{generation}/cards/first-front.html"),
        format!("/{generation}/cards/0/front.html"),
    ] {
        let (status, _, _) = request(address, "GET", &path, &host);
        assert_eq!(status, 404, "{path}");
    }
}

#[test]
fn preview_escapes_the_card_data() {
    let (_work, dir) = common::example();
    let css = dir.join("notetypes/element/style.css");
    let mut text = std::fs::read_to_string(&css).unwrap();
    text.push_str("/* </script><script>parent.x = 1</script> */\n");
    std::fs::write(&css, text).unwrap();
    let address = start(&dir);
    let json = preview(address);
    let generation = json["generation"].as_str().unwrap();
    let row = first_row_of(&json, "element");
    let (status, _, body) =
        request(address, "GET", &format!("/{generation}/cards/{row}-front.html"), &address.to_string());
    assert_eq!(status, 200);
    assert!(card_data(&body)["html"].as_str().unwrap().contains("</script><script>parent.x = 1</script>"));
}

#[test]
fn preview_serves_a_media_file_of_the_snapshot() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let json = preview(address);
    let generation = json["generation"].as_str().unwrap();
    let (status, head, body) =
        request_bytes(address, "GET", &format!("/{generation}/media/diamond.png"), &address.to_string());
    assert_eq!(status, 200);
    assert!(head.contains("content-type: image/png"), "{head}");
    assert!(head.contains("access-control-allow-origin: *"), "{head}");
    assert!(
        head.contains("content-security-policy: default-src 'none'; style-src 'unsafe-inline'; sandbox"),
        "{head}"
    );
    assert_eq!(body, std::fs::read(dir.join("media/images/diamond.png")).unwrap());
}

#[test]
fn preview_refuses_a_media_name_that_is_not_in_the_snapshot() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let json = preview(address);
    let generation = json["generation"].as_str().unwrap();
    for name in [
        "deck.toml",
        "..%2Fdeck.toml",
        "images%2Fdiamond.png",
        "%2E%2E",
        "missing.png",
    ] {
        let (status, _, _) = request(address, "GET", &format!("/{generation}/media/{name}"), &address.to_string());
        assert_eq!(status, 404, "{name}");
    }
}

#[test]
fn preview_refuses_a_media_path_with_a_slash() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let json = preview(address);
    let generation = json["generation"].as_str().unwrap();
    for path in ["images/diamond.png", "../deck.toml", "../../media/images/diamond.png"] {
        let (status, _, _) = request(address, "GET", &format!("/{generation}/media/{path}"), &address.to_string());
        assert_eq!(status, 404, "{path}");
    }
}

#[test]
fn preview_refuses_an_old_generation() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let old = preview(address);
    let old = old["generation"].as_str().unwrap();
    preview(address);
    let host = address.to_string();
    for path in [
        format!("/{old}/cards/0-front.html"),
        format!("/{old}/media/diamond.png"),
        "/0/cards/0-front.html".into(),
    ] {
        let (status, _, _) = request(address, "GET", &path, &host);
        assert_eq!(status, 410, "{path}");
    }
}

#[test]
fn preview_sends_the_sounds_of_the_side() {
    let (_work, dir) = common::example();
    let address = start(&dir);
    let json = preview(address);
    let generation = json["generation"].as_str().unwrap();
    let row = first_row_of(&json, "element");
    let host = address.to_string();
    let (_, _, back) = request(address, "GET", &format!("/{generation}/cards/{row}-back.html"), &host);
    let data = card_data(&back);
    assert!(data["html"].as_str().unwrap().contains("[anki:play:a:0]"), "{data}");
    let sounds = data["sounds"].as_array().unwrap();
    assert_eq!(sounds.len(), 1, "{data}");
    assert_eq!(sounds[0]["kind"], "file");
    assert!(sounds[0]["name"].as_str().unwrap().ends_with(".ogg"), "{data}");
    let (_, _, front) = request(address, "GET", &format!("/{generation}/cards/{row}-front.html"), &host);
    assert_eq!(card_data(&front)["sounds"], serde_json::json!([]));
}

#[test]
fn preview_compares_the_typed_answer_on_the_back() {
    let (_work, dir) = common::example();
    let cards = dir.join("notetypes/group/cards");
    let front = std::fs::read_to_string(cards.join("group.front.html")).unwrap();
    std::fs::write(cards.join("group.front.html"), format!("{front}{{{{type:Property}}}}")).unwrap();
    std::fs::write(cards.join("group.back.html"), "{{FrontSide}}<hr id=answer>{{type:Property}}").unwrap();
    let address = start(&dir);
    let json = preview(address);
    let generation = json["generation"].as_str().unwrap();
    let row = rows(&json).iter().position(|row| row["entry"]["id"] == "halogens").unwrap();
    assert_eq!(rows(&json)[row]["type_answer"], true);
    let host = address.to_string();

    let (_, _, body) = request(address, "GET", &format!("/{generation}/cards/{row}-front.html"), &host);
    let html = card_data(&body)["html"].as_str().unwrap().to_string();
    assert!(html.contains("<input type=text id=typeans onkeypress=\"_typeAnsPress();\""), "{html}");
    assert!(!html.contains("[[type:"), "{html}");

    let path = format!("/{generation}/cards/{row}-back.html?typed=forms%20sa%C3%BCts");
    let (status, _, body) = request(address, "GET", &path, &host);
    assert_eq!(status, 200, "{body}");
    let html = card_data(&body)["html"].as_str().unwrap().to_string();
    assert!(html.contains("<hr id=answer>\n<div style=\"font-family: 'Arial'; font-size: 20px\">"), "{html}");
    assert!(html.contains("typeGood") && html.contains("typeBad"), "{html}");
    assert!(!html.contains("[[type:") && !html.contains("<input"), "{html}");

    let (_, _, body) = request(address, "GET", &format!("/{generation}/cards/{row}-back.html"), &host);
    let html = card_data(&body)["html"].as_str().unwrap().to_string();
    assert!(html.contains("<code id=typeans>forms salts</code>"), "{html}");
}

fn export(dir: &Path, out: &Path) -> (janky_cli::diagnostic::Diagnostics, Vec<String>) {
    let mut diagnostics = janky_cli::diagnostic::Diagnostics::default();
    let mut messages = Vec::new();
    janky_cli::preview::export(
        &dir.join("deck.toml"),
        Default::default(),
        out,
        &mut diagnostics,
        &mut messages,
        &mut Vec::new(),
    );
    (diagnostics, messages)
}

fn export_codes(diagnostics: &janky_cli::diagnostic::Diagnostics) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.to_json()["severity"] == "error")
        .map(|diagnostic| diagnostic.to_json()["code"].as_str().unwrap().to_string())
        .collect()
}

/// The files below `dir` by their relative path.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = common::tree(dir)
        .into_iter()
        .map(|(path, bytes)| (path.strip_prefix(dir).unwrap().to_string_lossy().into_owned(), bytes))
        .collect();
    files.sort();
    files
}

fn siblings(out: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(out.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn export_writes_the_preview_as_static_files() {
    if !janky_cli::preview::ui_built() {
        return;
    }
    let (work, dir) = common::example();
    let out = work.path().join("site");
    let (diagnostics, messages) = export(&dir, &out);
    assert!(export_codes(&diagnostics).is_empty(), "{:?}", export_codes(&diagnostics));
    assert!(messages.iter().any(|message| message.starts_with("wrote ")), "{messages:?}");

    let json: Value = serde_json::from_slice(&std::fs::read(out.join("preview.json")).unwrap()).unwrap();
    assert_eq!(json["exported"], true);
    assert_eq!(json["stale"], false);
    assert_eq!(json["diagnostics"], serde_json::json!([]));
    assert_eq!(rows(&json).len(), card_count(&dir));
    let generation = json["generation"].as_str().unwrap();
    assert!(generation.len() == 32 && generation.bytes().all(|byte| byte.is_ascii_hexdigit()), "{generation}");

    assert!(std::fs::read_to_string(out.join("index.html")).unwrap().contains("<title>janky preview</title>"));
    assert!(out.join(".janky-export").is_file());
    for row in 0..rows(&json).len() {
        for side in ["front", "back"] {
            let body = std::fs::read_to_string(out.join(format!("{generation}/cards/{row}-{side}.html"))).unwrap();
            assert!(body.contains("<base href=\"../media/\">"), "{body}");
            assert_eq!(card_data(&body)["side"], side);
        }
    }
    assert_eq!(
        std::fs::read(out.join(format!("{generation}/media/diamond.png"))).unwrap(),
        std::fs::read(dir.join("media/images/diamond.png")).unwrap()
    );
    assert_eq!(siblings(&out), ["example", "site"]);
}

#[test]
fn export_gives_the_same_files_for_the_same_project() {
    if !janky_cli::preview::ui_built() {
        return;
    }
    let (work, dir) = common::example();
    let (first, second) = (work.path().join("first"), work.path().join("second"));
    export(&dir, &first);
    export(&dir, &second);
    // Anki writes a different `deck.apkg` each time, so only its name is compared.
    let site = |dir: &Path| -> Vec<(String, Vec<u8>)> {
        files(dir)
            .into_iter()
            .map(|(path, bytes)| if path == "deck.apkg" { (path, Vec::new()) } else { (path, bytes) })
            .collect()
    };
    assert_eq!(site(&first), site(&second));
}

#[test]
fn export_writes_no_files_in_the_project() {
    if !janky_cli::preview::ui_built() {
        return;
    }
    let (work, dir) = common::example();
    let before = common::tree(&dir);
    export(&dir, &work.path().join("site"));
    assert_eq!(common::tree(&dir), before);
}

#[test]
fn export_refuses_a_project_with_errors_and_writes_nothing() {
    if !janky_cli::preview::ui_built() {
        return;
    }
    let (work, dir) = common::example();
    edit(&dir.join("content/groups/basics.toml"), "property = 'forms salts'\n", "");
    let out = work.path().join("site");
    let (diagnostics, _) = export(&dir, &out);
    assert!(!export_codes(&diagnostics).is_empty());
    assert!(!out.exists());
    assert_eq!(siblings(&out), ["example"]);
}

#[test]
fn export_refuses_a_directory_that_is_not_an_earlier_export() {
    if !janky_cli::preview::ui_built() {
        return;
    }
    let (work, dir) = common::example();
    let full = work.path().join("full");
    std::fs::create_dir(&full).unwrap();
    std::fs::write(full.join("keep.txt"), "mine").unwrap();
    let file = work.path().join("file");
    std::fs::write(&file, "mine").unwrap();
    let link = work.path().join("link");
    std::os::unix::fs::symlink(work.path().join("elsewhere"), &link).unwrap();
    for out in [&full, &file, &link] {
        let (diagnostics, _) = export(&dir, out);
        assert_eq!(export_codes(&diagnostics), ["preview::export-target"], "{}", out.display());
    }
    assert_eq!(std::fs::read_to_string(full.join("keep.txt")).unwrap(), "mine");
    assert!(!work.path().join("elsewhere").exists());
    assert_eq!(siblings(&full), ["example", "file", "full", "link"]);
}

#[test]
fn export_replaces_an_earlier_export_and_fills_an_empty_directory() {
    if !janky_cli::preview::ui_built() {
        return;
    }
    let (work, dir) = common::example();
    let out = work.path().join("site");
    std::fs::create_dir(&out).unwrap();
    let (diagnostics, _) = export(&dir, &out);
    assert!(export_codes(&diagnostics).is_empty(), "{:?}", export_codes(&diagnostics));
    let first = files(&out);
    std::fs::write(out.join("stray.txt"), "old").unwrap();
    edit(&dir.join("content/groups/basics.toml"), "property = 'forms salts'", "property = 'forms many salts'");
    let (diagnostics, _) = export(&dir, &out);
    assert!(export_codes(&diagnostics).is_empty(), "{:?}", export_codes(&diagnostics));
    assert!(!out.join("stray.txt").exists());
    assert_ne!(files(&out), first);
    assert_eq!(siblings(&out), ["example", "site"]);
}

#[test]
fn export_shows_the_expected_answer_without_a_comparison() {
    if !janky_cli::preview::ui_built() {
        return;
    }
    let (work, dir) = common::example();
    let cards = dir.join("notetypes/group/cards");
    let front = std::fs::read_to_string(cards.join("group.front.html")).unwrap();
    std::fs::write(cards.join("group.front.html"), format!("{front}{{{{type:Property}}}}")).unwrap();
    std::fs::write(cards.join("group.back.html"), "{{FrontSide}}<hr id=answer>{{type:Property}}").unwrap();
    let out = work.path().join("site");
    export(&dir, &out);
    let json: Value = serde_json::from_slice(&std::fs::read(out.join("preview.json")).unwrap()).unwrap();
    let generation = json["generation"].as_str().unwrap();
    let row = rows(&json).iter().position(|row| row["entry"]["id"] == "halogens").unwrap();
    assert_eq!(rows(&json)[row]["type_answer"], true);
    let body = std::fs::read_to_string(out.join(format!("{generation}/cards/{row}-back.html"))).unwrap();
    let html = card_data(&body)["html"].as_str().unwrap().to_string();
    assert!(html.contains("<code id=typeans>forms salts</code>"), "{html}");
    assert!(!html.contains("typeBad") && !html.contains("[[type:"), "{html}");
}

fn export_warnings(diagnostics: &janky_cli::diagnostic::Diagnostics) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.to_json()["severity"] == "warning")
        .map(|diagnostic| diagnostic.to_json()["code"].as_str().unwrap().to_string())
        .collect()
}

fn import(apkg: &Path, collection: &Path) -> janky_anki::Imported {
    let options = janky_anki::ImportOptions {
        update_if_newer: true,
        merge_notetypes: true,
        with_deck_configs: true,
    };
    janky_anki::import(apkg, collection, options).unwrap()
}

#[test]
fn export_includes_the_package_of_the_build_when_deck_lock_is_current() {
    if !janky_cli::preview::ui_built() {
        return;
    }
    let (work, dir) = common::example();
    let out = work.path().join("site");
    let (diagnostics, messages) = export(&dir, &out);
    assert!(export_codes(&diagnostics).is_empty(), "{:?}", export_codes(&diagnostics));
    assert!(!export_warnings(&diagnostics).contains(&"preview::export-lock".to_string()));
    assert!(messages.iter().any(|message| message.ends_with(", deck.apkg")), "{messages:?}");

    let lock = std::fs::read(dir.join("deck.lock")).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_janky"))
        .args(["--deck", dir.join("deck.toml").to_str().unwrap(), "build"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(std::fs::read(dir.join("deck.lock")).unwrap(), lock, "the build does not change a current lock");

    let collection = work.path().join("collection");
    let first = import(&out.join("deck.apkg"), &collection);
    assert!(first.new > 0);
    let second = import(&dir.join(".janky/build/periodic.apkg"), &collection);
    assert_eq!((second.new, second.updated, second.duplicate), (0, 0, first.new));
}

#[test]
fn export_has_no_package_when_deck_lock_is_missing_or_out_of_date() {
    if !janky_cli::preview::ui_built() {
        return;
    }
    let (work, dir) = common::example();
    let basics = dir.join("content/groups/basics.toml");
    let mut text = std::fs::read_to_string(&basics).unwrap();
    text.push_str("\n[[entries]]\nid = 'noble-gases'\ngroup = 'Group 18'\nproperty = 'does not react'\n");
    std::fs::write(&basics, text).unwrap();
    let stale = work.path().join("stale");
    let (diagnostics, messages) = export(&dir, &stale);
    assert!(export_codes(&diagnostics).is_empty(), "{:?}", export_codes(&diagnostics));
    assert!(export_warnings(&diagnostics).contains(&"preview::export-lock".to_string()));
    assert!(!messages.iter().any(|message| message.contains("deck.apkg")), "{messages:?}");
    assert!(stale.join("preview.json").is_file());
    assert!(!stale.join("deck.apkg").exists());

    std::fs::remove_file(dir.join("deck.lock")).unwrap();
    let missing = work.path().join("missing");
    let (diagnostics, _) = export(&dir, &missing);
    assert!(export_warnings(&diagnostics).contains(&"preview::export-lock".to_string()));
    assert!(!missing.join("deck.apkg").exists());
    assert!(!dir.join("deck.lock").exists(), "the export does not write deck.lock");
}
