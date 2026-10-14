//! Loads `deck.toml` and its note types into a [`Project`], and checks how they fit together.
//!
//! [`load_with`] parses `deck.toml` and checks it against its schema through [`crate::config`].
//! Then it checks what the schema cannot: the syntax of keys, unique full deck names, references
//! to presets and fields, loops in preset `extends`, one default note type, unique entry types,
//! and the fields of each note type source. It merges each preset with the presets that its
//! `extends` chain names. Then it reads each note type file, from the project directory or from a
//! `git` or `url` source in the cache. It checks each note type file in the same way, and loads
//! the JSON Schema file that its `schema` names.
//!
//! Paths in `deck.toml` are relative to the project directory. Paths in a note type file are
//! relative to the directory of that file. Each path to an input file must name a regular file,
//! and it must stay inside its directory, also after symlinks are resolved. The `output` path
//! gets only a lexical check, and a missing media directory is a warning.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::Value;

use crate::config::deck::{Deck, DeckFile, NotetypeRef};
use crate::config::notetype::{FieldKind, FieldSpec, NotetypeFile, SchemaSource, ShortSchema};
use crate::config::{self};
use crate::diagnostic::{Diagnostic, Diagnostics, Severity, Source};
use crate::lock::{self, LockedSource};
use crate::source::{self, Reference};
use crate::tree::{Tree, child};

const KEY_HELP: &str = "a key starts with a lowercase letter and holds only lowercase letters, digits, `_` and `-`";

/// A loaded and checked `deck.toml`, with the note types that it names.
#[derive(Debug)]
pub struct Project {
    /// The directory of `deck.toml`. It is empty if the path to `deck.toml` has no directory part.
    pub root: PathBuf,
    /// The deserialised `deck.toml`.
    pub deck: DeckFile,
    /// The parsed `deck.toml` with its spans, for diagnostics.
    pub deck_tree: Tree,
    /// The note types that loaded, by their key in `[notetypes]`, in file order.
    pub notetypes: IndexMap<String, Notetype>,
    /// The resolved `git` and `url` sources, by note type key. `deck.lock` records them.
    pub sources: IndexMap<String, LockedSource>,
}

/// A loaded note type file, with the directory that its paths are relative to.
#[derive(Debug)]
pub struct Notetype {
    /// The directory on disk that holds the note type file. For a remote source, it is in the
    /// cache.
    pub dir: PathBuf,
    /// The name of [`dir`](Self::dir) in diagnostics. For a local note type, it is relative to the
    /// project directory. For a remote source, it starts with `<key>@` and the first 12 characters
    /// of the source id.
    pub dir_name: String,
    /// The deserialised note type file.
    pub file: NotetypeFile,
    /// The parsed note type file with its spans, for diagnostics.
    pub tree: Tree,
    /// The JSON Schema from the file that `schema` names. `None` if `schema` is inline or absent,
    /// if the note type file has errors, or if the schema file did not load.
    pub schema_file: Option<Value>,
}

/// How [`load_with`] gets remote note type sources.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Use only the cache. A source that is not in the cache is an error.
    pub offline: bool,
    /// Ignore the commits that `deck.lock` pins, and resolve each `git` reference again.
    pub update: bool,
}

/// [`load_with`] with the default [`Options`]: it uses the network, and the commits that
/// `deck.lock` pins.
pub fn load(deck_path: &Path, diagnostics: &mut Diagnostics) -> Option<Project> {
    load_with(deck_path, Options::default(), diagnostics)
}

