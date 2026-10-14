# Presets

In Anki, each deck uses a **preset**: a named group of deck options, such as the number of new
cards each day. In janky, you define presets in `[presets.<key>]` tables, and a deck uses one with
its `preset` key.

```toml
[deck]
name = "Spanish"
preset = "spanish"

[deck.children.vocab]
name = "1 Vocabulary"
preset = "beginner"

[presets.spanish]
name = "Spanish"
new_per_day = 10
reviews_per_day = 100

[presets.beginner]
name = "Spanish beginner"
new_per_day = 15
reviews_per_day = 150
learning_steps = ["1m", "10m", "1h"]
desired_retention = 0.9
maximum_interval = "1y"
```

A deck without `preset` uses Anki's "Default" preset. A `preset` value that is not the key of a
`[presets]` table gives the error `config::unknown-preset`.

janky puts only the presets that a deck names into the package. A preset that no deck names does
not go to Anki.

## Share options with `extends`

If two presets have the same options, put these options in a base preset. Then name the base
preset in `extends`:

```toml
[deck]
name = "Spanish"
preset = "spanish"

[deck.children.vocab]
name = "1 Vocabulary"
preset = "beginner"

[presets.base]
learning_steps = ["1m", "10m"]
desired_retention = 0.92
bury_new = true

[presets.spanish]
extends = "base"
name = "Spanish"
new_per_day = 10

[presets.beginner]
extends = "base"
name = "Spanish beginner"
new_per_day = 15
desired_retention = 0.9
```

A preset gets each option of its base preset that it does not set itself. In this example,
`beginner` gets `learning_steps` and `bury_new` from `base`, and keeps its own
`desired_retention`. No deck names `base`, so janky does not put it into the package.

These rules apply:

- A preset does not get the `name` of its base preset. Without its own `name`, a preset shows its
  key in Anki.
- A base preset can also use `extends`. The nearest preset in the chain wins.
- An `extends` value that is not the key of a `[presets]` table gives the error
  `config::unknown-preset`.
- A chain that comes back to the same preset gives the error `config::preset-loop`.

A change to a base preset is also a change to each preset that extends it. `janky plan` shows each
of these presets as changed.

## Anki imports a preset only once

> [!IMPORTANT]
> Anki adds a preset only on the first import, and only if you tick **Import any deck presets** in
> the import window. After that, a package cannot change the preset. Make all later changes to the
> preset in Anki, in the deck options screen.

`janky plan` reminds you when the package has a new preset:

```text
When you import:
  Tick "Import any deck presets". Anki adds a preset only the first time.
```

If you change a preset in `deck.toml` after the first import, `janky plan` tells you to change it
in Anki with a line in this form:

```text
~ preset <preset name>: a package cannot change a preset that Anki has. Change it in Anki
```

So the preset tables in `deck.toml` are mostly useful for the first import. They are also a record
of the options that you chose.

## FSRS only

janky supports only the FSRS scheduler. It has no options for the older SM-2 scheduler. An SM-2
option, such as `starting_ease`, gives the error `config::unknown-key`.

FSRS is a setting for your whole Anki collection, and a package cannot change it. Each
`janky build` reminds you:

```text
In Anki, turn FSRS on. A package cannot change collection settings.
```

## Preset options

Every option is optional. If you leave an option out, janky sends nothing for it, and Anki uses its
own default. The "Anki default" column gives the default that janky documents. The "In Anki"
column gives the name of the option in Anki's deck options screen.

### Name and daily limits

| Key | Type | Allowed values | Anki default | In Anki |
|-----|------|----------------|--------------|---------|
| `name` | string | any text | the preset key | the preset name |
| `new_per_day` | integer | 0 to 9999 | 20 | New cards/day |
| `reviews_per_day` | integer | 0 to 9999 | 200 | Maximum reviews/day |

### Steps and intervals

