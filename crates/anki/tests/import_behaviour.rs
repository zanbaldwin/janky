use std::path::Path;

use janky_anki::{Card, Deck, Field, ImportOptions, Imported, Note, Notetype, Package, Preset, PresetOptions};

const DEFAULTS: ImportOptions = ImportOptions {
    update_if_newer: true,
    merge_notetypes: false,
    with_deck_configs: false,
};
const MERGE: ImportOptions = ImportOptions {
    update_if_newer: true,
    merge_notetypes: true,
    with_deck_configs: true,
};

fn notetype(id: i64, fields: &[(i64, &str)], cards: &[(i64, &str, &str)], modified: i64) -> Notetype {
    Notetype {
        id,
        name: format!("Type {id}"),
        css: String::new(),
        sort_field: 0,
        modified,
        fields: fields
            .iter()
            .map(|(id, name)| Field {
                id: *id,
                name: name.to_string(),
                description: String::new(),
                collapsed: false,
            })
            .collect(),
        cards: cards
            .iter()
            .map(|(id, name, front)| Card {
                id: *id,
                name: name.to_string(),
                front: front.to_string(),
                back: "{{FrontSide}}<hr id=answer>".into(),
            })
            .collect(),
    }
}

fn note(guid: &str, notetype: i64, deck: i64, fields: &[&str], modified: i64) -> Note {
    Note {
        guid: guid.into(),
        notetype,
        deck,
        fields: fields.iter().map(|field| field.to_string()).collect(),
        tags: vec![],
        modified,
    }
}

fn base() -> Package {
    Package {
        notetypes: vec![notetype(
            1001,
            &[(11, "Front"), (12, "Back")],
            &[(21, "Card 1", "{{Front}}")],
            1000,
        )],
        presets: vec![Preset {
            id: 200,
            name: "Preset".into(),
            options: PresetOptions {
                new_per_day: Some(15),
                ..Default::default()
            },
        }],
        decks: vec![
            Deck {
                id: 100,
                name: "Root".into(),
                preset: Some(200),
            },
            Deck {
                id: 101,
                name: "Root::A".into(),
                preset: Some(200),
            },
            Deck {
                id: 102,
                name: "Root::B".into(),
                preset: Some(200),
            },
        ],
        notes: vec![
            note("n1", 1001, 101, &["one", "eins"], 1000),
            note("n2", 1001, 101, &["two", "zwei"], 1000),
        ],
        media: vec![],
    }
}

fn run(options: ImportOptions, change: impl FnOnce(&mut Package)) -> (Imported, Imported) {
    let work = tempfile::tempdir().unwrap();
    let collection = work.path().join("collection");
    let first = import(&base(), &work.path().join("first.apkg"), &collection, options);
    let mut changed = base();
    change(&mut changed);
    let second = import(&changed, &work.path().join("second.apkg"), &collection, options);
    (first, second)
}

fn deck_names(imported: &Imported) -> Vec<&str> {
    imported.decks.iter().map(|(name, _)| name.as_str()).filter(|name| name.starts_with("Root")).collect()
}

fn found<'a>(imported: &'a Imported, guid: &str) -> &'a janky_anki::ImportedNote {
    imported.notes.iter().find(|note| note.guid == guid).unwrap()
}

fn notetype_names(imported: &Imported) -> Vec<&str> {
    imported
        .notetypes
        .iter()
        .map(|notetype| notetype.name.as_str())
        .filter(|name| name.starts_with("Type"))
        .collect()
}

fn field_names(imported: &Imported, id: i64) -> Vec<&str> {
    let notetype = imported.notetypes.iter().find(|notetype| notetype.id == id).unwrap();
    notetype.fields.iter().map(|(_, name, _)| name.as_str()).collect()
}

fn counts(imported: &Imported) -> (usize, usize, usize) {
    (imported.new, imported.updated, imported.duplicate)
}

fn import(package: &Package, apkg: &Path, collection: &Path, options: ImportOptions) -> Imported {
    janky_anki::write_apkg(package, apkg).unwrap();
    janky_anki::import(apkg, collection, options).unwrap()
}

#[test]
fn decks_without_cards_are_not_imported() {
    let (first, _) = run(DEFAULTS, |_| {});
    assert_eq!(deck_names(&first), ["Root", "Root::A"]);
}

#[test]
fn a_renamed_deck_is_a_new_empty_deck() {
    for options in [DEFAULTS, MERGE] {
        let (_, second) = run(options, |package| package.decks[1].name = "Root::A2".into());
        assert_eq!(deck_names(&second), ["Root", "Root::A", "Root::A2"]);
        assert_eq!(found(&second, "n1").deck, "Root::A");
    }
}

#[test]
fn a_note_that_moves_deck_keeps_its_cards_in_the_old_deck() {
    for options in [DEFAULTS, MERGE] {
        let (_, second) = run(options, |package| {
            package.notes[0].deck = 102;
            package.notes[0].modified = 2000;
        });
        assert_eq!(counts(&second), (0, 1, 1));
        assert_eq!(deck_names(&second), ["Root", "Root::A", "Root::B"]);
        assert_eq!(found(&second, "n1").deck, "Root::A");
    }
}

