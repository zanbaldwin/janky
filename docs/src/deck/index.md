# Defining a deck

Each project has one `deck.toml` file. It is the only file with a fixed name, and it tells janky
where everything else is. It holds four tables:

- `[deck]`: the root deck, its child decks, and the content files of each deck (required).
- `[presets]`: the Anki deck options of your decks (optional).
- `[notetypes]`: the note types of the project (optional in the file, but a project needs at least
  one to build).
- `[plugins]`: the plugin instances of the project (optional).

All paths in `deck.toml` are relative to the directory that holds it. janky reads `deck.toml`, but
it never writes it.

## A short TOML primer

`deck.toml` is a TOML file. TOML is a plain text format for settings. You only need these parts:

```toml
[deck]                         # a table: a group of keys, until the next table header
name = "Spanish"               # a key and a string value (strings are in double quotes)

[deck.children.vocab]          # a dotted header: the table `vocab` inside `children` inside `deck`
content = ["content/vocab/*.yaml"]   # a list (TOML calls it an array): values in [ ]

[plugins.frequency.filesystem]
read = { lists = "data" }      # an inline table: a small table on one line, in { }
```

Text after `#` is a comment. Numbers (`15`, `0.9`) and booleans (`true`, `false`) have no
quotes.

## An example

This is the `deck.toml` of the tutorial project:

```toml
[deck]
name = "Spanish"
output = ".janky/build/spanish.apkg"
media_dirs = ["media"]
preset = "spanish"

[deck.children.vocab]
name = "1 Vocabulary"
preset = "beginner"
content = ["content/vocab/*.yaml"]

[deck.children.phrases]
name = "2 Phrases"
preset = "beginner"
content = ["content/phrases/*.toml"]

[presets.spanish]
name = "Spanish"
new_per_day = 10
reviews_per_day = 100

[presets.beginner]
name = "Spanish beginner"
new_per_day = 15
reviews_per_day = 150
learning_steps = ["1m", "10m", "1h"]
desired_retention = 0.9
maximum_interval = "1y"

[notetypes.word]
path = "notetypes/word"
default = true

[notetypes.phrase]
path = "notetypes/phrase.toml"
alias = "expression"

[plugins.frequency]
path = "plugins/frequency.lua"
[plugins.frequency.variables]
list = "frequency.json"
common_below = 1000
[plugins.frequency.filesystem]
read = { lists = "data" }
```

## The pages in this part

- [Decks and child decks](decks.md): the root deck, child decks, the package file, media
  directories and content files.
- [Presets](presets.md): the Anki deck options, and how Anki imports them.
- [Adding note types](notetypes.md): local note types, and note types from a Git repository or an
  archive.
- [deck.toml reference](reference.md): every key in one place, and the rules that janky checks.
