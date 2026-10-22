mod common;

use janky_cli::content::ContentFile;
use janky_cli::diagnostic::Diagnostics;
use janky_cli::lock::{self, LockFile, LockedMedia};
use janky_cli::plan::{Change, OrderChange, Plan, plan};
use janky_cli::project::Project;
use janky_cli::render::Rendered;
use janky_cli::{content, package, plugin, project, render};

struct Example {
    _work: tempfile::TempDir,
    project: Project,
    files: Vec<ContentFile>,
    rendered: Rendered,
    previous: LockFile,
}

impl Example {
    fn load() -> Self {
        let (work, dir) = common::example();
        let mut diagnostics = Diagnostics::default();
        let project = project::load(&dir.join("deck.toml"), &mut diagnostics).unwrap();
        let files = content::load(&project, &mut diagnostics);
        let rendered =
            render::render_with(&project, plugin::load(&project, &mut diagnostics).as_ref(), &files, &mut diagnostics);
        let previous = lock::read(&dir.join("deck.lock"), &mut diagnostics).unwrap();
        assert!(diagnostics.is_empty());
        Self {
            _work: work,
            project,
            files,
            rendered,
            previous,
        }
    }

    fn plan(&self) -> Plan {
        let mut next = self.previous.clone();
        let mut diagnostics = Diagnostics::default();
        package::package(&self.project, &self.files, &self.rendered, &mut next, 1_900_000_000, &mut diagnostics)
            .unwrap();
        plan(&self.previous, &next)
    }
}

fn s(text: &str) -> String {
    text.to_string()
}

#[test]
fn a_build_without_edits_has_an_empty_plan() {
    let plan = Example::load().plan();
    assert_eq!(plan.changes, []);
    assert_eq!(plan.unchanged_notes, 7);
}

#[test]
fn notes_added_changed_removed_moved_and_retyped() {
    let mut example = Example::load();
    example.rendered.notes[2].fields["name"] = s("but");
    example.rendered.notes.remove(3);
    example.previous.notes.shift_remove("c");
    example.previous.notes["fe"].deck = s("groups");
    example.previous.notes["au"].notetype = s("group");
    let plan = example.plan();
    assert_eq!(
        plan.changes,
        [
            Change::NoteChanged { id: s("li"), deck: s("elements") },
            Change::NoteAdded { id: s("c"), deck: s("elements") },
            Change::NoteMoved {
                id: s("fe"),
                from: s("groups"),
                to: s("elements")
            },
            Change::NoteRetyped {
                id: s("au"),
                from: s("group"),
                to: s("element")
            },
            Change::NoteRemoved { id: s("water"), deck: s("elements") },
        ]
    );
    assert_eq!(plan.unchanged_notes, 2);
}

#[test]
fn decks_renamed_added_and_removed() {
    let mut example = Example::load();
    example.project.deck.deck.deck.name = s("Chemistry");
    example.previous.deck.as_mut().unwrap().children.shift_remove("elements");
    example.project.deck.deck.deck.children.shift_remove("groups");
    let plan = example.plan();
    let decks: Vec<&Change> = plan.changes.iter().filter(|change| format!("{change:?}").starts_with("Deck")).collect();
    assert_eq!(
        decks,
        [
            &Change::DeckRenamed {
                key: s(""),
                from: s("Periodic Table"),
                to: s("Chemistry")
            },
            &Change::DeckAdded {
                key: s("elements"),
                name: s("Chemistry::0 Elements")
            },
            &Change::DeckRemoved {
                key: s("groups"),
                name: s("Periodic Table::1 Groups 🧪")
            },
        ]
    );
}

#[test]
fn presets_changed_and_removed() {
    let mut example = Example::load();
    example.project.deck_tree.value["presets"]["elements"]["new_per_day"] = 30.into();
    example.project.deck.presets.shift_remove("parent");
    let plan = example.plan();
    assert_eq!(
        plan.changes,
        [
            Change::PresetChanged {
                key: s("elements"),
                name: s("Element drills")
            },
            Change::PresetRemoved { key: s("parent"), name: s("parent") },
        ]
    );
}

