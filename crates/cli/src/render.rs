//! Field rendering with minijinja, and the checks of the card templates.
//!
//! [`render_with`] works in two passes. First it prepares each note type: it reads the CSS,
//! the assets and the card templates, checks the field names in the card templates, and
//! compiles the field templates. Then it renders the fields of every entry in the content
//! files. A field gets its text from its `value` or `template`. Without either, it gets the
//! entry value with the same key. Card templates stay Anki templates: janky does not render
//! them.
//!
//! [`environment`] sets up minijinja to work like Anki. Undefined values are chainable, and the
//! output escapes HTML with [`escape_html`]. It adds the filters `markdown`, `json`, `nfc`,
//! `sound` and `img`, and the function `media`. With plugins, [`render_with`] also adds the
//! filters and functions of each plugin, and the generator calls: `media(input, instance)` and
//! `input | media(instance)` for media jobs, and `enrich(input, instance)` and
//! `input | enrich(instance)` for data jobs.
//!
//! Rendering also collects the referenced media (through `media(path)`) and the generator jobs. A
//! card with `when` gets a flag field `_make_<card>` that holds `y` when the expression is
//! true. Its front template shows only when the flag is set.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use minijinja::value::{Object, Serde, Value};
use minijinja::{AutoEscape, Environment, Error, ErrorKind, UndefinedBehavior, context};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::config::deck::Deck;
use crate::content::ContentFile;
use crate::diagnostic::{Diagnostic, Diagnostics, Severity, Source};
use crate::generate::{Job, Jobs, Kind};
use crate::media::{Library, MediaFile, Problem};
use crate::plugin::{self, Failure, Plugins, Warnings};
use crate::project::{Project, join_name};
use crate::tree::{Tree, child};

/// The maximum size of one rendered field, in bytes.
pub const FIELD_LIMIT: usize = 1024 * 1024;
/// The maximum size of all rendered fields together, in bytes.
pub const TOTAL_LIMIT: usize = 256 * 1024 * 1024;
const SPECIAL_FIELDS: [&str; 7] = ["FrontSide", "Tags", "Type", "Deck", "Subdeck", "Card", "CardFlag"];

/// Everything that rendering makes: the note types ready for Anki, the notes, the media and
/// the generator jobs. The `expected/rendered.json` golden files hold its JSON form.
#[derive(Debug, Serialize)]
pub struct Rendered {
    /// By note type key.
    pub notetypes: IndexMap<String, RenderedNotetype>,
    /// In content file order, then entry order.
    pub notes: Vec<RenderedNote>,
    /// The plugin log lines (`print`, `janky.log`). Not serialised.
    #[serde(skip)]
    pub log: Vec<String>,
    /// The referenced media files, then the generated files that are in the cache. One file
    /// for each Anki file name.
    pub media: Vec<MediaFile>,
    /// The generator jobs that the field templates asked for. A repeated job is here once.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<Job>,
}

/// The parts of a note type that rendering reads from its files.
#[derive(Debug, Serialize)]
pub struct RenderedNotetype {
    /// Empty if the note type has no CSS file.
    pub css: String,
    /// By card key.
    pub cards: IndexMap<String, RenderedCard>,
}

/// The front and back Anki templates of a card type, with known `<script src>` assets inlined
/// by [`inline_scripts`].
#[derive(Debug, Serialize)]
pub struct RenderedCard {
    /// The front template. For a card with `when`, it is wrapped in `{{#_make_<card>}}`.
    pub front: String,
    /// The back template.
    pub back: String,
}

/// One entry with its rendered fields.
#[derive(Debug, Serialize)]
pub struct RenderedNote {
    /// The entry id.
    pub id: String,
    /// The note type key.
    pub notetype: String,
    /// By field key, in note type order, then the `_make_<card>` flag fields. A field that
    /// failed to render is empty.
    pub fields: IndexMap<String, String>,
}

#[derive(Debug, Clone)]
struct Site {
    source: Arc<Source>,
    base: Option<usize>,
    whole: Range<usize>,
}

enum FieldPlan {
    Template(String, Site),
    Entry,
    Broken,
}

struct Flag {
    field: String,
    when: Option<String>,
    site: Site,
}

