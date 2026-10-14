use std::path::{Path, PathBuf};

use janky_cli::content::{self, ContentFile};
use janky_cli::diagnostic::Diagnostics;
use janky_cli::lock::{self, LockFile};
use janky_cli::project::{self, Project};
use janky_cli::{plugin, render};

mod common;

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn load(dir: &Path) -> (Option<Project>, Vec<ContentFile>, Diagnostics) {
    let mut diagnostics = Diagnostics::default();
    let mut log = Vec::new();
    let checked = janky_cli::cli::check(&dir.join("deck.toml"), Default::default(), &mut diagnostics, &mut log);
    lock::read(&dir.join("deck.lock"), &mut diagnostics);
    match checked {
        Some(checked) => (Some(checked.project), checked.files, diagnostics),
        None => (None, Vec::new(), diagnostics),
    }
}

#[test]
fn example_loads_without_diagnostics() {
    let (_work, dir) = common::example();
    let (project, files, diagnostics) = load(&dir);
    assert_eq!(diagnostics.to_json_lines(), "");
    let project = project.unwrap();
    assert_eq!(project.notetypes.keys().collect::<Vec<_>>(), ["element", "group"]);
    assert_eq!(
        project.notetypes["element"].file.fields.keys().collect::<Vec<_>>(),
        [
            "symbol",
            "name",
            "notes",
            "image",
            "examples",
            "audio",
            "mass",
            "config",
            "state",
            "block",
            "discovered",
            "colour",
            "data"
        ]
    );

    let summary: Vec<_> = files
        .iter()
        .map(|file| {
            let entries: Vec<_> =
                file.entries.iter().map(|entry| (entry.id.as_str(), entry.notetype.as_str())).collect();
            (file.path.as_str(), file.deck.clone(), entries)
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                "content/groups/basics.toml",
                vec!["groups".to_string()],
                vec![("alkali-metals", "group"), ("halogens", "group")]
            ),
            (
                "content/periods/early.yaml",
                vec!["elements".to_string()],
                vec![("li", "element"), ("water", "element"), ("c", "element")]
            ),
            (
                "content/periods/late.json",
                vec!["elements".to_string()],
                vec![("fe", "element"), ("au", "element")]
            ),
        ]
    );

    let early = &files[1];
    assert_eq!(early.tree.value["entries"][0]["tags"], serde_json::json!(["early"]));
    assert_eq!(early.tree.value["entries"][1]["tags"], serde_json::json!(["common"]));
    let default_tags = early.tree.loc("/defaults/tags/0").unwrap().value.clone();
    assert_eq!(early.tree.loc("/entries/0/tags/0").unwrap().value, default_tags);
}

#[test]
fn error_fixtures_give_the_expected_diagnostics() {
    let mut cases: Vec<_> =
        std::fs::read_dir(data().join("errors")).unwrap().map(|entry| entry.unwrap().path()).collect();
    cases.sort();
    assert!(!cases.is_empty());
    for case in cases {
        let (_, _, diagnostics) = load(&case);
        assert!(diagnostics.has_errors(), "{}", case.display());
        let expected = std::fs::read_to_string(case.join("expected.jsonl")).unwrap_or_default();
        assert_eq!(diagnostics.to_json_lines(), expected, "{}", case.display());
    }
}

fn example_lock() -> (tempfile::TempDir, Project, LockFile) {
    let (work, dir) = common::example();
    let (project, _, diagnostics) = load(&dir);
    assert_eq!(diagnostics.to_json_lines(), "");
    let mut diagnostics = Diagnostics::default();
    let lock = lock::read(&dir.join("deck.lock"), &mut diagnostics).unwrap();
    assert!(diagnostics.is_empty());
    (work, project.unwrap(), lock)
}

#[test]
fn example_lock_stays_the_same() {
    let (_work, project, mut lock) = example_lock();
    lock.update(&project);
    let text = std::fs::read_to_string(data().join("example/deck.lock")).unwrap();
    assert_eq!(lock.to_toml(), text);
}

#[test]
fn new_lock_reads_back_and_keeps_its_ids() {
    let (_work, project, _) = example_lock();
    let mut lock = LockFile::new();
    lock.update(&project);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("deck.lock"), lock.to_toml()).unwrap();
    let mut diagnostics = Diagnostics::default();
    let mut read = lock::read(&dir.path().join("deck.lock"), &mut diagnostics).unwrap();
    assert!(diagnostics.is_empty());
    assert_eq!(read, lock);
    read.update(&project);
    assert_eq!(read, lock);
}

