# 1. Start a project

A project is a directory with a `deck.toml` file. In this step, you make the directory and describe
the decks and their options.

## Make the directories

1. Open a terminal.
2. Make the project directory and its subdirectories:

   ```console
   $ mkdir -p spanish/content/vocab spanish/content/phrases spanish/media/images spanish/notetypes
   ```

3. Go into the project directory:

   ```console
   $ cd spanish
   ```

Run all the janky commands in this tutorial from the `spanish` directory.

## Write deck.toml

`deck.toml` is a TOML file. TOML is a text format for settings. A line `key = value` sets one
value, and a line in brackets, for example `[deck]`, starts a table (a group of keys). Text values
are in double quotes, and lists are in square brackets.

Make the file `deck.toml` with this text:

```toml
[deck]
name = "Spanish"
output = ".janky/build/spanish.apkg"
media_dirs = ["media"]
preset = "spanish"

[deck.children.vocab]
name = "1 Vocabulary"
preset = "beginner"
content = ["content/vocab/*.yaml"]

[deck.children.phrases]
name = "2 Phrases"
preset = "beginner"
content = ["content/phrases/*.toml"]

[presets.spanish]
name = "Spanish"
new_per_day = 10
reviews_per_day = 100

[presets.beginner]
name = "Spanish beginner"
new_per_day = 15
reviews_per_day = 150
learning_steps = ["1m", "10m", "1h"]
desired_retention = 0.9
maximum_interval = "1y"
```

### The root deck

The `[deck]` table is the root deck. Every other deck of the project is inside it.

| Key | Meaning |
|-----|---------|
| `name` | The deck name that Anki shows. |
| `output` | Where `janky build` writes the `.apkg` package. |
| `media_dirs` | The directories where janky looks for images and audio. |
| `preset` | The preset of this deck. `spanish` is the key of the `[presets.spanish]` table. |

### The child decks

Each `[deck.children.<key>]` table is a child deck. The part after `children.` is the deck **key**
(`vocab` and `phrases`). janky uses the key to identify the deck. Anki shows the `name`, so the two
child decks become "Spanish::1 Vocabulary" and "Spanish::2 Phrases".

The `content` key lists the content files of the deck. `*` matches any text, so
`content/vocab/*.yaml` means "every file in `content/vocab/` whose name ends in `.yaml`". The
directories are empty now. You add the files in [step 3](content.md).

### The presets

A preset is a set of Anki deck options. Each `[presets.<key>]` table is one preset, and a deck uses
it with `preset = "<key>"`.

| Key | Meaning |
|-----|---------|
| `name` | The preset name that Anki shows. |
| `new_per_day` | The maximum number of new cards each day. |
| `reviews_per_day` | The maximum number of reviews each day. |
| `learning_steps` | The learning steps. `m` is minutes, `h` is hours. |
| `desired_retention` | The FSRS desired retention. |
| `maximum_interval` | The longest interval between reviews. `1y` is one year. |

[Decks and child decks](../deck/decks.md) and [Presets](../deck/presets.md) explain all the
options.

## Write .gitignore

janky writes its build output and caches into a `.janky/` directory in the project. If you use
[Git](https://git-scm.com) to keep the history of your project, Git must ignore this directory. Git
must also ignore `.env`, a file that can hold secrets (passwords and keys for online services).

Make the file `.gitignore` with this text:

```text
.janky/
.env
```

If you do not use Git, the file does no harm.

> [!IMPORTANT]
> Later in the tutorial, janky also writes a `deck.lock` file. Do not ignore it. Keep `deck.lock`
> with your project, and commit it if you use Git.

## What you have now

```text
spanish/
  .gitignore
  deck.toml
  content/
    phrases/
    vocab/
  media/
    images/
  notetypes/
```

Do not run `janky check` yet. It reports an error until the project has a note type.

Next: [2. Make a note type](notetype.md).
