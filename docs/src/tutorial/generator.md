# 6. Add a generator

A generator is a plugin that makes media or data for your notes. In this step, you add a generator
that looks up how common each word is, in a word list on your computer. The word cards then show a
rank, for example "#150 (common)".

A real generator often uses an online service that costs money (for example, text-to-speech audio).
This one reads a local file, so it is free. The commands and the safety steps are the same.

## Add the word list

1. Make the directory `data/` in the project:

   ```console
   $ mkdir data
   ```

2. Make the file `data/frequency.json` with this text:

   ```json
   {
     "casa": 150,
     "perro": 820,
     "gato": 1400,
     "pan": 640,
     "manzana": 2100
   }
   ```

This is a JSON file (another text format). Each line gives a word and its rank. "aguacate" is not
in the list on purpose.

## Add the generator script

1. Make the directory `plugins/` in the project:

   ```console
   $ mkdir plugins
   ```

2. Make the file `plugins/frequency.lua` with this text:

   ```lua
   local function plan(job, vars)
     return { key = { list = vars.list, common_below = vars.common_below }, units = 1, unit = "lookups" }
   end

   local function generate(job, vars)
     local word = job.input
     local ranks = janky.json.decode(janky.fs.read("lists/" .. vars.list))
     local rank = ranks[word]
     if rank == nil then
       return { value = {}, warning = "not in the frequency list" }
     end
     local band = "rare"
     if rank < vars.common_below then
       band = "common"
     end
     return { value = { rank = rank, band = band } }
   end

   return {
     id = "frequency",
     api = 1,
     generates = "data",
     variables = {
       list = { type = "string", required = true },
       common_below = { type = "int", required = true },
     },
     plan = plan,
     generate = generate,
   }
   ```

In short, the script does this:

- `generates = "data"` makes it a data generator.
- `variables` lists the settings that `deck.toml` must give it.
- `generate` runs once for each word. It reads the word list, finds the rank, and returns the rank
  and a band ("common" or "rare"). If the word is not in the list, it returns a warning.
- `plan` tells janky which settings change the result, and how to count the work (one lookup for
  each word).

[Writing a generator](../authoring/generators.md) explains how to write a script like this.

## Add the plugin instance to deck.toml

A plugin instance is a `[plugins.<name>]` table in `deck.toml`. It names the script and gives it
its settings. Add these lines to the end of `deck.toml`:

```toml
[plugins.frequency]
path = "plugins/frequency.lua"
[plugins.frequency.variables]
list = "frequency.json"
common_below = 1000
[plugins.frequency.filesystem]
read = { lists = "data" }
```

| Table or key | Meaning |
|--------------|---------|
| `[plugins.frequency]` | The instance name is `frequency`. Templates use this name. |
| `path` | The script. |
| `variables` | The settings of the script: the list file, and the rank below which a word is "common". |
| `filesystem` | The files that the script can read. `lists = "data"` lets the script read the `data/` directory under the name `lists`. That is why the script reads `lists/frequency.json`. |

A plugin can only read files, use the network or use secrets if its instance allows it. This
instance allows only the `data/` directory.

> [!TIP]
> Use only letters, digits and `_` in a plugin instance name.

[Plugin instances and secrets](../plugins/instances.md) explains the instance tables.

## Add the Frequency field

1. Open `notetypes/word/notetype.toml`.
2. Add this table after the `[fields.notes]` table, before `[cards.recognition]`:

   ```toml
   [fields.frequency]
   name = "Frequency"
   value = "{% set f = enrich(entry.spanish | nfc, 'frequency') %}{% if f.rank %}#{{ f.rank }} ({{ f.band }}){% endif %}"
   ```

3. Open `notetypes/word/cards/recognition.back.html`.
4. Add the Frequency line after the Gender line. The file is now:

   ```html
   {{FrontSide}}<hr id="answer">
   <div class="meaning">{{English}}</div>
   {{#Gender}}<div class="gender">{{Gender}}</div>{{/Gender}}
   {{#Frequency}}<div class="frequency">{{Frequency}}</div>{{/Frequency}}
   {{#Examples}}<div class="examples tap-reveal">{{Examples}}</div>{{/Examples}}
   {{#Notes}}<div class="notes">{{Notes}}</div>{{/Notes}}
   <script src="reveal.js"></script>
   ```

