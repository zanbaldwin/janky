# Introduction

> This project is vibecoded slop because I needed a tool to solve a problem I
> had in the style of the tools/workflows I am used to as a software engineer,
> and would not have made it if I had to write it myself.

janky is a command-line tool that builds [Anki](https://apps.ankiweb.net) decks from plain text
files. You write your cards in text files, and janky turns them into an `.apkg` file. Anki imports
that file like any shared deck.

In Anki itself, a deck lives inside a database, and you edit it one card at a time through windows
and buttons. With janky, the deck lives in files that you own. You can:

- edit many cards at once in a text editor.
- keep the history of every change (for example, with [Git](https://git-scm.com)).
- reuse one card design for hundreds of notes.
- get an error that names the file, line and column when you make a typo.
- see a list of every change before you build.
- see your cards in your browser while you design them, without an import into Anki.
- update cards that you already study, without losing your review history.

janky can also run small add-on scripts, called plugins. A plugin can make card content for you, for
example audio from a text-to-speech service, or data from a dictionary.

> [!NOTE]
> janky is pre-release software (version 0.1.0). Commands, error codes and the plugin API can still
> change. There are no ready-made downloads yet, so you build janky from its source code (see
> [Installation](installation.md)).

## Who this guide is for

This guide is for people who use Anki and are comfortable with a terminal and a text editor. You do
not need to be a programmer. The guide explains the file formats (TOML, YAML and JSON) and the
template language when they first appear.

Only the last part, [Creating plugins](authoring/index.md), expects you to write code (in the Lua
language).

## How to read this guide

1. Read [How janky works](how-it-works.md). It explains the main ideas in a few minutes.
2. Follow the [tutorial](tutorial/index.md). You build a small Spanish deck, import it into Anki and
   change it.
3. Use the guide chapters when you need the details of one subject:
   - [Using the command line](cli/index.md): the commands, errors and the Anki import.
   - [Defining a deck](deck/index.md): the `deck.toml` file.
   - [Creating note types](notetypes/index.md): fields, card designs and styles.
   - [Creating content](content/index.md): the files that hold your notes.
   - [Using plugins](plugins/index.md): audio, dictionary data and other add-ons.
   - [Creating plugins](authoring/index.md): write your own add-ons.
4. Use the [Reference](reference/layout.md) chapters to look up a file, an error code or a term.

## The rule that matters most

janky keeps the link between your files and the notes in Anki in a file called `deck.lock`. Keep
this file with your project, and never delete it. If you delete it, the next import makes a second
copy of every note, with no review history. [Changing content safely](content/changing.md) explains
why.
