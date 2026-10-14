# Creating plugins

A plugin is a small script in the [Lua](https://www.lua.org) language. janky runs it while it
renders your field templates, or while `janky generate` runs. This part shows you how to write your
own plugins. It expects you to write some code, but it does not expect you to know Lua already.

There are two jobs that a plugin can do:

- **Filters and functions.** These are new words for your field templates, for example
  `article(entry.gender)`. They run during `check`, `plan` and `build`, and they cannot use the
  network or read files.
- **A generator.** It makes media files (for example audio) or data (for example dictionary
  values) during `janky generate`. Only a generator can use the network, secrets and files, and
  only when the deck author allows it.

[Using plugins](../plugins/index.md) explains how a deck author adds a plugin to a project. Read
it first if you have not used a plugin before.

## Lua in ten lines

If Lua is new to you, this example shows the parts that you need for most plugins. Text after `--`
is a comment.

```lua
local name = "casa"                 -- a local variable (always write `local`)
local count = 3                     -- numbers have no quotes
local word = { spanish = "casa" }   -- a table with named keys (a map)
local list = { "el", "la" }         -- a table with positions 1, 2, ... (a list)
print(word.spanish, list[1])        -- read a key, read position 1 (lists start at 1)
local text = "la " .. name          -- `..` joins two strings
local function shout(s)             -- a function with one argument
  if s == nil then return "" end    -- `nil` means "no value"
  return string.upper(s) .. "!"     -- `return` gives the result back
end
```

A missing key or a missing argument is `nil`. Only `nil` and `false` count as false in an `if`. The
empty string `""` and the number `0` count as true. The [Lua 5.4 reference
manual](https://www.lua.org/manual/5.4/) explains the full language.

## Pages in this part

- [Your first plugin: a filter](first-plugin.md): write a filter and a function, attach them to a
  note type, and use them in a field template.
- [Writing a generator](generators.md): make media or data during `janky generate`, with a full
  offline example and an HTTP example.
- [The janky API](api.md): every function in the `janky` table that janky gives to plugins.
- [The sandbox and its limits](sandbox.md): what Lua can and cannot do inside janky, the limits,
  and how to test a plugin.
