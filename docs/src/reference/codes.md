# Diagnostic codes

Each diagnostic starts with a code, for example `content::required`. The part before `::` is the
area of the problem. The part after `::` is the kind of problem. This page lists every code that you
can meet, grouped by area. [Reading errors and warnings](../cli/diagnostics.md) explains the rest of
a diagnostic.

In the tables, **E** is an error, **W** is a warning and **I** is an info. An error stops `plan`,
`build`, `generate`, `prune`, `publish` and `pull`, and janky exits with status 1. A warning or an info never stops a
command. Without `--verbose`, the terminal shows the infos of one code as one line with a count.

## Codes from the schema check

janky checks `deck.toml`, the note type files, `deck.lock` and the content files against a schema
(a list of the allowed keys and values). A problem that the schema finds gets a code in this form:

```text
<area>::<rule>
```

- The area is `config` for `deck.toml`, `notetype.toml` and `deck.lock`, `content` for content
  files, and `variables` for the `variables` of a plugin instance in `deck.toml`.
- The rule is the name of the schema rule that failed.

All of these codes are errors.

| Rule | Meaning | Typical fix |
|------|---------|-------------|
| `required` | A key that must be there is missing, for example ``missing key `english` in `entries[2]` ``. | Add the key. |
| `unknown-key` | A key that is not allowed, often a typo, for example ``unknown key `entries[2].englsh` ``. | Correct the spelling, or remove the key. |
| `type` | A value has the wrong type, for example a number where text must be. | Change the value. In YAML, put quotes around text that looks like a number or a boolean (`'012'`, `'True'`). The `help` line shows the quoted value. |
| `enum` | A value is not one of the allowed values. | Use one of the values that the message lists. |
| `pattern` | A text value does not have the necessary form, for example an `id` that is not a slug, or a host name with `https://`. | Change the value to the form that the message shows. |
| `minimum`, `maximum` | A number is too small or too large. | Use a number in the range. |
| `min-length` | A text value is too short (often empty). | Give a longer value. |
| `min-properties` | A table is empty, but it must have at least one key. | Add a key to the table. |
| `const` | A value must be one exact value. | Use the value that the message names. |
| `property-names` | A key name in a table is not allowed, for example a secret name with a hyphen. | Rename the key. Secret names and `filesystem.read` names use letters, digits and `_`. |

A note type with a full JSON Schema file can use other rules. Their codes follow the same form, for
example `content::min-items`. [The content schema](../notetypes/schema.md) explains the schema.

> [!NOTE]
> janky checks the schema of `deck.toml` before the other checks. When you correct the schema
> errors, a second group of errors can show on the next run.

## Parsing: `toml`, `yaml`, `json`

These errors mean that janky cannot read the file as TOML, YAML or JSON. A content file with one
of these errors gives no entries.

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `toml::syntax` | E | The TOML is not valid, for example a text value without quotes. | Correct the line that the message shows. |
| `toml::number` | E | An integer is out of range, or a number is `inf` or `nan`. | Use a normal number. |
| `yaml::syntax` | E | The YAML is not valid. | Correct the line that the message shows. Check the indentation and the quotes. |
| `yaml::duplicate-key` | E | One key is two times in one mapping. | Remove or rename one of the keys. |
| `yaml::documents` | E | The file has more than one YAML document (a second `---`). | Use one document for each file. |
| `yaml::key` | E | A key is not text, for example `1:`. | Put quotes around the key. |
| `yaml::number` | E | A number is `.inf` or `.nan`. | Use a normal number, or put quotes around the value. |
| `yaml::unsupported` | E | The file uses a YAML feature that janky does not support. | Write the value in full. |
| `json::syntax` | E | The JSON is not valid, for example a trailing comma or a comment. | Correct the line that the message shows. JSON has no comments and no trailing commas. |
| `json::duplicate-key` | E | One key is two times in one object. | Remove one of the keys. |
| `json::number` | E | A number is out of range. | Use a smaller number. |

## Project and configuration: `project`, `config`, `lock`

