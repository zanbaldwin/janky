# Troubleshooting

This page gives the usual problems, their cause and the fix. Each answer links to the guide page
with the details. For the meaning of one diagnostic code, see [Diagnostic codes](codes.md).

## Anki and the import

### Anki made duplicates of my whole deck

The `deck.lock` file was deleted or replaced. janky makes the hidden id (GUID) of each note from a
random value in `deck.lock`. A new `deck.lock` gives every note a new id, so Anki adds every note
again, with no review history.

1. Get the old `deck.lock` back from Git or from your backup.
2. Put it in the project directory.
3. Run `janky plan`. Make sure that it shows `changed` or `unchanged` notes, not only `added` notes.
4. In Anki, delete the new copies of the notes (the copies with no review history).

If you cannot get the old `deck.lock` back, the link to the old notes is lost. The next imports
update only the new copies. The old copies keep your review history, but janky cannot update them.

[Changing content safely](../content/changing.md) explains how `deck.lock` keeps the link.

### Anki made duplicates of some notes

A note keeps its link to Anki only while its entry `id` and its note type stay the same. These
changes give a new note in Anki:

- You changed the `id` of an entry.
- You changed the `type` of an entry to a different note type.
- You renamed a note type key in `deck.toml` (for example `[notetypes.word]` to
  `[notetypes.words]`).

`janky plan` shows each of these changes before you build.

1. If you did not want the change, change it back. The old link then returns.
2. If you want the change, import the package. Then delete the old notes in Anki.

See [Changing content safely](../content/changing.md).

### My preset changes do not show in Anki

Anki adds a preset from a package only one time, at the first import. A package cannot change a
preset that Anki already has. `janky plan` tells you this with the line
`~ preset <name>: a package cannot change a preset that Anki has. Change it in Anki`.

1. In Anki, open the deck options of the deck.
2. Make the same change there.

For a new preset, tick **Import any deck presets** in the import window. See
[Presets](../deck/presets.md) and [Importing into Anki](../cli/importing.md).

### FSRS is off after the import

FSRS is a setting of the whole Anki collection. A package cannot change it. `janky build` reminds
you each time with `In Anki, turn FSRS on. A package cannot change collection settings.`

1. In Anki, open the deck options.
2. Turn FSRS on.

See [Presets](../deck/presets.md).

### A renamed deck is empty

You changed the `name` of a deck, and you imported before you renamed the deck in Anki. Anki then
makes a new deck with the new name, and the cards stay in the deck with the old name. `janky plan`
warns you with `~ deck <old> is renamed to <new>: rename it in Anki before you import`.

1. In Anki, open **Browse**.
2. Select the cards in the deck with the old name.
3. Use **Change Deck** to move them to the deck with the new name.
4. Delete the deck with the old name.

Next time, rename the deck in Anki first, then import. See [Decks and child decks](../deck/decks.md).

### Cards stay in the old deck after a move

You moved an entry to a content file of a different deck. The import updates the note's fields, but
Anki does not move cards that already exist. `janky plan` shows
`~ note <id> moves from <deck A> to <deck B>: its cards stay in <deck A>. Move them in Anki (Browse, Change Deck)`.

1. In Anki, open **Browse**.
2. Select the cards of the moved notes.
3. Use **Change Deck** to move them to the new deck.

See [Importing into Anki](../cli/importing.md).

### A removed entry is still in Anki

An import never deletes notes. `janky plan` shows
`- note <id> in <deck>: the note stays in Anki. Delete it there`.

1. In Anki, open **Browse**.
2. Find the note and delete it.

The same is true for removed decks, presets, note types and media files. To remove unused media
files, use **Tools → Check Media** in Anki.

### A new field is empty after the import

You added a field to a note type. Anki needs two import options for this change. Without them, Anki
does not update the notes, or it adds a second note type with a `+` at the end of its name.
`janky plan` tells you which options to use, under `When you import:`.

1. In the Anki import window, turn on **Merge note types**.
2. Set **Update notes** to **Always**.
3. Import the package.
4. Do a full sync afterwards.

A removed or moved field cannot reach Anki through a package. Change the note type in Anki. See
[Importing into Anki](../cli/importing.md).

