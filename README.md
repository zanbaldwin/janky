# 🃏 Janky

> This project is vibecoded slop because I needed a tool to solve a problem I
> had in the style of the tools/workflows I am used to as a software engineer,
> and would not have made it if I had to write it myself.

`janky` is a command-line tool that builds [Anki](https://apps.ankiweb.net)
decks from plain files in a Git repository. You write your cards (YAML, TOML or
JSON) and note types (TOML with HTML and CSS), and `janky` turns the lot into an
`.apkg` file that Anki imports like any other shared deck.

- Decks as code
- Edits update your existing cards
- Generated audio and dictionary data via Lua plugins
- Shared note types from a Git repository or archive

## Why
My first question when trying out Anki for the first time was "what the fuck?"
My second question was "how the fuck does anyone deal with this?"

I dislike having databases as the source of truth. Git-back source code or GTFO.

### Why is it called `janky`?
It's a pun. If you choose to use my janky-ass Anki tool yourself, then you only
have yourself to blame.

## What it looks like
See the examples at [`janky-periodic`](https://github.com/zanbaldwin/janky-periodic)
and [`janky-http`](https://github.com/zanbaldwin/janky-http).

## Quick Start

1. There are no releases.
2. There are no pre-built binaries.
3. You have to build against whatever version of Anki you are running on your server.

```shell
just setup
cargo build --release
# Install `target/release/janky` onto your path.
```

## Contributing
`janky` is a young, vibecoded, solo-maintained project.
Contributions are genuinely welcome, but no support will be offered.

## Licence
[AGPL-3.0-or-later](https://www.gnu.org/licenses/agpl-3.0.html), because `janky`
links Anki's `rslib`, which uses the same licence.
