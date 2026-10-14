# The janky API

janky gives every plugin a global table with the name `janky`. This page lists all of it. You do
not load the table. It is there when your script starts.

## When each function is available

| Function | Filters and functions | `plan` | `generate` and `prepare` |
|----------|-----------------------|--------|--------------------------|
| `janky.html`, `janky.escape` | yes | yes | yes |
| `janky.log` (and `print`) | yes | yes | yes |
| `janky.json.encode`, `janky.json.decode` | yes | yes | yes |
| `janky.variables` | yes | yes | yes |
| `janky.http.request`, `janky.http.download` | no | no | yes |
| `janky.secret` | no | no | yes |
| `janky.wait` | no | no | yes |
| `janky.cache.get`, `janky.cache.put` | no | no | yes |
| `janky.fs.read`, `list`, `zip_entries`, `json_items` | no | no | yes |

The functions in the last five rows exist only while `janky generate` runs, and only inside
`generate` and `prepare`. In filters, functions and `plan`, `janky.http`, `janky.secret`,
`janky.wait`, `janky.cache` and `janky.fs` are `nil`. If you keep one of these functions in a
variable and call it later outside a job, it gives the error "... works only inside `generate` or
`prepare`".

There is no `janky.http.get` or `janky.http.post`, no `janky.xml` and no way to run a program.

## janky.html(text)

Marks `text` as trusted HTML. janky puts it into the field without escaping, also when it is inside
a returned table. `tostring` of the result gives `text` back.

