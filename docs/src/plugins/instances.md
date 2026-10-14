# Plugin instances and secrets

A plugin instance is a `[plugins.<name>]` table in `deck.toml`. This page lists every key of that
table, and explains how to give a plugin a secret such as an API key.

`deck.toml` uses the TOML format. A line `[plugins.voice]` starts a table, and the lines below it
set its keys. A line `[plugins.voice.http]` starts a table inside `[plugins.voice]`. See
[Defining a deck](../deck/index.md) for an introduction to TOML.

## A complete example

This instance connects a text-to-speech plugin. It uses every kind of key:

```toml
[plugins.voice]
path = "plugins/elevenlabs.lua"

[plugins.voice.variables]
voice_id = "2Lb1en5ujrODDIqmp7F3"
model = "eleven_v4"
output_format = "opus_48000_64"

[plugins.voice.http]
allowed_hosts = ["api.elevenlabs.io"]
max_concurrency = 2
rate_limit = { count = 120, unit = "minute" }
retry = { max_attempts = 4, base_delay_ms = 1000, max_delay_ms = 30000 }

[plugins.voice.secrets]
api_key = { env = "ELEVENLABS_API_KEY" }
```

## Choose a safe instance name

The instance name is the word after `plugins.`, for example `voice`. Field templates use it to call
the generator: `media(entry.spanish, 'voice')`.

Use only lowercase letters, digits and `_` in an instance name, and start with a letter. For
example, use `my_dict`, not `my-dict`.

> [!WARNING]
> janky accepts a hyphen (`-`) in an instance name, but a plugin with a hyphen in its instance
> name cannot use its own cache or files. A generator that keeps a cache or files then fails on every job, with
> the message "`my-dict` is not a valid plugin instance name".

## Instance keys

| Key | Type | Default | Meaning |
|---|---|---|---|
| `path` | text | (required) | The Lua script, relative to `deck.toml`. It must be a file inside the project directory. |
| `variables` | table | empty | The settings of the plugin. The plugin says which variables it accepts. |
| `http` | table | no network | The hosts that the plugin can connect to, and the network limits. |
| `secrets` | table | no secrets | Where janky gets each secret that the plugin needs. |
| `filesystem` | table | own cache only | Directories that the plugin can read. |

### Variables

The plugin defines which variables it accepts, their types and which ones are required. janky
checks your `[plugins.<name>.variables]` table against this list, and shows the file, line and
column of each problem. For example:

| Code | Message |
|---|---|
| `variables::required` | missing key `model` in `plugins.voice.variables` |
| `variables::unknown-key` | unknown key `plugins.voice.variables.speed` |
| `variables::type` | `plugins.voice.variables.voice_id`: 5 is not of type "string" |

A plugin that defines no variables accepts no variables.

If your code editor supports TOML schemas, run `janky schema`. The schema file that it writes
includes the variables of each plugin instance, so the editor can complete and check them. See
[Command reference](../cli/commands.md).

### Network access (`http`)

Without an `[http]` table, the plugin cannot use the network. The plugin can use the network only
during `janky generate`. `check`, `plan` and `build` never use it.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `allowed_hosts` | list of host names | `[]` (no host) | The hosts that the plugin can connect to. |
| `max_concurrency` | whole number, 1 to 64 | 1 | How many requests (and jobs of this instance) can run at the same time. |
| `rate_limit` | `{ count, unit }` | no limit | At most `count` requests per `unit`. `unit` is `second`, `minute` or `hour`. |
| `retry.max_attempts` | whole number, 1 to 10 | 4 | The number of attempts for one request, including the first attempt. |
| `retry.base_delay_ms` | whole number, 10 to 60000 | 1000 | The wait before the first retry, in milliseconds. The wait doubles after each attempt. |
| `retry.max_delay_ms` | whole number, 10 to 60000 | 30000 | The longest wait between two attempts, in milliseconds. |

Rules for `allowed_hosts`:

- Write a host name only, for example `api.elevenlabs.io`. Do not add `https://` or a port number.
  Use lowercase letters.
- `*.example.com` allows every subdomain of `example.com`, but not `example.com` itself.
- `localhost` and IPv4 addresses such as `127.0.0.1` are also allowed. IPv6 addresses are not.
- janky uses HTTPS only. Plain `http://` works only for your own computer (`localhost` or
  `127.0.0.1`).
- janky does not follow redirects, and it does not use proxy settings from the environment.

The rate limit spaces the requests evenly. For example, `{ count = 120, unit = "minute" }` sends at
most one request every half second. A rate limit slower than one request every 60 seconds is an
error when `janky generate` starts.

janky retries a request after an HTTP 429 status ("too many requests") or a 5xx status (a server
error, except 501). It also retries after a network error. For a POST or PATCH request, janky
retries a network error only when the connection itself failed. If the server sends a
`Retry-After` time, janky waits that long (at most `max_delay_ms`).

> [!NOTE]
> A paid service can receive a request more than once when it answers with a server error.
> Check with your provider if it charges for such requests.

### File access (`filesystem`)

