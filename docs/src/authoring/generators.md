# Writing a generator

A **generator** is a plugin that makes a result for a field template during `janky generate`. There
are two kinds:

- A **media** generator makes a file, for example an audio clip of a word.
- A **data** generator makes a table of values, for example the rank of a word in a frequency list.

Field templates ask for results with `media(input, 'instance')` or `enrich(input, 'instance')`.
These calls only record a **job**. They never run the generator. `janky generate` runs the jobs and
keeps each result in `.janky/cache/`. [Generating media and data](../plugins/generating.md)
explains this from the deck author's side.

This page uses the tutorial plugin `plugins/frequency.lua` as its main example. It is a data
generator that reads a local JSON file, so it needs no account and no network.

## Where a generator lives

A generator must be a plugin instance in `deck.toml`. A note type cannot list a generator. If it
does, janky gives the error "a note type plugin cannot supply `generate`".

```toml
[plugins.frequency]
path = "plugins/frequency.lua"
[plugins.frequency.variables]
list = "frequency.json"
common_below = 1000
[plugins.frequency.filesystem]
read = { lists = "data" }
```

The instance name (`frequency` here) is the name that templates use.

> [!WARNING]
> Use only lowercase letters, digits and `_` in the name of a generator instance, for example `my_dict`. An
> instance with a `-` in its name loads, but every call to `janky.cache`, `janky.http.download` and
> `janky.fs` with `cache/` paths fails.

## The keys of a generator

A generator returns the same table as a filter plugin (`id`, `api`, and optional `filters`,
`functions` and `variables`), plus these keys:

| Key | Type | Required | Meaning |
|-----|------|----------|---------|
| `generate` | function | yes | Runs one job and returns its result. |
| `generates` | `"media"` or `"data"` | yes, with `generate` | The kind of result. Templates must use `media` for `"media"` and `enrich` for `"data"`. |
| `plan` | function | no | Describes a job before it runs: the file type, the cache key and the size. |
| `prepare` | function | no | Runs once before the jobs of the instance, in each `janky generate`. |
| `secrets` | list of names | no | The secrets that the plugin needs, for example `{ "api_key" }`. |

`generates`, `plan` and `prepare` are allowed only together with `generate`.

## The plugin id names the cached results

janky makes the name of each cached result from the plugin `id`, the cache key (from `plan`) and
the job input. The instance name is not part of it.

- If you change the `id`, janky cannot find the old results. Every job runs again. For a paid
  service, you pay again.
- Two different scripts with a `generate` function cannot have the same `id`. janky gives the
  error `plugin::id`.
- Two instances of the same script share their results when the cache keys are equal.

Choose the `id` once, and do not change it.

## The generate function

janky calls `generate(job, vars)` once for each job.

- `job.input` is the value that the template gave: a string, or a table if the template gave a map.
- `job.kind` is `"media"` or `"data"`.
- `vars` is a copy of the instance variables from `deck.toml`.

The job has no other fields. It does not tell you the instance name or the entry.

Some facts about the input:

- janky changes text input to Unicode NFC form (one standard way to store accented letters).
- In a map, keys with no value are left out. So `{'word': entry.spanish, 'pos': entry.pos}` gives a
  table without `pos` when the entry has no `pos`.
- A whole decimal number such as `2.0` arrives as `2`.
- An empty input, or input with only spaces, records no job. A `media` call then gives an empty
  media value.
- The same kind, instance and input make one job, even if many fields ask for it.

### What generate returns

| Result | For | Meaning |
|--------|-----|---------|
| `{ data = bytes, ext = "ogg", cost = n }` | media | The file content as a string, and its file extension. `cost` is optional. |
| `{ value = table, warning = "text", info = "text", cost = n }` | data | A table with named keys. `warning`, `info` and `cost` are optional. |
| `nil, "message"` | both | A refusal: the service said no. `return nil` alone gives the message "no reason given". |
| a Lua error | both | A failure, shown with the script line. |

Rules for each result:

- A media result must not have `value`. If `plan` gave an `ext`, the result must have the same
  `ext`.
- A data result must not have `data` or `ext`. The `value` must be a table with names, not a list.
  An empty table `{}` is allowed. The value can be at most 1 MiB as JSON.
- A `warning` or an `info` is at most 2000 bytes, with no control characters except a line break.
- A value, warning or info that contains a secret is an error, and janky does not keep it.
- `cost` is a number of 0 or more. janky adds the costs of each instance and shows the total after
  the run, for example `Reported cost for dict: 4`. The unit is whatever the service counts.

### Not found and refused are different

A dictionary does not have every word. When a lookup finds nothing, or finds more than one answer,
return an empty value and a warning:

```lua
return { value = {}, warning = "not in the frequency list" }
```

janky keeps this result in the cache. It does not ask again in the next run. It shows the warning
as `enrich::warning` at the field template, on every `check`, `plan` and `build`.

Use `info` instead of `warning` for a result that the author expects often and cannot change, for
example a word that a small dictionary does not have:

```lua
return { value = {}, info = word .. ": not in the dictionary" }
```

janky shows an info as `enrich::info`. Without `--verbose`, the terminal shows all the infos as one
line with a count:

```text
info: 6 × enrich::info (run with --verbose to see them)
```

Keep `warning` for a result that the author can correct, for example a word with two meanings
that needs `pos`.

A refusal (`return nil, "message"`) is different. It means that the job could not get an answer
now, for example because the service gave an error status. janky does not keep a refusal.
`janky generate` shows `generate::refused`, exits with status 1, and runs the job again next time.

Use this rule:

- The answer is "this does not exist": return a value with a warning.
- The service did not give an answer: refuse.

> [!TIP]
> janky puts the instance name and the input in front of a warning, for example
> `frequency: aguacate: not in the frequency list`. Do not repeat the input in your warning text.

> [!NOTE]
> A cached warning does not change when you edit the warning text in the script. To see the new
> text, delete the cached result or change its cache key.

A job can end refused or failed. If its last response was a 401 or 403 to a request with a secret,
janky reports `generate::unauthorised`. janky then starts no more jobs in that run. Jobs that
already run will finish.

## The plan function

janky calls `plan(job, vars)` for each new job during every `check`, `plan`, `build` and `generate`.
It runs before anything is sent. It cannot use the network, files, secrets or the cache. It returns
a table with any of these keys:

| Key | Type | Meaning |
|-----|------|---------|
| `ext` | text, 1 to 10 of `a-z` and `0-9` | The file extension of a media result. It cannot be `json`. |
| `key` | any JSON value, at most 64 KiB | The cache key: the settings that change the result. |
| `units` | whole number, 0 or more | The size of the job, for the summary before a run. |
| `unit` | text, 1 to 32 bytes | The name of the unit, for example `"characters"`. |

An empty table `{}` is allowed. Any other key (for example `cost`) is an error. An error in `plan`
is an error in `check` and `build`, so a deck author sees it at once.

### What the key means

The key and the input together decide if janky has a result already. If the key changes, every job
of the instance is new, and it runs again.

Put in the key every setting that changes the result, and nothing else. For a paid text-to-speech
service, the voice and the audio format change the result. A setting for the number of retries does
not.

If there is no `plan`, or `plan` gives no `key`, the key is **all** the instance variables. Then a
change to any variable makes every job run again.

> [!IMPORTANT]
> A change to the shape of the key also makes every job new. For a paid service, every result costs
> money again. Decide the key before the deck author runs `janky generate` for real.

The key cannot see files. In the tutorial, a change to `data/frequency.json` does not make new jobs,
because the key holds only `list` and `common_below`. To make a way to refresh, put a version number
in the key (for example `v = 1`). Change the number when the source data changes.

### Other effects of plan

- With `ext`, janky knows the file name of a media result before the run. The field template can
  show the name (for example `[sound:<name>.ogg]`) while the file is still missing.