### My edits in Anki were lost

janky writes your files into the package, and the import replaces the note in Anki with this
version. An edit that you made in Anki stays only until the next import changes that note.

1. Make the same change in your content file or in the note type.
2. Run `janky build` and import again.

Make every change in your files. See [How janky works](../how-it-works.md).

### Tags I added in Anki disappeared

When an import updates a note, Anki replaces the note's tags with the tags in the package. This
comes from the Anki import code and is not confirmed with a real import. It can also remove tags
that Anki adds itself, such as `leech` and `marked`.

1. Add the tags to the entry's `tags` in your content file.
2. Run `janky build` and import again.

See [Entries: ids, types, tags and defaults](../content/entries.md).

### New cards come in an unexpected order

janky gives new cards their order from the paths of the content files. It sorts all paths of the
whole project, not deck by deck. In this sort, `10.yaml` comes before `2.yaml`, and capitals come
before lowercase letters.

1. Start file names with numbers that have the same length, for example `01-basics.yaml` and
   `02-food.yaml`.
2. Run `janky build` and import again.

The order applies to new cards only. See [Content files](../content/files.md).

## Fields and content

### A field is empty but there is no error

There are two usual causes.

A misspelt key in a field template, for example `{{ entry.englsh }}`, gives an empty value and no
error.

1. Open the note type file of the field.
2. Compare each `entry.<key>` with the keys of your entries.
3. Correct the spelling.

A generator result is not there yet. Before `janky generate` runs, a field that uses `enrich(...)`
is empty, and a field that uses `media(..., 'name')` can be empty. A data result with an
`enrich::warning` or an `enrich::info` (for example "not in the frequency list") is also empty.

1. Run `janky plan --verbose`. Look for `Generator jobs:` and for `enrich::warning` and
   `enrich::info` lines.
2. If there are jobs, run `janky generate`.
3. Run `janky build` and import again.

See [Field templates](../notetypes/fields.md) and
[Generating media and data](../plugins/generating.md).

### A YAML value gives a type error

YAML reads some values without quotes as numbers, booleans or empty values, for example `012`,
`True`, `1e3` and `~`. The `help` line shows the value with quotes.

```text
help: quote the value: '012'
```

1. Put single quotes around the value, for example `'012'`.
2. Run `janky check` again.

For a boolean field, write `true` or `false`. The value `yes` is text, not a boolean. See
[Content files](../content/files.md).

### My editor swap file breaks the build

Some editors save a hidden file next to the file that you edit, for example `.01-basics.yaml.swp`.
A pattern that ends in `*` also matches this file. janky then gives `content::format`, because the
file is not YAML, TOML or JSON.

1. Open `deck.toml`.
2. Make each `content` pattern end in an extension, for example `content/vocab/*.yaml`.

See [Content files](../content/files.md).

### A file saved on Windows gives strange errors

Some Windows editors put a byte order mark (BOM) at the start of a file. In a JSON file, this gives
`json::syntax` at line 1. In a YAML file, it gives ``missing key `entries` `` and an unknown key
with `\u{feff}` in its name. TOML files are not affected.

1. In your editor, save the file as "UTF-8" without BOM.
2. Run `janky check` again.

### check passes, but plan says config::preset

Some preset values are checked only when janky makes the package, so only `plan` and `build` find
them. Examples are a `maximum_interval` over 36500 days and a wrong number of `fsrs_params`.

1. Correct the value that the message shows.
2. Run `janky plan` again.

See [Presets](../deck/presets.md).

## Generators and plugins

### build says results are missing

A generator has jobs with no result in the cache. `janky build` stops, so that it does not make a
package with empty fields.

```console
$ janky build
enrich::missing

  x 6 enrichment results are missing
  help: run `janky generate`, or build with `--allow-missing`
```

1. Run `janky generate`.
2. Run `janky plan`. If it still shows `Generator jobs:`, run `janky generate` again. (One
   generator can use the result of another, and each run does one step.)
3. Run `janky build`.

With `janky build --allow-missing`, janky builds without the results, and their fields are empty.
After a later `janky generate`, these notes change again. See
[Generating media and data](../plugins/generating.md).

