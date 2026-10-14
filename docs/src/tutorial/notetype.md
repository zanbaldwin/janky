# 2. Make a note type

A note type tells janky which keys an entry can have, how to fill each Anki field, and which cards
to make. In this step, you make two note types: "Spanish Word" for vocabulary and "Spanish Phrase"
for phrases.

## Add the note types to deck.toml

Add these lines to the end of `deck.toml`:

```toml
[notetypes.word]
path = "notetypes/word"
default = true

[notetypes.phrase]
path = "notetypes/phrase.toml"
alias = "expression"
```

| Key | Meaning |
|-----|---------|
| `path` | Where the note type is. A directory means the file `notetype.toml` in that directory. |
| `default = true` | Entries that do not name a note type use this one. |
| `alias` | An extra name for the note type. An entry can use `expression` or `phrase`. |

`word` and `phrase` are the note type **keys**. Entries use the key to choose a note type.

[Adding note types](../deck/notetypes.md) explains these keys.

## The word note type

The word note type is a directory, because it has many files. Make the directories first:

```console
$ mkdir -p notetypes/word/fields notetypes/word/cards notetypes/word/assets
```

### notetype.toml

Make the file `notetypes/word/notetype.toml` with this text:

```toml
name = "Spanish Word"
sort_field = "spanish"
css = "style.css"
assets = ["assets/reveal.js"]
plugins = ["spanish.lua"]

[schema]
spanish = { type = "string", required = true }
english = { type = "string", required = true }
gender = { type = "enum", values = ["m", "f"] }
image = { type = "media" }
examples = { type = "list", of = { type = "string" } }
notes = { type = "markdown" }
production = { type = "bool" }

[fields.spanish]
name = "Spanish"
value = "{{ entry.spanish | nfc }}"

[fields.with_article]
name = "With Article"
value = "{{ entry.spanish | nfc | article(entry.gender) }}"

[fields.english]
name = "English"

[fields.gender]
name = "Gender"
value = "{{ gender_name(entry.gender) }}"

[fields.image]
name = "Image"
value = "{% if entry.image %}{{ media(entry.image) | img }}{% endif %}"

[fields.examples]
name = "Examples"
template = "fields/examples.html.j2"

[fields.notes]
name = "Notes"
value = "{{ entry.notes | markdown }}"

[cards.recognition]
name = "Recognition"
front = "cards/recognition.front.html"
back = "cards/recognition.back.html"

[cards.production]
name = "Production"
front = "cards/production.front.html"
back = "cards/production.back.html"
when = "entry.production"
```

The keys at the top describe the note type:

