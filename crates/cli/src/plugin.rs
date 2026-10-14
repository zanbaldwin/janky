//! Loads plugin scripts and connects them to the template environment.
//!
//! [`load`] runs each script once in the shared [`Sandbox`]: first the plugin instances of
//! `deck.toml`, then the plugins of the note types. Each script gets its own environment, with
//! copies of the globals and the library tables, so a plugin cannot change the functions of
//! another plugin. The script returns a table, which [`load`] checks and turns into a
//! [`Plugin`].
//!
//! The table has `id` and `api = 1`, and these optional keys:
//!
//! - `filters` and `functions`: template filters and functions, by name.
//! - `variables`: the schema of the instance variables, in the short form of a note type
//!   `[schema]`.
//! - `generate`: the generator function. Only a plugin instance of `deck.toml` can have it.
//!   It needs `generates`, which is `"media"` (for `media(input, instance)`) or `"data"` (for
//!   `enrich(input, instance)`).
//! - `plan` and `prepare`: optional functions of a generator. `plan` gives the cache key, the
//!   extension and the billable units of a job. `prepare` runs once before the jobs of an
//!   instance.
//! - `secrets`: the names of the secrets that the generator uses.
//!
//! [`register`] adds the plugin filters and functions to minijinja. [`register_generate`] adds
//! the `enrich` function and filter, and gives back the function for the generated form of
//! `media`. Both record generator jobs and run `plan`. They send nothing: [`crate::runner`]
//! runs the jobs later. Each call into Lua gets a new copy of the instance variables as
//! `janky.variables`, so a change that a call makes is lost after it.
//!
//! [`lua_error`] turns a Lua error into a diagnostic that points at the line in the script.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use mlua::{Function, Table, Value};

use crate::config::deck::SecretSource;
use crate::config::notetype::{FieldSpec, ShortSchema};
use crate::diagnostic::{Diagnostic, Diagnostics, Severity, Source};
use crate::generate::{Job, Jobs, Kind, Planned, canonical, job_hash, job_input, parse_plan};
use crate::lua::Sandbox;
use crate::project::{Project, join_name};
use crate::render::Media;
use crate::tree::{Tree, child, display_path};

const PROBLEM_LIMIT: usize = 20;
const SECRET_LIMIT: usize = 32;

/// The names of janky's own filters and functions. A plugin cannot supply these names.
pub const BUILT_IN: [&str; 7] = ["markdown", "json", "nfc", "sound", "img", "media", "generate"];
const KEYS: [&str; 10] = [
    "id",
    "api",
    "filters",
    "functions",
    "variables",
    "generate",
    "generates",
    "plan",
    "prepare",
    "secrets",
];

/// One loaded plugin script: a plugin instance of `deck.toml`, or a plugin of a note type.
pub struct Plugin {
    /// The instance name for a plugin instance, or `<note type key>/<path>` for a note type
    /// plugin.
    pub name: String,
    /// The `id` from the returned table. It names the cached results of a generator, so two
    /// scripts with `generate` cannot share it.
    pub id: String,
    /// The script text, for diagnostics.
    pub source: Arc<Source>,
    /// The short chunk name that Lua knows the script by (`plugin<index>`), for [`lua_error`].
    pub(crate) chunk: String,
    /// The filters, sorted by name.
    pub filters: IndexMap<String, Function>,
    /// The functions, sorted by name.
    pub functions: IndexMap<String, Function>,
    /// The schema of the instance variables, in the short form of a note type `[schema]`.
    pub variables: ShortSchema,
    /// The generator function, which [`crate::runner`] calls for each pending job.
    pub generate: Option<Function>,
    /// What `generate` makes: media files for `media(input, plugin)`, or data for `enrich()`. Set exactly
    /// when the plugin has `generate`.
    pub generates: Option<Kind>,
    /// Gives the cache key, the extension and the billable units of a new job. Allowed only with
    /// `generate`.
    pub plan: Option<Function>,
    /// Runs once for each instance before its pending jobs, for example to build an index.
    /// Allowed only with `generate`.
    pub prepare: Option<Function>,
    /// The secret names that the plugin uses, in the order of its `secrets` list.
    pub secrets: Vec<String>,
    /// Whether the plugin is a plugin instance of `deck.toml`. Only an instance can have
    /// required variables, secrets, network access, file access and a `generate` function.
    pub instance: bool,
    /// The own Lua environment of the script, with copies of the globals and library tables.
    pub(crate) env: Table,
    /// The instance variables from `deck.toml` (an empty map for a note type plugin), which
    /// [`set_variables`] copies into `janky.variables` before each call.
    pub(crate) values: minijinja::Value,
    /// The directories that `[plugins.<name>.filesystem]` grants, by name, as host paths.
    pub(crate) read: IndexMap<String, PathBuf>,
}

/// All plugins of a project, and the one Lua state that they share.
pub struct Plugins {
    /// The Lua state and limits that every call into a plugin runs under.
    pub sandbox: Arc<Sandbox>,
    /// The plugin instances of `deck.toml` first, then the note type plugins.
    pub plugins: Vec<Plugin>,
}

