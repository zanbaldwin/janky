# Kinds of plugin

You can connect a plugin to your project in two places. Where you connect it decides what the
plugin can do.

| | Note type plugin | Plugin instance |
|---|---|---|
| Where you connect it | `plugins = [...]` in `notetype.toml` | A `[plugins.<name>]` table in `deck.toml` |
| What it can give | Filters and functions | Filters, functions and a generator |
| Settings (variables) | None | `[plugins.<name>.variables]` |
| Network, secrets, files | Never | Only what you grant |
| Example in the tutorial | `spanish.lua` (the `article` filter) | `frequency` (a data generator) |

## Note type plugins

A note type plugin gives filters and functions to the field templates of its note type. You connect
it with the `plugins` key in `notetype.toml`. The path is relative to the directory of that
`notetype.toml` file.

The tutorial note type `notetypes/word/notetype.toml` uses the plugin `spanish.lua`:

```toml
name = "Spanish Word"
plugins = ["spanish.lua"]

[fields.with_article]
name = "With Article"
value = "{{ entry.spanish | nfc | article(entry.gender) }}"

[fields.gender]
name = "Gender"
value = "{{ gender_name(entry.gender) }}"
```

`spanish.lua` gives the filter `article` and the function `gender_name`. The field template uses
them in the same way as janky's own filters and functions (see
[Filters, functions and context](../notetypes/template-reference.md)).

A note type plugin has no settings, no network, no secrets and no file access. It cannot be a
generator. If you connect a generator in `notetype.toml`, janky gives this error:

```text
a note type plugin cannot supply `generate`: declare it as a `[plugins]` instance in deck.toml
```

A [remote note type](../deck/notetypes.md) (one that janky downloads from a Git repository or an
archive) can bring its own note type plugins. These have the same limits. A remote note type cannot
bring a plugin instance.

## Plugin instances

A plugin instance is a `[plugins.<name>]` table in `deck.toml`. The table connects one plugin script
to your project under a name that you choose. Field templates use this name to call the generator.

The tutorial project has one instance, called `frequency`:

```toml
[plugins.frequency]
path = "plugins/frequency.lua"
[plugins.frequency.variables]
list = "frequency.json"
common_below = 1000
[plugins.frequency.filesystem]
read = { lists = "data" }
```

An instance can have settings (`variables`), network access (`http`), secrets and read access to
directories (`filesystem`). [Plugin instances and secrets](instances.md) explains every key.

You can make more than one instance of the same script, each with different variables. For example,
two instances of a text-to-speech plugin can use two different voices.

## Filter and function names must be unique

All filters and functions share one list of names, for all plugins and for janky itself. If two
plugins give a filter with the same name, janky gives an error ("plugin `x` already supplies `y`").
A plugin also cannot use the name of a janky filter or function, such as `markdown`, `nfc`, `sound`,
`img` or `media`.

So connect a plugin that gives filters only once. If you make two instances of such a plugin, the
second instance gives the same filter names again, and janky refuses it.

## Generators: media and data

A generator is a plugin instance that makes results during `janky generate`. There are two kinds.
The plugin itself says which kind it is.

| Kind | What it makes | Template call | Example |
|---|---|---|---|
| Media generator | A file, for example an audio clip | `media(input, 'name')` or `input \| media('name')` | ElevenLabs text-to-speech |
| Data generator | A set of values, for example a dictionary entry | `enrich(input, 'name')` or `input \| enrich('name')` | `frequency`, PubChem element data |

If you call a generator with the wrong function, the template gives an error before anything
happens. For example, for a media generator called `say`:

```text
invalid operation: `say` generates media files, so call it with `media`
```

[Generating media and data](generating.md) explains how to call generators and how to run them.