#[test]
fn note_type_schema_template_and_name_changes() {
    let mut example = Example::load();
    let element = &mut example.project.notetypes.get_mut("element").unwrap().file;
    element.fields.move_index(element.fields.get_index_of("data").unwrap(), 0);
    element.fields.shift_remove("notes");
    example.rendered.notetypes["group"].cards["group"].front = s("<b>{{Group}}</b>");
    let plan = example.plan();
    assert_eq!(
        plan.changes,
        [
            Change::SchemaChanged {
                notetype: s("element"),
                fields: OrderChange {
                    added: vec![],
                    removed: vec![s("notes")],
                    moved: true
                },
                cards: OrderChange::default(),
            },
            Change::NotetypeChanged { notetype: s("group") },
        ]
    );

    let mut example = Example::load();
    example.project.notetypes.get_mut("group").unwrap().file.name = s("Groups");
    assert_eq!(
        example.plan().changes,
        [Change::NotetypeRenamed {
            key: s("group"),
            from: s("Element Group"),
            to: s("Groups")
        }]
    );
}

#[test]
fn media_added_changed_and_removed() {
    let mut example = Example::load();
    example.previous.media["diamond.png"].hash = s("0000000000000000");
    example.previous.media.insert(s("old.png"), LockedMedia { hash: s("1111111111111111") });
    assert_eq!(
        example.plan().changes,
        [
            Change::MediaChanged { name: s("diamond.png") },
            Change::MediaRemoved { name: s("old.png") },
        ]
    );

    let mut example = Example::load();
    example.previous.media.clear();
    let mut changes = example.plan().changes;
    let mut expected: Vec<Change> =
        example.rendered.media.iter().map(|file| Change::MediaAdded { name: file.name.clone() }).collect();
    assert!(expected.contains(&Change::MediaAdded { name: s("diamond.png") }));
    assert_eq!(expected.len(), 6);
    let name = |change: &Change| format!("{change:?}");
    changes.sort_by_key(name);
    expected.sort_by_key(name);
    assert_eq!(changes, expected);
}

#[test]
fn text_names_decks_and_says_what_anki_does() {
    let mut example = Example::load();
    example.rendered.notes.remove(3);
    example.previous.notes["fe"].deck = s("groups");
    let plan = example.plan();
    let mut next = example.previous.clone();
    let mut diagnostics = Diagnostics::default();
    package::package(&example.project, &example.files, &example.rendered, &mut next, 1_900_000_000, &mut diagnostics)
        .unwrap();
    assert_eq!(
        plan.text(&example.previous, &next),
        "Notes: 0 added, 0 changed, 1 removed, 1 moved, 5 unchanged\n\
         ~ note fe moves from Periodic Table::1 Groups 🧪 to Periodic Table::0 Elements: its cards stay in Periodic Table::1 Groups 🧪. \
         Move them in Anki (Browse, Change Deck)\n\
         - note water in Periodic Table::0 Elements: the note stays in Anki. Delete it there\n"
    );
}

