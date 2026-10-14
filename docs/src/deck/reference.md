# deck.toml reference

This page lists every key of `deck.toml`, and the rules that janky checks for the whole project.
The other pages of this part explain the keys in full.

## General rules

- janky looks for `deck.toml` in the current directory. To use another file, give its path with
  `--deck`, for example `janky check --deck ../spanish/deck.toml`.
- All paths in `deck.toml` are relative to the directory that holds it.
- The top level can have only the tables `deck` (required), `presets`, `notetypes`, `plugins` and
  `publish`.
- An unknown key anywhere in the file is an error. janky does not suggest a correct key.
- Keys after a dot in a table header (deck keys under `children`, preset keys, note type keys and
  plugin instance names) follow the key rule. A key starts with a lowercase letter. After that, it
  holds only lowercase letters, digits, `_` and `-`.

## [deck]

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `name` | string | required | The deck name that Anki shows. Also the project name. |
| `output` | string (path) | `".janky/build/deck.apkg"` | Where `janky build` writes the package. Must be inside the project. |
| `media_dirs` | list of strings | `["media"]` | The directories for media files, in search order. An empty list means `["media"]`. |
| `new_order` | string | `"file"` | The order of new cards inside each content file: `"file"` or `"shuffle-in-file"`. |
| `preset` | string | Anki's "Default" preset | The key of a `[presets.<key>]` table. |
| `content` | list of strings | `[]` | Patterns for the content files of this deck. |
| `children` | table | none | The child decks. |

See [Decks and child decks](decks.md).

## [deck.children.\<key\>]

A child deck can have its own `children`, to any depth:
`[deck.children.<key>.children.<key>]`.

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `name` | string | required | The deck name. The full Anki name is the parent's full name, `::` and this name. |
| `preset` | string | Anki's "Default" preset | The key of a `[presets.<key>]` table. |
| `content` | list of strings | `[]` | Patterns for the content files of this deck. |
| `children` | table | none | The child decks of this deck. |

`output`, `media_dirs` and `new_order` are not allowed in a child deck.

## [presets.\<key\>]

Every key is optional. Without a key, Anki uses its own default.

| Key | Type | Allowed values | Anki default |
|-----|------|----------------|--------------|
| `name` | string | any text | the preset key |
| `extends` | string | the key of another preset | none |
| `new_per_day` | integer | 0 to 9999 | 20 |
| `reviews_per_day` | integer | 0 to 9999 | 200 |
| `learning_steps` | list of durations | number and `s`, `m`, `h` or `d` | `["1m", "10m"]` |
| `relearning_steps` | list of durations | number and `s`, `m`, `h` or `d` | `["10m"]` |
| `desired_retention` | number | 0.7 to 0.99 | 0.9 |
| `maximum_interval` | string | whole number and `d`, `w` or `y`, at most 36500 days | `"36500d"` |
| `fsrs_params` | list of numbers | 17, 19 or 21 numbers | Anki's built-in values |
| `new_gather_order` | string | `deck`, `deck-then-random-notes`, `ascending-position`, `descending-position`, `random-notes`, `random-cards` | `"deck"` |
| `new_sort_order` | string | `template`, `no-sort`, `template-then-random`, `random-note-then-template`, `random-card` | `"template"` |
| `review_sort_order` | string | `due`, `due-then-deck`, `deck-then-due`, `ascending-intervals`, `descending-intervals`, `ascending-difficulty`, `descending-difficulty`, `ascending-retrievability`, `descending-retrievability`, `relative-overdueness`, `random`, `added`, `reverse-added` | `"due"` |
| `new_review_mix` | string | `mix-with-reviews`, `after-reviews`, `before-reviews` | `"mix-with-reviews"` |
| `interday_learning_mix` | string | `mix-with-reviews`, `after-reviews`, `before-reviews` | `"mix-with-reviews"` |
| `bury_new` | boolean | `true`, `false` | `false` |
| `bury_reviews` | boolean | `true`, `false` | `false` |
| `bury_interday_learning` | boolean | `true`, `false` | `false` |
| `leech_threshold` | integer | 1 to 9999 | 8 |
| `leech_action` | string | `tag-only`, `suspend` | `"tag-only"` |
| `autoplay` | boolean | `true`, `false` | `true` |
| `replay_question_audio` | boolean | `true`, `false` | `true` |
| `show_timer` | boolean | `true`, `false` | `false` |
| `max_answer_seconds` | integer | 1 to 9999 | 60 |