#[test]
fn removed_keys_keep_their_ids_and_come_back() {
    let (_work, mut project, mut lock) = example_lock();
    let before = lock.clone();
    let preset = project.deck.presets.shift_remove("elements").unwrap();
    let deck = project.deck.deck.deck.children.shift_remove("groups").unwrap();
    let field = project.notetypes.get_mut("element").unwrap().file.fields.shift_remove("examples").unwrap();
    lock.update(&project);
    assert!(lock.presets["elements"].removed);
    assert_eq!(lock.presets["elements"].id, before.presets["elements"].id);
    assert!(lock.deck.as_ref().unwrap().children["groups"].removed);
    assert!(lock.notetypes["element"].fields["examples"].removed);
    assert!(lock.to_toml().contains("examples = { id = "));
    assert!(lock.to_toml().contains(", removed = true }"));

    project.deck.presets.insert("elements".into(), preset);
    project.deck.deck.deck.children.insert("groups".into(), deck);
    project.notetypes.get_mut("element").unwrap().file.fields.insert("notes".into(), field);
    lock.update(&project);
    assert_eq!(lock.presets["elements"], before.presets["elements"]);
    assert_eq!(lock.deck.as_ref().unwrap().children["groups"], before.deck.as_ref().unwrap().children["groups"]);
    assert!(lock.notetypes["element"].fields["examples"].removed);
    let notes = lock.notetypes["element"].fields["notes"].id;
    assert!(!lock.to_toml().replace(&format!("notes = {{ id = {notes} }}"), "").contains(&notes.to_string()));
}

#[test]
fn example_renders_the_expected_fields() {
    let (_work, dir) = common::example();
    let expected_dir = data().join("example");
    let mut diagnostics = Diagnostics::default();
    let project = project::load(&dir.join("deck.toml"), &mut diagnostics).unwrap();
    let files = content::load(&project, &mut diagnostics);
    let rendered =
        render::render_with(&project, plugin::load(&project, &mut diagnostics).as_ref(), &files, &mut diagnostics);
    assert_eq!(diagnostics.to_json_lines(), "");
    let expected = std::fs::read_to_string(expected_dir.join("expected/rendered.json")).unwrap();
    assert_eq!(serde_json::to_string_pretty(&rendered).unwrap() + "\n", expected);
}

#[test]
fn lock_records_names_and_the_current_order() {
    let (_work, mut project, mut lock) = example_lock();
    project.deck.deck.deck.name = "Elements".into();
    project.deck.presets.get_mut("elements").unwrap().name = None;
    let fields = &mut project.notetypes.get_mut("element").unwrap().file.fields;
    fields.move_index(fields.get_index_of("data").unwrap(), 0);
    fields.shift_remove("notes");
    lock.update(&project);

    let deck = lock.deck.as_ref().unwrap();
    assert_eq!(deck.name.as_deref(), Some("Elements"));
    assert_eq!(deck.children["elements"].name.as_deref(), Some("Elements::0 Elements"));
    assert_eq!(lock.presets["elements"].name.as_deref(), Some("elements"));
    assert_eq!(lock.notetypes["element"].name.as_deref(), Some("Element"));
    let order: Vec<(&str, bool)> =
        lock.notetypes["element"].fields.iter().map(|(key, field)| (key.as_str(), field.removed)).collect();
    assert_eq!(
        order,
        [
            ("data", false),
            ("symbol", false),
            ("name", false),
            ("image", false),
            ("examples", false),
            ("audio", false),
            ("mass", false),
            ("config", false),
            ("state", false),
            ("block", false),
            ("discovered", false),
            ("colour", false),
            ("_make_production", false),
            ("notes", true),
        ]
    );
}

#[test]
fn plugin_filters_and_functions_render() {
    let dir = data().join("plugin-test");
    let mut diagnostics = Diagnostics::default();
    let mut log = Vec::new();
    let checked =
        janky_cli::cli::check(&dir.join("deck.toml"), Default::default(), &mut diagnostics, &mut log).unwrap();
    assert_eq!(diagnostics.to_json_lines(), "");
    assert_eq!(log, ["length of\t<p><strong>\u{1f469}\u{200d}\u{1f52c}</strong></p>"]);
    let expected = std::fs::read_to_string(dir.join("expected/rendered.json")).unwrap();
    assert_eq!(serde_json::to_string_pretty(&checked.rendered).unwrap() + "\n", expected);
}

#[test]
fn generate_records_jobs_and_returns_media_values() {
    let dir = data().join("generate-test");
    let mut diagnostics = Diagnostics::default();
    let mut log = Vec::new();
    let checked =
        janky_cli::cli::check(&dir.join("deck.toml"), Default::default(), &mut diagnostics, &mut log).unwrap();
    assert_eq!(diagnostics.to_json_lines(), "");
    let expected = std::fs::read_to_string(dir.join("expected/rendered.json")).unwrap();
    assert_eq!(serde_json::to_string_pretty(&checked.rendered).unwrap() + "\n", expected);
}
