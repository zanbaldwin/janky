# Content files

A content file holds the entries of your deck. This page explains the file formats, how janky
finds your files, and what janky does when a file has an error.

## Formats and file extensions

janky reads three formats. The file extension tells janky which format a file uses.

| Extension | Format |
|-----------|--------|
| `.yaml` or `.yml` | YAML |
| `.toml` | TOML |
| `.json` | JSON |

The extension must be in lowercase. janky does not read `LESSON.YAML`. Any other file that a deck
selects gives the error `content::format`, for example:

```text
`content/notes.md` is not a YAML, TOML or JSON file
```

Save every content file as UTF-8 text. Most code editors do this by default. If a file is not
UTF-8, you get `content::read` ("stream did not contain valid UTF-8").

### The byte order mark

Some Windows editors put an invisible mark (a byte order mark, or BOM) at the start of a UTF-8
file. TOML files with this mark work. JSON and YAML files with this mark fail:

- JSON gives `json::syntax` "Unexpected token" at line 1, column 1.
- YAML gives `content::unknown-key` for the key `"\u{feff}entries"`, and `content::required` for
  the key `entries`.

If you see one of these errors on a file that looks correct, save the file as "UTF-8" without a
BOM. Your editor shows this option in its encoding menu.

## The shape of a content file

A content file has only two keys at the top level:

| Key | Required | Meaning |
|-----|----------|---------|
| `entries` | Yes | A list of entries. Each entry must have an `id`. |
| `defaults` | No | Values that janky copies into each entry of this file. See [defaults](entries.md#defaults-values-for-every-entry-in-a-file). |

Any other key at the top gives `content::unknown-key`. An empty list of entries (`entries: []`) is
valid and makes no notes.

This is a complete YAML content file. It is from the tutorial, with one entry left out:

```yaml
defaults:
  tags: ['food']

entries:
  - id: 'pan'
    spanish: 'pan'
    english: 'bread'
    gender: 'm'
    production: true
    examples:
      - 'Compro pan cada mañana.'
  - id: 'manzana'
    spanish: 'manzana'
    english: 'apple'
    gender: 'f'
```

[Entries: ids, types, tags and defaults](entries.md) explains the keys of an entry.

## How a deck selects its content files

`deck.toml` tells janky which files belong to which deck. Each deck has a `content` list of
patterns (globs). A pattern is a file path that can contain wildcards:

| Pattern part | Matches |
|--------------|---------|
| `*` | Any characters in one file or directory name. It does not match `/`. |
| `**` | Zero or more directories. |

The patterns are relative to the project directory (the directory of `deck.toml`). In the
tutorial, the child deck `vocab` selects all YAML files in one directory:

```toml
[deck.children.vocab]
name = "1 Vocabulary"
content = ["content/vocab/*.yaml"]
```

With this pattern, `content/vocab/01-basics.yaml` belongs to the deck. A file in
`content/vocab/old/` does not. To also select the files in subdirectories, use
`content/vocab/**/*.yaml`.

Some rules apply to all patterns:

- A pattern that matches no file is not an error. janky shows no warning.
- A content file that no pattern matches is ignored. janky shows no warning.
- If two patterns of one deck match the same file, janky reads the file once.
- One file can belong to only one deck. If two decks match the same file, you get
  `content::two-decks`. The first deck in `deck.toml` keeps the file (the root deck comes first).

[Decks and child decks](../deck/decks.md) explains the `content` key and the deck tree.

### End every pattern with a file extension

The wildcard `*` also matches hidden files, whose names start with a dot. Many editors write a
hidden swap file next to the file that you edit. For example, Vim writes `.01-basics.yaml.swp`.

With the pattern `content/vocab/*`, janky selects this swap file. The swap file is not YAML, TOML
or JSON, so you get `content::format` and the build stops. Use a pattern that ends in an extension,
such as `content/vocab/*.yaml`.

A pattern that ends in an extension also helps your editor. The command `janky schema` uses these
patterns to give your editor the list of valid keys (see the
[command reference](../cli/commands.md)). It gives no editor settings for a pattern such as
`content/vocab/*`.

## File order is the order of new cards

janky gives new cards their positions in this order:

1. The decks, in the order of `deck.toml`. A parent deck comes before its child decks.
2. The `content` patterns of each deck, in the order of the list.
3. The files that one pattern finds, in the order of their paths.
4. The entries of each file, from top to bottom (see [`new_order`](../deck/decks.md#the-order-of-new-cards)).

In the tutorial, the vocabulary deck is first in `deck.toml`. So `content/vocab/01-basics.yaml`
gets the first new-card positions, although `content/phrases/01-greetings.toml` comes first in
path order.

The path sort compares characters by their code, not like a dictionary:

- Uppercase letters come before lowercase letters (`Z.yaml` before `a.yaml`).
- `10.yaml` comes before `2.yaml`.

> [!TIP]
> Start your file names with numbers of the same length (`01-basics.yaml`, `02-food.yaml`) or with
> a date (`2026-10-08.yaml`). Then the file order is the order that you expect.

## Writing YAML

YAML is a format for structured text. It uses indentation (spaces at the start of a line) to show
which values belong together. These are the parts that a content file uses:

```yaml
# A line that starts with # is a comment. janky ignores it.
defaults:
  tags: ['basics']           # a key, a colon, a space, then the value

entries:                     # a list: each item starts with "- "
  - id: 'casa'               # the first entry
    spanish: 'casa'
    english: 'house'
    production: true         # a boolean (true or false)
    examples:                # a list inside the entry
      - 'Mi casa es pequeña.'
      - 'La casa tiene un jardín.'
    notes: |
      From Latin *casa*, "hut".
      Compare **en casa**: "at home".
  - id: 'perro'              # the second entry
    spanish: 'perro'
    english: 'dog'
```

- Use spaces for indentation, not tabs. Keep the keys of one entry at the same indentation.
- A list can also be on one line: `['basics', 'animals']`.
- After `|`, every indented line is part of one text value, with its line breaks. After `>`, YAML
  joins the lines with spaces. With `|-` or `>-`, the value has no line break at the end.

Anki shows field values as HTML, so a line break in a value shows as a space on the card. To show
line breaks, use a field template with the `markdown` filter (see
[Field templates](../notetypes/fields.md)).

### When YAML reads a value as text

YAML guesses the type of each value that has no quotes. janky reads YAML 1.2, which has these
rules:

| Value without quotes | YAML reads it as |
|----------------------|------------------|
| `casa`, `2026-10-08`, `yes`, `no`, `on`, `y` | text |
| `true`, `True`, `false` | a boolean |
| `1`, `012`, `1e3`, `0x1F` | a number (`012` becomes 12) |
| `~`, `Null`, or no value at all | null (nothing) |
| `.inf`, `nan` | an error (`yaml::number`) |

If a key needs text and gets a number, a boolean or null, you get `content::type`. janky also
shows the fix:

```text
`entries[1].id`: 42 is not of type "string"
help: quote the value: '42'
```

The opposite also happens. A key of type `bool` needs `true` or `false`. If you write `yes`, YAML
reads the text "yes", and you get `content::type`. janky never changes a value to the type that a
key needs: `'3'` is not a number.

> [!TIP]
> Put single quotes around every text value, as the examples in this guide do. Then YAML never
> guesses. To write a single quote inside a quoted value, write it twice: `'It''s'`.

### Other YAML rules

- A content file holds one YAML document. A second document (after a `---` line) gives
  `yaml::documents`.
- A key can occur only once in a mapping. A second `english:` in one entry gives
  `yaml::duplicate-key`.
- Every key must be text. A key such as `1:` gives `yaml::key`.
- Anchors and aliases work. An anchor (`&name`) marks a value, and an alias (`*name`) repeats it.
- Merge keys (`<<: *name`) do not work. janky reports `<<` as an unknown key. To share values
  between entries, use [defaults](entries.md#defaults-values-for-every-entry-in-a-file).

## Writing TOML

TOML is the format of `deck.toml`. In a content file, `[defaults]` starts the defaults table, and
each `[[entries]]` line starts a new entry:

```toml
[defaults]
type = "phrase"
tags = ["greetings"]

[[entries]]
id = "hola"
spanish = "¡Hola!"
english = "Hello!"

[[entries]]
id = "que-tal"
spanish = "¿Qué tal?"
english = "How are you?"
literal = "What such?"
```

A key belongs to the table of the last header above it. Put the `defaults` keys directly under
`[defaults]`, before the first `[[entries]]` line.

In TOML, text always has quotes, so the YAML problems above do not occur. A date without quotes
(`2026-10-08`) is accepted as text. `nan`, `inf` and integers that are too large give
`toml::number`.

## Writing JSON

JSON is strict:

- Every text value and every key needs double quotes.
- Comments are not allowed.
- A comma after the last item of a list or object gives `json::syntax` ("Trailing commas are not
  allowed").
- A missing comma gives `json::syntax` ("Expected comma").
- A key that occurs twice in one object gives `json::duplicate-key`.

The top level of the file must be an object with `entries`, not a list.

## When a file has an error

`janky check` reads every file and reports every error in one run. Each error names the file, the
line and the column:

```console
$ janky check
content::required

  x missing key `english` in `entries[0]`
   ,-[content/vocab/02-food.yaml:5:5]
 4 | entries:
 5 |   - id: 'pan'
   :     ^^^^|^^^^
   :         `-- `english` is missing
 6 |     spanish: 'pan'
   `----
content::unknown-key

  x unknown key `entries[0].englsh`
   ,-[content/vocab/02-food.yaml:7:5]
 6 |     spanish: 'pan'
 7 |     englsh: 'bread'
   :     ^^^|^^
   :        `-- not allowed here
 8 |     gender: 'm'
   `----
...
```

In the message, `entries[0]` is the first entry of the file (janky counts from 0).

What janky does with the rest of the file depends on the error:

- A syntax error, or a wrong shape (for example, `entries` is not a list), stops janky from
  reading the file. janky uses no entries from it.
- An error in `defaults` affects every entry of the file. janky uses no entries from it.
- An error in one entry affects only that entry. janky still checks the other entries.

`janky plan` and `janky build` do not run while there is an error. They show the same errors and
stop, so janky never builds a package with part of your content missing. Correct every error, then
run the command again.

[Reading errors and warnings](../cli/diagnostics.md) explains the error format.
