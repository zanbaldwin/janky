# 3. Add content

Content files hold your notes. Each item in a content file is an **entry**, and each entry becomes
one Anki note. In this step, you add six words in YAML files, three phrases in TOML files, and one
image.

## Add the image

The word "casa" has an image.

1. Find a small PNG image of a house (any image works).
2. Copy it to `media/images/casa.png`.

janky looks for images in the `media_dirs` of `deck.toml` (here, `media/`). An entry names the
image by its path inside that directory: `images/casa.png`.

## Write the vocabulary files

YAML is a text format for lists and values. A line `key: value` sets one value. A line that starts
with `- ` is one item of a list. The indentation (the spaces at the start of a line) shows what
belongs to what, so keep it exactly as shown. Put text values in single quotes.

Make the file `content/vocab/01-basics.yaml` with this text:

```yaml
defaults:
  tags: ['basics']

entries:
  - id: 'casa'
    spanish: 'casa'
    english: 'house'
    gender: 'f'
    image: 'images/casa.png'
    production: true
    examples:
      - 'Mi casa es pequeña.'
      - 'La casa tiene un jardín.'
    notes: |
      From Latin *casa*, "hut".
      Compare **en casa**: "at home".
  - id: 'perro'
    spanish: 'perro'
    english: 'dog'
    gender: 'm'
    tags: ['basics', 'animals']
    examples:
      - 'El perro duerme.'
  - id: 'gato'
    spanish: 'gato'
    english: 'cat'
    gender: 'm'
    tags: ['basics', 'animals']
```

Make the file `content/vocab/02-food.yaml` with this text:

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
  - id: 'aguacate'
    spanish: 'aguacate'
    english: 'avocado'
    gender: 'm'
```

### The parts of a content file

- `entries` is the list of entries.
- `id` identifies the entry. It must be unique in the whole project. Use lowercase letters and
  digits, in groups joined by single hyphens (for example `que-tal`).
- The other keys (`spanish`, `english`, ...) are the keys of the note type schema.
- `notes: |` starts a text of more than one line. The lines below it, indented, are the text.
- `defaults` gives values to every entry in the file. An entry that has the same key replaces the
  default.

These entries do not name a note type, so they use the default note type, `word`.

> [!TIP]
> Do not change an `id` after you import the deck. Anki sees an entry with a new `id` as a new
> note, without the review history of the old one.

### Tags

`tags` are the Anki tags of the note. A `tags` key in an entry replaces the `tags` of `defaults`. It
does not add to them. So `perro` and `gato` repeat `basics` to keep it.

janky also adds a tag for the deck of each note. It uses the deck key: the notes of the vocabulary
deck get the tag `deck::vocab`.

## Write the phrase files

The phrases use TOML, the same format as `deck.toml`. In TOML, each `[[entries]]` line starts one
entry.

Make the file `content/phrases/01-greetings.toml` with this text:

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

Make the file `content/phrases/02-travel.toml` with this text:

```toml
[[entries]]
id = "donde-esta-el-bano"
type = "expression"
spanish = "¿Dónde está el baño?"
english = "Where is the bathroom?"
tags = ["travel"]
```

The `type` key chooses the note type. In the first file, `defaults` sets `type = "phrase"` for both
entries. The second file uses `expression`, the alias of the phrase note type from
[step 2](notetype.md).

[Content files](../content/files.md) explains the three file formats.
[Entries: ids, types, tags and defaults](../content/entries.md) explains `id`, `type`, `tags` and
`defaults`. [Images and audio](../content/media.md) explains media files.

## Check the content

Run `janky check`:

```console
$ janky check
9 notes are valid
```

janky found nine entries, and each one matches the schema of its note type.

## What you have now

```text
spanish/
  .gitignore
  deck.toml
  content/
    phrases/
      01-greetings.toml
      02-travel.toml
    vocab/
      01-basics.yaml
      02-food.yaml
  media/
    images/
      casa.png
  notetypes/
    phrase.toml
    phrase/       (2 files)
    word/         (9 files)
```

Next: [4. Build and import](build.md).