struct Script<'a> {
    name: String,
    path: PathBuf,
    shown: String,
    tree: &'a Tree,
    pointer: String,
    values: Option<&'a serde_json::Map<String, serde_json::Value>>,
    secrets: Option<(&'a IndexMap<String, SecretSource>, String)>,
    http: bool,
    read: Option<(&'a IndexMap<String, String>, String, &'a std::path::Path)>,
}

/// Runs every plugin script of the project and checks the table that it returns.
///
/// Each script runs under the limits of the sandbox, with its instance variables (none for a
/// note type plugin) in `janky.variables`. The checks push an error when a script cannot be read
/// or fails in Lua, and when its table is wrong. A wrong table gives at most 20 errors for each
/// script, and then one error with the number of the others. Other errors are a name that
/// another plugin or janky already supplies, and a generator `id` that another script also
/// uses. A `[plugins.<name>.filesystem]` path that starts with `~/` when `HOME` is not set is an
/// error. A configured secret that the plugin does not list is a warning. So is `http` or `secrets` on
/// an instance without `generate`.
///
/// Gives back `None` when `diagnostics` has an error, also one from before the call.
pub fn load(project: &Project, diagnostics: &mut Diagnostics) -> Option<Plugins> {
    let mut scripts = Vec::new();
    for (key, instance) in &project.deck.plugins {
        scripts.push(Script {
            name: key.clone(),
            path: project.root.join(&instance.path),
            shown: instance.path.clone(),
            tree: &project.deck_tree,
            pointer: child(&child("/plugins", key), "path"),
            values: Some(&instance.variables),
            secrets: Some((&instance.secrets, child(&child("/plugins", key), "secrets"))),
            http: instance.http.is_some(),
            read: instance.filesystem.as_ref().map(|filesystem| {
                (
                    &filesystem.read,
                    child(&child(&child("/plugins", key), "filesystem"), "read"),
                    project.root.as_path(),
                )
            }),
        });
    }
    for (key, notetype) in &project.notetypes {
        for (index, path) in notetype.file.plugins.iter().enumerate() {
            scripts.push(Script {
                name: format!("{key}/{path}"),
                path: notetype.dir.join(path),
                shown: join_name(&notetype.dir_name, path),
                tree: &notetype.tree,
                pointer: format!("/plugins/{index}"),
                values: None,
                secrets: None,
                http: false,
                read: None,
            });
        }
    }
    let sandbox = match Sandbox::new() {
        Ok(sandbox) => sandbox,
        Err(error) => {
            diagnostics.push(Diagnostic::error("plugin::lua", format!("cannot start Lua: {error}")));
            return None;
        },
    };
    if scripts.is_empty() {
        return Some(Plugins {
            sandbox: Arc::new(sandbox),
            plugins: Vec::new(),
        });
    }
    let mut found = Diagnostics::default();
    let mut plugins = Vec::new();
    let mut owners: HashMap<String, String> = BUILT_IN.iter().map(|name| (name.to_string(), String::new())).collect();
    let mut ids: HashMap<String, String> = HashMap::new();
    for (index, script) in scripts.iter().enumerate() {
        if let Some(plugin) = load_script(&sandbox, script, format!("plugin{index}"), &mut owners, &mut found) {
            if plugin.generate.is_some() {
                let first = ids.entry(plugin.id.clone()).or_insert_with(|| script.shown.clone());
                if *first != script.shown {
                    let (loc, _) = script.tree.nearest(&script.pointer);
                    found.push(
                        Diagnostic::error(
                            "plugin::id",
                            format!(
                                "`{}` has the id `{}`, like `{first}`: the id names the cached results, so it must be unique",
                                script.shown, plugin.id
                            ),
                        )
                        .with_source(script.tree.source.clone())
                        .with_label(loc.value.clone(), "here"),
                    );
                }
            }
            plugins.push(plugin);
        }
    }
    diagnostics.append_sorted(found);
    (!diagnostics.has_errors()).then_some(Plugins { sandbox: Arc::new(sandbox), plugins })
}

