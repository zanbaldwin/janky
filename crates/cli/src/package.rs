//! Turns the project, the rendered output and the lock file into a [`janky_anki::Package`].
//!
//! This is the last stage before [`janky_anki::write_apkg`]. It takes every Anki id from the
//! lock file, and converts the preset options of `deck.toml` to Anki's units (steps in
//! minutes, the maximum interval in days). It also computes the hashes that decide the
//! modification times of note types and notes (see [`lock::content_hash`]).
//!
//! Each card type with `when` gets a collapsed flag field `_make_<card key>` after the
//! fields of its note type. Each note gets a deck tag in addition to its `tags`: `deck` for the
//! root deck, or the deck key path after `deck::` (for example `deck::lessons::basics`).
//!
//! Anki gives new cards their position in the order of the notes. The notes follow the decks in
//! the order of `deck.toml` (a parent before its children), then the `content` patterns of each
//! deck, then the paths of the content files. Inside a file, they keep the entry order, or with
//! `new_order = "shuffle-in-file"` they are sorted by a hash of their GUID.

use std::collections::HashMap;

use indexmap::IndexMap;
use janky_anki::{Card, Deck as AnkiDeck, Field, Media, Note, Notetype, Package, Preset, PresetOptions};
use serde_json::Value;

use crate::config::deck::{Deck, NewOrder, Preset as PresetConfig};
use crate::content::ContentFile;
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::lock::{self, LockFile, LockedDeck, LockedMedia, Stamp};
use crate::project::Project;
use crate::render::Rendered;
use crate::tree::{Tree, child};

const MAX_INTERVAL_DAYS: u32 = 36_500;

/// Builds the package, and updates `lock` to match it.
///
/// The function first gives every key of the project an id in `lock` (see
/// [`LockFile::update`]). Then it stamps the note types and notes with their hashes and
/// modification times, and replaces the media table. A changed hash gets the time `now`
/// (seconds since the Unix epoch), at least the old time plus one.
///
/// It returns `None` in two cases:
///
/// - A preset option is out of range: a maximum interval over 36500 days, or a number of FSRS
///   parameters other than 17, 19 or 21. Each problem gives a `config::preset` error.
/// - A media file cannot be read. This gives a `build::media` error.
///
/// After `None`, `lock` can already have changes, so the caller must not write it.
pub fn package(
    project: &Project,
    files: &[ContentFile],
    rendered: &Rendered,
    lock: &mut LockFile,
    now: i64,
    diagnostics: &mut Diagnostics,
) -> Option<Package> {
    package_with(project, files, rendered, lock, now, true, diagnostics)
}

