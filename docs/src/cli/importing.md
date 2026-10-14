# Importing into Anki

janky writes a package, and you import it into Anki. This page explains the import options, the
steps of an import and what happens in Anki for each kind of change.

If you run your own Anki sync server, `janky publish` does the work of an import for you, also the
work in Anki that this page lists. [Publishing through a sync server](publishing.md) explains it.

## The import options

When you import a package, Anki shows some options. These are the options that matter for janky,
with the Anki defaults:

| Option | Anki default | What to use |
|--------|--------------|-------------|
| Update notes | If newer | Keep **If newer**. Use **Always** only when the plan tells you to. |
| Update note types | If newer | Keep **If newer**. |
| Merge note types | Off | Turn it on only when the plan tells you to. |
| Import any deck presets | Off | Tick it on the first import, and when the plan adds a preset. |

Anki remembers the options of your last import. Look at them again before each import.

`janky plan` and `janky build` tell you which options to change, in the `When you import:` part.
[Command reference](commands.md#the-advice-lines) lists every advice line.

## Import a package

1. Run `janky plan`. Read every line.
2. If the plan says that a deck is renamed, rename the deck in Anki now.
3. Run `janky build`. The `wrote` line gives the path of the package, for example
   `.janky/build/spanish.apkg`.
4. In Anki, select **File → Import**.
5. Select the package file.
6. Set the import options that the `When you import:` lines give. If there are no such lines, use
   the options in the table above.
7. Select **Import**.
8. Do the work in Anki that the plan lines ask for. For example, delete removed notes, or move
   cards to their new deck.
9. If this was your first import, turn on FSRS (see below).

## What each option does

### Update notes: If newer

janky gives a note a new modification time only when the note changed. With **If newer**, Anki
updates the notes that you changed, and does not touch the other notes. This is correct for most
imports.

### Update note types: If newer

With **If newer**, changes to the card templates and the styles (CSS) reach Anki. You do not need
**Merge note types** for these changes. The plan shows them as
`~ note type <name>: card templates or CSS`.

### Import any deck presets

A preset is a group of deck options in Anki (new cards per day, review limits, learning steps and so
on). [Presets](../deck/presets.md) explains how you define them in `deck.toml`.

Anki adds a preset from a package only once, and only when **Import any deck presets** is ticked. A
later change to a preset never reaches Anki through a package. When you change a preset in
`deck.toml`, the plan shows this line:

```text
~ preset Spanish beginner: a package cannot change a preset that Anki has. Change it in Anki
```

Then make the same change in Anki, in the deck options.

### Merge note types

A schema change is a change to the fields or card types of a note type: you add, remove or move
one. The plan shows it like this:

```text
~ note type Spanish Word: fields added: plural. Needs a full sync
When you import:
  Turn on "Merge note types". The import needs a full sync afterwards.
  Set "Update notes" to "Always", so that the new fields get their values.
```

For a schema change, turn on **Merge note types**. Without it, Anki adds a copy of the note type
(with a name such as "Spanish Word+") and skips the notes. After a merge, the next sync is a full
sync.

### Update notes: Always, after you add fields

When you add fields, set **Update notes** to **Always** for that import. A merge gives every note
of the note type the time of the import. Then, with **If newer**, Anki skips the notes from the
package, and the new fields stay empty.

### Removed and moved fields

A package cannot remove a field or a card type from a note type in Anki, and it cannot change their
order. The plan says so:

```text
~ note type Spanish Word: fields added: remarks; fields removed: notes. Needs a full sync. Removed or moved fields and card types do not reach Anki: change them there
```

Make the same change in Anki: select **Tools → Manage Note Types**, then the note type, then
**Fields** or **Cards**.

## Turn on FSRS

FSRS is the scheduler in Anki that uses the desired retention of your presets. It is a setting of the
whole collection, not of a deck, so a package cannot turn it on. Each successful build reminds you:

```text
In Anki, turn FSRS on. A package cannot change collection settings.
```

Turn it on once in Anki, in the deck options. After that, you can ignore the reminder.

## What happens in Anki when you change something

An import adds and updates. It never deletes anything in Anki, and it never moves cards. This table
shows the result of each kind of change, and what you must do.

| You change this in your files | What happens in Anki | What to do |
|-------------------------------|----------------------|------------|
| The text, fields or tags of an entry | The note updates. Its review history stays. | Nothing. |
| You remove an entry | The note stays in Anki. | If you do not want it, delete the note in Anki. |
| The `id` of an entry | Anki adds a new note with no review history. The old note stays. | Delete the old note in Anki. Better: do not change ids. |
| The note type of an entry | janky gives the note a new identity. Anki adds a new note with no review history. The old note stays, with its history. | Delete the old note in Anki. |
| You move an entry to a different deck | The fields update, but the cards stay in the old deck. | In Anki, select **Browse**, select the cards, then **Change Deck**. |
| The `name` of a deck | Anki makes a new, empty deck with the new name. The cards stay in the old deck. | Rename the deck in Anki **before** you import. |
| You remove a deck, preset, note type or media file | It stays in Anki. | Delete it in Anki if you do not want it. For media, use **Tools → Check Media**. |
| A preset | Nothing. A package cannot change a preset that Anki has. | Make the same change in the deck options in Anki. |
| You add a preset | Anki adds it, but only if **Import any deck presets** is ticked. | Tick that option. |
| Card templates or CSS | The note type updates. | Nothing. Keep **Update note types** at **If newer**. |
| You add fields or card types | The note type updates. | Turn on **Merge note types**. For new fields, also set **Update notes** to **Always**. Then do a full sync. |
| You remove or reorder fields or card types | With **Merge note types**, the note type keeps all its fields in the old order. A removed field becomes empty in the notes that update. | Make the same change in Anki. |
| A deck that has no cards | Anki does not make the deck. It makes a parent deck when a child deck has cards. | Nothing. |

[Changing content safely](../content/changing.md) explains how to change ids and note types without
losing review history.

### Edits that you make in Anki

janky never reads your Anki collection. If you edit a note in Anki, the next import that updates
that note replaces your edit with the text from your files. Make every change in your files.

Tags that you add in Anki can also go when the note updates, because the note gets the tags from
your files. This is not confirmed with a real import yet.

### Never delete deck.lock

`deck.lock` holds the identity of each note. If you delete it, janky makes new identities for every
note. Anki then imports the whole deck again as duplicates, with no review history. `janky plan`
does not warn you about this. It only shows every note as added.

> [!WARNING]
> If `janky plan` shows every note as added for a deck that you already imported, stop. Do not
> import. Get `deck.lock` back from your backup or from Git.
