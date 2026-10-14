# Generating media and data

A generator makes results for your notes: an audio file for each word, or a set of values from a
word list. This page explains how to call a generator from a field template, how to run it, and how
janky keeps the results.

The examples use the `frequency` data generator from the [tutorial](../tutorial/generator.md). It
looks up each Spanish word in a local word list, and gives its rank and a band ("common" or
"rare"). For a media generator, the examples use an instance called `voice` of the ElevenLabs
plugin (see [The bundled plugin](bundled.md)).

## Call a generator from a field template

A field template calls a generator with the instance name from `deck.toml`.

For a media generator, use `media` with two arguments, or the `media` filter:

```jinja
{{ entry.spanish | nfc | media('voice') | sound }}
{{ media(entry.spanish, 'voice') | sound }}
{% if media(entry.spanish, 'voice').exists %}...{% endif %}
```

The result is a media value. It prints as the file name, and the `sound` filter turns it into
`[sound:<file name>]` for Anki. A media value also has `.file` (the file name), `.kind` and
`.exists` (true when the file is in the cache).

For a data generator, use `enrich`. It gives a set of named values, and you pick one with a dot:

```jinja
{% set f = enrich(entry.spanish | nfc, 'frequency') %}{% if f.rank %}#{{ f.rank }} ({{ f.band }}){% endif %}
```

The input can be text or a map (a set of named values), for example
`{'word': entry.spanish, 'gloss': entry.english}`. In a map, janky leaves out a key whose
value is missing.

The `nfc` filter makes sure that the same text always has the same form (Unicode NFC). Use it on
text input, so the same word always gives the same job.

### A template call only records a job

A template call never sends a request. It only records a **job**: one request that the generator
must run. `check`, `plan` and `build` never use the network.

- One input for one instance is one job, even if many fields call it with the same input.
- An empty input makes no job. Empty text, text with only spaces, or a map with no values makes no
  request. So a misspelt key name in a template cannot become a paid empty request.

## The workflow

1. Run `janky plan`. At the end, it lists the pending jobs:

   ```console
   $ janky plan
   ...
   Generator jobs: 6 to run (6 lookups)
   + enrich frequency: casa
   + enrich frequency: perro
   + enrich frequency: gato
   + enrich frequency: pan
   + enrich frequency: manzana
   + enrich frequency: aguacate
   ```

   The count in brackets comes from the plugin. For ElevenLabs, it is the number of characters. It
   is not an amount of money.

2. Run `janky generate`. It shows a summary of the jobs, the hosts and the secrets, and asks you
   to confirm. Type `y` to start.

   ```console
   $ janky generate
   Generator jobs: 6 to run (6 lookups)
   ...
   frequency: 6 jobs
     network: none
   ```

3. Run `janky build`. It puts the results into the package.

### The summary before a run

The summary has one block for each instance with pending jobs. It shows the hosts that the instance
can connect to (`network: none` when it has no `[http]` table) and where janky looks for each
secret. It never shows the value of a secret. For example:

```text
frequency: 4 jobs
  network: none
voice: 5 jobs
  network: api.elevenlabs.io
  secret `api_key`: environment variable `ELEVENLABS_API_KEY`, then `.env`
```

The question is `Run N jobs? [y/N]`. Only `y` or `yes` starts the run. Any other answer prints
`Nothing was generated.` and changes nothing.

If janky does not run in a terminal (for example, in a script), it cannot ask. Add `--yes` to
start without the question:

```console
$ janky generate
...
generate::confirm

  x janky asks before it changes the cache, and stdin is not a terminal
  help: run again with `--yes`
```

### During and after the run

janky prints one line for each finished job:

```console
$ janky generate --yes
...
[1/6] frequency: casa: wrote f25d6251e5acc09b93bfcafc6cc2ef00.json
[2/6] frequency: perro: wrote 70d27e3d3306c86f65cd945776eb7092.json
...
[6/6] frequency: aguacate: wrote 70fb3d4384dc7e2bfb6c2507c0332fbf.json
Generated 6 results: 0 refused, 0 failed, 0 not started
```

