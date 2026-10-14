//! Parses YAML, TOML and JSON files to a [`Value`], with a source span for each part.
//!
//! A [`Tree`] holds the parsed value and a map from each JSON Pointer in it to a [`Loc`]. JSON
//! Schema errors name a JSON Pointer, so [`Tree::schema_diagnostics`] can turn them into
//! diagnostics with a label at the right key or value.
//!
//! All spans are byte ranges into the source text. saphyr gives character positions, so the
//! YAML parser converts them.
//!
//! The YAML parser uses the core schema: `yes` and dates stay strings. It rejects anchors,
//! aliases, keys that are not strings and a second document. It ignores YAML tags.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use serde_json::{Map, Number, Value};
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use jsonc_parser::ast::{self, ObjectPropName};
use jsonc_parser::{CollectOptions, ParseOptions};
use jsonschema::JsonType;
use jsonschema::error::{TypeKind, ValidationErrorKind};
use saphyr::{LoadableYamlNode, MarkedYaml, Marker, Scalar, YamlData};

use crate::diagnostic::{Diagnostic, Diagnostics, Source};

/// The source spans of one value in a [`Tree`].
#[derive(Debug, Clone)]
pub struct Loc {
    /// The span of the key in its table. `None` for the root and for array items.
    pub key: Option<Range<usize>>,
    /// The span of the value. A label on a large value uses [`Tree::first_line`] to stay short.
    pub value: Range<usize>,
}

/// A parsed file: its value and the source span of each part of it.
#[derive(Debug)]
pub struct Tree {
    /// The file that the spans point into.
    pub source: Arc<Source>,
    /// The parsed value. TOML datetimes are strings.
    pub value: Value,
    /// Keyed by JSON Pointer. The root pointer (`""`) is always present.
    pub spans: HashMap<String, Loc>,
}

/// Appends `segment` to `pointer`, escaped as RFC 6901 requires (`~` to `~0`, `/` to `~1`).
pub fn child(pointer: &str, segment: &str) -> String {
    format!("{pointer}/{}", segment.replace('~', "~0").replace('/', "~1"))
}

/// Turns a JSON Pointer into the dotted path that diagnostics show, such as `entries[0].symbol`.
///
/// A segment that is a number shows as an index. A segment with characters other than ASCII
/// letters, digits, `_` and `-` shows in quotes. The root pointer gives an empty string.
pub fn display_path(pointer: &str) -> String {
    let mut out = String::new();
    for segment in pointer.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        if segment.parse::<usize>().is_ok() {
            out.push_str(&format!("[{segment}]"));
            continue;
        }
        if !out.is_empty() {
            out.push('.');
        }
        if !segment.is_empty() && segment.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            out.push_str(&segment);
        } else {
            out.push_str(&format!("{segment:?}"));
        }
    }
    out
}

impl Tree {
    /// Parses a TOML file. A TOML datetime becomes a string.
    ///
    /// It returns `None` after it pushes `toml::syntax` errors, or `toml::number` errors for an
    /// integer outside `i64` or an infinite or NaN float.
    pub fn parse_toml(source: Arc<Source>, diagnostics: &mut Diagnostics) -> Option<Self> {
        let (document, errors) = DeTable::parse_recoverable(source.text());
        if !errors.is_empty() {
            for error in errors {
                let span = error.span().unwrap_or(0..0);
                diagnostics.push(
                    Diagnostic::error("toml::syntax", error.message().trim())
                        .with_source(source.clone())
                        .with_label(span, "here"),
                );
            }
            return None;
        }
        let mut tree = Self {
            source: source.clone(),
            value: Value::Null,
            spans: HashMap::new(),
        };
        tree.spans.insert(String::new(), Loc { key: None, value: document.span() });
        let mut found = Vec::new();
        tree.value = tree.table(document.get_ref(), "", &mut found);
        let valid = found.is_empty();
        for diagnostic in found {
            diagnostics.push(diagnostic);
        }
        valid.then_some(tree)
    }