/// Loads and checks `deck.toml` at `deck_path` and every note type that it names.
///
/// It pushes each problem to `diagnostics` and continues. It returns `None` if `diagnostics`
/// then holds an error, also an error that was there before the call. A missing media directory
/// and a `git://` source without a pinned commit are warnings.
///
/// It fetches remote sources into the cache, unless [`Options::offline`] is set. A `deck.lock`
/// that does not load is ignored. A note type with invalid source fields is not loaded.
pub fn load_with(deck_path: &Path, options: Options, diagnostics: &mut Diagnostics) -> Option<Project> {
    let root = deck_path.parent().unwrap_or(Path::new("")).to_path_buf();
    let name =
        deck_path.file_name().map_or_else(|| deck_path.display().to_string(), |name| name.to_string_lossy().into());
    let text = match std::fs::read_to_string(deck_path) {
        Ok(text) => text,
        Err(error) => {
            diagnostics
                .push(Diagnostic::error("project::read", format!("cannot read `{}`: {error}", deck_path.display())));
            return None;
        },
    };
    let (mut deck, tree) = config::load::<DeckFile>(Source::new(name, text), diagnostics)?;
    let mut found = Diagnostics::default();
    let invalid = check_deck_file(&deck, &tree, &root, &mut found);
    diagnostics.append_sorted(found);
    resolve_presets(&mut deck, &tree);

    let mut ignored = Diagnostics::default();
    let pinned = lock::read(&root.join("deck.lock"), &mut ignored).filter(|_| !options.update);
    let cache = source::cache_root();
    let mut notetypes = IndexMap::new();
    let mut sources = IndexMap::new();
    for (key, reference) in &deck.notetypes {
        let pointer = child("/notetypes", key);
        let path_pointer = child(&pointer, "path");
        if invalid.contains(key) {
            continue;
        }
        let (base, prefix) = match (&reference.git, &reference.url) {
            (Some(url), _) => {
                let wanted = git_reference(reference);
                let locked = pinned.as_ref().and_then(|lock| lock.notetypes.get(key)).and_then(|notetype| {
                    notetype
                        .source
                        .as_ref()
                        .filter(|source| source.git.as_ref() == Some(url) && same_reference(source, reference))
                });
                let commit = locked.and_then(|source| source.commit.as_deref());
                if commit.is_none() && url.starts_with("git://") {
                    let mut warning = error_at(
                        &tree,
                        "source::insecure",
                        &child(&pointer, "git"),
                        "`git://` has no encryption, so the fetched files can be changed on the network".to_string(),
                        "unencrypted source",
                    );
                    warning.severity = Severity::Warning;
                    diagnostics.push(warning.with_help(
                        "check the commit that deck.lock records after the build. Later builds use that commit",
                    ));
                }
                match source::git(url, wanted, commit, &cache, options.offline) {
                    Ok(checkout) => {
                        let prefix = format!("{key}@{}", &checkout.id[..12]);
                        sources.insert(
                            key.clone(),
                            LockedSource {
                                git: Some(url.clone()),
                                tag: reference.tag.clone(),
                                rev: reference.rev.clone(),
                                branch: reference.branch.clone(),
                                commit: Some(checkout.id),
                                url: None,
                                sha256: None,
                            },
                        );
                        (checkout.dir, prefix)
                    },
                    Err(message) => {
                        diagnostics.push(error_at(&tree, "source::git", &child(&pointer, "git"), message, "here"));
                        continue;
                    },
                }
            },
            (None, Some(url)) => {
                let sha256 = reference.sha256.clone().unwrap_or_default();
                match source::archive(url, &sha256, &cache, options.offline) {
                    Ok(checkout) => {
                        let prefix = format!("{key}@{}", &checkout.id[..12]);
                        sources.insert(
                            key.clone(),
                            LockedSource {
                                git: None,
                                tag: None,
                                rev: None,
                                branch: None,
                                commit: None,
                                url: Some(url.clone()),
                                sha256: Some(sha256),
                            },
                        );
                        (checkout.dir, prefix)
                    },
                    Err(message) => {
                        diagnostics.push(error_at(&tree, "source::archive", &child(&pointer, "url"), message, "here"));
                        continue;
                    },
                }
            },
            (None, None) => (root.clone(), String::new()),
        };
        let trimmed = reference.path.as_deref().unwrap_or_default().trim_end_matches('/');
        let inner = if base.join(trimmed).is_dir() { join_name(trimmed, "notetype.toml") } else { trimmed.to_string() };
        let inner_dir = Path::new(&inner).parent().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = base.join(&inner_dir);
        let dir_name = join_name(&prefix, &inner_dir).trim_end_matches('/').to_string();
        let file_name = join_name(&prefix, &inner);
        let Ok(text) = std::fs::read_to_string(base.join(&inner)) else {
            diagnostics.push(error_at(
                &tree,
                "config::missing-notetype",
                &path_pointer,
                format!("`{file_name}` does not exist"),
                "no note type file here",
            ));
            continue;
        };
        if let Some((file, notetype_tree)) = config::load::<NotetypeFile>(Source::new(file_name, text), diagnostics) {
            let mut found = Diagnostics::default();
            check_notetype_file(&file, &notetype_tree, &dir, &mut found);
            let failed = found.has_errors();
            diagnostics.append_sorted(found);
            let schema_file = match &file.schema {
                Some(SchemaSource::File(path)) if !failed => {
                    let name = join_name(&dir_name, path);
                    load_schema_file(&dir.join(path), name, diagnostics)
                },
                _ => None,
            };
            notetypes.insert(
                key.clone(),
                Notetype {
                    dir,
                    dir_name,
                    file,
                    tree: notetype_tree,
                    schema_file,
                },
            );
        }
    }

    (!diagnostics.has_errors()).then_some(Project {
        root,
        deck,
        deck_tree: tree,
        notetypes,
        sources,
    })
}

