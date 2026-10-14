//! The serde types of a note type file (`notetype.toml` or a file that `path` names).
//!
//! [`NotetypeFile`] is the whole file. Its `[fields.<key>]` tables are [`Field`] values and its
//! `[cards.<key>]` tables are [`Card`] values. Paths in the file are relative to its directory.
//!
//! The `schema` key is a [`SchemaSource`]: the path of a JSON Schema file, or a [`ShortSchema`]
//! table in the short form. Each key of the short form is a [`FieldSpec`], and its `type` is a
//! [`FieldKind`]. Plugins use the same short form for their `variables` schema.
//! [`crate::schema`] compiles the short form to JSON Schema.
//!
//! The types that derive [`JsonSchema`], and their enum variants, have no doc comments on
//! purpose. schemars would copy a doc comment into the JSON Schema. A field has a doc comment
//! only if its `#[schemars]` attribute sets both `title` and `description`.

use std::borrow::Cow;

use indexmap::IndexMap;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotetypeFile {
    #[schemars(title = r#"Note type name"#, description = r#"The note type name that Anki shows."#)]
    pub name: String,
    /// A key of [`fields`](Self::fields). `None` uses the first field.
    #[schemars(
        title = r#"Sort field"#,
        description = r#"The key of the field that Anki shows and sorts by in the browser. Default: the first field."#
    )]
    pub sort_field: Option<String>,
    #[schemars(
        title = r#"CSS file"#,
        description = r#"The CSS of the note type, relative to this file's directory. Default: no CSS."#
    )]
    pub css: Option<String>,
    #[schemars(
        title = r#"Script assets"#,
        description = r#"JavaScript files that card templates include with `<script src="<file name>"></script>`. janky puts their content inline. A file must not contain `</script>`."#
    )]
    #[serde(default)]
    pub assets: Vec<String>,
    #[schemars(
        title = r#"Plugins"#,
        description = r#"Lua scripts that supply filters and functions for this note type, relative to this file's directory. They get no network, secrets or file access."#
    )]
    #[serde(default)]
    pub plugins: Vec<String>,
    /// `None` allows only `id`, `type` and `tags`.
    #[schemars(
        title = r#"Content schema"#,
        description = r#"The keys that an entry of this note type can have: a table in the short form, or the path of a JSON Schema file. Default: only `id`, `type` and `tags`."#
    )]
    pub schema: Option<SchemaSource>,
    /// Keyed by the field key, in the order of the Anki note type.
    #[schemars(
        title = r#"Fields"#,
        description = r#"The fields of the Anki note type, in this order. At least one."#
    )]
    #[schemars(extend("minProperties" = 1))]
    pub fields: IndexMap<String, Field>,
    /// Keyed by the card key, in the order of the Anki note type.
    #[schemars(
        title = r#"Card types"#,
        description = r#"The card types of the note type, in this order. At least one."#
    )]
    #[schemars(extend("minProperties" = 1))]
    pub cards: IndexMap<String, Card>,
}

/// The `schema` key of a note type file.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SchemaSource {
    /// The path of a JSON Schema file, relative to the note type file's directory.
    File(String),
    /// A table in the short form.
    Inline(ShortSchema),
}

impl JsonSchema for SchemaSource {
    fn schema_name() -> Cow<'static, str> {
        "SchemaSource".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let inline = generator.subschema_for::<ShortSchema>();
        json_schema!({ "if": { "type": "string" }, "else": inline })
    }
}

/// A schema in the short form: one [`FieldSpec`] for each key, plus named object definitions.
///
/// The default value defines no keys. A key called `defs` always holds the definitions, so an
/// entry key cannot have that name.
#[derive(Debug, Default)]
pub struct ShortSchema {
    /// Every key of the table except `defs`, in file order.
    pub fields: IndexMap<String, FieldSpec>,
    /// The `defs` table: object definitions that a [`FieldSpec`] names in its `schema` key.
    pub defs: IndexMap<String, IndexMap<String, FieldSpec>>,
}

