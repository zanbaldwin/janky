# Changing content safely

You can change your content files at any time and import the new package into the same Anki
profile. Most changes update your notes and keep their review history. Some changes make a new note
instead, or need a step in Anki. This page tells you which changes do what.

## How Anki recognises a note

Anki finds an existing note by a hidden id. janky makes this id from three things:

- the entry's `id`
- the key of the entry's note type in `deck.toml`
- a random value that janky stores in `deck.lock` the first time that you build

If these three things stay the same, the import updates the existing note. The deck, the file, the
position in the file and the field values are not part of the hidden id. You can change them, and
the note keeps its review history.

## What each change does

Run `janky plan` before each build. It shows each change, and it tells you what to do in Anki.

| You change | `janky plan` shows | What Anki does on import |
|------------|--------------------|--------------------------|
| A value or a tag of an entry | `~ note <id> in <deck>` | Updates the note. The review history stays. |
| The `id` of an entry | `+ note <new id> in <deck>` and `- note <old id> in <deck>: the note stays in Anki. Delete it there` | Adds a new note with no review history. The old note stays. |
| The `type`, to an entry type of another note type | `~ note <id> changes note type from <old> to <new>: janky adds a new note. Delete the old note in Anki` | Adds a new note with no review history. The old note stays. |
| The `type`, to another entry type of the same note type | `No changes.` | Nothing. |
| The file of an entry, to another file of the same deck | `No changes.` | Nothing. |
| The deck of an entry (you move it to a file of another deck) | `~ note <id> moves from <old deck> to <new deck>: its cards stay in <old deck>. Move them in Anki (Browse, Change Deck)` | Updates the note, but its cards stay in the old deck. |
| You delete an entry | `- note <id> in <deck>: the note stays in Anki. Delete it there` | Nothing. An import never deletes a note. |
| You put a deleted entry back, with the same `id` and note type | `+ note <id> in <deck>` | If the old note is still in Anki, updates it. The review history stays. |
| The `name` of a deck in `deck.toml` | `~ deck <old> is renamed to <new>: rename it in Anki before you import` | If you do not rename the deck in Anki first, Anki makes a new, empty deck. The cards stay in the old deck. |

The summary line at the top of the plan counts a change of note type under "moved".

A change of entry type can still change the note if a field template uses `entry.type`. Then
`janky plan` shows `~ note <id> in <deck>`.

### Examples from the tutorial

After an edit to the entry `gato`:

```console
$ janky plan
...
Notes: 0 added, 1 changed, 0 removed, 0 moved, 8 unchanged
~ note gato in Spanish::1 Vocabulary
```

After you delete the entry `manzana`:

```console
$ janky plan
...
Notes: 0 added, 0 changed, 1 removed, 0 moved, 8 unchanged
- note manzana in Spanish::1 Vocabulary: the note stays in Anki. Delete it there
```

### Work that you must do in Anki

A package can add and update notes, but it cannot delete or move them. So for some changes, you do
one step in Anki:

- If you delete an entry or change its `id`, delete the old note in Anki (in **Browse**).
- If you change the note type of an entry, delete the old note in Anki.
- If you move an entry to another deck, move its cards in Anki (**Browse**, then **Change Deck**).
- If you rename a deck, rename it in Anki **before** you import.

[Importing into Anki](../cli/importing.md) explains the import options.

### Change deck keys with care

Each deck has a key in `deck.toml` (for example `vocab` in `[deck.children.vocab]`). janky uses the
key, not the name, to recognise the deck. If you change a key, janky sees a removed deck and a new
deck. The automatic deck tag of each note in the deck also changes (see
[tags](entries.md#the-automatic-deck-tag)). To change what Anki shows, change the `name`, and keep
the key.

## Never delete deck.lock

`deck.lock` holds the random value that is part of each hidden note id. janky writes this file when
you build. Keep it with your project, and if you use Git, commit it.

> [!WARNING]
> If you delete `deck.lock`, the next build makes a new random value. Every note then gets a new
> hidden id. Anki imports the whole deck again as new notes, with no review history. `janky plan`
> does not warn you about this. It shows every note as `+ note`.

There is one case where you delete `deck.lock` on purpose. If you copy another person's project to
start your own deck, delete the copied `deck.lock` before your first build. Your deck then gets its
own ids.

## Edits in Anki do not last

janky never reads your Anki collection. If you edit a note in Anki, the next import of that note
replaces your edit with the text from your files. The same applies to tags that you add in Anki
(see [Tags that you add in Anki](entries.md#tags-that-you-add-in-anki)).

Make every change in your content files, then build and import again.