See [Presets](presets.md).

## [notetypes.\<key\>]

The key is also an entry type of the note type.

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `path` | string | required for a local note type | A `notetype.toml` file, or a directory that holds one. For a remote note type: the file or directory inside the source (default: `notetype.toml` at the top). |
| `alias` | string or list of strings | none | More entry types for this note type. |
| `default` | boolean | `false` | Use this note type for entries without a type. One note type at most. |
| `git` | string | none | The URL of a public Git repository (`https://` or `git://`). Needs one of `tag`, `rev` and `branch`. |
| `tag` | string | none | A Git tag. Only with `git`. |
| `rev` | string | none | A Git commit id or other revision. Only with `git`. |
| `branch` | string | none | A Git branch. Only with `git`. |
| `url` | string | none | The URL of a `.tar.gz`, `.tgz` or `.zip` archive (`https://`). Needs `sha256`. |
| `sha256` | string | none | The SHA-256 checksum of the archive, 64 lowercase hexadecimal characters. Only with `url`. |

See [Adding note types](notetypes.md).

## [plugins.\<name\>]

The name is the plugin instance name that your field templates use. For a generator instance, use
only lowercase letters, digits and `_` in the name. Do not use `-`. [Plugin instances and
secrets](../plugins/instances.md) explains all the keys below.

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `path` | string | required | The Lua script. It must be a regular file inside the project. |
| `variables` | table | `{}` | Settings for the plugin. The plugin's own schema checks them. |
| `http` | table | none (no network) | Network access during `janky generate`. |
| `secrets` | table | none (no secrets) | Secret values, such as API keys. |
| `filesystem` | table | none (only the instance's own cache area) | Directories that the plugin can read. |

### [plugins.\<name\>.http]

| Key | Type | Default | Allowed values | Meaning |
|-----|------|---------|----------------|---------|
| `allowed_hosts` | list of strings | `[]` (no host) | lowercase host names, `*.` and a domain, `localhost` or an IPv4 address | The hosts that the plugin can connect to. No scheme and no port. |
| `max_concurrency` | integer | 1 | 1 to 64 | The number of requests at the same time. |
| `rate_limit` | table | no limit | see below | The most requests in a period. |
| `retry` | table | see below | see below | Retries after a failed request. |

`rate_limit` has two keys:

| Key | Type | Allowed values | Meaning |
|-----|------|----------------|---------|
| `count` | integer | 1 or more | The number of requests. |
| `unit` | string | `second`, `minute`, `hour` | The period. |

`[plugins.<name>.http.retry]` has three keys:

| Key | Type | Default | Allowed values | Meaning |
|-----|------|---------|----------------|---------|
| `max_attempts` | integer | 4 | 1 to 10 | All attempts, including the first. |
| `base_delay_ms` | integer | 1000 | 10 to 60000 | The first wait, in milliseconds. |
| `max_delay_ms` | integer | 30000 | 10 to 60000 | The longest wait, in milliseconds. |

### [plugins.\<name\>.secrets]

Each key is a secret name that the plugin asks for. A secret name starts with a letter or `_`, and
holds only letters, digits and `_`. The value is an inline table:

| Key | Type | Meaning |
|-----|------|---------|
| `env` | string | The name of an environment variable. janky also looks for this name in `.env` files. |
| `file` | string | A file. With `env`, a `.env`-style file. Without `env`, a file that holds only the secret. |

A secret needs at least `env` or `file`.

### [plugins.\<name\>.filesystem]

| Key | Type | Meaning |
|-----|------|---------|
| `read` | table of strings | Names for directories that the plugin can read, for example `read = { lists = "data" }`. A name starts with a letter or `_`, and holds only letters, digits and `_`. The name `cache` is not allowed. |

## [publish]

The sync server and the account that `janky publish` and `janky pull` use. Without this table,
both commands stop with `publish::config`. [Publishing through a sync
server](../cli/publishing.md) explains it.

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `endpoint` | string | required | The address of the sync server, for example `https://anki.example.org/`. It must use `https://`, or `http://` on `localhost`, `127.0.0.1` or `[::1]`. A path must end with `/`. |
| `username` | string | required | The user name of the account. On a self-hosted server, it is the name before the colon in `SYNC_USER1`. |
| `password` | table | required | Where the password is: `env`, `file` or both, as in [`[plugins.<name>.secrets]`](#pluginsnamesecrets). |

## The rules that janky checks

janky checks `deck.toml` in steps. It reads the TOML first, then checks the keys and values, then
checks the project as a whole. If the keys and values have errors, janky stops before the last
step. So after you correct the first errors, you can see new ones.

### Keys and values

| Rule | Code |
|------|------|
| The file must be valid TOML. | `toml::syntax` |
| The file must exist. | `project::read` |
| A required key, such as `deck` or `name`, must be there. | `config::required` |
| Every key must be a known key. | `config::unknown-key` |
| A value must have the correct type. | `config::type` |
| A number must be in its range. | `config::minimum`, `config::maximum` |
| A string must have the correct form (durations, checksums, host names). | `config::pattern` |
| A string must be one of the allowed values. | `config::enum` |
| A secret name or a `filesystem.read` name must have the correct form. | `config::property-names` |
| A deck key, preset key, note type key or plugin instance name must follow the key rule. | `config::key` |

### Decks and content

| Rule | Code |
|------|------|
| `output` must be inside the project. | `config::outside-project` |
| A directory in `media_dirs` should exist (a warning). | `config::missing-media-dir` |
| A `preset` must be the key of a `[presets]` table. | `config::unknown-preset` |
| An `extends` must be the key of a `[presets]` table. | `config::unknown-preset` |
| An `extends` chain cannot come back to its first preset. | `config::preset-loop` |
| Two decks cannot have the same full name. | `config::duplicate-deck` |
| A content pattern must be valid. | `content::pattern` |
| A matched content file must be YAML, TOML or JSON. | `content::format` |
| One content file can belong to one deck only. | `content::two-decks` |

### Presets (found only by plan and build)

| Rule | Code |
|------|------|
| `maximum_interval` can be at most 36500 days. | `config::preset` |
| `fsrs_params` must have 17, 19 or 21 numbers. | `config::preset` |

### Note types

| Rule | Code |
|------|------|
| A note type file must exist. | `config::missing-notetype` |
| Only one note type can have `default = true`. | `config::duplicate-default` |
| An entry type (a key or an alias) can belong to one note type only. | `config::duplicate-type` |
| A source must use a valid set of keys: `git` with one of `tag`, `rev` and `branch`, or `url` with `sha256`. A local note type needs `path`. | `config::source` |
| A `git` URL must start with `https://` or `git://`. A `url` must start with `https://` and end in `.tar.gz`, `.tgz` or `.zip`. | `config::source` |
| A remote `path` must stay inside the source. | `config::source` |
| A Git source must be reachable, or in the cache with `--offline`. | `source::git` |
| An archive must download, match its `sha256` and unpack. | `source::archive` |
| A `git://` source without a pinned commit gives a warning. | `source::insecure` |

A project also needs at least one note type. Without a `[notetypes]` table, janky gives the
unclear error `content::schema` ("cannot compile the content schema").

### Plugin instances

| Rule | Code |
|------|------|
| The script must exist. | `config::missing-file` |
| The script must be inside the project. | `config::outside-project` |
| The script must be a regular file, not a directory. | `config::not-a-file` |
| A variable must be one that the plugin knows. | `variables::unknown-key` |
| A required variable must be there. | `variables::required` |
| A variable must have the correct type. | `variables::type` |
| Each secret that the plugin needs must be in `[plugins.<name>.secrets]`. | `plugin::table` |
| A secret that the plugin does not use gives a warning. | `plugin::unused-secret` |
| `http` or `secrets` on a plugin without a generator gives a warning. | `plugin::unused-access` |
| Two different generator scripts cannot have the same plugin id. | `plugin::id` |
| Each secret must have a value (checked only by `janky generate`). | `secret::missing` |

### Publish

| Rule | Code |
|------|------|
| `endpoint` must be a valid URL with `https://`, or `http://` on a loopback address. It cannot hold a user name or a password, and its path must end with `/`. | `config::endpoint` |
| The password must have a value (checked only by `janky publish` and `janky pull`). | `secret::missing` |

### deck.lock

| Rule | Code |
|------|------|
| `janky update` must be able to write `deck.lock` (no `deck.lock.tmp` left over). | `update::lock` |

[Diagnostic codes](../reference/codes.md) lists all the codes of janky.