These codes come from `deck.toml`, the note type files and `deck.lock`.

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `project::read` | E | janky cannot read the deck file. | Run janky in the project directory, or give the path to the file (not the directory) with `--deck`. |
| `config::invalid` | E | janky cannot read a value after the schema check. | Correct the value that the message names. |
| `config::key` | E | A table key (deck, preset, note type or plugin instance) is not valid, for example `Extra`. | Use lowercase letters, digits, `_` and `-`, and start with a letter. |
| `config::duplicate-deck` | E | Two decks have the same full name. | Give one of the decks a different `name`. |
| `config::duplicate-default` | E | More than one note type has `default = true`. | Keep `default = true` on one note type. |
| `config::duplicate-type` | E | Two note types use the same entry type (a key or an `alias`). | Rename the `alias`. Entry types are for the whole project. |
| `config::unknown-preset` | E | A deck or an `extends` names a preset that does not exist. | Add the `[presets.<key>]` table, or correct the name. |
| `config::preset-loop` | E | The `extends` chain of a preset comes back to the same preset. | Remove `extends` from one preset of the loop. |
| `config::preset` | E | Anki does not accept a preset value, for example a `maximum_interval` over 36500 days, or the wrong number of `fsrs_params`. Only `plan` and `build` find this, not `check`. | Change the value in the preset. |
| `config::source` | E | The source of a remote note type is not valid, for example `git` with `url`, or a `url` without `sha256`. | Use one of the combinations in [Adding note types](../deck/notetypes.md). |
| `config::endpoint` | E | The `endpoint` of `[publish]` is not a valid URL, does not use `https://`, holds a user name or a password, or has a path that does not end with `/`. | Use an address such as `https://anki.example.org/`. Plain `http://` is allowed only on loopback. |
| `config::outside-project` | E | `output` or a plugin `path` is outside the project directory. | Use a path inside the project. |
| `config::outside-notetype` | E | A path in a note type file goes outside the note type's directory. | Move the file into the note type's directory. |
| `config::missing-notetype` | E | The note type file does not exist. | Correct the `path`. |
| `config::missing-file` | E | A file that a note type or plugin instance names does not exist. | Correct the path, or add the file. |
| `config::not-a-file` | E | A path names a directory or another thing that is not a normal file. | Give the path of a file. |
| `config::missing-media-dir` | W | A directory in `media_dirs` does not exist. | Make the directory, or remove it from `media_dirs`. |
| `config::read` | E | janky cannot read a schema file of a note type. | Make sure that the file exists and that you can read it. |
| `config::invalid-schema` | E | The note type's JSON Schema file is not a valid JSON Schema. | Correct the schema file. |
| `config::unknown-field` | E | `sort_field` names a field that does not exist. | Use the key of a field in `[fields]`. |
| `config::value-and-template` | E | A field has both `value` and `template`. | Keep one of them. |
| `config::reserved-key` | E | The `[schema]` uses `type` or `tags`. janky adds these keys itself. | Remove the key, or give it a different name. |
| `config::id-type` | E | `id` in `[schema]` is not `string` or `slug`. | Use `string` or `slug`. |
| `config::field-spec` | E | A `[schema]` option does not fit the type, for example `values` without `enum`, a `list` without `of`, or an `object` with both `schema` and `fields`. | Correct the key as the message tells you. |
| `config::unknown-def` | E | A `[schema]` key names a definition that is not in `[schema.defs]`. | Add the definition, or correct the name. |
| `lock::read` | E | janky cannot read `deck.lock`. | Make sure that you can read the file. If you changed it by hand, get it back from Git or your backup. |

A `deck.lock` with wrong contents gives `config::` schema codes. Do not change `deck.lock` by
hand.

## Remote note types: `source`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `source::git` | E | janky cannot clone or fetch the Git repository, or it cannot find the tag, branch or commit. With `--offline`, the repository is not in the cache. | Check the URL and the reference. Run without `--offline` once. |
| `source::archive` | E | janky cannot download or unpack the archive, or the SHA-256 is different. With `--offline`, the archive is not in the cache. | Check the URL and the `sha256`. Run without `--offline` once. |
| `source::insecure` | W | A `git://` source has no pinned commit. `git://` has no encryption. | Use `https://`. If you must use `git://`, check the commit that `deck.lock` records after the build. |

## Content files: `content`

