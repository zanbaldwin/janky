# Command reference

This page describes each janky command, the flags, the exit codes and the output of `janky plan`.
The examples use the Spanish project from the [tutorial](../tutorial/index.md). Some examples are
trimmed: `...` shows where lines are left out.

A command line has this form:

```text
janky [flags] <command> [flags]
```

The general flags (`--deck`, `--quiet`, `--verbose`, `--json`, `--offline` and `--allow-missing`) can go
before or after the command name. `--yes` belongs to `generate`, `prune` and `publish` only, so it
must come after the command name: `janky generate --yes`. The other flags of `publish` also come
after its name, and so do `--port` and `--export` of `preview` and the flags of `show`.

`janky --help` lists the commands and flags. `janky <command> --help` shows the help of one command,
for example `janky generate --help`. `janky --version` prints the version.

## The commands at a glance

| Command | What it does | What it writes |
|---------|--------------|----------------|
| [`check`](#check) | Finds errors in all project files | Nothing |
| [`plan`](#plan) | Shows what the next build changes, and the missing generator jobs | Nothing |
| [`build`](#build) | Writes the package | The `.apkg` file and `deck.lock` |
| [`preview`](#preview) | Shows each card in your browser | Nothing. With `--export`, the export directory |
| [`show`](#show) | Prints the cards of one entry as text, JSON or HTML | Nothing |
| [`schema`](#schema) | Writes schema files for your code editor | `.janky/schema/` |
| [`update`](#update) | Moves remote note types to their newest commit | `deck.lock` (the source records only) |
| [`generate`](#generate) | Runs the generator jobs | `.janky/cache/` |
| [`prune`](#prune) | Deletes cached results that no job uses | Deletes files in `.janky/cache/` |
| [`publish`](#publish) | Puts the project into your collection through your sync server | Your collection on the server, `.janky/publish/` and `deck.lock` |
| [`pull`](#pull) | Downloads missing generated media from your sync server | `.janky/cache/media/` |

## check

`janky check` reads `deck.toml`, the note types, the plugins, the content files and the media. It
renders every entry with the field templates and checks the card templates. It writes no files.

If there are no errors, the last line gives the number of notes:

```console
$ janky check
...
9 notes are valid
```

If there are errors, janky prints each one and leaves out the last line.
[Reading errors and warnings](diagnostics.md) explains the format.

When generator results are missing, `check` gives a warning, not an error:

```console
$ janky check
enrich::missing

  ! 6 enrichment results are missing
  help: run `janky generate`
9 notes are valid
```

Media results and data results have separate warnings: `generate::missing` for media and
`enrich::missing` for data.

A small number of errors appear only when janky makes the package, so `check` cannot find them.
Examples are a preset value that Anki does not accept, and a media file that janky cannot read. Run
`janky plan` to find these errors too.

## plan

`janky plan` does the same checks as `check`. Then it reads `deck.lock`, makes the package in
memory and compares the two. It shows what the next build adds, changes and removes. It writes no
files.

If `check` finds an error, `plan` prints the errors and no plan, and exits with code 1.

If there is no `deck.lock` (a new project), `plan` shows every deck, note and file as added.

```console
$ janky plan
...
Notes: 9 added, 0 changed, 0 removed, 0 moved, 0 unchanged
+ deck Spanish
+ deck Spanish::1 Vocabulary
+ deck Spanish::2 Phrases
+ preset Spanish
+ preset Spanish beginner
+ note type Spanish Word
+ note type Spanish Phrase
+ note hola in Spanish::2 Phrases
...
+ note aguacate in Spanish::1 Vocabulary
+ media casa.png
When you import:
  Tick "Import any deck presets". Anki adds a preset only the first time.
Generator jobs: 6 to run (6 lookups)
+ enrich frequency: casa
...
```

[How to read the plan](#how-to-read-the-plan) explains each part.

## build

`janky build` does the same work as `plan`. Then it writes the `.apkg` package, and then
`deck.lock`.

```console
$ janky build
...
Notes: 0 added, 1 changed, 0 removed, 0 moved, 8 unchanged
~ note gato in Spanish::1 Vocabulary

wrote .janky/build/spanish.apkg: 9 notes, 1 media file
In Anki, turn FSRS on. A package cannot change collection settings.
```

- The package goes to the `output` path in `deck.toml`. Without `output`, it goes to
  `.janky/build/deck.apkg`. The path must be inside the project directory.
- `build` prints the plan before it writes the package.
- The FSRS line appears after each successful build. FSRS is a collection setting in Anki, and a
  package cannot change it. [Importing into Anki](importing.md#turn-on-fsrs) explains it.

### Missing generator results stop the build

If a generator job has not run, `build` stops with an error. It prints no plan and writes nothing.

```console
$ janky build
enrich::missing

  x 6 enrichment results are missing
  help: run `janky generate`, or build with `--allow-missing`
```

Run `janky generate` first. To build without the results, use `janky build --allow-missing`. Then the
warnings stay warnings, the fields that need a missing result are empty, and the missing files are
not in the package.

### A build that stopped early

janky writes `deck.lock` to a temporary file `deck.lock.tmp` first, and then renames it. If a build
stops before the rename (for example, after a crash), the temporary file stays. The next build
refuses to start:

```text
build::lock

  x `deck.lock.tmp` already exists
  help: a previous build stopped early; remove the file and build again
```

To continue, do these steps:

1. Delete `deck.lock.tmp`. Do not delete `deck.lock`.
2. Run `janky build` again.

## preview

`janky preview` starts a small web server that shows each card of the project in your browser. Use
it while you design a note type. You see a change without a build, an import into Anki and a study
session.

```console
$ janky preview
Preview at http://127.0.0.1:5259/ — reload the page to read the files again, press Ctrl+C to stop
```

Open the address in your browser. The page has two parts:

- On the left, a table has one row for each card: the deck, the entry, the source, the note type
  and the card type. The source is the content file and the position of the entry in it, from 0, for
  example `content/vocab/01-basics.yaml #2`. The rows are in the order of the new cards in the
  package (see [The order of new cards](../deck/decks.md#the-order-of-new-cards)). Type in the
  filter field to find a card. Select **Show empty cards** to show the cards with an empty front.
- On the right, the page shows the selected card. Use **Front** and **Back** to turn the card. Use
  **Night mode** to see the night colours, and the platform menu to see the card at the width of a
  phone.

To change the widths of the two parts, drag the line between them. Your browser keeps the width.

To see a change, save the file and reload the page (Ctrl+R or F5). Each reload reads all files
again. The page keeps the selected card and the settings. janky does not watch the files.

Errors and warnings show above the table. If the project does not load, the page keeps the cards of
the last load that worked, and a note above the table says so.

Press Ctrl+C in the terminal to stop the server.

### What the preview shows

- janky renders each card with the template code of Anki, so the HTML is the same as in Anki.
- The card gets the same classes as in Anki, for example `card card1` and `nightMode`. See
  [See your cards in the browser](../notetypes/styling.md#see-your-cards-in-the-browser).
- The JavaScript of the card runs as in the reviewer of Anki, with `onUpdateHook`, `onShownHook`
  and `pycmd`. Code that waits for `DOMContentLoaded` does not run, as in Anki.
- MathJax shows `\(…\)` and `\[…\]`. LaTeX (`[latex]`, `[$]` and `[$$]`) shows as plain text,
  because it needs a LaTeX program.
- A play button plays each sound. Text to speech uses the speech voices of your browser.
- `{{type:Field}}` shows an input. Type your answer and press Enter. The back compares your answer
  with the field, as in Anki.
- A card with an empty front has the label `empty`. Anki does not make this card, for example
  because the `when` condition of the card type does not apply to the entry. The table shows these
  rows only when you select **Show empty cards**.
- A note type that no entry uses gets one row for each card type, with N/A as the entry and the deck.
  Each field shows its name, for example `(Front)`, as in the card layout screen of Anki.

### What the preview does not do

- It writes no files in the project. It does not write `deck.lock`, the package or `.janky/`.
  Only `--export` writes files, and only in the export directory.
- It does not run generators. Missing generated media and data show as in `check`.
- It does not show answer buttons, scheduling or image occlusion. It does not have add-on filters,
  jQuery or the JavaScript API of AnkiDroid.
- It does not play sounds automatically.

### The server

- The server listens only on `127.0.0.1`, so other computers cannot connect to it. It refuses a
  request for another host name.
- To use another port, give `--port`, for example `janky preview --port 8080`. `--port 0` lets your
  system choose a free port. If the port is in use, janky stops with `preview::bind`.
- The JavaScript of a card runs in a sandbox. It cannot read the page or the other cards.
- If a plugin runs longer than its time limit, janky stops, and the server stops too. See
  [The sandbox](../authoring/sandbox.md).

### Export to a web server

`janky preview --export <DIR>` writes the preview as static files. A web server such as Nginx can
then show your cards to other people, without janky. Use it to show or demonstrate a deck.

```console
$ janky preview --export site
wrote site: 12 cards, 1 media file, deck.apkg
```

The export starts no server. `--export` and `--port` cannot be used together.

The export reads the project in the same way as `janky build`. If the project has an error, the
export stops and writes nothing. Missing generated media and data are warnings, as in `plan`. The
page of the export shows no errors or warnings.

The export directory contains:

| Path | Contents |
|------|----------|
| `index.html`, `assets/` | The page. |
| `preview.json` | The rows of the table. |
| `<generation>/cards/` | One HTML file for each side of each card. |
| `<generation>/media/` | The media files of the cards. |
| `deck.apkg` | The package of `janky build`, if `deck.lock` is up to date. |
| `.janky-export` | A marker file. It shows that the directory is an export. |

`<generation>` is a hash of the cards and the media. The same project gives the same name, and a
change to a card or a media file gives a new name. A web server can keep the files below it for a
long time.

All links in the export are relative. You can put the export at the root of a site or below a path,
for example `https://example.com/decks/periodic/`.

#### The package

The export contains `deck.apkg` only if `deck.lock` exists and a build would not change it. The
package is then the same deck as the package of `janky build`, with the same ids. If `deck.lock` is
missing or out of date, the ids of a package would differ from those of the next build. A user who
imports both would get duplicate notes. So the export then has no `deck.apkg`, and janky gives the
warning `preview::export-lock`. To get the package, run `janky build`, then export again. The
export never writes `deck.lock`.

#### The export directory

The directory must not exist, it must be empty, or it must be an earlier export. janky writes
the files into a new directory next to it, then renames that directory to the export directory. A
web server never sees half an export.

- If the directory is an earlier export, janky replaces it and deletes the old files.
- If the directory has other files, janky does not touch it and stops with
  `preview::export-target`. It also refuses a file or a symbolic link at the path.

#### What an export cannot do

- The back of a `{{type:Field}}` card shows the expected answer. It does not compare a typed answer,
  because only the server of `janky preview` can do that. The page shows a note on these cards.
- The page does not read your files again. Export again after a change.

#### Settings for Nginx

The card pages and the media files need the same headers that the server of `janky preview` sends:

- `Content-Security-Policy` keeps the JavaScript of a card in a sandbox, also when somebody opens a
  card page directly.
- `Access-Control-Allow-Origin` lets the sandboxed card load the MathJax fonts and the fonts of your
  note type. Without it, the cards still show, but with other fonts.

This example serves an export from `/srv/site`:

```nginx
server {
    server_name decks.example.com;
    root /srv/site;

    location ~ /\.janky-export$ {
        return 404;
    }
    location ~ "^/[0-9a-f]{32}/cards/" {
        add_header Content-Security-Policy "sandbox allow-scripts";
        add_header X-Content-Type-Options nosniff;
    }
    location ~ "^/[0-9a-f]{32}/media/" {
        add_header Content-Security-Policy "default-src 'none'; style-src 'unsafe-inline'; sandbox";
        add_header Access-Control-Allow-Origin *;
        add_header X-Content-Type-Options nosniff;
    }
    location /assets/ {
        add_header Access-Control-Allow-Origin *;
    }
}
```

Nginx needs the quotes around a regular expression that has `{` in it. For an export below a path,
add the path to each `location`.

> [!WARNING]
> Use a host name that serves only the export. Without the `Content-Security-Policy` header, a card
> page that somebody opens directly runs its JavaScript with the permissions of your site. This
> includes the scripts of remote note types.

> [!NOTE]
> The preview and the export need the web page files that `just setup` builds. A janky that you
> built without them shows a message in place of the page, and `--export` stops with
> `preview::no-ui`. See [Installation](../installation.md).

## show

`janky show` prints the cards of one entry and stops. It is for AI agents and scripts that make or
change a deck: they can read a card without a browser. It renders the cards in the same way as
`janky preview`, and it writes no files.

Give an identifier: `<entry>` for all cards of an entry, or `<entry>/<card>` for one card. The
card is the key of a card type in the note type file.

```console
$ janky show gato/recognition
gato/recognition · Spanish Word · Recognition · content/vocab/01-basics.yaml #2
front: el gato
back: el gato cat ...
```

- Without an identifier, `janky show` lists all identifiers, one on each line, in the order of the
  preview table.
- With `--sample`, the identifier is a note type key, or `<notetype>/<card>`. janky shows the sample
  note of that note type, with the name of each field in place of its value.
- If no entry has the id, janky stops with `show::unknown-entry`. An entry with errors is not shown.
- If the entry has no card type with the key, janky stops with `show::unknown-card` and lists the
  card types.

An entry id can contain `/` if its schema allows any text. janky first looks for an entry with the
whole identifier as its id, and only then splits the identifier at the last `/`.

### The text output

Each card has a header line, then one line for each side:

- The header has the identifier, the note type, the card type and the source of the entry.
- A sound shows as `[sound:<file>]`, and text to speech as `[tts <language>: <text>]`.
- An image shows as its file name.
- The input of `{{type:Field}}` shows as `[type answer]`. `--typed` gives the typed answer that the
  back compares.
- A card with an empty front gets the line `empty: Anki does not make this card`.

### The JSON output

With `--json`, the diagnostics come first, then one line with the cards:

```json
{"cards":[{"id":"gato/recognition","notetype":{"key":"word","name":"Spanish Word"},"entry":{"id":"gato","file":"content/vocab/01-basics.yaml","index":2},"deck":{"key":"vocab","full_name":"Spanish::1 Vocabulary"},"card":{"key":"recognition","name":"Recognition","ord":0},"css":"…","empty":false,"error":null,"front":{"html":"…","text":"…","sounds":[…]},"back":{…},"type_answer":null}]}
```

Each side has the HTML, the text and the sounds. With `--side`, the other side is `null`. Without
an identifier, the line is `{"identifiers":[…]}`.

### The HTML output

`--format html` prints one side of one card as an HTML document. The document needs no server: it
has the CSS and the JavaScript of the preview in it, and it refers to the media files with
`file://` URLs. So you can save it to a file and open it with a browser, also a headless browser:

```console
$ janky show gato/recognition --side back --format html --platform iphone > /tmp/card.html
$ chromium --headless --screenshot=/tmp/card.png --window-size=390,800 file:///tmp/card.html
```

- Give `--side front` or `--side back`, and an identifier for one card. Otherwise janky stops with
  `show::one-side`.
- `--night` and `--platform` set the same classes as the controls of the preview. The browser sets
  the width: use 412 pixels for `android` and 390 pixels for `iphone`.
- MathJax does not run in this document, because the browser does not load its fonts from a
  `file:///` page. The maths shows as TeX, and janky gives the warning `show::mathjax`.

| Flag | What it does |
|------|--------------|
| `--side <front\|back>` | Shows only one side. `--format html` needs it. |
| `--format <text\|html>` | The output without `--json`. The default is `text`. |
| `--typed <text>` | The typed answer, for a back with `{{type:Field}}`. |
| `--night` | The night classes, for `--format html`. |
| `--platform <desktop\|android\|iphone>` | The platform classes, for `--format html`. The default is `desktop`. |
| `--sample` | The identifier names a note type, and janky shows its sample note. |

## schema

`janky schema` writes JSON Schema files into `.janky/schema/`. A JSON Schema file describes which
keys and values a file can have. Many code editors read these files. The editor can then complete
keys and mark errors while you type.

`schema` reads `deck.toml`, the note types and the plugins, but not the content files. It writes
`deck.schema.json`, `notetype.schema.json`, `lock.schema.json` and, if the project loads,
`content.schema.json`. Then it prints the settings to add to your editor:

```console
$ janky schema
wrote .janky/schema/deck.schema.json
wrote .janky/schema/notetype.schema.json
wrote .janky/schema/lock.schema.json
wrote .janky/schema/content.schema.json

Add to .taplo.toml next to deck.toml:

[[rule]]
include = ["deck.toml"]
[rule.schema]
path = ".janky/schema/deck.schema.json"

[[rule]]
include = ["notetypes/word/notetype.toml","notetypes/phrase.toml"]
[rule.schema]
path = ".janky/schema/notetype.schema.json"

[[rule]]
include = ["deck.lock"]
[rule.schema]
path = ".janky/schema/lock.schema.json"

[[rule]]
include = ["content/phrases/*.toml"]
[rule.schema]
path = ".janky/schema/content.schema.json"

Add to .vscode/settings.json for YAML and JSON content:

{
  "yaml.schemas": {
    ".janky/schema/content.schema.json": [
      "content/vocab/*.yaml"
    ]
  },
  "json.schemas": [
    {
      "fileMatch": [],
      "url": "./.janky/schema/content.schema.json"
    }
  ]
}
```

- The `.taplo.toml` part is for Taplo, a TOML tool that many editors use. It covers `deck.toml`, the
  note types, `deck.lock` and the TOML content files.
- The `.vscode/settings.json` part is for Visual Studio Code. It covers the YAML and JSON content
  files.
- The Spanish project has no JSON content files, so its `fileMatch` list is empty.

The content schema comes from your note types. Run `janky schema` again after you change a note
type's schema or add a note type. janky never adds schema lines to your own files.

## update

`janky update` is for remote note types: note types that `deck.toml` loads from a Git repository or
an archive URL. [Adding note types](../deck/notetypes.md) explains them.

`deck.lock` records the exact commit of each remote note type. `check`, `plan` and `build` use that
commit, so a change upstream does not change your deck by surprise. `update` gets the newest
commit for each `tag` or `branch`, and records it in `deck.lock`.

`update` prints one line for each remote note type:

| Line | Meaning |
|------|---------|
| `<key>: unchanged at <commit>` | The newest commit is the commit in `deck.lock`. |
| `<key>: <old commit> -> <new commit>` | `deck.lock` now records the new commit. |
| ``<key>: new, `janky build` records it`` | `deck.lock` has no record yet. The next build adds it. |
| `No remote note types.` | The project has no remote note types. |

```console
$ janky update
remote: ce4bec3f6b9d -> 152067e53c49
```

`update` changes only the source records in `deck.lock`. Run `janky plan` after it to see what the
new commit changes, then run `janky build`. A source with a fixed `rev`, or an archive, keeps its
commit or file.

With `--offline`, `update` is a dry run. It shows what it would change from the repositories that
are already on your computer, and writes nothing. It first prints this line:

```text
Dry run with --offline: these are the changes that `janky update` would make, from the cached repositories. Commits that are not in the cache yet do not show.
```

## generate

`janky generate` runs the generator jobs that are missing from the cache. It is the only command
that can send your content to a service. [Generating media and data](../plugins/generating.md)
explains generators.

`generate` does these steps:

1. It runs the same checks as `check`. If there is an error, it stops.
2. If no job is missing, it prints `All generator results are in the cache.` and stops.
3. It reads the secrets of the generators. If a secret is missing, it stops before it sends
   anything.
4. It prints a summary: the jobs, and for each plugin instance the number of jobs, the network
   hosts and the secrets.
5. It asks `Run N jobs? [y/N]`. Only `y` or `yes` (in any case) continues. Any other answer prints
   `Nothing was generated.`
6. It runs the jobs and prints one line for each finished job.
7. It prints a total.

```console
$ janky generate --yes
Generator jobs: 6 to run (6 lookups)
+ enrich frequency: casa
+ enrich frequency: perro
+ enrich frequency: gato
+ enrich frequency: pan
+ enrich frequency: manzana
+ enrich frequency: aguacate
frequency: 6 jobs
  network: none
[1/6] frequency: casa: wrote f25d6251e5acc09b93bfcafc6cc2ef00.json
...
[6/6] frequency: aguacate: wrote 70fb3d4384dc7e2bfb6c2507c0332fbf.json
Generated 6 results: 0 refused, 0 failed, 0 not started
```

- `network: none` means that the plugin instance cannot use the network. Otherwise, the line names
  the hosts that it can contact.
- A secret line shows where janky looks for the secret, for example
  ``secret `api_key`: environment variable `ELEVENLABS_API_KEY`, then `.env` ``. It never shows the
  value.
- janky needs the secrets of every generator in the project, also of a generator that has no
  missing jobs. Make sure that all secrets are available before you run `generate`.

Each progress line has the form `[i/N] <instance>: <input>: <result>`. The result is one of these:

| Result | Meaning |
|--------|---------|
| `wrote <file>` | The result is in the cache. janky does not ask for it again. |
| `refused` | The plugin or the service said no. janky stores nothing, so the job runs again next time. |
| `unauthorised` | The service refused the secret. janky starts no more jobs. |
| `failed` | The plugin had an error. The other jobs continue. |
| `not started` | The run stopped before this job started. |

The total line is `Generated N results: R refused, F failed, S not started`. If a plugin reports a
cost, a line `Reported cost for <instance>: <amount>` follows. The amount is the number that the
service gives, in the unit of that service.

If a job is refused, unauthorised or failed, `generate` exits with code 1.

janky stores each result as soon as its job ends. If you stop the run with Ctrl+C, the finished
results stay in the cache.

### Confirm without a terminal

If janky cannot ask you (for example, in a script), it stops with an error:

```text
generate::confirm

  x janky asks before it changes the cache, and stdin is not a terminal
  help: run again with `--yes`
```

`--yes` answers the question for you. Use it only after you have read the summary.

### One run at a time

Only one `generate` or `prune` can run in a project at a time. janky keeps a lock in
`.janky/generate.lock`. A second run stops with `generate::locked`: "another `janky generate` or
`janky prune` is running for this project". The lock is released when janky stops, also after a
crash.

## prune

`janky prune` deletes cached results that no current job uses. It looks only in
`.janky/cache/media/` and `.janky/cache/data/`. It never deletes the plugin files in
`.janky/cache/plugins/`, or files with names that janky did not make.

```console
$ janky prune --yes
1 cached files are not used by any job (87 bytes).
...
Removed 1 files (87 bytes).
```

- If there are errors in the project, `prune` does nothing.
- If any generator job is missing, `prune` refuses to start (`prune::pending`). A missing job can hide
  other jobs, and `prune` could then delete a result that you still need. Run `janky generate`
  first.
- `prune` asks `Remove them? [y/N]` before it deletes anything. Use `--yes` when there is no
  terminal. Any answer other than `y` or `yes` prints `Nothing was removed.`
- If nothing is unused, it prints `Nothing to prune.`
- A media result is two files (the media file and its `.json` file), so it counts as two.

> [!WARNING]
> A deleted result for a paid service costs money to make again. Run `prune` only when your content
> is final.

## publish

`janky publish` puts the project into your Anki collection through your own sync server. It needs
a `[publish]` table in `deck.toml`. [Publishing through a sync server](publishing.md) explains it
in full.

`publish` does these steps:

1. It does the same work as `plan`. Missing generator results stop it, like `build`.
2. It logs in to the server, and syncs its copy of your collection in `.janky/publish/`.
3. It compares the project with the copy, and prints the plan.
4. With `--dry-run`, it stops here.
5. If the plan has a schema change, it stops, unless you give `--allow-full-sync`.
6. It asks `Publish these changes to the sync server? [y/N]`. If nothing changes, it does not ask.
7. It changes the copy, syncs it to the server, and syncs the media.
8. It writes `deck.lock`.

```console
$ janky publish
...
Notes: 0 added, 1 changed, 0 kept, 0 removed, 8 unchanged
~ note gato
Publish these changes to the sync server? [y/N] y
Published: 0 notes added, 1 changed, 0 deleted, 0 cards moved.
```

| Flag | What it does |
|------|--------------|
| `--yes` | Answers the question. janky needs it when there is no terminal. |
| `--dry-run` | Shows the plan. janky changes nothing on the server. |
| `--allow-full-sync` | Allows a schema change, which replaces the collection on the server. Sync your other devices first. |
| `--delete-removed` | Deletes the notes of entries that you removed, with their cards. |

Only one `publish` can run in a project at a time (`publish::locked`).

## pull

`janky pull` downloads the generated media files that are missing from the cache, from your sync
server. Use it on a new computer or in a fresh clone, so that you do not pay for the same audio
again. It needs a `[publish]` table in `deck.toml`.

```console
$ janky pull
Pulled 42 media files from the server.
```

- `pull` takes a file only when `deck.lock` records it with the same content.
- It never replaces a file in the cache.
- Data results are not on the server. `janky generate` makes them again.
- It takes the lock of `generate` and `prune`.

[Get generated media on a new computer](publishing.md#get-generated-media-on-a-new-computer-janky-pull)
explains it in full.

## Flags

| Flag | Default | What it does |
|------|---------|--------------|
| `--deck <deck.toml>` | `deck.toml` | The path to the `deck.toml` file (not to the directory). The project directory is the directory of that file. |
| `--quiet` | off | Hides the messages. Errors and warnings still appear. |
| `--verbose` | off | Shows each info in full. Without it, the infos of one code show as one line with a count. |
| `--json` | off | Prints errors, warnings and infos as JSON Lines on standard output. |
| `--offline` | off | Uses only the remote note types that are already on your computer. |
| `--allow-missing` | off | Lets `build`, `publish` and `prune` continue when generator results are missing. |
| `--yes` | off | Answers the confirmation question of `generate`, `prune` and `publish`. |
| `--dry-run` | off | `publish` only: shows the plan and changes nothing on the server. |
| `--allow-full-sync` | off | `publish` only: allows a schema change. |
| `--delete-removed` | off | `publish` only: deletes the notes of removed entries. |
| `--port <port>` | `5259` | `preview` only: the port of the server on `127.0.0.1`. `0` lets your system choose a free port. |
| `--export <DIR>` | – | `preview` only: writes the preview as static files to `DIR`, and starts no server. See [Export to a web server](#export-to-a-web-server). |
| `--side`, `--format`, `--typed`, `--night`, `--platform`, `--sample` | – | `show` only. See [show](#show). |

### How each flag applies to each command

Some flags do less than their names suggest. This table shows where each flag has an effect.

| Flag | `check` | `plan` | `build` | `schema` | `update` | `generate` | `prune` | `publish` | `pull` | `preview` | `show` |
|------|---------|--------|---------|----------|----------|------------|---------|-----------|--------|-----------|--------|
| `--quiet` | Hides `N notes are valid` | No effect (the plan is not a message) | Hides the plan, the `wrote` line and the FSRS line | Hides the `wrote` lines and the editor settings | Hides the messages, for example `No remote note types.` | Hides the progress lines and the total, but not the summary | Hides the messages, for example `Nothing to prune.` | Hides the plan and the messages, but not the question | Hides the messages | No effect: the address always shows | No effect: the output is not a message |
| `--verbose` | Full infos | Full infos | Full infos | Full infos | Full infos | Full infos | Full infos | Full infos | Full infos | No effect: the page shows each info in full | Full infos |
| `--json` | JSON diagnostics | JSON diagnostics and one plan line. No plan text | JSON diagnostics and one plan line. The plan text still goes to standard error | JSON diagnostics | JSON diagnostics | JSON diagnostics | JSON diagnostics | JSON diagnostics and one `publish` line. The plan text still goes to standard error | JSON diagnostics | JSON diagnostics of the start only. The page shows the other diagnostics | JSON diagnostics and one `cards` line. `--format` has no effect |
| `--offline` | Remote note types from the cache | Remote note types from the cache | Remote note types from the cache | Ignored: `schema` can still fetch remote note types | Dry run | Remote note types from the cache. Generators still use the network | Remote note types from the cache | Remote note types from the cache. The sync still uses the network | Remote note types from the cache. The download still uses the network | Remote note types from the cache | Remote note types from the cache |
| `--allow-missing` | No effect | No effect | Missing results are warnings | No effect | No effect | No effect | Runs while jobs are missing | Missing results are warnings | No effect | No effect | No effect |
| `--yes` | – | – | – | – | – | Skips the question | Skips the question | Skips the question | – | – | – |

A dash means that the flag does not apply to that command.

> [!IMPORTANT]
> `--offline` does not stop generators. `janky --offline generate` still sends requests to the
> services of your plugins. To stop a service, do not run `generate`.

`--allow-missing` has no effect on `plan`. `plan` always shows missing results as warnings and lists
the jobs.

#### `--deck`

Use `--deck` to run janky from a different directory. Give the path of the file:

```console
$ janky --deck spanish/deck.toml check
...
9 notes are valid
```

If you give the directory instead, janky stops with `project::read`: "cannot read `spanish`: Is a
directory".

#### `--json` and the plan line

With `--json`, `plan` and `build` print one extra line after the diagnostics. It holds the plan as
one JSON object:

```json
{"plan":{"changes":[{"kind":"note-changed","id":"gato","deck":"vocab"}],"unchanged_notes":8,"advice":[],"pending_jobs":[]}}
```

| Key | Meaning |
|-----|---------|
| `changes` | One object for each change. `kind` is the kind of change (see the table in [Every plan line](#every-plan-line)). |
| `unchanged_notes` | The number of unchanged notes. |
| `advice` | The `When you import:` lines. |
| `pending_jobs` | One object for each missing job: `instance`, `input`, `units` and `unit`. `units` and `unit` are `null` when the plugin gives no units. |

In the JSON plan, decks have their key path in `deck.toml`, not their names. For example, `vocab` is
a child deck, and `""` is the root deck. Note types are given by their keys (for example `word`).

## Exit codes and output streams

| Exit code | Meaning |
|-----------|---------|
| `0` | No errors. There can be warnings. |
| `1` | There was at least one error. |
| `2` | The command line is wrong, for example an unknown command or flag. |

A terminal has two output streams: standard output and standard error. Normally you see both. If you
send the output to a file or another program, they separate.

- Errors, warnings and messages go to standard error.
- The plan text of `janky plan` and its list of generator jobs go to standard output. So
  `janky plan > plan.txt` writes only the plan to the file.
- `janky build` and `janky publish` print the plan on standard error, as a message.
- With `--json`, the JSON lines go to standard output. The messages still go to standard error,
  unless you also use `--quiet`.

janky never prints the value of a secret. In its place, it prints `<secret:name>`.

## How to read the plan

`janky plan` and `janky build` print the plan in this order:

1. The summary line. It is always there.
2. `No changes.`, if nothing changes. The plan stops here.
3. One line for each change: first decks, then presets, note types, notes and media.
4. `When you import:` and the advice lines, if any advice applies.
5. `Generator jobs:` and one line for each missing job, if any job is missing.

### The summary line

```text
Notes: 0 added, 1 changed, 0 removed, 0 moved, 8 unchanged
```

The summary counts notes only. `moved` counts the notes that move to a different deck and also the
notes that change note type.

### The symbols

| Symbol | Meaning |
|--------|---------|
| `+` | Added |
| `~` | Changed, renamed, moved, or changed to a different note type |
| `-` | Removed |

### Every plan line

Many lines end with a short instruction after the colon. Do that work in Anki.
[Importing into Anki](importing.md#what-happens-in-anki-when-you-change-something) explains the
reason for each one.

| Line | JSON `kind` | What it means |
|------|-------------|---------------|
| `+ deck <name>` | `deck-added` | A new deck. |
| `- deck <name>: the deck stays in Anki` | `deck-removed` | You removed a deck from `deck.toml`. Anki keeps it. |
| `~ deck <old> is renamed to <new>: rename it in Anki before you import` | `deck-renamed` | You changed the `name` of a deck. Rename it in Anki first. |
| `+ preset <name>` | `preset-added` | A new preset. |
| `- preset <name>: the preset stays in Anki` | `preset-removed` | You removed a preset. Anki keeps it. |
| `~ preset <name>: a package cannot change a preset that Anki has. Change it in Anki` | `preset-changed` | You changed a preset. Make the same change in Anki. |
| `+ note type <name>` | `notetype-added` | A new note type. |
| `- note type <name>: the note type stays in Anki` | `notetype-removed` | You removed a note type. Anki keeps it. |
| `~ note type <old> is renamed to <new>` | `notetype-renamed` | The note type has a new name. |
| `~ note type <name>: fields added: ...; fields removed: ...; ... Needs a full sync` | `schema-changed` | Fields or card types were added, removed or moved. See below. |
| `~ note type <name>: card templates or CSS` | `notetype-changed` | Only the card templates or the styles changed. |
| `+ note <id> in <deck>` | `note-added` | A new note. |
| `~ note <id> in <deck>` | `note-changed` | The fields or tags of a note changed. |
| `- note <id> in <deck>: the note stays in Anki. Delete it there` | `note-removed` | You removed an entry. Delete the note in Anki if you do not want it. |
| `~ note <id> moves from <deck A> to <deck B>: its cards stay in <deck A>. Move them in Anki (Browse, Change Deck)` | `note-moved` | The entry is now in a different deck. Move its cards in Anki. |
| `~ note <id> changes note type from <A> to <B>: janky adds a new note. Delete the old note in Anki` | `note-retyped` | The entry has a different note type. Anki gets a new note. |
| `+ media <file>` | `media-added` | A new media file. |
| `~ media <file>` | `media-changed` | A media file changed. |
| `- media <file>: the file stays in Anki. Tools, Check Media removes unused files` | `media-removed` | No note uses the file now. |

A note that changes note type shows only as `changes note type`, not also as moved. A deck move shows
only when the note type stays the same.

The `schema-changed` line lists each part of the change, for example
`~ note type Spanish Word: fields added: plural. Needs a full sync`. The parts are `fields added`,
`fields removed`, `fields moved`, `card types added`, `card types removed` and `card types moved`.
If something was removed or moved, the line ends with
`. Removed or moved fields and card types do not reach Anki: change them there`.

When a deck is renamed in the same build, the note lines use the new deck name. In Anki, the deck
keeps its old name until you rename it.

### The advice lines

The `When you import:` part tells you which import options to set in Anki. Each line appears only
when its reason applies.

| Advice | When it appears |
|--------|-----------------|
| `Tick "Import any deck presets". Anki adds a preset only the first time.` | The plan adds a preset. |
| `Turn on "Merge note types". The import needs a full sync afterwards.` | The fields or card types of a note type changed. |
| `Set "Update notes" to "Always", so that the new fields get their values.` | A note type has new fields. |

There is no advice line for renamed decks, moved notes, removed notes or changed presets. The
change line itself tells you what to do.

### The generator jobs

```text
Generator jobs: 6 to run (6 lookups)
+ enrich frequency: casa
```

- The first line gives the number of missing jobs. The part in brackets adds up the units that the
  plugins report, for example characters for a text-to-speech service. It is not a price.
- Each job line has the form `+ media <instance>: <input>` or `+ enrich <instance>: <input>`.
  `media` jobs make files. `enrich` jobs make data.
- janky shortens an input to 60 characters and ends it with `…`.