fn git_reference(reference: &NotetypeRef) -> Reference<'_> {
    match (&reference.tag, &reference.branch, &reference.rev) {
        (Some(tag), _, _) => Reference::Tag(tag),
        (None, Some(branch), _) => Reference::Branch(branch),
        (None, None, rev) => Reference::Rev(rev.as_deref().unwrap_or("HEAD")),
    }
}

fn same_reference(source: &LockedSource, reference: &NotetypeRef) -> bool {
    source.tag == reference.tag && source.branch == reference.branch && source.rev == reference.rev
}

fn remote(reference: &NotetypeRef) -> Option<(&'static str, &String)> {
    match (&reference.git, &reference.url) {
        (Some(git), _) => Some(("git", git)),
        (None, Some(url)) => Some(("url", url)),
        (None, None) => None,
    }
}

fn check_source(key: &str, reference: &NotetypeRef, tree: &Tree, diagnostics: &mut Diagnostics) -> bool {
    let before = diagnostics.iter().count();
    let pointer = child("/notetypes", key);
    let mut error = |at: &str, message: String, label: &str| {
        diagnostics.push(error_at(tree, "config::source", &child(&pointer, at), message, label));
    };
    let refs = [
        ("tag", &reference.tag),
        ("rev", &reference.rev),
        ("branch", &reference.branch),
    ];
    let given: Vec<&str> = refs.iter().filter(|(_, value)| value.is_some()).map(|(name, _)| *name).collect();
    match (&reference.git, &reference.url) {
        (Some(_), Some(_)) => error("url", "a note type has `git` or `url`, not both".into(), "second source"),
        (Some(git), None) => {
            if !(git.starts_with("https://") || git.starts_with("git://")) {
                error("git", "a `git` source must use `https://` or `git://`".into(), "unsupported URL");
            }
            if given.is_empty() {
                error("git", "a `git` source needs one of `tag`, `rev` or `branch`".into(), "no reference");
            }
            for extra in given.iter().skip(1) {
                error(extra, format!("`{}` and `{extra}` cannot be used together", given[0]), "second reference");
            }
            if reference.sha256.is_some() {
                error("sha256", "`sha256` is only for `url` sources".into(), "not for `git`");
            }
        },
        (None, Some(url)) => {
            if reference.sha256.is_none() {
                error("url", "a `url` source needs `sha256`".into(), "no `sha256`");
            }
            if source::archive_kind(url).is_none() {
                error("url", "a `url` source must end in `.tar.gz`, `.tgz` or `.zip`".into(), "unknown archive format");
            }
            if !secure(url) {
                error("url", "a `url` source must use `https://`".into(), "plain HTTP is only allowed on loopback");
            }
            for name in &given {
                error(name, format!("`{name}` is only for `git` sources"), "not for `url`");
            }
        },
        (None, None) => {
            if reference.path.is_none() {
                error("", "a local note type needs `path`".into(), "no `path`");
            }
            for name in &given {
                error(name, format!("`{name}` is only for `git` sources"), "not for a local note type");
            }
            if reference.sha256.is_some() {
                error("sha256", "`sha256` is only for `url` sources".into(), "not for a local note type");
            }
        },
    }
    if remote(reference).is_some()
        && let Some(path) = &reference.path
        && inside_path(path).is_none()
    {
        error("path", format!("`{path}` is outside the source"), "the path must stay inside the source");
    }
    diagnostics.iter().count() == before
}