| Key | Type | Allowed values | Anki default | In Anki |
|-----|------|----------------|--------------|---------|
| `learning_steps` | list of durations | see [Durations](#durations) | `["1m", "10m"]` | Learning steps |
| `relearning_steps` | list of durations | see [Durations](#durations) | `["10m"]` | Relearning steps |
| `maximum_interval` | string | a whole number and `d`, `w` or `y`, at most 36500 days | `"36500d"` | Maximum interval |

### FSRS

| Key | Type | Allowed values | Anki default | In Anki |
|-----|------|----------------|--------------|---------|
| `desired_retention` | number | 0.7 to 0.99 | 0.9 | Desired retention |
| `fsrs_params` | list of numbers | 17, 19 or 21 numbers | Anki's built-in values | FSRS parameters |

`desired_retention` is the probability of recall that FSRS aims for. Normally, leave
`fsrs_params` out. Anki then uses its built-in values until you run Anki's optimiser. Use 17
numbers for FSRS-4.5, 19 for FSRS-5 and 21 for FSRS-6.

### Display order

| Key | Type | Allowed values | Anki default | In Anki |
|-----|------|----------------|--------------|---------|
| `new_gather_order` | string | see below | `"deck"` | New card gather order |
| `new_sort_order` | string | see below | `"template"` | New card sort order |
| `review_sort_order` | string | see below | `"due"` | Review sort order |
| `new_review_mix` | string | `"mix-with-reviews"`, `"after-reviews"`, `"before-reviews"` | `"mix-with-reviews"` | New/review order |
| `interday_learning_mix` | string | `"mix-with-reviews"`, `"after-reviews"`, `"before-reviews"` | `"mix-with-reviews"` | Interday learning/review order |

`new_review_mix` sets when Anki shows new cards, compared with reviews. `interday_learning_mix`
does the same for learning cards with steps of a day or more.

`new_gather_order` sets how Anki gathers new cards from the deck tree:

| Value | Meaning |
|-------|---------|
| `deck` | deck by deck, in deck order |
| `deck-then-random-notes` | deck by deck, random notes in each deck |
| `ascending-position` | lowest position first (file order) |
| `descending-position` | highest position first |
| `random-notes` | random notes |
| `random-cards` | random cards |

`new_sort_order` sets how Anki sorts the new cards that it gathered:

| Value | Meaning |
|-------|---------|
| `template` | by card type, then gather order |
| `no-sort` | gather order |
| `template-then-random` | by card type, then random |
| `random-note-then-template` | random note, then card type |
| `random-card` | random |

`review_sort_order` sets how Anki sorts review cards:

| Value | Meaning |
|-------|---------|
| `due` | due date, then random |
| `due-then-deck` | due date, then deck |
| `deck-then-due` | deck, then due date |
| `ascending-intervals` | shortest interval first |
| `descending-intervals` | longest interval first |
| `ascending-difficulty` | easy cards first |
| `descending-difficulty` | difficult cards first |
| `ascending-retrievability` | lowest retrievability first |
| `descending-retrievability` | highest retrievability first |
| `relative-overdueness` | most overdue first, relative to interval |
| `random` | random |
| `added` | oldest added first |
| `reverse-added` | newest added first |

The values are case-sensitive. `"Due"` gives an error:

```text
x `presets.bad.review_sort_order`: "Due" is not one of "due", "due-then-deck" or 11 other candidates
```

### Burying

Burying hides the other cards of a note until the next day.

| Key | Type | Allowed values | Anki default | In Anki |
|-----|------|----------------|--------------|---------|
| `bury_new` | boolean | `true`, `false` | `false` | Bury new siblings |
| `bury_reviews` | boolean | `true`, `false` | `false` | Bury review siblings |
| `bury_interday_learning` | boolean | `true`, `false` | `false` | Bury interday learning siblings |

### Leeches

| Key | Type | Allowed values | Anki default | In Anki |
|-----|------|----------------|--------------|---------|
| `leech_threshold` | integer | 1 to 9999 | 8 | Leech threshold |
| `leech_action` | string | `"tag-only"`, `"suspend"` | `"tag-only"` | Leech action |

A leech is a card that you forget many times. `leech_threshold` is the number of lapses after
which a card is a leech. With `"tag-only"`, Anki adds the `leech` tag. With `"suspend"`, Anki
also suspends the card.

### Audio and timer

| Key | Type | Allowed values | Anki default | In Anki |
|-----|------|----------------|--------------|---------|
| `autoplay` | boolean | `true`, `false` | `true` | Don't play audio automatically (the opposite value) |
| `replay_question_audio` | boolean | `true`, `false` | `true` | Skip question when replaying answer (the opposite value) |
| `show_timer` | boolean | `true`, `false` | `false` | Show on-screen timer |
| `max_answer_seconds` | integer | 1 to 9999 | 60 | Maximum answer seconds |

Two of these options have the opposite meaning of the Anki checkbox. `autoplay = false` ticks
"Don't play audio automatically". `replay_question_audio = false` ticks "Skip question when
replaying answer". `max_answer_seconds` is the longest answer time that Anki records.

## Durations

A step in `learning_steps` or `relearning_steps` is a number and a unit, with no space:

| Unit | Meaning | Example |
|------|---------|---------|
| `s` | seconds | `"30s"` |
| `m` | minutes | `"10m"` |
| `h` | hours | `"1.5h"` |
| `d` | days | `"1d"` |

The number can have decimals. There is no unit for weeks. A value such as `"10 min"` or `"2w"`
gives an error:

```text
x `presets.bad.learning_steps[0]`: "10 min" does not match "^[0-9]+(\.[0-9]+)?[smhd]$"
```

`maximum_interval` uses a different form: a whole number from 1, and `d` (days), `w` (weeks) or
`y` (365 days). Examples are `"90d"`, `"12w"` and `"1y"`. Decimals, such as `"1.5y"`, are not
allowed.

## Limits that only plan and build find

`janky check` does not find two mistakes in presets. `janky plan` and `janky build` find them, and
stop with the error `config::preset`:

- a `maximum_interval` longer than 36500 days (for example `"101y"`):

  ```text
  config::preset
    x the maximum interval is longer than Anki's limit of 36500 days
  ```

- an `fsrs_params` list with a length other than 17, 19 or 21:

  ```text
  config::preset
    x FSRS needs 17, 19 or 21 parameters, not 3
  ```

janky checks only the presets that a deck names. A wrong value in a base preset gives the error in
each preset that extends it.
