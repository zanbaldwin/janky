# The content schema

The **schema** of a note type lists the keys that an entry of this note type can have, and the
type of each value. janky checks every entry against it. A typo in a key name, a missing key or a
wrong value then gives a diagnostic with the file, line and column.

You write the schema in one of two ways:

- The `[schema]` table in `notetype.toml` (the short form).
- A separate JSON Schema file.

If a note type has no `schema` key, an entry can only have `id`, `type` and `tags`.

## The short form

Each key of the `[schema]` table is a key that an entry can have. Its value is a small table that
gives the type and the options. This is the schema of the tutorial's word note type:

```toml
[schema]
spanish = { type = "string", required = true }
english = { type = "string", required = true }
gender = { type = "enum", values = ["m", "f"] }
image = { type = "media" }
examples = { type = "list", of = { type = "string" } }
notes = { type = "markdown" }
production = { type = "bool" }
```

An entry of this note type can look like this (in YAML):

```yaml
- id: 'casa'
  spanish: 'casa'
  english: 'house'
  gender: 'f'
  image: 'images/casa.png'
  production: true
  examples:
    - 'Mi casa es pequeña.'
```

### Options

| Option | Type | Default | Meaning |
|--------|------|---------|---------|
| `type` | text | (required) | One of the types in the next table. |
| `required` | `true` or `false` | `false` | If `true`, every entry must have this key. |
| `values` | list of text | | The allowed values. Required for `enum`, and only allowed for `enum`. |
| `of` | a table like this one | | The type of each item. Required for `list`, and only allowed for `list`. |
| `schema` | text | | The name of a `[schema.defs.<name>]` table. Only for `object`, and not together with `fields`. |
| `fields` | table of tables like this one | | The keys of the object. Only for `object`, and not together with `schema`. |

