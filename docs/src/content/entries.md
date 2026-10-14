# Entries: ids, types, tags and defaults

Each entry becomes one Anki note. Most keys of an entry come from the schema of its note type (see
[The content schema](../notetypes/schema.md)). Three keys are the same for every note type:

| Key | Required | Type | Meaning |
|-----|----------|------|---------|
| `id` | Yes | text | The name that links the entry to its note in Anki. |
| `type` | No | text | The entry type. It selects the note type. |
| `tags` | No | list of text | The Anki tags of the note. |

A file can also give values to all of its entries with `defaults`. This page explains the three
keys and `defaults`.

## The id links an entry to its note

Anki finds an existing note by a hidden id. janky makes this hidden id from the entry's `id`, its
note type and a value in `deck.lock`. The deck, the file and the field values are not part of it.
So while the `id` stays the same, a new import updates the same note in Anki and keeps its review
history.

If you change the `id`, Anki sees a new note. See
[Changing content safely](changing.md).

### What an id can contain

By default, an id is a lowercase "slug":

- lowercase letters `a` to `z` and digits `0` to `9`
- single hyphens between groups of letters and digits

| Id | Valid? |
|----|--------|
| `casa` | Yes |
| `donde-esta-el-bano` | Yes |
| `lesson-08` | Yes |
| `Casa` | No: uppercase letter |
| `bano_nuevo` | No: underscore |
| `baño` | No: a letter that is not `a` to `z` |
| `-casa`, `casa-`, `que--tal` | No: hyphen at the start, at the end, or two together |

An id that breaks this rule gives `content::pattern`:

```text
`entries[4].id`: "Bad_Slug" does not match "^[a-z0-9]+(-[a-z0-9]+)*$"
```

An id is always text. In YAML, `id: 42` is a number and gives `content::type`. Write `id: '42'`.

A note type can allow any text as an id (see [The content schema](../notetypes/schema.md)). Then
an id can contain spaces, capitals and letters such as `ñ`.

> [!WARNING]
> If your note type allows any text, janky compares ids letter by letter. Some letters, such as
> `ñ`, have two forms that look the same: one character, or `n` with a separate `~` mark. janky
> sees two different ids and makes two notes. The slug rule prevents this.

### An id must be unique in the whole project

No two entries in a project can have the same id. This applies across all files, all decks and all
note types. A second use gives `content::duplicate-id`, with the place of the first use:

```text
entry id `pan` is already used at `content/vocab/02-food.yaml:5:9`
```

The entry in the file that comes first in path order keeps the id.

### How to choose a good id

You keep an id for the life of the note, so choose it with care:

- Make it from the part of the entry that does not change. For a word, use the word itself
  (`casa`). For a phrase, use the words of the phrase without accents or punctuation
  (`donde-esta-el-bano`).
- Do not use a number that shows a position (`word-17`). If you add or remove an entry later, the
  numbers no longer match.
- Do not include the deck or the lesson. You can then move the entry to another file or deck
  later, and the id stays correct.
- If two entries have the same word, add a short word that tells them apart, for example
  `banco-bank` and `banco-bench`.

## The type selects the note type

The `type` key tells janky which note type to use for the entry. The value is an **entry type**.
Each note type in `deck.toml` gives one or more entry types:

- the key of the note type, for example `phrase` for `[notetypes.phrase]`
- each value of its `alias` key

In the tutorial, `deck.toml` has these note types:

```toml
[notetypes.word]
path = "notetypes/word"
default = true

[notetypes.phrase]
path = "notetypes/phrase.toml"
alias = "expression"
```

So `type: 'phrase'` and `type: 'expression'` both select the phrase note type. `alias` can also be
a list: `alias = ["expression", "saying"]`. [Adding note types](../deck/notetypes.md) explains
`alias` and `default`.

### How janky finds the type of an entry

janky uses the first of these that it finds:

1. The `type` key in the entry.
2. The `type` key in the file's `defaults`.
3. The note type that has `default = true` in `deck.toml`.

