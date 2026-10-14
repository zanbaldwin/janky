# Filters, functions and context

This page lists everything that a field template and a `when` expression can use. For an
introduction to the template language, see [Field templates](fields.md).

All field templates and `when` expressions of all note types share one set of filters and
functions. A filter from the plugin of one note type also works in the templates of another note
type.

## The template context

These values are available in every field template and every `when` expression. The examples
come from the entry `casa` of the tutorial.

| Name | Value | Example |
|------|-------|---------|
| `entry` | The entry, after janky adds the file's `defaults` to it. A key in the entry wins over the same key in `defaults`. | `entry.spanish` is `casa` |
| `entry.id` | The id of the entry. | `casa` |
| `entry.tags` | The tags of the entry, or the tags from `defaults`. | `['basics']` |
| `entry.type` | The entry type, but only if the entry or its `defaults` sets `type`. It is empty for an entry that uses the default note type without `type`. | (empty) |
| `file.path` | The path of the content file, from the project directory. | `content/vocab/01-basics.yaml` |
| `file.stem` | The file name without its extension. | `01-basics` |
| `file.defaults` | The `defaults` table of the content file. | `{'tags': ['basics']}` |
| `deck.key` | The keys of the deck and its parent decks, joined with `.`. Empty for the root deck. | `vocab` |
| `deck.name` | The display name of the deck. | `1 Vocabulary` |
| `deck.full_name` | The full Anki name of the deck, with `::` between the parent and the child deck. | `Spanish::1 Vocabulary` |
| `notetype.key` | The key of the note type in `deck.toml`. | `word` |
| `notetype.name` | The name of the note type in Anki. | `Spanish Word` |

[Entries: ids, types, tags and defaults](../content/entries.md) explains how `defaults` and
`tags` work.

## Filters and functions of janky