fn load_script(
    sandbox: &Sandbox,
    script: &Script,
    chunk: String,
    owners: &mut HashMap<String, String>,
    found: &mut Diagnostics,
) -> Option<Plugin> {
    let config_error = |code: &str, message: String| {
        let (loc, _) = script.tree.nearest(&script.pointer);
        Diagnostic::error(code, message).with_source(script.tree.source.clone()).with_label(loc.value.clone(), "here")
    };
    let text = match std::fs::read_to_string(&script.path) {
        Ok(text) => text,
        Err(error) => {
            found.push(config_error("plugin::read", format!("cannot read `{}`: {error}", script.shown)));
            return None;
        },
    };
    let source = Source::new(script.shown.clone(), text);
    let values = minijinja::Value::from(minijinja::value::Serde(script.values.cloned().unwrap_or_default()));
    let env = match new_environment(sandbox).and_then(|env| set_variables(sandbox, &env, &values).map(|()| env)) {
        Ok(env) => env,
        Err(error) => {
            found.push(config_error("plugin::lua", format!("cannot start `{}`: {error}", script.shown)));
            return None;
        },
    };
    let returned = sandbox.exec_in(source.text(), &chunk, &script.shown, env.clone());
    let table = match returned {
        Ok(Value::Table(table)) => table,
        Ok(_) => {
            found.push(config_error("plugin::table", format!("`{}` must return a table", script.shown)));
            return None;
        },
        Err(error) => {
            found.push(lua_error(&source, &chunk, &error));
            return None;
        },
    };

    let mut problems = Vec::new();
    let mut unknown = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        if unknown.len() >= PROBLEM_LIMIT {
            break;
        }
        let Ok((key, _)) = pair else { continue };
        let Value::String(key) = key else {
            if !problems.is_empty() {
                continue;
            }
            problems.push("the returned table must have only string keys".to_string());
            continue;
        };
        let key = key.to_string_lossy();
        if !KEYS.contains(&key.as_str()) {
            unknown.push(key);
        }
    }
    unknown.sort();
    problems.extend(unknown.into_iter().map(|key| format!("unknown key `{key}` in the returned table")));
    let id = match table.raw_get::<Value>("id") {
        Ok(Value::String(id)) if !id.to_string_lossy().is_empty() => id.to_string_lossy(),
        _ => {
            problems.push("`id` must be a non-empty string".to_string());
            String::new()
        },
    };
    if !matches!(table.raw_get::<Value>("api"), Ok(Value::Integer(1))) {
        problems.push("`api` must be 1".to_string());
    }
    let filters = named_functions(&table, "filters", &mut problems);
    let functions = named_functions(&table, "functions", &mut problems);
    let variables = match table.raw_get::<Value>("variables") {
        Ok(value @ Value::Table(_)) => short_schema(&value, &mut problems),
        Ok(Value::Nil) => ShortSchema::default(),
        _ => {
            problems.push("`variables` must be a table".to_string());
            ShortSchema::default()
        },
    };
    let generate = optional_function(&table, "generate", &mut problems);
    let plan = optional_function(&table, "plan", &mut problems);
    if plan.is_some() && generate.is_none() {
        problems.push("`plan` is allowed only with `generate`".into());
    }
    let prepare = optional_function(&table, "prepare", &mut problems);
    if prepare.is_some() && generate.is_none() {
        problems.push("`prepare` is allowed only with `generate`".into());
    }
    let generates = match table.raw_get::<Value>("generates") {
        Ok(Value::Nil) => None,
        Ok(Value::String(text)) if text == "media" => Some(Kind::Media),
        Ok(Value::String(text)) if text == "data" => Some(Kind::Data),
        _ => {
            problems.push("`generates` must be \"media\" or \"data\"".into());
            None
        },
    };
    match (&generate, generates) {
        (Some(_), None) if !problems.iter().any(|problem| problem.starts_with("`generates`")) => problems.push(
            "a plugin with `generate` must say what it generates: `generates = \"media\"` or `generates = \"data\"`"
                .into(),
        ),
        (None, Some(_)) => problems.push("`generates` is allowed only with `generate`".into()),
        _ => {},
    }
    if generate.is_some() && script.values.is_none() {
        problems.push(
            "a note type plugin cannot supply `generate`: declare it as a `[plugins]` instance in deck.toml".into(),
        );
    }
    let secrets = secret_names(&table, &mut problems);
    let mut read = IndexMap::new();
    if let Some((grants, pointer, root)) = &script.read {
        for (name, path) in grants.iter() {
            match crate::secrets::path(root, path) {
                Ok(dir) => {
                    read.insert(name.clone(), dir);
                },
                Err(message) => {
                    let (loc, _) = script.tree.nearest(&child(pointer, name));
                    found.push(
                        Diagnostic::error("plugin::filesystem", format!("{}: {message}", script.shown))
                            .with_source(script.tree.source.clone())
                            .with_label(loc.value.clone(), "here"),
                    );
                },
            }
        }
    }
    match &script.secrets {
        None if !secrets.is_empty() => problems
            .push("the plugin needs secrets, and only a `[plugins]` instance in deck.toml can supply them".into()),
        None => {},
        Some((configured, pointer)) => {
            for name in secrets.iter().filter(|name| !configured.contains_key(*name)) {
                problems.push(format!(
                    "the plugin needs the secret `{name}`: add it to `[plugins.{}.secrets]`",
                    script.name
                ));
            }
            for name in configured.keys().filter(|name| !secrets.contains(name)) {
                let (loc, _) = script.tree.nearest(&child(pointer, name));
                found.push(
                    Diagnostic::warning(
                        "plugin::unused-secret",
                        format!("{}: the plugin does not use the secret `{name}`", script.shown),
                    )
                    .with_source(script.tree.source.clone())
                    .with_label(loc.key.clone().unwrap_or_else(|| loc.value.clone()), "not used"),
                );
            }
        },
    }
    if script.values.is_none() && variables.fields.values().any(|spec| spec.required) {
        problems.push("the plugin needs variables, and only a `[plugins]` instance in deck.toml can set them".into());
    }
    for name in filters.keys().chain(functions.keys()) {
        match owners.get(name) {
            Some(owner) if owner.is_empty() => {
                problems.push(format!("`{name}` is a built-in filter or function of janky"))
            },
            Some(owner) => problems.push(format!("plugin `{owner}` already supplies `{name}`")),
            None => {
                owners.insert(name.clone(), script.name.clone());
            },
        }
    }
    if (script.secrets.as_ref().is_some_and(|(configured, _)| !configured.is_empty()) || script.http)
        && generate.is_none()
    {
        let (loc, _) = script.tree.nearest(&script.pointer);
        found.push(
            Diagnostic::warning(
                "plugin::unused-access",
                format!(
                    "{}: the plugin has no `generate` function, so it cannot use `http` or `secrets`",
                    script.shown
                ),
            )
            .with_source(script.tree.source.clone())
            .with_label(loc.value.clone(), "here"),
        );
    }
    if !problems.is_empty() {
        let more = problems.len().saturating_sub(PROBLEM_LIMIT);
        for problem in problems.into_iter().take(PROBLEM_LIMIT) {
            found.push(config_error("plugin::table", format!("{}: {problem}", script.shown)));
        }
        if more > 0 {
            found.push(config_error("plugin::table", format!("{}: {more} more problems", script.shown)));
        }
        return None;
    }
    Some(Plugin {
        name: script.name.clone(),
        id,
        source,
        chunk,
        filters,
        functions,
        variables,
        generate,
        generates,
        plan,
        prepare,
        secrets,
        instance: script.values.is_some(),
        env,
        values,
        read,
    })
}