struct Plan {
    fields: IndexMap<String, FieldPlan>,
    flags: Vec<Flag>,
}

/// The value of the `media` template function and filter.
///
/// It prints as its Anki file name. Templates can also read `file`, `kind` and `exists`. The
/// `sound` and `img` filters accept it or a plain file name.
#[derive(Debug)]
pub struct Media {
    /// The Anki file name. For a referenced file, the last part of the path. For a generated
    /// file, the cache file name. It is empty for an empty input, or while the job has no
    /// file name yet.
    pub file: String,
    /// `referenced` for `media(path)`, `generated` for a media job.
    pub kind: &'static str,
    /// For a referenced file, the file is in one of the media directories. For a generated
    /// file, the cache has the result of the job.
    pub exists: bool,
}

impl Object for Media {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "file" => Some(Value::from(self.file.clone())),
            "kind" => Some(Value::from(self.kind)),
            "exists" => Some(Value::from(self.exists)),
            _ => None,
        }
    }

    fn render(self: &Arc<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.file)
    }
}

/// Makes the minijinja environment with the janky filters and the `media` function.
///
/// `media(path)` looks the path up in `library`, which records the file or the problem. With
/// plugins, [`render_with`] adds the second form, `media(input, instance)`, and the filter
/// `input | media(instance)`.
pub fn environment(library: Arc<Mutex<Library>>) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_debug(true);
    env.set_undefined_behavior(UndefinedBehavior::Chainable);
    env.set_auto_escape_callback(|_| AutoEscape::Html);
    env.set_formatter(|out, _, value| {
        if value.is_safe() {
            write!(out, "{value}").map_err(Error::from)
        } else {
            out.write_str(&escape_html(&value.to_string())).map_err(Error::from)
        }
    });
    env.add_filter("markdown", markdown);
    env.add_filter("json", json);
    env.add_filter("nfc", nfc);
    env.add_filter("sound", sound);
    env.add_filter("img", img);
    env.add_function("media", move |path: String| referenced(&library, &path));
    env
}

fn referenced(library: &Mutex<Library>, path: &str) -> Value {
    let exists = library.lock().expect("media library").lookup(path);
    let file = Path::new(path).file_name().map_or_else(|| path.to_string(), |name| name.to_string_lossy().into());
    Value::from_object(Media { file, kind: "referenced", exists })
}

/// Replaces `media` with its two forms, `media(path)` for a referenced file and
/// `media(input, plugin)` for a media job, and adds the filter `input | media(plugin)`.
fn add_media_jobs(env: &mut Environment<'static>, library: Arc<Mutex<Library>>, job: plugin::MediaJob) {
    let function = job.clone();
    env.add_function("media", move |input: Value, plugin: Option<String>| match plugin {
        None => Ok(referenced(&library, &input.to_string())),
        Some(plugin) => function(input, plugin),
    });
    env.add_filter("media", move |input: Value, plugin: String| job(input, plugin));
}

/// Renders the content files without plugins. See [`render_with`].
pub fn render(project: &Project, files: &[ContentFile], diagnostics: &mut Diagnostics) -> Rendered {
    render_with(project, None, files, diagnostics)
}

