# How janky works

This page explains the main ideas behind janky. The other chapters assume that you know them.

## Your files are the source of truth

A janky **project** is a directory of text files. janky reads these files and writes an Anki package
(an `.apkg` file). You then import the package into Anki.

```text
your files  ──janky build──▶  deck.apkg  ──File → Import──▶  Anki
```

The direction is one way. janky never reads your Anki collection, and it never writes your files. If
you edit a note inside Anki, the next import that updates that note replaces your edit with the text
from your files. Make every change in your files.

janky writes only these things:

- the `.apkg` package.
- the `deck.lock` file.
- its own `.janky/` directory (build output and caches).
- a cache of downloaded note types in your home directory, if your project uses one (see
  [Project layout](reference/layout.md#the-user-wide-cache)).

## The parts of a project

| Part | File | What it holds |
|------|------|---------------|
| The deck definition | `deck.toml` | The deck names, the deck options (presets), the note types and the plugins |
| Note types | `notetype.toml`, plus HTML, CSS and JavaScript files | The fields of a note, how janky fills them, and the card designs |
| Content | YAML, TOML or JSON files | Your notes, one **entry** for each note |
| Media | Images and audio files | The files that your cards show or play |
| Plugins | Lua scripts | Optional add-ons: text helpers, audio, dictionary data |
| The lock | `deck.lock` | The ids that link your entries to notes in Anki (janky writes it) |

Only `deck.toml` must have that name. `deck.toml` tells janky where everything else is.

## From an entry to a card

Each entry in a content file becomes one Anki note. The entry has an `id` and your own keys, for
example `spanish` and `english`.

1. janky finds the note type of the entry.
2. janky checks the entry against the note type's **schema** (the list of keys that an entry can
   have).
3. janky fills each Anki field from a **field template**. A field template is a small text
   template that uses the entry's keys, for example `{{ entry.spanish }}`.
4. Anki makes the cards from the **card templates** (front and back) when you study.

So there are two kinds of template. Field templates run in janky, once, at build time. Card
templates run in Anki, each time Anki shows a card. [Creating note types](notetypes/index.md)
explains both.

## The lock file keeps your review history

Anki finds an existing note by a hidden id (a GUID). janky makes this id from your entry's `id`, the
key of its note type and a random value that it stores in `deck.lock`. While these stay the same, a new import
updates the note in Anki and keeps its review history.

- Change the text of an entry: the note in Anki updates.
- Change the `id` of an entry: Anki sees a new note. The old note stays in Anki.
- Delete `deck.lock`: Anki sees a new note for every entry. You get a duplicate of the whole deck.

Keep `deck.lock` with your project. If you use Git, commit it.

## Check, plan, build

You use three commands most of the time:

| Command | What it does | Writes files? |
|---------|--------------|---------------|
| `janky check` | Finds errors in all your files | No |
| `janky plan` | Shows what the next build adds, changes or removes, and which import options to tick in Anki | No |
| `janky build` | Writes the `.apkg` package and updates `deck.lock` | Yes |

These commands give the same result each time you run them on the same files. They do not send
your content to any service. (The only network use is to download a shared note type for the first
time, if your project uses one.)

## Generators run only when you ask

Some plugins make content for you. A text-to-speech plugin makes audio, and a dictionary plugin
looks up data. These plugins are **generators**. A generator can cost money, so janky never runs it
during `check`, `plan` or `build`.

1. `janky plan` lists the **jobs** that the generators must run (for example, one audio clip for
   each new word).
2. `janky generate` shows a summary of the jobs, the services that it contacts and the secrets, and
   asks you to confirm.
3. janky keeps each result in `.janky/cache/`. It does not ask a service again for a result that is
   in the cache.
4. `janky build` puts the results into the package.

[Using plugins](plugins/index.md) explains this in full.
