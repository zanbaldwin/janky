# Decks and child decks

The `[deck]` table is the root deck of the project. A root deck can have child decks, and each
child deck can have its own child decks. janky puts every deck into one package.

## The root deck

```toml
[deck]
name = "Spanish"
output = ".janky/build/spanish.apkg"
media_dirs = ["media"]
new_order = "shuffle-in-file"
preset = "spanish"
```

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `name` | string | required | The deck name that Anki shows. The root deck name is also the project name. |
| `output` | string (a file path) | `.janky/build/deck.apkg` | Where `janky build` writes the package. Root deck only. |
| `media_dirs` | list of strings | `["media"]` | The directories where janky looks for media files, in this order. Root deck only. |
| `new_order` | string | `"file"` | The order of new cards inside each content file: `"file"` or `"shuffle-in-file"`. Root deck only. See [The order of new cards](#the-order-of-new-cards). |
| `preset` | string | Anki's "Default" preset | The key of a `[presets.<key>]` table. See [Presets](presets.md). |
| `content` | list of strings | `[]` | The content files of this deck, as patterns. |
| `children` | table of decks | none | The child decks. |

## Child decks

A child deck is a table under `children`. The name after `children.` is the deck **key**:

```toml
[deck.children.vocab]
name = "1 Vocabulary"
preset = "beginner"
content = ["content/vocab/*.yaml"]

[deck.children.phrases]
name = "2 Phrases"
preset = "beginner"
content = ["content/phrases/*.toml"]
```

A child deck has the same keys as the root deck, except `output`, `media_dirs` and `new_order`. If
you put one of these keys in a child deck, janky gives the error `config::unknown-key`.

You can nest child decks to any depth. Add `children` again under a child deck:

```toml
[deck.children.vocab.children.food]
name = "Food"
content = ["content/vocab/food/*.yaml"]
```

## Full deck names

Anki shows a deck tree. The full name of a deck is the full name of its parent, then `::`, then
its own `name`. In the tutorial project, the full names are `Spanish`, `Spanish::1 Vocabulary`
and `Spanish::2 Phrases`. `janky plan` shows these full names:

```console
$ janky plan
...
+ deck Spanish
+ deck Spanish::1 Vocabulary
+ deck Spanish::2 Phrases
...
```

Two decks cannot have the same full name. If they do, janky gives the error
`config::duplicate-deck` ("two decks have the full name ..."). Use child decks to make a tree. Do
not put `::` in a `name`, and do not leave a `name` empty.

## Names and keys

A deck has two names, and they have different jobs:

- The **key** (`vocab` in `[deck.children.vocab]`) is the identity of the deck in your project.
  `deck.lock` stores the Anki id of the deck under this key.
- The **name** (`"1 Vocabulary"`) is the text that Anki shows.

A key starts with a lowercase letter. After that, it holds only lowercase letters, digits, `_`
and `-`. The same rule applies to preset keys, note type keys and plugin instance names. A key
that breaks the rule gives the error `config::key`:

```text
config::key
  x `Extra` is not a valid key
  help: a key starts with a lowercase letter and holds only lowercase letters, digits, `_` and `-`
```

Do not change a key after you import the deck. To janky, a new key is a new deck, and the deck
with the old key is removed.

Every note also gets an automatic tag from the deck keys. Notes in the root deck get the tag
`deck`. Notes in a child deck get `deck::` and the key path, for example `deck::vocab`. A change
to a `name` does not change this tag. A change to a key does.

## Renaming a deck

You can change the `name` of a deck. But when you import, Anki does not rename the deck. Anki
makes a new, empty deck with the new name, and your cards stay in the old deck. `janky plan` warns
you with a line in this form:

```text
~ deck <old full name> is renamed to <new full name>: rename it in Anki before you import
```

To rename a deck:

1. Change the `name` in `deck.toml`.
2. Run `janky plan` and find the `is renamed to` line.
3. In Anki, rename the deck to the new full name.
4. Run `janky build` and import the package.

## The package file

`output` sets where `janky build` writes the `.apkg` file. The path must stay inside the project
directory. A path outside it, or an absolute path, gives the error `config::outside-project`:

```text
config::outside-project
  x `../out.apkg` is outside the project directory
    `-- the package must be written inside the project directory
```

## Media directories

`media_dirs` lists the directories where janky looks for the media files that your field
templates refer to. janky looks in the directories in the order of the list. An empty list means
`["media"]`. If a directory does not exist, janky gives a warning, not an error:

```text
config::missing-media-dir
  ! media directory `pics` does not exist
```

[Images and audio](../content/media.md) explains how janky finds a media file.

## Content files

`content` is a list of patterns. Each pattern selects content files for this deck. A pattern is a
path relative to the project directory, with these special characters:

| Pattern | Matches |
|---------|---------|
| `*` | Any characters in one part of a path. It does not match `/`. |
| `**` | Zero or more directories. |
| `?` | One character. |
| `[abc]` | One of the characters in the brackets. |

Some examples:

- `content/vocab/*.yaml` matches `content/vocab/01-basics.yaml`, but not
  `content/vocab/food/fruit.yaml`.
- `content/vocab/**/*.yaml` matches both.

A content file must end in `.yaml`, `.yml`, `.toml` or `.json`. A matched file with another
extension gives the error `content::format`. A `*` also matches hidden files, such as the swap
files that some editors make. So end each pattern with an extension, for example `*.yaml`, not
`*`.

A pattern that matches no file is not an error, and janky gives no warning. If a deck has fewer
notes than you expect, check its patterns. An invalid pattern (for example `content/[`) gives the
error `content::pattern`.

### A file can belong to one deck only

Two patterns of the same deck can match the same file. janky loads the file once. But if the
patterns of two different decks match one file, janky gives the error `content::two-decks`:

```text
content::two-decks
  x `<file>` already belongs to deck `<full deck name>`
```

janky reads the root deck first, then the child decks in the order of `deck.toml`. The first deck
keeps the file. The error points at the pattern of the later deck.

### The order of new cards

janky reads all content files of the project in the order of their paths, across all decks. This
order is also the order of new cards in Anki. In the tutorial, `content/phrases/...` comes before
`content/vocab/...`, so the phrases get the first new-card positions.

The root deck key `new_order` sets the order inside each file:

| Value | Meaning |
|-------|---------|
| `"file"` (default) | The entries keep their order in the file. |
| `"shuffle-in-file"` | janky shuffles the entries of each file. The files stay in path order. |

Use `"shuffle-in-file"` if each content file is one lesson. You then learn the lessons in order,
but the entries of one lesson come in a mixed order.

The shuffle uses a hash of each note's GUID, so each build gives the same order. A new entry does
not move the other entries.

> [!NOTE]
> Anki keeps the position of a card that it already has. A change to `new_order` or to the entry
> order moves only the cards of new notes. To shuffle the cards that Anki already has, use
> **Browse > Reposition** in Anki with **Randomize order**.

## Decks without content

A deck with no `content` has no notes of its own. Use it as a parent for child decks.

Anki does not make a deck that has no cards. If a child deck has cards, Anki also makes its parent
decks.
