# Project layout

This page lists the files and directories of a janky project. It tells you which program writes
each one, which ones to keep in Git, and which ones you can delete.

## The project tree

Only `deck.toml` must have this name. The other names are the usual choices, and `deck.toml` tells
janky where each part is. The tree below is the [tutorial](../tutorial/index.md) project, with the
files that janky writes.

```text
spanish/
  deck.toml                    the deck definition
  deck.lock                    the ids that link entries to notes in Anki (janky writes it)
  .gitignore                   the files that Git must not keep (you write it)
  .env                         secrets for generators (optional, you write it)
  notetypes/                   note types
    word/
      notetype.toml
      style.css
      spanish.lua              a note type plugin
      assets/reveal.js
      fields/examples.html.j2
      cards/recognition.front.html  cards/recognition.back.html
      cards/production.front.html   cards/production.back.html
    phrase.toml                a note type in one file
    phrase/front.html  phrase/back.html
  content/                     content files
    vocab/01-basics.yaml  vocab/02-food.yaml
    phrases/01-greetings.toml  phrases/02-travel.toml
  media/                       images and audio that your entries name
    images/casa.png
  plugins/                     plugins for [plugins.<name>] instances
    frequency.lua
  data/frequency.json          files that a generator reads
  .janky/                      janky's own directory
    build/spanish.apkg         the package (the default name is deck.apkg)
    schema/                    editor schemas from `janky schema`
      deck.schema.json  notetype.schema.json  lock.schema.json  content.schema.json
    cache/
      media/                   generated media: <hash>.<ext> and <hash>.json
      data/                    generated data: <hash>.json
      plugins/<instance>/      a plugin instance's own files and cache.sqlite
    generate.lock              the lock of `janky generate`, `janky prune` and `janky pull`
    publish/                   the copy of your collection for `janky publish`
      collection.anki2  collection.media/  collection.mdb  state.json  lock
```

During a build, janky also writes `deck.lock.tmp` for a moment. It renames this file to `deck.lock`
when the build is complete. If `deck.lock.tmp` stays after a build, see
[Troubleshooting](troubleshooting.md).

## The user-wide cache

janky keeps downloaded note types (from Git or from an archive URL) outside the project, in one
cache for all your projects:

- In `$XDG_CACHE_HOME/janky/sources/`, if the `XDG_CACHE_HOME` environment variable is set.
- Otherwise in `~/.cache/janky/sources/`.

You can delete this directory. janky downloads the note types again at the next `check`, `plan` or
`build` (but not with `--offline`). [Adding note types](../deck/notetypes.md) explains remote note
types.

## What to keep and what to delete

| File or directory | Written by | Keep in Git? | Safe to delete? |
|-------------------|------------|--------------|-----------------|
| `deck.toml` | You | Yes | No. It is the project. |
| `deck.lock` | `janky build`, `janky update`, `janky publish` | Yes | **Never.** Without it, the next import makes a copy of every note, with no review history. |
| `deck.lock.tmp` | `janky build`, `janky update` (for a moment) | No | Yes, when no janky command runs. |
| `notetypes/`, `content/`, `media/`, `plugins/` | You | Yes | No. They are your deck. |
| Files that a generator reads (for example `data/`) | You | Yes | No. |
| `.env` and other secret files | You | **Never** | Yes, but `janky generate` then cannot find the secrets. |
| `.gitignore` | You | Yes | Yes |
| `.janky/build/` | `janky build` | No | Yes. `janky build` makes the package again. |
| `.janky/schema/` | `janky schema` | No | Yes. `janky schema` makes the files again. |
| `.janky/cache/media/` | `janky generate`, `janky pull` | Usually no | **Back it up.** It holds results that a service made, and some services charge for each result. |
| `.janky/cache/data/` | `janky generate` | Usually no | **Back it up**, for the same reason. |
| `.janky/cache/plugins/` | Plugins, during `janky generate` | No | Yes, but the plugin makes it again on the next run. This can mean a large download, for example a whole dictionary. |
| `.janky/generate.lock` | `janky generate`, `janky prune`, `janky pull` | No | Not necessary. The lock ends when janky stops, also after a crash. |
| `.janky/publish/` | `janky publish` | No | The collection files, yes: `janky publish` downloads them again. Do not delete `state.json`. [The local copy](../cli/publishing.md#the-local-copy) explains why. |
| `~/.cache/janky/sources/` | Each command that loads the project | Not in the project | Yes. janky downloads again. |

janky does not write a `.gitignore` file for you. Put at least these lines in it:

```text
.janky/
.env
```

> [!WARNING]
> Do not delete the whole `.janky/` directory to "clean up". The generated audio and data in
> `.janky/cache/` exist only there. If you delete them, `janky generate` must ask the services again,
> and a paid service charges you again. To remove only the results that no entry uses, run
> [`janky prune`](../plugins/generating.md).

> [!TIP]
> If you want the generated results in Git, remove `.janky/` from `.gitignore` and add
> `.janky/build/`, `.janky/schema/` and `.janky/generate.lock` instead. Then Git keeps `.janky/cache/`. Large plugin
> downloads in `.janky/cache/plugins/` can make the repository big, so you can also add that
> directory to `.gitignore`.

## Two decks in one directory

The `.janky/` directory belongs to the directory, not to one deck file. If two deck files are in one
directory (you choose each one with `--deck`), they share one cache. Then `janky prune` for one deck can
delete the results of the other deck. Put each deck in its own directory.