/// Validates `[plugins.<name>.variables]` in `deck.toml` against the `variables` schema of each
/// plugin instance, and pushes a diagnostic with a position for each problem.
pub fn check_variables(project: &Project, plugins: &Plugins, diagnostics: &mut Diagnostics) {
    let instances: Vec<(&str, &ShortSchema)> = plugins
        .plugins
        .iter()
        .filter(|plugin| plugin.instance)
        .map(|plugin| (plugin.name.as_str(), &plugin.variables))
        .collect();
    if instances.is_empty() {
        return;
    }
    match jsonschema::validator_for(&crate::schema::variables_schema(&instances)) {
        Ok(validator) => diagnostics.append_sorted(project.deck_tree.schema_diagnostics(&validator, "variables")),
        Err(error) => diagnostics
            .push(Diagnostic::error("variables::schema", format!("cannot compile the variables schema: {error}"))),
    }
}

fn short_schema(value: &Value, problems: &mut Vec<String>) -> ShortSchema {
    let json = match crate::host::from_lua(value).map(|value| serde_json::to_value(&value)) {
        Ok(Ok(serde_json::Value::Array(items))) if items.is_empty() => serde_json::json!({}),
        Ok(Ok(json)) => json,
        Ok(Err(error)) => {
            problems.push(format!("`variables`: {error}"));
            return ShortSchema::default();
        },
        Err(error) => {
            problems.push(format!("`variables`: {error}"));
            return ShortSchema::default();
        },
    };
    let validator = jsonschema::validator_for(&crate::config::schema_for::<ShortSchema>()).expect("schema compiles");
    let before = problems.len();
    for error in validator.iter_errors(&json).take(PROBLEM_LIMIT) {
        let path = display_path(error.instance_path().as_str());
        let place = if path.is_empty() { "variables".to_string() } else { format!("variables.{path}") };
        problems.push(format!("`{place}`: {error}"));
    }
    if problems.len() > before {
        return ShortSchema::default();
    }
    match serde_json::from_value::<ShortSchema>(json) {
        Ok(short) => {
            for (name, spec) in &short.fields {
                spec_problems(spec, &format!("variables.{name}"), &short, problems);
            }
            for (def, fields) in &short.defs {
                for (name, spec) in fields {
                    spec_problems(spec, &format!("variables.defs.{def}.{name}"), &short, problems);
                }
            }
            short
        },
        Err(error) => {
            problems.push(format!("`variables`: {error}"));
            ShortSchema::default()
        },
    }
}

fn spec_problems(spec: &FieldSpec, path: &str, short: &ShortSchema, problems: &mut Vec<String>) {
    use crate::config::notetype::FieldKind;
    let rules = [
        (spec.values.is_some(), spec.kind == FieldKind::Enum, "values", "enum"),
        (spec.of.is_some(), spec.kind == FieldKind::List, "of", "list"),
        (spec.schema.is_some(), spec.kind == FieldKind::Object, "schema", "object"),
        (spec.fields.is_some(), spec.kind == FieldKind::Object, "fields", "object"),
    ];
    for (present, allowed, key, kind) in rules {
        if present && !allowed {
            problems.push(format!("`{path}.{key}` is only for `{kind}`"));
        }
    }
    match (spec.kind, &spec.schema, &spec.fields) {
        (FieldKind::Enum, _, _) if spec.values.is_none() => {
            problems.push(format!("`{path}`: an `enum` needs `values`"))
        },
        (FieldKind::List, _, _) if spec.of.is_none() => problems.push(format!("`{path}`: a `list` needs `of`")),
        (FieldKind::Object, None, None) => problems.push(format!("`{path}`: an `object` needs `schema` or `fields`")),
        (FieldKind::Object, Some(_), Some(_)) => {
            problems.push(format!("`{path}`: an `object` has `schema` or `fields`, not both"))
        },
        (FieldKind::Object, Some(name), None) if !short.defs.contains_key(name) => {
            problems.push(format!("`{path}.schema`: `{name}` is not in `variables.defs`"))
        },
        _ => {},
    }
    if let Some(of) = &spec.of {
        spec_problems(of, &format!("{path}.of"), short, problems);
    }
    for (name, field) in spec.fields.iter().flatten() {
        spec_problems(field, &format!("{path}.fields.{name}"), short, problems);
    }
}