- Without `ext`, the file name is empty until the result exists, and `sound` shows nothing.
- `units` and `unit` show in the summary: `Generator jobs: 6 to run (6 lookups)`.

### Plan must be pure

`plan` must give the same result for the same job and variables, every time. janky does not check
this. If `plan` gives a different key on the next run, janky cannot find the cached result, and the
job runs again.

## The prepare function

`prepare(vars)` is optional. janky calls it once for each instance in each `janky generate`, before
the jobs of that instance. It runs only when the instance has jobs to run. janky prints
`` frequency: running `prepare` `` (with the instance name) when it starts.

Use `prepare` for work that all jobs need, for example to download a large file once and make an
index of it in `janky.cache`. `prepare` must decide for itself whether that work is already done.

- It has the same `janky` functions and the same limits as one job.
- janky ignores its return value.
- All `janky.cache` reads and writes in `prepare` are one transaction. They are kept when `prepare`
  returns, and they are discarded when it fails.
- If `prepare` fails, every job of that instance fails with "`frequency`: `prepare` failed:
  message", and janky sends nothing for those jobs.

## Declare variables

The `variables` key says which settings the deck author can give in
`[plugins.<name>.variables]`. It uses the same short form as the `[schema]` of a note type (see [The
content schema](../notetypes/schema.md)). Each key names a setting and gives its rules:

| Rule | Meaning |
|------|---------|
| `type` | Required. One of `string`, `slug`, `markdown`, `int`, `float`, `bool`, `enum`, `media`, `list`, `object`. |
| `required` | `true` if the deck author must give the setting. The default is `false`. |
| `values` | The allowed values of an `enum` (a list of strings). |
| `of` | The rules for each item of a `list`. |
| `fields` | The rules for the keys of an `object`. |
| `schema` | The name of a definition under `defs`, for an `object` (instead of `fields`). |

```lua
variables = {
  list = { type = "string", required = true },
  common_below = { type = "int", required = true },
}
```

There is no `default` rule. To have a default value, use `vars.name or "default"` in your code.

janky checks the deck author's values against these rules, and shows each error at its line in
`deck.toml`. A plugin without `variables` accepts no settings. `janky schema` adds the variables of
each plugin to the editor schema of `deck.toml`.

## Declare secrets

A **secret** is a value such as an API key. The deck author keeps it outside `deck.toml` (see
[Plugin instances and secrets](../plugins/instances.md)). List the names that your plugin needs:

```lua
secrets = { "api_key" }
```

Each name follows the filter name rules. A plugin can list at most 32 names. If the deck author does
not give a listed secret, janky gives the error "the plugin needs the secret `api_key`: add it to
`[plugins.p.secrets]`".