`enrich(entry.spanish | nfc, 'frequency')` asks the `frequency` instance for the data of this word.
`{% set f = ... %}` keeps the result in `f`. Before the generator has run, the result is empty, so
`{% if f.rank %}` prints nothing until then.

## See the jobs

1. Run `janky check`:

   ```console
   $ janky check
   enrich::missing

     ⚠ 6 enrichment results are missing
     help: run `janky generate`
   9 notes are valid
   ```

   This is a warning (`⚠`), not an error. The field templates ask for six results that do not exist
   yet.

2. Run `janky plan`:

   ```console
   $ janky plan
   enrich::missing

     ⚠ 6 enrichment results are missing
     help: run `janky generate`
   Notes: 0 added, 6 changed, 0 removed, 0 moved, 3 unchanged
   ~ note type Spanish Word: fields added: frequency. Needs a full sync
   ~ note casa in Spanish::1 Vocabulary
   ~ note perro in Spanish::1 Vocabulary
   ~ note gato in Spanish::1 Vocabulary
   ~ note pan in Spanish::1 Vocabulary
   ~ note manzana in Spanish::1 Vocabulary
   ~ note aguacate in Spanish::1 Vocabulary
   When you import:
     Turn on "Merge note types". The import needs a full sync afterwards.
     Set "Update notes" to "Always", so that the new fields get their values.
   Generator jobs: 6 to run (6 lookups)
   + enrich frequency: casa
   + enrich frequency: perro
   + enrich frequency: gato
   + enrich frequency: pan
   + enrich frequency: manzana
   + enrich frequency: aguacate
   ```

The plan has two new parts:

- The note type has a new field, so the import needs other options. You use them
  [below](#import-the-new-version).
- `Generator jobs` lists the six jobs: one lookup for each word. `plan` only lists them. It does not
  run them.

## Try to build

Run `janky build`:

```console
$ janky build
enrich::missing

  × 6 enrichment results are missing
  help: run `janky generate`, or build with `--allow-missing`
```

`build` refuses, because the package would have empty Frequency fields. Do not use
`--allow-missing` here. Run the generator first, so that your first import has the data.

## Run the generator

1. Run `janky generate`. It shows a summary and asks you to confirm:

   ```console
   $ janky generate
   Generator jobs: 6 to run (6 lookups)
   + enrich frequency: casa
   + enrich frequency: perro
   + enrich frequency: gato
   + enrich frequency: pan
   + enrich frequency: manzana
   + enrich frequency: aguacate
   frequency: 6 jobs
     network: none
   Run 6 jobs? [y/N]
   ```

   The summary shows the jobs of each instance and the services that it can contact. `network:
   none` means that this instance uses no network. For a paid service, read this summary with care.

2. Type `y` and press Enter:

   ```console
   Run 6 jobs? [y/N] y
   [1/6] frequency: casa: wrote f25d6251e5acc09b93bfcafc6cc2ef00.json
   [2/6] frequency: perro: wrote 70d27e3d3306c86f65cd945776eb7092.json
   [3/6] frequency: gato: wrote 686fcb4d93d4d620433b324eebae4ba6.json
   [4/6] frequency: pan: wrote 427bbccba034ea08ef42d62a6681b818.json
   [5/6] frequency: manzana: wrote d0385910d38edb05f0d86ec3f4a32ea3.json
   [6/6] frequency: aguacate: wrote 70fb3d4384dc7e2bfb6c2507c0332fbf.json
   Generated 6 results: 0 refused, 0 failed, 0 not started
   ```

janky keeps each result in `.janky/cache/data/`. If you run `janky generate` again, it runs no jobs:

```console
$ janky generate
...
All generator results are in the cache.
```

> [!IMPORTANT]
> Back up `.janky/cache/`. It holds every generator result, and Git ignores it (your `.gitignore`
> has `.janky/`). For a paid service, a lost cache means that you pay again for every result. Do
> not delete the `.janky/` directory.

## Read the not-found warning

Run `janky build`:

```console
$ janky build
enrich::warning

  ⚠ frequency: aguacate: not in the frequency list
    ╭─[notetypes/word/notetype.toml:45:9]
 44 │ name = "Frequency"
 45 │ value = "{% set f = enrich(entry.spanish | nfc, 'frequency') %}{% if f.rank %}#{{ f.rank }} ({{ f.band }}){% endif %}"
    ·         ───────────────────────────────────────────────────────┬──────────────────────────────────────────────────────
    ·                                                                ╰── `enrich` is called here
 46 │ 
    ╰────
  help: first seen in entry `aguacate` in `content/vocab/02-food.yaml`
Notes: 0 added, 6 changed, 0 removed, 0 moved, 3 unchanged
~ note type Spanish Word: fields added: frequency. Needs a full sync
~ note casa in Spanish::1 Vocabulary
...
~ note aguacate in Spanish::1 Vocabulary
When you import:
  Turn on "Merge note types". The import needs a full sync afterwards.
  Set "Update notes" to "Always", so that the new fields get their values.

wrote .janky/build/spanish.apkg: 9 notes, 1 media file
In Anki, turn FSRS on. A package cannot change collection settings.
```

The build works. The warning comes from the generator: "aguacate" is not in the word list. janky
puts the instance name and the word in front of the plugin's text (`frequency: aguacate:`). The
`help` line names the entry. The Frequency field of `aguacate` stays empty, so its card shows no
rank.

janky keeps the not-found result in the cache too, so it does not look up the word again. The
warning shows again on each command: `check`, `plan`, `build`, `generate` and `prune`.

> [!NOTE]
> The cache uses the `list` and `common_below` settings to identify a result. If you edit
> `data/frequency.json` but keep the same settings, janky uses the old cached results.

## Import the new version

The note type has a new field, so this import needs two changed options. `plan` told you which.

1. In Anki, select **File → Import**.
2. Select `.janky/build/spanish.apkg`.
3. Turn on **Merge note types**.
4. Set **Update notes** to **Always**, so that the notes get the new field values.
5. Select **Import**.
6. If you sync your collection with AnkiWeb, do a full sync after this import.
7. For the next imports, set **Update notes** back to **If newer**.

The back of the "casa" card now shows "#150 (common)".

[Importing into Anki](../cli/importing.md) explains these options.

## Remove unused results with prune

When you remove an entry or change a generator setting, some cached results are no longer used.
`janky prune` removes them. Run it now:

```console
$ janky prune
...
Nothing to prune.
```

Every result is in use, so there is nothing to remove. If you remove the `manzana` entry, its
result becomes unused. `prune` then shows the unused files and asks before it removes them:

```console
$ janky prune
1 cached files are not used by any job (87 bytes).
Remove them? [y/N] y
...
Removed 1 files (87 bytes).
```

> [!WARNING]
> A pruned result is gone. If you put the entry back, `janky generate` must make it again, and a
> paid service charges you again. Prune only when you are sure.

[Generating media and data](../plugins/generating.md) explains `generate`, the cache and `prune`.

## What you have now

```text
spanish/
  .gitignore
  deck.lock
  deck.toml
  .janky/
    build/spanish.apkg
    cache/data/        (6 results: back this up)
  content/  (4 files)
  data/frequency.json
  media/    (1 file)
  notetypes/
  plugins/frequency.lua
```

You have a complete project: two note types, a note type plugin, nine notes, an image, and a data
generator. Keep `deck.lock` with the project, and back up `.janky/cache/`.

To learn more, read [Using the command line](../cli/index.md), or go to the chapter of the part
that you want to change: [Defining a deck](../deck/index.md),
[Creating note types](../notetypes/index.md), [Creating content](../content/index.md) or
[Using plugins](../plugins/index.md).