/// Prepares every note type, then renders every entry of `files`.
///
/// It pushes a diagnostic for each file that it cannot read, each template that does not
/// compile or render, each unknown field in a card template, each field over [`FIELD_LIMIT`],
/// and each media problem. The same template error shows only once, for the first entry that
/// has it. A field with an error renders as empty text.
///
/// When the rendered fields together pass [`TOTAL_LIMIT`], it pushes an error and stops.
/// [`Rendered::notes`] then has only the notes before that entry.
///
/// With `plugins`, it registers their filters, functions and generators. A plugin error comes
/// with a related diagnostic at the template that called the plugin. The warning of a cached
/// data result becomes an `enrich::warning` warning, and its info an `enrich::info` info, at the template that called `enrich`.
pub fn render_with(
    project: &Project,
    plugins: Option<&Plugins>,
    files: &[ContentFile],
    diagnostics: &mut Diagnostics,
) -> Rendered {
    let library = Arc::new(Mutex::new(Library::new(&project.root, &project.deck.deck.media_dirs)));
    let mut env = environment(library.clone());
    let failure: Failure = Arc::default();
    let jobs: Jobs = Arc::default();
    let warnings: Warnings = Arc::default();
    if let Some(plugins) = plugins {
        plugin::register(&mut env, plugins, &failure);
        let job = plugin::register_generate(&mut env, plugins, &failure, &jobs, &warnings, &project.root);
        add_media_jobs(&mut env, library.clone(), job);
    }
    let failed = |site: &Site, error: &Error| match failure.lock().expect("plugin failure").take() {
        Some(diagnostic) => diagnostic.with_related(
            Diagnostic::error("plugin::call", "the plugin was called here")
                .with_source(site.source.clone())
                .with_label(error_span(site, error), "here"),
        ),
        None => template_error(site, error),
    };
    let mut notetypes = IndexMap::new();
    let mut plans = HashMap::new();
    for key in project.notetypes.keys() {
        let mut found = Diagnostics::default();
        let (notetype, plan) = prepare(project, key, &mut env, &mut found);
        diagnostics.append_sorted(found);
        notetypes.insert(key.clone(), notetype);
        plans.insert(key.clone(), plan);
    }

    let mut seen = HashSet::new();
    let mut notes = Vec::new();
    let mut total = 0;
    'files: for file in files {
        let (deck_name, full_name) = deck_names(&project.deck.deck.deck, &file.deck);
        let stem = Path::new(&file.path).file_stem().map(|stem| stem.to_string_lossy().into_owned());
        let defaults = file.tree.value.get("defaults").cloned().unwrap_or_default();
        let file_value =
            context! { path => file.path.clone(), stem => stem, defaults => Value::from(Serde(&defaults)) };
        let deck_value = context! { key => file.deck.join("."), name => deck_name, full_name => full_name };
        for entry in &file.entries {
            let notetype = &project.notetypes[&entry.notetype];
            let entry_value = file.tree.value.pointer(&entry.pointer).cloned().unwrap_or_default();
            let ctx = context! {
                entry => Value::from(Serde(&entry_value)),
                file => file_value.clone(),
                deck => deck_value.clone(),
                notetype => context! { key => entry.notetype.clone(), name => notetype.file.name.clone() },
            };
            let first_seen = format!("first seen in entry `{}` in `{}`", entry.id, file.path);
            let plan = &plans[&entry.notetype];
            let mut fields = IndexMap::new();
            for (key, field) in &plan.fields {
                let value = match field {
                    FieldPlan::Template(name, site) => {
                        match env.get_template(name).and_then(|template| template.render(&ctx)) {
                            Ok(value) if value.len() > FIELD_LIMIT => {
                                diagnostics.push(
                                    Diagnostic::error(
                                        "template::too-large",
                                        format!(
                                            "field `{key}` has {} bytes, more than the limit of {FIELD_LIMIT} bytes",
                                            value.len()
                                        ),
                                    )
                                    .with_help(format!("entry `{}` in `{}`", entry.id, file.path))
                                    .with_source(site.source.clone())
                                    .with_label(site.whole.clone(), "this template"),
                                );
                                String::new()
                            },
                            Ok(value) => value,
                            Err(error) => {
                                if seen.insert((name.clone(), error.to_string())) {
                                    diagnostics.push(failed(site, &error).with_help(first_seen.clone()));
                                }
                                String::new()
                            },
                        }
                    },
                    FieldPlan::Entry => match entry_value.get(key) {
                        None | Some(serde_json::Value::Null) => String::new(),
                        Some(serde_json::Value::String(text)) => escape_html(text),
                        Some(value @ (serde_json::Value::Number(_) | serde_json::Value::Bool(_))) => value.to_string(),
                        Some(_) => {
                            if seen.insert((format!("{}#{key}", entry.notetype), String::new())) {
                                let pointer = child("/fields", key);
                                let (loc, _) = notetype.tree.nearest(&pointer);
                                diagnostics.push(
                                    Diagnostic::error(
                                        "template::not-text",
                                        format!("field `{key}` has no `value` or `template`, but the entry value is a list or an object"),
                                    )
                                    .with_help(format!("add a `value` or `template` that renders it; {first_seen}"))
                                    .with_source(notetype.tree.source.clone())
                                    .with_label(notetype.tree.first_line(&loc.value), "no `value` or `template`"),
                                );
                            }
                            String::new()
                        },
                    },
                    FieldPlan::Broken => String::new(),
                };
                if let FieldPlan::Template(_, site) = field {
                    take_warnings(&warnings, site, &first_seen, diagnostics);
                }
                total += value.len();
                fields.insert(key.clone(), value);
            }
            if total > TOTAL_LIMIT {
                diagnostics.push(Diagnostic::error(
                    "template::too-large",
                    format!(
                        "the rendered fields have more than {TOTAL_LIMIT} bytes; janky stopped at entry `{}` in `{}`",
                        entry.id, file.path
                    ),
                ));
                break 'files;
            }
            for flag in &plan.flags {
                let Some(when) = &flag.when else {
                    fields.insert(flag.field.clone(), String::new());
                    continue;
                };
                let value = match env.compile_expression(when).and_then(|expression| expression.eval(&ctx)) {
                    Ok(value) if value.is_true() => "y".to_string(),
                    Ok(_) => String::new(),
                    Err(error) => {
                        if seen.insert((flag.field.clone(), error.to_string())) {
                            diagnostics.push(failed(&flag.site, &error).with_help(first_seen.clone()));
                        }
                        String::new()
                    },
                };
                take_warnings(&warnings, &flag.site, &first_seen, diagnostics);
                fields.insert(flag.field.clone(), value);
            }
            let problems = library.lock().expect("media library").take_problems();
            for (path, problem) in problems {
                let dirs = library.lock().expect("media library").dirs().join(", ");
                let diagnostic = media_error(&file.tree, &entry.pointer, &path, problem, &dirs);
                let span = diagnostic.labels.first().map(|label| label.span.start).unwrap_or_default();
                if seen.insert((format!("{}#{span}", file.path), diagnostic.code.clone())) {
                    diagnostics.push(diagnostic);
                }
            }
            notes.push(RenderedNote {
                id: entry.id.clone(),
                notetype: entry.notetype.clone(),
                fields,
            });
        }
    }
    let mut media = library.lock().expect("media library").files();
    let jobs: Vec<Job> = jobs.lock().expect("jobs").values().cloned().collect();
    let mut names: HashSet<String> = media.iter().map(|file| file.name.clone()).collect();
    let generated = jobs.iter().filter(|job| job.kind == Kind::Media && job.exists);
    media.extend(generated.filter(|job| names.insert(job.file.clone())).map(|job| MediaFile {
        name: job.file.clone(),
        path: format!("{}/{}", crate::cache::MEDIA_DIR, job.file),
        kind: "generated",
    }));
    let log = plugins.map(|plugins| plugins.sandbox.take_log()).unwrap_or_default();
    Rendered { notetypes, notes, media, log, jobs }
}