#[test]
fn a_removed_entry_stays_in_anki() {
    let (_, second) = run(DEFAULTS, |package| {
        package.notes.pop();
    });
    assert_eq!(second.notes.len(), 2);
}

fn change_note_type(package: &mut Package) {
    package.notetypes.push(notetype(1002, &[(31, "Word"), (32, "Meaning")], &[(41, "Card W", "{{Word}}")], 2000));
    package.notes[0] = note("n1", 1002, 101, &["one", "eins"], 2000);
}

#[test]
fn a_note_type_change_is_skipped_without_merge() {
    let (_, second) = run(DEFAULTS, change_note_type);
    assert_eq!(counts(&second), (0, 0, 1));
    assert_eq!(found(&second, "n1").notetype, 1001);
}

#[test]
fn a_note_type_change_with_merge_mixes_both_note_types() {
    let (_, second) = run(MERGE, change_note_type);
    assert_eq!(found(&second, "n1").notetype, 1002);
    assert_eq!(field_names(&second, 1002), ["Word", "Meaning", "Front", "Back"]);
    assert_eq!(found(&second, "n1").cards, [0, 1]);
}

fn add_field(modified: i64) -> impl FnOnce(&mut Package) {
    move |package| {
        package.notetypes[0] =
            notetype(1001, &[(11, "Front"), (12, "Back"), (13, "Extra")], &[(21, "Card 1", "{{Front}}")], 2000);
        for note in &mut package.notes {
            note.fields.push("x".into());
            note.modified = modified;
        }
    }
}

#[test]
fn a_schema_change_without_merge_adds_a_copy_and_skips_the_notes() {
    let (_, second) = run(DEFAULTS, add_field(2000));
    assert_eq!(counts(&second), (0, 0, 0));
    assert_eq!(notetype_names(&second), ["Type 1001", "Type 1001+"]);
    assert_eq!(found(&second, "n1").fields, ["one", "eins"]);
}

#[test]
fn a_schema_change_with_merge_stamps_notes_with_the_import_time() {
    let (_, second) = run(MERGE, add_field(2000));
    assert_eq!(counts(&second), (0, 0, 2));
    assert_eq!(field_names(&second, 1001), ["Front", "Back", "Extra"]);
    assert_eq!(found(&second, "n1").fields, ["one", "eins", ""]);
    assert!(found(&second, "n1").modified > 2000);

    let (_, always) = run(ImportOptions { update_if_newer: false, ..MERGE }, add_field(2000));
    assert_eq!(found(&always, "n1").fields, ["one", "eins", "x"]);
    let (_, later) = run(MERGE, add_field(4_000_000_000));
    assert_eq!(found(&later, "n1").fields, ["one", "eins", "x"]);
}

#[test]
fn merge_keeps_removed_and_moved_fields() {
    let (_, removed) = run(MERGE, |package| {
        package.notetypes[0] = notetype(1001, &[(11, "Front")], &[(21, "Card 1", "{{Front}}")], 2000);
        for note in &mut package.notes {
            note.fields.pop();
            note.modified = 2000;
        }
    });
    assert_eq!(field_names(&removed, 1001), ["Front", "Back"]);
    assert_eq!(found(&removed, "n1").fields, ["one", ""]);

    let (_, moved) = run(MERGE, |package| {
        package.notetypes[0] = notetype(1001, &[(12, "Back"), (11, "Front")], &[(21, "Card 1", "{{Front}}")], 2000);
        for note in &mut package.notes {
            note.fields.reverse();
            note.modified = 2000;
        }
    });
    assert_eq!(field_names(&moved, 1001), ["Front", "Back"]);
    assert_eq!(found(&moved, "n1").fields, ["one", "eins"]);
}

#[test]
fn merge_adds_a_card_type_and_its_cards() {
    let (_, second) = run(MERGE, |package| {
        package.notetypes[0] = notetype(
            1001,
            &[(11, "Front"), (12, "Back")],
            &[(21, "Card 1", "{{Front}}"), (22, "Card 2", "{{Back}}")],
            2000,
        );
    });
    assert_eq!(found(&second, "n1").cards, [0, 1]);
}

#[test]
fn a_template_change_imports_without_merge() {
    let (_, second) = run(DEFAULTS, |package| {
        package.notetypes[0] =
            notetype(1001, &[(11, "Front"), (12, "Back")], &[(21, "Card 1", "<b>{{Front}}</b>")], 2000);
    });
    let notetype = second.notetypes.iter().find(|notetype| notetype.id == 1001).unwrap();
    assert_eq!(notetype.cards[0].2, "<b>{{Front}}</b>");
}

#[test]
fn presets_import_only_when_asked_and_only_once() {
    let (first, _) = run(DEFAULTS, |_| {});
    assert!(first.presets.iter().all(|preset| preset.0 != 200));
    assert!(first.decks.iter().all(|(_, preset)| *preset == 1));

    let (first, second) = run(MERGE, |package| package.presets[0].options.new_per_day = Some(99));
    assert!(first.presets.contains(&(200, "Preset".into(), 15, 0.9)));
    assert!(second.presets.contains(&(200, "Preset".into(), 15, 0.9)));
}