/// Sets `janky.variables` in `env` to a new Lua copy of `values`. Does nothing when `env` has
/// no `janky` table.
pub(crate) fn set_variables(sandbox: &Sandbox, env: &Table, values: &minijinja::Value) -> mlua::Result<()> {
    if let Value::Table(janky) = env.raw_get("janky")? {
        janky.raw_set("variables", crate::host::to_lua(&sandbox.lua, values)?)?;
    }
    Ok(())
}

/// The full diagnostic of the last plugin call that failed during rendering.
///
/// A minijinja error can carry only a message. So a failed call stores its diagnostic here, and
/// the renderer takes it out to report the error with its position in the script.
pub type Failure = Arc<std::sync::Mutex<Option<Diagnostic>>>;

/// The warnings and infos of cached data results that `enrich` met while a template rendered,
/// each with the job (`instance: input`), the severity and the text. The renderer takes them
/// out after each template and shows them at that template.
pub type Warnings = Arc<Mutex<Vec<(String, Severity, String)>>>;

/// The template function of media jobs: it takes the input and the plugin instance, and gives
/// back a media value.
pub type MediaJob = Arc<dyn Fn(minijinja::Value, String) -> Result<minijinja::Value, minijinja::Error> + Send + Sync>;

/// The most bytes of cached data values, as JSON, that one render loads for `enrich`.
pub const ENRICHED_LIMIT: usize = 256 * 1024 * 1024;

/// The cached data results that one render loaded, by instance and hash, converted once for
/// the templates, with their warnings. `warned` holds the jobs whose warning was already given.
#[derive(Default)]
struct LoadedData {
    values: HashMap<(String, String), Option<Enriched>>,
    warned: std::collections::HashSet<(String, String)>,
    bytes: usize,
}

type Loaded = Arc<Mutex<LoadedData>>;

/// A cached data value converted for the templates, and its warning and info.
type Enriched = (minijinja::Value, Vec<(Severity, String)>);

/// Adds the filters and functions of all plugins to `env`.
///
/// A filter gets the filtered value as its first argument. Each call runs under the limits of
/// the sandbox. When the call fails, or returns a value that [`crate::host::from_lua`] cannot
/// convert, the diagnostic goes into `failure` and the template gets an error.
pub fn register(env: &mut minijinja::Environment<'static>, plugins: &Plugins, failure: &Failure) {
    for plugin in &plugins.plugins {
        for (kind, entries) in [("filter", &plugin.filters), ("function", &plugin.functions)] {
            for (name, function) in entries {
                let sandbox = plugins.sandbox.clone();
                let function = function.clone();
                let lua_env = plugin.env.clone();
                let values = plugin.values.clone();
                let source = plugin.source.clone();
                let chunk = plugin.chunk.clone();
                let failure = failure.clone();
                let label = format!("{}: {kind} `{name}`", plugin.name);
                let call = move |args: &[minijinja::Value]| -> Result<minijinja::Value, minijinja::Error> {
                    let fail = |diagnostic: Diagnostic| {
                        let message = diagnostic.message.clone();
                        *failure.lock().expect("plugin failure") = Some(diagnostic);
                        minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, message)
                    };
                    let result: Value = sandbox
                        .guarded(&label, || {
                            set_variables(&sandbox, &lua_env, &values)?;
                            let lua_args = args
                                .iter()
                                .map(|arg| crate::host::to_lua(&sandbox.lua, arg))
                                .collect::<mlua::Result<Vec<_>>>()?;
                            function.call(mlua::MultiValue::from_vec(lua_args))
                        })
                        .map_err(|error| fail(lua_error(&source, &chunk, &error)))?;
                    crate::host::from_lua(&result).map_err(|message| {
                        fail(
                            Diagnostic::error(
                                "plugin::result",
                                format!("{label} returned a value janky cannot use: {message}"),
                            )
                            .with_source(source.clone()),
                        )
                    })
                };
                if kind == "filter" {
                    env.add_filter(
                        name.clone(),
                        move |value: minijinja::Value, rest: minijinja::value::Rest<minijinja::Value>| {
                            let mut args = vec![value];
                            args.extend(rest.0);
                            call(&args)
                        },
                    );
                } else {
                    env.add_function(name.clone(), move |rest: minijinja::value::Rest<minijinja::Value>| call(&rest.0));
                }
            }
        }
    }
}

struct Generator {
    id: String,
    generates: Kind,
    plan: Option<Function>,
    env: Table,
    values: minijinja::Value,
    source: Arc<Source>,
    chunk: String,
}

