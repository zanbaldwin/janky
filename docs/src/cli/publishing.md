# Publishing through a sync server

`janky publish` puts your project into your Anki collection through your own Anki sync server.
You do not import a package. janky syncs like one more device, next to your desktop and your
phone, and your devices get the changes at their next sync.

`publish` does more than an import. It can do these things, which a package cannot do:

- Update a preset that Anki already has.
- Rename a deck in place, with its cards.
- Move the cards of an entry that you moved to a different deck.
- Delete the notes of entries that you removed, when you ask for it.

> [!WARNING]
> `publish` changes your real collection. Read the plan before you answer the question. Only a
> self-hosted sync server is tested. AnkiWeb probably works, but its terms do not allow automated
> clients.

## What you need

- An Anki sync server that you run yourself, for example Anki's own `anki-sync-server`. Its
  address must use `https://`. Plain `http://` is allowed only on your own computer (`localhost`,
  `127.0.0.1` or `[::1]`).
- An account on that server. On a self-hosted server, the account comes from the environment
  variable `SYNC_USER1=<user name>:<password>` of the server.
- A clock that is correct. The server refuses a device whose clock is more than 5 minutes off.

## Set up `[publish]`

Add a `[publish]` table to `deck.toml`:

```toml
[publish]
endpoint = "https://anki.example.org/"
username = "zan"
password = { env = "ANKI_SYNC_PASSWORD" }
```

- `endpoint` is the address of the server. If it has a path, the path must end with `/`. Do not
  put the user name or the password in the address.