fn secure(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return url.starts_with("https://");
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host
        .rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
        .map_or(host, |(host, _)| host);
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Why `endpoint` cannot be the sync server of `[publish]`: the message and the label.
fn endpoint_problem(endpoint: &str) -> Option<(String, &'static str)> {
    match url::Url::parse(endpoint) {
        Err(error) => Some((format!("the sync server address is not a valid URL: {error}"), "not a URL")),
        Ok(url) if !url.username().is_empty() || url.password().is_some() => Some((
            "the sync server address cannot hold a user name or a password".into(),
            "use `username` and `password`",
        )),
        Ok(_) if !secure(endpoint) => {
            Some((format!("`{endpoint}` must use `https://`"), "plain HTTP is only allowed on loopback"))
        },
        Ok(url) if !url.path().ends_with('/') => Some((
            format!("`{endpoint}` must end with `/`, or Anki drops the last part of the path"),
            "add `/` at the end",
        )),
        Ok(_) => None,
    }
}

fn check_deck_file(deck: &DeckFile, tree: &Tree, root: &Path, diagnostics: &mut Diagnostics) -> HashSet<String> {
    let mut invalid = HashSet::new();
    for (key, reference) in &deck.notetypes {
        if !check_source(key, reference, tree, diagnostics) {
            invalid.insert(key.clone());
        }
    }
    if let Some(output) = &deck.deck.output
        && inside_path(output).is_none()
    {
        diagnostics.push(error_at(
            tree,
            "config::outside-project",
            "/deck/output",
            format!("`{output}` is outside the project directory"),
            "the package must be written inside the project directory",
        ));
    }
    for (index, dir) in deck.deck.media_dirs.iter().enumerate() {
        if !root.join(dir).is_dir() {
            let mut diagnostic = error_at(
                tree,
                "config::missing-media-dir",
                &format!("/deck/media_dirs/{index}"),
                format!("media directory `{dir}` does not exist"),
                "no such directory",
            );
            diagnostic.severity = Severity::Warning;
            diagnostics.push(diagnostic);
        }
    }
    check_keys(tree, "/presets", deck.presets.keys(), diagnostics);
    for (key, preset) in &deck.presets {
        let Some(base) = &preset.extends else {
            continue;
        };
        let pointer = child(&child("/presets", key), "extends");
        if !deck.presets.contains_key(base) {
            diagnostics.push(error_at(
                tree,
                "config::unknown-preset",
                &pointer,
                format!("preset `{base}` does not exist"),
                "no `[presets]` table has this key",
            ));
        } else if let Some(last) = deck.preset_chain(key).last()
            && deck.presets[*last].extends.as_deref() == Some(key)
        {
            diagnostics.push(error_at(
                tree,
                "config::preset-loop",
                &pointer,
                format!("the `extends` chain of preset `{key}` comes back to `{key}`"),
                "loop",
            ));
        }
    }
    check_keys(tree, "/notetypes", deck.notetypes.keys(), diagnostics);
    check_keys(tree, "/plugins", deck.plugins.keys(), diagnostics);
    for (key, plugin) in &deck.plugins {
        check_path(tree, root, Place::Project, &child(&child("/plugins", key), "path"), &plugin.path, diagnostics);
    }
    if let Some(publish) = &deck.publish
        && let Some((message, label)) = endpoint_problem(&publish.endpoint)
    {
        diagnostics.push(error_at(tree, "config::endpoint", "/publish/endpoint", message, label));
    }
    let mut full_names = HashSet::new();
    check_deck(deck, tree, &deck.deck.deck, "/deck", &deck.deck.deck.name, &mut full_names, diagnostics);

    let mut default = None;
    let mut types: HashMap<String, &str> = HashMap::new();
    for (key, reference) in &deck.notetypes {
        let pointer = child("/notetypes", key);
        if reference.default {
            match default {
                None => default = Some(key),
                Some(first) => diagnostics.push(error_at(
                    tree,
                    "config::duplicate-default",
                    &child(&pointer, "default"),
                    format!("note type `{first}` is already the default"),
                    "second default",
                )),
            }
        }
        let alias = child(&pointer, "alias");
        let mut names = vec![(key.clone(), None)];
        for (index, name) in reference.alias.names.iter().enumerate() {
            let at = if reference.alias.is_list { format!("{alias}/{index}") } else { alias.clone() };
            names.push((name.clone(), Some(at)));
        }
        for (name, at) in names {
            match types.get(&name) {
                Some(owner) if owner == key => {},
                Some(owner) => {
                    let message = format!("entry type `{name}` already belongs to note type `{owner}`");
                    let diagnostic = match &at {
                        Some(at) => error_at(tree, "config::duplicate-type", at, message, "duplicate entry type"),
                        None => {
                            let loc = tree.loc(&pointer).expect("note type span");
                            Diagnostic::error("config::duplicate-type", message)
                                .with_source(tree.source.clone())
                                .with_label(
                                    loc.key.clone().unwrap_or_else(|| loc.value.clone()),
                                    "duplicate entry type",
                                )
                        },
                    };
                    diagnostics.push(diagnostic);
                },
                None => {
                    types.insert(name, key);
                },
            }
        }
    }
    invalid
}

/// Replaces each preset with a merge of its own table and the tables of its `extends` chain.
/// A key that is nearer in the chain wins. `name` and `extends` come only from the preset
/// itself. The tables passed the schema check, so the merge cannot fail.
fn resolve_presets(file: &mut DeckFile, tree: &Tree) {
    let mut resolved = Vec::new();
    for key in file.presets.keys() {
        let mut table = serde_json::Map::new();
        for current in file.preset_chain(key) {
            if let Some(Value::Object(values)) = tree.value.pointer(&child("/presets", current)) {
                for (name, value) in values {
                    if current == key || (name != "name" && name != "extends") {
                        table.entry(name.clone()).or_insert_with(|| value.clone());
                    }
                }
            }
        }
        if let Ok(preset) = serde_json::from_value(Value::Object(table)) {
            resolved.push((key.clone(), preset));
        }
    }
    for (key, preset) in resolved {
        file.presets[&key] = preset;
    }
}

fn check_deck(
    file: &DeckFile,
    tree: &Tree,
    deck: &Deck,
    pointer: &str,
    full_name: &str,
    full_names: &mut HashSet<String>,
    diagnostics: &mut Diagnostics,
) {
    if !full_names.insert(full_name.to_string()) {
        diagnostics.push(error_at(
            tree,
            "config::duplicate-deck",
            &child(pointer, "name"),
            format!("two decks have the full name `{full_name}`"),
            "duplicate deck name",
        ));
    }
    if let Some(preset) = &deck.preset
        && !file.presets.contains_key(preset)
    {
        diagnostics.push(error_at(
            tree,
            "config::unknown-preset",
            &child(pointer, "preset"),
            format!("preset `{preset}` does not exist"),
            "no `[presets]` table has this key",
        ));
    }
    let children = child(pointer, "children");
    check_keys(tree, &children, deck.children.keys(), diagnostics);
    for (key, deck_child) in &deck.children {
        check_deck(
            file,
            tree,
            deck_child,
            &child(&children, key),
            &format!("{full_name}::{}", deck_child.name),
            full_names,
            diagnostics,
        );
    }
}

fn check_notetype_file(file: &NotetypeFile, tree: &Tree, dir: &Path, diagnostics: &mut Diagnostics) {
    check_keys(tree, "/fields", file.fields.keys(), diagnostics);
    check_keys(tree, "/cards", file.cards.keys(), diagnostics);
    if let Some(sort_field) = &file.sort_field
        && !file.fields.contains_key(sort_field)
    {
        diagnostics.push(error_at(
            tree,
            "config::unknown-field",
            "/sort_field",
            format!("field `{sort_field}` does not exist"),
            "no `[fields]` table has this key",
        ));
    }

    if let Some(SchemaSource::Inline(schema)) = &file.schema {
        check_short_schema(schema, tree, diagnostics);
    }

    let mut paths = Vec::new();
    if let Some(css) = &file.css {
        paths.push(("/css".to_string(), css));
    }
    for (index, asset) in file.assets.iter().enumerate() {
        paths.push((format!("/assets/{index}"), asset));
    }
    for (index, plugin) in file.plugins.iter().enumerate() {
        paths.push((format!("/plugins/{index}"), plugin));
    }
    if let Some(SchemaSource::File(schema)) = &file.schema {
        paths.push(("/schema".to_string(), schema));
    }
    for (key, field) in &file.fields {
        let pointer = child("/fields", key);
        if let Some(template) = &field.template {
            if field.value.is_some() {
                diagnostics.push(error_at(
                    tree,
                    "config::value-and-template",
                    &child(&pointer, "template"),
                    format!("field `{key}` has both `value` and `template`"),
                    "remove `value` or `template`",
                ));
            }
            paths.push((child(&pointer, "template"), template));
        }
    }
    for (key, card) in &file.cards {
        let pointer = child("/cards", key);
        paths.push((child(&pointer, "front"), &card.front));
        paths.push((child(&pointer, "back"), &card.back));
    }
    for (pointer, path) in paths {
        check_path(tree, dir, Place::Notetype, &pointer, path, diagnostics);
    }
}

fn load_schema_file(path: &Path, name: String, diagnostics: &mut Diagnostics) -> Option<Value> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            diagnostics.push(Diagnostic::error("config::read", format!("cannot read `{name}`: {error}")));
            return None;
        },
    };
    let mut found = Diagnostics::default();
    let tree = Tree::parse_json(Source::new(name, text), &mut found);
    let failed = found.has_errors();
    diagnostics.append_sorted(found);
    let tree = tree.filter(|_| !failed)?;
    let (message, pointer) = match jsonschema::meta::validate(&tree.value) {
        Ok(()) if tree.value.is_object() => return Some(tree.value),
        Ok(()) => ("the schema must be a JSON object".to_string(), String::new()),
        Err(error) => (error.to_string(), error.instance_path().as_str().to_string()),
    };
    let (loc, _) = tree.nearest(&pointer);
    diagnostics.push(
        Diagnostic::error("config::invalid-schema", format!("invalid JSON Schema: {message}"))
            .with_source(tree.source.clone())
            .with_label(tree.first_line(&loc.value), "here"),
    );
    None
}