/// Adds the `enrich` function and filter to `env`, `enrich(input, instance)` and
/// `input | enrich(instance)`, and gives back the function for media jobs, which the renderer
/// adds as the second form of `media`: `media(input, instance)` and `input | media(instance)`.
///
/// A call records a job in `jobs` and gives back a media value (`media`) or the cached data
/// as a map (`enrich`, an empty map before the job has run). It sends nothing. The input comes
/// from [`job_input`], and a job with the same kind, instance and input is recorded once. An
/// empty input records no job. The warning of a cached data result goes into `warnings` once
/// for each job. For a new
/// job, `plan` runs (if the plugin has one) and gives the cache key, the extension and the
/// billable units. Without `plan`, the cache key is the instance variables. The file name comes
/// from the plugin id, the cache key and the input. When the cache under `root` has no result
/// and `plan` gives no extension, the file has no name yet, and the media value prints as empty
/// text.
///
/// An instance that is not a plugin instance with `generate` is an error. A `plan` that fails
/// or returns a wrong result puts its diagnostic into `failure`.
pub fn register_generate(
    env: &mut minijinja::Environment<'static>,
    plugins: &Plugins,
    failure: &Failure,
    jobs: &Jobs,
    warnings: &Warnings,
    root: &std::path::Path,
) -> MediaJob {
    let root = root.to_path_buf();
    let loaded: Loaded = Arc::default();
    let loading = loaded.clone();
    let generators: Arc<HashMap<String, Generator>> = Arc::new(
        plugins
            .plugins
            .iter()
            .filter(|plugin| plugin.instance && plugin.generate.is_some())
            .map(|plugin| {
                let generator = Generator {
                    id: plugin.id.clone(),
                    generates: plugin.generates.unwrap_or(Kind::Media),
                    plan: plugin.plan.clone(),
                    env: plugin.env.clone(),
                    values: plugin.values.clone(),
                    source: plugin.source.clone(),
                    chunk: plugin.chunk.clone(),
                };
                (plugin.name.clone(), generator)
            })
            .collect(),
    );
    let sandbox = plugins.sandbox.clone();
    let failure = failure.clone();
    let jobs = jobs.clone();
    let record = Arc::new(
        move |kind: Kind, input: minijinja::Value, instance: String| -> Result<Option<Job>, minijinja::Error> {
            let Some(generator) = generators.get(&instance) else {
                return Err(minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("`{instance}` is not a plugin instance with a `generate` function"),
                ));
            };
            if generator.generates != kind {
                let (makes, call) = match generator.generates {
                    Kind::Media => ("media files", "media"),
                    Kind::Data => ("data", "enrich"),
                };
                return Err(minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("`{instance}` generates {makes}, so call it with `{call}`"),
                ));
            }
            let input = job_input(&input)
                .map_err(|message| minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, message))?;
            let Some(input) = input else { return Ok(None) };
            let text = input.to_string();
            if let Some(job) = jobs.lock().expect("jobs").get(&(kind, instance.clone(), text.clone())) {
                return Ok(Some(job.clone()));
            }
            let fail = |diagnostic: Diagnostic| {
                let message = diagnostic.message.clone();
                *failure.lock().expect("plugin failure") = Some(diagnostic);
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, message)
            };
            let planned = match &generator.plan {
                None => Planned::default(),
                Some(plan) => {
                    let label = format!("{instance}: `plan`");
                    let result: Value = sandbox
                        .guarded(&label, || {
                            set_variables(&sandbox, &generator.env, &generator.values)?;
                            let job = sandbox.lua.create_table()?;
                            let value = minijinja::Value::from(minijinja::value::Serde(&input));
                            job.raw_set("input", crate::host::to_lua(&sandbox.lua, &value)?)?;
                            job.raw_set("kind", kind.name())?;
                            plan.call((job, crate::host::to_lua(&sandbox.lua, &generator.values)?))
                        })
                        .map_err(|error| fail(lua_error(&generator.source, &generator.chunk, &error)))?;
                    let parsed = crate::host::lua_to_json(&result).and_then(parse_plan);
                    parsed.map_err(|message| {
                        fail(
                            Diagnostic::error("plugin::plan", format!("{label} returned a wrong result: {message}"))
                                .with_source(generator.source.clone()),
                        )
                    })?
                },
            };
            let key = canonical(&match planned.key {
                Some(key) => key,
                None => serde_json::to_value(&generator.values).unwrap_or_default(),
            });
            let hash = job_hash(&generator.id, kind, &key, &input);
            let (file, exists) = match kind {
                Kind::Media => {
                    let cached = crate::cache::lookup(&root, &hash, planned.ext.as_deref());
                    let exists = cached.is_some();
                    let file = match (cached, &planned.ext) {
                        (Some(file), _) => file,
                        (None, Some(ext)) => format!("{hash}.{ext}"),
                        (None, None) => String::new(),
                    };
                    (file, exists)
                },
                Kind::Data => {
                    let data = crate::cache::lookup_data(&root, &hash);
                    let mut loading = loading.lock().expect("enriched data");
                    if let Some(data) = &data {
                        loading.bytes += data.value.to_string().len();
                        if loading.bytes > ENRICHED_LIMIT {
                            return Err(minijinja::Error::new(
                                minijinja::ErrorKind::InvalidOperation,
                                format!("the enriched data of this project is larger than {ENRICHED_LIMIT} bytes"),
                            ));
                        }
                    }
                    let exists = data.is_some();
                    let entry = data.map(|data| {
                        let notes = [(Severity::Warning, data.warning), (Severity::Info, data.info)]
                            .into_iter()
                            .filter_map(|(severity, text)| text.map(|text| (severity, text)))
                            .collect();
                        (minijinja::Value::from(minijinja::value::Serde(&data.value)), notes)
                    });
                    loading.values.insert((instance.clone(), hash.clone()), entry);
                    (String::new(), exists)
                },
            };
            let job = Job {
                instance: instance.clone(),
                kind,
                input,
                hash,
                key,
                ext: planned.ext.filter(|_| kind == Kind::Media),
                units: planned.units,
                unit: planned.unit,
                file,
                exists,
            };
            jobs.lock().expect("jobs").insert((kind, instance, text), job.clone());
            Ok(Some(job))
        },
    );
    let generate = {
        let record = record.clone();
        Arc::new(move |input: minijinja::Value, instance: String| -> Result<minijinja::Value, minijinja::Error> {
            let job = record(Kind::Media, input, instance)?;
            Ok(minijinja::Value::from_object(Media {
                file: job.as_ref().map(|job| job.file.clone()).unwrap_or_default(),
                kind: "generated",
                exists: job.is_some_and(|job| job.exists),
            }))
        })
    };
    let warnings = warnings.clone();
    let enrich =
        Arc::new(move |input: minijinja::Value, instance: String| -> Result<minijinja::Value, minijinja::Error> {
            let empty = || minijinja::Value::from(minijinja::value::Serde(&serde_json::json!({})));
            let Some(job) = record(Kind::Data, input, instance)? else {
                return Ok(empty());
            };
            let mut loaded = loaded.lock().expect("enriched data");
            let key = (job.instance.clone(), job.hash.clone());
            let Some(Some((value, notes))) = loaded.values.get(&key) else {
                return Ok(empty());
            };
            let (value, notes) = (value.clone(), notes.clone());
            if !notes.is_empty() && loaded.warned.insert(key) {
                let job = format!("{}: {}", job.instance, job.input_text());
                let mut warnings = warnings.lock().expect("warnings");
                warnings.extend(notes.into_iter().map(|(severity, text)| (job.clone(), severity, text)));
            }
            Ok(value)
        });
    let function = enrich.clone();
    env.add_function("enrich", move |input: minijinja::Value, instance: String| function(input, instance));
    env.add_filter("enrich", move |input: minijinja::Value, instance: String| enrich(input, instance));
    generate
}