    /// Parses a YAML file that holds at most one document.
    ///
    /// On a syntax error it pushes one `yaml::syntax` or `yaml::duplicate-key` error and returns
    /// `None`. It also returns `None` after it pushes errors for a second document, a key that is
    /// not a string, an infinite or NaN number, or an unsupported construct (`yaml::unsupported`).
    /// An empty file gives a null value.
    pub fn parse_yaml(source: Arc<Source>, diagnostics: &mut Diagnostics) -> Option<Self> {
        let offsets = CharOffsets::new(source.text());
        let documents = match MarkedYaml::load_from_str(source.text()) {
            Ok(documents) => documents,
            Err(error) => {
                let at = offsets.at(error.marker());
                let code = if error.info().contains("duplicated key") { "yaml::duplicate-key" } else { "yaml::syntax" };
                diagnostics
                    .push(Diagnostic::error(code, error.info()).with_source(source.clone()).with_label(at..at, "here"));
                return None;
            },
        };
        let mut tree = Self {
            source: source.clone(),
            value: Value::Null,
            spans: HashMap::new(),
        };
        let mut found = Vec::new();
        if let Some(extra) = documents.get(1) {
            let at = offsets.at(&extra.span.start);
            found.push(
                Diagnostic::error("yaml::documents", "a content file holds one YAML document")
                    .with_source(source.clone())
                    .with_label(at..at, "second document"),
            );
        }
        if let Some(document) = documents.first() {
            tree.value = tree.yaml(document, "", None, &offsets, &mut found);
        } else {
            tree.spans.insert(String::new(), Loc { key: None, value: 0..0 });
        }
        let valid = found.is_empty();
        for diagnostic in found {
            diagnostics.push(diagnostic);
        }
        valid.then_some(tree)
    }

    /// Parses a strict JSON file: no comments, trailing commas or other JSON5 extensions.
    ///
    /// On a syntax error it pushes one `json::syntax` error and returns `None`. A number out of
    /// range pushes `json::number` and also gives `None`. A duplicate key pushes
    /// `json::duplicate-key` but still gives a tree, which keeps the first value of the key.
    /// An empty file gives a null value.
    pub fn parse_json(source: Arc<Source>, diagnostics: &mut Diagnostics) -> Option<Self> {
        let strict = ParseOptions {
            allow_comments: false,
            allow_loose_object_property_names: false,
            allow_trailing_commas: false,
            allow_missing_commas: false,
            allow_single_quoted_strings: false,
            allow_hexadecimal_numbers: false,
            allow_unary_plus_numbers: false,
            allow_bare_decimal_point_numbers: false,
            allow_non_finite_numbers: false,
            allow_extended_string_escapes: false,
        };
        let parsed = match jsonc_parser::parse_to_ast(source.text(), &CollectOptions::default(), &strict) {
            Ok(parsed) => parsed,
            Err(error) => {
                let range = error.range();
                diagnostics.push(
                    Diagnostic::error("json::syntax", error.kind().to_string())
                        .with_source(source.clone())
                        .with_label(range.start..range.end, "here"),
                );
                return None;
            },
        };
        let mut tree = Self {
            source: source.clone(),
            value: Value::Null,
            spans: HashMap::new(),
        };
        let mut found = Vec::new();
        match &parsed.value {
            Some(value) => tree.value = tree.json(value, "", None, &mut found),
            None => {
                tree.spans.insert(String::new(), Loc { key: None, value: 0..0 });
            },
        }
        let valid = found.iter().all(|diagnostic| diagnostic.code == "json::duplicate-key");
        for diagnostic in found {
            diagnostics.push(diagnostic);
        }
        valid.then_some(tree)
    }

    /// The spans of the value at `pointer`, if the file has that value.
    pub fn loc(&self, pointer: &str) -> Option<&Loc> {
        self.spans.get(pointer)
    }

    /// The spans of the value at `pointer`, or of its nearest ancestor that the file has.
    ///
    /// The `bool` is `true` if the spans belong to `pointer` itself.
    pub fn nearest(&self, pointer: &str) -> (&Loc, bool) {
        let mut current = pointer;
        loop {
            if let Some(loc) = self.spans.get(current) {
                return (loc, current.len() == pointer.len());
            }
            current = current.rfind('/').map_or("", |index| &current[..index]);
        }
    }

    /// Cuts `span` at the end of its first line, so that a label on a large table stays short.
    pub fn first_line(&self, span: &Range<usize>) -> Range<usize> {
        let text = self.source.text();
        let end = text[span.start..].find('\n').map_or(text.len(), |index| span.start + index);
        span.start..end.min(span.end.max(span.start))
    }