fn take_warnings(warnings: &Warnings, site: &Site, first_seen: &str, diagnostics: &mut Diagnostics) {
    for (job, severity, text) in std::mem::take(&mut *warnings.lock().expect("warnings")) {
        let code = if severity == Severity::Info { "enrich::info" } else { "enrich::warning" };
        diagnostics.push(
            Diagnostic::new(severity, code, format!("{job}: {text}"))
                .with_help(first_seen.to_string())
                .with_source(site.source.clone())
                .with_label(site.whole.clone(), "`enrich` is called here"),
        );
    }
}

fn media_error(tree: &Tree, pointer: &str, path: &str, problem: Problem, dirs: &str) -> Diagnostic {
    let span = tree
        .spans
        .iter()
        .filter(|(at, _)| *at == pointer || at.starts_with(&format!("{pointer}/")))
        .filter(|(at, _)| tree.value.pointer(at).and_then(serde_json::Value::as_str) == Some(path))
        .map(|(_, loc)| loc.value.clone())
        .min_by_key(|span| span.start)
        .unwrap_or_else(|| tree.first_line(&tree.nearest(pointer).0.value));
    let diagnostic = match problem {
        Problem::Missing => Diagnostic::error("media::missing", format!("media file `{path}` does not exist"))
            .with_help(format!("janky looks in these media directories: {dirs}")),
        Problem::Outside => {
            Diagnostic::error("media::outside", format!("media path `{path}` is outside the media directories"))
        },
        Problem::Collision { first } => {
            let name = Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            Diagnostic::error("media::name-collision", format!("`{first}` already has the Anki file name `{name}`"))
                .with_help("Anki keeps media in one folder, so each file needs its own file name")
        },
    };
    diagnostic.with_source(tree.source.clone()).with_label(span, "here")
}

