# Note type files

A note type is a TOML file, plus the HTML, CSS, JavaScript and template files that it names. TOML
is a plain text format for settings. It has `key = value` lines, and table headers in square
brackets, such as `[fields.english]`, that group the lines below them.

## Tell janky where the note type is

`deck.toml` names each note type in a `[notetypes.<key>]` table. The `path` key gives the location
of the note type file:

```toml
[notetypes.word]
path = "notetypes/word"
default = true

[notetypes.phrase]
path = "notetypes/phrase.toml"
alias = "expression"
```

The other keys of this table (`alias`, `default` and the keys for shared note types) are in
[Adding note types](../deck/notetypes.md).

The key (`word`, `phrase`) is part of the identity of every note of this note type. If you change
the key, Anki sees new notes. [Changing content safely](../content/changing.md) explains this.

## Directory form and single-file form

A note type can have one of two forms.

| Form | `path` in `deck.toml` | File that janky reads | Paths in the file are relative to |
|------|-----------------------|-----------------------|-----------------------------------|
| Directory | `notetypes/word` | `notetypes/word/notetype.toml` | `notetypes/word/` |
| Single file | `notetypes/phrase.toml` | `notetypes/phrase.toml` | `notetypes/` |

If `path` names a directory, janky adds `notetype.toml` to it. If `path` names a file, janky reads
that file.

The tutorial project uses both forms:

```text
notetypes/
  word/                    the directory form
    notetype.toml
    style.css
    spanish.lua
    assets/reveal.js
    fields/examples.html.j2
    cards/recognition.front.html
    cards/recognition.back.html
    cards/production.front.html
    cards/production.back.html
  phrase.toml              the single-file form
  phrase/front.html
  phrase/back.html
```

## Paths inside the file

Every path in a note type file is relative to the directory that holds the file. This applies to
`css`, `assets`, `plugins`, a `schema` file, a field `template`, and a card `front` and `back`.

A path must stay inside that directory. A symbolic link (a file that points to another file) that
points outside the directory is also an error. These are the errors for a bad path:

| Code | Message |
|------|---------|
| `config::outside-notetype` | `../deck.toml` is outside the note type directory |
| `config::missing-file` | file `missing.js` does not exist |
| `config::not-a-file` | `cards` is not a regular file |

## Share files between note types

Two note type files in the same directory can use the same files. In the tutorial project,
`notetypes/phrase.toml` uses the styling of the word note type:

```toml
name = "Spanish Phrase"
css = "word/style.css"
```

This works because the paths of `phrase.toml` are relative to `notetypes/`. The directory form
cannot do this. `notetypes/word/notetype.toml` can only read files inside `notetypes/word/`.

> [!TIP]
> If you want several note types to share one CSS file, use the single-file form for them, and
> keep the files side by side in one directory.

## The top-level keys of `notetype.toml`

| Key | Type | Required | Default | Meaning |
|-----|------|----------|---------|---------|
| `name` | text | yes | | The name of the note type in Anki. |
| `sort_field` | text | no | the first field | The field that the Anki browser sorts by. Give the field key (for example `spanish`), not the display name. |
| `css` | path | no | no CSS | A CSS file for all cards. See [Styling and scripts](styling.md). |
| `assets` | list of paths | no | `[]` | JavaScript files that janky puts into the card templates. See [Styling and scripts](styling.md). |
| `plugins` | list of paths | no | `[]` | Lua scripts that add filters and functions. See [Kinds of plugin](../plugins/kinds.md). |
| `schema` | table, or a path | no | only `id`, `type` and `tags` | The keys that an entry can have. See [The content schema](schema.md). |
| `fields` | tables `[fields.<key>]` | yes, at least one | | The Anki fields. See [Field templates](fields.md). |
| `cards` | tables `[cards.<key>]` | yes, at least one | | The Anki card types. See [Card templates](cards.md). |

janky rejects a key that is not in this table. For example, `kind = "cloze"` gives
`config::unknown-key` with the message "unknown key `kind`". This also applies to the keys inside
`[fields.<key>]` and `[cards.<key>]`.

A `sort_field` that names no field gives `config::unknown-field` with the message "field `nope`
does not exist".

This is the word note type of the tutorial (the field and card tables are shortened):

```toml
name = "Spanish Word"
sort_field = "spanish"
css = "style.css"
assets = ["assets/reveal.js"]
plugins = ["spanish.lua"]

[schema]
spanish = { type = "string", required = true }
english = { type = "string", required = true }
...

[fields.spanish]
name = "Spanish"
value = "{{ entry.spanish | nfc }}"

[fields.english]
name = "English"
...

[cards.recognition]
name = "Recognition"
front = "cards/recognition.front.html"
back = "cards/recognition.back.html"
...
```

## Keys and display names

Each field and each card type has a **key** (the part after `fields.` or `cards.` in the table
header) and a **display name** (its `name`).

- The key links the field or card type to its id in `deck.lock`. Do not change a key after you
  import the deck.
- The display name is the name that Anki shows. You can change it at any time.

A key starts with a lowercase letter. It holds only lowercase letters, digits, `_` and `-`. Any
other key gives this error:

```text
config::key   `T-Nfc` is not a valid key
              help: a key starts with a lowercase letter and holds only lowercase letters, digits, `_` and `-`
```

## The order of fields and cards

The order of the `[fields.<key>]` tables in the file is the order of the fields in Anki. If you do not
set `sort_field`, the Anki browser sorts by the first field.

The order of the `[cards.<key>]` tables is the order of the card types in Anki. The first card type
has a special role: see [Anki always makes the first card](cards.md#anki-always-makes-the-first-card).

If a card type has a `when` condition, janky adds a hidden field after all your fields. See
[Cards that only some notes get](cards.md#cards-that-only-some-notes-get).

## What a janky note type cannot do

Every janky note type is a standard Anki note type. janky does not support these features:

- Cloze note types.
- Image occlusion note types.
- A different deck for each card type.
- The LaTeX header and footer.
- The font, size and right-to-left options of a field in the Anki editor.
- A field that the Anki editor shows collapsed.
- Assets other than JavaScript (for example fonts, or an image for every card).