In the tutorial, the vocabulary files have no `type`, so their entries use `word` (the default).
`content/phrases/01-greetings.toml` sets `type = "phrase"` once in `[defaults]`.

If janky finds no type, you get `content::required` "missing key `type` in `entries[1]`".

### Entry types are case-sensitive

`type: 'Phrase'` does not select `phrase`. An unknown entry type gives `content::enum` with the
list of valid values:

```text
`entries[0].type`: "Phrase" is not one of "word", "phrase" or "expression"
```

## Tags

`tags` is a list of text values. Anki shows them as the note's tags.

```yaml
  - id: 'perro'
    spanish: 'perro'
    english: 'dog'
    tags: ['basics', 'animals']
```

A single value without a list (`tags: 'animals'`) gives `content::type`. Write `tags: ['animals']`.

To make a tree of tags, join the levels with `::`, for example `lesson::08`. Anki shows `08` under
`lesson` in its tag list.

### The automatic deck tag

janky adds one tag to every note. The tag shows the deck of the note:

| Deck | Tag |
|------|-----|
| The root deck | `deck` |
| The child deck `vocab` | `deck::vocab` |
| The child deck `basics` inside the child deck `lessons` | `deck::lessons::basics` |

The tag uses the deck **keys** from `deck.toml`, not the deck names. If you change the `name` of a
deck, the tag stays the same.

### How Anki changes your tags

janky does not check the text of a tag. Anki changes some tags when it stores them:

- A space separates tags. `'two words'` becomes the two tags `two` and `words`.
- Anki tags ignore case. `'Lesson::08'` and `'lesson::08'` become one tag. Anki keeps the first
  spelling.
- Anki removes a tag that occurs twice, and an empty tag (`''`).

> [!TIP]
> Use lowercase letters, digits, `-` and `::` in tags. Then the tags in Anki are the same as the
> tags in your files.

If the note type uses a full JSON Schema file (see [The content schema](../notetypes/schema.md)),
janky does not check `tags`. janky then ignores a `tags` value that is not a list, and each item
that is not text.

### Tags that you add in Anki

A change to the tags of an entry changes the note. `janky plan` shows it as `~ note <id>`, and the
next import updates the note in Anki.

When an import updates a note, Anki replaces the note's tags with the tags from the package. This
also removes tags that you add in Anki, and tags that Anki adds itself, such as `leech` and
`marked`. A note keeps its Anki tags only until its entry changes.

> [!IMPORTANT]
> Put every tag that you want to keep in your content files.

## Defaults: values for every entry in a file

`defaults` holds values that janky copies into each entry of the file. It can hold `type`, `tags`
and any key of the note type's schema.

```yaml
defaults:
  tags: ['basics']

entries:
  - id: 'casa'
    spanish: 'casa'
    english: 'house'
  - id: 'perro'
    spanish: 'perro'
    english: 'dog'
    tags: ['basics', 'animals']
```

An `id` in `defaults` has no effect. Each entry must have its own `id`.

### An entry value replaces the default value

janky copies a default key only into entries that do not have that key. If an entry has the key,
janky uses the entry's value and ignores the default. janky never combines the two values.

This is important for `tags`. In the example above:

- `casa` has no `tags`, so it gets `basics`.
- `perro` has its own `tags`, so it does not get `basics` from `defaults`. That is why the entry
  repeats `basics`.
- An entry with `tags: []` has no tags from the file. It keeps only the automatic deck tag.

The same rule applies to every list and every nested object. An entry's list replaces the default
list as a whole.

### An error in defaults affects every entry

If a value in `defaults` is wrong, janky cannot use any entry of the file. janky shows the error
once, not once for each entry.

The message names the first entry, but the line and column point at the value in `defaults`. For
example, an unknown key `bogus` in `defaults` gives "unknown key `entries[0].bogus`", with the
marker on the `bogus: 1` line under `defaults:`. Correct the value in `defaults`.