    /// Validates the value and turns each schema error into an error diagnostic.
    ///
    /// Each code is `<prefix>::` and the JSON Schema keyword in kebab case, such as
    /// `config::required` or `config::min-length`. An `additionalProperties` error gives one
    /// `<prefix>::unknown-key` diagnostic for each unknown key. A missing key gets a label on the
    /// first line of its parent, and an unknown key gets a label on the key. A YAML scalar that
    /// must be a string gets help that quotes it. A host name that does not match
    /// [`HOST_PATTERN`](crate::config::deck::HOST_PATTERN) gets its own message and help.
    pub fn schema_diagnostics(&self, validator: &jsonschema::Validator, prefix: &str) -> Diagnostics {
        let mut found = Diagnostics::default();
        for error in validator.iter_errors(&self.value) {
            for diagnostic in self.map_error(&error, prefix) {
                found.push(diagnostic);
            }
        }
        found
    }

    fn map_error(&self, error: &jsonschema::ValidationError, prefix: &str) -> Vec<Diagnostic> {
        let pointer = error.instance_path().as_str();
        let path = display_path(pointer);
        let diagnostic =
            |code: &str, message: String| Diagnostic::error(code, message).with_source(self.source.clone());
        match error.kind() {
            ValidationErrorKind::Required { property } => {
                let name = property.as_str().unwrap_or_default();
                let (loc, _) = self.nearest(pointer);
                let message = if path.is_empty() {
                    format!("missing key `{name}`")
                } else {
                    format!("missing key `{name}` in `{path}`")
                };
                vec![
                    diagnostic(&format!("{prefix}::required"), message)
                        .with_label(self.first_line(&loc.value), format!("`{name}` is missing")),
                ]
            },
            ValidationErrorKind::AdditionalProperties { unexpected } => unexpected
                .iter()
                .map(|key| {
                    let key_pointer = child(pointer, key);
                    let (loc, _) = self.nearest(&key_pointer);
                    let span = loc.key.clone().unwrap_or_else(|| loc.value.clone());
                    diagnostic(
                        &format!("{prefix}::unknown-key"),
                        format!("unknown key `{}`", display_path(&key_pointer)),
                    )
                    .with_label(span, "not allowed here")
                })
                .collect(),
            kind => {
                let (loc, exact) = self.nearest(pointer);
                let span = if exact { loc.value.clone() } else { self.first_line(&loc.value) };
                let host = matches!(kind, ValidationErrorKind::Pattern { pattern } if pattern == crate::config::deck::HOST_PATTERN);
                let message = if host {
                    format!("`{path}`: {} is not a host name", error.instance())
                } else if path.is_empty() {
                    error.to_string()
                } else {
                    format!("`{path}`: {error}")
                };
                let mut found = diagnostic(&format!("{prefix}::{}", kebab(kind.keyword())), message)
                    .with_label(span.clone(), "here");
                if host {
                    found = found
                        .with_help("use a name such as `api.example.com`, `*.example.com`, `localhost` or `127.0.0.1`");
                }
                if exact && let Some(help) = self.quote_hint(kind, pointer, &span) {
                    found = found.with_help(help);
                }
                vec![found]
            },
        }
    }

    fn quote_hint(&self, kind: &ValidationErrorKind, pointer: &str, span: &Range<usize>) -> Option<String> {
        let name = self.source.name();
        let ValidationErrorKind::Type { kind } = kind else { return None };
        let wants_string = match kind {
            TypeKind::Single(single) => *single == JsonType::String,
            TypeKind::Multiple(set) => set.contains(JsonType::String),
        };
        let scalar = self.value.pointer(pointer).is_some_and(|value| !value.is_array() && !value.is_object());
        let text = self.source.text().get(span.clone())?;
        let yaml = name.ends_with(".yaml") || name.ends_with(".yml");
        (yaml && wants_string && scalar && !text.is_empty())
            .then(|| format!("quote the value: '{}'", text.replace('\'', "''")))
    }

    fn table(&mut self, table: &DeTable, pointer: &str, found: &mut Vec<Diagnostic>) -> Value {
        let mut object = Map::new();
        for (key, value) in table.iter() {
            let path = child(pointer, key.get_ref());
            self.spans.insert(
                path.clone(),
                Loc {
                    key: Some(key.span()),
                    value: value.span(),
                },
            );
            object.insert(key.get_ref().to_string(), self.value(value, &path, found));
        }
        Value::Object(object)
    }