- `username` is the user name of the account.
- `password` tells janky where the password is. It works like a plugin secret: an environment
  variable, a `.env` file, a file of its own, or a combination.
  [Plugin instances and secrets](../plugins/instances.md#secrets) explains each source.

Put the password in the `.env` file next to `deck.toml`, and keep `.env` out of Git:

```text
ANKI_SYNC_PASSWORD=your password
```

janky never prints the password. In its place, it prints `<secret:password>`.

## Your first publish

1. On each of your devices, sync with the server. Then the server has all your reviews.
2. Run `janky publish --dry-run`. janky downloads your collection and shows the plan. It changes
   nothing on the server.
3. Read every line of the plan.
4. Run `janky publish`. janky shows the plan again and asks `Publish these changes to the sync
   server? [y/N]`.
5. Type `y` and press Enter.
6. On each of your other devices, sync.

The first run downloads your whole collection and all its media, so it can take some time. Later
runs download only the changes.

```console
$ janky publish
...
Notes: 9 added, 0 changed, 0 kept, 0 removed, 0 unchanged
+ note type Spanish Word
+ note type Spanish Phrase
+ preset Spanish
+ preset Spanish beginner
+ deck Spanish
+ deck Spanish::1 Vocabulary
+ deck Spanish::2 Phrases
+ note aguacate
...
Publish these changes to the sync server? [y/N] y
Published: 9 notes added, 0 changed, 0 deleted, 0 cards moved.
```

### An empty account

If the account on the server has no cards yet, janky uploads its whole copy of the collection.
Then it prints this message:

```text
The account on the server had no cards, so janky uploaded the whole collection. When another device syncs, Anki asks whether to upload or to download. Download replaces the collection on that device with the server's. Upload replaces the server's collection with the one on the device, and the next `janky publish` adds the project again.
```

To start from the project, choose **Download** on each device.

## Read the publish plan

The plan of `publish` compares your project with your collection, not with `deck.lock`. So it also
shows the changes that you made in Anki and that janky sets back.

The summary line comes first:

```text
Notes: 0 added, 1 changed, 1 kept, 1 removed, 7 unchanged
```

`kept` counts the notes that are different in your project, but that you edited in Anki after the
last change in your files. janky keeps your edit in Anki.

Then each change has one line. Notes show with their entry ids.

| Line | What it means |
|------|---------------|
| `+ note type <name>` | A new note type. |
| `~ note type <name>: card templates, CSS or names` | The note type updates. |
| ``~ note type <name>: fields or card types change. Needs a full sync (`--allow-full-sync`)`` | A schema change. See [Schema changes and full syncs](#schema-changes-and-full-syncs). |
| `- note type <copy>: a copy of <name>. Its notes move to <name>, and the copy is deleted` | An earlier import without **Merge note types** made a copy. janky merges it back. This needs a full sync. |
| `+ preset <name>` | A new preset. |
| `~ preset <name>: sets <keys>` | The preset changes. The line names the keys of `deck.toml` that change, and `name` for a new name. |
| `+ deck <name>` | A new deck. |
| `~ deck <old> is renamed to <new>` | The deck gets its new name, and keeps its cards. |
| `~ deck <name>: sets the preset back` | You gave the deck a different preset in Anki. janky sets the preset of `deck.toml` again. |
| `+ note <id>` | A new note. |
| `~ note <id>` | The fields or tags of the note change. The note keeps its cards and review history. |
| `= note <id>: edited in Anki, so it stays as it is` | You edited the note in Anki after the last change in your files. |
| ``- note <id>: stays in Anki. `--delete-removed` deletes it`` | You removed the entry. The note stays. |
| `- note <id>: deleted, with its cards` | You removed the entry, and used `--delete-removed`. |
| `= note <GUID>: made in Anki with a janky note type. janky never deletes it` | You added this note in Anki. janky does not know it. |
| `~ N cards move to the decks of their notes` | You moved entries to a different deck. |

If nothing changes, the plan is the summary line and `No changes.` Then janky does not ask.

With `--json`, janky also prints the plan as one JSON line, `{"publish": {...}}`, on standard
output. It has one list for each kind of line, and the numbers `notes_unchanged` and
`cards_moved`.

## What publish does with each change

| You change this in your files | What happens in your collection |
|-------------------------------|---------------------------------|
| The text, fields or tags of an entry | The note updates. Its review history stays. |
| You remove an entry | The note stays. With `--delete-removed`, janky deletes it. |
| The `name` of a deck | The deck gets the new name, and keeps its cards. |
| You move an entry to a different deck | Its cards move to the new deck. |
| A preset | The preset updates. The options that `deck.toml` does not set keep their values in Anki. |
| Card templates or CSS | The note type updates. |
| You add, remove or move fields or card types | A full sync. See below. |
| The `id` or the note type of an entry | A new note, like with an import. The old note is a removed entry. |

### Edits that you make in Anki

- A note that you edit in Anki stays as you edited it. The next change to its entry in your files
  replaces your edit.
- A note type that you change in Anki stays as it is, until the note type changes in your files.
- A card that you move to a different deck in Anki stays there. janky moves cards only when you
  move their entry in your files.
- A preset or a deck preset that you change in Anki goes back to the value in `deck.toml`. The
  plan shows it.
- A deck that you rename in Anki gets its name from `deck.toml` again. The plan shows it.

## Remove notes with `--delete-removed`

When you remove an entry, its note stays in Anki. To delete these notes, run:

```console
$ janky publish --delete-removed
```

The plan shows each note that janky deletes. janky deletes only notes that it published or that
`deck.lock` knows. It never deletes a note that you added in Anki. Anki keeps the review log of a
deleted note, but the note and its cards go on every device.

## Schema changes and full syncs

A schema change is a change to the fields or card types of a note type: you add, remove or move
one. Anki can then only do a full sync: one device replaces the whole collection of the other. If
the server replaces its collection with janky's, the reviews that the server does not have yet are
lost.

So `publish` stops when the plan has a schema change:

```text
publish::full-sync

  x the changes need a full sync, which replaces the collection on the server
  help: sync your other devices first, then run again with `--allow-full-sync`. Reviews that the server does not have yet are lost
```

Do these steps:

1. On each of your devices, sync with the server.
2. Do not review on any device until step 4 is complete.
3. Run `janky publish --allow-full-sync`.
4. On each of your other devices, sync. When Anki asks, choose **Download**. Do not choose
   **Upload**.

## Get generated media on a new computer: `janky pull`

The cache of generated results (`.janky/cache/`) is not in Git. On a new computer, or in a fresh
clone, the generated media files are missing. `janky generate` can make them again, but a paid
service then charges you again.

`publish` uploads the media files to the server. `janky pull` downloads them back into the cache:

```console
$ janky pull
Pulled 42 media files from the server.
```

- `pull` downloads only the files of media jobs that are missing from the cache.
- It takes a file only when `deck.lock` records it, and only when the content is the same as that
  record. A file that `deck.lock` does not record stays on the server, and `janky generate` makes
  it.
- It never replaces a file that is already in the cache.
- Data results (from `enrich`) are not on the server. `pull` counts them, and `janky generate`
  makes them again.
- `pull` takes the same lock as `generate` and `prune`, so only one of them runs at a time.

On a new computer, do these steps:

1. Clone the project.
2. Put the password in `.env`.
3. Run `janky pull`.
4. Run `janky plan`. If jobs are still missing, run `janky generate`.

## The local copy

janky keeps its copy of your collection in `.janky/publish/`:

| File | What it holds |
|------|---------------|
| `collection.anki2`, `collection.media/`, `collection.mdb` | The copy of your collection and its media. |
| `state.json` | The record of the last publish: the server, the user name, the Anki id of each deck, and the entry id and deck of each published note. |
| `lock` | The lock. Only one `publish` runs in a project at a time. |
| `applying` | A marker while janky changes the copy. |

The copy is disposable. If a run stops while janky changes the copy, the marker stays. The next
run then deletes the copy and downloads your collection again, so a half-made change never reaches
the server. janky also downloads the collection again when you change `endpoint` or `username`.

Do not delete `state.json`. Without it, janky finds decks by their names only, it cannot move the
cards of moved entries, and `--delete-removed` deletes only the notes that `deck.lock` knows.

## deck.lock

`publish` writes `deck.lock` like `build`, but it writes no package. Commit `deck.lock` after each
publish. If another command changed `deck.lock` while `publish` ran, janky leaves the file as it
is and gives the error `publish::lock`. Your collection is up to date. Run `janky publish` again to
update `deck.lock`.