impl<'de> Deserialize<'de> for ShortSchema {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut raw = IndexMap::<String, Value>::deserialize(deserializer)?;
        let defs = match raw.shift_remove("defs") {
            Some(defs) => serde_json::from_value(defs).map_err(D::Error::custom)?,
            None => IndexMap::new(),
        };
        let fields = raw
            .into_iter()
            .map(|(name, spec)| serde_json::from_value(spec).map(|spec| (name, spec)))
            .collect::<Result<_, _>>()
            .map_err(D::Error::custom)?;
        Ok(Self { fields, defs })
    }
}

impl JsonSchema for ShortSchema {
    fn schema_name() -> Cow<'static, str> {
        "ShortSchema".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let field = generator.subschema_for::<FieldSpec>();
        json_schema!({
            "type": "object",
            "properties": {
                "defs": {
                    "title": "Definitions",
                    "description": "Object definitions that `object` keys name with `schema = \"<name>\"`.",
                    "type": "object",
                    "additionalProperties": { "type": "object", "additionalProperties": field },
                },
            },
            "additionalProperties": field,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    /// The `type` key.
    #[schemars(
        title = r#"Type"#,
        description = r#"`string`, `slug`, `markdown`, `int`, `float`, `bool`, `enum`, `media`, `list` or `object`."#
    )]
    #[serde(rename = "type")]
    pub kind: FieldKind,
    #[schemars(
        title = r#"Required"#,
        description = r#"The entry must have this key. Default: false."#
    )]
    #[serde(default)]
    pub required: bool,
    #[schemars(
        title = r#"Enum values"#,
        description = r#"The allowed strings. Required for `enum`, and only for `enum`."#
    )]
    pub values: Option<Vec<String>>,
    #[schemars(
        title = r#"List item type"#,
        description = r#"The type of the items. Required for `list`, and only for `list`."#
    )]
    pub of: Option<Box<FieldSpec>>,
    #[schemars(
        title = r#"Object definition"#,
        description = r#"The name of a `[schema.defs.<name>]` table. For `object` only, instead of `fields`."#
    )]
    pub schema: Option<String>,
    /// Used only when [`schema`](Self::schema) is `None`.
    #[schemars(
        title = r#"Object keys"#,
        description = r#"The keys of an inline object. For `object` only, instead of `schema`."#
    )]
    pub fields: Option<IndexMap<String, FieldSpec>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    String,
    Slug,
    Markdown,
    Int,
    Float,
    Bool,
    Enum,
    Media,
    List,
    Object,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Field {
    #[schemars(
        title = r#"Field name"#,
        description = r#"The field name that Anki shows. Card templates use this name."#
    )]
    pub name: String,
    /// Without `value` and [`template`](Self::template), the field gets the entry key with the
    /// same name.
    #[schemars(
        title = r#"Inline template"#,
        description = r#"A minijinja template that computes the field value. Not with `template`. Without both, the field gets the entry key with the same name as text."#
    )]
    pub value: Option<String>,
    #[schemars(
        title = r#"Template file"#,
        description = r#"A minijinja template file, relative to this file's directory. Not with `value`."#
    )]
    pub template: Option<String>,
    #[schemars(
        title = r#"Editor description"#,
        description = r#"A description that the Anki editor shows for the field."#
    )]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Card {
    #[schemars(title = r#"Card type name"#, description = r#"The card type name that Anki shows."#)]
    pub name: String,
    #[schemars(
        title = r#"Front template"#,
        description = r#"The question side: an Anki template file, relative to this file's directory."#
    )]
    pub front: String,
    #[schemars(
        title = r#"Back template"#,
        description = r#"The answer side: an Anki template file, relative to this file's directory."#
    )]
    pub back: String,
    #[schemars(
        title = r#"Condition"#,
        description = r#"A minijinja expression. The card exists only for entries where it is true. janky adds a hidden field `_make_<card key>` for it."#
    )]
    pub when: Option<String>,
}