    fn value(&mut self, value: &Spanned<DeValue>, pointer: &str, found: &mut Vec<Diagnostic>) -> Value {
        match value.get_ref() {
            DeValue::String(text) => Value::String(text.to_string()),
            DeValue::Integer(integer) => match i64::from_str_radix(integer.as_str(), integer.radix()) {
                Ok(number) => Value::Number(number.into()),
                Err(_) => {
                    found.push(self.number_error("toml::number", value.span(), "integer out of range"));
                    Value::Null
                },
            },
            DeValue::Float(float) => match float.as_str().parse::<f64>().ok().and_then(Number::from_f64) {
                Some(number) => Value::Number(number),
                None => {
                    found.push(self.number_error(
                        "toml::number",
                        value.span(),
                        "infinite and NaN numbers are not allowed",
                    ));
                    Value::Null
                },
            },
            DeValue::Boolean(boolean) => Value::Bool(*boolean),
            DeValue::Datetime(datetime) => Value::String(datetime.to_string()),
            DeValue::Array(array) => Value::Array(
                array
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let path = format!("{pointer}/{index}");
                        self.spans.insert(path.clone(), Loc { key: None, value: item.span() });
                        self.value(item, &path, found)
                    })
                    .collect(),
            ),
            DeValue::Table(table) => self.table(table, pointer, found),
        }
    }

    fn yaml(
        &mut self,
        node: &MarkedYaml,
        pointer: &str,
        key: Option<Range<usize>>,
        offsets: &CharOffsets,
        found: &mut Vec<Diagnostic>,
    ) -> Value {
        let span = offsets.span(node);
        self.spans.insert(pointer.to_string(), Loc { key: key.clone(), value: span.clone() });
        match &node.data {
            YamlData::Value(scalar) => match scalar {
                Scalar::Null => Value::Null,
                Scalar::Boolean(boolean) => Value::Bool(*boolean),
                Scalar::Integer(integer) => Value::Number((*integer).into()),
                Scalar::FloatingPoint(float) => match Number::from_f64(float.into_inner()) {
                    Some(number) => Value::Number(number),
                    None => {
                        found.push(self.number_error("yaml::number", span, "infinite and NaN numbers are not allowed"));
                        Value::Null
                    },
                },
                Scalar::String(text) => Value::String(text.to_string()),
            },
            YamlData::Sequence(items) => Value::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| self.yaml(item, &format!("{pointer}/{index}"), None, offsets, found))
                    .collect(),
            ),
            YamlData::Mapping(mapping) => {
                let mut object = Map::new();
                for (key, value) in mapping.iter() {
                    let key_span = offsets.span(key);
                    let YamlData::Value(Scalar::String(name)) = &key.data else {
                        found.push(
                            Diagnostic::error("yaml::key", "a key must be a string")
                                .with_source(self.source.clone())
                                .with_label(key_span, "quote this key"),
                        );
                        continue;
                    };
                    let path = child(pointer, name);
                    let value = self.yaml(value, &path, Some(key_span), offsets, found);
                    object.insert(name.to_string(), value);
                }
                Value::Object(object)
            },
            YamlData::Tagged(_, inner) => self.yaml(inner, pointer, key, offsets, found),
            YamlData::Alias(_) | YamlData::Representation(..) | YamlData::BadValue => {
                found.push(
                    Diagnostic::error(
                        "yaml::unsupported",
                        "anchors, aliases and this YAML construct are not supported",
                    )
                    .with_source(self.source.clone())
                    .with_label(span, "here"),
                );
                Value::Null
            },
        }
    }

    fn json(
        &mut self,
        value: &ast::Value,
        pointer: &str,
        key: Option<Range<usize>>,
        found: &mut Vec<Diagnostic>,
    ) -> Value {
        let range = match value {
            ast::Value::StringLit(literal) => &literal.range,
            ast::Value::NumberLit(literal) => &literal.range,
            ast::Value::BooleanLit(literal) => &literal.range,
            ast::Value::Object(object) => &object.range,
            ast::Value::Array(array) => &array.range,
            ast::Value::NullKeyword(keyword) => &keyword.range,
        };
        let span = range.start..range.end;
        self.spans.insert(pointer.to_string(), Loc { key, value: span.clone() });
        match value {
            ast::Value::StringLit(literal) => Value::String(literal.value.to_string()),
            ast::Value::NumberLit(literal) => match serde_json::from_str::<Number>(literal.value) {
                Ok(number) => Value::Number(number),
                Err(_) => {
                    found.push(self.number_error("json::number", span, "number out of range"));
                    Value::Null
                },
            },
            ast::Value::BooleanLit(literal) => Value::Bool(literal.value),
            ast::Value::NullKeyword(_) => Value::Null,
            ast::Value::Array(array) => Value::Array(
                array
                    .elements
                    .iter()
                    .enumerate()
                    .map(|(index, element)| self.json(element, &format!("{pointer}/{index}"), None, found))
                    .collect(),
            ),
            ast::Value::Object(object) => {
                let mut map = Map::new();
                for property in &object.properties {
                    let (name, key_range) = match &property.name {
                        ObjectPropName::String(name) => (name.value.to_string(), &name.range),
                        ObjectPropName::Word(word) => (word.value.to_string(), &word.range),
                    };
                    let key_span = key_range.start..key_range.end;
                    if map.contains_key(&name) {
                        found.push(
                            Diagnostic::error("json::duplicate-key", format!("duplicate key `{name}`"))
                                .with_source(self.source.clone())
                                .with_label(key_span, "duplicate key"),
                        );
                        continue;
                    }
                    let path = child(pointer, &name);
                    let value = self.json(&property.value, &path, Some(key_span), found);
                    map.insert(name, value);
                }
                Value::Object(map)
            },
        }
    }

    fn number_error(&self, code: &str, span: Range<usize>, message: &str) -> Diagnostic {
        Diagnostic::error(code, message).with_source(self.source.clone()).with_label(span, "here")
    }
}