| Key | Meaning |
|-----|---------|
| `name` | The note type name that Anki shows. |
| `sort_field` | The field that the Anki browser sorts by. |
| `css` | The style sheet of the cards. |
| `assets` | JavaScript files for the cards. |
| `plugins` | Plugins that give extra template helpers (see [The plugin](#the-plugin-spanishlua)). |

All paths in this file are relative to the `notetypes/word/` directory, and must stay inside it.

### The schema: what an entry can have

The `[schema]` table lists the keys that an entry of this note type can have. janky checks every
entry against it, and reports a missing or unknown key as an error.

| Key | Type | Meaning |
|-----|------|---------|
| `spanish` | `string` (text), required | The Spanish word. |
| `english` | `string`, required | The meaning. |
| `gender` | `enum` | One of the listed values: `m` or `f`. |
| `image` | `media` | The path of an image in `media/`. |
| `examples` | `list` of `string` | Example sentences. |
| `notes` | `markdown` | Free notes in Markdown (see below). |
| `production` | `bool` | `true` or `false`: make a production card or not. |

[The content schema](../notetypes/schema.md) lists every type.

### The fields: field templates

Each `[fields.<key>]` table is one Anki field. The order of the tables is the order of the fields in
Anki. `name` is the field name that Anki shows.

`value` is a field template. janky runs it once for each entry when you build. The template
language is Jinja (janky uses a version called minijinja):

- `{{ ... }}` prints a value. `entry.spanish` is the `spanish` key of the entry.
- `|` sends a value through a **filter**. `nfc` stores accented letters in one standard form.
  `markdown` turns Markdown text into HTML.
- `{% if ... %} ... {% endif %}` prints the text between them only if the condition is true.
- `media(entry.image)` finds the image in `media/` and adds it to the package. The `img` filter
  makes the HTML that shows it.

A field with no `value` (here, `english`) gets the entry key with the same name. A field with
`template` reads its field template from a file.

[Field templates](../notetypes/fields.md) explains field templates in full.

### The examples field template

The examples need a loop, so they have their own file. Make the file
`notetypes/word/fields/examples.html.j2` with this text:

```jinja
{% for example in entry.examples %}<p class="example">{{ example | nfc }}</p>{% endfor %}
```

`{% for %}` repeats the text for each item in the list. `<p class="example">` is HTML, the language
of web pages and of Anki cards. A `<p>` element is one paragraph.

### The cards: card templates

Each `[cards.<key>]` table is one Anki card type, with a front and a back card template. Anki uses
card templates each time it shows a card. They use Anki's own syntax, not Jinja:

- `{{Field Name}}` shows a field. Use the field `name`, not the key.
- `{{#Field Name}} ... {{/Field Name}}` shows the text between them only if the field is not
  empty.
- `{{FrontSide}}` on the back shows the front again.

The `production` card has `when = "entry.production"`. janky makes this card only for entries
that have `production: true`. To do this, janky adds a hidden field `_make_production` to the note
type. Anki shows it after your own fields, collapsed in the note editor. Do not edit it.

Make the file `notetypes/word/cards/recognition.front.html`:

```html
<div class="word">{{With Article}}</div>
{{Image}}
```

Make the file `notetypes/word/cards/recognition.back.html`:

```html
{{FrontSide}}<hr id="answer">
<div class="meaning">{{English}}</div>
{{#Gender}}<div class="gender">{{Gender}}</div>{{/Gender}}
{{#Examples}}<div class="examples tap-reveal">{{Examples}}</div>{{/Examples}}
{{#Notes}}<div class="notes">{{Notes}}</div>{{/Notes}}
<script src="reveal.js"></script>
```

Make the file `notetypes/word/cards/production.front.html`:

```html
<div class="meaning">{{English}}</div>
{{Image}}
```

Make the file `notetypes/word/cards/production.back.html`:

```html
{{FrontSide}}<hr id="answer">
<div class="word">{{With Article}}</div>
{{#Examples}}<div class="examples tap-reveal">{{Examples}}</div>{{/Examples}}
<script src="reveal.js"></script>
```

[Card templates](../notetypes/cards.md) explains card templates and `when`.

### The style and the script

CSS sets the look of the cards (colours, fonts, sizes). Make the file `notetypes/word/style.css`:

```css
.card {
  font-family: system-ui, sans-serif;
  font-size: 22px;
  text-align: center;
}
.word { font-size: 36px; }
.article-m { color: #1f5fbf; }
.article-f { color: #bf1f5f; }
.gender, .frequency { color: #777; font-size: 16px; }
.examples { font-style: italic; }
.tap-reveal { filter: blur(6px); cursor: pointer; }
.tap-reveal.shown { filter: none; }
.notes { font-size: 16px; text-align: left; }
```

The examples are blurred until you tap them. A short JavaScript file removes the blur. Make the file
`notetypes/word/assets/reveal.js`:

```js
document.querySelectorAll(".tap-reveal").forEach((element) => {
  element.addEventListener("click", () => element.classList.add("shown"));
});
```

The back templates contain `<script src="reveal.js"></script>`. When you build, janky puts the text
of `reveal.js` in its place, so the script works in Anki without a separate file.

[Styling and scripts](../notetypes/styling.md) explains CSS and assets.

### The plugin: spanish.lua

The field templates use two helpers that janky does not have: the `article` filter and the
`gender_name` function. A note type plugin gives them. A plugin is a short script in the Lua
language.

Make the file `notetypes/word/spanish.lua` with this text:

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

- `article` puts "el" or "la" in front of the word, in a coloured `<span>`. For `casa`, the "With
  Article" field becomes `<span class="article-f">la</span> casa`.
- `gender_name` turns `m` into "masculine" and `f` into "feminine".

You do not need to understand the Lua code to follow the tutorial. [Kinds of
plugin](../plugins/kinds.md) explains what a plugin can do, and [Your first plugin: a
filter](../authoring/first-plugin.md) explains how to write one.

## The phrase note type

A small note type can be a single file. Make the file `notetypes/phrase.toml` with this text:

```toml
name = "Spanish Phrase"
css = "word/style.css"

[schema]
spanish = { type = "string", required = true }
english = { type = "string", required = true }
literal = { type = "string" }

[fields.spanish]
name = "Spanish"

[fields.english]
name = "English"

[fields.literal]
name = "Literal"

[cards.meaning]
name = "Meaning"
front = "phrase/front.html"
back = "phrase/back.html"
```

The paths of a single-file note type are relative to its directory (`notetypes/`). So
`word/style.css` uses the same style sheet as the word note type.

Make the directory `notetypes/phrase/`:

```console
$ mkdir notetypes/phrase
```

Make the file `notetypes/phrase/front.html`:

```html
<div class="word">{{Spanish}}</div>
```

Make the file `notetypes/phrase/back.html`:

```html
{{FrontSide}}<hr id="answer">
<div class="meaning">{{English}}</div>
{{#Literal}}<div class="gender">literally: {{Literal}}</div>{{/Literal}}
```

[Note type files](../notetypes/structure.md) explains the two forms of a note type.

## Check the project

1. Run `janky check`:

   ```console
   $ janky check
   0 notes are valid
   ```

   There are no errors. There are no notes yet, because the content directories are empty.

2. Run `janky plan` to see what a build would make:

   ```console
   $ janky plan
   Notes: 0 added, 0 changed, 0 removed, 0 moved, 0 unchanged
   + deck Spanish
   + deck Spanish::1 Vocabulary
   + deck Spanish::2 Phrases
   + preset Spanish
   + preset Spanish beginner
   + note type Spanish Word
   + note type Spanish Phrase
   When you import:
     Tick "Import any deck presets". Anki adds a preset only the first time.
   ```

   Each `+` line is something that the build adds.

If `janky check` shows an error, compare the file that it names with the text on this page.

## What you have now

```text
spanish/
  .gitignore
  deck.toml
  content/
    phrases/
    vocab/
  media/
    images/
  notetypes/
    phrase.toml
    phrase/
      back.html
      front.html
    word/
      notetype.toml
      spanish.lua
      style.css
      assets/
        reveal.js
      cards/
        production.back.html
        production.front.html
        recognition.back.html
        recognition.front.html
      fields/
        examples.html.j2
```

Next: [3. Add content](content.md).
