# Using plugins

A plugin is a small script that adds a feature to janky. Plugins are written in Lua, a small
programming language. You do not need to know Lua to use a plugin. You copy the script into your
project and connect it in a configuration file.

A plugin can do two kinds of work:

- It can give your field templates new filters and functions. For example, a filter can put the
  correct article ("el" or "la") before a Spanish word.
- It can be a **generator**. A generator makes card content for you when you run
  `janky generate`. For example, it can make audio with a text-to-speech service, or get data from
  a word list or a dictionary.

## What a plugin can and cannot do

Each plugin runs in a sandbox (a closed space with strict limits). A plugin cannot read your files,
use the network or run other programs. A plugin instance gets access only when you grant it in
`deck.toml`:

- network access only to the host names that you list, and only during `janky generate`
- read access only to the directories that you list
- secrets (for example an API key) only from the sources that you name

A plugin never sees the value of a secret. It can only send the secret to a host that you allowed.
janky also removes secret values from everything that it prints.

## Pages in this chapter

- [Kinds of plugin](kinds.md): note type plugins, plugin instances, and the two kinds of generator.
- [Plugin instances and secrets](instances.md): every key of a `[plugins.<name>]` table, and how to
  set up a secret.
- [Generating media and data](generating.md): call a generator from a field template, run
  `janky generate`, and look after the cache.
- [The bundled plugin](bundled.md): the ElevenLabs text-to-speech plugin in the janky
  repository.

To write your own plugin, see [Creating plugins](../authoring/index.md).
