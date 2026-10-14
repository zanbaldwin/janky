use anki::collection::Collection;
use anki::deckconfig::DeckConfigId;
use anki::decks::DeckId;
use janky_anki::{
    Applied, Apply, Card, Deck, Diff, Field, Note, Notetype, Package, Preset, PresetOptions, apply, diff, open_copy,
};

fn base() -> Package {
    Package {
        notetypes: vec![Notetype {
            id: 1001,
            name: "Type".into(),
            css: String::new(),
            sort_field: 0,
            modified: 1000,
            fields: vec![
                Field {
                    id: 11,
                    name: "Front".into(),
                    description: String::new(),
                    collapsed: false,
                },
                Field {
                    id: 12,
                    name: "Back".into(),
                    description: String::new(),
                    collapsed: false,
                },
            ],
            cards: vec![Card {
                id: 21,
                name: "Card 1".into(),
                front: "{{Front}}".into(),
                back: "{{Back}}".into(),
            }],
        }],
        presets: vec![Preset {
            id: 200,
            name: "Lessons".into(),
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
                preset: None,
            },
        ],
        notes: vec![note("n1", "one", 1000), note("n2", "two", 1000)],
        media: vec![],
    }
}

fn note(guid: &str, front: &str, modified: i64) -> Note {
    Note {
        guid: guid.into(),
        notetype: 1001,
        deck: 101,
        fields: vec![front.into(), "back".into()],
        tags: vec![],
        modified,
    }
}

fn copy() -> (tempfile::TempDir, Collection) {
    let work = tempfile::tempdir().unwrap();
    let col = open_copy(&work.path().join("copy.anki2")).unwrap();
    (work, col)
}

fn first(col: &mut Collection) -> Applied {
    apply(col, &base(), &Apply::default()).unwrap()
}

fn query(col: &Collection, sql: &str) -> i64 {
    col.storage.db().query_row(sql, [], |row| row.get(0)).unwrap()
}

fn card_deck(col: &Collection, guid: &str) -> i64 {
    query(col, &format!("select c.did from cards c join notes n on n.id = c.nid where n.guid = '{guid}'"))
}

#[test]
fn a_first_apply_adds_notes_decks_and_presets() {
    let (_work, mut col) = copy();
    let applied = first(&mut col);
    assert_eq!(query(&col, "select count() from notes"), 2);
    assert_eq!(applied.decks.len(), 3);
    assert!(applied.renamed.is_empty() && applied.presets.is_empty());
    let a = col.get_deck(DeckId(applied.decks[&101])).unwrap().unwrap();
    assert_eq!(a.human_name(), "Root::A");
    assert_eq!(a.config_id(), Some(DeckConfigId(200)));
    assert_eq!(card_deck(&col, "n1"), applied.decks[&101]);
}

#[test]
fn a_preset_change_reaches_a_preset_that_the_collection_has() {
    let (_work, mut col) = copy();
    first(&mut col);
    let mut kept = Collection::get_deck_config(&col, DeckConfigId(200), false).unwrap().unwrap();
    kept.inner.reviews_per_day = 77;
    let json = serde_json::to_vec(&anki::deckconfig::DeckConfSchema11::from(kept)).unwrap();
    let _ = anki::services::DeckConfigService::add_or_update_deck_config_legacy(
        &mut col,
        anki_proto::generic::Json { json },
    )
    .unwrap();

    let mut changed = base();
    changed.presets[0].options.new_per_day = Some(30);
    let applied = apply(&mut col, &changed, &Apply::default()).unwrap();
    assert_eq!(applied.presets, [200]);
    let config = Collection::get_deck_config(&col, DeckConfigId(200), false).unwrap().unwrap();
    assert_eq!((config.inner.new_per_day, config.inner.reviews_per_day), (30, 77));

    let again = apply(&mut col, &changed, &Apply::default()).unwrap();
    assert!(again.presets.is_empty());
}