fn prepare(
    project: &Project,
    key: &str,
    env: &mut Environment<'static>,
    found: &mut Diagnostics,
) -> (RenderedNotetype, Plan) {
    let notetype = &project.notetypes[key];
    let tree = &notetype.tree;
    let prefix = notetype.dir_name.clone();
    let read = |path: &str, found: &mut Diagnostics| -> Option<Arc<Source>> {
        match std::fs::read_to_string(notetype.dir.join(path)) {
            Ok(text) => Some(Source::new(join_name(&prefix, path), text)),
            Err(error) => {
                let name = join_name(&prefix, path);
                found.push(Diagnostic::error("template::read", format!("cannot read `{name}`: {error}")));
                None
            },
        }
    };

    let css = notetype.file.css.as_deref().and_then(|css| read(css, found)).map(|source| source.text().to_string());
    let mut assets = HashMap::new();
    for (index, path) in notetype.file.assets.iter().enumerate() {
        let Some(source) = read(path, found) else { continue };
        if source.text().to_ascii_lowercase().contains("</script") {
            found.push(error_at(
                tree,
                "template::script-end",
                &format!("/assets/{index}"),
                format!("`{path}` contains `</script>`"),
            ));
            continue;
        }
        let name = Path::new(path).file_name().map_or_else(|| path.clone(), |name| name.to_string_lossy().into());
        assets.insert(name, source.text().to_string());
    }

    let names: HashSet<&str> = notetype.file.fields.values().map(|field| field.name.as_str()).collect();
    let mut cards = IndexMap::new();
    let mut flags = Vec::new();
    let mut checked = HashSet::new();
    for (card_key, card) in &notetype.file.cards {
        let mut sides = Vec::new();
        for path in [&card.front, &card.back] {
            let Some(source) = read(path, found) else {
                sides.push(String::new());
                continue;
            };
            if checked.insert(path.clone()) {
                check_card_fields(&source, &names, found);
            }
            sides.push(inline_scripts(source.text(), &assets));
        }
        let back = sides.pop().unwrap_or_default();
        let mut front = sides.pop().unwrap_or_default();
        if let Some(when) = &card.when {
            let field = format!("_make_{card_key}");
            front = format!("{{{{#{field}}}}}{front}{{{{/{field}}}}}");
            let site = inline_site(tree, &child(&child("/cards", card_key), "when"), when);
            let when = match env.compile_expression(when) {
                Ok(_) => Some(when.clone()),
                Err(error) => {
                    found.push(template_error(&site, &error));
                    None
                },
            };
            flags.push(Flag { field, when, site });
        }
        cards.insert(card_key.clone(), RenderedCard { front, back });
    }

    let mut fields = IndexMap::new();
    for (field_key, field) in &notetype.file.fields {
        let template = match (&field.value, &field.template) {
            (Some(value), _) => {
                let site = inline_site(tree, &child(&child("/fields", field_key), "value"), value);
                Some((format!("{}#fields.{field_key}", tree.source.name()), value.clone(), site))
            },
            (None, Some(path)) => read(path, found).map(|source| {
                let whole = 0..source.text().len();
                (source.name().to_string(), source.text().to_string(), Site { source, base: Some(0), whole })
            }),
            (None, None) => None,
        };
        let plan = match template {
            None if field.template.is_some() => FieldPlan::Broken,
            None => FieldPlan::Entry,
            Some((name, text, site)) => match env.add_template_owned(name.clone(), text) {
                Ok(()) => FieldPlan::Template(name, site),
                Err(error) => {
                    found.push(template_error(&site, &error));
                    FieldPlan::Broken
                },
            },
        };
        fields.insert(field_key.clone(), plan);
    }

    (RenderedNotetype { css: css.unwrap_or_default(), cards }, Plan { fields, flags })
}

