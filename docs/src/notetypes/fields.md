# Field templates

Each `[fields.<key>]` table in `notetype.toml` makes one Anki field. A **field template** tells
janky how to fill the field from an entry. janky runs it once for each entry, when you run
`janky check`, `plan` or `build`. The result is the text that Anki stores in the note.

## The keys of a field

| Key | Type | Required | Meaning |
|-----|------|----------|---------|
| `name` | text | yes | The display name of the field in Anki. Card templates use this name. |
| `value` | text | no | A field template, written in the file. Not together with `template`. |
| `template` | path | no | A file that holds the field template. Not together with `value`. |
| `description` | text | no | The field description that the Anki editor shows. |

The field key (the part after `fields.`) and the display name are different things. See
[Keys and display names](structure.md#keys-and-display-names).

## Three ways to fill a field

### No template: copy the entry key with the same name

If a field has no `value` and no `template`, janky uses the entry key with the same name as the
field key. In the tutorial, the field `english` takes the value of `english` in each entry:

```toml
[fields.english]
name = "English"
```

janky turns the value into text like this:

| Entry value | Field text |
|-------------|------------|
| text | the text, with HTML characters escaped (see [Auto-escaping](#auto-escaping)) |
| a number | the number, for example `3` or `2.0` |
| `true` or `false` | `true` or `false` |
| no value, or `null` | empty |
| a list or an object | an error |

A list or an object gives this error:

```text
template::not-text  field `list` has no `value` or `template`, but the entry value is a list or an object
                    help: add a `value` or `template` that renders it; first seen in entry `one` in `content/a.yaml`
```

### `value`: a template in the file

For a short template, write it in `value`:

```toml
[fields.spanish]
name = "Spanish"
value = "{{ entry.spanish | nfc }}"
```

### `template`: a template in its own file

For a longer template, put it in a file and give the path in `template`. The path is relative to
the note type file. The tutorial uses `fields/examples.html.j2`:

```toml
[fields.examples]
name = "Examples"
template = "fields/examples.html.j2"
```

```jinja
{% for example in entry.examples %}<p class="example">{{ example | nfc }}</p>{% endfor %}
```

janky removes one newline at the end of a template file. It keeps all other newlines.

If a field has both `value` and `template`, janky gives `config::value-and-template` with the
message "field `md` has both `value` and `template`".

## The template language

Field templates use **minijinja**, a template language that is almost the same as Jinja. A template
is normal text with some special tags in it. janky copies the normal text to the field and runs the
tags.

### Show a value: `{{ }}`

`{{ ... }}` shows the value of an expression. The entry is in the variable `entry`:

```jinja
{{ entry.english }}
```

For the entry `casa`, this gives `house`. You can also join text with `~`, for example
`{{ entry.spanish ~ '!' }}` gives `casa!`. The other variables (`file`, `deck` and `notetype`) are
in [Filters, functions and context](template-reference.md#the-template-context).

### Change a value: filters with `|`

A **filter** changes a value. Write `|` and the filter name after the value. You can put several
filters in a row. Some filters take arguments in parentheses.

```jinja
{{ entry.english | upper }}
{{ entry.examples | join(' / ') }}
{{ entry.spanish | nfc | article(entry.gender) }}
```

For the entry `casa`, the first two lines give `HOUSE` and
`Mi casa es pequeña. / La casa tiene un jardín.`. The third line uses the `article` filter from the
tutorial's plugin.

A **function** is called by its name, with arguments in parentheses:

```jinja
{{ gender_name(entry.gender) }}
```

[Filters, functions and context](template-reference.md) lists all filters and functions of janky.

### Choose: `{% if %}`

`{% if ... %}` shows a part of the template only when a condition is true. `{% else %}` is
optional.

```jinja
{% if entry.gender == 'f' %}feminine{% else %}other{% endif %}
```

### Repeat: `{% for %}`

`{% for ... in ... %}` repeats a part of the template for each item of a list:

```jinja
{% for example in entry.examples %}<p class="example">{{ example }}</p>{% endfor %}
```

### Keep a value: `{% set %}`

`{% set %}` gives a value a name, so that you can use it more than once:

```jinja
{% set f = enrich(entry.spanish | nfc, 'frequency') %}{% if f.rank %}#{{ f.rank }} ({{ f.band }}){% endif %}
```

### Comments: `{# #}`

Text between `{#` and `#}` is a comment. janky does not put it in the field.

```jinja
{# This field shows the first example only. #}
```

### Remove spaces and newlines: `-`

Put `-` inside a tag to remove the spaces and newlines next to it. `{%-` removes them before the
tag, and `-%}` removes them after the tag.

```jinja
{% macro example(text) %}<p class="example">{{ text | nfc }}</p>{% endmacro -%}
{% for e in entry.examples %}{{ example(e) }}{% endfor %}
```

Without the `-`, the field starts with the newline after `{% endmacro %}`.

### Reuse a part: macros

A **macro** is a part of a template with a name, like a small function. The example above defines
a macro `example` and calls it for each example.

You can use a macro from another field template file with `{% from ... import ... %}`. You can also
put the full text of another template file in a template with `{% include ... %}`. Give the path of
the file from the project directory. For example, if `notetypes/word/fields/examples.html.j2` holds
the macro `example` from the example above, another field can use it:

```jinja
{% from 'notetypes/word/fields/examples.html.j2' import example %}{% if entry.examples %}{{ example(entry.examples[0]) }}{% endif %}
```

You can also import from or include a file that no field uses as its `template`. The name of the
file must end with `.j2`. The file must be in the directory that holds the note type file, or in a
directory below it. Use this to keep parts of templates, for example macros, in their own files:

```jinja
{% from 'notetypes/word/fields/parts/compare.j2' import compare %}
```

janky reads such a file only when a template imports or includes it. An error in a file that no
template uses does not show.

janky does not find these files:

- a file outside the directory of the note type, also through `..` or a symbolic link
- a file whose name does not end with `.j2`, for example `notes.txt`

An include of such a file gives this error:

```text
template::render  template not found: tried to include non-existing template 'notetypes/word/fields/notes.txt'
```

## Auto-escaping

HTML is the language of web pages and of Anki cards. In HTML, some characters have a special
meaning. For example, `<b>` starts bold text. If you write `a < b` in your content, Anki can read
the `<` as the start of an HTML tag.

To prevent this, janky **escapes** every value that a template shows. It changes these characters
to their HTML codes:

| Character | Becomes |
|-----------|---------|
| `&` | `&amp;` |
| `<` | `&lt;` |
| `>` | `&gt;` |
| `"` | `&quot;` |
| `'` | `&#39;` |

So the text `Tom & <b>'Jerry'</b>` in your content shows on the card exactly as you typed it. It
does not become bold. Your content is text, not HTML.

### Show trusted HTML

Auto-escaping applies to values, not to the normal text of the template. HTML that you write in
the template itself, for example `<p class="example">`, stays HTML.

To get HTML from your content, use one of these:

- **The `markdown` filter.** Write the content in Markdown, a simple text format (`*word*` for
  italic, `**word**` for bold). The filter turns it into HTML. See
  [`markdown`](template-reference.md#markdown).
- **A filter that makes HTML**, such as `img`, `sound` or a plugin filter that returns
  `janky.html(...)`. janky does not escape their result again.
- **The `safe` filter.** `{{ entry.notes | safe }}` shows the value without escaping.

> [!WARNING]
> Use `safe` only for content that you wrote and trust. HTML in the content then goes onto the
> card as it is, and this includes scripts.

## Missing keys show as empty text

If an entry does not have a key, `{{ entry.key }}` shows nothing. There is no error. This also
applies to a misspelt key: `{{ entry.englsh }}` gives an empty field for every entry, and janky
does not tell you.

To find these mistakes:

- Look at the cards after a build. A field that is always empty is a sign of a typo.
- Give a value for a missing key with the `default` filter:
  `{{ entry.literal | default('no literal translation') }}`.
- Use `{% if entry.key %}` around a part that needs the key.

Some filters need a value and give an error for a missing key. For example,
`{{ entry.examples | length }}` gives this error for an entry without `examples`:

```text
template::render  invalid operation: cannot calculate length of value of type undefined (`entry.examples` is undefined)
```

Write `{{ (entry.examples or []) | length }}` instead. `or` gives the value on its right when the
value on its left is missing or empty.

## How values print

A template prints some values in a different way from the entry file:

| Value | `{{ ... }}` prints | A field without a template prints |
|-------|--------------------|-----------------------------------|
| `true` | `True` | `true` |
| `false` | `False` | `false` |
| no value, or `null` | (empty) | (empty) |
| a list | `['x', 'y']`, escaped | an error |
| the number `2.0` | `2.0` | `2.0` |

To show a list, use a `for` loop or the `join` filter. To show a value as JSON, use the
[`json` filter](template-reference.md#json).

## Errors in field templates

| Code | Example message | Cause |
|------|-----------------|-------|
| `template::syntax` | syntax error: unexpected `}`, expected end of variable block | A tag is not closed correctly |
| `template::unknown-filter` | unknown filter: filter markdwn is unknown | A misspelt filter name |
| `template::unknown-function` | unknown function: nofn is unknown | A misspelt function name |
| `template::render` | invalid operation: markdown needs a string | A filter got a value of the wrong type |
| `template::not-text` | field `list` has no `value` or `template`, but the entry value is a list or an object | See [No template](#no-template-copy-the-entry-key-with-the-same-name) |
| `template::too-large` | field `md` has 1048577 bytes, more than the limit of 1048576 bytes | See [Field size limits](#field-size-limits) |

This is a misspelt filter in the tutorial project:

```console
$ janky check
...
template::unknown-filter

  x unknown filter: filter markdwn is unknown
    ,-[notetypes/word/notetype.toml:41:27]
 40 | name = "Notes"
 41 | value = "{{ entry.notes | markdwn }}"
    :                           ^^^|^^^
    :                              `-- here
 42 | 
    `----
  help: first seen in entry `casa` in `content/vocab/01-basics.yaml`
...
```

Some things to know about these errors:

- janky finds an unknown filter or function only when an entry uses that part of the template. A
  part inside an `{% if %}` that no entry reaches is not checked.
- After the first error in a template, janky stops that template. If a template has two mistakes,
  you see the second one after you correct the first.
- janky shows an error in a template once, for the first entry that has it ("first seen in
  entry ...").
- A field with an error is empty, and `janky build` does not write the package.

## Field size limits

| Limit | Value | Error |
|-------|-------|-------|
| One field of one note | 1 MiB (1,048,576 bytes) | `template::too-large`: field `md` has 1048577 bytes, more than the limit of 1048576 bytes |
| All fields of all notes together | 256 MiB | `template::too-large`: janky stops at the entry that goes over the limit |
