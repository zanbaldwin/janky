# Spec: `janky preview`

Status: draft, not implemented.

## Purpose

`janky preview` starts a local web server that shows each card of the project in the browser. An author can see a
note type change without a build, an import into Anki, and a study session.

The command runs until the user stops it with Ctrl+C. To see the current files, the user reloads the page in the
browser (Ctrl+R or F5). The command does not watch the files.

## Scope

In scope:

- One table of all cards, and one card view with front and back.
- Exact template output, from rslib, for each card of each entry.
- One sample note for each note type that no entry uses.
- The parts of the Anki reviewer that card CSS and card JS often need (see [The card document](#the-card-document)).
- The diagnostics of `janky check`, shown in the page.

Out of scope:

- File watching and automatic reload.
- LaTeX images (they need a LaTeX binary). MathJax is in scope.
- The AnkiDroid JavaScript API, AnkiMobile behaviour, answer buttons, scheduling, image occlusion, and add-on
  filters.
- Access from other computers. `--export` writes static files for a web server instead (see
  [Export](#export)).

## Command line

```text
janky preview [--port <PORT> | --export <DIR>] [--offline] [--deck <deck.toml>]
```

| Flag | Default | Meaning |
| --- | --- | --- |
| `--port` | `5259` | The TCP port on `127.0.0.1`. `0` lets the operating system choose a port. |
| `--export` | none | Write the site to this directory, and start no server. It conflicts with `--port`. |
| `--offline` | off | The global flag. Remote note types come only from the cache. |
| `--deck` | `deck.toml` | The global flag. The path of `deck.toml`. |

On start, the command prints the URL to stderr, for example `http://127.0.0.1:5259/`. It prints the URL also with
`--quiet`. If the port is in use, the command stops with a `preview::bind` error and exit code 1.

`--json` applies only to the diagnostics of the start. The diagnostics of each page load go to the page, not to the
terminal. The plugin log lines of a page load are not shown.

## The read-only rule

`janky preview` without `--export` writes no file. It does not write `deck.lock`, the `.apkg`, `.janky/` or the media cache. The
only exception is the user-wide source cache, which remote note types fill in the same way as for `check`.

Generators never run. The preview reads cached `media` and `enrich` results if they exist. A missing result shows
as a placeholder, in the same way as for `check`.

## Pipeline

Each `GET /preview.json` request runs the pipeline again on the current files:

1. `cli::check` loads the project, the plugins and the content, and renders the fields.
2. `lock::read` reads `deck.lock` if it exists. The preview continues if `deck.lock.tmp` exists. The preview uses a
   default lock file if `deck.lock` does not exist.
3. `package::package_with` runs on the lock file in memory, without `hash_media`. It does not read the media
   files, so a large media directory does not slow down a page load. The preview discards the lock file.
4. The preview adds one sample note for each note type that no entry uses (see [Sample notes](#sample-notes)).
5. `janky_anki::render` renders each card of each note.
6. The result becomes the new snapshot, with a new generation: 16 random bytes in hex (see [Security](#security)).

The pipeline runs on one blocking worker behind a mutex, because the Lua sandbox is single-threaded. Two requests
at the same time run one after the other.

The server keeps only the latest snapshot. A request for an older generation gets `410 Gone`, and the page then
loads `preview.json` again.

`package::package_with` runs when `cli::check` returns `Some`, also if the diagnostics have errors. The render output
does not have the entries that have errors, so the other entries still show.

If `cli::check`, `package::package_with` or `janky_anki::render` fails, the server keeps the last snapshot that has
rows. A failure of `janky_anki::render` gives a `preview::render` error. The response then has the new diagnostics, the old rows, and `"stale": true`. An author with a CSS typo can
then still see the last good cards.

The Lua watchdog calls `process::exit(1)` when a plugin passes the time limit. In the preview, this stops the
server. The user guide must say this.

### Sample notes

A note type that no entry uses gets one sample note. Its deck is empty, and its entry is empty. The table shows
these rows below the content rows, with "N/A" in the entry and deck columns.

The sample note has empty fields. `janky_anki::render` calls `render_uncommitted_card` with `fill_empty = true`.
rslib then puts `(Field name)` in each empty field. This is the same sample that Anki shows in its card layout
screen. The preview does not run field templates, schemas or `enrich`
for sample notes.

## janky-anki: `render`

A new module `crates/anki/src/render.rs`, re-exported from `lib.rs`. The CLI still sends only plain data, and only
janky-anki uses Anki types.

```rust
pub struct Rendered {
    /// One list for each note of the package, with one card for each card template of its note type.
    pub notes: Vec<Vec<std::result::Result<RenderedCard, String>>>,
    /// One list for each note type id in `samples`, with one card for each card template.
    pub samples: Vec<Vec<std::result::Result<RenderedCard, String>>>,
}

pub struct RenderedCard {
    pub question: RenderedSide,
    pub answer: RenderedSide,
    /// The front side is empty, so Anki does not make this card.
    pub is_empty: bool,
}

pub struct RenderedSide {
    pub html: String,
    pub sounds: Vec<Sound>,
}

pub enum Sound {
    File(String),
    Speech { text: String, language: String },
}

pub fn render(package: &Package, samples: &[i64]) -> Result<Rendered>;
```

`render`:

- Makes a temporary collection and writes the package with the same code as `write_apkg`, but without the media.
  The preview runs on each page load, so it must not read every media file.
- Calls `render_uncommitted_card` for each card template of each note. For a card that Anki made, rslib uses the
  real card, so `{{Deck}}` and `{{Subdeck}}` show the deck names. For a card with an empty front, Anki made no
  card, so rslib uses a new card, and `{{Deck}}` shows `(Deck)`.
- Calls `render_uncommitted_card` with `fill_empty = true` for each card of a sample note. A sample note is not in
  the collection, so `{{Deck}}` shows `(Deck)`, as in Anki's card layout screen.
- Calls `extract_av_tags` on each side, so `[anki:play:q:0]` tags replace the sound tags.
- Puts an error of one card in `Err`, and continues with the next card.
- Returns an `Error` if Anki rejects a card template. Anki checks the templates when it adds a note type, so the
  preview shows the same error as `janky build`.

janky note types are never cloze note types, so each card template gives exactly one card.

Anki's Python reviewer replaces the `[[type:…]]` tags that rslib renders for `{{type:Field}}`
(`typeAnsQuestionFilter` and `typeAnsAnswerFilter` in `qt/aqt/reviewer.py`). The module `type_answer.rs` of
janky-anki does the same, with the same HTML and the `regex` crate (already a dependency of rslib):

- `render` replaces the tags of the question. The first tag names the field, with an optional `nc:` prefix. A known
  field with a value gives Anki's `<input id=typeans onkeypress="_typeAnsPress();">` with the font of the field,
  and `RenderedCard::type_answer` gets the value. An empty field removes the tags. An unknown field gives Anki's
  "Type answer: unknown field" message. The `cloze:` prefix is not handled, because janky has no cloze note types.
- `compare_typed(answer_html, type_answer, typed) -> Result<String>` replaces the tags of the answer. It removes
  `<hr id=answer>` and puts it in front of each comparison, as Anki does. rslib's `compare_answer` is not public,
  so the function calls it through `CardRenderingService` on an in-memory collection.

A back that starts with `{{FrontSide}}` also has the tag of the front, so it shows the comparison twice, as in
Anki. Anki's own type-in note type uses `{{Front}}` on the back.

`{{CardFlag}}` shows `flag0` and `{{CardID}}` shows `0`, as for a new card without a flag in Anki.

## HTTP routes

The server uses `axum` 0.8. `axum` is already a dependency of rslib, so it adds no new crate to the build. A future
Anki version can change the `axum` version. Bumping Anki must then also check the `axum` version of janky-cli.

All routes are `GET`. Any other method gets `405`.

Each route is also a file path, so that `--export` can write the same site as static files. All URLs between the
files are relative, so the site also works below a subpath.

| Route | Response |
| --- | --- |
| `/` | The page: `index.html` from the Vite build (`base: "./"`). |
| `/assets/{path}` | A file from the Vite build. |
| `/preview.json` | Runs the pipeline and returns the snapshot as JSON. |
| `/{generation}/cards/{row}-{side}.html` | The card document for the iframe. `side` is `front` or `back`. |
| `/{generation}/media/{name}` | A media file of the snapshot, by its Anki name. |

### `GET /preview.json`

```json
{
  "generation": "9f2c51d0a4e8b7c63e1f0a5d2b8c4e71",
  "exported": false,
  "stale": false,
  "diagnostics": [ ... ],
  "rows": [
    {
      "row": 0,
      "notetype": { "key": "vocab", "name": "Vocabulary" },
      "entry": { "id": "hello", "file": "content/words.yaml", "index": 0 },
      "deck": { "key": "words", "full_name": "Japanese::Words" },
      "card": { "key": "recognition", "name": "Recognition", "ord": 0 },
      "empty": false,
      "type_answer": false,
      "error": null
    }
  ]
}
```

- `diagnostics` has the same objects as the JSON Lines of `janky check --json`.
- For sample notes, `entry` and `deck` are `null`.
- `entry.index` is the position of the entry in the `entries` of its file, from 0. It also counts the entries with
  errors, so it matches the file.
- `empty` is `true` if the front is empty, so Anki does not make the card. An example is a card with `when` that
  does not apply to the entry. The table shows these rows.
- `type_answer` is `true` if the front has a `{{type:Field}}` input.
- `exported` is `false` from the server, and `true` in an export.
- `error` is the error message of the card, or `null`.
- The rows are in deck order (as in `deck.toml`, a parent before its children), then in the order of the `content`
  patterns of the deck, then in path order, then in the order of the notes inside each file (`new_order`), then card
  ordinal. Sample rows come last, in note type order.
- All text goes through `json_text()`, so secrets are redacted.

### `GET /{generation}/cards/{row}-{side}.html`

Query parameters. The shim reads `night` and `platform` from its own URL, so they also work for static files. The
server reads only `typed`.

| Parameter | Values | Default |
| --- | --- | --- |
| `night` | `0`, `1` | `0` |
| `platform` | `desktop`, `android`, `iphone` | `desktop` |
| `typed` | the typed answer. Only the back uses it. | empty |

The page changes these parameters and loads the iframe again. The server keeps no state for them. An unknown value
of `night` or `platform` gives the default classes. An unknown side, or a row that is out of range or did not render,
gets `404`.

## The card document

The server makes one HTML document for each card side:

```html
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <base href="../media/">
  <style>/* the CSS of the Vite `card` entry, inline */</style>
</head>
<body class="card card{N}">
  <div id="qa"></div>
  <script type="application/json" id="janky-card">{"html": …, "ord": …, "side": …, "sounds": […]}</script>
  <script type="module">/* the JS of the Vite `card` entry, inline */</script>
</body>
</html>
```

The shim JS and CSS are inline, so the iframe, which has an opaque origin, needs no CORS header for them. The
document has no generation in it. `janky show --format html` uses the same builder without `<base>`, and its card
data has `bodyclass`, `media` and `mathjax: false`.

`html` is `<style>{note type CSS}</style>{card HTML}`, as Anki's `question_and_style()` gives it. The server escapes
each `<` in the JSON as `\u003c`, so that card HTML cannot end the `<script>` element.

The shim does not get the card HTML in the document, because Anki's reviewer also puts it in later. It follows
`_updateQA` of `ts/reviewer/index.ts`:

1. It sets `onUpdateHook` to one callback that sets the body class (`bodyclass`, or else the classes from `ord` and
   the query of the URL), and `onShownHook` to an empty array.
2. It puts the HTML into `#qa` with `opacity: 0`, and replaces each `<script>` with a new element, so that it runs.
3. It runs `onUpdateHook`, sets `opacity: 1`, and runs `onShownHook`. A hook that throws is logged.
4. On the back, it scrolls to `#answer`.

Card JS that waits for `DOMContentLoaded` does not run, as in Anki.

The class names come from `qt/aqt/reviewer.py` and `qt/aqt/theme.py` of the pinned Anki tag:

| Setting | Classes |
| --- | --- |
| always | `card card{ord + 1}` |
| `night=1` | `nightMode night_mode` |
| `platform=desktop` | `isLin` |
| `platform=android` | `mobile android` |
| `platform=iphone` | `mobile iphone` |

The page shows `android` and `iphone` in a narrow frame, so media queries in the card CSS also apply.

The `card` Vite entry has the reviewer shim and MathJax:

- **Base CSS.** Only the rules of `ts/reviewer/reviewer.scss` that change the layout of card content: `hr`,
  `body` (`margin` and `overflow-wrap`), `body.nightMode` (with the `--canvas` and `--fg` values of the Anki
  palette), `img`, `li` and `pre`. The flag and mark icons and image occlusion are not copied. A comment in the CSS
  names the Anki tag of the source.
- **MathJax 3.** The `tex-chtml-full` component of MathJax 3.2.2 (the version and the component of Anki's
  reviewer), with the configuration of `ts/mathjax/index.ts`. A Vite plugin copies the component and its fonts to
  `assets/mathjax-3.2.2/`. The shim loads it from `../../assets/mathjax-3.2.2`, relative to the `<base>` of the
  card document. Their names have no hash, because MathJax loads the fonts by name, so the directory has
  the version. The shim loads MathJax only for a side that has `\(` or `\[`, after `onUpdateHook`, as Anki does.
  It needs no network access.
- **Hooks.** `onUpdateHook` and `onShownHook` are arrays on `window`. The shim runs them in the same order as
  `ts/reviewer/index.ts`. `pycmd` writes its argument to the console and does nothing else. jQuery (`$`) and the
  `anki` object of the desktop reviewer are not there.
- **Sound.** The card data has the sounds of the side. Each `[anki:play:q:N]` tag becomes Anki's play button
  (`av_refs_to_play_icons` in `qt/aqt/sound.py`), before the card scripts run. Its `pycmd('play:q:N')` plays sound
  `N`: a file from `/media/`, or speech through the speech synthesis of the browser, in the language of the tag.
  A new sound stops the sound before it. The preview does not play sounds automatically.
- **Type answer.** The shim focuses `#typeans` on the front, as Anki does. On each change of the input, it sends
  the typed text to the page with `postMessage`, with the origin of the server as the target. `_typeAnsPress`
  calls `pycmd("ans")` on Enter, and `pycmd("ans")` also asks the page to show the back. The page accepts only
  messages whose source is its iframe. It adds the text as `typed` to the URL of the back. On the back, the server
  replaces the tags with `janky_anki::compare_typed`. A new row or the Front button clears the typed text. The
  base CSS has the `#typeans`, `.typeGood`, `.typeBad` and `.typeMissed` rules of `reviewer.scss`.

## The page

React 19 and TypeScript, built with Vite. The source is in `crates/cli/ui/`.

- On the left: a table with the columns deck, entry, source, notetype and card. The source is the file and the
  entry index, for example `content/words.yaml #0`. A filter field above the table filters the
  rows by text. A "Show empty cards" checkbox below the filter field hides the rows with `empty: true`. It is off
  by default, and the page does not keep its value.
- On the right: the iframe, with buttons for front and back, a night mode checkbox (on by default), and a platform
  menu. Android shows the card 412 pixels wide, and iPhone 390 pixels wide.
- Above the table: the diagnostics, as errors and warnings with their file, line and column. A banner shows when
  the rows are stale.
- The row, the side, night mode and the platform are in the URL hash, so Ctrl+R keeps them.
- A line between the table and the card changes their widths: drag it, or focus it and use the arrow keys. The
  table is at least 200 pixels wide, and the card at least 240. The browser keeps the width in `localStorage`. The
  panes stay side by side on a narrow screen.
- A row with a template error shows the error in the iframe area.

## Build

### Front-end assets

- Copy `crates/vite` from rawr into `crates/vite` here, as janky's own copy. The copy uses the janky workspace
  licence, AGPL-3.0-or-later. The user owns the copyright of both projects.
- `crates/cli/build.rs` follows rawr's `crates/web/build.rs`:
  - It reads `ui/dist/.vite/manifest.json` and sets `cfg(ui_built)`.
  - A debug build without the UI still compiles. The server then shows a message at `/` that says "run
    `just build`".
  - A release build without the UI fails.
  - It also writes a table of `include_bytes!` entries for each file in `ui/dist/` into `OUT_DIR`. This replaces
    `rust-embed`, which is not in the dependency tree.
- `ui/dist/` is in `.gitignore`. The build output is not committed.
- `vite.config.ts` has two entries, `index` and `card`, and a dev server proxy to `127.0.0.1:5259` for
  `/preview.json` and the generation paths.

### `justfile`

| Recipe | Behaviour |
| --- | --- |
| `just assets` | Runs `npm ci` if `node_modules/` does not exist, then the Vite build. Fails if `npm` is not installed. |
| `just setup` | Fetches protoc and Anki, then runs `just assets`. If `just assets` fails, it shows a warning and continues. |
| `just build` | Runs `just assets`, then `cargo build`. If `just assets` fails, it shows a warning and continues. A release build then fails in `build.rs`. |
| `just ui-dev` | Starts the Vite dev server. |
| `just ui-check` | Runs the TypeScript check and the `vitest` tests. |
| `just check` | Runs clippy and rustfmt, then `just ui-check`. If `npm` is not installed, it shows a warning and does not run `just ui-check`. |

npm runs on the host, not in the toolbox container. Node is a requirement to build the preview UI from source.
`docs/src/installation.md` must say this, and give the Node version from `crates/cli/ui/.nvmrc`.

## Security

The preview matches the security of the rest of janky:

- The server listens on `127.0.0.1` only. It does not listen on IPv6 or on `0.0.0.0`.
- The server refuses a request with `421` if the `Host` header is not `127.0.0.1:{port}` or `localhost:{port}`.
  This stops DNS rebinding.
- The iframe has `sandbox="allow-scripts"` without `allow-same-origin`. Card JS then has an opaque origin and
  cannot read `/preview.json`. Card JS still has network access, as in Anki.
- `/{generation}/media/{name}` looks up `name` in the media map of the snapshot. A URL never becomes a file path.
  The server reads the file in the same way as the build.
- The iframe has an opaque origin, so its fonts and the MathJax files need CORS. The shim is inline, so it needs
  none. `/assets/` and `/{generation}/media/` send `Access-Control-Allow-Origin: *`. Another web page can send
  requests to `127.0.0.1` too, because the `Host` header is then correct. But it cannot read `/preview.json`, which
  has no CORS header, so it cannot know the
  random generation in the card and media URLs. A counting generation number would let it guess them.
- All text that comes from the project or from plugins goes through `json_text()` or the HTML escape of the
  server. Secrets are redacted.
- The responses have `Cache-Control: no-store`, except `/assets/`, which has hashed file names and
  `immutable`.
- The page and the API have `Content-Security-Policy` headers. The card document has only `sandbox allow-scripts`,
  because card JS must work as in Anki. Media files have `default-src 'none'; style-src 'unsafe-inline'; sandbox`.
  So the card document and the media files (for example an SVG) never run in the origin of the page, also when the
  user opens them directly.

## Export

`janky preview --export <DIR>` writes the site as static files (`crates/cli/src/preview/export.rs`). The user guide
(`docs/src/cli/commands.md`) has the directory layout and an Nginx example.

1. It refuses before the pipeline if the UI is not built (`preview::no-ui`), or if `DIR` is a link, a file, or a
   directory that is not empty and has no `.janky-export` marker (`preview::export-target`).
2. `cli::prepare`, as for `janky build`. Any error stops the export, and it writes nothing. Missing generator
   results are warnings (`missing_media` with `strict = false`).
3. `deck.apkg` is written only if `deck.lock` exists and `previous == next`. Otherwise the ids of the package would
   differ from the next build, so the export gives the warning `preview::export-lock` and has no `deck.apkg`. The
   export never writes `deck.lock`.
4. `preview::render` with `Selection::All`. The back of each card is `compare_typed` with an empty typed answer.
5. The files go into a new sibling directory `<DIR>.tmp-<hex>`. The cards and media go into a pending directory,
   which gets its name when the generation is known. The generation is the first 32 hex digits of a BLAKE3 hash
   of the rows, the card documents and the media files, each with length prefixes. The same project gives the same
   files, except `deck.apkg`, which Anki writes differently each time.
6. `preview.json` has `"exported": true` and no diagnostics. The marker is the last file.
7. An absent or empty `DIR` is replaced with a rename. An earlier export is renamed to `<DIR>.old-<hex>`, the new
   directory is renamed to `DIR`, then the old one is deleted. After an error, janky deletes the staging directory.

The export is the only part of janky that writes outside the project. It never writes through a link, because it
writes only into the directory that it made.

In an export, the page does not send the typed text, and the back of a card with `type_answer` has a note that only
`janky preview` compares the typed answer.

## Diagnostics

The preview adds these codes. `docs/src/reference/codes.md` lists them:

- `preview::bind`: the server cannot listen on the port.
- `preview::serve`: the server stopped because of an error.
- `preview::render`: Anki rejects a card template. The response is stale.
- `preview::no-ui`: `--export` in a janky without the UI build.
- `preview::export-target`: the export directory is not absent, empty, or an earlier export.
- `preview::export-write`: the export cannot write its files.
- `preview::export-lock` (warning): the export has no `deck.apkg`.

`tests/data/errors/` has no fixtures for these codes, because those fixtures run only `cli::check`. The server tests
cover them.

## Documentation

- `docs/src/cli/commands.md`: a `preview` section, and the flag table.
- `docs/src/notetypes/styling.md`: a short section about the preview, the body classes and the platform presets.
- `docs/src/installation.md`: Node as a build requirement.
- `docs/src/how-it-works.md`: one line about the preview, and that it writes nothing.
- The `//!` of `crates/cli/src/lib.rs`: one line for the `preview` module.

## Tests

janky-anki (`crates/anki/tests/render.rs`):

- `render_shows_the_front_side_on_the_back` (`{{FrontSide}}`).
- `render_fills_empty_fields_of_a_sample_note_with_the_field_names`.
- `render_makes_one_card_for_each_card_template`.
- `render_shows_the_deck_of_the_note_in_special_fields`.
- `render_marks_a_card_with_an_empty_front`.
- `render_fails_for_a_sample_of_an_unknown_notetype`.
- `render_fails_on_a_card_template_that_anki_rejects`.
- `render_replaces_sound_tags_with_play_tags`.
- `render_puts_a_text_input_in_place_of_a_type_tag`.
- `render_compares_without_combining_characters_for_type_nc`.
- `render_removes_the_type_tag_of_an_empty_field`.
- `compare_typed_marks_the_wrong_characters`.
- `compare_typed_shows_the_answer_when_nothing_is_typed`.
- `compare_typed_keeps_a_back_without_type_tags_after_the_line`.

janky-cli (`crates/cli/tests/preview.rs`, loopback server on port 0, like `tests/http.rs`):

- `preview_returns_one_row_for_each_card_of_the_example_project`.
- `preview_adds_a_sample_row_for_an_unused_notetype`.
- `preview_writes_no_files` (compare the project tree before and after).
- `preview_refuses_a_foreign_host_header`.
- `preview_refuses_other_methods`.
- `preview_serves_the_page`.
- `preview_returns_rows_when_an_entry_has_an_error`.
- `preview_does_not_read_the_media_files`.
- `preview_reports_a_card_template_that_anki_rejects`.
- `preview_refuses_an_old_generation`.
- `preview_serves_a_card_document`.
- `preview_sets_the_night_and_platform_classes`.
- `preview_refuses_an_unknown_side_platform_or_row`.
- `preview_escapes_the_card_data`.
- `preview_serves_a_media_file_of_the_snapshot`.
- `preview_sends_the_sounds_of_the_side`.
- `preview_refuses_a_media_name_that_is_not_in_the_snapshot`.
- `preview_refuses_a_media_path_with_a_slash`.
- `preview_keeps_the_last_rows_when_the_project_breaks`.
- `preview_reads_the_files_again_on_each_request`.
- `preview_compares_the_typed_answer_on_the_back`.

Front end: `vitest` tests for the hash and the filter, and for the reviewer shim in `jsdom` (scripts run, hook
order, scroll to the answer, MathJax only for a side with a delimiter, play buttons before the card scripts, the
`play:q:N` commands, the focus of `#typeans`, the typed messages, Enter in `_typeAnsPress`). In `jsdom`,
the card scripts run in another global object than the test, so the tests check the scripts through the DOM.

All tests stay offline.

## Order of work

Each step is one commit.

1. Copy `crates/vite`, add the `ui/` skeleton, `build.rs`, and the `justfile` recipes.
2. `janky_anki::render`, with its tests.
3. `cli/src/preview.rs`: the server, the pipeline, the snapshot and `/api/preview`, with the server tests.
4. The page: table, iframe, front and back.
5. The card document: base CSS, body classes, night mode, platforms, hooks.
6. Sample notes. Done in step 3: `/api/preview` already has the sample rows.
7. MathJax and sound.
8. Type answer.
9. Documentation.

## Decisions

- `package::package_with` runs on render output that has errors (see [Pipeline](#pipeline)).
  `preview_returns_rows_when_an_entry_has_an_error` tests this.
- `--deck` is a global flag that gives the path of `deck.toml`. The preview accepts it in the same way as `check`.
- `{{Deck}}` shows the deck name for content notes, and `(Deck)` for sample notes (see
  [janky-anki: `render`](#janky-anki-render)).
- The base CSS has only the rules that change the layout of card content (see
  [The card document](#the-card-document)).
