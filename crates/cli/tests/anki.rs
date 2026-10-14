mod common;

use janky_cli::diagnostic::Diagnostics;
use janky_cli::lock::{self, LockFile};
use janky_cli::project::Project;
use janky_cli::render::Rendered;
use janky_cli::{content, package, plugin, project, render};

const NOW: i64 = 1_760_000_000;

fn example() -> (tempfile::TempDir, Project, Vec<content::ContentFile>, Rendered, LockFile) {
    let (work, dir) = common::example();
    let mut diagnostics = Diagnostics::default();
    let project = project::load(&dir.join("deck.toml"), &mut diagnostics).unwrap();
    let files = content::load(&project, &mut diagnostics);
    let rendered =
        render::render_with(&project, plugin::load(&project, &mut diagnostics).as_ref(), &files, &mut diagnostics);
    let lock = lock::read(&dir.join("deck.lock"), &mut diagnostics).unwrap();
    assert_eq!(diagnostics.to_json_lines(), "");
    (work, project, files, rendered, lock)
}

#[test]
fn example_package_imports_into_anki_and_stays_stable() {
    let (_work, project, files, mut rendered, mut lock) = example();
    lock.notes.clear();
    for notetype in lock.notetypes.values_mut() {
        (notetype.hash, notetype.modified) = (None, None);
    }
    let mut diagnostics = Diagnostics::default();
    let package = package::package(&project, &files, &rendered, &mut lock, NOW, &mut diagnostics).unwrap();
    assert!(diagnostics.is_empty());
    let work = tempfile::tempdir().unwrap();
    let apkg = work.path().join("example.apkg");
    assert_eq!(janky_anki::write_apkg(&package, &apkg).unwrap(), 7);

    let collection = work.path().join("collection");
    let first = janky_anki::import(
        &apkg,
        &collection,
        janky_anki::ImportOptions {
            update_if_newer: true,
            merge_notetypes: true,
            with_deck_configs: true,
        },
    )
    .unwrap();
    assert_eq!((first.new, first.updated, first.duplicate), (7, 0, 0));

    for notetype in &package.notetypes {
        let imported = first.notetypes.iter().find(|imported| imported.id == notetype.id).unwrap();
        assert_eq!(imported.modified, NOW);
        let fields: Vec<_> =
            notetype.fields.iter().map(|field| (Some(field.id), field.name.clone(), field.collapsed)).collect();
        assert_eq!(imported.fields, fields);
        let cards: Vec<_> = notetype.cards.iter().map(|card| card.id).collect();
        assert_eq!(imported.cards.iter().map(|card| card.0.unwrap()).collect::<Vec<_>>(), cards);
    }
    let element = first.notetypes.iter().find(|notetype| notetype.name == "Element").unwrap();
    assert!(element.cards[1].2.starts_with("{{#_make_production}}"));
    assert!(element.cards[0].3.contains("<script>\ndocument.querySelectorAll"));

    let ids: Vec<&str> = rendered.notes.iter().map(|note| note.id.as_str()).collect();
    assert_eq!(ids, ["alkali-metals", "halogens", "li", "water", "c", "fe", "au"]);
    for (rendered_note, note) in rendered.notes.iter().zip(&first.notes) {
        assert_eq!(note.guid, lock::guid(&lock.namespace, &rendered_note.notetype, &rendered_note.id));
        assert_eq!(note.modified, NOW);
    }
    let cards: Vec<(&str, Vec<u32>, &str)> =
        first.notes.iter().map(|note| (note.deck.as_str(), note.cards.clone(), note.tags.as_str())).collect();
    assert_eq!(
        cards,
        [
            ("Periodic Table::1 Groups 🧪", vec![0], "deck::groups"),
            ("Periodic Table::1 Groups 🧪", vec![0], "deck::groups"),
            ("Periodic Table::0 Elements", vec![0, 1], "deck::elements early"),
            ("Periodic Table::0 Elements", vec![0, 1], "common deck::elements"),
            ("Periodic Table::0 Elements", vec![0], "deck::elements early"),
            ("Periodic Table::0 Elements", vec![0, 1], "deck::elements late"),
            ("Periodic Table::0 Elements", vec![0, 1], "deck::elements late"),
        ]
    );
    assert!(first.notes.windows(2).all(|pair| pair[0].due < pair[1].due));
    assert_eq!(first.notes[4].fields[3], "<img src=\"diamond.png\">");

    let elements = lock.presets["elements"].id;
    assert!(first.presets.contains(&(elements, "Element drills".into(), 20, 0.92)));
    assert!(first.presets.contains(&(lock.presets["parent"].id, "parent".into(), 25, 0.9)));
    assert!(first.decks.contains(&("Periodic Table::0 Elements".into(), elements)));
    let png = std::fs::read(project.root.join("media/images/diamond.png")).unwrap();
    let mut expected: Vec<(String, Vec<u8>)> = rendered
        .jobs
        .iter()
        .filter(|job| job.kind == janky_cli::generate::Kind::Media)
        .map(|job| (job.file.clone(), common::fake_audio(&job.input_text()).into_bytes()))
        .collect();
    expected.push(("diamond.png".to_string(), png.clone()));
    expected.sort();
    let mut media = first.media.clone();
    media.sort();
    assert_eq!(media, expected);

    assert_eq!(
        (lock.notes["alkali-metals"].notetype.as_str(), lock.notes["alkali-metals"].deck.as_str()),
        ("group", "groups")
    );
    assert_eq!(lock.media["diamond.png"].hash, lock::file_hash(&png));

    let second = janky_anki::import(
        &apkg,
        &collection,
        janky_anki::ImportOptions {
            update_if_newer: true,
            merge_notetypes: true,
            with_deck_configs: true,
        },
    )
    .unwrap();
    assert_eq!((second.new, second.updated), (0, 0));
    assert_eq!(second.notes, first.notes);

    let mut again = lock.clone();
    let rebuilt = package::package(&project, &files, &rendered, &mut again, NOW + 1000, &mut diagnostics).unwrap();
    assert_eq!(again, lock);
    assert!(rebuilt.notes.iter().all(|note| note.modified == NOW));

    rendered.notes[2].fields["name"] = "but".into();
    let edited = package::package(&project, &files, &rendered, &mut again, NOW + 1000, &mut diagnostics).unwrap();
    let times: Vec<i64> = edited.notes.iter().map(|note| note.modified).collect();
    assert_eq!(times, [NOW, NOW, NOW + 1000, NOW, NOW, NOW, NOW]);
    janky_anki::write_apkg(&edited, &apkg).unwrap();
    let third = janky_anki::import(
        &apkg,
        &collection,
        janky_anki::ImportOptions {
            update_if_newer: true,
            merge_notetypes: true,
            with_deck_configs: true,
        },
    )
    .unwrap();
    assert_eq!((third.new, third.updated), (0, 1));
    assert_eq!(third.notes[2].fields[1], "but");
}