fn check_short_schema(schema: &ShortSchema, tree: &Tree, diagnostics: &mut Diagnostics) {
    for reserved in ["type", "tags"] {
        let Some(loc) = schema.fields.contains_key(reserved).then(|| tree.loc(&child("/schema", reserved))).flatten()
        else {
            continue;
        };
        diagnostics.push(
            Diagnostic::error("config::reserved-key", format!("`{reserved}` is a reserved key"))
                .with_source(tree.source.clone())
                .with_label(loc.key.clone().unwrap_or_else(|| loc.value.clone()), "janky adds this key"),
        );
    }
    if let Some(id) = schema.fields.get("id")
        && !matches!(id.kind, FieldKind::String | FieldKind::Slug)
    {
        diagnostics.push(error_at(
            tree,
            "config::id-type",
            "/schema/id/type",
            "`id` must be a `string` or a `slug`".to_string(),
            "not allowed for `id`",
        ));
    }
    for (name, spec) in &schema.fields {
        check_field_spec(spec, &child("/schema", name), schema, tree, diagnostics);
    }
    for (def, fields) in &schema.defs {
        for (name, spec) in fields {
            check_field_spec(spec, &child(&child("/schema/defs", def), name), schema, tree, diagnostics);
        }
    }
}

fn check_field_spec(spec: &FieldSpec, pointer: &str, schema: &ShortSchema, tree: &Tree, diagnostics: &mut Diagnostics) {
    let rules = [
        (spec.values.is_some(), spec.kind == FieldKind::Enum, "values", "enum"),
        (spec.of.is_some(), spec.kind == FieldKind::List, "of", "list"),
        (spec.schema.is_some(), spec.kind == FieldKind::Object, "schema", "object"),
        (spec.fields.is_some(), spec.kind == FieldKind::Object, "fields", "object"),
    ];
    for (present, allowed, key, kind) in rules {
        if present && !allowed {
            diagnostics.push(error_at(
                tree,
                "config::field-spec",
                &child(pointer, key),
                format!("`{key}` is only for `{kind}`"),
                "not allowed for this type",
            ));
        }
    }
    let missing = match (spec.kind, &spec.schema, &spec.fields) {
        (FieldKind::Enum, _, _) if spec.values.is_none() => Some("an `enum` needs `values`"),
        (FieldKind::List, _, _) if spec.of.is_none() => Some("a `list` needs `of`"),
        (FieldKind::Object, None, None) => Some("an `object` needs `schema` or `fields`"),
        _ => None,
    };
    if let Some(message) = missing {
        let (loc, _) = tree.nearest(pointer);
        diagnostics.push(
            Diagnostic::error("config::field-spec", message)
                .with_source(tree.source.clone())
                .with_label(tree.first_line(&loc.value), "incomplete"),
        );
    }
    if spec.kind == FieldKind::Object {
        match (&spec.schema, &spec.fields) {
            (Some(_), Some(_)) => diagnostics.push(error_at(
                tree,
                "config::field-spec",
                &child(pointer, "fields"),
                "an `object` has `schema` or `fields`, not both".to_string(),
                "remove `schema` or `fields`",
            )),
            (Some(name), None) if !schema.defs.contains_key(name) => diagnostics.push(error_at(
                tree,
                "config::unknown-def",
                &child(pointer, "schema"),
                format!("`{name}` is not in `[schema.defs]`"),
                "unknown definition",
            )),
            _ => {},
        }
    }
    if let Some(of) = &spec.of {
        check_field_spec(of, &child(pointer, "of"), schema, tree, diagnostics);
    }
    for (name, field) in spec.fields.iter().flatten() {
        check_field_spec(field, &child(&child(pointer, "fields"), name), schema, tree, diagnostics);
    }
}