- janky saves each result when its job ends. If you stop the run (Ctrl-C), you keep the finished
  results.
- If the plugin reports a cost, janky prints `Reported cost for <instance>: <n>` at the end.
- janky prints lines from the plugin itself after the run, each with the prefix `plugin: `.
- The exit code is 1 if a job was refused or failed.
- Only one `janky generate` or `janky prune` can run for a project at a time.

When all results are in the cache, `janky generate` does nothing:

```console
$ janky generate --yes
All generator results are in the cache.
```

> [!WARNING]
> The global `--offline` flag does not stop `janky generate` from using the network. It only stops
> janky from downloading remote note types.

## Build without the results

`janky build` refuses to write a package while results are missing:

```console
$ janky build
enrich::missing

  x 6 enrichment results are missing
  help: run `janky generate`, or build with `--allow-missing`
```

With `--allow-missing`, janky writes the package anyway. The missing results then render as
follows:

- `enrich(...)` gives an empty set of values, so every field that reads it is empty.
- A media value from a plugin that knows its file type (such as ElevenLabs) renders its file name,
  for example `[sound:<name>.ogg]`. The file itself is not in the package yet.
- A media value from a plugin that does not know its file type renders nothing.

After `janky generate`, the next build fills these fields, so `plan` shows the notes as changed.
If you import a package that you built with `--allow-missing`, Anki gets a second update of those
notes later. To avoid this, run `janky generate` before your first real build.

## Generators that use the result of another generator

A field template can pass the result of one generator to another generator. For example, a
dictionary gives an example sentence, and a text-to-speech plugin reads it aloud:

```jinja
{{ enrich(entry.spanish, 'dict').example | media('voice') | sound }}
```

janky runs this in two steps. Before the first run, the example sentence is empty, so there is no
audio job.

1. Run `janky generate`. It makes the dictionary data.
2. Run `janky plan`. It now shows the audio jobs.
3. Run `janky generate` again. It makes the audio.

## What can happen to a job

| Result | Cause | What janky does |
|---|---|---|
| Stored | The plugin gave a result. | janky caches it, and never asks the service again. |
| Stored with a warning | A data plugin found nothing, for example the word is not in the list. | janky caches the empty result and the warning. It does not ask again. |
| Refused | The service said no. | An error. janky caches nothing, so it runs the job again on the next `janky generate`. |
| Unauthorised | The service rejected the secret (HTTP 401 or 403). | An error. janky starts no more jobs. Jobs that already started finish. |
| Failed | The plugin had an error, or went over a limit. | An error. The run goes on with the other jobs. |
| Not started | The run stopped first. | Nothing. The job stays pending. |

### Warnings are cached

A data plugin can give a warning instead of a value. janky shows the warning on every `check`,
`plan` and `build`, at the field template that calls `enrich`:

```console
$ janky check
enrich::warning

  ! frequency: aguacate: not in the frequency list
    ,-[notetypes/word/notetype.toml:45:9]
 44 | name = "Frequency"
 45 | value = "{% set f = enrich(entry.spanish | nfc, 'frequency') %}{% if f.rank %}#{{ f.rank }} ({{ f.band }}){% endif %}"
...
  help: first seen in entry `aguacate` in `content/vocab/02-food.yaml`
9 notes are valid
```

