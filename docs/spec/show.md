# Spec: `janky show`

Status: draft, not implemented.

## Purpose

`janky show <identifier>` renders one entry or one card, prints the result, and stops. It is for AI agents and
scripts that make or change a deck. An agent can change a note type or an entry, run `janky show`, and read the
result without a browser and without `janky preview`.

The output is text, JSON or HTML. janky makes no images. If an agent needs an image, it saves the HTML to a
temporary file and opens it with a headless browser through a `file:///` URL, for example:

```console
$ janky show li/recognition --side back --format html > /tmp/card.html
$ chromium --headless --screenshot=/tmp/card.png --window-size=412,800 file:///tmp/card.html
```

This also works with a browser from Flatpak.

## Scope

In scope:

- The cards of one entry, or one card of one entry.
- The sample note of a note type (see [Samples](#samples)).
- A list of all identifiers.
- Text, JSON and HTML output.
- The same rendering as `janky preview`: rslib renders the templates, and the play tags and the `[[type:…]]` tags
  get the same treatment.

Out of scope:

- Images. The browser of the agent makes them from the HTML.
- MathJax in the HTML output (see [Limits](#limits)).
- More than one card side in one HTML document.

## Command line

```text
janky show [<identifier>] [--side <front|back>] [--format <text|html>] [--typed <text>]
           [--night] [--platform <desktop|android|iphone>] [--sample]
```

| Flag | Default | Meaning |
| --- | --- | --- |
| `--side` | both sides | Only the front, or only the back. `--format html` needs it. |
| `--format` | `text` | `text` or `html`. The global `--json` gives JSON (see [JSON output](#json-output)), and then `--format` has no effect. |
| `--typed` | empty | The typed answer, for a back with `{{type:Field}}`. |
| `--night` | off | The night classes, as in the preview. |
| `--platform` | `desktop` | The platform classes, as in the preview. |
| `--sample` | off | The identifier names a note type, not an entry (see [Samples](#samples)). |

The global flags `--deck`, `--offline`, `--quiet` and `--json` work as for `check`. Without an identifier,
`janky show` lists the identifiers (see [The list of identifiers](#the-list-of-identifiers)).

## Identifiers

An identifier is `<entry>` or `<entry>/<card>`:

- `<entry>` is the `id` of an entry. It names all cards of the entry, in the order of the card types in the note
  type file.
- `<card>` is the key of a card type in the note type of the entry, for example `recognition`. It names one card.

An entry id can contain `/` if the schema of its note type allows any text. Card keys never contain `/`. So janky
reads an identifier in this order:

1. If the whole identifier is an entry id, it names that entry.
2. Otherwise janky splits it at the last `/`. The first part is the entry id, and the second part is the card key.

Errors:

- No entry has the id: `show::unknown-entry`. The help says that an entry with errors is not shown, and that
  `janky show` lists the identifiers.
- The entry exists, but its note type has no card type with the key: `show::unknown-card`. The help lists the
  card keys of that note type, for example "the card types of `element` are `recognition` and `production`".

### Samples

With `--sample`, the identifier is `<notetype>` or `<notetype>/<card>`, with the key of a note type in
`deck.toml`. janky renders the sample note of that note type, in the same way as the N/A rows of the preview. Each
field shows its name, for example `(Front)`. This works for every note type, also one that entries use.

Errors: `show::unknown-notetype`, with the note type keys in the help, and `show::unknown-card`, with the card keys
of the note type.

### The list of identifiers

`janky show` without an identifier prints one identifier for each card, one for each line, in the order of the
preview table:

```console
$ janky show
li/recognition
li/production
water/recognition
...
```

`janky show --sample` prints `<notetype>/<card>` for each card type of each note type.

## Pipeline

`janky show` runs the pipeline of `janky preview` once:

1. `cli::check`, `lock::read`, and `package::package_with` without `hash_media`, on the lock file in memory.
2. janky finds the note of the identifier, or the note type of the sample.
3. `janky_anki::render` renders only that note: the package keeps all note types and decks, but only one note.

The code of step 1 moves from `preview.rs` to a shared function, so that both commands use the same code.

The read-only rule of the preview applies. `janky show` writes no files, and generators never run. Missing
generated media and data are warnings, as in `check`. The output goes to stdout, and the diagnostics go to stderr.

If `cli::check` gives errors, `janky show` still shows the entry if the entry has no errors. The exit status is 1
if there is any error, as for every janky command.

## Text output

The text output has a header line for each card, then one line for each side:

```text
li/recognition · Element · Recognition · content/periods/early.yaml #0
front: Li
back: Li Lithium Soft enough to cut: Li floats on oil. [sound:2edd12ed13025a468de79acd727380dd.ogg] Li powers phone batteries 🔋
```

- janky converts the HTML of each side with rslib's `html_to_text_line`, with `preserve_media_filenames` set, so
  an image shows as its file name. A new janky-anki function wraps it.
- Before the conversion, each play tag becomes `[sound:<name>]` or `[tts <language>: <text>]`.
- If the front has a `[[type:…]]` input, the front line ends with `[type answer]`. On the back, the comparison
  with `--typed` becomes text.
- Each run of white space becomes one space, so each side is on one line.
- A card with an empty front gets the line `empty: Anki does not make this card`.
- A card that did not render gets the line `error: <message>`.
- A sample has `sample` in place of the entry and the file in the header.
- An empty line separates two cards.

All text goes through `visible()`, so control characters are escaped and secrets are redacted.

## JSON output

With the global `--json`, the diagnostics come first as JSON Lines, then one line with the cards. This is the same
form as the plan line of `janky plan --json`:

```json
{"cards":[{
  "id": "li/recognition",
  "notetype": { "key": "element", "name": "Element" },
  "entry": { "id": "li", "file": "content/periods/early.yaml", "index": 0 },
  "deck": { "key": "elements", "full_name": "Periodic Table::0 Elements" },
  "card": { "key": "recognition", "name": "Recognition", "ord": 0 },
  "css": ".card { font-family: sans-serif; } …",
  "empty": false,
  "error": null,
  "front": { "html": "Li", "text": "Li", "sounds": [] },
  "back": { "html": "…", "text": "…", "sounds": [{ "kind": "file", "name": "2edd….ogg" }] },
  "type_answer": null
}]}
```

- `entry` and `deck` are `null` for a sample.
- With `--side`, the other side is `null`.
- `html` is the card HTML without the CSS, with the play buttons and the type answer input as in the preview. The
  back has the comparison with `--typed`.
- `type_answer` is `{ "expected": …, "combining": … }` if the front has `[[type:…]]` for a field with a value.
- Without an identifier, the last line is `{"identifiers":[…]}`.
- All JSON goes through `json_text()`.

## HTML output

`--format html` prints one card side as one HTML document that needs no server. It needs `--side`, and an
identifier that names one card. Otherwise janky stops with `show::one-side`.

```html
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <style>/* the CSS of the `card` entry of the UI build */</style>
</head>
<body class="card card1 isLin">
  <div id="qa"></div>
  <script type="application/json" id="janky-card">{"html": …, "media": {"diamond.png": "file:///…/media/images/diamond.png"}, …}</script>
  <script type="module">/* the JS of the `card` entry of the UI build */</script>
</body>
</html>
```

- The card data is the same as in the preview (each `<` escaped as `<`), with two more keys:
  - `media` maps each media file of the render to its absolute `file://` URL. `url::Url::from_file_path` makes the
    URLs.
  - `mathjax` is `false`.
- The CSS and the JS of the `card` entry are in the document, so the document loads no files from janky. Chrome
  runs an inline module from a `file:///` page. It does not load a module file from one.
- The HTML output needs the UI build. A janky without it stops with `show::no-ui`. The text and JSON output work
  without the UI build.

### Media in the HTML output

Card HTML names media files by their Anki names, for example `<img src="diamond.png">`. The project has the files in
several media directories and in the cache, so one `<base href>` cannot find them all. The shim changes the names
before the browser loads them:

1. The shim parses the card HTML into a `<template>` element. The content of a template loads nothing.
2. In the template, it replaces each `src` and `poster` attribute whose value is a key of `media`, also after URL
   decoding, with the `file://` URL.
3. In each `<style>` element and each `style` attribute, it replaces each `url(<name>)` whose name is a key of
   `media` in the same way.
4. It moves the content into `#qa`, and replaces the `<script>` elements as before.

A play button plays its file from the `file://` URL. Without `media` in the card data (in the preview), the shim
works as before.

### Build checks

`crates/cli/build.rs` makes sure that the inline module is safe. A build with the UI fails if one of these is
false:

- The `card` entry is one JS file: its manifest entry has no `imports`.
- The JS and the CSS of the `card` entry do not contain `</script` or `</style`, in any case.

## Diagnostics

| Code | Severity | Meaning |
| --- | --- | --- |
| `show::unknown-entry` | E | No entry has the id. |
| `show::unknown-card` | E | The note type of the entry, or the sample note type, has no card type with the key. The help lists the keys. |
| `show::unknown-notetype` | E | `--sample` names a note type that `deck.toml` does not have. The help lists the keys. |
| `show::one-side` | E | `--format html` without `--side`, or with an identifier for more than one card. The help gives an example identifier for one card. |
| `show::no-ui` | E | `--format html` in a janky without the UI build. |
| `show::mathjax` | W | `--format html` for a side with `\(` or `\[`. The maths shows as TeX source. |
| `show::render` | E | Anki rejects a card template, as `preview::render`. |

All codes go in `docs/src/reference/codes.md`.

## Limits

- The HTML output does not run MathJax. MathJax loads its fonts as files, and Chrome does not load them for a
  `file:///` page without `--allow-file-access-from-files`. The text and JSON output show the TeX source. A later
  version can add the `tex-svg-full` component, which needs no font files.
- In the HTML output, media that only the note type CSS names, for example a font, is not in the media map. This
  is the same in the preview.
- The browser of the agent sets the width. To see a phone layout, use `--platform android` and a window width of
  412 pixels, or `--platform iphone` and 390 pixels.

## Code

- `crates/cli/src/show.rs`: the command, the identifiers, the list, and the three outputs.
- `crates/cli/src/preview.rs`: the shared pipeline function, and the card document with the server mode and the file
  mode.
- `crates/anki/src/render.rs` or a new module: `html_to_text(html) -> String` around rslib's
  `html_to_text_line`.
- `crates/cli/ui/src/card/`: the `media` and `mathjax` keys of the card data.
- `crates/cli/build.rs`: the build checks.

## Documentation

- `docs/src/cli/commands.md`: a `show` section, a row in the table of commands, and the flags.
- `docs/src/reference/codes.md`: the `show` codes.
- A short section about agents and scripts in `docs/src/cli/workflow.md`, with the screenshot example.

## Tests

janky-cli (`crates/cli/tests/show.rs`, with the library functions and the binary):

- `show_lists_the_identifiers_in_the_order_of_the_preview`.
- `show_lists_the_sample_identifiers`.
- `show_prints_the_text_of_both_sides_of_each_card`.
- `show_prints_one_side_of_one_card`.
- `show_writes_sound_tags_and_the_type_answer_as_text`.
- `show_rejects_an_unknown_entry`.
- `show_rejects_an_unknown_card_and_lists_the_card_types`.
- `show_finds_an_entry_id_that_contains_a_slash`.
- `show_renders_the_sample_of_a_notetype`.
- `show_puts_the_cards_after_the_diagnostics_in_json`.
- `show_compares_the_typed_answer`.
- `show_html_needs_one_side_of_one_card`.
- `show_html_has_file_urls_and_loads_nothing_from_janky`.
- `show_html_warns_about_mathjax`.
- `show_writes_no_files`.

Front end (`vitest`, `jsdom`):

- The shim replaces the media names in `src`, `poster` and `url()` before the browser loads them.
- The shim leaves other names alone.
- A play button plays the `file://` URL.
- The shim does not load MathJax when `mathjax` is `false`.

## Order of work

Each step is one commit, with its tests.

1. Move the pipeline of `preview.rs` to a shared function. The behaviour does not change.
2. `janky_anki::html_to_text`.
3. `show.rs`: the identifiers, the list, the text output and the JSON output, with the diagnostics and their
   codes in `codes.md`.
4. The HTML output: the file mode of the card document, the shim changes and the build checks.
5. Documentation.