fn inline_site(tree: &Tree, pointer: &str, text: &str) -> Site {
    let (loc, _) = tree.nearest(pointer);
    let raw = &tree.source.text()[loc.value.clone()];
    Site {
        source: tree.source.clone(),
        base: raw.find(text).map(|index| loc.value.start + index),
        whole: loc.value.clone(),
    }
}

fn template_error(site: &Site, error: &Error) -> Diagnostic {
    let code = match error.kind() {
        ErrorKind::SyntaxError => "template::syntax",
        ErrorKind::UnknownFilter => "template::unknown-filter",
        ErrorKind::UnknownFunction => "template::unknown-function",
        _ => "template::render",
    };
    let message = match error.detail() {
        Some(detail) => format!("{}: {detail}", error.kind()),
        None => error.kind().to_string(),
    };
    Diagnostic::error(code, message).with_source(site.source.clone()).with_label(error_span(site, error), "here")
}

fn error_span(site: &Site, error: &Error) -> Range<usize> {
    match (site.base, error.range()) {
        (Some(base), Some(range)) => base + range.start..base + range.end,
        _ => site.whole.clone(),
    }
}

fn check_card_fields(source: &Arc<Source>, names: &HashSet<&str>, found: &mut Diagnostics) {
    for (span, name) in card_tags(source.text()) {
        if names.contains(name) || SPECIAL_FIELDS.contains(&name) {
            continue;
        }
        let mut known: Vec<&str> = names.iter().copied().collect();
        known.sort_unstable();
        found.push(
            Diagnostic::error("card::unknown-field", format!("the note type has no field `{name}`"))
                .with_help(format!("the fields are: {}", known.join(", ")))
                .with_source(source.clone())
                .with_label(span, "unknown field"),
        );
    }
}

/// Finds the field references in an Anki card template, with their byte spans.
///
/// It skips HTML comments. For `{{#Field}}`, `{{^Field}}`, `{{/Field}}` and `{{filter:Field}}`
/// it gives only `Field`.
pub fn card_tags(text: &str) -> Vec<(Range<usize>, &str)> {
    let mut tags = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let comment = rest.find("<!--");
        let Some(open) = rest.find("{{") else { break };
        if let Some(comment) = comment
            && comment < open
        {
            at += rest[comment..].find("-->").map_or(rest.len(), |end| comment + end + 3);
            continue;
        }
        let Some(close) = rest[open..].find("}}") else { break };
        let start = at + open;
        let end = start + close + 2;
        let inner = text[start + 2..end - 2].trim_start_matches('{').trim();
        let key = inner.strip_prefix(['#', '^', '/']).unwrap_or(inner).trim();
        let name = key.rsplit(':').next().unwrap_or_default().trim();
        if !name.is_empty() {
            tags.push((start..end, name));
        }
        at = end;
    }
    tags
}

/// Replaces each empty `<script src="...">` element whose `src` is a key of `assets` with an
/// inline `<script>` that holds the asset text. Other script elements stay as they are.
pub fn inline_scripts(text: &str, assets: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<script") {
        let Some(close) = rest[start..].find("</script>") else { break };
        let end = start + close + "</script>".len();
        let element = &rest[start..end];
        let open_end = element.find('>').unwrap_or(element.len() - 1);
        let body = &element[open_end + 1..close];
        match attribute(&element[..open_end], "src").and_then(|src| assets.get(src)) {
            Some(code) if body.trim().is_empty() => {
                out.push_str(&rest[..start]);
                out.push_str("<script>\n");
                out.push_str(code.trim_end());
                out.push_str("\n</script>");
            },
            _ => out.push_str(&rest[..end]),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!("{name}="))? + name.len() + 1;
    let quote = tag[at..].chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &tag[at + 1..];
    value.find(quote).map(|end| &value[..end])
}

fn deck_names(root: &Deck, keys: &[String]) -> (String, String) {
    let mut deck = root;
    let mut full_name = root.name.clone();
    for key in keys {
        deck = &deck.children[key];
        full_name = format!("{full_name}::{}", deck.name);
    }
    (deck.name.clone(), full_name)
}

fn error_at(tree: &Tree, code: &str, pointer: &str, message: String) -> Diagnostic {
    let (loc, _) = tree.nearest(pointer);
    Diagnostic::error(code, message).with_source(tree.source.clone()).with_label(loc.value.clone(), "here")
}

