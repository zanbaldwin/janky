# 5. Make changes

Your deck lives in your files, so you make every change in the files. In this step, you make a
typo and read the error, edit a note, and remove a note.

## Make a typo and read the error

1. Open `content/vocab/02-food.yaml`.
2. In the `pan` entry, change `english:` to `englsh:`.
3. Save the file.
4. Run `janky check`:

   ```console
   $ janky check
   content::required

     × missing key `english` in `entries[0]`
      ╭─[content/vocab/02-food.yaml:5:5]
    4 │ entries:
    5 │   - id: 'pan'
      ·     ────┬────
      ·         ╰── `english` is missing
    6 │     spanish: 'pan'
      ╰────
   content::unknown-key

     × unknown key `entries[0].englsh`
      ╭─[content/vocab/02-food.yaml:7:5]
    6 │     spanish: 'pan'
    7 │     englsh: 'bread'
      ·     ───┬──
      ·        ╰── not allowed here
    8 │     gender: 'm'
      ╰────
   ```

One typo gives two errors, because the schema requires `english` and does not allow `englsh`. Each
error is a **diagnostic** with these parts:

| Part | Example | Meaning |
|------|---------|---------|
| Code | `content::unknown-key` | The kind of problem. Use it to look up the problem. |
| Mark | `×` | `×` is an error. `⚠` is a warning. |
| Message | ``unknown key `entries[0].englsh` `` | What is wrong. `entries[0]` is the first entry of the list (janky counts from 0 here). |
| Position | `content/vocab/02-food.yaml:7:5` | The file, the line (7) and the column (5). |
| Snippet | the lines with `───┬──` | The lines around the problem, with the problem underlined and labelled. |

> [!NOTE]
> In a terminal, janky shows diagnostics in colour, with the characters above. If you send the
> output to a file or to another program, janky uses plain characters. Then `x` is an error, `!` is
> a warning, and `^^^` underlines the problem. Other pages of this guide show this plain form (see
> [Reading errors and warnings](../cli/diagnostics.md#two-forms-of-the-same-output)).

While the project has an error, `janky build` stops and writes nothing:

```console
$ janky build
content::required

  × missing key `english` in `entries[0]`
...
```

Fix the typo:

1. Change `englsh:` back to `english:`.
2. Save the file.
3. Run `janky check`:

   ```console
   $ janky check
   9 notes are valid
   ```

[Reading errors and warnings](../cli/diagnostics.md) explains diagnostics, and
[Diagnostic codes](../reference/codes.md) lists the codes.

## Edit a note

1. Open `content/vocab/01-basics.yaml`.
2. Add an example to the `gato` entry, at the end of the file. The entry is now:

   ```yaml
     - id: 'gato'
       spanish: 'gato'
       english: 'cat'
       gender: 'm'
       tags: ['basics', 'animals']
       examples:
         - 'El gato es negro.'
   ```

3. Save the file.
4. Run `janky plan`:

   ```console
   $ janky plan
   Notes: 0 added, 1 changed, 0 removed, 0 moved, 8 unchanged
   ~ note gato in Spanish::1 Vocabulary
   ```

   `~` marks a change. Only `gato` changed.

5. Run `janky build`:

   ```console
   $ janky build
   Notes: 0 added, 1 changed, 0 removed, 0 moved, 8 unchanged
   ~ note gato in Spanish::1 Vocabulary

   wrote .janky/build/spanish.apkg: 9 notes, 1 media file
   In Anki, turn FSRS on. A package cannot change collection settings.
   ```

6. In Anki, import `.janky/build/spanish.apkg` again. Keep **Update notes** at **If newer**.

Anki updates the `gato` note and keeps its review history. janky marks only the changed notes as
newer, so Anki leaves the other eight notes as they are.

> [!NOTE]
> Do not edit notes inside Anki. The next import of a note replaces your Anki edits with the text
> from your files.

## Remove a note

1. Open `content/vocab/02-food.yaml`.
2. Delete the four lines of the `manzana` entry (from `- id: 'manzana'` to `gender: 'f'`).
3. Save the file.
4. Run `janky plan`:

   ```console
   $ janky plan
   Notes: 0 added, 0 changed, 1 removed, 0 moved, 8 unchanged
   - note manzana in Spanish::1 Vocabulary: the note stays in Anki. Delete it there
   ```

`-` marks a removal. An import never deletes notes from Anki. If you build and import now, the
`manzana` note stays in Anki with its cards. To remove it from your collection, delete it in Anki
yourself, in the Anki **Browse** window.

The next step uses `manzana`, so put it back:

1. Undo the deletion in your editor, or type the four lines again after the `pan` entry:

   ```yaml
     - id: 'manzana'
       spanish: 'manzana'
       english: 'apple'
       gender: 'f'
   ```

2. Save the file.
3. Run `janky plan`:

   ```console
   $ janky plan
   Notes: 0 added, 0 changed, 0 removed, 0 moved, 9 unchanged
   No changes.
   ```

You did not build while the entry was gone, so nothing changed.

[Changing content safely](../content/changing.md) explains what each kind of change does in Anki:
changed ids, moved notes, renamed decks and more.

## What you have now

- The same nine notes, and `gato` has an example sentence.
- Anki has the updated `gato` note.
- You can read a diagnostic and find the line that it names.

Next: [6. Add a generator](generator.md).
