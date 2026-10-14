# Creating note types

A **note type** tells Anki what a note holds and how its cards look. Anki has its own note types,
for example "Basic". With janky, you write your own note types in text files, and janky puts them
into the package.

## The parts of an Anki note type

An Anki note type has three parts:

- **Fields.** A field is one named piece of text in a note, for example "Spanish" or "English".
  Each note has a value for each field.
- **Card types.** A card type makes one card from a note. Each card type has a front template and
  a back template. A note type with two card types gives two cards for each note, for example a
  Spanish-to-English card and an English-to-Spanish card.
- **Styling.** CSS (the language that sets colours, fonts and sizes on a web page) for all the
  cards of the note type.

In janky, one `notetype.toml` file holds these three parts. It also holds two parts that Anki does
not have:

- The **schema**: the keys that an entry can have.
- The **field templates**: how janky fills each field from an entry.

## Two kinds of template

A janky note type uses two kinds of template. They run at different times, in different programs.

```text
 content file          janky build                 deck.apkg            Anki (review)
┌────────────┐    ┌──────────────────────┐    ┌───────────────┐    ┌──────────────────┐
│ entry      │    │ field templates      │    │ note fields   │    │ card templates   │
│  spanish:  ├───▶│ {{ entry.spanish }}  ├───▶│  Spanish:     ├───▶│ {{Spanish}}      │
│   'casa'   │    │ (once, at build time)│    │   casa        │    │ (at each review) │
└────────────┘    └──────────────────────┘    └───────────────┘    └──────────────────┘
```

- A **field template** runs in janky, once for each entry, when you run `janky check`, `plan` or
  `build`. It reads the keys of the entry (for example `entry.spanish`) and makes the text of one
  Anki field. Field templates use the minijinja template language.
- A **card template** runs in Anki, each time Anki shows a card. It reads the fields of the note by
  their names (for example `{{Spanish}}`) and makes the HTML of the card. Card templates use Anki's
  own template language.

The two languages look alike, because both use `{{ }}`. Keep them apart: a field template names an
entry key, and a card template names a field.

## Pages in this part

- [Note type files](structure.md): the layout of `notetype.toml` and its files, and every top-level
  key.
- [The content schema](schema.md): the keys that an entry can have, and their types.
- [Field templates](fields.md): how janky fills each Anki field from an entry.
- [Card templates](cards.md): the front and back of each card, and cards that only some notes get.
- [Styling and scripts](styling.md): CSS and JavaScript for the cards.
- [Filters, functions and context](template-reference.md): every filter, function and value that a
  field template can use.
