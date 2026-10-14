# Creating content

Your notes live in **content files**. A content file is a YAML, TOML or JSON file with a list of
**entries**. Each entry becomes one Anki note. The entry has an `id`, and it has the keys that its
note type asks for (for example `spanish` and `english`).

You choose the format. All three formats hold the same data, and janky gives the same errors for
each of them. You can use a different format in each file of one project.

## One entry in three formats

This entry is from the Spanish deck of the [tutorial](../tutorial/index.md). It is a phrase with
the key `id`, the entry type `expression`, a tag and two text values.

In YAML:

```yaml
entries:
  - id: 'donde-esta-el-bano'
    type: 'expression'
    spanish: '¿Dónde está el baño?'
    english: 'Where is the bathroom?'
    tags: ['travel']
```

In TOML:

```toml
[[entries]]
id = "donde-esta-el-bano"
type = "expression"
spanish = "¿Dónde está el baño?"
english = "Where is the bathroom?"
tags = ["travel"]
```

In JSON:

```json
{
  "entries": [
    {
      "id": "donde-esta-el-bano",
      "type": "expression",
      "spanish": "¿Dónde está el baño?",
      "english": "Where is the bathroom?",
      "tags": ["travel"]
    }
  ]
}
```

YAML is the shortest to write when you have many entries. TOML is the format of `deck.toml`, so you
already know it. JSON is the strictest, and other programs can write it.
[Content files](files.md) explains each format.

## Pages in this part

- [Content files](files.md): the formats, how janky finds your files, and the YAML traps.
- [Entries: ids, types, tags and defaults](entries.md): the keys that every entry can have.
- [Images and audio](media.md): how to use your own media files on cards.
- [Changing content safely](changing.md): which changes keep your review history in Anki.