Each instance can always read its own file area in `.janky/cache/plugins/<name>/files/`. Only its
downloads write to this area.
To let a plugin read other files, add a `read` table:

```toml
[plugins.frequency.filesystem]
read = { lists = "data" }
```

Each entry gives a name to a directory. Here the name `lists` points to the `data/` directory of the
project. The plugin then reads `lists/frequency.json`, not `data/frequency.json`.

- The directory is relative to `deck.toml`. You can also start it with `~/` for your home
  directory.
- A name starts with a letter or `_`, and has only letters, digits and `_`. You cannot use the name
  `cache`, because janky keeps it for the instance's own file area.
- The access is read-only. The plugin cannot change the files.

### Warnings about unused access

janky warns when you grant access that the plugin cannot use:

- `plugin::unused-secret`: you set a secret that the plugin does not ask for.
- `plugin::unused-access`: you set `http` or `secrets` on a plugin that is not a generator.

If the plugin needs a secret that you did not set, janky gives an error:

```text
the plugin needs the secret `api_key`: add it to `[plugins.voice.secrets]`
```

## Secrets

A secret is a private value, for example the API key of a paid service. You never write the value
in `deck.toml`. Instead, you tell janky where to find it.

### Where janky looks for a secret

Each key in `[plugins.<name>.secrets]` is the name of one secret that the plugin asks for. Its value
is a source:

| Source | Where janky looks, in order |
|---|---|
| `{ env = "NAME" }` | 1. The environment variable `NAME`. 2. The line `NAME=...` in the `.env` file next to `deck.toml`. |
| `{ env = "NAME", file = "secrets.env" }` | 1. The environment variable `NAME`. 2. The line `NAME=...` in `secrets.env`. 3. The line `NAME=...` in `.env`. |
| `{ file = "~/.config/elevenlabs/key" }` | The whole content of that file. janky removes spaces and line breaks at the start and end. |

- A `file` path is relative to `deck.toml`. You can start it with `~/` for your home directory.
- If an environment variable is set but empty, janky goes on to the files. An empty value in a file
  also goes on to the next file.
- A secret value must have 8 to 8192 characters. It can contain letters, digits, punctuation and
  spaces, but no line breaks or other control characters.

### Set up a secret in a `.env` file

A `.env` file is a text file with one `NAME=value` line for each secret.

1. Add the source to `deck.toml`:

   ```toml
   [plugins.voice.secrets]
   api_key = { env = "ELEVENLABS_API_KEY" }
   ```

2. Make a file called `.env` in the project directory, next to `deck.toml`.
3. Add this line to `.env`, with your own key after the `=`:

   ```text
   ELEVENLABS_API_KEY=your-key-here
   ```

4. If you use Git, add `.env` to your `.gitignore` file. janky does not do this for you.
5. Run `janky generate`. The summary shows where janky looks for the secret.

The `.env` file can also have:

- comment lines that start with `#`
- `export ` before the name
- a value in single or double quotes
- a comment after an unquoted value, after a space and `#`

If a name is on more than one line, the last line wins.

### Set up a secret in an environment variable

An environment variable is a value that your terminal gives to the programs that it starts.

1. Add the source to `deck.toml`, as in step 1 above.
2. Set the variable for one command:

   ```console
   $ ELEVENLABS_API_KEY=your-key-here janky generate
   ```

An environment variable takes priority over the `.env` file.

### Set up a secret in a file of its own

Use this source to keep the key outside the project, for example in your home directory.

1. Save the key alone in a file, for example `~/.config/elevenlabs/key`.
2. Add the source to `deck.toml`:

   ```toml
   [plugins.voice.secrets]
   api_key = { file = "~/.config/elevenlabs/key" }
   ```

### When janky reads secrets

Only `janky generate` reads secrets. It reads them after it finds the pending jobs, and before it
shows the summary. If there are no pending jobs, it reads no secret. `check`, `plan` and `build`
never read secrets.

> [!IMPORTANT]
> `janky generate` needs the secrets of every generator instance in the project. This includes
> instances that have no pending jobs. For example, if only your dictionary lookups are pending,
> you still need the API key of your text-to-speech instance.

If janky cannot find a secret, it stops before it runs any job:

```text
secret::missing

  x the secret `key` of `say`: set the environment variable `SAY_KEY`, or add it to `secrets.env` or `.env`
    ,-[deck.toml:14:7]
 13 | [plugins.say.secrets]
 14 | key = { env = "SAY_KEY", file = "secrets.env" }
    :       ^^^^^^^^^^^^^^^^^^^^|^^^^^^^^^^^^^^^^^^^^
    :                           `-- here
    `----
```

### How janky protects secrets

- A plugin never gets the value of a secret. It can only put the secret in a request header, and
  only to a host in `allowed_hosts`.
- janky replaces the value of a secret with `<secret:name>` in everything that it prints, also in
  `--json` output.
- If a server sends the secret back in its answer, janky does not give the answer to the plugin.
  The job fails.
- janky does not store a result or a warning that contains a secret.