The schema codes (`content::required` and the others) are in the
[first section](#codes-from-the-schema-check).

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `content::format` | E | A file that a `content` pattern matches is not a `.yaml`, `.yml`, `.toml` or `.json` file. This includes editor swap files. | Make the pattern end in an extension, for example `content/vocab/*.yaml`. |
| `content::read` | E | janky cannot read a content file, for example because it is not UTF-8 text. | Save the file as UTF-8. |
| `content::pattern` | E | A `content` pattern in `deck.toml` is not valid, for example `content/[`. (The same code is also the schema rule for a value with the wrong form.) | Correct the pattern. |
| `content::two-decks` | E | Two decks match the same file. | Change the patterns so that each file belongs to one deck. |
| `content::duplicate-id` | E | An entry `id` is already used. Ids are for the whole project. | Give the entry a different `id`. |
| `content::schema` | E | janky cannot make the content schema. This also happens when `deck.toml` has no `[notetypes]`. | Add at least one note type to `deck.toml`. |

## Templates and card templates: `template`, `card`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `template::syntax` | E | A field template or a `when` expression has a syntax error. | Correct the template at the place that the message shows. |
| `template::unknown-filter` | E | A field template uses a filter that does not exist. janky finds it only when an entry uses the template. | Correct the name, or add the plugin that gives the filter. |
| `template::unknown-function` | E | A field template uses a function that does not exist. | Correct the name, or add the plugin that gives the function. |
| `template::render` | E | Another error while janky fills a field. Examples are a filter that gets the wrong kind of value, and `media` or `enrich` with the wrong plugin instance. | Read the message. It names the problem. |
| `template::read` | E | janky cannot read a template file or an asset file. | Correct the path. |
| `template::too-large` | E | A field is larger than 1 MiB, or all fields together are larger than 256 MiB. | Make the field smaller. |
| `template::not-text` | E | A field has no `value` or `template`, and the entry's value is a list or a table. | Add a `value` or `template` that turns the value into text. |
| `template::script-end` | E | An asset file that janky puts into a card template contains `</script>`. | Remove `</script>` from the asset file. |
| `card::unknown-field` | E | A card template names a field that the note type does not have, for example `{{Englsh}}`. | Use the field's display name (`name`). The `help` line lists the names. |

## Media: `media`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `media::missing` | E | A media file that an entry names does not exist in `media_dirs`. An empty name (``media file `` does not exist``) means that the entry has no value for the key. | Add the file, or correct the name. Put `media(...)` inside `{% if entry.<key> %}`. |
| `media::outside` | E | A media path goes outside the media directories, for example `../deck.toml`. | Put the file in a media directory. |
| `media::name-collision` | E | Two media files have the same file name in different directories. Anki keeps all media in one folder. | Rename one of the files. |

## Plugins: `plugin`, `variables`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `plugin::read` | E | janky cannot read the plugin script. | Correct the `path`. |
| `plugin::lua` | E | janky cannot start the plugin script. | Ask the plugin author. |
| `plugin::syntax` | E | The plugin script has a Lua syntax error. | Correct the line, or ask the plugin author. |
| `plugin::runtime` | E | The plugin had an error while it ran. During `janky generate`, the job fails. | Read the message and the `help` line. Ask the plugin author if the cause is not in your content. |
| `plugin::table` | E | The table that the plugin returns is not correct. Other causes: the instance does not supply something that the plugin needs (for example a secret), or two plugins give the same filter name. | Read the message. Often you must add a key to the plugin instance in `deck.toml`. |
| `plugin::id` | E | Two different generator scripts use the same plugin `id`. | Remove one of the scripts, or ask the plugin author. |
| `plugin::filesystem` | E | A `[plugins.<name>.filesystem]` setting is not valid, for example `~/` when `HOME` is not set. | Correct the directory. |
| `plugin::unused-secret` | W | The instance has a secret that the plugin does not use. | Remove the secret from `[plugins.<name>.secrets]`. |
| `plugin::unused-access` | W | The instance has `http` or `secrets`, but the plugin is not a generator. | Remove the `http` and `secrets` tables of that instance. |
| `plugin::result` | E | A plugin filter or function returned a value that janky cannot use. | Ask the plugin author. |
| `plugin::plan` | E | The generator's `plan` function returned a wrong result. | Ask the plugin author. |
| `variables::schema` | E | janky cannot make the schema of the plugin's `variables`. | Ask the plugin author. |

A diagnostic can also carry a related line with the code `plugin::call` and the text "the plugin
was called here". It shows the place in your template that called the plugin. The `variables::`
schema codes are in the [first section](#codes-from-the-schema-check).

A plugin call that runs longer than 10 seconds stops janky at once. You then see a line that starts
with `error: the plugin call`, without a code. See
[Troubleshooting](troubleshooting.md).

## Generators: `generate`, `enrich`, `secret`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `generate::missing` | W, E | Generated media files are missing from the cache. A warning in `check` and `plan`, and in `build --allow-missing`. An error in `build`. | Run `janky generate`. |
| `enrich::missing` | W, E | Generated data results are missing. Same rule as `generate::missing`. | Run `janky generate`. |
| `enrich::warning` | W | A data generator gave a result with a warning, for example "2 dictionary entries, set `pos`". janky keeps the result and shows the warning on each command. | Change the input of the job (for example add `pos`), or accept the result. |
| `enrich::info` | I | A data generator gave a result with an info, for example "not in the dictionary". janky keeps the result. Without `--verbose`, all infos show as one line. | Nothing, or run with `--verbose` to see each one. |
| `secret::missing` | E | `janky generate` cannot find a secret, or `janky publish` or `janky pull` cannot find the password of `[publish]`. Or the value is not valid (8 to 8192 printable ASCII characters). | Set the environment variable, or add it to `.env`, as the message tells you. |
| `generate::locked` | E | Another `janky generate`, `janky prune` or `janky pull` runs for this project. | Wait until it ends, then run again. |
| `generate::lock` | E | janky cannot open or lock `.janky/generate.lock`. | Make sure that you can write to `.janky/`. |
| `generate::confirm` | E | janky must ask before it changes the cache, but the input is not a terminal. | Run again with `--yes`. |
| `generate::client` | E | janky cannot make the network client, for example because a `rate_limit` allows fewer than one request in 60 seconds. | Correct `[plugins.<name>.http]`. |
| `generate::host` | E | janky cannot prepare the functions for a plugin. | Run again. If it happens again, report it. |
| `generate::runtime` | E | janky cannot start the job runner. | Run again. If it happens again, report it. |
| `generate::prepare` | E | The `prepare` step of an instance failed. All jobs of that instance fail. | Read the message. Often it is a network or file problem. |
| `generate::instance` | E | The instance has no `generate` function. | Use an instance of a generator plugin. |
| `generate::time` | E | A job ran longer than its time limit (600 seconds). | Run again. If it happens again, ask the plugin author. |
| `generate::result` | E | The generator returned a wrong result. | Ask the plugin author. |
| `generate::store` | E | janky cannot write the result into the cache. | Make sure that you can write to `.janky/cache/` and that the disk has space. |
| `generate::refused` | E | The plugin refused the job, for example because the service said no. janky does not keep the job, so it runs again next time. | Read the message. Correct the input, or wait and run again. |
| `generate::unauthorised` | E | The service refused the secret (HTTP 401 or 403). janky starts no more jobs. | Check the secret's value and your account's quota. |

## Prune: `prune`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `prune::pending` | E | Some jobs have no result yet, so janky cannot see every job of the project. | Run `janky generate` first. |
| `prune::remove` | E | janky cannot delete a file in the cache. | Make sure that you can write to `.janky/cache/`. |

## Publish and pull: `publish`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `publish::config` | E | `deck.toml` has no `[publish]` table. | Add `[publish]` with `endpoint`, `username` and `password`. |
| `publish::login` | E | The server refused the user name or the password, or janky cannot connect to it. | Check `username` and the password. Both must match `SYNC_USER<n>` on the server. Check the address and your network. |
| `publish::locked` | E | Another `janky publish` runs for this project. | Wait until it ends, then run again. |
| `publish::lock` | E | janky cannot open or lock `.janky/publish/lock`, or it cannot write `deck.lock`. If another command changed `deck.lock` while `publish` ran, janky leaves it as it is. | Make sure that you can write to `.janky/`. For `deck.lock`, run `janky publish` again. |
| `publish::write` | E | janky cannot write in `.janky/publish/`, or a file there is a symbolic link. | Make sure that `.janky/publish/` is a normal directory inside the project. |
| `publish::copy` | E | janky cannot open its copy of the collection. | Delete `.janky/publish/collection.anki2`. The next run downloads it again. |
| `publish::sync` | E | The sync with the server failed, for example a network error, or a clock that is more than 5 minutes off. | Read the message. Check your network and your clock, and run again. |
| `publish::plan` | E | janky cannot compare the project with the collection, for example because a deck must get a name that another deck has, or a deck is a filtered deck in Anki. | Rename or delete the other deck in Anki, then run again. |
| `publish::full-sync` | E | The changes need a full sync, which replaces the collection on the server. | Sync your other devices, then run again with `--allow-full-sync`. |
| `publish::confirm` | E | janky must ask before it changes your collection, but the input is not a terminal. | Run again with `--yes`. |
| `publish::apply` | E | janky cannot apply the project to its copy. It sent nothing. | Read the message. The next run downloads the collection again. |
| `publish::media` | W, E | The media sync or a media download failed, or a file from the server does not match `deck.lock`. A warning when the cache already has the file without its metadata, or when the server did not send all files. | Run the command again. janky does not store a file that does not match `deck.lock`. |

## Preview: `preview`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `preview::bind` | E | janky cannot listen on the port, for example because another program uses it. | Stop the other program, or use another port with `--port`. |
| `preview::no-ui` | E | `--export` needs the web page files, and this janky was built without them. | Run `just assets`, then build janky again. |
| `preview::export-target` | E | The export directory has other files, or the path is a file or a symbolic link. janky wrote nothing. | Use a new directory, or remove the directory first. |
| `preview::export-write` | E | janky cannot write the export, for example because the disk is full. The export directory did not change. | Read the message, then export again. |
| `preview::export-lock` | W | `deck.lock` is missing, or a build would change it, so the export has no `deck.apkg`. | Run `janky build`, then export again. |
| `preview::serve` | E | The preview server stopped because of an error. | Read the message, then start `janky preview` again. |
| `preview::render` | E | Anki rejects a card template, for example a `{{#Field}}` without `{{/Field}}`. The page shows the last cards that rendered. | Fix the card template that the message names, then reload the page. |

## Show: `show`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `show::unknown-entry` | E | No entry has the id. An entry with errors is left out. | Run `janky show` to list the identifiers. Fix the errors of the entry. |
| `show::unknown-card` | E | The note type of the entry, or the note type of `--sample`, has no card type with the key after the `/`. | Use a card key from the help line. |
| `show::unknown-notetype` | E | `--sample` names a note type that `deck.toml` does not have. | Use a note type key from the help line. |
| `show::render` | E | Anki rejects a card template, for example a `{{#Field}}` without `{{/Field}}`. | Fix the card template that the message names. |
| `show::one-side` | E | `--format html` has no `--side`, or the identifier names more than one card. | Add `--side front` or `--side back`, and the card key, for example `gato/recognition`. |
| `show::no-ui` | E | `--format html` needs the preview UI, and this janky was built without it. | Run `just assets`, then build janky again. |
| `show::mathjax` | W | The side has `\(` or `\[`, and MathJax does not run in the HTML output. The maths shows as TeX. | Use `janky preview` to see the maths. |

## Writing files: `build`, `update`, `schema`

| Code | Sev. | Meaning | Typical fix |
|------|------|---------|-------------|
| `build::lock` | E | `deck.lock.tmp` already exists, or janky cannot write `deck.lock`. | If no other janky runs, delete `deck.lock.tmp` and build again. |
| `build::write` | E | janky cannot write the `.apkg` file, or the target is a link or outside the project. | Make sure that `output` is a normal path inside the project. |
| `build::media` | E | janky cannot read a media file while it makes the package. | Make sure that the file exists and that you can read it. |
| `update::lock` | E | `deck.lock.tmp` already exists, or janky cannot write `deck.lock`. | If no other janky runs, delete `deck.lock.tmp` and run `janky update` again. |
| `schema::write` | E | janky cannot write a file in `.janky/schema/`. | Make sure that you can write to `.janky/`. |