fn kebab(keyword: &str) -> String {
    let mut out = String::new();
    for c in keyword.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

struct CharOffsets(Vec<usize>);

impl CharOffsets {
    fn new(text: &str) -> Self {
        let mut offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
        offsets.push(text.len());
        Self(offsets)
    }

    fn at(&self, marker: &Marker) -> usize {
        self.0.get(marker.index()).copied().unwrap_or(*self.0.last().unwrap_or(&0))
    }

    fn span(&self, node: &MarkedYaml) -> Range<usize> {
        self.at(&node.span.start)..self.at(&node.span.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans<'a>(tree: &'a Tree, pointer: &str) -> (Option<&'a str>, &'a str) {
        let text = tree.source.text();
        let loc = tree.loc(pointer).unwrap();
        (loc.key.clone().map(|key| &text[key]), &text[loc.value.clone()])
    }

    fn parse(name: &str, text: &str) -> Tree {
        let mut diagnostics = Diagnostics::default();
        let source = Source::new(name, text);
        let tree = match name.rsplit('.').next() {
            Some("yaml") => Tree::parse_yaml(source, &mut diagnostics),
            Some("json") => Tree::parse_json(source, &mut diagnostics),
            _ => Tree::parse_toml(source, &mut diagnostics),
        };
        assert!(diagnostics.is_empty(), "{}", diagnostics.to_json_lines());
        tree.unwrap()
    }

    #[test]
    fn yaml_spans_after_emoji() {
        let tree =
            parse("a.yaml", "entries:\n  - { id: '⚛️', symbol: '👩‍🔬', n: 3 }\n  - id: '🇬🇧'\n    tags: ['🧪', '👩‍🔬']\n");
        assert_eq!(spans(&tree, "/entries/0/symbol"), (Some("symbol"), "'👩‍🔬'"));
        assert_eq!(spans(&tree, "/entries/0/n"), (Some("n"), "3"));
        assert_eq!(spans(&tree, "/entries/1/tags/1"), (None, "'👩‍🔬'"));
        assert_eq!(tree.value["entries"][0]["n"], 3);
    }

    #[test]
    fn yaml_core_schema_keeps_yes_and_dates_as_strings() {
        let tree = parse("a.yaml", "entries:\n  - id: yes\n    discovered: 1869-03-06\n    n: 012\n");
        assert_eq!(tree.value["entries"][0]["id"], "yes");
        assert_eq!(tree.value["entries"][0]["discovered"], "1869-03-06");
        assert_eq!(tree.value["entries"][0]["n"], 12);
    }

    #[test]
    fn toml_spans_after_emoji() {
        let tree = parse("a.toml", "[[entries]]\nid = '⚛️'\nexamples = [{ symbol = '👩‍🔬', en = 7 }]\n");
        assert_eq!(spans(&tree, "/entries/0/id"), (Some("id"), "'⚛️'"));
        assert_eq!(spans(&tree, "/entries/0/examples/0/en"), (Some("en"), "7"));
    }

    #[test]
    fn json_spans_after_emoji() {
        let tree = parse("a.json", r#"{ "entries": [ { "id": "⚛️", "symbol": "👩‍🔬", "tags": ["🧪"] } ] }"#);
        assert_eq!(spans(&tree, "/entries/0/symbol"), (Some(r#""symbol""#), r#""👩‍🔬""#));
        assert_eq!(spans(&tree, "/entries/0/tags/0"), (None, r#""🧪""#));
    }
}
