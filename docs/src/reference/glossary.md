# Glossary

This page explains the words that this guide uses. Each entry links to the page with the details.

**alias**
: A second name for a note type, which entries can use in their `type`. You set it with `alias` in
  `[notetypes.<key>]`. See [Adding note types](../deck/notetypes.md).

**.apkg (package)**
: The file that `janky build` writes and that Anki imports with **File → Import**. It holds the
  decks, presets, note types, notes and media. See [Importing into Anki](../cli/importing.md).

**card**
: One thing that you study in Anki. Anki makes one card from a note for each card type of its note
  type. See [Card templates](../notetypes/cards.md).

**card template**
: The front and back HTML of a card type, in Anki's own template language. Anki uses it each time it
  shows a card. See [Card templates](../notetypes/cards.md).

**child deck**
: A deck inside another deck, for example `Spanish::1 Vocabulary`. You make it with a
  `[deck.children.<key>]` table. See [Decks and child decks](../deck/decks.md).

**content file**
: A YAML, TOML or JSON file that holds entries. The `content` patterns of a deck tell janky which
  files belong to that deck. See [Content files](../content/files.md).

**content schema**
: The list of keys that an entry of a note type can have, with the type of each value. It is the
  `[schema]` table of a note type, or a JSON Schema file. See
  [The content schema](../notetypes/schema.md).

**data generator**
: A generator that makes data, for example a dictionary lookup. A field template gets the data with
  `enrich(...)`. See [Kinds of plugin](../plugins/kinds.md).

**deck**
: An Anki deck. A project has one root deck, and the root deck can have child decks. See
  [Decks and child decks](../deck/decks.md).

**deck.lock**
: The file where janky keeps the ids that link your entries to the notes in Anki. janky writes it.
  Keep it with your project and never delete it. See
  [Changing content safely](../content/changing.md).

**deck.toml**
: The file that defines the project: the decks, presets, note types and plugin instances. See
  [deck.toml reference](../deck/reference.md).

**defaults**
: A table at the top of a content file. Every entry of the file gets its values, unless the entry
  sets the key itself. See [Entries: ids, types, tags and defaults](../content/entries.md).

**diagnostic**
: An error or a warning that janky prints. It has a code, a message and usually the file, line and
  column. See [Reading errors and warnings](../cli/diagnostics.md) and
  [Diagnostic codes](codes.md).

**entry**
: One item in a content file. One entry becomes one Anki note. See
  [Entries: ids, types, tags and defaults](../content/entries.md).

**entry id**
: The `id` of an entry, for example `casa`. It must be unique in the whole project. If you change it,
  Anki sees a new note. See [Entries: ids, types, tags and defaults](../content/entries.md).

**entry type**
: The `type` of an entry. It is the key or an alias of a note type. An entry without a `type` uses
  the default note type. See [Entries: ids, types, tags and defaults](../content/entries.md).

**field**
: One value of an Anki note, for example "Spanish" or "English". The note type file defines the
  fields in `[fields.<key>]` tables. See [Field templates](../notetypes/fields.md).

**field template**
: The janky template that makes the value of an Anki field from an entry, at build time. It uses the
  Jinja template language, for example `{{ entry.spanish }}`. See
  [Field templates](../notetypes/fields.md).

**filter**
: A template helper that changes a value, written after `|`, for example
  `{{ entry.notes | markdown }}`. janky has built-in filters, and plugins can add more. See
  [Filters, functions and context](../notetypes/template-reference.md).

**function**
: A template helper that you call with parentheses, for example `media('images/casa.png')`. janky has
  built-in functions, and plugins can add more. See
  [Filters, functions and context](../notetypes/template-reference.md).

**generator**
: A plugin that makes media or data during `janky generate`, for example audio from a text-to-speech
  service. janky never runs a generator during `check`, `plan` or `build`. See
  [Generating media and data](../plugins/generating.md).

**GUID**
: The hidden id with which Anki finds an existing note. janky makes it from the entry id, the note
  type key and a random value in `deck.lock`. See [How janky works](../how-it-works.md).

**job**
: One request that a generator must run, for example one audio clip for one word. `janky plan` lists
  the jobs, and `janky generate` runs them. See [Generating media and data](../plugins/generating.md).

**key**
: The name of a table in `deck.toml` after the dot, for example `vocab` in `[deck.children.vocab]`.
  janky uses keys to keep track of things, so do not change them. The `name` is the text that Anki
  shows. See [deck.toml reference](../deck/reference.md).

**media directory**
: A directory that holds the images and audio that your entries name. You list these directories in
  `media_dirs`. See [Images and audio](../content/media.md).

**media generator**
: A generator that makes media files, for example audio. A field template gets the file with
  `media(text, 'instance')`. See [Kinds of plugin](../plugins/kinds.md).

**note**
: An Anki note. It holds the fields, and Anki makes its cards from them. One entry becomes one note.
  See [How janky works](../how-it-works.md).

**note type**
: Anki's note type: the fields of a note, the card types and the styling. In janky, a note type is a
  `notetype.toml` file with its templates and CSS. See [Note type files](../notetypes/structure.md).

**note type plugin**
: A plugin that gives filters and functions to the field templates of one note type. The note type
  file names it in `plugins`. It cannot use the network or secrets. See
  [Kinds of plugin](../plugins/kinds.md).

**plan**
: The list of what the next build adds, changes or removes in Anki, with the import options to use.
  `janky plan` shows it, and `janky build` shows it again. See
  [The everyday workflow](../cli/workflow.md).

**plugin**
: A script in the Lua language that adds to janky. It can give filters and functions, or it can be a
  generator. See [Kinds of plugin](../plugins/kinds.md).

**plugin instance**
: A `[plugins.<name>]` table in `deck.toml`. It names a plugin script and gives its settings. Field
  templates use the instance name, for example `enrich(entry.spanish, 'frequency')`. See
  [Plugin instances and secrets](../plugins/instances.md).

**preset**
: Anki's deck options preset: new cards a day, reviews a day, learning steps and similar settings.
  See [Presets](../deck/presets.md).

**project**
: The directory that holds `deck.toml`. All paths in `deck.toml` are relative to this directory. See
  [Project layout](layout.md).

**prune**
: The command `janky prune`. It deletes cached generator results that no entry uses. See
  [Generating media and data](../plugins/generating.md).

**remote note type**
: A note type that janky downloads from a Git repository or an archive URL. `deck.lock` records the
  exact version, and `janky update` moves to a newer version. See
  [Adding note types](../deck/notetypes.md).

**root deck**
: The top deck of a project, defined by the `[deck]` table. See [Decks and child decks](../deck/decks.md).

**schema**
: A list of the keys and values that a file can have. janky checks your files against schemas.
  `janky schema` writes them for your editor. See [The content schema](../notetypes/schema.md).

**secret**
: A private value that a generator needs, for example an API key. You give it in an environment
  variable, a `.env` file or another file, never in `deck.toml`. See
  [Plugin instances and secrets](../plugins/instances.md).

**variables**
: The settings of a plugin instance, in `[plugins.<name>.variables]`. The plugin defines which
  variables it accepts. See [Plugin instances and secrets](../plugins/instances.md).