#[test]
fn a_renamed_deck_keeps_its_cards() {
    let (_work, mut col) = copy();
    let applied = first(&mut col);
    let mut changed = base();
    changed.decks[1].name = "Root::Lessons".into();
    let renamed = apply(
        &mut col,
        &changed,
        &Apply {
            decks: applied.decks.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(renamed.renamed, [("Root::A".to_string(), "Root::Lessons".to_string())]);
    assert_eq!(renamed.decks[&101], applied.decks[&101]);
    assert_eq!(card_deck(&col, "n1"), applied.decks[&101]);
    assert_eq!(col.get_deck_id("Root::A").unwrap(), None);
}

#[test]
fn a_moved_note_moves_its_cards() {
    let (_work, mut col) = copy();
    let applied = first(&mut col);
    let mut changed = base();
    changed.notes[1].deck = 102;
    changed.notes[1].modified = 2000;
    let moved = apply(
        &mut col,
        &changed,
        &Apply {
            decks: applied.decks.clone(),
            moved: vec!["n2".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(moved.moved, 1);
    assert_eq!(card_deck(&col, "n2"), applied.decks[&102]);
    assert_eq!(card_deck(&col, "n1"), applied.decks[&101]);
}

#[test]
fn removed_notes_stay_unless_they_are_deleted() {
    let (_work, mut col) = copy();
    first(&mut col);
    let mut changed = base();
    changed.notes.pop();
    let kept = apply(&mut col, &changed, &Apply::default()).unwrap();
    assert_eq!((kept.deleted, query(&col, "select count() from notes")), (0, 2));
    let request = Apply {
        delete: vec!["n2".into(), "unknown".into()],
        ..Default::default()
    };
    let deleted = apply(&mut col, &changed, &request).unwrap();
    assert_eq!((deleted.deleted, query(&col, "select count() from notes")), (1, 1));
}

#[test]
fn review_history_survives_a_field_change() {
    let (_work, mut col) = copy();
    first(&mut col);
    let card = query(&col, "select c.id from cards c join notes n on n.id = c.nid where n.guid = 'n1'");
    col.storage
        .db()
        .execute_batch(&format!(
            "update cards set type = 2, queue = 2, ivl = 10, due = 100 where id = {card};
             insert into revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type)
             values (1700000000000, {card}, -1, 3, 10, 1, 2500, 5000, 1);"
        ))
        .unwrap();

    let mut changed = base();
    changed.notes[0] = note("n1", "one, edited", 2000);
    apply(&mut col, &changed, &Apply::default()).unwrap();
    let fields: String =
        col.storage.db().query_row("select flds from notes where guid = 'n1'", [], |row| row.get(0)).unwrap();
    assert!(fields.starts_with("one, edited"), "{fields}");
    assert_eq!(query(&col, &format!("select ivl from cards where id = {card}")), 10);
    assert_eq!(query(&col, &format!("select count() from revlog where cid = {card}")), 1);
}

#[test]
fn a_rename_onto_another_deck_is_refused() {
    let (_work, mut col) = copy();
    let applied = first(&mut col);
    let theirs = col.get_or_create_normal_deck("Root::Lessons").unwrap().id;
    let mut changed = base();
    changed.decks[1].name = "Root::Lessons".into();
    let request = Apply {
        decks: applied.decks.clone(),
        ..Default::default()
    };
    let error = diff(&mut col, &changed, &request).unwrap_err();
    assert!(error.0.contains("already has a deck with that name"), "{error}");
    let error = apply(&mut col, &changed, &request).unwrap_err();
    assert!(error.0.contains("already has a deck with that name"), "{error}");
    assert_eq!(col.get_deck_id("Root::A").unwrap(), Some(DeckId(applied.decks[&101])));
    assert_eq!(col.get_deck(theirs).unwrap().unwrap().config_id(), Some(DeckConfigId(1)));
    assert_eq!(card_deck(&col, "n1"), applied.decks[&101]);
}

#[test]
fn a_filtered_deck_with_a_package_name_stops_the_import() {
    for name in ["Root::B", "Root"] {
        let (_work, mut col) = copy();
        let mut filtered = anki::decks::Deck::new_filtered();
        filtered.name = anki::decks::NativeDeckName::from_human_name(name);
        col.add_or_update_deck(&mut filtered).unwrap();
        let error = diff(&mut col, &base(), &Apply::default()).unwrap_err();
        assert!(error.0.contains(&format!("`{name}` is a filtered deck")), "{error}");
        let error = apply(&mut col, &base(), &Apply::default()).unwrap_err();
        assert!(error.0.contains(&format!("`{name}` is a filtered deck")), "{error}");
        assert_eq!(query(&col, "select count() from notes"), 0);
    }
}

#[test]
fn a_card_that_the_user_moved_stays() {
    let (_work, mut col) = copy();
    let applied = first(&mut col);
    let card = query(&col, "select c.id from cards c join notes n on n.id = c.nid where n.guid = 'n1'");
    col.set_deck(&[anki::prelude::CardId(card)], DeckId(applied.decks[&102])).unwrap();
    let request = Apply {
        decks: applied.decks.clone(),
        ..Default::default()
    };
    let again = apply(&mut col, &base(), &request).unwrap();
    assert_eq!(again.moved, 0);
    assert_eq!(card_deck(&col, "n1"), applied.decks[&102]);
}

fn set_preset(col: &mut Collection, id: i64, change: impl FnOnce(&mut anki::deckconfig::DeckConfig)) {
    let mut config = Collection::get_deck_config(col, DeckConfigId(id), false).unwrap().unwrap();
    change(&mut config);
    let json = serde_json::to_vec(&anki::deckconfig::DeckConfSchema11::from(config)).unwrap();
    let _ =
        anki::services::DeckConfigService::add_or_update_deck_config_legacy(col, anki_proto::generic::Json { json })
            .unwrap();
}

#[test]
fn a_fresh_copy_gets_everything() {
    let (_work, mut col) = copy();
    let found = diff(&mut col, &base(), &Apply::default()).unwrap();
    assert_eq!(found.notetypes_added, ["Type"]);
    assert_eq!(found.presets_added, ["Lessons"]);
    assert_eq!(found.decks_added, ["Root", "Root::A", "Root::B"]);
    assert_eq!(found.notes_added, ["n1", "n2"]);
    assert!(!found.is_empty());
}

#[test]
fn nothing_changes_after_an_apply() {
    let (_work, mut col) = copy();
    let applied = first(&mut col);
    let request = Apply {
        decks: applied.decks,
        ..Default::default()
    };
    let found = diff(&mut col, &base(), &request).unwrap();
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(found.notes_unchanged, 2);
}

#[test]
fn the_diff_lists_each_change_and_apply_makes_it() {
    let (_work, mut col) = copy();
    let applied = first(&mut col);
    set_preset(&mut col, 200, |config| config.inner.new_per_day = 99);
    let b = applied.decks[&102];
    let mut deck = (*col.get_deck(DeckId(b)).unwrap().unwrap()).clone();
    if let anki::decks::DeckKind::Normal(normal) = &mut deck.kind {
        normal.config_id = 200;
    }
    col.update_deck(&mut deck).unwrap();

    let mut changed = base();
    changed.decks[1].name = "Root::Lessons".into();
    changed.notetypes[0].cards[0].back = "{{Back}}<br>".into();
    changed.notetypes[0].modified = 2000;
    changed.notes[0] = note("n1", "one, edited", 2000);
    changed.notes[1] = note("n3", "three", 2000);
    let request = Apply {
        decks: applied.decks.clone(),
        ..Default::default()
    };
    let found = diff(&mut col, &changed, &request).unwrap();
    assert_eq!(
        found,
        Diff {
            notetypes_changed: vec!["Type".into()],
            presets_changed: vec![("Lessons".into(), vec!["new_per_day"])],
            decks_renamed: vec![("Root::A".into(), "Root::Lessons".into())],
            decks_preset: vec!["Root::B".into()],
            notes_added: vec!["n3".into()],
            notes_changed: vec!["n1".into()],
            notes_removed: vec!["n2".into()],
            ..Default::default()
        }
    );

    let applied = apply(&mut col, &changed, &request).unwrap();
    let request = Apply {
        decks: applied.decks,
        ..Default::default()
    };
    let after = diff(&mut col, &changed, &request).unwrap();
    assert_eq!(
        after,
        Diff {
            notes_unchanged: 2,
            notes_removed: vec!["n2".into()],
            ..Default::default()
        }
    );
}

#[test]
fn a_note_edited_in_anki_is_kept() {
    let (_work, mut col) = copy();
    first(&mut col);
    let id = query(&col, "select id from notes where guid = 'n1'");
    let mut edited = col.storage.get_note(anki::notes::NoteId(id)).unwrap().unwrap();
    edited.set_field(0, "edited in Anki").unwrap();
    col.update_note(&mut edited).unwrap();

    let found = diff(&mut col, &base(), &Apply::default()).unwrap();
    assert_eq!((found.notes_kept, found.notes_changed.len(), found.notes_unchanged), (vec!["n1".to_string()], 0, 1));
    apply(&mut col, &base(), &Apply::default()).unwrap();
    let fields: String =
        col.storage.db().query_row("select flds from notes where guid = 'n1'", [], |row| row.get(0)).unwrap();
    assert!(fields.starts_with("edited in Anki"), "{fields}");
}

#[test]
fn an_added_field_is_a_schema_change() {
    let (_work, mut col) = copy();
    first(&mut col);
    let mut changed = base();
    changed.notetypes[0].fields.push(Field {
        id: 13,
        name: "Extra".into(),
        description: String::new(),
        collapsed: false,
    });
    for note in &mut changed.notes {
        note.fields.push(String::new());
    }
    let found = diff(&mut col, &changed, &Apply::default()).unwrap();
    assert_eq!(found.schema_changed, ["Type"]);
    assert!(found.notetypes_changed.is_empty());
}

#[test]
fn cards_of_moved_notes_are_counted() {
    let (_work, mut col) = copy();
    let applied = first(&mut col);
    let mut changed = base();
    changed.notes[1].deck = 102;
    changed.notes[1].modified = 2000;
    let request = Apply {
        decks: applied.decks,
        moved: vec!["n2".into()],
        ..Default::default()
    };
    let found = diff(&mut col, &changed, &request).unwrap();
    assert_eq!((found.cards_moved, found.notes_unchanged), (1, 2));
}

#[test]
fn a_note_under_another_note_type_is_a_schema_change() {
    let (_work, mut col) = copy();
    first(&mut col);
    let basic = col.get_notetype_by_name("Basic").unwrap().unwrap();
    let mut theirs = basic.new_note();
    theirs.guid = "n3".into();
    theirs.set_field(0, "made in Anki").unwrap();
    col.add_note(&mut theirs, DeckId(1)).unwrap();

    let mut changed = base();
    changed.notes.push(note("n3", "three", 2000));
    let found = diff(&mut col, &changed, &Apply::default()).unwrap();
    assert_eq!(found.schema_changed, ["Type"]);
    assert_eq!((found.notes_added.len(), found.notes_changed), (0, vec!["n3".to_string()]));

    let schema = query(&col, "select scm from col");
    apply(&mut col, &changed, &Apply::default()).unwrap();
    assert_ne!(query(&col, "select scm from col"), schema);
}

#[test]
fn a_copy_of_a_note_type_is_merged_with_a_full_sync() {
    let (_work, mut col) = copy();
    first(&mut col);
    let mut sibling = (*col.get_notetype(anki::notetype::NotetypeId(1001)).unwrap().unwrap()).clone();
    sibling.id = anki::notetype::NotetypeId(0);
    sibling.name = "Type+".into();
    sibling.config.original_id = Some(1001);
    col.add_notetype(&mut sibling, false).unwrap();
    let mut theirs = sibling.new_note();
    theirs.set_field(0, "in the copy").unwrap();
    col.add_note(&mut theirs, DeckId(1)).unwrap();

    let found = diff(&mut col, &base(), &Apply::default()).unwrap();
    assert_eq!(found.notetypes_merged, [("Type+".to_string(), "Type".to_string())]);
    assert_eq!(found.schema_changed, ["Type"]);

    let schema = query(&col, "select scm from col");
    apply(&mut col, &base(), &Apply::default()).unwrap();
    assert_ne!(query(&col, "select scm from col"), schema);
    assert!(col.get_notetype_by_name("Type+").unwrap().is_none());
}

#[test]
fn cards_that_move_into_a_new_deck_are_counted() {
    let (_work, mut col) = copy();
    let applied = first(&mut col);
    let mut changed = base();
    changed.decks.push(Deck {
        id: 103,
        name: "Root::C".into(),
        preset: None,
    });
    changed.notes[1].deck = 103;
    changed.notes[1].modified = 2000;
    let request = Apply {
        decks: applied.decks,
        moved: vec!["n2".into()],
        ..Default::default()
    };
    let found = diff(&mut col, &changed, &request).unwrap();
    assert_eq!((found.decks_added.clone(), found.cards_moved), (vec!["Root::C".to_string()], 1));
    assert_eq!(apply(&mut col, &changed, &request).unwrap().moved, 1);
}

#[test]
fn a_note_type_that_is_newer_in_anki_is_not_listed() {
    let (_work, mut col) = copy();
    first(&mut col);
    let mut changed = base();
    changed.notetypes[0].cards[0].back = "{{Back}}<hr>".into();
    let found = diff(&mut col, &changed, &Apply::default()).unwrap();
    assert!(found.notetypes_changed.is_empty(), "{found:?}");
    apply(&mut col, &changed, &Apply::default()).unwrap();
    let template =
        col.get_notetype(anki::notetype::NotetypeId(1001)).unwrap().unwrap().templates[0].config.a_format.clone();
    assert_eq!(template, "{{Back}}");
}

#[test]
fn the_collection_is_newer_than_its_last_sync_after_an_apply() {
    let (_work, mut col) = copy();
    first(&mut col);
    col.storage.db().execute_batch("update col set ls = 9000000000000").unwrap();
    apply(&mut col, &base(), &Apply::default()).unwrap();
    let (modified, synced) = (query(&col, "select mod from col"), query(&col, "select ls from col"));
    assert!(modified > synced, "{modified} {synced}");
}