/// Escapes `&`, `<`, `>`, `"` and `'` for HTML text and attribute values.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn markdown(value: Value) -> Result<Value, Error> {
    if value.is_undefined() || value.is_none() {
        return Ok(Value::from_safe_string(String::new()));
    }
    let text = value.as_str().ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "markdown needs a string"))?;
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(text));
    Ok(Value::from_safe_string(html.trim_end().to_string()))
}

fn json(value: Value) -> Result<Value, Error> {
    let text = serde_json::to_string(&value)
        .map_err(|error| Error::new(ErrorKind::InvalidOperation, format!("cannot write JSON: {error}")))?;
    Ok(Value::from_safe_string(text.replace('<', "\\u003c").replace('&', "\\u0026")))
}

fn nfc(value: Value) -> Value {
    let text: String = value.to_string().nfc().collect();
    if value.is_safe() { Value::from_safe_string(text) } else { Value::from(text) }
}

fn media_name(value: &Value) -> String {
    match value.downcast_object_ref::<Media>() {
        Some(media) => media.file.clone(),
        None => value.to_string(),
    }
}

fn sound(value: Value) -> Value {
    let name = media_name(&value);
    if name.is_empty() {
        return Value::from_safe_string(String::new());
    }
    Value::from_safe_string(format!("[sound:{}]", escape_html(&name)))
}

fn img(value: Value) -> Value {
    let name = media_name(&value);
    if name.is_empty() {
        return Value::from_safe_string(String::new());
    }
    Value::from_safe_string(format!("<img src=\"{}\">", escape_html(&name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_str(template: &str) -> String {
        let env = environment(Arc::new(Mutex::new(Library::new(Path::new("/nonexistent"), &[]))));
        env.render_str(template, context! { name => "Tom & \"Jerry\" / <b>", path => "audio/⚛️.ogg" }).unwrap()
    }

    #[test]
    fn card_tags_find_field_names() {
        let text = "{{Symbol}} <!-- {{Hidden}} --> {{#Hint}}{{text:Hint}}{{/Hint}} {{^Image}}x{{/Image}} {{ tts en_GB:Symbol }}";
        let names: Vec<&str> = card_tags(text).into_iter().map(|(_, name)| name).collect();
        assert_eq!(names, ["Symbol", "Hint", "Hint", "Hint", "Image", "Image", "Symbol"]);
        let (span, _) = &card_tags(text)[0];
        assert_eq!(&text[span.clone()], "{{Symbol}}");
    }

    #[test]
    fn scripts_with_known_assets_are_inlined() {
        let assets = HashMap::from([("reveal.js".to_string(), "run();\n".to_string())]);
        let text = "a<script src=\"reveal.js\"></script>b<script src='other.js'></script><script>keep();</script>";
        assert_eq!(
            inline_scripts(text, &assets),
            "a<script>\nrun();\n</script>b<script src='other.js'></script><script>keep();</script>"
        );
    }

    #[test]
    fn output_escapes_like_anki() {
        assert_eq!(render_str("{{ name }}"), "Tom &amp; &quot;Jerry&quot; / &lt;b&gt;");
        assert_eq!(escape_html("it's"), "it&#39;s");
    }

    #[test]
    fn media_filters_make_anki_markup() {
        assert_eq!(render_str("{{ media(path) | sound }}"), "[sound:⚛️.ogg]");
        assert_eq!(
            render_str("{{ media(path).file }} {{ media(path).kind }} {{ 'yes' if media(path).exists else 'no' }}"),
            "⚛️.ogg referenced no"
        );
        assert_eq!(render_str("{{ 'a\"b.png' | img }}"), "<img src=\"a&quot;b.png\">");
        assert_eq!(render_str("{{ missing | sound }}{{ missing | img }}"), "");
    }

    #[test]
    fn json_and_markdown_filters() {
        assert_eq!(render_str("{{ {'a': name} | json }}"), "{\"a\":\"Tom \\u0026 \\\"Jerry\\\" / \\u003cb>\"}");
        assert_eq!(render_str("{{ '*x*' | markdown }}"), "<p><em>x</em></p>");
        assert_eq!(render_str("{{ missing | markdown }}"), "");
    }
}