/// Joins a display name and a relative path with `/`. An empty `dir` gives `path` unchanged.
pub fn join_name(dir: &str, path: &str) -> String {
    if dir.is_empty() { path.to_string() } else { format!("{dir}/{path}") }
}

/// Normalises a relative path without access to the file system.
///
/// It removes `.` parts and resolves `..` parts. It returns `None` for an absolute path, or if
/// `..` goes above the start. It does not see symlinks.
pub fn inside_path(path: &str) -> Option<PathBuf> {
    let mut normal = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => normal.push(part),
            Component::CurDir => {},
            Component::ParentDir => {
                if !normal.pop() {
                    return None;
                }
            },
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(normal)
}

#[derive(Clone, Copy)]
enum Place {
    Notetype,
    Project,
}

fn check_path(tree: &Tree, dir: &Path, place: Place, pointer: &str, path: &str, diagnostics: &mut Diagnostics) {
    let (code, what) = match place {
        Place::Notetype => ("config::outside-notetype", "the note type directory"),
        Place::Project => ("config::outside-project", "the project directory"),
    };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let normal = inside_path(path);
    let mut inside = normal.is_some();
    let full = dir.join(normal.unwrap_or_default());
    if inside && full.is_file() {
        inside = match (full.canonicalize(), dir.canonicalize()) {
            (Ok(full), Ok(dir)) => full.starts_with(dir),
            _ => false,
        };
    } else if inside && full.exists() {
        diagnostics.push(error_at(
            tree,
            "config::not-a-file",
            pointer,
            format!("`{path}` is not a regular file"),
            "not a regular file",
        ));
        return;
    } else if inside {
        diagnostics.push(error_at(
            tree,
            "config::missing-file",
            pointer,
            format!("file `{path}` does not exist"),
            &format!("no such file in {what}"),
        ));
        return;
    }
    if !inside {
        diagnostics.push(error_at(
            tree,
            code,
            pointer,
            format!("`{path}` is outside {what}"),
            &format!("paths must stay inside {what}"),
        ));
    }
}