A warning never makes a command fail. Because janky caches the warning, `janky generate` does not
try the job again. To try again, change the input, or delete the cached result (see
[Run one job again](#run-one-job-again)).

### Unauthorised stops the run

When a service rejects your secret, janky stops the run. The summary counts the job under "refused":

```console
$ janky generate --yes
...
[1/3] say: uno: unauthorised
generate::unauthorised

  x say: the server refused the secret `key` for `uno`: HTTP 401: {"detail":{"message":"invalid api key"}}
  help: check the secret's value and the account's quota; janky stopped the run
Generated 0 results: 1 refused, 0 failed, 2 not started
```

Check the value of the secret, and the quota of your account at the service.

## The cache

janky keeps the results in `.janky/cache/` in your project:

| Path | What it holds |
|---|---|
| `.janky/cache/media/` | Generated files, each with a small `.json` file next to it. |
| `.janky/cache/data/` | Generated data, one `.json` file for each job. |
| `.janky/cache/plugins/<instance>/` | Files and a database that a plugin keeps for itself, for example a downloaded dictionary. |

Each file name is a code of 32 characters that janky makes from the job. A media file keeps this
name in Anki too.

### Back up the cache

> [!WARNING]
> Paid results exist only in `.janky/cache/`. If you delete `.janky/`, or clone the project on a
> new computer, `janky generate` pays the service again for every result.

Back up `.janky/cache/media/` and `.janky/cache/data/`, or commit them to Git on purpose. Do not
delete `.janky/` to "clean up". You can delete `.janky/build/` and `.janky/cache/plugins/` safely,
because janky makes them again.

janky does not write a `.gitignore` file. If you use Git, add the lines yourself. A common choice is
to ignore `.janky/` and `.env`, and to keep a separate backup of the cache.

If two projects share one directory, they also share one cache.

### What makes a job new

Each job has a key. The plugin chooses what goes into the key, usually some of the instance
variables. If the plugin does not choose, the key is all the instance variables. When the key or the
input changes, the job is new, and janky runs it again.

- Change the input (for example, the Spanish word): one new job for that note.
- Change a variable in the key (for example, the ElevenLabs voice): a new job for every note. With a
  paid service, this costs the full amount again.
- Change the plugin script: no new jobs.
- Change a file that the plugin reads (for example, `data/frequency.json`): no new jobs. The old
  results stay in the cache.

> [!TIP]
> The old results stay in the cache until you run `janky prune`. If you change a variable back
> before you prune, janky finds the old results again, and you pay nothing.

For example, in the tutorial project, a change of `common_below` from 1000 to 500 makes all six
lookups pending again:

```console
$ janky plan
...
Generator jobs: 6 to run (6 lookups)
+ enrich frequency: casa
...
```

### Run one job again

There is no command to run one job again. Delete its cached result instead:

1. Find the file name of the result. The progress line of `janky generate` shows it
   (`wrote f25d6251e5acc09b93bfcafc6cc2ef00.json`). For audio, the field in Anki also shows it
   (`[sound:<name>.ogg]`).
2. For data, delete `.janky/cache/data/<name>.json`.
3. For media, delete both `.janky/cache/media/<name>.<ext>` and `.janky/cache/media/<name>.json`.
4. Run `janky generate`.

You do not need to do this for a refused job. janky caches nothing for it, so it runs again
automatically.

## Remove unused results with prune

`janky prune` deletes cached results that no current job uses, for example the result of an entry
that you removed.

```console
$ janky prune --yes
1 cached files are not used by any job (87 bytes).
...
Removed 1 files (87 bytes).
```

The rules:

- `prune` does nothing if the project has errors.
- `prune` refuses while any job is pending. A pending job can hide other jobs (for example, behind
  `{% if media(...).exists %}` or a chain of generators), and their results would look unused.
- `prune` deletes files only in `.janky/cache/media/` and `.janky/cache/data/`. It never touches
  `.janky/cache/plugins/`.
- `prune` asks before it deletes. Without a terminal, add `--yes`.

```console
$ janky prune
...
prune::pending

  x 1 generated results are missing, so janky cannot see every job of the project
  help: run `janky generate` first, or prune with `--allow-missing`. Pending: frequency `casa`
```

`--allow-missing` makes `prune` run while jobs are pending. Use it with care.

> [!WARNING]
> Prune only when `janky plan` shows no pending jobs and your content is final. If you change an
> input for a short time and prune, janky deletes the old result. If you change the input back, the
> job runs again. With a paid service, you pay again. Two projects in one directory also prune each
> other's results.