"Trusted HTML" in this table means that janky does not escape the result again. See
[Auto-escaping](fields.md#auto-escaping).

| Name | Form | Input | Output |
|------|------|-------|--------|
| [`markdown`](#markdown) | filter | text | trusted HTML |
| [`json`](#json) | filter | any value | trusted JSON text |
| [`nfc`](#nfc) | filter | text | the same text in Unicode NFC form |
| [`media`](#media-for-a-file-that-you-supply) | function, `media(path)` | a path in a media directory | a media value |
| [`media`](#media-from-a-generator) | function `media(input, instance)`, or filter `input \| media(instance)` | text or a table | a media value for a generated file |
| [`enrich`](#enrich) | function `enrich(input, instance)`, or filter `input \| enrich(instance)` | text or a table | a table of data from a generator |
| [`sound`](#sound) | filter | a media value, or a file name | trusted `[sound:...]` text |
| [`img`](#img) | filter | a media value, or a file name | a trusted `<img>` element |

### `markdown`

Turns Markdown text into HTML. Markdown is a simple text format: `*word*` gives italic,
`**word**` gives bold, and lines that start with `- ` give a list.

```jinja
{{ entry.notes | markdown }}
```

With the entry `casa` of the tutorial, the content:

```yaml
notes: |
  From Latin *casa*, "hut".
  Compare **en casa**: "at home".
```

gives this field:

```html
<p>From Latin <em>casa</em>, "hut".
Compare <strong>en casa</strong>: "at home".</p>
```

- The filter uses plain CommonMark Markdown. It has no tables, no `~~strikethrough~~` and no
  footnotes.
- A single newline stays inside the paragraph. Two spaces at the end of a line give a line break
  (`<br />`). An empty line starts a new paragraph.
- A missing value gives an empty field.
- A value that is not text, such as a number, gives `template::render` with the message
  "invalid operation: markdown needs a string".
- janky removes spaces and newlines at the end of the result.

> [!WARNING]
> HTML in the Markdown text goes into the field without change. This includes `<script>`
> elements. Use `markdown` only for content that you trust.

### `json`

Turns any value into compact JSON text. JSON is a text format for data, with lists in `[ ]` and
tables in `{ }`.

```jinja
{{ {'id': entry.id, 'deck': deck.full_name} | json }}
```

For the entry `casa`, this gives `{"id":"casa","deck":"Spanish::1 Vocabulary"}`.

- `{{ entry | json }}` gives the full entry, with its `tags`.
- A missing value gives `null`.
- janky writes `<` as `\u003c` and `&` as `\u0026`, so the text is safe inside HTML.

### `nfc`

Changes text to the Unicode NFC form. Some characters, such as `é`, can be stored as one code or as
two (a letter and an accent). Both look the same, but they are different text. NFC always uses the
form with one code.

```jinja
{{ entry.spanish | nfc }}
```

Use `nfc` before you give text to a generator. Then the same word always gives the same job. If the
input was trusted HTML, the result is trusted HTML too.

### `media` for a file that you supply

`media(path)` finds a media file that you supply, and puts it in the package.

```jinja
{% if entry.image %}{{ media(entry.image) | img }}{% endif %}
```

- The path is relative to a media directory (the `media_dirs` in `deck.toml`, by default
  `media/`). It is not relative to the content file. So `images/casa.png` finds
  `media/images/casa.png`.
- The result prints as the file name only: `casa.png`. Anki keeps all media files in one folder.
- The result has these attributes: `.file` (the file name), `.kind` (`referenced`) and `.exists`
  (`True` or `False`).

These are the errors:

| Code | Message |
|------|---------|
| `media::missing` | media file `img/missing.png` does not exist |
| `media::outside` | media path `../deck.toml` is outside the media directories |
| `media::name-collision` | `media/img/a.png` already has the Anki file name `a.png` |
| `config::missing-media-dir` (warning) | media directory `nodir` does not exist |

> [!IMPORTANT]
> Put `media()` inside `{% if entry.image %}` when the key is optional. For an entry without the
> key, `media(entry.image)` gives `media::missing` with the message "media file `` does not exist".

[Images and audio](../content/media.md) explains media files in more detail.

### `media` from a generator

`media(input, instance)` asks a generator plugin instance for a media file, for example an audio
file from a text-to-speech service. The filter form `input | media(instance)` does the same.

```jinja
{{ entry.spanish | nfc | media('voice') | sound }}
```

Here `voice` is the name of a `[plugins.voice]` instance in `deck.toml` that makes media.

- The template only records a job. It does not send anything. `janky generate` runs the job.
- The result prints as the name of the file in the cache: 32 hexadecimal characters and an
  extension, for example `43601fdcef43c4eb1e520e78ba6e32cc.ogg`. If janky does not know the
  extension yet, it prints nothing.
- `.kind` is `generated`. `.exists` is `False` until `janky generate` has made the file.
- An empty input records no job and prints nothing.

[Generating media and data](../plugins/generating.md) explains generators and jobs.

### `enrich`

`enrich(input, instance)` gets data from a generator plugin instance that makes data, for example
a dictionary lookup. The filter form `input | enrich(instance)` does the same.

```jinja
{% set f = enrich(entry.spanish | nfc, 'frequency') %}{% if f.rank %}#{{ f.rank }} ({{ f.band }}){% endif %}
```

- The result is a table of values from the data cache.
- Before `janky generate` runs the job, the result is an empty table. Use `{% if %}` to show
  nothing in that case. Without it, the template above shows `# ()`.
- If the input is a table, janky removes the keys with no value (`none`).
- If the generator gave a warning, janky shows it as `enrich::warning`. If it gave an info,
  janky shows it as `enrich::info`.

### `sound`

Makes the Anki sound tag for a media value or a file name.

```jinja
{{ media(entry.audio) | sound }}
```

- `media(...)` for the file `s.ogg` gives `[sound:s.ogg]`.
- An empty value gives an empty result.
- janky escapes the file name.

### `img`

Makes an HTML image element for a media value or a file name.

```jinja
{{ media(entry.image) | img }}
```

- For the entry `casa`, this gives `<img src="casa.png">`.
- The element has no `alt` text and no other attributes. To add attributes, write the element in
  the template: `<img src="{{ media(entry.image) }}" class="photo">`.
- An empty value gives an empty result.
- janky escapes the file name: `'q"x.png' | img` gives `<img src="q&quot;x.png">`.

> [!WARNING]
> `sound` and `img` with a plain file name, such as `{{ 'casa.png' | img }}`, do not put the file
> in the package. Only `media(path)` does that. Always use `media()` for a file that you supply.

## Errors when you call a generator

| Message (code `template::render`) | Cause |
|-----------------------------------|-------|
| invalid operation: `voice` is not a plugin instance with a `generate` function | No generator instance has this name |
| invalid operation: `frequency` generates data, so call it with `enrich` | `media()` with a data generator |
| invalid operation: `voice` generates media files, so call it with `media` | `enrich()` with a media generator |

## Filters and functions of minijinja

minijinja has its own filters and tags. These are the ones that you use most:

| Name | Example | Result for the entry `casa` |
|------|---------|-----------------------------|
| `default` | `{{ entry.literal \| default('none') }}` | `none` |
| `join` | `{{ entry.examples \| join(' / ') }}` | `Mi casa es pequeña. / La casa tiene un jardín.` |
| `length` | `{{ entry.examples \| length }}` | `2` |
| `upper` | `{{ entry.english \| upper }}` | `HOUSE` |
| `lower` | `{{ entry.english \| lower }}` | `house` |
| `safe` | `{{ '<b>x</b>' \| safe }}` | `<b>x</b>` (not escaped) |
| `escape` | `{{ entry.english \| escape }}` | `house` (escaped) |
| `format` | `{{ '%s!' \| format(entry.english) }}` | `house!` |

Tags: `{% if %}`, `{% for %}`, `{% set %}`, `{% macro %}`, and `{% include %}` and
`{% from ... import %}` for other field template files. See
[The template language](fields.md#the-template-language).

## Filters and functions from plugins

A note type plugin (the `plugins` key in `notetype.toml`) and a plugin instance in `deck.toml` can
add filters and functions. The tutorial's `spanish.lua` adds the filter `article` and the function
`gender_name`:

```jinja
{{ entry.spanish | nfc | article(entry.gender) }}
{{ gender_name(entry.gender) }}
```

A filter gets the value before `|` as its first argument, and the values in parentheses after it.

A plugin filter or function cannot have the same name as a filter or function of janky, or of
another plugin. If it does, janky gives `plugin::table` with a message such as
"nt/p.lua: `markdown` is a built-in filter or function of janky".

> [!WARNING]
> Do not name a plugin filter or function `enrich` or `generate`. janky rejects the name
> `generate`. It accepts a plugin filter named `enrich`, but janky's own `enrich` replaces it.

[Kinds of plugin](../plugins/kinds.md) and [Your first plugin: a filter](../authoring/first-plugin.md)
explain plugins.

## Limits

| Limit | Value |
|-------|-------|
| One field of one note | 1 MiB (1,048,576 bytes) |
| All fields of all notes together | 256 MiB |
| All data from `enrich` in a project | 256 MiB |

A field over the limit gives `template::too-large`. See
[Field size limits](fields.md#field-size-limits). A plugin filter or function also has limits on
its time and memory. See [The sandbox and its limits](../authoring/sandbox.md).
