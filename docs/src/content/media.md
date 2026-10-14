# Images and audio

Cards can show images and play audio. You keep these media files in your project, and janky puts
the files that your notes use into the package. This page explains how to use your own media
files. Plugins can also make media for you (see the last section).

## Use a media file in an entry

To use a media file, three parts work together:

1. The note type's schema has a key of type `media` (see [The content schema](../notetypes/schema.md)).
2. The entry gives the path of the file in that key.
3. A field template finds the file with `media()` and makes the HTML for the card.

In the tutorial, the word note type has an `image` key:

```toml
[schema]
image = { type = "media" }

[fields.image]
name = "Image"
value = "{% if entry.image %}{{ media(entry.image) | img }}{% endif %}"
```

The entry `casa` gives the path of its image:

```yaml
  - id: 'casa'
    spanish: 'casa'
    english: 'house'
    image: 'images/casa.png'
```

janky finds the file `media/images/casa.png` and puts it into the package. The Image field of the
note gets this value:

```html
<img src="casa.png">
```

For audio, use the `sound` filter instead of `img`. It makes the value `[sound:<file name>]`,
which Anki plays. [Filters, functions and context](../notetypes/template-reference.md) lists
`media()`, `img` and `sound`.

## Where janky looks for media files

janky looks for media files in the directories of `media_dirs` in `deck.toml`. The default is one
directory, `media`:

```toml
[deck]
name = "Spanish"
media_dirs = ["media"]
```

If you give more than one directory, janky looks in them in the order of the list. If a directory
in the list does not exist, you get the warning `config::missing-media-dir`.

A path in an entry is relative to a media directory. It is not relative to the project or to the
content file. So:

- `images/casa.png` finds `media/images/casa.png`.
- `./images/casa.png` finds the same file.
- `media/images/casa.png` does not work. janky looks for this path inside `media/` and gives
  `media::missing`.

A path cannot point outside the media directories. A path such as `../deck.toml`, an absolute
path, or a symbolic link to a file outside gives `media::outside`.

## Each file needs its own file name

Anki keeps all media files in one folder, without subdirectories. So janky gives each file only its
file name in Anki: `images/casa.png` becomes `casa.png`.

Two different files with the same file name cannot both go into Anki. For example,
`media/animals/gato.png` and `media/toys/gato.png` give `media::name-collision`. This is the
message for two files that are both named `p.png`:

```text
`media/x/p.png` already has the Anki file name `p.png`
help: Anki keeps media in one folder, so each file needs its own file name
```

Rename one of the files. If many entries use the same file, janky puts it into the package once.

## When janky finds a missing file

janky looks for a file only when a field template calls `media()` with the path. Then a missing
file gives `media::missing`, with the list of directories that janky searched.

> [!WARNING]
> janky does not check a `media` key that no field template uses. If you misspell the path in such
> a key, janky gives no error.

If a default path in `defaults` is missing, janky shows the error once, at `defaults`.

## Changes to media files

`janky plan` shows the media changes of the next build:

| Change | `janky plan` shows |
|--------|--------------------|
| A note uses a new file | `+ media casa.png` |
| The content of a file changes | `~ media casa.png` |
| No note uses the file any more | `- media casa.png: the file stays in Anki. Tools, Check Media removes unused files` |

An import never deletes media files from Anki. To remove files that no card uses, use
**Tools → Check Media** in Anki.

## Generated media

A generator plugin can make media for you, for example audio from a text-to-speech service.
janky keeps these files in `.janky/cache/`, not in your media directories. You do not write a path
for them in your entries. A field template asks the generator for the file.
[Generating media and data](../plugins/generating.md) explains how this works.