#[test]
fn shuffle_in_file_keeps_the_files_in_order() {
    let (_work, dir) = common::example();
    let deck = std::fs::read_to_string(dir.join("deck.toml")).unwrap();
    let deck = deck.replacen("preset = \"parent\"\n", "preset = \"parent\"\nnew_order = \"shuffle-in-file\"\n", 1);
    std::fs::write(dir.join("deck.toml"), deck).unwrap();
    let mut diagnostics = Diagnostics::default();
    let project = project::load(&dir.join("deck.toml"), &mut diagnostics).unwrap();
    let files = content::load(&project, &mut diagnostics);
    let rendered =
        render::render_with(&project, plugin::load(&project, &mut diagnostics).as_ref(), &files, &mut diagnostics);
    let mut lock = lock::read(&dir.join("deck.lock"), &mut diagnostics).unwrap();
    let package = package::package(&project, &files, &rendered, &mut lock, NOW, &mut diagnostics).unwrap();
    assert_eq!(diagnostics.to_json_lines(), "");

    let guid = |id: &str| {
        let note = rendered.notes.iter().find(|note| note.id == id).unwrap();
        lock::guid(&lock.namespace, &note.notetype, id)
    };
    let mut expected = Vec::new();
    for file in [&["alkali-metals", "halogens"][..], &["li", "water", "c"], &["fe", "au"]] {
        let mut guids: Vec<String> = file.iter().map(|id| guid(id)).collect();
        guids.sort_by_key(|guid| lock::content_hash([guid.as_str()]));
        expected.extend(guids);
    }
    let actual: Vec<&str> = package.notes.iter().map(|note| note.guid.as_str()).collect();
    assert_eq!(actual, expected);
    let in_file_order: Vec<String> = rendered.notes.iter().map(|note| guid(&note.id)).collect();
    assert_ne!(actual, in_file_order);
}

#[test]
fn a_preset_gets_the_options_of_the_preset_that_it_extends() {
    let (_work, dir) = common::example();
    let deck = std::fs::read_to_string(dir.join("deck.toml")).unwrap();
    let deck = deck.replacen("[presets.parent]\n", "[presets.parent]\nextends = \"elements\"\n", 1);
    std::fs::write(dir.join("deck.toml"), deck).unwrap();
    let mut diagnostics = Diagnostics::default();
    let project = project::load(&dir.join("deck.toml"), &mut diagnostics).unwrap();
    let files = content::load(&project, &mut diagnostics);
    let rendered =
        render::render_with(&project, plugin::load(&project, &mut diagnostics).as_ref(), &files, &mut diagnostics);
    let mut lock = lock::read(&dir.join("deck.lock"), &mut diagnostics).unwrap();
    let hash = lock.presets["parent"].hash.clone();
    let package = package::package(&project, &files, &rendered, &mut lock, NOW, &mut diagnostics).unwrap();
    assert_eq!(diagnostics.to_json_lines(), "");

    let parent = package.presets.iter().find(|preset| preset.id == lock.presets["parent"].id).unwrap();
    assert_eq!(parent.name, "parent");
    assert_eq!(parent.options.new_per_day, Some(25));
    assert_eq!(parent.options.desired_retention, Some(0.92));
    assert_eq!(parent.options.learning_steps, Some(vec![1.0, 10.0]));
    assert_ne!(lock.presets["parent"].hash, hash);
}

#[test]
fn a_preset_that_no_deck_names_is_not_exported() {
    let (_work, dir) = common::example();
    let mut deck = std::fs::read_to_string(dir.join("deck.toml")).unwrap();
    deck.push_str("\n[presets.base]\nnew_per_day = 5\n");
    std::fs::write(dir.join("deck.toml"), deck).unwrap();
    let mut diagnostics = Diagnostics::default();
    let project = project::load(&dir.join("deck.toml"), &mut diagnostics).unwrap();
    let files = content::load(&project, &mut diagnostics);
    let rendered =
        render::render_with(&project, plugin::load(&project, &mut diagnostics).as_ref(), &files, &mut diagnostics);
    let mut lock = lock::read(&dir.join("deck.lock"), &mut diagnostics).unwrap();
    let package = package::package(&project, &files, &rendered, &mut lock, NOW, &mut diagnostics).unwrap();
    assert_eq!(diagnostics.to_json_lines(), "");

    assert_eq!(lock.presets.keys().collect::<Vec<_>>(), ["parent", "elements"]);
    let ids: Vec<i64> = package.presets.iter().map(|preset| preset.id).collect();
    assert_eq!(ids, [lock.presets["parent"].id, lock.presets["elements"].id]);
}