Your plugin never sees the value of a secret. `janky.secret("api_key")` gives a handle that you can
put only in a request header. [The janky API](api.md#secret-handles) explains it.

## A full example: an offline data generator

This is the tutorial generator. It reads a list of word ranks from `data/frequency.json`, and
gives the rank and a band (`common` or `rare`) for each word.

```json
{
  "casa": 150,
  "perro": 820,
  "gato": 1400,
  "pan": 640,
  "manzana": 2100
}
```

```lua
local function plan(job, vars)
  return { key = { list = vars.list, common_below = vars.common_below }, units = 1, unit = "lookups" }
end

local function generate(job, vars)
  local word = job.input
  local ranks = janky.json.decode(janky.fs.read("lists/" .. vars.list))
  local rank = ranks[word]
  if rank == nil then
    return { value = {}, warning = "not in the frequency list" }
  end
  local band = "rare"
  if rank < vars.common_below then
    band = "common"
  end
  return { value = { rank = rank, band = band } }
end

return {
  id = "frequency",
  api = 1,
  generates = "data",
  variables = {
    list = { type = "string", required = true },
    common_below = { type = "int", required = true },
  },
  plan = plan,
  generate = generate,
}
```

The `deck.toml` instance is in [Where a generator lives](#where-a-generator-lives). Its
`filesystem` table gives the plugin the name `lists` for the project directory `data`. So the script
reads `lists/frequency.json`, not `data/frequency.json`.

The field template asks for the result, and shows it only when there is a rank:

```toml
[fields.frequency]
name = "Frequency"
value = "{% set f = enrich(entry.spanish | nfc, 'frequency') %}{% if f.rank %}#{{ f.rank }} ({{ f.band }}){% endif %}"
```

Before the jobs run, `enrich` gives an empty map, so the `if` stops a lone `#` on the card. Run the
jobs:

```console
$ janky generate --yes
Generator jobs: 6 to run (6 lookups)
+ enrich frequency: casa
+ enrich frequency: perro
...
frequency: 6 jobs
  network: none
[1/6] frequency: casa: wrote f25d6251e5acc09b93bfcafc6cc2ef00.json
[2/6] frequency: perro: wrote 70d27e3d3306c86f65cd945776eb7092.json
...
[6/6] frequency: aguacate: wrote 70fb3d4384dc7e2bfb6c2507c0332fbf.json
Generated 6 results: 0 refused, 0 failed, 0 not started
```

The word `aguacate` is not in the list. Its result is cached with a warning, and the next
`janky check` shows it:

```console
$ janky check
enrich::warning

  ! frequency: aguacate: not in the frequency list
    ,-[notetypes/word/notetype.toml:45:9]
...
  help: first seen in entry `aguacate` in `content/vocab/02-food.yaml`
9 notes are valid
```

The field of `casa` in the package is `#150 (common)`.

## A sketch: an HTTP media generator

This sketch shows the shape of a text-to-speech generator. It is modelled on the bundled ElevenLabs
plugin (see [The bundled plugin](../plugins/bundled.md)). The service `tts.example.com` and its
request format are made up. Change them to match the documentation of your service.

```lua
local function plan(job, vars)
  return {
    ext = "mp3",
    key = { voice = vars.voice, v = 1 },
    units = utf8.len(job.input),
    unit = "characters",
  }
end

local function generate(job, vars)
  local response = janky.http.request {
    method = "POST",
    url = "https://tts.example.com/v1/speech",
    headers = {
      authorization = janky.secret("api_key", "Bearer "),
      ["content-type"] = "application/json",
      accept = "audio/mpeg",
    },
    body = janky.json.encode { text = job.input, voice = vars.voice },
  }
  if response.status ~= 200 then
    return nil, "HTTP " .. response.status .. ": " .. response.body:sub(1, 200)
  end
  return { data = response.body, ext = "mp3", cost = tonumber(response.headers["x-cost"]) }
end

return {
  id = "tts",
  api = 1,
  generates = "media",
  secrets = { "api_key" },
  variables = { voice = { type = "string", required = true } },
  plan = plan,
  generate = generate,
}
```

The deck author gives the instance network access to one host, and a source for the secret:

```toml
[plugins.speech]
path = "plugins/tts.lua"
[plugins.speech.variables]
voice = "lucia"
[plugins.speech.http]
allowed_hosts = ["tts.example.com"]
[plugins.speech.secrets]
api_key = { env = "TTS_API_KEY" }
```

A field template then plays the audio:

```jinja
{{ entry.spanish | nfc | media('speech') | sound }}
```

Some points about this sketch:

- `plan` gives `ext`, so the file name is known before the run. The `ext` in the result must be the
  same.
- The key holds the voice and a version number. A new voice makes new clips. The text is the input,
  so janky adds it to the cache name for you.
- An error status is a refusal, so janky does not keep it and tries again in the next run. A 401 or
  403 stops the run, because the request used a secret.
- janky does not treat an error status as a Lua error. Your code must check `response.status`.
- `cost` comes from a response header, if the service sends one.

## Limits for each job

Each job (and each `prepare`) can send at most 5 requests. It can run at most 200,000,000 Lua
instructions, and take at most 600 seconds, in total. [The sandbox and its limits](sandbox.md) lists the
limits for each call.
