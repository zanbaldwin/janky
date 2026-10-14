//! Compiles the short form of a schema ([`ShortSchema`]) to JSON Schema 2020-12.
//!
//! [`file_schema`] builds the schema of a content file from all note types of a project. Each
//! note type becomes an `if`/`then` branch in `allOf`, chosen by the entry `type`.
//! [`variables_schema`] builds the schema of the `variables` tables of plugin instances in
//! `deck.toml`.
//!
//! Each [`FieldKind`] maps to one JSON Schema type. A `defs` definition goes into `$defs` under
//! a prefix (`notetype.<key>.` or `plugin.<name>.`), so names from two sources cannot clash.

use indexmap::IndexMap;
use serde_json::{Map, Value, json};

use crate::config::notetype::{FieldKind, FieldSpec, SchemaSource, ShortSchema};
use crate::project::Project;

/// The pattern of a `slug` value and of an entry `id`: lowercase ASCII letters and digits in
/// groups joined by single hyphens.
pub const SLUG: &str = "^[a-z0-9]+(-[a-z0-9]+)*$";

/// The JSON Schema of a content file of `project`, with `entries` (required) and `defaults`
/// (any table).
///
/// An entry must have a `type` that is an entry type of one of the note types. If a note type
/// has `default = true`, an entry without `type` uses that note type's branch instead. Each
/// branch checks the entry against the note type's schema.
///
/// An inline or absent note type schema compiles to an object that allows `id` (always
/// required, a [`SLUG`] unless the short form defines `id`), `type`, `tags` and the short-form
/// keys. A JSON Schema file goes into `$defs` as it is. It gets an `$id` of
/// `urn:janky:notetype:<key>` if it has none.
pub fn file_schema(project: &Project) -> Value {
    let mut defs = Map::new();
    let mut all_types = Vec::new();
    let mut branches = Vec::new();
    let mut has_default = false;
    for (key, notetype) in &project.notetypes {
        let reference = &project.deck.notetypes[key];
        let entry_types = reference.entry_types(key);
        all_types.extend(entry_types.iter().cloned());
        let name = format!("notetype.{key}");
        let target = match (&notetype.file.schema, &notetype.schema_file) {
            (Some(SchemaSource::File(_)), Some(file)) => {
                let mut file = file.clone();
                let id = match file.get("$id").and_then(Value::as_str) {
                    Some(id) => id.to_string(),
                    None => format!("urn:janky:notetype:{key}"),
                };
                file["$id"] = Value::String(id.clone());
                defs.insert(name, file);
                json!({ "$ref": id })
            },
            (source, _) => {
                let empty = ShortSchema::default();
                let short = match source {
                    Some(SchemaSource::Inline(short)) => short,
                    _ => &empty,
                };
                let entry = entry_schema(key, short, &entry_types, &mut defs);
                defs.insert(name.clone(), entry);
                json!({ "$ref": format!("#/$defs/{name}") })
            },
        };
        let mut condition = json!({ "properties": { "type": { "enum": entry_types } }, "required": ["type"] });
        if reference.default {
            has_default = true;
            condition = json!({ "anyOf": [condition, { "not": { "required": ["type"] } }] });
        }
        branches.push(json!({ "if": condition, "then": target }));
    }
    let mut entry = json!({
        "type": "object",
        "properties": { "type": { "enum": all_types } },
        "allOf": branches,
    });
    if !has_default {
        entry["required"] = json!(["type"]);
    }
    defs.insert("entry".into(), entry);
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "defaults": { "type": "object" },
            "entries": { "type": "array", "items": { "$ref": "#/$defs/entry" } },
        },
        "required": ["entries"],
        "additionalProperties": false,
        "$defs": defs,
    })
}

fn entry_schema(key: &str, short: &ShortSchema, types: &[String], defs: &mut Map<String, Value>) -> Value {
    let prefix = format!("notetype.{key}");
    for (def, fields) in &short.defs {
        defs.insert(format!("{prefix}.{def}"), object(&prefix, fields));
    }
    let mut entry = object(&prefix, &short.fields);
    if !short.fields.contains_key("id") {
        entry["properties"]["id"] = json!({ "type": "string", "pattern": SLUG });
    }
    entry["properties"]["type"] = json!({ "enum": types });
    entry["properties"]["tags"] = json!({ "type": "array", "items": { "type": "string" } });
    if let Some(required) = entry["required"].as_array_mut()
        && !required.contains(&json!("id"))
    {
        required.insert(0, json!("id"));
    }
    entry
}

/// The JSON Schema of the `variables` tables of the plugin instances, keyed by instance name.
///
/// The result is a fragment for `deck.toml`. It only describes `plugins.<name>.variables`.
/// A `variables` table is required if its schema has a required key. An instance whose
/// schema has no keys accepts no variables.
pub fn variables_schema(instances: &[(&str, &ShortSchema)]) -> Value {
    let mut defs = Map::new();
    let mut properties = Map::new();
    for (name, short) in instances {
        let prefix = format!("plugin.{name}");
        for (def, fields) in &short.defs {
            defs.insert(format!("{prefix}.{def}"), object(&prefix, fields));
        }
        let mut instance = json!({ "properties": { "variables": object(&prefix, &short.fields) } });
        if short.fields.values().any(|spec| spec.required) {
            instance["required"] = json!(["variables"]);
        }
        properties.insert(name.to_string(), instance);
    }
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "properties": { "plugins": { "properties": properties } },
        "$defs": defs,
    })
}

fn object(key: &str, fields: &IndexMap<String, FieldSpec>) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (name, spec) in fields {
        if spec.required {
            required.push(Value::String(name.clone()));
        }
        properties.insert(name.clone(), field(key, spec));
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn field(key: &str, spec: &FieldSpec) -> Value {
    match spec.kind {
        FieldKind::String | FieldKind::Markdown | FieldKind::Media => json!({ "type": "string" }),
        FieldKind::Slug => json!({ "type": "string", "pattern": SLUG }),
        FieldKind::Int => json!({ "type": "integer" }),
        FieldKind::Float => json!({ "type": "number" }),
        FieldKind::Bool => json!({ "type": "boolean" }),
        FieldKind::Enum => json!({ "enum": spec.values.clone().unwrap_or_default() }),
        FieldKind::List => match &spec.of {
            Some(of) => json!({ "type": "array", "items": field(key, of) }),
            None => json!({ "type": "array" }),
        },
        FieldKind::Object => match (&spec.schema, &spec.fields) {
            (Some(def), _) => json!({ "$ref": format!("#/$defs/{key}.{def}") }),
            (None, Some(fields)) => object(key, fields),
            (None, None) => json!({ "type": "object" }),
        },
    }
}
