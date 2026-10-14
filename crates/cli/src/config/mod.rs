//! The configuration files of a project, as serde types with a generated JSON Schema.
//!
//! [`deck`] has the types of `deck.toml` and [`notetype`] has the types of a note type file.
//! [`load`] reads one of these files in two passes. First the JSON Schema from
//! [`schema_for`] checks the parsed [`Tree`], so that every error has a file position. Then serde
//! converts the checked value to the Rust type.
//!
//! The schema is also the editor documentation: `janky schema` writes it to disk. A unit test
//! makes sure that every key in the schemas of `deck.toml`, the note type file and `deck.lock`
//! has a title and a description.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::diagnostic::{Diagnostic, Diagnostics, Source};
use crate::tree::Tree;

pub mod deck;
pub mod notetype;

/// Gives the JSON Schema of `T`, with `null` removed from every type.
///
/// An `Option` field becomes a key that can be absent but cannot be `null`. A `type` list with
/// one type left becomes a single type, and an `anyOf` with one schema left merges into its
/// parent.
pub fn schema_for<T: JsonSchema>() -> Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(T)).expect("schema serialises");
    strip_null(&mut schema);
    schema
}

/// Parses the TOML file in `source`, checks it against the schema of `T` and converts it.
///
/// It returns the value together with its [`Tree`], so that later checks can point at a key.
/// It returns `None` after it pushes diagnostics for TOML errors, for schema errors (codes
/// that start with `config::`, sorted by position) or for a serde error (`config::invalid`,
/// without a label).
pub fn load<T: JsonSchema + DeserializeOwned>(source: Arc<Source>, diagnostics: &mut Diagnostics) -> Option<(T, Tree)> {
    let tree = Tree::parse_toml(source, diagnostics)?;
    let validator = jsonschema::validator_for(&schema_for::<T>()).expect("schema compiles");
    let found = tree.schema_diagnostics(&validator, "config");
    if !found.is_empty() {
        diagnostics.append_sorted(found);
        return None;
    }
    match serde_json::from_value(tree.value.clone()) {
        Ok(value) => Some((value, tree)),
        Err(error) => {
            diagnostics.push(Diagnostic::error("config::invalid", error.to_string()).with_source(tree.source.clone()));
            None
        },
    }
}

fn strip_null(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(types)) = map.get_mut("type") {
                types.retain(|t| t != "null");
                if types.len() == 1 {
                    let only = types[0].clone();
                    map.insert("type".into(), only);
                }
            }
            if let Some(Value::Array(any_of)) = map.get("anyOf") {
                let rest: Vec<Value> =
                    any_of.iter().filter(|schema| schema.get("type") != Some(&Value::from("null"))).cloned().collect();
                if rest.len() == 1 {
                    map.remove("anyOf");
                    if let Value::Object(only) = &rest[0] {
                        for (key, inner) in only {
                            map.insert(key.clone(), inner.clone());
                        }
                    }
                } else if rest.len() < any_of.len() {
                    map.insert("anyOf".into(), Value::Array(rest));
                }
            }
            map.values_mut().for_each(strip_null);
        },
        Value::Array(items) => items.iter_mut().for_each(strip_null),
        _ => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn undocumented(value: &Value, path: &str, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(Value::Object(properties)) = map.get("properties") {
                    for (key, property) in properties {
                        let documented = ["title", "description"].iter().all(|word| property.get(word).is_some());
                        if !documented {
                            out.push(format!("{path}/{key}"));
                        }
                    }
                }
                for (key, inner) in map {
                    undocumented(inner, &format!("{path}/{key}"), out);
                }
            },
            Value::Array(items) => items.iter().for_each(|item| undocumented(item, path, out)),
            _ => {},
        }
    }

    #[test]
    fn every_key_has_a_title_and_a_description() {
        let mut out = Vec::new();
        undocumented(&schema_for::<deck::DeckFile>(), "deck", &mut out);
        undocumented(&schema_for::<notetype::NotetypeFile>(), "notetype", &mut out);
        undocumented(&schema_for::<crate::lock::LockFile>(), "lock", &mut out);
        assert_eq!(out, Vec::<String>::new());

        let deck = schema_for::<deck::DeckFile>();
        let output = &deck["$defs"]["RootDeck"]["properties"]["output"];
        assert_eq!(output["title"], "Package path");
        assert!(output["description"].as_str().unwrap().starts_with("Where `janky build` writes"));
        let instance = &deck["$defs"]["PluginInstance"]["properties"];
        assert_eq!(instance["http"]["title"], "Network access");
        assert_eq!(instance["secrets"]["title"], "Secrets");
    }
}