fn new_environment(sandbox: &Sandbox) -> mlua::Result<Table> {
    let env = sandbox.lua.create_table()?;
    for pair in sandbox.lua.globals().pairs::<Value, Value>() {
        let (key, value) = pair?;
        if matches!(&key, Value::String(name) if name == "_G") {
            continue;
        }
        env.raw_set(key, copy_value(&sandbox.lua, value)?)?;
    }
    env.raw_set("_G", env.clone())?;
    Ok(env)
}

fn copy_value(lua: &mlua::Lua, value: Value) -> mlua::Result<Value> {
    let Value::Table(table) = value else { return Ok(value) };
    let copy = lua.create_table()?;
    for entry in table.pairs::<Value, Value>() {
        let (name, item) = entry?;
        copy.raw_set(name, copy_value(lua, item)?)?;
    }
    Ok(Value::Table(copy))
}

fn named_functions(table: &Table, key: &str, problems: &mut Vec<String>) -> IndexMap<String, Function> {
    let mut out = IndexMap::new();
    match table.raw_get::<Value>(key) {
        Ok(Value::Nil) => {},
        Ok(Value::Table(entries)) => {
            let mut pairs: Vec<(String, Value)> = entries
                .pairs::<Value, Value>()
                .filter_map(Result::ok)
                .map(|(name, value)| match name {
                    Value::String(name) => (name.to_string_lossy(), value),
                    _ => (String::new(), value),
                })
                .collect();
            pairs.sort_by(|left, right| left.0.cmp(&right.0));
            for (name, value) in pairs {
                match value {
                    Value::Function(function) if is_name(&name) => {
                        out.insert(name, function);
                    },
                    Value::Function(_) => problems.push(format!("`{name}` in `{key}` is not a valid name")),
                    _ => problems.push(format!("`{key}.{name}` must be a function")),
                }
            }
        },
        _ => problems.push(format!("`{key}` must be a table of functions")),
    }
    out
}

fn optional_function(table: &Table, key: &str, problems: &mut Vec<String>) -> Option<Function> {
    match table.raw_get::<Value>(key) {
        Ok(Value::Function(function)) => Some(function),
        Ok(Value::Nil) => None,
        _ => {
            problems.push(format!("`{key}` must be a function"));
            None
        },
    }
}

