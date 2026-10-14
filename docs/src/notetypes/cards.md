# Card templates

Each `[cards.<key>]` table in `notetype.toml` makes one Anki card type. A card type has a front
template and a back template. These **card templates** are normal Anki card templates. Anki uses
them each time it shows a card.

## The keys of a card type

| Key | Type | Required | Meaning |
|-----|------|----------|---------|
| `name` | text | yes | The name of the card type in Anki. |
| `front` | path | yes | The file with the front template (the question). |
| `back` | path | yes | The file with the back template (the answer). |
| `when` | text | no | A condition. A note gets this card only when the condition is true. See [Cards that only some notes get](#cards-that-only-some-notes-get). |

The paths are relative to the note type file. This is the recognition card of the tutorial:

```toml
[cards.recognition]
name = "Recognition"
front = "cards/recognition.front.html"
back = "cards/recognition.back.html"
```

## Card templates use Anki's template language

janky does not run card templates. It copies them into the package, and Anki runs them during
review. So a card template uses Anki's template language, not the language of
[field templates](fields.md).

| | Field templates | Card templates |
|-|-----------------|----------------|
| Language | minijinja | Anki's template language |
| Runs | in janky, once for each entry, at `check`, `plan` and `build` | in Anki, each time a card shows |
| Names | entry keys: `{{ entry.english }}` | field display names: `{{English}}` |
| Makes | the text of one field | the HTML of the card |
| Filters | janky filters, minijinja filters, plugin filters | Anki filters, such as `text:`, `tts`, `type:` and `hint:` |
| Errors | janky diagnostics | Anki shows them on the card |

This is the back template of the tutorial's recognition card:

```html
{{FrontSide}}<hr id="answer">
<div class="meaning">{{English}}</div>
{{#Gender}}<div class="gender">{{Gender}}</div>{{/Gender}}
{{#Frequency}}<div class="frequency">{{Frequency}}</div>{{/Frequency}}
{{#Examples}}<div class="examples tap-reveal">{{Examples}}</div>{{/Examples}}
{{#Notes}}<div class="notes">{{Notes}}</div>{{/Notes}}
<script src="reveal.js"></script>
```

The parts of the Anki language that you use most:

| Write | Anki shows |
|-------|------------|
| `{{English}}` | the field with the display name `English` |
| `{{FrontSide}}` | the front of the card (on the back template) |
| `{{#Notes}} ... {{/Notes}}` | the part between the tags, only if the field `Notes` is not empty |
| `{{^Notes}} ... {{/Notes}}` | the part between the tags, only if the field `Notes` is empty |
| `{{text:Spanish}}` | the field without its HTML |

The `<script src="reveal.js">` line is a janky feature. See [Styling and scripts](styling.md).

> [!IMPORTANT]
> A card template names a field by its display name (the `name` of the field), not by its key.
> For the field `[fields.with_article]` with `name = "With Article"`, write `{{With Article}}`.

## How janky checks card templates

janky reads every `{{...}}` tag in the card templates. Each tag must name a field display name of
the note type, or an Anki special field. The special fields are `FrontSide`, `Tags`, `Type`,
`Deck`, `Subdeck`, `Card` and `CardFlag`.

- janky ignores the `#`, `^` and `/` at the start of a tag.
- janky ignores an Anki filter before the field name. `{{text:English}}` and
  `{{tts es_ES:Spanish}}` check `English` and `Spanish`.
- janky skips tags inside an HTML comment (`<!-- ... -->`).

A name that is not a field gives `card::unknown-field`. The help line lists the display names of
the fields:

```console
$ janky check
card::unknown-field

  x the note type has no field `Englsh`
   ,-[notetypes/phrase/back.html:2:22]
 1 | {{FrontSide}}<hr id="answer">
 2 | <div class="meaning">{{Englsh}}</div>
   :                      ^^^^^|^^^^
   :                           `-- unknown field
 3 | {{#Literal}}<div class="gender">literally: {{Literal}}</div>{{/Literal}}
   `----
  help: the fields are: English, Literal, Spanish
...
```

janky gives one error for each tag. So `{{#Englsh}}...{{/Englsh}}` gives two errors.

janky does not check other things in a card template:

- It does not warn when the front of a card is empty.
- It accepts `{{cloze:Spanish}}`, but janky note types are not cloze note types, so this has no
  use.

## Cards that only some notes get

By default, every note gets one card of each card type. Add `when` to a card type to give the card
only to some notes. In the tutorial, only words with `production: true` get a production card:

```toml
[cards.production]
name = "Production"
front = "cards/production.front.html"
back = "cards/production.back.html"
when = "entry.production"
```

The value of `when` is a minijinja **expression**, not a template. Do not put it in `{{ }}`. It can
use the same values as a field template (`entry`, `file`, `deck` and `notetype`).

janky evaluates the expression for each entry. These values count as false: `false`, an empty text,
an empty list, `none` and a missing key. All other values count as true.

| Expression | Entry | Card |
|------------|-------|------|
| `entry.production` | `production: true` | yes |
| `entry.production` | no `production` key | no |
| `entry.num > 2` | `num: 3` | yes |
| `entry.num > 5` | `num: 3` | no |

To make a card that is on unless the entry turns it off, give a default with `or`:

```toml
when = "'production' in (entry.cards or ['production'])"
```

An entry without `cards` gets the card. An entry with `cards: ['listening']` does not.

### The hidden `_make_` field

Anki makes a card only when its front is not empty. janky uses this rule to make `when` work:

1. janky adds a field named `_make_<card key>` after your fields, for example `_make_production`.
2. For each entry, janky sets this field to `y` when the condition is true. Otherwise the field is
   empty.
3. janky puts the front template of the card between `{{#_make_production}}` and
   `{{/_make_production}}`.

So in Anki, the front of the production card looks like this:

```text
{{#_make_production}}<div class="meaning">{{English}}</div>
{{Image}}
{{/_make_production}}
```

You see the `_make_production` field in the Anki editor (collapsed) and in `deck.lock`. Do not
edit it in Anki. Do not use it in your own card templates: `{{#_make_production}}` in a card
template gives `card::unknown-field`.

If you change a `when` condition, the `_make_` field changes for some notes. `janky plan` then shows
these notes as changed, and the import updates them.

### Errors in `when`

| Code | Example message | Cause |
|------|-----------------|-------|
| `template::syntax` | syntax error: unexpected end of input, expected expression | An incomplete expression, such as `entry.num >` |
| `template::unknown-filter` | unknown filter: filter nofilter is unknown | A misspelt filter name |

## Anki always makes the first card

Anki makes at least one card for each note. If the front of every card type is empty for a note,
Anki still makes a card of the **first** card type.

So `when` cannot give a note zero cards. Suppose that all card types of a note type have `when`.
A note with all conditions false still gets a card of the first card type, with an empty front.

> [!TIP]
> Put a card type without `when` first. In the tutorial, `recognition` has no condition and comes
> before `production`.

## Choose display names with care

janky does not check some problems with display names. Anki changes the names when it imports the
package, and the card templates then show the wrong field.

- **Duplicate names.** If two fields have the same display name, Anki adds `+` to the second
  (`Plain` and `Plain+`). This also applies to two card types with the same name.
- **A name that starts with `_make_`.** Suppose that a field has the display name
  `_make_production`, and the card type `production` has `when`. Anki then renames janky's own
  field. The card then depends on your field.
- **The characters `:`, `{`, `}` and `"`.** Anki removes them from field names. The display name
  `Md: {x}` becomes `Md x` in Anki, but janky checks the card templates against `Md: {x}`.

Use short, plain display names: letters, digits and spaces. Give each field and each card type its
own name.