### generate asks for a secret that I do not use

`janky generate` reads the secrets of every generator instance before it starts. It does this also
for an instance with no jobs in this run. So a run for a dictionary plugin can stop with
`secret::missing` for a text-to-speech plugin.

1. Read the message. It names the instance and the environment variable.
2. Set the environment variable, or add the line `NAME=value` to the `.env` file of the project.
3. Run `janky generate` again.

See [Plugin instances and secrets](../plugins/instances.md).

### Another janky is running

`janky generate` and `janky prune` lock the project while they run. A second run gives:

```text
generate::locked

  x another `janky generate` or `janky prune` is running for this project
  help: wait until it ends, then run again
```

1. Look for another terminal that runs `janky generate` or `janky prune` for this project.
2. Wait until it ends.
3. Run your command again.

You do not need to delete `.janky/generate.lock`. The lock ends when janky stops, also after a
crash.

### generate keeps asking a service for the same job

The plugin refused the job (`generate::refused`), for example because the service said no. janky
does not keep a refused job, so the next `janky generate` asks again.

1. Read the message of the refusal.
2. Correct the cause, for example the input text or a setting in `deck.toml`.
3. Run `janky generate` again.

If the service refused your secret (`generate::unauthorised`), janky stops the run. Check the value
of the secret and the quota of your account.

### A plugin hit the time limit and janky exited

A plugin call can run for at most 10 seconds. If a call runs longer, janky stops at once with a line
such as:

```text
error: the plugin call p: filter `spin` ran longer than 10 seconds
```

A generator job has a limit of 600 seconds in total. A job that runs longer fails with
`generate::time`.

1. Find the plugin and the filter, function or instance in the message.
2. If you changed your content shortly before this, check the entry that uses the plugin. A very long
   input can make the plugin slow.
3. If the problem stays, give the message to the author of the plugin.

See [The sandbox and its limits](../authoring/sandbox.md).

### A plugin instance name with a hyphen fails

janky accepts a hyphen in an instance name, for example `[plugins.my-dict]`. But a generator that
uses its own cache or file area then fails. The message contains
`` `my-dict` is not a valid plugin instance name``.

1. Rename the instance with `_` instead of `-`, for example `[plugins.my_dict]`.
2. Change the name in each field template that uses it, for example `enrich(entry.spanish, 'my_dict')`.
3. Run `janky check`.

Use letters, digits and `_` in instance names. The cached results stay valid after the rename. A
plugin that downloads files to its file area downloads them again. See
[Plugin instances and secrets](../plugins/instances.md).

### prune removed a result that I needed again

`janky prune` deletes each cached result that no current job uses. If you change an input for a
short time and prune, the old result is gone. A change back then needs a new `janky generate`, and a
paid service charges again.

1. Run `janky prune` only when your content is final.
2. Run `janky plan` first. Make sure that it shows no `Generator jobs:`.

See [Generating media and data](../plugins/generating.md).

## Files and commands

### deck.lock.tmp already exists

A build or an update stopped before it was complete. janky writes the new lock as `deck.lock.tmp`,
then renames it. While this file exists, `janky build` and `janky update` refuse to run.

```text
build::lock

  x `deck.lock.tmp` already exists
  help: a previous build stopped early; remove the file and build again
```

1. Make sure that no other janky command runs for this project.
2. Delete `deck.lock.tmp`. Do not delete `deck.lock`.
3. Run `janky build` again.
4. Import the new package.

See [Project layout](layout.md).

### janky cannot find deck.toml

janky reads `deck.toml` in the current directory. With `--deck`, it reads the file that you
give.

1. Change to the project directory, or use `--deck path/to/deck.toml`.
2. Give the path of the file, not of the directory. With a directory, janky gives `project::read`
   with `Is a directory`.

See [Command reference](../cli/commands.md).

### generate or prune says that stdin is not a terminal

janky asks before it changes the cache. When you run janky from a script, it cannot ask, and it
gives `generate::confirm`.

1. Read the summary that janky prints.
2. Run the command again with `--yes`.

See [Command reference](../cli/commands.md).
