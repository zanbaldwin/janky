# The everyday workflow

This page shows the order in which you run the janky commands. The examples use the Spanish project
from the [tutorial](../tutorial/index.md).

## The edit, check, plan, build loop

Each time that you change your deck, do these steps:

1. Edit your files (content files, note types or `deck.toml`).
2. Run `janky check` to find errors.
3. If `check` shows errors, correct them and run `janky check` again.
4. Run `janky plan` to see what the next build changes.
5. Read the plan. If it asks for work in Anki before the import, do that work now.
6. Run `janky build` to write the package and update `deck.lock`.
7. In Anki, import the package with the options that the plan gives.
8. If you use Git, commit your changes, including `deck.lock`.

`check` and `plan` never write files, so you can run them as often as you like. `build` is the only
command in this loop that writes the package and `deck.lock`.

### Example: you add an example sentence

You add the example `El gato es negro.` to the entry `gato` in `content/vocab/01-basics.yaml`.
`janky plan` shows one changed note (the warning above the plan is trimmed):

```console
$ janky plan
...
Notes: 0 added, 1 changed, 0 removed, 0 moved, 8 unchanged
~ note gato in Spanish::1 Vocabulary
```

`janky build` prints the same plan, then writes the package:

```console
$ janky build
...
Notes: 0 added, 1 changed, 0 removed, 0 moved, 8 unchanged
~ note gato in Spanish::1 Vocabulary

wrote .janky/build/spanish.apkg: 9 notes, 1 media file
In Anki, turn FSRS on. A package cannot change collection settings.
```

After the build, `janky plan` shows `No changes.` until you edit a file again.

The plan has no `When you import:` lines here, so the default import options are correct.
[Importing into Anki](importing.md) explains the options. [Command reference](commands.md#plan)
explains each line of the plan.

## The generator loop

A generator is a plugin that makes media or data, for example audio from a text-to-speech service.
janky runs generators only in `janky generate`. The other commands only list the jobs that are
missing. [Generating media and data](../plugins/generating.md) explains generators in full.

When your project uses a generator, the loop has extra steps:

1. Edit your files.
2. Run `janky plan`. Below the plan, it lists the generator jobs that must run.
3. Run `janky generate`. Read the summary of jobs, services and secrets.
4. If the summary is correct, answer `y`.
5. Run `janky plan` again. If it lists new jobs, run `janky generate` again.
6. Run `janky build`.

Step 5 is necessary when one generator uses the result of another (for example, audio of an example
sentence that a dictionary plugin gave). janky runs one round of jobs each time.

This is the job list from the first `plan` of the tutorial project:

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

`janky build` stops with an error while jobs are missing. `janky build --allow-missing` writes the
package anyway, with empty fields where a result is missing.

> [!TIP]
> Run `janky generate` before your first real build and import. If you import a package that you
> built with `--allow-missing`, the next build changes those notes again, when the results arrive.

### Remove unused results

When you remove or change entries, some results in the cache are no longer used. `janky prune`
deletes them. It asks you to confirm first.

> [!WARNING]
> `prune` deletes results that no current job uses. If you change an entry back later, janky must
> make that result again, and a paid service charges you again. Run `prune` only when `plan` shows
> no pending jobs and your content is final.

## Using Git with janky

Git is a version control tool: it records the history of the files in a directory, and lets you go
back to an earlier version. You do not need Git to use janky, but it is a good way to keep your
project safe. A Git project has a `.gitignore` file that lists the files that Git must not record.

janky does not write a `.gitignore` file for you. Make one in the project directory, next to
`deck.toml`:

```text
.janky/
.env
```

### What to commit

| Path | Commit it? | Why |
|------|------------|-----|
| `deck.toml`, note types, content files, media, plugins | Yes | These are your source files. |
| `deck.lock` | Yes | It links your entries to the notes in Anki. Without it, the next import makes a duplicate of every note. |
| `.janky/` | No | janky writes it: the package, the schemas and the caches. |
| `.env` | Never | It can hold secrets, for example the API key of a paid service. |

[Plugin instances and secrets](../plugins/instances.md) explains `.env` and the other places for
secrets.

### Back up the generated results

The results of generators are only in `.janky/cache/`. Git does not record them, because `.janky/`
is in `.gitignore`. If you delete `.janky/`, or you clone the project on a different computer,
`janky generate` must make every result again. For a paid service, you pay again.

Back up these two directories, for example with your usual backup tool:

- `.janky/cache/media/`: generated audio and images.
- `.janky/cache/data/`: generated data, for example dictionary results.

You do not need to back up `.janky/build/` or `.janky/cache/plugins/`. janky can make them again.

If you publish through a sync server, the server also keeps the generated media. `janky pull`
gets them back. [Publishing through a sync server](publishing.md#get-generated-media-on-a-new-computer-janky-pull)
explains it.

> [!WARNING]
> Do not delete the `.janky/` directory to "clean" a project. It holds results that you paid for.

If two `deck.toml` files are in one directory, the two projects share one `.janky/cache/`. Then
`janky prune` in one project can delete the results of the other. Give each project its own
directory.