You cannot join a marked value with `..`, also not to another marked value. Join plain strings
first, and call `janky.html` once on the whole result. [Your first plugin](first-plugin.md#text-and-trusted-html)
shows the pattern.

## janky.escape(text)

Returns `text` with the HTML characters escaped:

| Character | Becomes |
|-----------|---------|
| `&` | `&amp;` |
| `<` | `&lt;` |
| `>` | `&gt;` |
| `"` | `&quot;` |
| `'` | `&#39;` |

Bytes that are not valid UTF-8 become the replacement character U+FFFD. The result is at most
16 MiB.

## janky.log(...)

Writes a message to the plugin log. `print` is the same function. janky joins the arguments with a
tab. After the command, it prints each message on the error output with the prefix `plugin: `.
janky replaces the value of every secret in the log with `<secret:name>`. The log keeps the last
1 MiB of messages.

Calls at the top of the script also go to the log, because janky runs the script when it loads the
plugin.

## janky.json.encode(value) and janky.json.decode(text)

`encode` turns a Lua value into JSON text:

- The keys of an object are sorted.
- An empty table becomes `[]`.
- A table with keys 1 to n becomes a list. Other tables become objects.
- A `janky.html` value becomes its string.
- NaN and infinity are errors ("not a finite number").
- A table that contains itself, a function or a secret handle is an error.
- The result is at most 16 MiB.

For example, a table with the keys `zeta`, `alpha`, `mid`, `f`, `h` and `i` encodes as
`{"alpha":[3,2],"f":0.5,"h":"<b>","i":3,"mid":[],"zeta":1}`.

`decode` turns JSON text into a Lua value:

- JSON `null` becomes `nil`. So `[1,null,3]` becomes a table with a gap, and `#t` can give `1`.
- `1.0` becomes a decimal number. Whole numbers too large for a 64-bit integer become decimal
  numbers.
- Bad JSON gives the error "bad JSON: ...".

## janky.variables

A table with the instance variables from `[plugins.<name>.variables]` in `deck.toml`. For a note
type plugin, it is an empty table.

janky gives a fresh copy before every call. If you change it, the change is lost after the call.
`plan`, `generate` and `prepare` also get the same values as their `vars` argument.

## janky.http.request(spec)

Sends one HTTP request and returns the response. The deck author must give the instance an
`[plugins.<name>.http]` table. Without it, the call fails with "has no network access".

| Key of `spec` | Type | Default | Meaning |
|---------------|------|---------|---------|
| `method` | text | `"GET"` | `GET`, `POST`, `PUT`, `PATCH`, `DELETE` or `HEAD`, in upper or lower case. |
| `url` | text | (required) | The full address of the request. |
| `headers` | table of name to text or secret handle | no headers | At most 64 headers. |
| `body` | text | empty | The bytes to send. |

Any other key gives the error "a request has only `method`, `url`, `headers` and `body`". There is
no option for a timeout or for JSON. Use `janky.json.encode` for the body.

The response is a table:

| Key | Meaning |
|-----|---------|
| `status` | The HTTP status, as a whole number. |
| `headers` | A table of header names in lower case. Repeated headers are joined with `, `. |
| `body` | The response body, as a string. |

### Rules for the URL

- The URL must start with `https://`. Plain `http://` works only for the local computer
  (`localhost` and `127.x.x.x`).
- The host must match an item in `allowed_hosts`. `*.example.com` matches the subdomains of
  `example.com`, not `example.com` itself. The port is not checked.
- An IPv4 address must be in the list as written. IPv6 hosts are never allowed.
- A URL with a user name or password is refused.
- janky encodes characters outside ASCII in the path for you.

### Rules for headers

- Header names cannot hold a line break or `:`. Values cannot hold a line break.
- You cannot set these headers (in any case): `host`, `accept-encoding`, `transfer-encoding`,
  `content-length`, `connection`, `te`, `upgrade`, `keep-alive`, `trailer` and any name that starts
  with `proxy-`.
- janky sends no `User-Agent` header unless you set one.

### What janky does with the response

- An error status (for example 404 or 500) is not a Lua error. Your plugin gets the response, and
  must check `status`.
- janky does not follow redirects. A 302 response comes back to your plugin with
  `headers.location`.
- janky does not accept compressed responses. A `Content-Encoding` other than `identity` gives the
  error "the server sent a compressed response, which janky does not accept".
- A response can hold the value of a secret, in the body, a header name or a header value (also in
  JSON-escaped form). Then janky gives the error "the server sent the secret `api_key` back, so janky does
  not give the response to the plugin". The job fails.
- A body larger than 32 MiB is an error.

### Retries and time limits

janky repeats some requests for you. The deck author sets the number of attempts and the delays in
`[plugins.<name>.http]`. The defaults are 4 attempts, a base delay of 1000 ms and a maximum delay of
30000 ms.

- janky repeats a request after a 429 status or a 5xx status (except 501), for every method.
- If the server sends `Retry-After` in seconds, janky waits that long, up to the maximum delay.
- janky repeats a request after a network error for every method except `POST` and `PATCH`. It
  repeats a `POST` or `PATCH` only when the connection did not open.
- After the last attempt, your plugin gets the last response.

Each attempt has a limit of 120 seconds, with 30 seconds to connect. A plugin cannot change these
limits.

`max_concurrency` (from 1 to 64, default 1) limits both the jobs of the instance that run at the
same time and the requests in progress. `rate_limit` spaces the requests evenly.

### The request limit for each job

A job can call `janky.http.request` and `janky.http.download` at most 5 times. `prepare` has the
same limit. The sixth call gives the error "a job may send at most 5 requests". Retries do not
count.

The HTTP errors in this section are Lua errors, so `pcall` can catch them.

## janky.http.download(spec)

Downloads a file into the file area of the instance. The body never goes into Lua, so you can
download files that are too large for `janky.http.request`.

`spec` has the same keys as `janky.http.request`, plus a required `file`:

| Key | Meaning |
|-----|---------|
| `file` | The file name: 1 to 100 of `A-Z`, `a-z`, `0-9`, `.`, `_` and `-`. It cannot start with `.`. |

The result is a table with `status`, `headers` and `size`. If the status is a 2xx status, janky
writes the body to `.janky/cache/plugins/<instance>/files/<file>`. It replaces an old file with the
same name. If the status is not 2xx, janky writes nothing, and `size` is `0`. A body that holds a
secret leaves no file.

Read the file later with `janky.fs.read("cache/<file>")` or `janky.fs.json_items`.

| Limit | Value |
|-------|-------|
| One download | 128 MiB |
| One attempt | 600 seconds |
| All downloads of one instance in one run | 512 MiB |

## Secret handles

`janky.secret(name)` and `janky.secret(name, prefix)` return a handle to a secret. The `name` must
be in the `secrets` list of the plugin. If it is not, you get the error "the plugin does not list
the secret ...".

`prefix` is text that janky puts in front of the value, for example
`janky.secret("api_key", "Bearer ")`. It is at most 64 bytes with no control characters.

The handle is not the value. You can only use it as a header value in `janky.http.request` or
`janky.http.download`. janky puts the real value in when it sends the request.

- `tostring(handle)` gives `<secret:api_key>`.
- `..` with a handle is an error.
- `janky.json.encode` of a table with a handle is an error.
- A handle as `data` in a media result is an error.

## janky.wait(ms)

Waits `ms` milliseconds (from 0 to 60000). Other jobs run while your job waits. The wait does not
count towards the per-call limits, but it counts towards the 600-second total of the job.

## janky.cache.get(key) and janky.cache.put(key, value)

A small store that stays between runs. Each instance has its own store in
`.janky/cache/plugins/<instance>/cache.sqlite`. Two instances of one script do not share it.

- `key` is text, 1 to 1024 bytes.
- `value` is any value that `janky.json.encode` accepts, at most 1 MiB as JSON.
- `put(key, nil)` deletes the key.
- `get` of a missing key gives `nil`.
- One instance can store at most 256 MiB.

Values go through JSON. So a whole decimal number such as `2.0` comes back as the integer `2`, and
an empty table comes back as an empty list.

In `generate`, each `put` is saved at once. In `prepare`, all reads and writes are one transaction.
janky saves them when `prepare` returns, and discards them when `prepare` fails.

## janky.fs: read files

A plugin can read files only in directories that the deck author gives it:

```toml
[plugins.frequency.filesystem]
read = { lists = "data" }
```

The key (`lists`) is a name that the plugin uses. The value is a directory, relative to `deck.toml`
or starting with `~/`. In the plugin, every path starts with the name: `lists/frequency.json` reads
`data/frequency.json`.

The name `cache` is reserved. `cache/<file>` reads the file area of the instance, where
`janky.http.download` writes. Every instance has this area, also without a `filesystem` table.

| Function | Result |
|----------|--------|
| `janky.fs.read(path)` | The whole file as a string, at most 16 MiB. A file that holds a secret is refused. |
| `janky.fs.list(path)` | A sorted list of the names in a directory, at most 100,000. Directory names end in `/`. |
| `janky.fs.zip_entries(path)` | The names of the files in a zip archive, in archive order. |
| `janky.fs.json_items(path, pointer)` | An iterator over the items of a JSON list (see below). |
| `janky.fs.json_items(path, pointer, { entry = name })` | The same, for the file `name` inside a zip archive. |

There is no function that writes a file. `janky.cache.put` and `janky.http.download` are the only
ways to keep data.

### Path rules

These paths give an error:

- An absolute path.
- A path with `..` or with an empty part.
- A name that the deck author did not give ("`other` is not a directory that this plugin may
  read").
- A path through a symbolic link (a link to another file). `list` shows links, but `read` cannot
  open them.
- A directory given to `read` ("is not a regular file").

### Stream a large JSON file

`json_items` reads one item at a time, so it can handle files larger than the 16 MiB limit of
`read`. It returns an iterator for a `for` loop:

```lua
for item in janky.fs.json_items("cache/x.json", "/list") do
  -- use item
end
```

`pointer` says where the list is. `""` means that the whole file is the list. `/list` means the key
`list` of the top object. A pointer can name object keys only. Write `~1` for a `/` and `~0` for a
`~` in a key.

| Limit | Value |
|-------|-------|
| Unpacked data for one stream | 2 GiB |
| One item | 1 MiB |
| Streams open at one time | 8 |
| Entries in a zip archive | 100,000 |

An error stops the stream with a Lua error. The per-call limits of the sandbox apply to each item,
not to the whole loop.
