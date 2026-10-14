# Tutorial: your first deck

In this tutorial, you make a small Spanish deck from an empty directory. You import it into Anki,
change it, and then add a plugin that looks up data for each word.

The finished deck has:

- a root deck "Spanish" with two child decks, "1 Vocabulary" and "2 Phrases"
- two presets (deck options)
- a "Spanish Word" note type with two card types, an image, examples and a tap-to-reveal script
- a "Spanish Phrase" note type with one card type
- nine notes
- a frequency rank on each word card, from a generator plugin

Everything runs on your computer. The tutorial uses no accounts, no secrets and no network.

## Before you start

- Install janky (see [Installation](../installation.md)). The command `janky --version` must work.
- Install Anki on the same computer.
- Have a text editor that can edit plain text files (for example VS Code).
- Read [How janky works](../how-it-works.md). The tutorial uses its terms.

You do not need to know TOML, YAML, HTML or Lua. Each step explains what you need when it first
appears.

## The steps

1. [Start a project](project.md): make the directory and the `deck.toml` file.
2. [Make a note type](notetype.md): the fields and the card designs.
3. [Add content](content.md): the words, the phrases and an image.
4. [Build and import](build.md): check, plan and build the package, then import it into Anki.
5. [Make changes](changes.md): fix an error, edit a note and remove a note.
6. [Add a generator](generator.md): add frequency data with a plugin.

Each step shows the full text of every file. Type or copy each file exactly. Each step ends with a
list of what you have and a link to the next step.
