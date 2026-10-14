# Your first plugin: a filter

On this page you write the plugin `spanish.lua` from the tutorial project. It gives two new words
for field templates:

- The filter `article`, which puts `el` or `la` in front of a Spanish noun, in a colour.
- The function `gender_name`, which turns `m` into `masculine` and `f` into `feminine`.

A **filter** changes a value with the `|` sign: `entry.spanish | article(entry.gender)`. A
**function** is called by its name: `gender_name(entry.gender)`. [Filters, functions and
context](../notetypes/template-reference.md) lists the filters and functions that janky has
already.

## Step 1: write the script

1. In the note type directory `notetypes/word/`, make a file with the name `spanish.lua`.
2. Put this text in the file.

```lua
local articles = { m = "el", f = "la" }
local names = { m = "masculine", f = "feminine" }

local filters = {}

function filters.article(word, gender)
  local article = articles[gender]
  if article == nil then
    return word
  end
  return janky.html('<span class="article-' .. gender .. '">' .. article .. "</span> " .. janky.escape(word))
end

local functions = {}

function functions.gender_name(gender)
  return names[gender] or ""
end

return { id = "spanish", api = 1, filters = filters, functions = functions }
```

The last line is the important part. A plugin script must `return` a table, and janky reads the
plugin from that table. A filter or function plugin uses these keys:

| Key | Type | Required | Meaning |
|-----|------|----------|---------|
| `id` | text | yes | A name for the plugin. It must not be empty. |
| `api` | the whole number `1` | yes | The plugin API version. `2`, `"1"` and `1.0` are refused. |
| `filters` | table of name to function | no | The filters that the plugin gives. |
| `functions` | table of name to function | no | The functions that the plugin gives. |
| `variables` | table | no | The settings that the plugin accepts (see [Plugins with settings](#plugins-with-settings)). |

Any other key gives the error ``unknown key `x` in the returned table``. There is no `name` key.
Generators use more keys, which [Writing a generator](generators.md) explains.

The `janky` table (`janky.html`, `janky.escape` and the others) is always there. You do not load
it. [The janky API](api.md) lists all of it.

## Step 2: attach the plugin to the note type

1. Open `notetypes/word/notetype.toml`.
2. Add the `plugins` key near the top. The path is relative to the directory of `notetype.toml`.

```toml
name = "Spanish Word"
sort_field = "spanish"
css = "style.css"
assets = ["assets/reveal.js"]
plugins = ["spanish.lua"]
```

A plugin that a note type lists is a **note type plugin**. Every field template of that note type
can use its filters and functions.

## Step 3: use the filter and the function

Use them in field templates, in the same note type file:

```toml
[fields.with_article]
name = "With Article"
value = "{{ entry.spanish | nfc | article(entry.gender) }}"

[fields.gender]
name = "Gender"
value = "{{ gender_name(entry.gender) }}"
```

A filter gets the value before the `|` as its first argument. The values in the brackets come after
it. So `entry.spanish | article(entry.gender)` calls `article(entry.spanish, entry.gender)` in Lua.
A function gets only the values in its brackets.

Run `janky check`. If there is no error, run `janky build`. For the entry `casa` with
`gender: 'f'`, the package has these field values:

```text
  With Article: '<span class="article-f">la</span> casa'
  Gender: 'feminine'
```

The CSS of the note type gives the class `article-f` its colour.

## Text and trusted HTML

Anki shows field values as HTML (the markup language of web pages, where `<b>word</b>` shows a bold
word). Text from a plugin can hold characters such as `<` and `&`, which HTML reads as markup. So
janky **escapes** every plain string that a plugin returns. It changes `<` to `&lt;`, `&` to
`&amp;`, and so on, and Anki then shows the characters as text.

If your plugin makes HTML on purpose, mark the result with `janky.html(text)`. janky then puts the
text into the field without changes. Only mark text that you control. Escape every part that comes
from an entry with `janky.escape(text)`, as `article` does with `word`.

For example, with a filter `wrap` that returns
`janky.html("<" .. tag .. ">" .. janky.escape(text) .. "</" .. tag .. ">")`, the template
`{{ entry.english | wrap('b') }}` turns `but & <however>` into
`<b>but &amp; &lt;however&gt;</b>`.

You cannot join a marked value to other text with `..`. That gives the error "marked HTML cannot
be joined with `..`: escape the text parts with janky.escape, join them, and mark the whole result
once with janky.html". So build the full string first, and call `janky.html` once at the end:

```lua
return janky.html(janky.escape(a) .. "<b>" .. janky.escape(b) .. "</b>")
```

> [!WARNING]
> HTML from another filter (for example the output of `markdown`) reaches your plugin as plain
> text. Your plugin cannot tell it from normal text. If you escape it, the HTML tags show on the
> card as text.

## What a filter receives and returns

janky changes template values to Lua values when it calls your plugin:

| Template value | Lua value |
|----------------|-----------|
| text | string |
| whole number, decimal number | number |
| `true`, `false` | boolean |
| a missing key, `none` | `nil` |
| a list | a table with positions 1, 2, ... |
| a map | a table with text keys |
| a media value, for example `media('a.png')` | an empty table |

A media value arrives empty, so your plugin cannot read its file name. Pass the file name instead,
for example `media(entry.image).file`.

janky also changes the value that your plugin returns:

| Lua value | In the field |
|-----------|--------------|
| a string | escaped text |
| `janky.html(s)` | `s`, without changes (also inside a returned table) |
| a number or boolean | the value |
| a table with keys 1 to n only | a list |
| an empty table `{}` | an empty list |
| another table | a map (the keys must be text or whole numbers) |
| `nil` | the text `None` |

> [!TIP]
> If your filter has nothing to show, return `""`. If it returns `nil`, the field shows the word
> `None`.

janky uses only the first value that a function returns. `return "a", "b"` gives `a`.

Templates cannot give keyword arguments to a plugin. `x | f(1, size=3)` gives the error "too many
arguments: unexpected keyword arguments". Use positions only: `x | f(1, 3)`. A missing argument is
`nil` in Lua.

## Log messages

To see what your plugin does, call `janky.log(...)` (or `print(...)`, which is the same). janky
joins the arguments with a tab and prints each message after the run, with the prefix `plugin: `.
With `janky.log("gender_name got", gender)` in `gender_name`, `janky check` shows:

```console
$ janky check
...
plugin: gender_name got	f
plugin: gender_name got	m
plugin: gender_name got	m
...
9 notes are valid
```

## When the plugin fails

If your Lua code calls `error(...)` or fails, janky shows two diagnostics. The first points at the
line in the script. The second points at the field template that called it. The help line names the
first entry that failed. The field gets an empty value.

```console
$ janky check
plugin::runtime

  x no article for gato
   ,-[notetypes/word/spanish.lua:8:3]
 7 |   local article = articles[gender]
 8 |   if word == "gato" then error("no article for " .. word, 0) end
   :   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^|^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   :                                  `-- here
 9 |   if article == nil then
   `----
  help: first seen in entry `gato` in `content/vocab/01-basics.yaml`

Error: plugin::call

  x the plugin was called here
    ,-[notetypes/word/notetype.toml:22:35]
 21 | name = "With Article"
 22 | value = "{{ entry.spanish | nfc | article(entry.gender) }}"
    :                                   ^^^^^^^^^^|^^^^^^^^^^
    :                                             `-- here
 23 |
    `----
...
```

A syntax error in the script gives `plugin::syntax`, and an error while the script loads gives
`plugin::runtime`. Both point at the line. A script that does not return a table gives
`plugin::table`.

## Names of filters and functions

A filter or function name must start with a letter or `_`, and have only letters, digits and `_`.

All filter and function names share one list across janky and every plugin of the project. Two
plugins cannot give the same name. The second one gives the error "plugin `p` already supplies
`up`". For the same reason, a script with filters normally has only one instance in a project.

These names belong to janky, and a plugin cannot use them: `markdown`, `json`, `nfc`, `sound`,
`img`, `media` and `generate`. If you try, `janky check` shows:

```console
$ janky check
plugin::table

  x notetypes/word/spanish.lua: `markdown` is a built-in filter or function of janky
   ,-[notetypes/word/notetype.toml:5:12]
 4 | assets = ["assets/reveal.js"]
 5 | plugins = ["spanish.lua"]
   :            ^^^^^^|^^^^^^
   :                  `-- here
 6 |
   `----
```

> [!WARNING]
> Do not call a filter or function `enrich`. janky does not refuse the name, but its own `enrich`
> function replaces yours, and calls to it then fail.

## Keep filters predictable

janky runs the whole script once when it loads the plugin. Local variables at the top of the script
(such as `articles`) keep their values between calls. Do not use them to count or remember calls.
A filter must give the same result for the same arguments, because `plan` compares each build with
the last one. [The sandbox and its limits](sandbox.md) explains the other rules.

## Plugins with settings

A note type plugin gets no settings. Its `janky.variables` is an empty table, and a plugin with a
`required` variable cannot be a note type plugin.

To give a filter plugin settings, add it to `deck.toml` as a **plugin instance** instead, and list
the values under `variables`:

```lua
local filters = {}
function filters.shout(text)
  return string.upper(text) .. janky.variables.ending
end
return { id = "text", api = 1, filters = filters,
  variables = { ending = { type = "string", required = true } } }
```

```toml
[plugins.text]
path = "plugins/text.lua"
variables = { ending = "!" }
```

The `variables` key of the script says which settings the plugin accepts. [Writing a
generator](generators.md#declare-variables) explains its format. [Plugin instances and
secrets](../plugins/instances.md) explains the `deck.toml` side.