/// [`package`], but without `hash_media` it does not read the media files. The media table of `lock` then stays as
/// it is, and an unreadable media file is no error. `janky preview` uses this, because it runs on each page load and
/// writes no lock file.
pub fn package_with(
    project: &Project,
    files: &[ContentFile],
    rendered: &Rendered,
    lock: &mut LockFile,
    now: i64,
    hash_media: bool,
    diagnostics: &mut Diagnostics,
) -> Option<Package> {
    lock.update(project);
    let mut found = Diagnostics::default();
    let presets: Vec<Preset> = project
        .deck
        .used_presets()
        .into_iter()
        .map(|key| {
            let preset = &project.deck.presets[key];
            Preset {
                id: lock.presets[key].id,
                name: preset.name.clone().unwrap_or_else(|| key.clone()),
                options: options(preset, &project.deck_tree, &child("/presets", key), &mut found),
            }
        })
        .collect();
    let failed = found.has_errors();
    diagnostics.append_sorted(found);
    if failed {
        return None;
    }

    let mut decks = Vec::new();
    let root = lock.deck.as_ref().expect("synced deck");
    collect_decks(&project.deck.deck.deck, root, &project.deck.deck.deck.name, lock, &mut decks);

    let mut notetypes = Vec::new();
    let mut field_counts = HashMap::new();
    for (key, notetype) in &project.notetypes {
        let locked = &lock.notetypes[key];
        let rendered_notetype = &rendered.notetypes[key];
        let mut fields: Vec<Field> = notetype
            .file
            .fields
            .iter()
            .map(|(field_key, field)| Field {
                id: locked.fields[field_key].id,
                name: field.name.clone(),
                description: field.description.clone().unwrap_or_default(),
                collapsed: false,
            })
            .collect();
        for (card_key, card) in &notetype.file.cards {
            if card.when.is_some() {
                let name = format!("_make_{card_key}");
                fields.push(Field {
                    id: locked.fields[&name].id,
                    name,
                    description: String::new(),
                    collapsed: true,
                });
            }
        }
        let cards: Vec<Card> = notetype
            .file
            .cards
            .iter()
            .map(|(card_key, card)| Card {
                id: locked.cards[card_key].id,
                name: card.name.clone(),
                front: rendered_notetype.cards[card_key].front.clone(),
                back: rendered_notetype.cards[card_key].back.clone(),
            })
            .collect();
        let sort_field = notetype
            .file
            .sort_field
            .as_ref()
            .and_then(|sort_field| notetype.file.fields.get_index_of(sort_field))
            .unwrap_or(0) as u32;
        let id = locked.id;
        let mut parts = vec![
            notetype.file.name.clone(),
            rendered_notetype.css.clone(),
            sort_field.to_string(),
        ];
        for field in &fields {
            parts.extend([
                field.id.to_string(),
                field.name.clone(),
                field.description.clone(),
                field.collapsed.to_string(),
            ]);
        }
        for card in &cards {
            parts.extend([
                card.id.to_string(),
                card.name.clone(),
                card.front.clone(),
                card.back.clone(),
            ]);
        }
        let hash = lock::content_hash(parts.iter().map(String::as_str));
        let modified = lock.stamp_notetype(key, hash, now);
        field_counts.insert(key.clone(), fields.len());
        notetypes.push(Notetype {
            id,
            name: notetype.file.name.clone(),
            css: rendered_notetype.css.clone(),
            sort_field,
            modified,
            fields,
            cards,
        });
    }

    let ranks = file_ranks(project, files);
    let mut origins = HashMap::new();
    for (index, file) in files.iter().enumerate() {
        for entry in &file.entries {
            let tags = file
                .tree
                .value
                .pointer(&format!("{}/tags", entry.pointer))
                .and_then(Value::as_array)
                .map(|tags| tags.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_else(Vec::new);
            origins.insert(entry.id.as_str(), (ranks[index], file.deck.clone(), tags));
        }
    }
    let mut notes = Vec::new();
    let mut note_files = Vec::new();
    let mut hashes = Vec::new();
    for note in &rendered.notes {
        let (file, keys, mut tags) = origins[note.id.as_str()].clone();
        note_files.push(file);
        tags.push(if keys.is_empty() { "deck".to_string() } else { format!("deck::{}", keys.join("::")) });
        let notetype = lock.notetypes[&note.notetype].id;
        let fields: Vec<String> = note.fields.values().cloned().collect();
        let notetype_id = notetype.to_string();
        let hash = lock::content_hash(
            [notetype_id.as_str()]
                .into_iter()
                .chain(fields.iter().map(String::as_str))
                .chain(tags.iter().map(String::as_str)),
        );
        hashes.push(Stamp {
            id: note.id.clone(),
            hash,
            notetype: note.notetype.clone(),
            deck: keys.join("."),
        });
        notes.push(Note {
            guid: lock::guid(&lock.namespace, &note.notetype, &note.id),
            notetype,
            deck: deck_id(lock.deck.as_ref().expect("synced deck"), &keys),
            fields,
            tags,
            modified: 0,
        });
    }
    for (note, modified) in notes.iter_mut().zip(lock.stamp_notes(hashes, now)) {
        note.modified = modified;
    }
    let shuffle = project.deck.deck.new_order == NewOrder::ShuffleInFile;
    let mut keyed: Vec<_> = note_files
        .into_iter()
        .zip(notes)
        .map(|(file, note)| (file, shuffle.then(|| lock::content_hash([note.guid.as_str()])), note))
        .collect();
    keyed.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    notes = keyed.into_iter().map(|(_, _, note)| note).collect();

    let mut media = Vec::new();
    let mut hashes = IndexMap::new();
    for file in &rendered.media {
        let path = project.root.join(&file.path);
        if hash_media {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    hashes.insert(file.name.clone(), LockedMedia { hash: lock::file_hash(&bytes) });
                },
                Err(error) => {
                    diagnostics
                        .push(Diagnostic::error("build::media", format!("cannot read `{}`: {error}", file.path)));
                    return None;
                },
            }
        }
        media.push(Media { name: file.name.clone(), path });
    }
    if hash_media {
        lock.media = hashes;
    }
    Some(Package { notetypes, presets, decks, notes, media })
}

fn collect_decks(deck: &Deck, locked: &LockedDeck, full_name: &str, lock: &LockFile, out: &mut Vec<AnkiDeck>) {
    let preset = deck.preset.as_ref().map(|key| lock.presets[key].id);
    out.push(AnkiDeck {
        id: locked.id,
        name: full_name.to_string(),
        preset,
    });
    for (key, child_deck) in &deck.children {
        let name = format!("{full_name}::{}", child_deck.name);
        collect_decks(child_deck, &locked.children[key], &name, lock, out);
    }
}