fn check_keys<'a>(tree: &Tree, pointer: &str, keys: impl Iterator<Item = &'a String>, diagnostics: &mut Diagnostics) {
    for key in keys {
        if is_key(key) {
            continue;
        }
        let Some(loc) = tree.loc(&child(pointer, key)) else { continue };
        let span = loc.key.clone().unwrap_or_else(|| loc.value.clone());
        diagnostics.push(
            Diagnostic::error("config::key", format!("`{key}` is not a valid key"))
                .with_help(KEY_HELP)
                .with_source(tree.source.clone())
                .with_label(span, "invalid key"),
        );
    }
}

fn is_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn error_at(tree: &Tree, code: &str, pointer: &str, message: String, label: &str) -> Diagnostic {
    let (loc, _) = tree.nearest(pointer);
    Diagnostic::error(code, message).with_source(Arc::clone(&tree.source)).with_label(loc.value.clone(), label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sync_server_needs_https_except_on_loopback() {
        assert_eq!(endpoint_problem("https://anki.example.org/"), None);
        assert_eq!(endpoint_problem("http://127.0.0.1:8080/"), None);
        assert_eq!(endpoint_problem("http://localhost/"), None);
        let label = |endpoint: &str| endpoint_problem(endpoint).map(|(_, label)| label);
        assert_eq!(label("http://anki.lan/"), Some("plain HTTP is only allowed on loopback"));
        assert_eq!(label("https://user:secret@anki.example.org/"), Some("use `username` and `password`"));
        assert_eq!(label("anki.example.org"), Some("not a URL"));
        assert_eq!(endpoint_problem("https://anki.example.org"), None);
        assert_eq!(endpoint_problem("https://example.org/anki/"), None);
        assert_eq!(label("https://example.org/anki"), Some("add `/` at the end"));
        let (message, _) = endpoint_problem("https://user:secret@anki.example.org:99999/").unwrap();
        assert!(!message.contains("secret"), "{message}");
    }
}
