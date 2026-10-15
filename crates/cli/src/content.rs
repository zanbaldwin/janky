//! Finds the content files of each deck, checks them, and maps each entry to its note type.
//!
//! The `content` patterns of a deck are globs relative to the project directory. A file can
//! belong to one deck only. [`load`] reads each file (YAML, TOML or JSON) and checks its shape:
//! an object with `entries` and optional `defaults`, and an `id` on each entry. Then it copies
//! the `defaults` into each entry that does not set the key, and checks the file against the
//! content schema from [`crate::schema::file_schema`].
//!
//! The note type of an entry comes from its `type` (after the merge with `defaults`), then from
//! the note type with `default = true`. Entry ids are unique across all content files.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::config::deck::Deck;
use crate::diagnostic::{Diagnostic, Diagnostics, Source};
use crate::project::Project;
use crate::schema;
use crate::tree::{Tree, child};

/// A content file that has the correct shape, with the entries that passed every check.
#[derive(Debug)]
pub struct ContentFile {
    /// The path relative to the project directory, with `/` between the parts.
    pub path: String,
    /// The keys of the child decks from the root deck down to the deck of this file. Empty for the
    /// root deck.
    pub deck: Vec<String>,
    /// The index of the first pattern in the `content` of its deck that matches this file.
    pub pattern: usize,
    /// The parsed file, with `defaults` merged into each entry. The spans of a copied value point
    /// into `defaults`.
    pub tree: Tree,
    /// The entries without errors, in file order. An entry with an error, a duplicate id or no note
    /// type is left out.
    pub entries: Vec<Entry>,
}

/// An entry of a [`ContentFile`] that passed every check.
#[derive(Debug)]
pub struct Entry {
    /// The JSON Pointer of the entry in [`ContentFile::tree`], such as `/entries/0`.
    pub pointer: String,
    /// The `id` of the entry. It is unique across all content files.
    pub id: String,
    /// The key of the note type in `[notetypes]`.
    pub notetype: String,
}

struct Match {
    file: PathBuf,
    deck: Vec<String>,
    deck_name: String,
    pattern: String,
    index: usize,
}

/// Loads every content file of every deck, in the order of their paths.
///
/// It pushes each problem to `diagnostics` and continues. It leaves out a file that does not
/// parse or does not have the correct shape. If an error is in `defaults`, the file gives no
/// entries, and each such error shows once, not once for each entry.
pub fn load(project: &Project, diagnostics: &mut Diagnostics) -> Vec<ContentFile> {
    let mut found = Diagnostics::default();
    let mut matches = BTreeMap::new();
    find_files(
        project,
        &project.deck.deck.deck,
        &mut Vec::new(),
        &project.deck.deck.deck.name,
        "/deck",
        &mut matches,
        &mut found,
    );
    diagnostics.append_sorted(found);

    let shape = jsonschema::validator_for(&json!({
        "type": "object",
        "required": ["entries"],
        "additionalProperties": false,
        "properties": {
            "defaults": { "type": "object", "properties": { "type": { "type": "string" } } },
            "entries": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id"],
                    "properties": { "id": { "type": "string" }, "type": { "type": "string" } },
                },
            },
        },
    }))
    .expect("schema compiles");
    let schema = match jsonschema::validator_for(&schema::file_schema(project)) {
        Ok(schema) => Some(schema),
        Err(error) => {
            diagnostics
                .push(Diagnostic::error("content::schema", format!("cannot compile the content schema: {error}")));
            None
        },
    };
    let mut types = HashMap::new();
    let mut default = None;
    for (key, reference) in &project.deck.notetypes {
        for entry_type in reference.entry_types(key) {
            types.entry(entry_type).or_insert(key.as_str());
        }
        if reference.default && default.is_none() {
            default = Some(key.as_str());
        }
    }

    let mut loader = Loader {
        project,
        shape,
        schema,
        types,
        default,
        ids: HashMap::new(),
    };
    let mut files = Vec::new();
    for (path, found_match) in matches {
        let mut found = Diagnostics::default();
        if let Some(file) = loader.load_file(&path, found_match, &mut found) {
            files.push(file);
        }
        diagnostics.append_sorted(found);
    }
    files
}

struct Loader<'a> {
    project: &'a Project,
    shape: jsonschema::Validator,
    schema: Option<jsonschema::Validator>,
    types: HashMap<String, &'a str>,
    default: Option<&'a str>,
    ids: HashMap<String, String>,
}

fn find_files(
    project: &Project,
    deck: &Deck,
    keys: &mut Vec<String>,
    deck_name: &str,
    pointer: &str,
    matches: &mut BTreeMap<String, Match>,
    found: &mut Diagnostics,
) {
    let root = glob::Pattern::escape(&project.root.to_string_lossy());
    for (index, pattern) in deck.content.iter().enumerate() {
        let pattern_pointer = format!("{}/{index}", child(pointer, "content"));
        let full = if root.is_empty() { pattern.clone() } else { format!("{root}/{pattern}") };
        let paths = match glob::glob(&full) {
            Ok(paths) => paths,
            Err(error) => {
                found.push(deck_error(
                    project,
                    "content::pattern",
                    &pattern_pointer,
                    format!("invalid pattern: {}", error.msg),
                ));
                continue;
            },
        };
        for path in paths.flatten().filter(|path| path.is_file()) {
            let relative = relative_path(&project.root, &path);
            match matches.get(&relative) {
                Some(existing) if existing.deck != *keys => found.push(deck_error(
                    project,
                    "content::two-decks",
                    &pattern_pointer,
                    format!("`{relative}` already belongs to deck `{}`", existing.deck_name),
                )),
                Some(_) => {},
                None => {
                    matches.insert(
                        relative,
                        Match {
                            file: path,
                            deck: keys.clone(),
                            deck_name: deck_name.to_string(),
                            pattern: pattern_pointer.clone(),
                            index,
                        },
                    );
                },
            }
        }
    }
    let children = child(pointer, "children");
    for (key, deck_child) in &deck.children {
        keys.push(key.clone());
        let name = format!("{deck_name}::{}", deck_child.name);
        find_files(project, deck_child, keys, &name, &child(&children, key), matches, found);
        keys.pop();
    }
}