/// The rank of each content file in the order of the notes: the decks in the order of `deck.toml` (a parent before
/// its children), then the `content` patterns of the deck, then the path.
fn file_ranks(project: &Project, files: &[ContentFile]) -> Vec<usize> {
    let mut decks = HashMap::new();
    let mut stack = vec![(Vec::new(), &project.deck.deck.deck)];
    while let Some((keys, deck)) = stack.pop() {
        for (key, child) in deck.children.iter().rev() {
            stack.push(([keys.as_slice(), std::slice::from_ref(key)].concat(), child));
        }
        decks.insert(keys, decks.len());
    }
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by_key(|&index| {
        let file = &files[index];
        (decks.get(&file.deck).copied().unwrap_or(usize::MAX), file.pattern, &file.path)
    });
    let mut ranks = vec![0; files.len()];
    for (rank, index) in order.into_iter().enumerate() {
        ranks[index] = rank;
    }
    ranks
}

fn deck_id(root: &LockedDeck, keys: &[String]) -> i64 {
    keys.iter().fold(root, |deck, key| &deck.children[key]).id
}

fn options(preset: &PresetConfig, tree: &Tree, pointer: &str, found: &mut Diagnostics) -> PresetOptions {
    let mut error = |key: &str, message: String| {
        let (loc, _) = tree.nearest(&child(pointer, key));
        found.push(
            Diagnostic::error("config::preset", message)
                .with_source(tree.source.clone())
                .with_label(loc.value.clone(), "here"),
        );
    };
    let maximum_interval = preset.maximum_interval.as_deref().map(days).filter(|days| {
        let fits = *days <= MAX_INTERVAL_DAYS;
        if !fits {
            error(
                "maximum_interval",
                format!("the maximum interval is longer than Anki's limit of {MAX_INTERVAL_DAYS} days"),
            );
        }
        fits
    });
    let fsrs_params =
        preset.fsrs_params.as_ref().map(|params| params.iter().map(|value| *value as f32).collect::<Vec<_>>());
    if let Some(params) = &fsrs_params
        && ![17, 19, 21].contains(&params.len())
    {
        error("fsrs_params", format!("FSRS needs 17, 19 or 21 parameters, not {}", params.len()));
    }
    PresetOptions {
        new_per_day: preset.new_per_day,
        reviews_per_day: preset.reviews_per_day,
        learning_steps: preset.learning_steps.as_ref().map(|steps| steps.iter().map(|step| minutes(step)).collect()),
        relearning_steps: preset
            .relearning_steps
            .as_ref()
            .map(|steps| steps.iter().map(|step| minutes(step)).collect()),
        desired_retention: preset.desired_retention.map(|retention| retention as f32),
        maximum_interval,
        fsrs_params,
        new_gather_order: preset.new_gather_order,
        new_sort_order: preset.new_sort_order,
        review_sort_order: preset.review_sort_order,
        new_review_mix: preset.new_review_mix,
        interday_learning_mix: preset.interday_learning_mix,
        bury_new: preset.bury_new,
        bury_reviews: preset.bury_reviews,
        bury_interday_learning: preset.bury_interday_learning,
        leech_threshold: preset.leech_threshold,
        leech_action: preset.leech_action,
        autoplay: preset.autoplay,
        replay_question_audio: preset.replay_question_audio,
        show_timer: preset.show_timer,
        max_answer_seconds: preset.max_answer_seconds,
    }
}

fn split(duration: &str) -> (f32, char) {
    let unit = duration.chars().last().unwrap_or('d');
    (duration[..duration.len() - unit.len_utf8()].parse().unwrap_or(0.0), unit)
}

fn minutes(step: &str) -> f32 {
    let (value, unit) = split(step);
    match unit {
        's' => value / 60.0,
        'h' => value * 60.0,
        'd' => value * 1440.0,
        _ => value,
    }
}

fn days(interval: &str) -> u32 {
    let (value, unit) = split(interval);
    let days = match unit {
        'w' => value * 7.0,
        'y' => value * 365.0,
        _ => value,
    };
    days as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_convert_to_anki_units() {
        assert_eq!(minutes("30s"), 0.5);
        assert_eq!(minutes("10m"), 10.0);
        assert_eq!(minutes("1.5h"), 90.0);
        assert_eq!(minutes("1d"), 1440.0);
        assert_eq!(days("90d"), 90);
        assert_eq!(days("4w"), 28);
        assert_eq!(days("1y"), 365);
    }
}