The short form has no other options. It has no default value, no pattern, no minimum or maximum,
no description and no "can be empty" option. If you need one of these, use a
[JSON Schema file](#a-full-json-schema-file).

### Types

| Type | The entry value must be | Example value (YAML) |
|------|-------------------------|----------------------|
| `string` | text | `'house'` |
| `slug` | lowercase letters and digits, in groups joined by single hyphens | `'que-tal'` |
| `markdown` | text | `'From Latin *casa*.'` |
| `int` | a whole number (`1.5` is an error) | `3` |
| `float` | a number (`2` is also correct) | `2.5` |
| `bool` | `true` or `false` (the text `yes` is an error) | `true` |
| `enum` | one of the `values` | `'f'` |
| `media` | text: a path in a media directory | `'images/casa.png'` |
| `list` | a list, where each item has the type in `of` | `['a', 'b']` |
| `object` | a table with the keys in `fields` or in the named definition | `{ book: 'Aula 1' }` |

Two types are hints for you, not extra checks:

- `markdown` is text. janky does not change it. To turn the Markdown into HTML, a field template
  must use the [`markdown` filter](template-reference.md#markdown).
- `media` is text. janky looks for the file only when a field template passes the value to
  [`media()`](template-reference.md#media-for-a-file-that-you-supply). If no template does this,
  janky does not check that the file exists, and does not put it in the package.

### Objects and named definitions

An `object` holds its own keys. You give these keys inline with `fields`, or you name a definition
with `schema`. Put a definition in a `[schema.defs.<name>]` table. Use a definition when several
keys have the same shape.

```toml
[schema]
spanish = { type = "string", required = true }
english = { type = "string", required = true }
words = { type = "list", of = { type = "object", schema = "word" } }
source = { type = "object", fields = { book = { type = "string", required = true }, page = { type = "int" } } }

[schema.defs.word]
spanish = { type = "string", required = true }
english = { type = "string" }
```

An object accepts only the keys that you give, and each object checks its own `required` keys.
Two note types can use the same definition name. They do not affect each other.

> [!NOTE]
> The key `defs` in `[schema]` is always the table of definitions. An entry cannot have a key
> named `defs`.

## The reserved keys `id`, `type` and `tags`

janky adds three keys to every short-form schema:

| Key | Meaning | In the schema |
|-----|---------|---------------|
| `id` | The id of the entry. Every entry must have it. | Optional. By default, `id` is a `slug`. You can declare `id = { type = "string" }` (or `"slug"`) to allow any text. |
| `type` | The entry type: the note type key or one of its aliases. | Not allowed. janky adds it. |
| `tags` | A list of Anki tags. | Not allowed. janky adds it. |

[Entries: ids, types, tags and defaults](../content/entries.md) explains these keys.

## Errors in the short form

`janky check` stops with an error when the `[schema]` table itself is wrong. For example, an
unknown type:

```console
$ janky check
config::enum

  x `schema.literal.type`: "text" is not one of "string", "slug" or 8 other candidates
   ,-[notetypes/phrase.toml:7:20]
 6 | english = { type = "string", required = true }
 7 | literal = { type = "text" }
   :                    ^^^|^^
   :                       `-- here
 8 | 
   `----
```

| Code | Message | Cause |
|------|---------|-------|
| `config::unknown-key` | unknown key `schema.x2.optional` | An option that the short form does not have |
| `config::reserved-key` | `type` is a reserved key | You declared `type` or `tags` |
| `config::id-type` | `id` must be a `string` or a `slug` | You declared `id` with another type |
| `config::field-spec` | an `enum` needs `values` | `enum` without `values` |
| `config::field-spec` | `values` is only for `enum` | `values` on another type |
| `config::field-spec` | a `list` needs `of` | `list` without `of` |
| `config::field-spec` | `of` is only for `list` | `of` on another type |
| `config::field-spec` | an `object` needs `schema` or `fields` | `object` without either |
| `config::field-spec` | an `object` has `schema` or `fields`, not both | `object` with both |
| `config::unknown-def` | `nodef` is not in `[schema.defs]` | `schema` names a definition that does not exist |

## Errors in the content

When an entry does not match the schema, the diagnostic points at the content file. This output
comes from the phrase note type with the schema in
[Objects and named definitions](#objects-and-named-definitions), plus
`register = { type = "enum", values = ["formal", "informal"] }`. The entry has three mistakes:

```console
$ janky check
content::enum

  x `entries[0].register`: "polite" is not one of "formal" or "informal"
   ,-[content/phrases/02-travel.toml:7:12]
 6 | tags = ["travel"]
 7 | register = "polite"
   :            ^^^^|^^^
   :                `-- here
 8 | words = [ { spanish = "baño", english = "bathroom" }, { english = "where" } ]
   `----
content::required

  x missing key `spanish` in `entries[0].words[1]`
   ,-[content/phrases/02-travel.toml:8:55]
 7 | register = "polite"
 8 | words = [ { spanish = "baño", english = "bathroom" }, { english = "where" } ]
   :                                                       ^^^^^^^^^^|^^^^^^^^^^
   :                                                                 `-- `spanish` is missing
 9 | source = { page = 12 }
   `----
content::required

  x missing key `book` in `entries[0].source`
   ,-[content/phrases/02-travel.toml:9:10]
 8 | words = [ { spanish = "baño", english = "bathroom" }, { english = "where" } ]
 9 | source = { page = 12 }
   :          ^^^^^^|^^^^^^
   :                `-- `book` is missing
   `----
...
```

Other content errors from the schema:

| Code | Example message |
|------|-----------------|
| `content::type` | `entries[0].num`: 1.5 is not of type "integer" |
| `content::type` | `entries[0].flag`: "yes" is not of type "boolean" |
| `content::pattern` | `entries[0].sl`: "Not_A_Slug" does not match "^[a-z0-9]+(-[a-z0-9]+)*$" |
| `content::unknown-key` | unknown key `entries[0].bogus` |
| `content::enum` | `entries[2].type`: "nosuch" is not one of "lab" |

[Reading errors and warnings](../cli/diagnostics.md) explains the parts of a diagnostic.

## A full JSON Schema file

JSON Schema is a standard format that describes the shape of data. It has many more checks than the
short form, for example a maximum length or a pattern. To use a JSON Schema file, give its path as
the value of `schema`:

```toml
name = "Spanish Phrase"
css = "word/style.css"
schema = "phrase.schema.json"
```

The path is relative to the note type file. The file describes one entry:

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string" },
    "type": { "type": "string" },
    "tags": { "type": "array", "items": { "type": "string" } },
    "spanish": { "type": "string", "maxLength": 40 },
    "english": { "type": "string" },
    "literal": { "type": "string" }
  },
  "required": ["id", "spanish", "english"],
  "additionalProperties": false
}
```

The file must be valid JSON Schema (version 2020-12). If it is not, janky gives
`config::invalid-schema`. A `$ref` inside the file to its own `$defs` works.

All keywords of JSON Schema work. For example, `maxLength` gives this error:

```console
$ janky check
content::max-length

  x `entries[0].spanish`: "¡Hola! ¡Hola! ¡Hola! ¡Hola! ¡Hola! ¡Hola! ¡Hola!" is longer than 40 characters
   ,-[content/phrases/01-greetings.toml:7:11]
 6 | id = "hola"
 7 | spanish = "¡Hola! ¡Hola! ¡Hola! ¡Hola! ¡Hola! ¡Hola! ¡Hola!"
   :           ^^^^^^^^^^^^^^^^^^^^^^^^^|^^^^^^^^^^^^^^^^^^^^^^^^
   :                                    `-- here
 8 | english = "Hello!"
   `----
...
```

### What janky does not add to a JSON Schema file

janky uses a JSON Schema file as it is. It adds nothing to it. So some checks of the short form
are not there unless you write them:

- **Unknown keys.** janky rejects an unknown key only if the file has
  `"additionalProperties": false`. Without it, a typo such as `englsh` is accepted without a
  diagnostic.
- **`type` and `tags`.** If the file has `"additionalProperties": false`, you must declare `type`
  and `tags` in `properties`. If you do not, entries that use them give `content::unknown-key`.
- **The slug rule for `id`.** janky checks only that `id` is text. Any other rule must be in the
  file.
- **The shape of `tags`.** If `tags` is not a list of text, janky does not give an error. It
  ignores the bad values.

> [!WARNING]
> Always put `"additionalProperties": false` in a JSON Schema file, and declare `id`, `type` and
> `tags` in it. Without these, janky cannot find typos in your content.

## Help from your editor

`janky schema` writes JSON Schema files for your content and for `notetype.toml` into
`.janky/schema/`. It also prints the settings that connect them to your editor. Your editor can
then show errors and complete key names while you type. See the
[Command reference](../cli/commands.md).
