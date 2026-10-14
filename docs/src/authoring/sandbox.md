# The sandbox and its limits

janky runs plugins in Lua 5.4 inside a **sandbox**: a closed space with a small set of functions and
fixed limits. A plugin cannot read your files, use the network or run programs on its own. It can do these
things only through the `janky` table, and only as far as the deck author allows.

## What a script can use

These globals are present:

```text
_G  _VERSION  assert  error  getmetatable  ipairs  janky  math  next  pairs  pcall  print
rawequal  rawget  rawlen  rawset  select  setmetatable  string  table  tonumber  tostring
type  utf8  warn  xpcall
```

These standard Lua globals are **not** present:

```text
io  os  debug  require  coroutine  load  loadfile  dofile  collectgarbage
```

Some more rules:

- `math.random` and `math.randomseed` are removed. The other `math` functions are there.
- There is no `os`, so a plugin cannot read the clock or the environment.
- There is no `require`. Put all the code of a plugin in one file.
- `string.dump` exists, but janky loads Lua as text only and refuses compiled Lua (bytecode).
- `warn` exists, but its output goes nowhere. Use `janky.log` or `print`.
- `print` is the same function as `janky.log`.

## Plugins cannot change each other

Each script gets its own copy of the globals. One plugin cannot replace `string.upper`,
`janky.json` or any other function for another plugin.

- The string metatable is locked. `getmetatable("")` gives `false`.
- `setmetatable` refuses a metatable with a `__gc` method. It gives the error "plugins cannot use
  __gc metamethods".

## Limits

Each call into a plugin has limits. A call is one filter, one function, one `plan`, one `prepare` or
one `generate`. The first run of the script, when janky loads it, is also a call.

| Limit | Value | What happens |
|-------|-------|--------------|
| Instructions in one call | 10,000,000 | Error: "the plugin ran more than 10000000 instructions in one call". `pcall` cannot catch it. |
| Memory | 64 MiB for all plugins together | Error: "memory error: not enough memory". |
| Time in one call | 10 seconds | janky stops at once, with exit status 1. |

Generator jobs have more limits. They apply to each job and to each `prepare`:

| Limit | Value |
|-------|-------|
| Requests (`janky.http.request` and `janky.http.download`) | 5 |
| Instructions in total | 200,000,000 |
| Time in total, with waits | 600 seconds |

In a job, the per-call limits apply to each part of the job between two pauses. A job pauses when it
calls `janky.wait`, sends a request, or gets the next item from `janky.fs.json_items`. So a long job
that pauses often does not reach the 10-second limit.

Values that pass between Lua and janky also have limits. One value can hold at most 1,000,000
values, 16 MiB of text and 128 levels of nesting. A table that contains itself is an error.

### The 10-second limit stops janky

The time limit protects you from a plugin that never ends, for example a slow string pattern. When
it fires, janky stops the whole command, not only the plugin, and exits with status 1. For a
filter `spin` in the instance `p`, the message is:

```text
error: the plugin call p: filter `spin` ran longer than 10 seconds
```

Find the slow code, and run the command again.

## Errors inside pcall

`pcall` and `xpcall` catch Lua errors as usual. An error from a `janky` function (for example a
refused URL) arrives as a special value, not as a string. Use `tostring(e)` to read it. The text
starts with `runtime error:` and ends with a stack traceback. An error that you raise with
`error("text", 0)` stays a plain string.

## Make results repeatable

`check`, `plan` and `build` must give the same result each time you run them on the same files.
Your plugin must help:

- The order of `pairs` changes from run to run, also for the same table. If the order matters, collect the keys, sort them with `table.sort`,
  and loop over the sorted list. janky sorts keys itself when it turns a table into JSON or a
  template value.
- Local and global variables of a script keep their values between calls. Do not let a result depend
  on earlier calls.
- `janky.variables` is a fresh copy for each call. A change to it is lost.
- `plan` must give the same key for the same job and variables. If not, janky cannot find the
  cached result, and the job runs again (see [Writing a generator](generators.md#plan-must-be-pure)).

## Test a plugin

Each command runs a different part of a plugin:

| Command | Runs |
|---------|------|
| `janky check` | the script load, every filter and function, and `plan` for each new job |
| `janky plan` | the same, and lists the generator jobs with their units |
| `janky build` | the same, and writes the package |
| `janky generate` | the same as `janky check`, then `prepare` and `generate` |

Some tips:

1. Use `janky check` while you write a filter. It shows errors at the script line and at the field
   template.
2. Add `janky.log(...)` calls to see values. janky prints them after the command, with the prefix
   `plugin: `.
3. Use `janky plan` to see the jobs that a generator gets, before you run anything.
4. Test an HTTP generator against a small server on your own computer. Plain `http://` is allowed
   for `localhost` and `127.0.0.1`. Set `allowed_hosts = ["127.0.0.1"]` in the instance.
5. If no terminal is attached, run `janky generate --yes`. Without `--yes`, janky stops with
   `generate::confirm`.
6. To run one data job again, delete its file in `.janky/cache/data/`. For a media job, delete
   both the media file and its `.json` file in `.janky/cache/media/`. The file name is in the
   `wrote ...` line of `janky generate`.

> [!WARNING]
> The `--offline` option does not stop `janky generate` from using the network. It affects only
> remote note types. To test with no network at all, remove the `[plugins.<name>.http]` table from
> your test copy of `deck.toml`.

> [!WARNING]
> Never test a new generator against a paid service with real content. A bug in `plan` or in the
> key can make every job run again, and each job costs money.