fn secret_names(table: &Table, problems: &mut Vec<String>) -> Vec<String> {
    let mut names = Vec::new();
    match table.raw_get::<Value>("secrets") {
        Ok(Value::Nil) => {},
        Ok(Value::Table(list)) if list.raw_len() > SECRET_LIMIT => {
            problems.push(format!("`secrets` may list at most {SECRET_LIMIT} names"))
        },
        Ok(Value::Table(list)) if list.raw_len() == list.pairs::<Value, Value>().count() => {
            for index in 1..=list.raw_len() {
                match list.raw_get::<Value>(index) {
                    Ok(Value::String(name)) if is_name(&name.to_string_lossy()) => {
                        let name = name.to_string_lossy();
                        if names.contains(&name) {
                            problems.push(format!("`secrets` lists `{name}` twice"));
                        } else {
                            names.push(name);
                        }
                    },
                    _ => {
                        problems.push(format!("`secrets[{index}]` must be a name such as `api_key`"));
                        break;
                    },
                }
            }
        },
        _ => problems.push("`secrets` must be a list of names".into()),
    }
    names
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Turns a Lua error into a `plugin::syntax` or `plugin::runtime` diagnostic on `source`.
///
/// `chunk` is the short chunk name that Lua knows the script by. The message is the first line
/// of the error, without the chunk name and line. The label marks that line in the script. When
/// the message has no position (for example an error from a host function), the label marks the
/// first line of the traceback that is in the script. Without any position, there is no label.
pub fn lua_error(source: &Arc<Source>, chunk: &str, error: &mlua::Error) -> Diagnostic {
    let text = error.to_string();
    let first = text.lines().next().unwrap_or_default().to_string();
    let (code, rest) = if first.starts_with("syntax error") {
        ("plugin::syntax", first.trim_start_matches("syntax error: "))
    } else {
        ("plugin::runtime", first.trim_start_matches("runtime error: "))
    };
    let prefix = format!("{chunk}:");
    let place = |text: &str| -> Option<(usize, String)> {
        let (line, message) = text.strip_prefix(&prefix)?.split_once(':')?;
        Some((line.parse::<usize>().ok()?, message.trim().to_string()))
    };
    let located = place(rest);
    let diagnostic = match &located {
        Some((_, message)) => Diagnostic::error(code, message.clone()),
        None => Diagnostic::error(code, rest.to_string()),
    };
    let line = located
        .map(|(line, _)| line)
        .or_else(|| text.lines().skip(1).find_map(|frame| place(frame.trim())).map(|(line, _)| line));
    let diagnostic = diagnostic.with_source(source.clone());
    match line.and_then(|line| line_span(source.text(), line)) {
        Some(span) => diagnostic.with_label(span, "here"),
        None => diagnostic,
    }
}

fn line_span(text: &str, line: usize) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let mut start = 0;
    for _ in 1..line {
        let at = start + bytes.get(start..)?.iter().position(|byte| matches!(byte, b'\n' | b'\r'))?;
        let pair = matches!(bytes.get(at..at + 2), Some(b"\r\n" | b"\n\r"));
        start = at + if pair { 2 } else { 1 };
    }
    let rest = text.get(start..)?;
    let content = rest.split(['\n', '\r']).next().unwrap_or_default();
    let end = start + content.trim_end().len();
    Some((start + content.len() - content.trim_start().len()).min(end)..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugins_cannot_change_each_other() {
        let sandbox = Sandbox::new().unwrap();
        let attack = "_G.print = nil\nstring.upper = function() return 'hacked' end\n\
                      local ok, meta = pcall(getmetatable, '')\n\
                      if type(meta) == 'table' then meta.__index = { upper = function() return 'hacked' end } end\n\
                      tostring = nil\njanky.json.encode = nil\nreturn {}";
        sandbox.exec_in(attack, "attack", "attack.lua", new_environment(&sandbox).unwrap()).unwrap();
        let victim = "return tostring(print ~= nil) .. string.upper('a') .. ('b'):upper() .. type(tostring) .. \
                      type(janky.json.encode)";
        let value = sandbox.exec_in(victim, "victim", "victim.lua", new_environment(&sandbox).unwrap()).unwrap();
        assert_eq!(value.to_string().unwrap(), "trueABfunctionfunction");
        let global = sandbox.exec("return type(print) .. string.upper('c')", "janky.lua").unwrap();
        assert_eq!(global.to_string().unwrap(), "functionC");
    }

    #[test]
    fn line_spans_count_line_breaks_like_lua() {
        let text = "a\r\nb\rc\n\rd\n   \ne";
        let lines: Vec<_> = (1..=6).map(|line| line_span(text, line).map(|span| &text[span])).collect();
        assert_eq!(lines, [Some("a"), Some("b"), Some("c"), Some("d"), Some(""), Some("e")]);
        assert_eq!(line_span(text, 5).unwrap().len(), 0);
        assert_eq!(line_span(text, 7), None);
    }

    #[test]
    fn errors_from_host_functions_point_at_the_calling_line() {
        let sandbox = Sandbox::new().unwrap();
        let text = "local x = 1\nreturn janky.json.encode(0/0)\n";
        let source = Source::new("a-very-long-plugin-file-name-that-lua-would-shorten-in-messages.lua", text);
        let error = sandbox.exec_in(text, "plugin0", "x", new_environment(&sandbox).unwrap()).unwrap_err();
        let diagnostic = lua_error(&source, "plugin0", &error);
        assert_eq!(
            diagnostic.labels.first().map(|label| &text[label.span.clone()]),
            Some("return janky.json.encode(0/0)")
        );
    }
}