impl Loader<'_> {
    fn load_file(&mut self, path: &str, found_match: Match, found: &mut Diagnostics) -> Option<ContentFile> {
        let extension = Path::new(path).extension().and_then(|extension| extension.to_str()).unwrap_or_default();
        if !matches!(extension, "yaml" | "yml" | "toml" | "json") {
            found.push(deck_error(
                self.project,
                "content::format",
                &found_match.pattern,
                format!("`{path}` is not a YAML, TOML or JSON file"),
            ));
            return None;
        }
        let text = match std::fs::read_to_string(&found_match.file) {
            Ok(text) => text,
            Err(error) => {
                found.push(deck_error(
                    self.project,
                    "content::read",
                    &found_match.pattern,
                    format!("cannot read `{path}`: {error}"),
                ));
                return None;
            },
        };
        let source = Source::new(path, text);
        let mut tree = match extension {
            "toml" => Tree::parse_toml(source, found),
            "json" => Tree::parse_json(source, found),
            _ => Tree::parse_yaml(source, found),
        }?;
        let errors = tree.schema_diagnostics(&self.shape, "content");
        if !errors.is_empty() {
            for diagnostic in errors.iter() {
                found.push(diagnostic.clone());
            }
            return None;
        }
        merge_defaults(&mut tree);
        let mut invalid = Vec::new();
        let mut invalid_defaults = false;
        if let Some(schema) = &self.schema {
            let defaults = tree.loc("/defaults").map(|loc| loc.value.clone());
            let mut seen = HashSet::new();
            for diagnostic in tree.schema_diagnostics(schema, "content").iter() {
                let span = diagnostic.labels.first().map(|label| label.span.clone());
                let copied = span
                    .as_ref()
                    .zip(defaults.as_ref())
                    .is_some_and(|(span, defaults)| defaults.start <= span.start && span.end <= defaults.end);
                invalid_defaults |= copied;
                invalid.extend(span.clone());
                if !copied || seen.insert((diagnostic.code.clone(), span)) {
                    found.push(diagnostic.clone());
                }
            }
        }

        let mut entries = Vec::new();
        let count = tree.value["entries"].as_array().map_or(0, Vec::len);
        for index in 0..count {
            let pointer = format!("/entries/{index}");
            let entry = &tree.value["entries"][index];
            let id = entry["id"].as_str().unwrap_or_default().to_string();
            let notetype = match entry.get("type").and_then(Value::as_str) {
                Some(entry_type) => self.types.get(entry_type).map(|notetype| notetype.to_string()),
                None => self.default.map(str::to_string),
            };
            let id_pointer = child(&pointer, "id");
            let here = tree.loc(&id_pointer).and_then(|loc| tree.source.location(loc.value.start).ok());
            let place = match here {
                Some(location) => format!("{path}:{}:{}", location.line, location.column),
                None => path.to_string(),
            };
            if let Some(first) = self.ids.get(&id) {
                found.push(error_at(
                    &tree,
                    "content::duplicate-id",
                    &id_pointer,
                    format!("entry id `{id}` is already used at `{first}`"),
                ));
                continue;
            }
            self.ids.insert(id.clone(), place);
            let span = tree.loc(&pointer).map(|loc| loc.value.clone()).unwrap_or_default();
            let valid =
                !invalid_defaults && !invalid.iter().any(|error| span.start <= error.start && error.end <= span.end);
            if let Some(notetype) = notetype
                && valid
            {
                entries.push(Entry { pointer, id, notetype });
            }
        }
        Some(ContentFile {
            path: path.to_string(),
            deck: found_match.deck,
            pattern: found_match.index,
            tree,
            entries,
        })
    }
}

fn merge_defaults(tree: &mut Tree) {
    let Some(defaults) = tree.value.get("defaults").and_then(Value::as_object).cloned() else {
        return;
    };
    let Some(entries) = tree.value.get_mut("entries").and_then(Value::as_array_mut) else {
        return;
    };
    let mut copies = Vec::new();
    for (index, entry) in entries.iter_mut().enumerate() {
        let Some(object) = entry.as_object_mut() else { continue };
        for (key, value) in &defaults {
            if object.contains_key(key) {
                continue;
            }
            object.insert(key.clone(), value.clone());
            let from = child("/defaults", key);
            let to = child(&format!("/entries/{index}"), key);
            for (pointer, loc) in &tree.spans {
                if pointer == &from || pointer.starts_with(&format!("{from}/")) {
                    copies.push((format!("{to}{}", &pointer[from.len()..]), loc.clone()));
                }
            }
        }
    }
    tree.spans.extend(copies);
}

fn relative_path(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative.components().map(|component| component.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

fn deck_error(project: &Project, code: &str, pointer: &str, message: String) -> Diagnostic {
    error_at(&project.deck_tree, code, pointer, message)
}

fn error_at(tree: &Tree, code: &str, pointer: &str, message: String) -> Diagnostic {
    let (loc, _) = tree.nearest(pointer);
    Diagnostic::error(code, message).with_source(tree.source.clone()).with_label(loc.value.clone(), "here")
}