#[test]
fn advice_for_new_presets_and_schema_changes() {
    let mut example = Example::load();
    example.previous.presets.shift_remove("elements");
    let element = example.previous.notetypes.get_mut("element").unwrap();
    element.fields.shift_remove("data");
    let plan = example.plan();
    assert_eq!(
        plan.advice(),
        [
            s(r#"Tick "Import any deck presets". Anki adds a preset only the first time."#),
            s(r#"Turn on "Merge note types". The import needs a full sync afterwards."#),
            s(r#"Set "Update notes" to "Always", so that the new fields get their values."#),
        ]
    );
    assert_eq!(plan.to_json()["plan"]["changes"][1]["kind"], "schema-changed");
}

fn every_publish_change() -> janky_anki::Diff {
    janky_anki::Diff {
        notetypes_added: vec!["Compound".into()],
        notetypes_changed: vec!["Element".into()],
        schema_changed: vec!["Element Group".into()],
        notetypes_merged: vec![("Element Group+".into(), "Element Group".into())],
        presets_added: vec!["Lessons".into()],
        presets_changed: vec![("Deck".into(), vec!["name", "new_per_day"])],
        decks_added: vec!["Periodic Table::02 Extra".into()],
        decks_renamed: vec![("Periodic Table::01 Metals".into(), "Periodic Table::01 Metals 🧪".into())],
        decks_preset: vec!["Periodic Table".into()],
        notes_added: vec!["guid-b".into(), "guid-a".into()],
        notes_changed: vec!["guid-c".into()],
        notes_kept: vec!["guid-d".into()],
        notes_unchanged: 7,
        notes_removed: vec!["guid-gone".into(), "guid-anki".into()],
        cards_moved: 2,
    }
}

fn publish_ids() -> std::collections::HashMap<String, String> {
    [
        ("guid-a", "ar"),
        ("guid-b", "c"),
        ("guid-c", "cu"),
        ("guid-d", "ne"),
        ("guid-gone", "gone"),
    ]
    .into_iter()
    .map(|(guid, id)| (guid.to_string(), id.to_string()))
    .collect()
}

#[test]
fn the_publish_plan_names_every_change() {
    let text = janky_cli::plan::publish_text(&every_publish_change(), &publish_ids(), false);
    assert_eq!(
        text,
        "Notes: 2 added, 1 changed, 1 kept, 1 removed, 7 unchanged\n\
         + note type Compound\n\
         ~ note type Element: card templates, CSS or names\n\
         - note type Element Group+: a copy of Element Group. Its notes move to Element Group, and the copy is deleted\n\
         ~ note type Element Group: fields or card types change. Needs a full sync (`--allow-full-sync`)\n\
         + preset Lessons\n\
         ~ preset Deck: sets name, new_per_day\n\
         + deck Periodic Table::02 Extra\n\
         ~ deck Periodic Table::01 Metals is renamed to Periodic Table::01 Metals 🧪\n\
         ~ deck Periodic Table: sets the preset back\n\
         + note ar\n\
         + note c\n\
         ~ note cu\n\
         = note ne: edited in Anki, so it stays as it is. `--overwrite-anki-edits` overwrites it\n\
         - note gone: stays in Anki. `--delete-removed` deletes it\n\
         = note guid-anki: made in Anki with a janky note type. janky never deletes it\n\
         ~ 2 cards move to the decks of their notes\n"
    );
    let deleting = janky_cli::plan::publish_text(&every_publish_change(), &publish_ids(), true);
    assert!(deleting.contains("- note gone: deleted, with its cards\n"), "{deleting}");
    assert!(deleting.contains("= note guid-anki: made in Anki"), "{deleting}");
}

#[test]
fn an_empty_publish_plan_says_so() {
    let diff = janky_anki::Diff { notes_unchanged: 3, ..Default::default() };
    assert_eq!(
        janky_cli::plan::publish_text(&diff, &publish_ids(), false),
        "Notes: 0 added, 0 changed, 0 kept, 0 removed, 3 unchanged\nNo changes.\n"
    );
}

#[test]
fn the_publish_plan_has_a_json_form() {
    let json = janky_cli::plan::publish_json(&every_publish_change(), &publish_ids());
    assert_eq!(
        json,
        serde_json::json!({ "publish": {
            "notetypes_added": ["Compound"],
            "notetypes_changed": ["Element"],
            "notetypes_merged": [{ "copy": "Element Group+", "into": "Element Group" }],
            "schema_changed": ["Element Group"],
            "presets_added": ["Lessons"],
            "presets_changed": [{ "name": "Deck", "keys": ["name", "new_per_day"] }],
            "decks_added": ["Periodic Table::02 Extra"],
            "decks_renamed": [{ "from": "Periodic Table::01 Metals", "to": "Periodic Table::01 Metals 🧪" }],
            "decks_preset": ["Periodic Table"],
            "notes_added": ["ar", "c"],
            "notes_changed": ["cu"],
            "notes_kept": ["ne"],
            "notes_removed": ["gone"],
            "notes_from_anki": ["guid-anki"],
            "notes_unchanged": 7,
            "cards_moved": 2,
        }})
    );
}

#[test]
fn collection_names_cannot_hide_or_forge_plan_lines() {
    let diff = janky_anki::Diff {
        decks_renamed: vec![("evil\u{1b}[2J\u{202e}\n+ note fake".into(), "Chemistry".into())],
        notes_removed: vec!["guid\u{7}".into()],
        ..Default::default()
    };
    let text = janky_cli::plan::publish_text(&diff, &publish_ids(), false);
    assert!(!text.contains('\u{1b}') && !text.contains('\u{202e}') && !text.contains('\u{7}'), "{text}");
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(text.contains("~ deck evil\\u{1b}[2J\\u{202e}\\n+ note fake is renamed to Chemistry"), "{text}");
}
